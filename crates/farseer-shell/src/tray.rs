//! The tray: what an operator needs to know without opening anything.
//!
//! `28 operator surface` made the canvas the home screen, and a canvas has to be
//! looked at. `35 notification plane` covered the other end - something happened,
//! wake somebody - and left the middle unserved: **is there quota left right
//! now**, asked at a glance, while doing something else.
//!
//! That question has an answer here and nowhere else, because `33 google quota`'s
//! reversal made `/v1/quota` report every account omp is logged into, polled on a
//! timer, **whether or not anything is running**. Every other surface in farseer
//! describes runs.
//!
//! Two rules carry over unchanged, and both are load-bearing:
//!
//! - **Never a percentage farseer computed.** `27 quota accounting` refused one
//!   because farseer's spend is a lower bound on a window drained by sessions it
//!   cannot see - most wrong exactly near exhaustion. A tray line is the worst
//!   possible place to put a number an operator cannot check.
//! - **Absent is absent.** A runner that states no percentage produces a line
//!   without one, not a zero.

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

/// How often the tray asks. Windows move on the scale of hours, and the poll
/// behind `/v1/quota` is itself on a five-minute timer - asking faster would
/// re-read the same snapshot.
const REFRESH: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Window {
    pub account: String,
    pub status: String,
    pub resets_at: Option<i64>,
    #[serde(default)]
    pub rate_limit_type: String,
    #[serde(default)]
    pub used_percent: Option<i64>,
    #[serde(default)]
    pub runners: Vec<String>,
    /// The provider's own id, when the source named one.
    #[serde(default)]
    pub provider: Option<String>,
    /// The provider's own name for the window.
    #[serde(default)]
    pub label: Option<String>,
}

/// What to call this window's owner in one or two words.
///
/// The account was an **email** for every window omp reports, and eleven lines
/// each beginning `abah.intelek@gmail.com` is a menu whose first column carries
/// no information at all. The provider is the part that differs.
fn who(window: &Window) -> String {
    if let Some(provider) = &window.provider {
        return provider
            .rsplit_once('-')
            .filter(|(_, tail)| matches!(*tail, "oauth" | "device" | "api" | "cli"))
            .map(|(head, _)| head)
            .unwrap_or(provider)
            .replace('-', " ");
    }
    window
        .runners
        .first()
        .cloned()
        .unwrap_or_else(|| window.account.clone())
}

/// This window's own short name, under a heading that already says the provider.
fn what(window: &Window) -> String {
    if let Some(label) = &window.label {
        return label.clone();
    }
    let id = match &window.provider {
        Some(provider) => window
            .rate_limit_type
            .strip_prefix(&format!("{provider}:"))
            .unwrap_or(&window.rate_limit_type),
        None => &window.rate_limit_type,
    };
    id.replace(['_', ':'], " ")
}

/// The one line a tray tooltip has room for.
///
/// **The most constrained window wins**, because that is the only one that
/// changes what an operator does next: an account with three windows at 2% and
/// one exhausted is, for every practical purpose, exhausted.
pub(crate) fn tooltip(windows: &[Window], now_secs: i64) -> String {
    let Some(window) = most_constrained(windows) else {
        // Not "0%" and not "ok". Nothing has been observed, and saying so is the
        // difference between a quiet fleet and a broken poller.
        return "farseer - no window reported yet".to_string();
    };
    let mut line = format!("farseer - {}", who(window));
    if window.status == "exhausted_until" {
        line.push_str(" exhausted");
    } else if let Some(percent) = window.used_percent {
        // Whose number this is matters more here than anywhere: a tray line is
        // read in half a second and remembered as fact.
        line.push_str(&format!(" {percent}% used (provider)"));
    } else {
        line.push_str(" - no percentage reported");
    }
    if let Some(reset) = window.resets_at {
        line.push_str(&format!(", {}", countdown(reset, now_secs)));
    }
    line
}

