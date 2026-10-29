//! Joma on this desktop, without a package or the install script: the launcher entry and
//! icons (written at every start when missing or stale), and on request the pieces the
//! setup banner offers: the tray icon pinned in the Omarchy bar, Super+Alt+M, and
//! starting at login (a systemd user unit, as Omarchy's session runs under uwsm).
//!
//! Each piece names the `joma` to run. Installed with mise that is the shim, which follows
//! upgrades; otherwise the running binary. A piece that names another binary is rewritten
//! at start, so moving Joma does not break them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| home().join(fallback))
}

/// The `joma` the launcher, the unit and the binding run.
pub fn exe() -> PathBuf {
    let current = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("joma"));
    let mise = xdg("XDG_DATA_HOME", ".local/share").join("mise");
    let shim = mise.join("shims/joma");
    if current.starts_with(mise.join("installs")) && shim.exists() {
        shim
    } else {
        current
    }
}

/// A test profile (`JOMA_PROFILE`) has its own HOME, but systemd and Hyprland are the
/// user's real ones: nothing there is touched.
fn test_profile() -> bool {
    std::env::var_os("JOMA_PROFILE").is_some()
}

/// Write `text` to `path` unless it already holds it. True when written.
fn write_if_changed(path: &Path, text: &[u8]) -> Result<bool> {
    if std::fs::read(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

// ---- Launcher ---------------------------------------------------------------

const DESKTOP: &str = include_str!("../../../assets/joma.desktop");
const ICONS: &[(&str, &[u8])] = &[
    ("scalable/apps/joma.svg", include_bytes!("../../../assets/icon/joma.svg")),
    ("32x32/apps/joma.png", include_bytes!("../../../assets/icon/joma-32.png")),
    ("48x48/apps/joma.png", include_bytes!("../../../assets/icon/joma-48.png")),
    ("64x64/apps/joma.png", include_bytes!("../../../assets/icon/joma-64.png")),
    ("128x128/apps/joma.png", include_bytes!("../../../assets/icon/joma-128.png")),
    ("256x256/apps/joma.png", include_bytes!("../../../assets/icon/joma-256.png")),
    ("512x512/apps/joma.png", include_bytes!("../../../assets/icon/joma-512.png")),
];

/// The launcher entry and icons, so the app launcher finds Joma.
pub fn ensure_launcher() -> Result<()> {
    let data = xdg("XDG_DATA_HOME", ".local/share");
    let desktop = DESKTOP.replace("@BIN@", &exe().to_string_lossy());
    let mut changed = write_if_changed(&data.join("applications/joma.desktop"), desktop.as_bytes())?;
    for (path, bytes) in ICONS {
        changed |= write_if_changed(&data.join("icons/hicolor").join(path), bytes)?;
    }
    if changed {
        let _ = Command::new("gtk-update-icon-cache").arg("-q").arg(data.join("icons/hicolor")).stderr(Stdio::null()).status();
    }
    Ok(())
}

// ---- Start at login -----------------------------------------------------------

const UNIT: &str = "joma.service";

fn unit_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("systemd/user").join(UNIT)
}

fn unit_text() -> String {
    format!(
        "# Written by Joma (Start at login).
[Unit]
Description=Joma, a Jami client
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart={} --hidden
Restart=on-failure

[Install]
WantedBy=graphical-session.target
",
        exe().display()
    )
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl").arg("--user").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

pub fn starts_at_login() -> bool {
    !test_profile() && unit_path().exists() && systemctl(&["is-enabled", "--quiet", UNIT])
}

pub fn set_start_at_login(on: bool) -> Result<()> {
    if test_profile() {
        bail!("not in a test profile");
    }
    if on {
        write_if_changed(&unit_path(), unit_text().as_bytes())?;
        systemctl(&["daemon-reload"]);
        if !systemctl(&["enable", UNIT]) {
            bail!("systemctl --user enable {UNIT} failed");
        }
    } else {
        systemctl(&["disable", UNIT]);
    }
    Ok(())
}

// ---- The shortcut --------------------------------------------------------------

const BIND_START: &str = "-- joma:";
const BIND_END: &str = "-- joma end";

fn bindings_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("hypr/bindings.lua")
}

fn bind_block() -> String {
    format!(
        "{BIND_START} open and close Joma (written by Joma)\no.bind(\"SUPER + ALT + M\", \"Joma\", \"{} toggle\")\n{BIND_END}\n",
        exe().display()
    )
}

/// The file without Joma's block, and whether it had one.
fn without_block(text: &str) -> (String, bool) {
    let mut out = String::new();
    let mut inside = false;
    let mut found = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with(BIND_START) {
            inside = true;
            found = true;
        } else if inside && t == BIND_END {
            inside = false;
        } else if !inside {
            out.push_str(line);
        }
    }
    (out, found)
}

