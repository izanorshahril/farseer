//! Where the window was last time.
//!
//! `24 ui state persistence` settled where an arrangement lives, and settled it
//! against the obvious answer: not `localStorage`, not a dotfile beside the
//! executable, but **farseer's own store**, so the command center comes back the
//! way it was left even from a different window, and a backup of the data
//! directory backs up the layout for free.
//!
//! A window's own size and position is the same class of fact as the widget
//! order inside it, so it goes to the same place through the same endpoint,
//! under its own key. That is why this reaches for `/v1/ui-state` rather than
//! `tauri-plugin-window-state`: a second store for one more fact is a second
//! thing to back up, and a fact kept somewhere else is a fact that gets left
//! behind.

use serde::{Deserialize, Serialize};
use tauri::{LogicalPosition, LogicalSize, Runtime};

/// The key this holds in the ui-state blob, beside `canvas`.
const KEY: &str = "window";

/// What farseer opened with the first time, and what it falls back to.
const DEFAULT: (f64, f64) = (1280.0, 820.0);

/// A window's geometry, in **logical** pixels.
///
/// Logical rather than physical because the two differ by the display's scale
/// factor, and a window saved on a 150% monitor and restored on a 100% one
/// would come back two-thirds the size if the number were physical. Tauri
/// reports physical and converts on request; that conversion is the whole
/// reason this type exists rather than storing what the event handed over.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    /// Absent when the last position could not be trusted - see [`Geometry::place`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Recorded beside the size rather than instead of it, because a maximized
    /// window's size is the screen's and not the operator's. Un-maximizing has
    /// to give back the window they actually chose.
    #[serde(default)]
    pub maximized: bool,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            width: DEFAULT.0,
            height: DEFAULT.1,
            x: None,
            y: None,
            maximized: false,
        }
    }
}

impl Geometry {
    /// A size no smaller than a canvas can use.
    ///
    /// A window can be dragged to nothing, and a stored 40x30 reopens as a
    /// title bar with no content and no obvious way back. Clamped on the way
    /// out rather than refused on the way in: the operator did resize it to
    /// that, and a record of what happened is not the place to argue.
    fn sane_size(self) -> (f64, f64) {
        (self.width.max(640.0), self.height.max(480.0))
    }

    /// Whether this position still lands on a display that exists.
    ///
    /// **The failure this prevents cannot be clicked out of.** A window saved
    /// on a second monitor and reopened without it is placed off-screen, where
    /// it cannot be moved, focused or closed - and on exit it saves that
    /// position again. Nothing looks broken; the app simply never appears.
    ///
    /// The position is kept only when the window's own top-left corner sits
    /// inside some monitor, inset far enough that a corner technically on a
    /// screen but with no reachable title bar still counts as lost.
    fn place<R: Runtime>(self, window: &tauri::WebviewWindow<R>) -> Option<LogicalPosition<f64>> {
        let (x, y) = (self.x?, self.y?);
        let scale = window.scale_factor().unwrap_or(1.0);
        let monitors = window.available_monitors().ok()?;
        let reachable = monitors.iter().any(|monitor| {
            let position = monitor.position();
            let size = monitor.size();
            let left = position.x as f64 / scale;
            let top = position.y as f64 / scale;
            let right = left + size.width as f64 / scale;
            let bottom = top + size.height as f64 / scale;
            x >= left - 64.0 && x <= right - 64.0 && y >= top - 32.0 && y <= bottom - 64.0
        });
        reachable.then_some(LogicalPosition::new(x, y))
    }
}

/// Read the stored geometry, or the default when there is none to read.
///
/// Async, and awaited on the runtime the shell has already built, so `reqwest`
/// keeps the feature set it has - a `blocking` client for one read at startup
/// would pull a second runtime into the binary to do what the first one is
/// sitting idle for.
///
/// Every failure - no daemon, no key, malformed JSON - yields the default. A
/// window that will not open because a preference could not be loaded is worse
/// than a window in the wrong place.
pub async fn load(farseer: &str, token: &str) -> Geometry {
    let read = async {
        let response = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .ok()?
            .get(format!("{farseer}/v1/ui-state/{KEY}"))
            .bearer_auth(token)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json::<Geometry>().await.ok()
    };
    read.await.unwrap_or_default()
}

/// Apply a geometry to a window that has just been built.
pub fn apply<R: Runtime>(window: &tauri::WebviewWindow<R>, geometry: Geometry) {
    let (width, height) = geometry.sane_size();
    let _ = window.set_size(LogicalSize::new(width, height));
    if let Some(position) = geometry.place(window) {
        let _ = window.set_position(position);
    }
    // After the size, so un-maximizing returns to the size just set rather than
    // to whatever the builder started with.
    if geometry.maximized {
        let _ = window.maximize();
    }
}

/// What this window is right now, or `None` while it is minimized.
///
/// A minimized window reports a position far off-screen on Windows - the
/// classic -32000 - and storing that is exactly how an app comes back
/// invisible. Reporting nothing is the honest answer: a minimized window has no
/// geometry the operator chose.
pub fn current<R: Runtime>(window: &tauri::WebviewWindow<R>, last: Geometry) -> Option<Geometry> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        // Only the flag is news: the size underneath is the screen's, and the
        // one worth keeping is the one already held.
        return Some(Geometry { maximized, ..last });
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    // **Inner size, outer position.** They are not a matched pair, and pairing
    // them wrongly is a bug that only shows up across restarts: `set_size` sets
    // the *inner* size, so saving the outer one adds the frame - a 16px border
    // and a 39px title bar on Windows - to the window every single launch. It
    // reopens slightly bigger, saves that, and creeps until it fills the screen.
    // `set_position` and `outer_position` do agree, so the position is read as
    // it is written.
    let size = window.inner_size().ok()?;
    let position = window.outer_position().ok()?;
    Some(Geometry {
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
        x: Some(position.x as f64 / scale),
        y: Some(position.y as f64 / scale),
        maximized,
    })
}

/// Write it back, best effort.
///
/// Failure is silent on purpose: this is a preference, the daemon may already
/// be shutting down when the window closes, and an error dialog on the way out
/// would be the last thing farseer ever said.
pub async fn save(farseer: String, token: String, geometry: Geometry) {
    let _ = reqwest::Client::new()
        .put(format!("{farseer}/v1/ui-state/{KEY}"))
        .bearer_auth(token)
        .json(&geometry)
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_dragged_to_nothing_reopens_usable() {
        let tiny = Geometry {
            width: 40.0,
            height: 30.0,
            ..Geometry::default()
        };
        assert_eq!(tiny.sane_size(), (640.0, 480.0));
    }

    /// The stored shape is JSON farseer never parses, per `24`, so what matters
    /// is that it round-trips and that an absent position stays absent rather
    /// than becoming a zero - `(0, 0)` is a real corner of a real screen.
    #[test]
    fn an_unplaced_window_stores_no_position_at_all() {
        let value = serde_json::to_value(Geometry::default()).unwrap();
        assert!(value.get("x").is_none(), "{value}");
        assert!(value.get("y").is_none(), "{value}");
        let back: Geometry = serde_json::from_value(value).unwrap();
        assert_eq!(back, Geometry::default());
    }

    /// A geometry written by an older shell, or by hand, must not stop the
    /// window opening.
    #[test]
    fn a_partial_record_fills_in_rather_than_failing() {
        let back: Geometry = serde_json::from_str(r#"{"width":900,"height":700}"#).unwrap();
        assert_eq!(back.width, 900.0);
        assert!(!back.maximized);
        assert_eq!(back.x, None);
    }
}