/// One menu line per window, in the same order the tooltip picked from.
pub(crate) fn lines(windows: &[Window], now_secs: i64) -> Vec<String> {
    let mut ranked: Vec<&Window> = windows.iter().collect();
    ranked.sort_by_key(|w| std::cmp::Reverse(pressure(w)));
    ranked
        .iter()
        .map(|w| {
            let limit = match what(w) {
                name if name.is_empty() => String::new(),
                name => format!(" {name}"),
            };
            let state = if w.status == "exhausted_until" {
                "exhausted".to_string()
            } else {
                match w.used_percent {
                    Some(percent) => format!("{percent}%"),
                    None => "-".to_string(),
                }
            };
            // Bare `4h 38m` rather than `resets in 4h 38m`: eleven lines of a
            // tray menu have no room to repeat the same three words eleven
            // times, and the tooltip above still spells it out once.
            let reset = w
                .resets_at
                .map(|r| format!("  {}", countdown(r, now_secs).replace("resets in ", "")))
                .unwrap_or_default();
            format!("{}{}  {}{}", who(w), limit, state, reset)
        })
        .collect()
}

/// Exhaustion first, then the provider's own percentage, then nothing.
///
/// A window with no stated percentage sorts **below** one at 0%, which is
/// deliberate: farseer knows less about it, and ranking an unknown above a known
/// zero would put the least informative row in the tooltip.
fn pressure(window: &Window) -> i64 {
    if window.status == "exhausted_until" {
        return 1_000;
    }
    window.used_percent.unwrap_or(-1)
}

fn most_constrained(windows: &[Window]) -> Option<&Window> {
    windows.iter().max_by_key(|w| pressure(w))
}

fn countdown(resets_at: i64, now_secs: i64) -> String {
    let seconds = resets_at - now_secs;
    if seconds <= 0 {
        return "reset due".to_string();
    }
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("resets in {hours}h {minutes}m")
    } else {
        format!("resets in {minutes}m")
    }
}

/// Build the tray and keep it current for as long as the shell runs.
///
/// The shell holds the operator token and the daemon's port, so the tray reads
/// the same `/v1/quota` the canvas does - one surface, one answer. Nothing here
/// is a second source of truth.
pub(crate) fn install<R: Runtime>(
    app: &AppHandle<R>,
    farseer: String,
    token: String,
) -> anyhow::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show canvas", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit farseer", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &PredefinedMenuItem::separator(app)?, &quit])?;

    let tray = TrayIconBuilder::with_id("farseer")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no window icon to use for the tray"))?,
        )
        .tooltip("farseer - reading windows...")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("canvas") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    let tray = Arc::new(tray);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::new();
        let mut ticker = tokio::time::interval(REFRESH);
        loop {
            ticker.tick().await;
            let Ok(response) = client
                .get(format!("{farseer}/v1/quota"))
                .bearer_auth(&token)
                .send()
                .await
            else {
                // A daemon that is restarting is not an error worth shouting
                // about; the next tick will find it.
                continue;
            };
            let Ok(body) = response.json::<serde_json::Value>().await else {
                continue;
            };
            let windows: Vec<Window> = body
                .get("windows")
                .and_then(|w| serde_json::from_value(w.clone()).ok())
                .unwrap_or_default();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or_default();

            let _ = tray.set_tooltip(Some(tooltip(&windows, now)));
            if let Ok(menu) = rebuild(&handle, &windows, now) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    });
    Ok(())
}

