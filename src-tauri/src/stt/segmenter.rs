use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use crate::audio::TARGET_SAMPLE_RATE;

const SR: usize = TARGET_SAMPLE_RATE as usize;

/// How often to run VAD
const VAD_INTERVAL: usize = SR * 3 / 10; // 300ms
/// Tail window fed to VAD
const TAIL_WINDOW: usize = SR * 12 / 10; // 1.2s
/// Treat the utterance as ended if the tail is silent for this long
const SILENCE_END: usize = SR * 6 / 10; // 600ms
/// How often to emit partials while speech continues
const PARTIAL_EVERY: usize = SR * 3 / 2; // 1.5s
/// Force-split length for an utterance
const MAX_UTTERANCE: usize = SR * 12; // 12s
/// Pre-roll kept while not in speech
const PRE_ROLL: usize = SR; // 1s

pub enum SegmenterOutput {
    Partial(Vec<f32>),
    Final(Vec<f32>),
}

/// whisper.cpp's VAD context accumulates internal state across calls, making
/// processing time grow linearly (2ms → several seconds), so recreate it periodically.
const VAD_RECREATE_EVERY: usize = 50;

/// State machine that takes 16kHz mono f32 audio and extracts speech segments with Silero VAD.
pub struct Segmenter {
    vad: WhisperVadContext,
    vad_model_path: String,
    vad_calls: usize,
    buffer: Vec<f32>,
    in_speech: bool,
    samples_since_vad: usize,
    last_partial_at: usize,
}

impl Segmenter {
    pub fn new(vad_model_path: &str) -> Result<Self, String> {
        Ok(Self {
            vad: create_vad(vad_model_path)?,
            vad_model_path: vad_model_path.to_string(),
            vad_calls: 0,
            buffer: Vec::with_capacity(MAX_UTTERANCE + SR),
            in_speech: false,
            samples_since_vad: 0,
            last_partial_at: 0,
        })
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<SegmenterOutput> {
        self.buffer.extend_from_slice(samples);
        self.samples_since_vad += samples.len();

        let mut outputs = Vec::new();
        if self.samples_since_vad < VAD_INTERVAL {
            return outputs;
        }
        self.samples_since_vad = 0;

        self.vad_calls += 1;
        if self.vad_calls >= VAD_RECREATE_EVERY {
            if let Ok(fresh) = create_vad(&self.vad_model_path) {
                self.vad = fresh;
                self.vad_calls = 0;
            }
        }

        let window_len = self.buffer.len().min(TAIL_WINDOW);
        let window_start = self.buffer.len() - window_len;
        let (speech_any, speech_recent) = {
            let (buffer, vad) = (&self.buffer, &mut self.vad);
            detect(vad, &buffer[window_start..])
        };

        if !self.in_speech {
            if speech_any {
                self.in_speech = true;
                self.last_partial_at = 0;
            } else if self.buffer.len() > PRE_ROLL {
                // Do not accumulate silence; keep only the pre-roll
                let excess = self.buffer.len() - PRE_ROLL;
                self.buffer.drain(..excess);
            }
            return outputs;
        }

        let ended = !speech_recent;
        let too_long = self.buffer.len() >= MAX_UTTERANCE;

        if ended || too_long {
            let utterance = std::mem::take(&mut self.buffer);
            self.in_speech = false;
            self.last_partial_at = 0;
            outputs.push(SegmenterOutput::Final(utterance));
        } else if self.buffer.len() - self.last_partial_at >= PARTIAL_EVERY {
            self.last_partial_at = self.buffer.len();
            outputs.push(SegmenterOutput::Partial(self.buffer.clone()));
        }
        outputs
    }

}

fn create_vad(model_path: &str) -> Result<WhisperVadContext, String> {
    let mut ctx_params = WhisperVadContextParams::new();
    ctx_params.set_n_threads(2);
    WhisperVadContext::new(model_path, ctx_params)
        .map_err(|e| format!("VAD モデルの読み込みに失敗 ({model_path}): {e}"))
}

/// Returns (any speech in the window, speech within the trailing SILENCE_END)
fn detect(vad: &mut WhisperVadContext, window: &[f32]) -> (bool, bool) {
    let mut params = WhisperVadParams::new();
    params.set_min_silence_duration(100);
    params.set_min_speech_duration(150);

    let Ok(segments) = vad.segments_from_samples(params, window) else {
        // On VAD failure, treat as speech and let whisper decide
        return (true, true);
    };

    let window_cs = (window.len() * 100 / SR) as f32;
    let silence_threshold_cs = window_cs - (SILENCE_END * 100 / SR) as f32;

    let mut any = false;
    let mut recent = false;
    for seg in segments {
        any = true;
        if seg.end >= silence_threshold_cs {
            recent = true;
        }
    }
    (any, recent)
}
