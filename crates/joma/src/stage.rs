//! How the stage shows a peer's desktop (or video): fitted, filling the stage, at one
//! picture pixel per screen pixel, or at a free zoom; panned by dragging or scrolling, and
//! optionally following the peer's pointer. A wide screen (an ultrawide) fitted into a
//! window is small; filling the height and panning sideways reads better.

use gpui::{point, px, size, Bounds, Pixels, Point};

/// How far one can zoom in: picture pixels shown this many screen pixels wide.
const MAX_PHYSICAL: f32 = 8.0;
/// With the pointer followed, it stays this far (a share of the stage) from the edges.
const FOLLOW_MARGIN: f32 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// The whole picture, letterboxed.
    Fit,
    /// The stage filled, the rest reached by panning.
    Fill,
    /// One picture pixel per screen pixel.
    Actual,
    /// Logical pixels per picture pixel.
    Free(f32),
}

#[derive(Clone, Debug)]
pub struct StageView {
    pub zoom: Zoom,
    /// The picture point in the middle of the stage, 0..1 on each axis.
    pub center: (f32, f32),
    /// Keep the peer's pointer in view when zoomed in.
    pub follow: bool,
    /// What was painted last: the stage, the picture's size, the window's scale.
    pub bounds: Bounds<Pixels>,
    pub picture: (f32, f32),
    pub scale_factor: f32,
    /// A drag in progress: where it started and the centre then.
    pub drag: Option<(Point<Pixels>, (f32, f32))>,
}

impl Default for StageView {
    fn default() -> Self {
        StageView {
            zoom: Zoom::Fit,
            center: (0.5, 0.5),
            follow: true,
            bounds: Bounds::default(),
            picture: (0., 0.),
            scale_factor: 1.,
            drag: None,
        }
    }
}

fn wh(b: &Bounds<Pixels>) -> (f32, f32) {
    (f32::from(b.size.width), f32::from(b.size.height))
}

impl StageView {
    /// Back to the whole picture, centred.
    pub fn reset(&mut self) {
        let follow = self.follow;
        *self = StageView { follow, bounds: self.bounds, picture: self.picture, scale_factor: self.scale_factor, ..Default::default() };
    }

    /// Note what is painted now.
    pub fn layout(&mut self, bounds: Bounds<Pixels>, picture: (f32, f32), scale_factor: f32) {
        self.bounds = bounds;
        self.picture = (picture.0.max(1.), picture.1.max(1.));
        self.scale_factor = scale_factor.max(0.1);
    }

    fn fit_scale(&self) -> f32 {
        let (bw, bh) = wh(&self.bounds);
        (bw / self.picture.0).min(bh / self.picture.1)
    }

    fn fill_scale(&self) -> f32 {
        let (bw, bh) = wh(&self.bounds);
        (bw / self.picture.0).max(bh / self.picture.1)
    }

    fn max_scale(&self) -> f32 {
        (MAX_PHYSICAL / self.scale_factor).max(self.fit_scale())
    }

