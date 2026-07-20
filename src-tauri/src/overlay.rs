use std::sync::Mutex;

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager};
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

// ---- Content-sized window ----
//
// The window hugs the visible pill instead of staying a large transparent
// rectangle: transparent dead zones would swallow clicks meant for the apps
// underneath (WKWebView receives mouse events across the whole window frame,
// and toggling setIgnoresMouseEvents dynamically desyncs WebKit's hover
// tracking on macOS 26). With the window fitting the pill, plain CSS :hover
// and click routing just work.

/// Padding kept around the pill; must match .overlay-root's CSS padding
const PAD_X: f64 = 12.0;
const PAD_TOP: f64 = 8.0;
const PAD_BOTTOM: f64 = 14.0;

/// Serializes resize_to_content calls. The frontend's ResizeObserver fires
/// (and invokes) without waiting for the previous call to finish — e.g.
/// collapsing then immediately expanding — and set_size/set_position are two
/// separate round trips. Without this lock, a second call can read the
/// window's geometry between the first call's set_size and set_position,
/// computing from a half-applied state and leaving the window's actual
/// frame drifted from where the pill is drawn, so clicks miss it entirely.
static RESIZE_LOCK: Mutex<()> = Mutex::new(());

/// Resizes the overlay window to fit the pill (logical px, reported by the
/// frontend's ResizeObserver), keeping the bottom-center anchor fixed so the
/// pill doesn't visually move when captions grow or shrink.
pub fn resize_to_content(app: &AppHandle, pill_width: f64, pill_height: f64) -> Result<(), String> {
    let _guard = RESIZE_LOCK.lock().unwrap();

    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found")?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;

    let new_w = (pill_width + PAD_X * 2.0).ceil();
    let new_h = (pill_height + PAD_TOP + PAD_BOTTOM).ceil();

    let pos = window
        .outer_position()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    let size = window
        .outer_size()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(scale);
    // Sub-pixel churn from ResizeObserver would loop forever; only act on real changes
    if (size.width - new_w).abs() < 1.0 && (size.height - new_h).abs() < 1.0 {
        return Ok(());
    }

    let x = pos.x + (size.width - new_w) / 2.0;
    let y = pos.y + (size.height - new_h);
    window
        .set_size(LogicalSize::new(new_w, new_h))
        .map_err(|e| e.to_string())?;
    window
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    Ok(())
}
