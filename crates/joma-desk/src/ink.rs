//! Drawing on a shared screen: the viewer's laser pointer and marker strokes, shown on
//! the viewer's stage and, on the sharer's side, in a click-through overlay over the
//! shared screen (a layer-shell surface of our own: GPUI has none).
//!
//! Positions are fractions of the shared picture (0..1), so they mean the same place on
//! both sides whatever the sizes. The messages travel as JSON lines over a second
//! connection through the share's tunnel (token, then `I` instead of a newline).

use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{delegate_noop, Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1};

/// One drawing message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Ink {
    /// The viewer's pointer, as a laser dot.
    Laser { x: f32, y: f32 },
    LaserOff,
    /// Points added to stroke `id` (created by its first message); colour 0xRRGGBB, width
    /// in screen pixels.
    Stroke { id: u32, color: u32, width: f32, pts: Vec<(f32, f32)> },
    /// Everything drawn goes.
    Clear,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub id: u32,
    pub color: u32,
    pub width: f32,
    pub pts: Vec<(f32, f32)>,
}

/// What is drawn: the strokes, and the laser while it is on.
#[derive(Clone, Debug, Default)]
pub struct Canvas {
    pub laser: Option<(f32, f32, Instant)>,
    pub strokes: Vec<Stroke>,
}

impl Canvas {
    pub fn apply(&mut self, ink: &Ink) {
        match ink {
            Ink::Laser { x, y } => self.laser = Some((*x, *y, Instant::now())),
            Ink::LaserOff => self.laser = None,
            Ink::Stroke { id, color, width, pts } => match self.strokes.iter_mut().find(|s| s.id == *id) {
                Some(s) => s.pts.extend_from_slice(pts),
                None => self.strokes.push(Stroke { id: *id, color: *color, width: *width, pts: pts.clone() }),
            },
            Ink::Clear => {
                self.strokes.clear();
                self.laser = None;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.laser.is_none() && self.strokes.is_empty()
    }
}

/// The overlay's pixels: premultiplied ARGB, `w` × `h`.
struct Pixels {
    w: usize,
    h: usize,
    px: Vec<u32>,
}

impl Pixels {
    fn new(w: usize, h: usize) -> Self {
        Pixels { w, h, px: vec![0; w * h] }
    }

    fn clear(&mut self, r: Rect) {
        for y in r.y0..r.y1 {
            self.px[y * self.w + r.x0..y * self.w + r.x1].fill(0);
        }
    }

    /// A filled disc, clipped to `clip`.
    fn disc(&mut self, cx: f32, cy: f32, rad: f32, argb: u32, clip: Rect) {
        let x0 = ((cx - rad).floor().max(clip.x0 as f32)) as usize;
        let x1 = ((cx + rad).ceil().min(clip.x1 as f32)).max(0.) as usize;
        let y0 = ((cy - rad).floor().max(clip.y0 as f32)) as usize;
        let y1 = ((cy + rad).ceil().min(clip.y1 as f32)).max(0.) as usize;
        let r2 = rad * rad;
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r2 {
                    let p = &mut self.px[y * self.w + x];
                    *p = over(argb, *p);
                }
            }
        }
    }

    fn segment(&mut self, a: (f32, f32), b: (f32, f32), rad: f32, argb: u32, clip: Rect) {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let steps = (len / (rad * 0.5).max(0.5)).ceil().max(1.) as usize;
        for i in 0..=steps {
            let k = i as f32 / steps as f32;
            self.disc(a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k, rad, argb, clip);
        }
    }
}

/// `src` over `dst`, both premultiplied ARGB.
fn over(src: u32, dst: u32) -> u32 {
    let sa = src >> 24;
    if sa == 255 {
        return src;
    }
    let inv = 255 - sa;
    let ch = |s: u32, shift: u32| ((s >> shift) & 255) + (((dst >> shift) & 255) * inv + 127) / 255;
    (ch(src, 24) << 24) | (ch(src, 16) << 16) | (ch(src, 8) << 8) | ch(src, 0)
}