/// The menu, with one **disabled** line per window above the actions.
///
/// Disabled because they are readings rather than commands. `28 operator
/// surface` keeps the verbs on the canvas, and a tray that can act is a second
/// control surface to keep in step with the first.
fn rebuild<R: Runtime>(
    app: &AppHandle<R>,
    windows: &[Window],
    now: i64,
) -> anyhow::Result<Menu<R>> {
    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::new();
    if windows.is_empty() {
        items.push(Box::new(MenuItem::with_id(
            app,
            "none",
            "no window reported yet",
            false,
            None::<&str>,
        )?));
    }
    for (index, line) in lines(windows, now).iter().enumerate() {
        items.push(Box::new(MenuItem::with_id(
            app,
            format!("w{index}"),
            line,
            false,
            None::<&str>,
        )?));
    }
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "show",
        "Show canvas",
        true,
        None::<&str>,
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "quit",
        "Quit farseer",
        true,
        None::<&str>,
    )?));
    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items.iter().map(|i| i.as_ref()).collect();
    Ok(Menu::with_items(app, &refs)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(account: &str, status: &str, percent: Option<i64>) -> Window {
        Window {
            account: account.to_string(),
            status: status.to_string(),
            resets_at: Some(1_000 + 3_600),
            rate_limit_type: "primary".to_string(),
            used_percent: percent,
            runners: vec![account.to_string()],
            provider: None,
            label: None,
        }
    }

    /// A window as omp reports one: a provider id, a label, and an email for an
    /// account that four other providers share.
    fn polled(provider: &str, label: &str, percent: i64) -> Window {
        Window {
            account: "abah.intelek@gmail.com".to_string(),
            status: "allowed".to_string(),
            resets_at: Some(1_000 + 3_600),
            rate_limit_type: format!("{provider}:primary"),
            used_percent: Some(percent),
            runners: vec!["omp".to_string()],
            provider: Some(provider.to_string()),
            label: Some(label.to_string()),
        }
    }

    /// Eleven lines that all begin with the same email is a first column
    /// carrying no information. The provider is the part that differs, and the
    /// sign-in suffix is not part of the provider's name.
    #[test]
    fn a_polled_window_is_named_by_its_provider_rather_than_its_login() {
        let lines = lines(
            &[
                polled("openai-codex", "5 hours", 98),
                polled("xai-oauth", "SuperGrok Weekly Credits", 3),
            ],
            1_000,
        );
        assert!(
            lines[0].starts_with("openai codex 5 hours"),
            "{:?}",
            lines[0]
        );
        assert!(
            lines[1].starts_with("xai SuperGrok Weekly Credits"),
            "the sign-in method is not the provider's name: {:?}",
            lines[1]
        );
        // The email appears nowhere: it is the one thing these two windows share.
        assert!(lines.iter().all(|line| !line.contains('@')));
    }

    /// The tooltip has room for one window, so it must be the one that changes
    /// what the operator does next.
    #[test]
    fn the_most_constrained_window_is_the_one_shown() {
        let windows = vec![
            window("codex", "allowed", Some(2)),
            window("anthropic-max", "exhausted_until", Some(100)),
            window("google", "allowed", Some(40)),
        ];
        let line = tooltip(&windows, 1_000);
        assert!(line.contains("anthropic-max"), "{line}");
        assert!(line.contains("exhausted"), "{line}");
        assert!(line.contains("resets in 1h 0m"), "{line}");
    }

    /// `27 quota accounting`'s refusal, at the surface where it matters most: a
    /// tray line is read in half a second and remembered as fact.
    #[test]
    fn a_window_with_no_stated_percentage_gets_none_invented() {
        let line = tooltip(&[window("pi", "allowed", None)], 1_000);
        assert!(line.contains("no percentage reported"), "{line}");
        assert!(!line.contains('%'), "{line}");

        // And it sorts below a known zero, because farseer knows less about it.
        let windows = vec![
            window("pi", "allowed", None),
            window("codex", "allowed", Some(0)),
        ];
        assert!(tooltip(&windows, 1_000).contains("codex"));
    }

    /// Nothing observed is its own state, and not a healthy one.
    #[test]
    fn an_empty_fleet_says_so_rather_than_reporting_zero() {
        let line = tooltip(&[], 1_000);
        assert!(line.contains("no window reported"), "{line}");
        assert!(!line.contains('%'), "{line}");
    }

    /// Every window gets a line, worst first, so the menu answers the question
    /// the tooltip had to compress.
    #[test]
    fn the_menu_lists_every_window_worst_first() {
        let windows = vec![
            window("codex", "allowed", Some(2)),
            window("anthropic-max", "exhausted_until", Some(100)),
        ];
        let lines = lines(&windows, 1_000);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("anthropic-max"), "{lines:?}");
        assert!(lines[0].contains("exhausted"), "{lines:?}");
        assert!(lines[1].contains("2%"), "{lines:?}");
    }
}
