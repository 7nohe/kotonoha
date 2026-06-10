use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::events::{PipelineErrorEvent, TranslationEvent};
use crate::state::AppState;

pub struct TranslationRequest {
    pub utterance_id: String,
    pub text: String,
}

#[derive(Clone)]
pub struct TranslationQueue {
    tx: mpsc::Sender<TranslationRequest>,
}

impl TranslationQueue {
    /// Called from the STT thread. Never blocks, even if the queue is full.
    pub fn submit(&self, req: TranslationRequest) {
        if self.tx.try_send(req).is_err() {
            eprintln!("[translate] queue full, dropping request");
        }
    }
}

/// Spawns the translation worker for Ollama (on tauri's tokio runtime).
pub fn spawn(app: AppHandle) -> TranslationQueue {
    let (tx, mut rx) = mpsc::channel::<TranslationRequest>(16);

    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::new();
        let mut warned = false;

        while let Some(req) = rx.recv().await {
            let model = {
                let state = app.state::<AppState>();
                let config = state.config.lock().unwrap();
                config.ollama_model.clone()
            };
            // If no model is configured, use the first model available in Ollama
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
                                let _ = app.emit(
                                    "pipeline-error",
                                    PipelineErrorEvent {
                                        message: "Ollama が起動していないかモデルがありません。翻訳をスキップします。".into(),
                                    },
                                );
                                warned = true;
                            }
                            continue;
                        }
                    }
                }
            };

            let app_emit = app.clone();
            let utterance_id = req.utterance_id.clone();
            let mut full_text = String::new();
            let result = super::ollama::translate_stream(&client, &model, &req.text, |delta, done| {
                full_text.push_str(&delta);
                let _ = app_emit.emit(
                    "translation",
                    TranslationEvent {
                        utterance_id: utterance_id.clone(),
                        delta,
                        done,
                    },
                );
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
                    // Report connection loss only once; do not stop transcription
                    if !warned {
                        let _ = app.emit("pipeline-error", PipelineErrorEvent { message: e });
                        warned = true;
                    }
                }
            }
        }
    });

    TranslationQueue { tx }
}
