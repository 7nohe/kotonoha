use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

// Event names shared with the frontend (src/lib/ipc.ts)
pub const EV_TRANSCRIPT: &str = "transcript";
pub const EV_TRANSLATION: &str = "translation";
pub const EV_CAPTURE_STATE: &str = "capture-state";
pub const EV_PIPELINE_ERROR: &str = "pipeline-error";
pub const EV_DOWNLOAD_PROGRESS: &str = "model-download-progress";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Mic,
    System,
}

impl SourceKind {
    /// Prefix used in utterance ids ("mic-3", "sys-7")
    pub fn id_prefix(self) -> &'static str {
        match self {
            SourceKind::Mic => "mic",
            SourceKind::System => "sys",
        }
    }

    /// Speaker label used in exported transcripts
    pub fn speaker_ja(self) -> &'static str {
        match self {
            SourceKind::Mic => "自分",
            SourceKind::System => "相手",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    pub utterance_id: String,
    pub source: SourceKind,
    pub text: String,
    pub is_final: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationEvent {
    pub utterance_id: String,
    pub delta: String,
    pub done: bool,
}

/// Reports a non-fatal error to the overlay (payload is the bare message string)
pub fn emit_pipeline_error(app: &AppHandle, message: impl Into<String>) {
    let _ = app.emit(EV_PIPELINE_ERROR, message.into());
}
