//! Closing the window keeps Joma running: under Hyprland the window moves to the hidden
//! special workspace `special:joma` and comes back on `joma`, `joma open` or a call.
//! GPUI ends the app with its last window, and Hyprland has no minimize, so this is how
//! Joma stays reachable from the bar and keeps receiving messages and calls.

use std::process::{Command, Stdio};

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

/// Bring the main window back to the current workspace if it is hidden.
pub fn show() {
    let Some((address, workspace)) = main_window() else { return };
    if workspace == HIDDEN {
        dispatch(&format!("hl.dsp.window.move({{ workspace = 'e+0', window = 'address:{address}' }})"));
    }
    dispatch(&format!("hl.dsp.focus({{ window = 'address:{address}' }})"));
}
