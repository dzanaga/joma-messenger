//! Closing the window keeps Joma running: under Hyprland the window moves to the hidden
//! special workspace `special:joma` and comes back from the tray icon, `joma toggle`,
//! `joma open` or a call. GPUI ends the app with its last window, and Hyprland has no
//! minimize, so this is how Joma stays in the tray and keeps receiving messages and calls.
//!
//! The window has two shapes, both floating: compact, a panel under the bar at the right
//! (like a tray app's popup), and full, a messenger-sized window centred on the screen.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

const HIDDEN: &str = "special:joma";

fn hyprctl(args: &[&str]) -> Option<String> {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let out = Command::new("hyprctl").args(args).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The main window's address and workspace name.
fn main_window() -> Option<(String, String)> {
    let clients: serde_json::Value = serde_json::from_str(&hyprctl(&["clients", "-j"])?).ok()?;
    let pid = std::process::id() as u64;
    clients.as_array()?.iter().find_map(|c| {
        (c["pid"].as_u64() == Some(pid) && c["class"] == "joma").then(|| {
            (c["address"].as_str().unwrap_or_default().to_owned(), c["workspace"]["name"].as_str().unwrap_or_default().to_owned())
        })
    })
}

fn dispatch(lua: &str) -> bool {
    hyprctl(&["dispatch", lua]).is_some_and(|o| o.trim() == "ok")
}

/// Hide the main window. False when that is not possible (not Hyprland): then it closes.
pub fn hide() -> bool {
    let Some((address, _)) = main_window() else { return false };
    dispatch(&format!("hl.dsp.window.move({{ workspace = '{HIDDEN}', follow = false, window = 'address:{address}' }})"))
}

/// The window opens straight into `special:joma` (starting in the tray): a rule on the
/// title it is created with, which no later window has.
pub const HIDDEN_TITLE: &str = "joma-start-hidden";

pub fn start_hidden() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        && hyprctl(&[
            "eval",
            &format!("hl.window_rule({{ match = {{ initial_title = '^{HIDDEN_TITLE}$' }}, workspace = '{HIDDEN} silent', no_initial_focus = true }})"),
        ])
        .is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Compact,
    Full,
}

/// Started with `--hidden` (at login) or restarted while hidden.
pub static START_HIDDEN: AtomicBool = AtomicBool::new(false);
static FULL: AtomicBool = AtomicBool::new(false);

/// The shape the window shows in.
pub fn shape() -> Shape {
    if FULL.load(Ordering::Relaxed) { Shape::Full } else { Shape::Compact }
}

pub fn set_shape(s: Shape) {
    FULL.store(s == Shape::Full, Ordering::Relaxed);
}

/// The compact panel's and the full window's sizes, logical pixels.
const PANEL: (i64, i64) = (460, 680);
const WINDOW: (i64, i64) = (980, 680);

/// A normal start maps the window floating and centred (a rule on its first title).
pub fn float_at_start() {
    let (w, h) = WINDOW;
    hyprctl(&[
        "eval",
        &format!("hl.window_rule({{ match = {{ class = '^joma$', initial_title = '^Joma$' }}, float = true, size = {{ {w}, {h} }}, center = true }})"),
    ]);
}

/// The focused monitor's usable size, logical pixels.
fn usable() -> Option<(i64, i64)> {
    let monitors: serde_json::Value = serde_json::from_str(&hyprctl(&["monitors", "-j"])?).ok()?;
    let m = monitors.as_array()?.iter().find(|m| m["focused"] == true)?;
    let scale = m["scale"].as_f64().filter(|s| *s > 0.0).unwrap_or(1.0);
    let r: Vec<i64> = m["reserved"].as_array()?.iter().filter_map(|v| v.as_i64()).collect();
    let w = (m["width"].as_f64()? / scale) as i64 - r.first().unwrap_or(&0) - r.get(2).unwrap_or(&0);
    let h = (m["height"].as_f64()? / scale) as i64 - r.get(1).unwrap_or(&0) - r.get(3).unwrap_or(&0);
    Some((w, h))
}
const GAP: i64 = 8;

/// Where the panel goes on the focused monitor: under the bar (the reserved top), at
/// the right.
fn panel_rect() -> Option<(i64, i64, i64, i64)> {
    let monitors: serde_json::Value = serde_json::from_str(&hyprctl(&["monitors", "-j"])?).ok()?;
    let m = monitors.as_array()?.iter().find(|m| m["focused"] == true)?;
    let scale = m["scale"].as_f64().filter(|s| *s > 0.0).unwrap_or(1.0);
    let (mx, my) = (m["x"].as_i64()?, m["y"].as_i64()?);
    let w = (m["width"].as_f64()? / scale) as i64;
    let h = (m["height"].as_f64()? / scale) as i64;
    let r: Vec<i64> = m["reserved"].as_array()?.iter().filter_map(|v| v.as_i64()).collect();
    let (top, right, bottom) = (r.get(1).copied().unwrap_or(0), r.get(2).copied().unwrap_or(0), r.get(3).copied().unwrap_or(0));
    let ph = PANEL.1.min(h - top - bottom - 2 * GAP).max(300);
    Some((mx + w - right - PANEL.0 - GAP, my + top + GAP, PANEL.0, ph))
}

/// Show the window in a shape, on the current workspace, focused.
pub fn show_as(shape: Shape) -> bool {
    let Some((address, _)) = main_window() else { return false };
    let w = format!("window = 'address:{address}'");
    // To the current workspace, from `special:joma` or wherever it was left. (Not
    // pinned on every workspace: a pinned window loses keys in Hyprland 0.56.)
    dispatch(&format!("hl.dsp.window.move({{ workspace = 'e+0', {w} }})"));
    match shape {
        Shape::Compact => {
            dispatch(&format!("hl.dsp.window.float({{ action = 'enable', {w} }})"));
            if let Some((x, y, pw, ph)) = panel_rect() {
                dispatch(&format!("hl.dsp.window.resize({{ x = {pw}, y = {ph}, {w} }})"));
                dispatch(&format!("hl.dsp.window.move({{ x = {x}, y = {y}, {w} }})"));
            }
        }
        Shape::Full => {
            dispatch(&format!("hl.dsp.window.float({{ action = 'enable', {w} }})"));
            let (uw, uh) = usable().unwrap_or(WINDOW);
            let (fw, fh) = (WINDOW.0.min(uw * 9 / 10), WINDOW.1.min(uh * 9 / 10));
            dispatch(&format!("hl.dsp.window.resize({{ x = {fw}, y = {fh}, {w} }})"));
            dispatch(&format!("hl.dsp.window.center({{ {w} }})"));
        }
    }
    dispatch(&format!("hl.dsp.focus({{ {w} }})"))
}

/// Is the main window the focused one?
pub fn focused() -> bool {
    let Some((address, _)) = main_window() else { return false };
    hyprctl(&["activewindow", "-j"])
        .and_then(|o| serde_json::from_str::<serde_json::Value>(&o).ok())
        .is_some_and(|a| a["address"].as_str() == Some(address.as_str()))
}

/// Is the main window hidden?
pub fn hidden() -> bool {
    main_window().is_some_and(|(_, workspace)| workspace == HIDDEN)
}
