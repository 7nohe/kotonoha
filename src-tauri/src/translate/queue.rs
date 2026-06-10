use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::events::{emit_pipeline_error, TranslationEvent, EV_TRANSLATION};
use crate::state::AppState;

/// Tokens are coalesced and flushed to the UI at most this often
const FLUSH_INTERVAL: Duration = Duration::from_millis(80);

pub struct TranslationRequest {
    pub utterance_id: String,
    pub text: String,
}

#[derive(Clone)]
pub struct TranslationQueue {
    tx: mpsc::Sender<TranslationRequest>,
}

impl TranslationQueue {
    /// Called from the STT thread. Never blocks, even if the queue is congested.
    pub fn submit(&self, req: TranslationRequest) {
        if self.tx.try_send(req).is_err() {
            eprintln!("[translate] queue full, dropping request");
        }
    }
}

/// Spawns the Ollama translation worker (on tauri's tokio runtime).
pub fn spawn(app: AppHandle) -> TranslationQueue {
    let (tx, mut rx) = mpsc::channel::<TranslationRequest>(16);

    tauri::async_runtime::spawn(async move {
        let mut warned = false;

        while let Some(req) = rx.recv().await {
            let model = {
                let state = app.state::<AppState>();
                let config = state.config.lock().unwrap();
                config.ollama_model.clone()
            };
            // No model configured: use the first one Ollama has installed
            let model = match model {
                Some(m) => m,
                None => {
                    let first = super::ollama::list_models()
                        .await
                        .ok()
                        .and_then(|models| models.into_iter().next());
                    match first {
                        Some(m) => {
                            let state = app.state::<AppState>();
                            state.config.lock().unwrap().ollama_model = Some(m.clone());
                            m
                        }
                        None => {
                            if !warned {
                                emit_pipeline_error(
                                    &app,
                                    "Ollama が起動していないかモデルがありません。翻訳をスキップします。",
                                );
                                warned = true;
                            }
                            continue;
                        }
                    }
                }
            };

            // Coalesce streamed tokens: emitting one IPC event per token floods
            // the webview with re-renders, so flush at most every FLUSH_INTERVAL
            let app_emit = app.clone();
            let utterance_id = req.utterance_id.clone();
            let mut full_text = String::new();
            let mut pending = String::new();
            let mut last_flush = Instant::now();
            let result = super::ollama::translate_stream(&model, &req.text, |delta, done| {
                full_text.push_str(&delta);
                pending.push_str(&delta);
                if done || last_flush.elapsed() >= FLUSH_INTERVAL {
                    let _ = app_emit.emit(
                        EV_TRANSLATION,
                        TranslationEvent {
                            utterance_id: utterance_id.clone(),
                            delta: std::mem::take(&mut pending),
                            done,
                        },
                    );
                    last_flush = Instant::now();
                }
            })
            .await;

            match result {
                Ok(()) => {
                    warned = false;
                    if !full_text.is_empty() {
                        app.state::<AppState>()
                            .history
                            .set_translation(&req.utterance_id, full_text);
                    }
                }
                Err(e) => {
                    // Notify connection problems only once; transcription keeps running
                    if !warned {
                        emit_pipeline_error(&app, e);
                        warned = true;
                    }
                }
            }
        }
    });

    TranslationQueue { tx }
}