/// Super+Alt+M runs this Joma.
pub fn has_shortcut() -> bool {
    std::fs::read_to_string(bindings_path()).is_ok_and(|t| t.contains(&bind_block()))
}

/// Can the shortcut be added: Omarchy's Lua bindings file, and the keys free.
pub fn shortcut_possible() -> bool {
    let Ok(text) = std::fs::read_to_string(bindings_path()) else { return false };
    !without_block(&text).0.contains("\"SUPER + ALT + M\"")
}

pub fn add_shortcut() -> Result<()> {
    let path = bindings_path();
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let (rest, _) = without_block(&text);
    if rest.contains("\"SUPER + ALT + M\"") {
        bail!("Super+Alt+M is already bound in {}", path.display());
    }
    let mut new = rest;
    if !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(&bind_block());
    std::fs::write(path.with_extension("lua.joma-backup"), &text)?;
    std::fs::write(&path, new)?;
    reload_hyprland(&path, &text)
}

/// Hyprland reloads on save; check it took the file, else put the old one back.
fn reload_hyprland(path: &Path, old: &str) -> Result<()> {
    if test_profile() {
        return Ok(());
    }
    let _ = Command::new("hyprctl").arg("reload").stdout(Stdio::null()).status();
    let errors = Command::new("hyprctl").arg("configerrors").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    if !errors.is_empty() && !errors.starts_with("no errors") {
        std::fs::write(path, old)?;
        let _ = Command::new("hyprctl").arg("reload").stdout(Stdio::null()).status();
        bail!("Hyprland did not take the binding: {errors}");
    }
    Ok(())
}

// ---- The tray icon in the Omarchy bar -----------------------------------------

/// The tray widget's id in the bar, and the id our icon has there.
const TRAY_WIDGET: &str = "omarchy.tray";
const TRAY_ID: &str = "joma";

fn shell_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("omarchy/shell.json")
}

/// The first object `{ "id": "omarchy.tray", … }` in the shell's config.
fn tray_entry(v: &mut serde_json::Value) -> Option<&mut serde_json::Map<String, serde_json::Value>> {
    match v {
        serde_json::Value::Object(m) => {
            if m.get("id").and_then(|i| i.as_str()) == Some(TRAY_WIDGET) {
                return Some(m);
            }
            m.values_mut().find_map(tray_entry)
        }
        serde_json::Value::Array(a) => a.iter_mut().find_map(tray_entry),
        _ => None,
    }
}

fn with_tray_entry<R>(v: &mut serde_json::Value, f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) -> R) -> Option<R> {
    tray_entry(v).map(f)
}

fn read_shell() -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(shell_path()).ok()?).ok()
}

/// The Omarchy bar has a tray widget (else there is nothing to pin into).
pub fn bar_has_tray() -> bool {
    read_shell().is_some_and(|mut v| with_tray_entry(&mut v, |_| ()).is_some())
}

pub fn pinned_in_bar() -> bool {
    read_shell().is_some_and(|mut v| {
        with_tray_entry(&mut v, |m| m.get("pinned").and_then(|p| p.as_array()).is_some_and(|p| p.iter().any(|i| i == TRAY_ID))).unwrap_or(false)
    })
}

/// Pin the icon in the bar's tray (else it waits in the tray's drawer). The shell
/// reloads its config on save.
pub fn pin_in_bar() -> Result<()> {
    let path = shell_path();
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut v: serde_json::Value = serde_json::from_str(&text).context("shell.json is not plain JSON")?;
    with_tray_entry(&mut v, |m| {
        let pinned = m.entry("pinned").or_insert_with(|| serde_json::json!([]));
        if let Some(a) = pinned.as_array_mut() {
            if !a.iter().any(|i| i == TRAY_ID) {
                a.push(TRAY_ID.into());
            }
        }
        if let Some(h) = m.get_mut("hidden").and_then(|h| h.as_array_mut()) {
            h.retain(|i| i != TRAY_ID);
        }
    })
    .context("the bar has no tray widget")?;
    std::fs::write(path.with_extension("json.joma-backup"), &text)?;
    std::fs::write(&path, serde_json::to_string_pretty(&v)? + "\n")?;
    Ok(())
}

