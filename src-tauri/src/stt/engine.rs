use std::collections::HashMap;

use crossbeam_channel::{Receiver, Sender};
use tauri::{AppHandle, Emitter, Manager};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::TARGET_SAMPLE_RATE;
use crate::events::{emit_pipeline_error, SourceKind, TranscriptEvent, EV_TRANSCRIPT};
use crate::translate::queue::{TranslationQueue, TranslationRequest};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Partial,
    Final,
}

pub struct TranscribeJob {
    pub source: SourceKind,
    pub utterance_id: String,
    pub kind: JobKind,
    pub audio: Vec<f32>,
    /// Language passed to whisper ("ja" / "en")
    pub language: &'static str,
    /// Whether the finalized utterance should be sent to the translation queue
    pub translate: bool,
}

#[derive(Clone)]
pub struct SttEngine {
    pub job_tx: Sender<TranscribeJob>,
}

/// Spawns the single inference thread that owns the whisper context (Metal).
/// Serializes jobs from both mic and system sources to avoid GPU contention.
/// The returned JoinHandle must be joined before process exit: tearing the
/// process down while ggml Metal resources are alive aborts in static destructors.
pub fn spawn(
    app: AppHandle,
    model_path: String,
    translation: TranslationQueue,
) -> (SttEngine, std::thread::JoinHandle<()>) {
    let (job_tx, job_rx) = crossbeam_channel::unbounded::<TranscribeJob>();

    let handle = std::thread::Builder::new()
        .name("whisper".into())
        .spawn(move || run(app, model_path, job_rx, translation))
        .expect("failed to spawn whisper thread");

    (SttEngine { job_tx }, handle)
}

fn run(
    app: AppHandle,
    model_path: String,
    job_rx: Receiver<TranscribeJob>,
    translation: TranslationQueue,
) {
    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.flash_attn(true);
    let ctx = match WhisperContext::new_with_params(&model_path, ctx_params) {
        Ok(ctx) => ctx,
        Err(e) => {
            emit_pipeline_error(&app, format!("Whisper モデルの読み込みに失敗: {e}"));
            return;
        }
    };
    let mut state = match ctx.create_state() {
        Ok(s) => s,
        Err(e) => {
            emit_pipeline_error(&app, format!("Whisper state の作成に失敗: {e}"));
            return;
        }
    };

    let n_threads = (std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        / 2)
    .clamp(2, 8) as i32;

    while let Ok(job) = job_rx.recv() {
        // Drain queued jobs in one batch and drop stale partials
        let mut pending = vec![job];
        while let Ok(next) = job_rx.try_recv() {
            pending.push(next);
        }
        let jobs = coalesce(pending);

        for job in jobs {
            let mut audio = job.audio;
            // whisper.cpp gets unstable with inputs under ~1s, so zero-pad
            let min_len = TARGET_SAMPLE_RATE as usize * 11 / 10;
            if audio.len() < min_len {
                audio.resize(min_len, 0.0);
            }

            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_language(Some(job.language));
            params.set_n_threads(n_threads);
            // No temperature fallback: on noisy input it retries the decode up to
            // 6 times at rising temperatures, blowing realtime latency
            params.set_temperature_inc(0.0);
            params.set_no_context(true);
            params.set_suppress_blank(true);
            params.set_suppress_nst(true);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            if job.kind == JobKind::Partial {
                params.set_single_segment(true);
            }

            // whisper always encodes a full 30s window (1500 frames, 50/s) no matter
            // how short the input is; shrinking the encoder context to the actual
            // audio length is the dominant speedup for short realtime segments.
            let audio_ctx = (audio.len() / (TARGET_SAMPLE_RATE as usize / 50) + 64).min(1500);
            params.set_audio_ctx(audio_ctx as i32);

            let started = std::time::Instant::now();
            if let Err(e) = state.full(params, &audio) {
                eprintln!("whisper inference failed: {e}");
                continue;
            }
            let elapsed_ms = started.elapsed().as_millis();

            let mut text = String::new();
            for segment in state.as_iter() {
                // Filter out hallucinations caused by silence or noise
                if segment.no_speech_probability() > 0.6 {
                    continue;
                }
                if let Ok(s) = segment.to_str() {
                    text.push_str(s);
                }
            }
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            if is_repetitive(&text) {
                eprintln!("[stt] dropped repetitive hallucination: {text}");
                continue;
            }

            let is_final = job.kind == JobKind::Final;
            eprintln!(
                "[stt] {:?} {} ({:.1}s audio, {elapsed_ms}ms): {}",
                job.source,
                if is_final { "final" } else { "partial" },
                audio.len() as f32 / TARGET_SAMPLE_RATE as f32,
                text
            );

            let _ = app.emit(
                EV_TRANSCRIPT,
                TranscriptEvent {
                    utterance_id: job.utterance_id.clone(),
                    source: job.source,
                    text: text.clone(),
                    is_final,
                },
            );

            if is_final {
                let state = app.state::<crate::state::AppState>();
                state
                    .history
                    .push_final(job.utterance_id.clone(), job.source, text.clone());

                if job.translate {
                    translation.submit(TranslationRequest {
                        utterance_id: job.utterance_id,
                        text,
                    });
                }
            }
        }
    }
}

/// Detects the degenerate repetition loops whisper produces on noise/silence:
/// when a short phrase repeats, the ratio of distinct character trigrams
/// collapses, while natural speech (Japanese or English) stays well above 0.5.
fn is_repetitive(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 24 {
        return false;
    }
    let distinct: std::collections::HashSet<&[char]> = chars.windows(3).collect();
    (distinct.len() as f32) < (chars.len() - 2) as f32 * 0.45
}

/// Keeps only the latest partial per utterance; finals are always kept and
/// processed first to minimize the latency of finalized captions.
/// (Utterance ids are globally unique and each source sends its jobs in order,
/// so a partial can never arrive after its own final within a batch.)
fn coalesce(jobs: Vec<TranscribeJob>) -> Vec<TranscribeJob> {
    let mut finals = Vec::new();
    let mut latest_partial: HashMap<String, TranscribeJob> = HashMap::new();

    for job in jobs {
        match job.kind {
            JobKind::Final => {
                latest_partial.remove(&job.utterance_id);
                finals.push(job);
            }
            JobKind::Partial => {
                latest_partial.insert(job.utterance_id.clone(), job);
            }
        }
    }

    finals.extend(latest_partial.into_values());
    finals
}

#[cfg(test)]
mod tests {
    use super::is_repetitive;

    #[test]
    fn repetition_loops_are_dropped() {
        // Real hallucinations observed on ambient-noise input
        assert!(is_repetitive(
            "I'm just a lot. I'm just I'm just a lot. I'm just a lot. I'm just a lot."
        ));
        assert!(is_repetitive(
            "I just, I just I just I just... I just... I just... I just..."
        ));
        assert!(is_repetitive("そうですねそうですねそうですねそうですねそうですね"));
    }

    #[test]
    fn natural_speech_is_kept() {
        assert!(!is_repetitive("Let's review the quarterly roadmap. The activation rate improved."));
        assert!(!is_repetitive("来週の定例までにオンボーディングの改善案をまとめておきます。"));
        assert!(!is_repetitive("はい、はい。そうですね。")); // short → skipped
    }
}
