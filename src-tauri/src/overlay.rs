use std::sync::Mutex;

use tauri::{AppHandle, Emitter, LogicalPosition, Manager};
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt};

use crate::events::EV_OVERLAY_PASSTHROUGH;
use crate::state::AppState;

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

// ---- Cursor hit-testing ----
//
// The overlay window is much larger than the visible pill, and the webview
// swallows mouse events across its whole frame. The frontend reports the
// pill's rect (logical px, webview top-left origin) and a poller flips
// setIgnoresMouseEvents so only the pill blocks clicks to apps underneath.

/// Visible pill bounds reported by the frontend
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

const HITTEST_INTERVAL_MS: u64 = 60;
/// Grace zone around the pill so the idle scale(0.97) shrink and fast cursor
/// moves don't flicker the interactive state
const REGION_MARGIN: f64 = 8.0;

/// Single owner of the overlay's ignore-mouse state. Every path that makes
/// the window non-interactive must go through here: the webview keeps its
/// stale :hover state once events stop arriving, so the frontend is told to
/// drop hover-driven UI.
pub fn apply_pass_through(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    pass_through: bool,
) -> tauri::Result<()> {
    window.set_ignore_cursor_events(pass_through)?;
    if pass_through {
        let _ = app.emit_to("overlay", EV_OVERLAY_PASSTHROUGH, true);
    }
    Ok(())
}

pub fn start_cursor_hittest(app: &AppHandle) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        // Last pass-through value applied by this poller (None = force reapply)
        let last = std::sync::Arc::new(Mutex::new(None::<bool>));
        let mut yielded_to_click_through = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(HITTEST_INTERVAL_MS));
            // Manual click-through owns the window state; don't even hop to
            // the main thread while it is on, and reapply once it turns off
            if app
                .state::<AppState>()
                .click_through
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                yielded_to_click_through = true;
                continue;
            }
            let force_reapply = std::mem::take(&mut yielded_to_click_through);
            let app2 = app.clone();
            let window2 = window.clone();
            let last2 = last.clone();
            // AppKit window/cursor APIs must run on the main thread
            let posted = app.run_on_main_thread(move || {
                if force_reapply {
                    *last2.lock().unwrap() = None;
                }
                hittest_tick(&app2, &window2, &last2);
            });
            if posted.is_err() {
                return; // event loop is gone (app shutting down)
            }
        }
    });
}

fn hittest_tick(app: &AppHandle, window: &tauri::WebviewWindow, last: &Mutex<Option<bool>>) {
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let region = *app.state::<AppState>().interactive_region.lock().unwrap();
    let pass_through = match region {
        // Nothing reported yet: keep the whole window interactive
        None => false,
        Some(r) => {
            let (Ok(cursor), Ok(win_pos)) = (app.cursor_position(), window.outer_position()) else {
                return;
            };
            let scale = window.scale_factor().unwrap_or(1.0);
            let x = (cursor.x - win_pos.x as f64) / scale;
            let y = (cursor.y - win_pos.y as f64) / scale;
            let inside = x >= r.x - REGION_MARGIN
                && x <= r.x + r.width + REGION_MARGIN
                && y >= r.y - REGION_MARGIN
                && y <= r.y + r.height + REGION_MARGIN;
            !inside
        }
    };

    let mut guard = last.lock().unwrap();
    if *guard == Some(pass_through) {
        return;
    }
    if apply_pass_through(app, window, pass_through).is_ok() {
        *guard = Some(pass_through);
    }
}