/// Pieces written by an earlier Joma that name another binary: point them at this one.
pub fn refresh() {
    if let Err(e) = ensure_launcher() {
        tracing::warn!("launcher: {e:#}");
    }
    if !test_profile() && unit_path().exists() && std::fs::read_to_string(unit_path()).is_ok_and(|t| t != unit_text()) {
        let _ = write_if_changed(&unit_path(), unit_text().as_bytes());
        systemctl(&["daemon-reload"]);
    }
    if let Ok(text) = std::fs::read_to_string(bindings_path()) {
        let (rest, had) = without_block(&text);
        if had && !text.contains(&bind_block()) && !rest.contains("\"SUPER + ALT + M\"") {
            let mut new = rest;
            if !new.ends_with('\n') {
                new.push('\n');
            }
            new.push_str(&bind_block());
            if std::fs::write(bindings_path(), &new).is_ok() {
                if let Err(e) = reload_hyprland(&bindings_path(), &text) {
                    tracing::warn!("shortcut: {e:#}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinning and the shortcut on a made-up HOME (a test profile, so Hyprland is left
    /// alone).
    #[test]
    fn pins_and_binds_in_the_config_files() {
        let dir = std::env::temp_dir().join(format!("joma-setup-{}", std::process::id()));
        let cfg = dir.join("config");
        std::fs::create_dir_all(cfg.join("omarchy")).unwrap();
        std::fs::create_dir_all(cfg.join("hypr")).unwrap();
        std::env::set_var("JOMA_PROFILE", &dir);
        std::env::set_var("XDG_CONFIG_HOME", &cfg);
        std::fs::write(cfg.join("omarchy/shell.json"), r#"{"bar":{"right":[{"id":"omarchy.tray","pinned":["x"],"hidden":["joma"]},{"id":"z","a":1}]}}"#).unwrap();
        std::fs::write(cfg.join("hypr/bindings.lua"), "o.bind(\"SUPER + W\", \"Close\", \"x\")\n-- joma: old\no.bind(\"SUPER + ALT + M\", \"old\", \"y\")\n-- joma end\n").unwrap();
        assert!(bar_has_tray() && !pinned_in_bar());
        pin_in_bar().unwrap();
        assert!(pinned_in_bar());
        let shell = std::fs::read_to_string(cfg.join("omarchy/shell.json")).unwrap();
        assert!(shell.find("omarchy.tray").unwrap() < shell.find("\"z\"").unwrap(), "key order kept: {shell}");
        assert!(!shell.contains("\"hidden\": [\n            \"joma\""));
        assert!(shortcut_possible() && !has_shortcut());
        add_shortcut().unwrap();
        assert!(has_shortcut());
        let binds = std::fs::read_to_string(cfg.join("hypr/bindings.lua")).unwrap();
        assert_eq!(binds.matches("SUPER + ALT + M").count(), 1);
        assert!(binds.starts_with("o.bind(\"SUPER + W\""));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_binding_block_is_replaced_not_repeated() {
        let old = "a\n-- joma: quick messages (development; not in omatwin)\no.bind(\"SUPER + ALT + M\", \"x\", \"y\")\n-- joma end\nb\n";
        let (rest, found) = without_block(old);
        assert!(found);
        assert_eq!(rest, "a\nb\n");
        assert!(!rest.contains("SUPER + ALT + M"));
    }

    #[test]
    fn finds_the_tray_entry_anywhere() {
        let mut v = serde_json::json!({"bar": {"layout": {"right": [{"id": "omarchy.clock"}, {"id": "omarchy.tray", "pinned": ["Signal_status_icon_1"]}]}}});
        assert_eq!(with_tray_entry(&mut v, |m| m["pinned"].as_array().unwrap().len()), Some(1));
        let mut none = serde_json::json!({"bar": {}});
        assert_eq!(with_tray_entry(&mut none, |_| ()), None);
    }
}