fn premul(rgb: u32, alpha: u32) -> u32 {
    let c = |shift: u32| (((rgb >> shift) & 255) * alpha + 127) / 255;
    (alpha << 24) | (c(16) << 16) | (c(8) << 8) | c(0)
}

/// A pixel rectangle, end-exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }

    fn around(x: f32, y: f32, pad: f32, w: usize, h: usize) -> Rect {
        let c = |v: f32, max: usize| v.clamp(0., max as f32) as usize;
        Rect { x0: c(x - pad, w), y0: c(y - pad, h), x1: c(x + pad + 1., w), y1: c(y + pad + 1., h) }
    }
}

/// Where the shared picture lies on the output, as fractions of it: x, y, w, h.
pub type Place = (f32, f32, f32, f32);

const LASER_RADIUS: f32 = 7.;
const LASER_GLOW: f32 = 15.;

/// Paint `canvas` into `p`, only inside `clip`.
fn render(p: &mut Pixels, canvas: &Canvas, place: Place, clip: Rect) {
    p.clear(clip);
    let at = |(x, y): (f32, f32)| (place.0 + x * place.2, place.1 + y * place.3);
    for s in &canvas.strokes {
        let argb = premul(s.color, 255);
        let rad = (s.width / 2.).max(1.);
        let pts: Vec<(f32, f32)> = s.pts.iter().map(|&q| at(q)).collect();
        if pts.len() == 1 {
            p.disc(pts[0].0, pts[0].1, rad, argb, clip);
        }
        for w in pts.windows(2) {
            p.segment(w[0], w[1], rad, argb, clip);
        }
    }
    if let Some((x, y, _)) = canvas.laser {
        let (x, y) = at((x, y));
        p.disc(x, y, LASER_GLOW, premul(0xff3355, 90), clip);
        p.disc(x, y, LASER_RADIUS, premul(0xff2244, 255), clip);
        p.disc(x, y, LASER_RADIUS * 0.4, premul(0xffffff, 230), clip);
    }
}

/// The area `canvas` covers (for redrawing what changed).
fn extent(canvas: &Canvas, place: Place, w: usize, h: usize) -> Option<Rect> {
    let at = |(x, y): (f32, f32)| (place.0 + x * place.2, place.1 + y * place.3);
    let mut r: Option<Rect> = None;
    let mut add = |x: f32, y: f32, pad: f32| {
        let n = Rect::around(x, y, pad, w, h);
        r = Some(r.map_or(n, |o| o.union(n)));
    };
    for s in &canvas.strokes {
        for &q in &s.pts {
            let (x, y) = at(q);
            add(x, y, s.width / 2. + 2.);
        }
    }
    if let Some((x, y, _)) = canvas.laser {
        let (x, y) = at((x, y));
        add(x, y, LASER_GLOW + 2.);
    }
    r
}

enum Cmd {
    Draw(Canvas, Place),
    Stop,
}

