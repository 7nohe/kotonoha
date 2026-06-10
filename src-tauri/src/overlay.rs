use tauri::{AppHandle, LogicalPosition, Manager};
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt};

tauri_panel! {
    panel!(OverlayPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// Converts the overlay window into an NSPanel.
/// nonactivating: clicking it does not steal focus.
/// full_screen_auxiliary: shows above full-screen meeting apps.
pub fn init_overlay_panel(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found")?;

    position_bottom_center(&window);

    let panel = window
        .to_panel::<OverlayPanel>()
        .map_err(|e| format!("to_panel failed: {e:?}"))?;

    // Panel conversion can re-enable the macOS window shadow, so explicitly disable it
    let _ = window.set_shadow(false);

    // Floating(4) does not appear above full-screen apps, so use Status(25)
    panel.set_level(PanelLevel::Status.into());
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    // Note: do not use alwaysOnTop / visibleOnAllWorkspaces in tauri.conf.json.
    // Tauri overwrites collectionBehavior after setup, dropping fullScreenAuxiliary,
    // so control it solely on the panel side here.
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .stationary()
            .ignores_cycle()
            .into(),
    );

    Ok(())
}

/// Window size as configured in tauri.conf.json (logical points)
const OVERLAY_WIDTH: f64 = 640.0;
const OVERLAY_HEIGHT: f64 = 360.0;
const MARGIN_BOTTOM: f64 = 96.0;

/// Places the overlay at the bottom center of the primary monitor.
/// Works entirely in logical (point) coordinates: mixing physical pixels across
/// monitors with different scale factors can place the window off-screen.
fn position_bottom_center(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let screen = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position().to_logical::<f64>(scale);
    let x = origin.x + (screen.width - OVERLAY_WIDTH) / 2.0;
    let y = origin.y + screen.height - OVERLAY_HEIGHT - MARGIN_BOTTOM;
    let _ = window.set_position(LogicalPosition::new(x, y));
}
