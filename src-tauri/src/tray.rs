use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

use crate::state::AppState;

pub fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle_overlay = MenuItem::with_id(app, "toggle_overlay", "オーバーレイを表示/隠す", true, None::<&str>)?;
    let click_through = CheckMenuItem::with_id(app, "click_through", "クリックスルー", true, false, None::<&str>)?;
    let export = MenuItem::with_id(app, "export", "議事録を書き出す", true, Some("Cmd+E"))?;
    let export_summary = MenuItem::with_id(app, "export_summary", "サマリ付きで書き出す (Ollama)", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "設定...", true, Some("Cmd+,"))?;
    let quit = MenuItem::with_id(app, "quit", "kotonoha を終了", true, Some("Cmd+Q"))?;

    let menu = Menu::with_items(
        app,
        &[
            &toggle_overlay,
            &click_through,
            &PredefinedMenuItem::separator(app)?,
            &export,
            &export_summary,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let state = app.state::<AppState>();
    *state.tray_click_through_item.lock().unwrap() = Some(click_through.clone());

    // The app icon (full-color squircle) washes out to white when used as a template,
    // so the menu bar uses a dedicated black+alpha glyph (design/tray.svg)
    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))
        .expect("tray icon");

    TrayIconBuilder::with_id("main")
        .icon(tray_icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle_overlay" => {
                if let Some(window) = app.get_webview_window("overlay") {
                    let visible = window.is_visible().unwrap_or(false);
                    let _ = if visible { window.hide() } else { window.show() };
                }
            }
            "click_through" => {
                let enabled = !app
                    .state::<AppState>()
                    .click_through
                    .load(std::sync::atomic::Ordering::Relaxed);
                let _ = crate::commands::set_click_through(app.clone(), enabled);
            }
            "export" | "export_summary" => {
                let with_summary = event.id().as_ref() == "export_summary";
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    match crate::commands::export_transcript(app.clone(), with_summary).await {
                        // Reveal the exported Markdown in Finder
                        Ok(path) => {
                            let _ = std::process::Command::new("open").args(["-R", &path]).spawn();
                        }
                        Err(e) => {
                            let _ = tauri::Emitter::emit(
                                &app,
                                "pipeline-error",
                                crate::events::PipelineErrorEvent { message: e },
                            );
                        }
                    }
                });
            }
            "settings" => {
                let _ = crate::commands::show_settings(app.clone());
            }
            "quit" => {
                // Shut down the audio/whisper threads BEFORE exiting: process
                // teardown while the ggml Metal context is alive calls abort()
                // in static destructors (crash-on-quit).
                let state = app.state::<AppState>();
                drop(state.mic_pipeline.lock().unwrap().take());
                drop(state.system_pipeline.lock().unwrap().take());
                drop(state.stt_engine.lock().unwrap().take());
                if let Some(handle) = state.stt_thread.lock().unwrap().take() {
                    // Bounded by the in-flight inference (~a second at most)
                    let _ = handle.join();
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

pub fn sync_click_through_item(app: &AppHandle, enabled: bool) {
    let state = app.state::<AppState>();
    let guard = state.tray_click_through_item.lock().unwrap();
    if let Some(item) = guard.as_ref() {
        let _ = item.set_checked(enabled);
    }
}