/// The sharer's overlay on one output. Dropping it removes it.
pub struct Overlay {
    tx: calloop::channel::Sender<Cmd>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Overlay {
    pub fn start(output: &str) -> Result<Overlay> {
        let (tx, rx) = calloop::channel::channel::<Cmd>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let output = output.to_owned();
        let thread = std::thread::Builder::new().name("joma-ink".into()).spawn(move || {
            if let Err(e) = run(&output, rx, &ready_tx) {
                tracing::warn!(error = %format!("{e:#}"), "ink overlay ended");
                let _ = ready_tx.send(Err(e));
            }
        })?;
        ready_rx.recv().context("the overlay thread stopped")??;
        Ok(Overlay { tx, thread: Some(thread) })
    }

    /// Show `canvas` with the shared picture at `place` (fractions of the output).
    pub fn draw(&self, canvas: &Canvas, place: Place) {
        let _ = self.tx.send(Cmd::Draw(canvas.clone(), place));
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct State {
    outputs: hypr_wl::Outputs,
    surface: Option<WlSurface>,
    layer: Option<ZwlrLayerSurfaceV1>,
    shm: WlShm,
    qh: QueueHandle<State>,
    file: Option<File>,
    buffer: Option<WlBuffer>,
    pixels: Option<Pixels>,
    busy: bool,
    /// What to show, and the area to redraw once the buffer is free.
    want: (Canvas, Place),
    shown: (Canvas, Place),
    dirty: Option<Rect>,
    /// The last place asked for, as fractions (for a new buffer size).
    frac: Place,
    stopped: bool,
}

impl State {
    /// Mark what changed between what is shown and what is wanted, and redraw it.
    fn update(&mut self, canvas: Canvas, place: Place) {
        // From fractions of the output to the overlay's pixels.
        let place = match &self.pixels {
            Some(p) => (place.0 * p.w as f32, place.1 * p.h as f32, place.2 * p.w as f32, place.3 * p.h as f32),
            None => place,
        };
        self.update_px(canvas, place);
    }

    fn update_px(&mut self, canvas: Canvas, place: Place) {
        let Some(p) = &self.pixels else {
            self.want = (canvas, place);
            return;
        };
        let (w, h) = (p.w, p.h);
        let old = extent(&self.shown.0, self.shown.1, w, h);
        let new = extent(&canvas, place, w, h);
        let changed = match (old, new) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        };
        if let Some(c) = changed {
            self.dirty = Some(self.dirty.map_or(c, |d| d.union(c)));
        }
        self.want = (canvas, place);
        self.flush();
    }

    fn flush(&mut self) {
        if self.busy {
            return;
        }
        let (Some(dirty), Some(p), Some(file), Some(buffer), Some(surface)) = (self.dirty.take(), self.pixels.as_mut(), &self.file, &self.buffer, &self.surface) else { return };
        render(p, &self.want.0, self.want.1, dirty);
        let stride = p.w * 4;
        for y in dirty.y0..dirty.y1 {
            let row = &p.px[y * p.w + dirty.x0..y * p.w + dirty.x1];
            let bytes: Vec<u8> = row.iter().flat_map(|v| v.to_le_bytes()).collect();
            let _ = file.write_at(&bytes, (y * stride + dirty.x0 * 4) as u64);
        }
        surface.attach(Some(buffer), 0, 0);
        surface.damage_buffer(dirty.x0 as i32, dirty.y0 as i32, (dirty.x1 - dirty.x0) as i32, (dirty.y1 - dirty.y0) as i32);
        surface.commit();
        self.busy = true;
        self.shown = self.want.clone();
    }

    fn allocate(&mut self, w: usize, h: usize) -> Result<()> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("joma-ink-{}-{}", std::process::id(), w * h));
        let file = File::options().read(true).write(true).create(true).truncate(true).open(&path)?;
        let _ = std::fs::remove_file(&path);
        let size = w * h * 4;
        file.set_len(size as u64)?;
        let pool: WlShmPool = self.shm.create_pool(file.as_fd(), size as i32, &self.qh, ());
        let buffer = pool.create_buffer(0, w as i32, h as i32, (w * 4) as i32, wl_shm::Format::Argb8888, &self.qh, ());
        pool.destroy();
        if let Some(b) = self.buffer.take() {
            b.destroy();
        }
        self.file = Some(file);
        self.buffer = Some(buffer);
        self.pixels = Some(Pixels::new(w, h));
        self.busy = false;
        self.shown = (Canvas::default(), (0., 0., 0., 0.));
        self.dirty = Some(Rect { x0: 0, y0: 0, x1: w, y1: h });
        Ok(())
    }
}

impl hypr_wl::LoopState for State {
    type Cmd = Cmd;

    fn on_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Draw(c, place) => {
                self.frac = place;
                self.update(c, place)
            }
            Cmd::Stop => self.stopped = true,
        }
    }

    fn stop(&mut self) {
        self.stopped = true;
    }

    fn stop_requested(&self) -> bool {
        self.stopped
    }
}