    /// Logical pixels per picture pixel.
    pub fn scale(&self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fit_scale(),
            Zoom::Fill => self.fill_scale(),
            Zoom::Actual => 1. / self.scale_factor,
            Zoom::Free(s) => s,
        }
    }

    /// The zoom as a percentage of the picture's own pixels (100: one per screen pixel).
    pub fn percent(&self) -> u32 {
        (self.scale() * self.scale_factor * 100.).round() as u32
    }

    /// Zoomed past the stage: there is something to pan to.
    pub fn zoomed(&self) -> bool {
        let (bw, bh) = wh(&self.bounds);
        let s = self.scale();
        self.picture.0 * s > bw + 0.5 || self.picture.1 * s > bh + 0.5
    }

    /// The centre kept so the picture never leaves a gap it could cover.
    fn clamp(&mut self) {
        let (bw, bh) = wh(&self.bounds);
        let s = self.scale();
        let clamp = |c: f32, shown: f32, room: f32| {
            if shown <= room {
                0.5
            } else {
                let half = room / 2. / shown;
                c.clamp(half, 1. - half)
            }
        };
        self.center = (clamp(self.center.0, self.picture.0 * s, bw), clamp(self.center.1, self.picture.1 * s, bh));
    }

    /// Where the picture goes now.
    pub fn rect(&mut self) -> Bounds<Pixels> {
        self.clamp();
        let s = self.scale();
        let (w, h) = (self.picture.0 * s, self.picture.1 * s);
        let (bw, bh) = wh(&self.bounds);
        let x = f32::from(self.bounds.origin.x) + bw / 2. - self.center.0 * w;
        let y = f32::from(self.bounds.origin.y) + bh / 2. - self.center.1 * h;
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    pub fn set(&mut self, zoom: Zoom) {
        self.zoom = zoom;
        self.clamp();
    }

    /// Zoom by `factor`, keeping the picture point under `at` where it is.
    pub fn zoom_at(&mut self, at: Point<Pixels>, factor: f32) {
        let r = self.rect();
        let (rw, rh) = (f32::from(r.size.width).max(1.), f32::from(r.size.height).max(1.));
        let u = (f32::from(at.x) - f32::from(r.origin.x)) / rw;
        let v = (f32::from(at.y) - f32::from(r.origin.y)) / rh;
        let fit = self.fit_scale();
        let s = (self.scale() * factor).clamp(fit, self.max_scale());
        let (w, h) = (self.picture.0 * s, self.picture.1 * s);
        let (bw, bh) = wh(&self.bounds);
        // The new origin keeps (u, v) under the pointer; the centre follows from it.
        let ox = f32::from(at.x) - u * w;
        let oy = f32::from(at.y) - v * h;
        self.center = (
            (f32::from(self.bounds.origin.x) + bw / 2. - ox) / w,
            (f32::from(self.bounds.origin.y) + bh / 2. - oy) / h,
        );
        self.zoom = if (s - fit).abs() < fit * 0.01 { Zoom::Fit } else { Zoom::Free(s) };
        self.clamp();
    }

    /// Zoom by `factor` around the middle of the stage.
    pub fn zoom_by(&mut self, factor: f32) {
        let c = self.bounds.center();
        self.zoom_at(c, factor);
    }

    /// Move the picture by logical pixels (positive: it moves right and down).
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let s = self.scale();
        self.center.0 -= dx / (self.picture.0 * s);
        self.center.1 -= dy / (self.picture.1 * s);
        self.clamp();
    }

    pub fn drag_to(&mut self, at: Point<Pixels>) {
        if let Some((start, center)) = self.drag {
            self.center = center;
            self.pan(f32::from(at.x - start.x), f32::from(at.y - start.y));
        }
    }

    /// Where `at` falls on the picture, as fractions (None: outside it).
    pub fn fraction(&mut self, at: Point<Pixels>) -> Option<(f32, f32)> {
        let r = self.rect();
        let x = (f32::from(at.x) - f32::from(r.origin.x)) / f32::from(r.size.width).max(1.);
        let y = (f32::from(at.y) - f32::from(r.origin.y)) / f32::from(r.size.height).max(1.);
        ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then_some((x, y))
    }

    /// Keep the pointer (in picture pixels) away from the stage's edges.
    pub fn follow_pointer(&mut self, x: f32, y: f32) {
        if !self.follow || self.drag.is_some() || !self.zoomed() {
            return;
        }
        let r = self.rect();
        let s = self.scale();
        let (bw, bh) = wh(&self.bounds);
        let (sx, sy) = (f32::from(r.origin.x) + x * s, f32::from(r.origin.y) + y * s);
        let (left, top) = (f32::from(self.bounds.origin.x), f32::from(self.bounds.origin.y));
        let (mx, my) = (bw * FOLLOW_MARGIN, bh * FOLLOW_MARGIN);
        let dx = if sx < left + mx { left + mx - sx } else if sx > left + bw - mx { left + bw - mx - sx } else { 0. };
        let dy = if sy < top + my { top + my - sy } else if sy > top + bh - my { top + bh - my - sy } else { 0. };
        if dx != 0. || dy != 0. {
            self.pan(dx, dy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ultrawide picture on a 1000x500 stage.
    fn view() -> StageView {
        let mut v = StageView::default();
        v.layout(Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(500.))), (5120., 1440.), 1.);
        v
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn fit_letterboxes_and_fill_covers() {
        let mut v = view();
        let r = v.rect();
        assert!(close(f32::from(r.size.width), 1000.));
        assert!(f32::from(r.origin.y) > 0.);
        assert!(!v.zoomed());
        v.set(Zoom::Fill);
        let r = v.rect();
        assert!(close(f32::from(r.size.height), 500.));
        assert!(v.zoomed());
        // Centred: the picture's middle is the stage's middle.
        assert!(close(f32::from(r.origin.x) + f32::from(r.size.width) / 2., 500.));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = view();
        v.set(Zoom::Fill);
        let at = point(px(800.), px(100.));
        let before = v.rect();
        let u = (800. - f32::from(before.origin.x)) / f32::from(before.size.width);
        v.zoom_at(at, 2.);
        let after = v.rect();
        let u2 = (800. - f32::from(after.origin.x)) / f32::from(after.size.width);
        assert!(close(u, u2), "{u} {u2}");
        assert!(matches!(v.zoom, Zoom::Free(_)));
        // Never smaller than the whole picture.
        v.zoom_at(at, 0.001);
        assert_eq!(v.zoom, Zoom::Fit);
    }

    #[test]
    fn pan_stops_at_the_edges() {
        let mut v = view();
        v.set(Zoom::Fill);
        v.pan(1e6, 0.);
        let r = v.rect();
        assert!(close(f32::from(r.origin.x), 0.));
        v.pan(-1e6, 0.);
        let r = v.rect();
        assert!(close(f32::from(r.origin.x) + f32::from(r.size.width), 1000.));
    }

    #[test]
    fn follow_brings_the_pointer_into_view() {
        let mut v = view();
        v.set(Zoom::Fill);
        // The pointer far right, out of view: it comes in, up to the margin.
        let before = f32::from(v.rect().origin.x) + 4500. * v.scale();
        assert!(before > 1000.);
        v.follow_pointer(4500., 700.);
        let r = v.rect();
        let sx = f32::from(r.origin.x) + 4500. * v.scale();
        assert!((sx - 1000. * (1. - FOLLOW_MARGIN)).abs() < 0.5, "{sx}");
        // At the very edge it goes as far as the picture allows.
        v.follow_pointer(5119., 700.);
        let r = v.rect();
        assert!((f32::from(r.origin.x) + f32::from(r.size.width) - 1000.).abs() < 0.5);
        assert!(v.percent() > 0);
    }

    #[test]
    fn actual_is_one_pixel_per_screen_pixel() {
        let mut v = view();
        v.scale_factor = 2.;
        v.set(Zoom::Actual);
        assert_eq!(v.percent(), 100);
        assert!(close(v.scale(), 0.5));
    }
}
