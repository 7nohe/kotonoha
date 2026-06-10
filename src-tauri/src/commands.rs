use tauri::{AppHandle, Emitter, Manager};

use crate::config::Config;
use crate::events::EV_CAPTURE_STATE;
use crate::pipeline::PipelineParams;
use crate::state::AppState;
use crate::stt::models;
use crate::translate::ollama;
use crate::{config, pipeline, stt, translate};

#[tauri::command]
pub fn set_click_through(app: AppHandle, enabled: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found")?;
    window
        .set_ignore_cursor_events(enabled)
        .map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    state
        .click_through
        .store(enabled, std::sync::atomic::Ordering::Relaxed);
    crate::tray::sync_click_through_item(&app, enabled);
    Ok(())
}

#[tauri::command]
pub fn show_settings(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("settings window not found")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_config(app: AppHandle) -> Config {
    app.state::<AppState>().config.lock().unwrap().clone()
}

/// Saves the configuration. Restarts the pipelines with the new settings if capturing.
#[tauri::command]
pub async fn set_config(app: AppHandle, config: Config) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let was_capturing = is_capturing(&app);

        {
            let state = app.state::<AppState>();
            *state.config.lock().unwrap() = config.clone();
        }
        config::save(&app, &config)?;

        if was_capturing {
            stop_capture_impl(&app)?;
            start_capture_impl(&app)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_ollama_models() -> Result<Vec<String>, String> {
    ollama::list_models().await
}

#[tauri::command]
pub async fn check_ollama() -> bool {
    ollama::check().await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub microphone: bool,
    pub screen_recording: bool,
}

#[tauri::command]
pub async fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        microphone: tauri_plugin_macos_permissions::check_microphone_permission().await,
        screen_recording: tauri_plugin_macos_permissions::check_screen_recording_permission().await,
    }
}

#[tauri::command]
pub async fn request_microphone_permission() -> Result<(), String> {
    tauri_plugin_macos_permissions::request_microphone_permission().await
}

#[tauri::command]
pub async fn request_screen_recording_permission() {
    tauri_plugin_macos_permissions::request_screen_recording_permission().await
}

#[tauri::command]
pub fn list_whisper_models(app: AppHandle) -> Result<Vec<models::WhisperModelInfo>, String> {
    models::list(&app)
}

#[tauri::command]
pub async fn download_whisper_model(app: AppHandle, file: String) -> Result<(), String> {
    models::download(app, file).await
}

#[tauri::command]
pub fn is_onboarding_needed(app: AppHandle) -> bool {
    !models::is_ready(&app)
}

/// Exports the session history as a Markdown file in ~/Downloads and returns its path.
/// With `with_summary`, prepends an Ollama-generated summary (key points / decisions / TODOs).
#[tauri::command]
pub async fn export_transcript(app: AppHandle, with_summary: bool) -> Result<String, String> {
    // Extract everything needed up front so no state lock is held across awaits
    let (body, plain, model) = {
        let state = app.state::<AppState>();
        if state.history.is_empty() {
            return Err("書き出す発話がまだありません".into());
        }
        let model = state.config.lock().unwrap().ollama_model.clone();
        (state.history.to_markdown(), state.history.to_plain_text(), model)
    };

    let now = chrono::Local::now();
    let mut md = format!("# ミーティング記録 {}\n\n", now.format("%Y-%m-%d %H:%M"));

    if with_summary {
        let model = match model {
            Some(m) => m,
            None => ollama::list_models()
                .await?
                .into_iter()
                .next()
                .ok_or("Ollama にモデルがありません")?,
        };
        let summary = ollama::summarize(&model, &plain).await?;
        md.push_str("## サマリ\n\n");
        md.push_str(&summary);
        md.push_str("\n\n## トランスクリプト\n\n");
    }
    md.push_str(&body);

    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("Downloads フォルダが見つかりません: {e}"))?;
    let path = dir.join(format!("kotonoha-{}.md", now.format("%Y%m%d-%H%M%S")));
    std::fs::write(&path, md).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Starts the capture + transcription pipelines per the current config.
/// Runs off the main thread: model loading and SCK enumeration are slow.
#[tauri::command]
pub async fn start_capture(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start_capture_impl(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn stop_capture(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || stop_capture_impl(&app))
        .await
        .map_err(|e| e.to_string())?
}

fn start_capture_impl(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (mic_enabled, system_enabled, language, translate) = {
        let config = state.config.lock().unwrap();
        (
            config.mic_enabled,
            config.system_enabled,
            config.whisper_language(),
            config.direction == crate::config::Direction::EnJa,
        )
    };

    let translation = {
        let mut guard = state.translation_queue.lock().unwrap();
        if guard.is_none() {
            *guard = Some(translate::queue::spawn(app.clone()));
        }
        guard.as_ref().unwrap().clone()
    };

    let engine = {
        let mut guard = state.stt_engine.lock().unwrap();
        if guard.is_none() {
            let model_path = models::whisper_model_path(app)?;
            let (engine, handle) = stt::engine::spawn(
                app.clone(),
                model_path.to_string_lossy().into_owned(),
                translation,
            );
            *state.stt_thread.lock().unwrap() = Some(handle);
            *guard = Some(engine);
        }
        guard.as_ref().unwrap().clone()
    };

    let params = PipelineParams {
        vad_model_path: models::vad_model_path(app)?.to_string_lossy().into_owned(),
        language,
        translate,
    };

    if mic_enabled {
        let mut guard = state.mic_pipeline.lock().unwrap();
        if guard.is_none() {
            *guard = Some(pipeline::start_mic(app.clone(), engine.clone(), params.clone())?);
        }
    }

    if system_enabled {
        let mut guard = state.system_pipeline.lock().unwrap();
        if guard.is_none() {
            *guard = Some(pipeline::start_system(app.clone(), engine.clone(), params)?);
        }
    }

    let _ = app.emit(EV_CAPTURE_STATE, true);
    Ok(())
}

fn stop_capture_impl(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    drop(state.mic_pipeline.lock().unwrap().take());
    drop(state.system_pipeline.lock().unwrap().take());
    let _ = app.emit(EV_CAPTURE_STATE, false);
    Ok(())
}

pub fn is_capturing(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let mic = state.mic_pipeline.lock().unwrap().is_some();
    let system = state.system_pipeline.lock().unwrap().is_some();
    mic || system
}

/// Tears down audio + whisper threads. Must run before exiting: process
/// teardown while the ggml Metal context is alive aborts in static destructors.
pub fn shutdown(app: &AppHandle) {
    let state = app.state::<AppState>();
    drop(state.mic_pipeline.lock().unwrap().take());
    drop(state.system_pipeline.lock().unwrap().take());
    drop(state.stt_engine.lock().unwrap().take());
    let handle = state.stt_thread.lock().unwrap().take();
    if let Some(handle) = handle {
        // Bounded by the in-flight inference (~a second at most)
        let _ = handle.join();
    }
}
