mod audio;
mod commands;
mod config;
mod events;
mod history;
mod overlay;
mod pipeline;
mod state;
mod stt;
mod translate;
mod tray;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_macos_permissions::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::set_click_through,
            commands::show_settings,
            commands::start_capture,
            commands::stop_capture,
            commands::get_config,
            commands::set_config,
            commands::list_ollama_models,
            commands::check_ollama,
            commands::check_permissions,
            commands::request_microphone_permission,
            commands::request_screen_recording_permission,
            commands::list_whisper_models,
            commands::download_whisper_model,
            commands::is_onboarding_needed,
            commands::export_transcript,
        ])
        .setup(|app| {
            // Redirect whisper.cpp's stderr spam through the log crate
            whisper_rs::install_logging_hooks();

            // Load persisted configuration
            let loaded = config::load(app.handle());
            *app.state::<state::AppState>().config.lock().unwrap() = loaded;

            // Resident app with no Dock icon (tray + overlay only)
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::init_tray(app.handle())?;
            overlay::init_overlay_panel(app.handle()).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

            // On first launch (no model downloaded yet), open onboarding in the settings window
            if !stt::models::is_ready(app.handle()) {
                if let Some(settings) = app.get_webview_window("settings") {
                    let _ = settings.show();
                    let _ = settings.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the settings window hides it instead of destroying it (reopened from the tray)
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