fn run(output: &str, rx: calloop::channel::Channel<Cmd>, ready: &std::sync::mpsc::Sender<Result<()>>) -> Result<()> {
    let (conn, globals, mut queue) = hypr_wl::init::<State>(&hypr_wl::Target::default())?;
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 4..=6, ()).context("no wl_compositor")?;
    let shm: WlShm = globals.bind(&qh, 1..=1, ()).context("no wl_shm")?;
    let shell: ZwlrLayerShellV1 = globals.bind(&qh, 1..=4, ()).context("no layer shell")?;
    let outputs = hypr_wl::Outputs::bind(&globals, &qh)?;
    let mut state = State {
        outputs,
        surface: None,
        layer: None,
        shm,
        qh: qh.clone(),
        file: None,
        buffer: None,
        pixels: None,
        busy: false,
        want: (Canvas::default(), (0., 0., 0., 0.)),
        shown: (Canvas::default(), (0., 0., 0., 0.)),
        dirty: None,
        frac: (0., 0., 1., 1.),
        stopped: false,
    };
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;
    let (wl_out, _) = state.outputs.find(output)?;
    let surface = compositor.create_surface(&qh, ());
    // An empty input region: clicks go through to the windows below.
    let region: WlRegion = compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let layer = shell.get_layer_surface(&surface, Some(&wl_out), Layer::Overlay, "joma-ink".into(), &qh, ());
    layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    surface.commit();
    state.surface = Some(surface);
    state.layer = Some(layer);
    queue.roundtrip(&mut state)?;
    let _ = ready.send(Ok(()));
    hypr_wl::run_loop(conn, queue, rx, &mut state)?;
    if let Some(b) = state.buffer.take() {
        b.destroy();
    }
    if let Some(l) = state.layer.take() {
        l.destroy();
    }
    if let Some(s) = state.surface.take() {
        s.destroy();
    }
    Ok(())
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(s: &mut Self, l: &ZwlrLayerSurfaceV1, e: zwlr_layer_surface_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match e {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                l.ack_configure(serial);
                let size = (width as usize, height as usize);
                if size.0 > 0 && size.1 > 0 && s.pixels.as_ref().is_none_or(|p| (p.w, p.h) != size) {
                    if let Err(e) = s.allocate(size.0, size.1) {
                        tracing::warn!(error = %e, "ink overlay buffer");
                        return;
                    }
                }
                let c = s.want.0.clone();
                let f = s.frac;
                s.update(c, f);
                if s.dirty.is_some() || s.shown.0.is_empty() {
                    s.dirty = Some(s.dirty.unwrap_or(Rect { x0: 0, y0: 0, x1: size.0, y1: size.1 }));
                    s.flush();
                }
            }
            zwlr_layer_surface_v1::Event::Closed => s.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(s: &mut Self, _: &WlBuffer, e: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = e {
            s.busy = false;
            s.flush();
        }
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(s: &mut Self, o: &WlOutput, e: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        s.outputs.handle(o, e);
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: wayland_client::protocol::wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore WlRegion);
delegate_noop!(State: ignore ZwlrLayerShellV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_applies_and_renders() {
        let mut c = Canvas::default();
        c.apply(&Ink::Stroke { id: 1, color: 0xff0000, width: 4., pts: vec![(0.1, 0.1)] });
        c.apply(&Ink::Stroke { id: 1, color: 0xff0000, width: 4., pts: vec![(0.9, 0.1)] });
        c.apply(&Ink::Laser { x: 0.5, y: 0.5 });
        assert_eq!(c.strokes[0].pts.len(), 2);
        let mut p = Pixels::new(100, 100);
        let all = Rect { x0: 0, y0: 0, x1: 100, y1: 100 };
        render(&mut p, &c, (0., 0., 100., 100.), all);
        assert_eq!(p.px[10 * 100 + 50] >> 24, 255, "the stroke is drawn");
        assert_eq!(p.px[50 * 100 + 50] >> 24, 255, "the laser is drawn");
        assert_eq!(p.px[90 * 100 + 90], 0, "the rest is clear");
        let e = extent(&c, (0., 0., 100., 100.), 100, 100).unwrap();
        assert!(e.x0 <= 8 && e.x1 >= 92);
        c.apply(&Ink::Clear);
        assert!(c.is_empty());
        let json = serde_json::to_string(&Ink::Laser { x: 0.5, y: 0.25 }).unwrap();
        assert_eq!(json, r#"{"t":"laser","x":0.5,"y":0.25}"#);
    }
}
