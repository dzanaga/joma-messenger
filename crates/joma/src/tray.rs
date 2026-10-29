//! Joma's icon in the system tray (StatusNotifierItem, as Signal and others use; the
//! Omarchy bar shows it with its tray widget). The icon is the logo's j in the theme's
//! foreground and its dot in the status colour: lavender online, green with unread
//! messages, red offline, grey invisible. A click opens or closes the window; the menu
//! has the rest.

use futures::channel::mpsc::UnboundedSender;
use ksni::blocking::{Handle, TrayMethods};

use crate::app::AppMsg;

/// What the tray asks of the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Toggle,
    Open,
    Invisible,
    StartAtLogin,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Online,
    Unread,
    Offline,
    Invisible,
}

impl Status {
    pub fn rgb(self) -> u32 {
        match self {
            Status::Online => 0xb193f7,
            Status::Unread => 0x39ff88,
            Status::Offline => 0xf7768e,
            Status::Invisible => 0x7a7f8a,
        }
    }
}

/// What the tray shows; the app sets it, the tray renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    pub status: Status,
    pub fg: u32,
    pub unread: u32,
    pub invisible: bool,
    pub start_at_login: bool,
}

pub struct JomaTray {
    tx: UnboundedSender<AppMsg>,
    look: Look,
}

impl JomaTray {
    fn send(&self, a: TrayAction) {
        let _ = self.tx.unbounded_send(AppMsg::Tray(a));
    }
}

impl ksni::Tray for JomaTray {
    fn id(&self) -> String {
        "joma".into()
    }

    fn title(&self) -> String {
        "Joma".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let title = match self.look.status {
            Status::Unread => format!("Joma · {} unread", self.look.unread),
            Status::Offline => "Joma · offline".into(),
            Status::Invisible => "Joma · invisible".into(),
            Status::Online => "Joma".into(),
        };
        ksni::ToolTip { title, ..Default::default() }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [16, 22, 24, 32, 48, 64].iter().map(|&n| icon(n, self.look.fg, self.look.status.rgb())).collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayAction::Toggle);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem { label: "Open Joma".into(), activate: Box::new(|t: &mut Self| t.send(TrayAction::Open)), ..Default::default() }.into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Invisible".into(),
                checked: self.look.invisible,
                activate: Box::new(|t: &mut Self| t.send(TrayAction::Invisible)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Start at login".into(),
                checked: self.look.start_at_login,
                activate: Box::new(|t: &mut Self| t.send(TrayAction::StartAtLogin)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem { label: "Quit Joma".into(), activate: Box::new(|t: &mut Self| t.send(TrayAction::Quit)), ..Default::default() }.into(),
        ]
    }
}

pub struct Tray {
    handle: Option<Handle<JomaTray>>,
    look: Option<Look>,
}

impl Tray {
    /// Register the icon; without a tray host it waits for one (the bar may start later).
    /// `JOMA_TRAY=0` leaves it out (tests on the user's session bus, whose tray is
    /// the real bar's).
    pub fn start(tx: UnboundedSender<AppMsg>, look: Look) -> Tray {
        if std::env::var("JOMA_TRAY").is_ok_and(|v| v == "0") {
            return Tray { handle: None, look: Some(look) };
        }
        let t = JomaTray { tx, look: look.clone() };
        let handle = match t.assume_sni_available(true).spawn() {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::info!("no tray icon: {e}");
                None
            }
        };
        Tray { handle, look: Some(look) }
    }


    pub fn set(&mut self, look: Look) {
        if self.look.as_ref() == Some(&look) {
            return;
        }
        if let Some(h) = &self.handle {
            let l = look.clone();
            h.update(move |t| t.look = l);
        }
        self.look = Some(look);
    }
}

/// The logo's shapes in the source PNG's pixels (as scripts/icon.py and the old bar
/// glyph): its box, the dot, and the j as rectangles.
const BOX: [i32; 4] = [377, 297, 877, 1021];
const DOT: [i32; 4] = [701, 297, 877, 460];
const J: [[i32; 4]; 3] = [[701, 491, 877, 1021], [377, 763, 556, 1021], [556, 889, 701, 1021]];

/// An `n`×`n` ARGB32 icon: the logo as tall as the icon, centred, edges on whole
/// pixels, and at least a pixel between the dot and the j.
pub fn icon(n: i32, fg: u32, dot: u32) -> ksni::Icon {
    let scale = n as f32 / (BOX[3] - BOX[1]) as f32;
    let width = ((BOX[2] - BOX[0]) as f32 * scale).round() as i32;
    let ox = (n - width) / 2;
    let sx = |v: i32| ox + ((v - BOX[0]) as f32 * scale).round() as i32;
    let sy = |v: i32| ((v - BOX[1]) as f32 * scale).round() as i32;
    let mut data = vec![0u8; (n * n * 4) as usize];
    let mut fill = |r: [i32; 4], bottom: i32, rgb: u32| {
        for y in sy(r[1]).max(0)..bottom.min(n) {
            for x in sx(r[0]).max(0)..sx(r[2]).min(n) {
                let i = ((y * n + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[0xff, (rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
            }
        }
    };
    for r in J {
        fill(r, sy(r[3]), fg);
    }
    fill(DOT, sy(DOT[3]).min(sy(J[0][1]) - 1), dot);
    ksni::Icon { width: n, height: n, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_keeps_a_gap_under_the_dot() {
        for n in [16, 22, 24, 32, 64] {
            let i = icon(n, 0xffffff, 0xff0000);
            assert_eq!(i.data.len(), (n * n * 4) as usize);
            // Down the middle of the stem: the dot, a gap, then the j.
            let scale = n as f32 / (BOX[3] - BOX[1]) as f32;
            let ox = (n - ((BOX[2] - BOX[0]) as f32 * scale).round() as i32) / 2;
            let x = ox + ((789 - BOX[0]) as f32 * scale) as i32;
            let column: String = (0..n)
                .map(|y| match &i.data[((y * n + x) * 4) as usize..][..4] {
                    [0, ..] => '.',
                    [_, 0xff, 0, 0] => 'd',
                    _ => 'j',
                })
                .collect();
            assert!(column.contains("d.") && column.contains(".j") && !column.contains("dj"), "n={n}: {column}");
        }
    }
}
