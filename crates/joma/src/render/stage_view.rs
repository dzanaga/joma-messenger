//! The stage (remote video or a shared desktop), its tools, the share strip and picker.

use super::*;

impl Joma {
    /// A peer's desktop (or video), fitted or zoomed, with their pointer on top. The wheel
    /// zooms (Ctrl + touchpad too), the touchpad pans, dragging pans, a double click fills
    /// the screen.
    pub(super) fn stage_canvas(&self, t: &Theme, image: Arc<RenderImage>, pointer: Option<(joma_desk::view::Pointer, Arc<RenderImage>)>, cx: &mut Context<Self>) -> AnyElement {
        let bg = t.bar;
        let view = self.stage_view.clone();
        // What we drew on the shared screen (only while watching one).
        let ink = self.viewing_key().and_then(|k| self.desk.viewer(&k)).map(|v| v.state.lock().unwrap().ink.clone());
        let inking = self.ink_tool != InkTool::None && ink.is_some();
        let painted = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                window.paint_quad(gpui::fill(bounds, bg));
                let sz = image.size(0);
                let (w, h) = (sz.width.0 as f32, sz.height.0 as f32);
                let mut v = view.borrow_mut();
                v.layout(bounds, (w, h), window.scale_factor());
                if let Some((p, _)) = pointer.as_ref().filter(|(p, _)| p.visible) {
                    v.follow_pointer(p.x as f32, p.y as f32);
                }
                let r = v.rect();
                drop(v);
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                    if let Some(c) = &ink {
                        paint_ink(window, c, r);
                    }
                    if let Some((p, cursor)) = &pointer {
                        if p.visible {
                            let k = f32::from(r.size.width) / w.max(1.);
                            let csz = cursor.size(0);
                            let origin = point(
                                r.origin.x + px((p.x as f32 - p.hot_x as f32) * k),
                                r.origin.y + px((p.y as f32 - p.hot_y as f32) * k),
                            );
                            // The pointer keeps a readable size when the picture is shrunk.
                            let ck = k.max(0.6);
                            let cb = Bounds::new(origin, size(px(csz.width.0 as f32 * ck), px(csz.height.0 as f32 * ck)));
                            let _ = window.paint_image(cb, Corners::default(), cursor.clone(), 0, false);
                        }
                    }
                });
            },
        )
        .size_full();
        div()
            .id("stage")
            .size_full()
            .child(painted)
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let mut v = this.stage_view.borrow_mut();
                match e.delta {
                    // A touchpad pans; with Ctrl (or a pinch) it zooms.
                    // Panning by hand stops following the pointer (the tools turn it back on).
                    ScrollDelta::Pixels(d) if !e.modifiers.control => {
                        v.follow = false;
                        v.pan(f32::from(d.x), f32::from(d.y));
                    }
                    ScrollDelta::Pixels(d) => v.zoom_at(e.position, (1. + f32::from(d.y) / 200.).clamp(0.5, 2.)),
                    // A mouse wheel zooms; with Shift it pans sideways.
                    ScrollDelta::Lines(d) if e.modifiers.shift => {
                        v.follow = false;
                        v.pan(d.y * 60., 0.);
                    }
                    ScrollDelta::Lines(d) => v.zoom_at(e.position, 1.2f32.powf(d.y)),
                }
                drop(v);
                cx.notify();
            }))
            .when(inking, |d| d.cursor_crosshair())
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                if e.click_count == 2 {
                    this.stage_view.borrow_mut().drag = None;
                    this.toggle_fullscreen(window, cx);
                    return;
                }
                if this.ink_tool != InkTool::None && this.viewing_key().is_some() {
                    let at = this.stage_view.borrow_mut().fraction(e.position);
                    if let Some((x, y)) = at {
                        this.ink_down(x, y, cx);
                    }
                    return;
                }
                let mut v = this.stage_view.borrow_mut();
                v.drag = Some((e.position, v.center));
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.ink_tool == InkTool::Laser {
                    this.ink(joma_desk::ink::Ink::LaserOff, cx);
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.ink_tool != InkTool::None && this.viewing_key().is_some() {
                    let at = this.stage_view.borrow_mut().fraction(e.position);
                    match at {
                        Some((x, y)) => this.ink_move(x, y, e.pressed_button == Some(MouseButton::Left), cx),
                        None if this.ink_tool == InkTool::Laser => this.ink(joma_desk::ink::Ink::LaserOff, cx),
                        None => {}
                    }
                    if e.pressed_button != Some(MouseButton::Left) {
                        this.ink_stroke = None;
                    }
                    return;
                }
                let mut v = this.stage_view.borrow_mut();
                if v.drag.is_some() {
                    if e.pressed_button == Some(MouseButton::Left) {
                        v.follow = false;
                        v.drag_to(e.position);
                        drop(v);
                        cx.notify();
                    } else {
                        v.drag = None;
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.stage_view.borrow_mut().drag = None;
                this.ink_stroke = None;
            }))
            .into_any_element()
    }

    /// The stage in the call or share panel: as tall as set, with a bar under it to drag
    /// (double click: the stage takes the whole window), or the whole window.
    pub(super) fn stage_box(&self, t: &Theme, stage: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        if self.stage {
            return div().relative().flex_1().min_h(px(0.)).child(stage).into_any_element();
        }
        let dragging = self.resizing.is_some();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .child(div().relative().h(px(self.stage_height)).child(stage))
            .child(
                div()
                    .id("stage-resize")
                    .h(px(7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_row_resize()
                    .bg(if dragging { t.accent_bg } else { t.bar })
                    .hover(|d| d.bg(t.accent_bg))
                    .child(div().w(px(36.)).h(px(2.)).bg(if dragging { t.accent } else { t.border }))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        if e.click_count == 2 {
                            this.resizing = None;
                            this.stage = true;
                        } else {
                            this.resizing = Some((f32::from(e.position.y), this.stage_height));
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Under the stage: what it shows, and the zoom.
    pub(super) fn stage_tools(&self, t: &Theme, info: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        use crate::stage::Zoom;
        let (zoom, pct, follow, zoomed) = {
            let v = self.stage_view.borrow();
            (v.zoom, v.percent(), v.follow, v.zoomed())
        };
        let small = |id: &'static str, glyph: Mark, label: &str, on: bool| button(id, glyph, label, t, on);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(6.))
            .px(px(12.))
            .py(px(3.))
            .text_size(px(t.size - 2.))
            .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.muted).child(info.unwrap_or_default()))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .when(self.viewing_key().is_some(), |d| {
                        let pen = self.ink_tool == InkTool::Pen;
                        let mut d = d
                            .child(small("ink-laser", (G_POINTER).into(), "laser", self.ink_tool == InkTool::Laser).on_click(cx.listener(|this, _, _, cx| this.set_ink_tool(InkTool::Laser, cx))))
                            .child(small("ink-pen", (G_PENCIL).into(), "pen", pen).on_click(cx.listener(|this, _, _, cx| this.set_ink_tool(InkTool::Pen, cx))));
                        if pen {
                            for (i, c) in [0xff3355u32, 0xffcc00, 0x33dd77, 0x33bbff].into_iter().enumerate() {
                                let on = self.ink_color == c;
                                d = d.child(
                                    div()
                                        .id(SharedString::from(format!("ink-c{i}")))
                                        .size(px(16.))
                                        .rounded_full()
                                        .bg(gpui::rgb(c))
                                        .border_2()
                                        .border_color(if on { t.bright } else { gpui::transparent_black() })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.ink_color = c;
                                            cx.notify();
                                        })),
                                );
                            }
                        }
                        d.child(small("ink-clear", (G_ERASER).into(), "clear", false).on_click(cx.listener(|this, _, _, cx| this.clear_ink(None, cx))))
                            .child(div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(t.border))
                    })
                    .child(small("z-fit", Mark::None, "fit", zoom == Zoom::Fit).on_click(cx.listener(|this, _, _, cx| this.zoom(Zoom::Fit, cx))))
                    .child(small("z-fill", Mark::None, "fill", zoom == Zoom::Fill).on_click(cx.listener(|this, _, _, cx| this.zoom(Zoom::Fill, cx))))
                    .child(small("z-1", Mark::None, "1:1", zoom == Zoom::Actual).on_click(cx.listener(|this, _, _, cx| this.zoom(Zoom::Actual, cx))))
                    .child(small("z-out", (G_ZOOM_OUT).into(), "", false).on_click(cx.listener(|this, _, _, cx| this.zoom_by(1. / 1.25, cx))))
                    .child(div().w(px(44.)).flex().justify_center().text_color(t.text).child(format!("{pct}%")))
                    .child(small("z-in", (G_ZOOM_IN).into(), "", false).on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))))
                    .child(
                        small("z-follow", (G_CROSSHAIRS).into(), "follow", follow && zoomed).on_click(cx.listener(|this, _, _, cx| {
                            let mut v = this.stage_view.borrow_mut();
                            v.follow = !v.follow;
                            drop(v);
                            cx.notify();
                        })),
                    )
                    .child(
                        small("z-full", (if self.fullscreen { G_COMPRESS } else { G_EXPAND }).into(), if self.fullscreen { "exit" } else { "full screen" }, self.fullscreen)
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx))),
                    ),
            )
            .into_any_element()
    }

    /// What the stage shows: the desktop being watched, else the call's video; with the
    /// peer's pointer and a line about it.
    pub(super) fn stage_source(&self) -> Option<(Arc<RenderImage>, Option<(joma_desk::view::Pointer, Arc<RenderImage>)>, Option<String>)> {
        if let (Some(key), Some(f)) = (self.viewing_key(), self.desk_frame.as_ref()) {
            let v = self.desk.viewer(&key)?;
            let st = v.state.lock().unwrap();
            let who = crate::desk::direct_peer(&key)
                .map(str::to_owned)
                .or_else(|| self.model.calls.get(&key).map(|c| c.peer.clone()))
                .map(|p| self.model.name_of(&p))
                .unwrap_or_default();
            let (w, h) = st.picture.as_ref().map_or((0, 0), |p| (p.width, p.height));
            let info = format!("{who}'s screen · {w}×{h} · {} · {}→{} · {:.0} fps", st.chroma, st.remote_pipeline, st.local_pipeline, st.fps);
            let pointer = self.pointer.as_ref().map(|p| (st.pointer.clone(), p.image.clone()));
            return Some((f.image.clone(), pointer, Some(info)));
        }
        let call = self.active_call()?;
        let video = call.sinks.iter().find_map(|s| self.frames.get(s))?;
        Some((video.image.clone(), None, Some(self.model.name_of(&call.peer))))
    }

    /// The stage alone, filling the screen, with its tools below.
    pub(super) fn fullscreen_stage(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (image, pointer, info) = self.stage_source()?;
        Some(
            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(t.bar)
                .child(div().flex_1().min_h(px(0.)).child(self.stage_canvas(t, image, pointer, cx)))
                .child(div().bg(t.panel).border_t_1().border_color(t.border).child(self.stage_tools(t, info.map(|i| format!("{i} · Esc to leave")), cx)))
                .into_any_element(),
        )
    }

    /// What we share and how it looks to the peer, with a way to change or stop it.
    pub(super) fn share_strip(&self, t: &Theme, key: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::desk::Sharing;
        let s = self.desk.sessions.get(key)?;
        let (what, joma) = match &s.sharing {
            Sharing::Joma { share, .. } => (source_label(&share.source, &share.screen), true),
            Sharing::Portal { .. } => ("the screen or window chosen in the portal (Jami's share)".to_owned(), false),
            Sharing::Off | Sharing::Picking => return None,
        };
        let state = s.sharing_label().unwrap_or_default();
        let preview = self.previews.get(key).map(|f| f.image.clone());
        let drawn = self.desk.shared(key).is_some_and(|(sh, _)| !sh.ink.lock().unwrap().canvas.is_empty());
        let bg = t.bar;
        let thumb = div()
            .flex_none()
            .w(px(160.))
            .h(px(90.))
            .border_1()
            .border_color(t.yellow)
            .bg(t.bar)
            .when_some(preview, |d, image| {
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            window.paint_quad(gpui::fill(bounds, bg));
                            let sz = image.size(0);
                            let r = fit(bounds, sz.width.0 as f32, sz.height.0 as f32);
                            let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                        },
                    )
                    .size_full(),
                )
            })
            .when(!joma, |d| d.flex().items_center().justify_center().text_color(t.muted).child(G_DESKTOP));
        let k1 = key.to_owned();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(6.))
                .border_t_1()
                .border_color(t.border)
                .child(thumb)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(labelled(G_BOLT, state).text_color(t.yellow))
                        .child(div().truncate().text_color(t.bright).child(format!("you share: {what}")))
                        .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(if joma { "the picture is what they see, about once a second" } else { "Jami's share shows no preview here" })),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(6.))
                        .when(drawn, |d| {
                            let k = key.to_owned();
                            d.child(button(SharedString::from(format!("inkclr-{key}")), G_ERASER, "clear drawings", t, false).on_click(cx.listener(move |this, _, _, cx| this.clear_ink(Some(&k), cx))))
                        })
                        .when(joma, |d| {
                            d.child(button(SharedString::from(format!("src-{key}")), G_EXCHANGE, "change", t, false).on_click(cx.listener(move |this, _, _, cx| this.change_source(&k1, cx))))
                        })
                        // A share without a call has its stop in the panel's header.
                        .when(crate::desk::direct_peer(key).is_none(), |d| {
                            d.child(button(SharedString::from(format!("unshare-{key}")), G_TIMES, "stop", t, false).text_color(t.red).on_click(cx.listener(|this, _, _, cx| this.toggle_share(cx))))
                        }),
                )
                .into_any_element(),
        )
    }

    /// Choosing what to share: every screen and window, with a thumbnail.
    pub(super) fn picker_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.picker.as_ref()?;
        let bg = t.bar;
        let title = match &p.target {
            PickFor::Switch(_) => format!("Share something else with {}", p.peer),
            _ => format!("What do you want to show {}?", p.peer),
        };
        let cards = p.sources.iter().enumerate().map(|(i, s)| {
            let thumb = p.thumbs.get(&s.source.key()).cloned();
            let window_src = matches!(s.source, joma_desk::share::Source::Window { .. });
            let glyph = if window_src { G_WINDOW } else { G_DESKTOP };
            let accent = if s.focused { t.accent } else { t.border };
            div()
                .id(SharedString::from(format!("pick-{i}")))
                .flex()
                .flex_col()
                .w(px(236.))
                .p(px(6.))
                .gap(px(4.))
                .border_1()
                .border_color(accent)
                .bg(t.panel)
                .cursor_pointer()
                .hover(|d| d.border_color(t.accent).bg(t.raised))
                .child(
                    div()
                        .w(px(222.))
                        .h(px(125.))
                        .bg(t.bar)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(t.muted)
                        .when_some(thumb.clone(), |d, image| {
                            d.child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, _| {
                                        window.paint_quad(gpui::fill(bounds, bg));
                                        let sz = image.size(0);
                                        let r = fit(bounds, sz.width.0 as f32, sz.height.0 as f32);
                                        let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                                    },
                                )
                                .size_full(),
                            )
                        })
                        .when(thumb.is_none(), |d| d.child("…")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .text_color(t.bright)
                        .child(div().text_color(if s.focused { t.accent } else { t.muted }).child(glyph))
                        .child(div().flex_1().min_w(px(0.)).truncate().child(s.label.clone()))
                        .child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("{}×{}", s.screen.width, s.screen.height))),
                )
                .child(div().truncate().text_size(px(t.size - 2.)).text_color(t.muted).child(if s.detail.is_empty() { " ".to_owned() } else { s.detail.clone() }))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(Some(i), window, cx)))
        });
        let portal = matches!(p.target, PickFor::Call(_));
        Some(
            div()
                .id("picker")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .child(
                    div()
                        .id("picker-box")
                        .flex()
                        .flex_col()
                        .max_w(px(1040.))
                        .max_h_full()
                        .m(px(16.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.accent)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .px(px(12.))
                                .py(px(8.))
                                .border_b_1()
                                .border_color(t.border)
                                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(title))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(6.))
                                        .child(button("pick-area", G_CROP, "area…", t, false).on_click(cx.listener(|this, _, window, cx| this.pick_area(window, cx))))
                                        .when(portal, |d| {
                                            d.child(button("pick-portal", G_EXCHANGE, "Jami's share", t, false).on_click(cx.listener(|this, _, window, cx| this.pick(None, window, cx))))
                                        })
                                        .child(button("pick-cancel", G_TIMES, "cancel", t, false).on_click(cx.listener(|this, _, window, cx| this.close_picker(window, cx)))),
                                ),
                        )
                        .child(
                            div()
                                .id("picker-list")
                                .overflow_y_scroll()
                                .flex()
                                .flex_wrap()
                                .gap(px(10.))
                                .p(px(12.))
                                .children(cards),
                        )
                        .child(div().px(px(12.)).pb(px(8.)).text_size(px(t.size - 2.)).text_color(t.muted).child("A window is sent alone, even when covered; area… lets you draw a rectangle. You can change it while sharing. Esc closes.")),
                )
                .into_any_element(),
        )
    }

    /// Shares without a call: an offer to answer, ours running, or theirs on the stage.
    pub(super) fn direct_panels(&self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::desk::{direct_peer, Sharing, Viewing};
        let mut out = Vec::new();
        for key in self.desk.direct_sessions() {
            let Some(s) = self.desk.sessions.get(&key) else { continue };
            let name = self.model.name_of(direct_peer(&key).unwrap_or_default());
            let (text, asked) = match (&s.viewing, &s.sharing) {
                (Viewing::Asked { .. }, _) => (format!("{name} wants to show you their screen"), true),
                (Viewing::On { .. }, _) => (format!("{name}'s screen"), false),
                (Viewing::Querying { .. } | Viewing::Opening { .. }, _) => (format!("connecting to {name}'s screen…"), false),
                (Viewing::Off, Sharing::Off) => continue,
                (Viewing::Off, _) => (format!("sharing with {name}"), false),
            };
            let (k1, k2) = (key.clone(), key.clone());
            let controls = div()
                .flex()
                .gap(px(6.))
                .when(asked, |d| {
                    d.child(button(SharedString::from(format!("watch-{key}")), G_EYE, "watch", t, true).on_click(cx.listener(move |this, _, _, cx| this.watch_direct(&k1, cx))))
                })
                .when(self.desk.viewer(&key).is_some(), |d| {
                    d.child(button(SharedString::from(format!("stage-{key}")), G_EXPAND, "", t, self.stage).on_click(cx.listener(|this, _, _, cx| {
                        this.stage = !this.stage;
                        cx.notify();
                    })))
                })
                .child(
                    button(SharedString::from(format!("stop-{key}")), G_TIMES, if asked { "decline" } else { "stop" }, t, false)
                        .text_color(t.red)
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_direct(&k2, cx))),
                );
            let mut panel = div()
                .flex()
                .flex_col()
                .border_b_1()
                .border_color(t.border)
                .bg(if asked { t.accent_bg } else { t.panel })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .px(px(12.))
                        .py(px(6.))
                        .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.bright).child(text))
                        .child(controls.flex_none()),
                );
            if let Some(strip) = self.share_strip(t, &key, cx) {
                panel = panel.child(strip);
            }
            if self.desk.viewer(&key).is_some() && self.viewing_key().as_deref() == Some(key.as_str()) {
                if let Some((image, pointer, info)) = self.stage_source() {
                    let stage = self.stage_canvas(t, image, pointer, cx);
                    panel = panel
                        .when(self.stage, |d| d.flex_1())
                        .child(self.stage_box(t, stage, cx))
                        .child(self.stage_tools(t, info, cx));
                }
            }
            out.push(panel.into_any_element());
        }
        out
    }
}
