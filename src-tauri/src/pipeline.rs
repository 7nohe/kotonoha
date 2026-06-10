use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use tauri::AppHandle;

use crate::audio::{downmix_to_mono, mic, resample::MonoResampler, system, system_catap};
use crate::events::{emit_pipeline_error, SourceKind};
use crate::stt::engine::{JobKind, SttEngine, TranscribeJob};
use crate::stt::segmenter::{Segmenter, SegmenterOutput};

/// Ring buffer capacity (about 5 seconds of 48kHz stereo)
const RING_CAPACITY: usize = 48_000 * 2 * 5;

static UTTERANCE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_utterance_id(source: SourceKind) -> String {
    let n = UTTERANCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{n}", source.id_prefix())
}

pub struct PipelineHandle {
    stop_flag: Arc<AtomicBool>,
    capture_stop_tx: mpsc::Sender<()>,
}

impl PipelineHandle {
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        let _ = self.capture_stop_tx.send(());
    }
}

impl Drop for PipelineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
pub struct PipelineParams {
    pub vad_model_path: String,
    /// Language passed to whisper ("ja" / "en")
    pub language: &'static str,
    /// Whether finalized utterances should be sent to the translation queue
    pub translate: bool,
}

/// Microphone pipeline:
/// cpal RT callback → rtrb → worker (downmix → 16kHz resample → VAD segmenter) → whisper job
pub fn start_mic(
    app: AppHandle,
    engine: SttEngine,
    params: PipelineParams,
) -> Result<PipelineHandle, String> {
    start_with(app, engine, params, SourceKind::Mic, |producer, stop_rx| {
        mic::start(producer, stop_rx).map(|info| (info.sample_rate, info.channels))
    })
}

/// System audio pipeline.
/// Tries a Core Audio process tap first (macOS 14.4+, no screen recording permission needed),
/// then falls back to ScreenCaptureKit if unavailable.
pub fn start_system(
    app: AppHandle,
    engine: SttEngine,
    params: PipelineParams,
) -> Result<PipelineHandle, String> {
    let catap = start_with(
        app.clone(),
        engine.clone(),
        params.clone(),
        SourceKind::System,
        |producer, stop_rx| {
            let info = system_catap::start(producer, stop_rx)?;
            eprintln!("[audio] system backend: Core Audio tap ({}Hz)", info.sample_rate);
            Ok((info.sample_rate, info.channels))
        },
    );
    match catap {
        Ok(handle) => Ok(handle),
        Err(catap_err) => {
            eprintln!("[audio] CATap unavailable ({catap_err}), falling back to ScreenCaptureKit");
            start_with(app, engine, params, SourceKind::System, |producer, stop_rx| {
                system::start(producer, stop_rx).map(|info| (info.sample_rate, info.channels))
            })
        }
    }
}

/// Wires one capture backend into a worker thread:
/// backend writes f32 samples into the ring buffer; the worker downmixes,
/// resamples to 16kHz, segments by VAD, and enqueues whisper jobs.
fn start_with(
    app: AppHandle,
    engine: SttEngine,
    params: PipelineParams,
    source: SourceKind,
    backend: impl FnOnce(rtrb::Producer<f32>, mpsc::Receiver<()>) -> Result<(u32, usize), String>,
) -> Result<PipelineHandle, String> {
    let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
    let (capture_stop_tx, capture_stop_rx) = mpsc::channel::<()>();

    let (sample_rate, channels) = backend(producer, capture_stop_rx)?;

    let mut resampler = MonoResampler::new(sample_rate)?;
    let mut segmenter = Segmenter::new(&params.vad_model_path)?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();
    let thread_name = format!("{}-pipeline", source.id_prefix());
    let language = params.language;
    let translate = params.translate;

    std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let mut raw = Vec::with_capacity(8192);
            let mut current_id: Option<String> = None;

            while !stop.load(Ordering::Relaxed) {
                raw.clear();
                while let Ok(sample) = consumer.pop() {
                    raw.push(sample);
                    if raw.len() >= 8192 {
                        break;
                    }
                }
                if raw.is_empty() {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }

                let downmixed;
                let mono: &[f32] = if channels > 1 {
                    downmixed = downmix_to_mono(&raw, channels);
                    &downmixed
                } else {
                    &raw
                };
                let resampled = match resampler.process(mono) {
                    Ok(r) => r,
                    Err(e) => {
                        emit_pipeline_error(&app, e);
                        break;
                    }
                };
                if resampled.is_empty() {
                    continue;
                }

                for output in segmenter.push(&resampled) {
                    let (kind, audio) = match output {
                        SegmenterOutput::Partial(a) => (JobKind::Partial, a),
                        SegmenterOutput::Final(a) => (JobKind::Final, a),
                    };
                    // A backlogged engine drops stale partials anyway — don't even queue them
                    if kind == JobKind::Partial && !engine.job_tx.is_empty() {
                        continue;
                    }
                    let id = current_id
                        .get_or_insert_with(|| next_utterance_id(source))
                        .clone();
                    if kind == JobKind::Final {
                        current_id = None;
                    }
                    let _ = engine.job_tx.send(TranscribeJob {
                        source,
                        utterance_id: id,
                        kind,
                        audio,
                        language,
                        translate,
                    });
                }
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(PipelineHandle {
        stop_flag,
        capture_stop_tx,
    })
}
