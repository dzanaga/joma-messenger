//! Settings, the profile and the pixel editor.

use super::*;

impl Joma {
    /// Settings, as a page in place of the list and the conversation: sections on the
    /// left (interface, audio, video, desktop share, about), the chosen one on the right.
    pub(super) fn settings_page(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let v = self.settings.clone()?;
        let title = |text: &str| div().pb(px(S2)).text_size(px(t.size + 2.)).text_color(t.bright).font_weight(FontWeight::BOLD).child(text.to_owned());
        let note = |text: &str| div().pt(px(10.)).text_size(px(t.size - 2.)).text_color(t.muted).child(text.to_owned());
        let label = |text: &str| div().w(px(if self.narrow { 96. } else { 130. })).flex_none().pt(px(4.)).text_color(t.text).child(text.to_owned());
        let row = |l: gpui::Div, chips: gpui::Div| div().flex().gap(px(10.)).py(px(3.)).child(l).child(chips.flex().flex_wrap().gap(px(4.)).flex_1().min_w(px(0.)));
        // A row of choices; `pick` runs with the chosen value.
        fn chips(t: &Theme, cx: &mut Context<Joma>, id: &str, options: Vec<(String, String)>, current: &str, pick: fn(&mut Joma, &str, &mut Context<Joma>)) -> gpui::Div {
            let mut d = div();
            for (i, (value, text)) in options.into_iter().enumerate() {
                let on = value == current;
                d = d.child(button(SharedString::from(format!("{id}-{i}")), "", &text, t, on).on_click(cx.listener(move |this, _, _, cx| pick(this, &value, cx))));
            }
            d
        }
        let idx = |i: usize| v.current.get(i).copied().unwrap_or(0).to_string();
        let devices = |list: &[String]| list.iter().enumerate().map(|(i, n)| (i.to_string(), if n.is_empty() { format!("device {i}") } else { n.clone() })).collect::<Vec<_>>();
        let processing = vec![("auto".to_owned(), "auto".to_owned()), ("audioProcessor".into(), "Jami's".into()), ("system".into(), "system".into()), ("none".into(), "off".into())];
        let size_now = v.camera_settings.get("size").cloned().unwrap_or_default();
        let rate_now = v.camera_settings.get("rate").cloned().unwrap_or_default();
        let rates: Vec<(String, String)> = v
            .sizes
            .iter()
            .find(|(s, _)| *s == size_now)
            .map(|(_, r)| r.iter().map(|x| (x.clone(), format!("{} fps", x.split('.').next().unwrap_or(x)))).collect())
            .unwrap_or_default();
        let page = div().flex().flex_col();
        let content = match v.section {
            0 => page
                .child(title("Interface"))
                .child(row(
                    label("corners"),
                    chips(t, cx, "round", vec![("on".into(), "rounded".into()), ("off".into(), "square".into())], if self.quick.rounded { "on" } else { "off" }, |j, x, cx| j.set_rounded(x == "on", cx)),
                ))
                .child(row(
                    label("version"),
                    chips(t, cx, "ver", vec![("on".into(), "shown in the tab strip".into()), ("off".into(), "hidden".into())], if self.quick.show_version { "on" } else { "off" }, |j, x, cx| {
                        j.set_show_version(x == "on", cx)
                    }),
                ))
                .child(note("Colours and the font follow your Omarchy theme and terminal.")),
            1 => page
                .child(title("Audio"))
                .child(row(label("microphone"), chips(t, cx, "ain", devices(&v.inputs), &idx(1), |j, x, cx| j.set_audio("input", x.parse().unwrap_or(0), cx))))
                .child(row(label("speakers"), chips(t, cx, "aout", devices(&v.outputs), &idx(0), |j, x, cx| j.set_audio("output", x.parse().unwrap_or(0), cx))))
                .child(row(label("ringtone on"), chips(t, cx, "aring", devices(&v.outputs), &idx(2), |j, x, cx| j.set_audio("ringtone", x.parse().unwrap_or(0), cx))))
                .child(row(label("noise removal"), chips(t, cx, "ns", processing.clone(), &v.noise, |j, x, cx| j.set_processing("noise", x, cx))))
                .child(row(label("echo removal"), chips(t, cx, "ec", processing, &v.echo, |j, x, cx| j.set_processing("echo", x, cx))))
                .child(row(
                    label("auto gain"),
                    chips(t, cx, "agc", vec![("on".into(), "on".into()), ("off".into(), "off".into())], if v.agc { "on" } else { "off" }, |j, x, cx| j.set_processing("agc", x, cx)),
                ))
                .child(row(
                    label("voice messages"),
                    chips(
                        t,
                        cx,
                        "tr",
                        vec![("".into(), "automatic".into()), ("superwhisper".into(), "superwhisper".into()), ("whisper".into(), "whisper.cpp".into()), ("off".into(), "off".into())],
                        &self.quick.transcribe,
                        |j, x, cx| j.set_transcribe(x, cx),
                    ),
                ))
                .child(note(match crate::media::transcriber(&self.quick.transcribe) {
                    Some("whisper") => "Voice messages are transcribed by themselves on this machine with whisper.cpp (language detected) and kept in Joma's cache. The model (about 150 MB) is downloaded at the first one.",
                    Some(_) if self.quick.transcribe.is_empty() => "Voice messages are transcribed by themselves with superwhisper, which is set to work offline: they stay on this machine. Language: superwhisper's (automatic by default).",
                    Some(_) => "Transcripts are made by superwhisper in the mode it is set to: in a cloud mode the voice message leaves this machine.",
                    None if self.quick.transcribe == "off" => "Voice messages are not transcribed.",
                    None if self.quick.transcribe.is_empty() => "Automatic uses superwhisper when it is set to work offline, or whisper.cpp (whisper-cli). Neither is ready, so voice messages are not transcribed.",
                    None => "Not installed, so voice messages are not transcribed.",
                }))
                .child(note("Devices and sound processing apply to the next call.")),
            2 => page
                .child(title("Video"))
                .when(v.cameras.is_empty(), |d| d.child(div().py(px(3.)).text_color(t.muted).child("no camera found")))
                .when(!v.cameras.is_empty(), |d| {
                    d.child(row(label("camera"), chips(t, cx, "cam", v.cameras.iter().map(|c| (c.clone(), c.clone())).collect(), &v.camera, |j, x, cx| j.set_camera(x, cx))))
                        .child(row(label("resolution"), chips(t, cx, "size", v.sizes.iter().take(8).map(|(s, _)| (s.clone(), s.clone())).collect(), &size_now, |j, x, cx| j.set_camera_mode(Some(x), None, cx))))
                        .child(row(label("frame rate"), chips(t, cx, "rate", rates, &rate_now, |j, x, cx| j.set_camera_mode(None, Some(x), cx))))
                })
                .child(row(
                    label("call quality"),
                    chips(
                        t,
                        cx,
                        "vq",
                        vec![("auto".into(), "auto".into()), ("low".into(), "low · 0.5 Mb/s".into()), ("medium".into(), "medium · 1.5".into()), ("high".into(), "high · 4".into())],
                        &self.quick.video_quality,
                        |j, x, cx| j.set_video_quality(x, cx),
                    ),
                ))
                .child(note("Auto lets Jami adapt the bitrate; the others fix it.")),
            3 => page
                .child(title("Desktop share"))
                .child(row(
                    label("frame rate"),
                    chips(t, cx, "sfps", vec![("30".into(), "30 fps · lighter".into()), ("60".into(), "60 fps · smoother".into())], &self.quick.share_fps.to_string(), |j, x, cx| {
                        j.set_share_fps(x.parse().unwrap_or(60), cx)
                    }),
                ))
                .child(note("Applies to the next Joma share. Screenshots of a share go to ~/Pictures/joma.")),
            _ => page
                .child(title("About"))
                .child(row(
                    label("Joma"),
                    div().items_center().child(div().text_color(t.bright).child(format!("v{}", joma_quick::version::pretty(env!("CARGO_PKG_VERSION"))))).when(!self.update_note.is_empty(), |d| {
                        d.child(div().text_color(if self.update.is_some() { t.accent } else { t.muted }).child(format!("· {}", self.update_note)))
                    }),
                ))
                .child(row(
                    label("updates"),
                    div()
                        .when_some(self.update.clone(), |d, v| {
                            d.child(
                                button("about-update", G_DOWNLOAD, &if self.updating { "installing…".to_owned() } else { format!("update to {} and restart", joma_quick::version::pretty(&v)) }, t, true)
                                    .on_click(cx.listener(|this, _, _, cx| this.update_now(cx))),
                            )
                        })
                        .child(button("about-check", G_UNDO, "check now", t, false).on_click(cx.listener(|this, _, _, cx| this.check_updates(true, cx)))),
                ))
                .child(row(
                    label("channel"),
                    chips(t, cx, "chan", vec![("off".into(), "stable releases only".into()), ("on".into(), "pre-releases too".into())], if self.quick.prereleases { "on" } else { "off" }, |j, x, cx| {
                        j.set_prereleases(x == "on", cx)
                    }),
                ))
                .child(row(label("account"), div().child(div().pt(px(4.)).child(format!("{} · libjami {}", if self.model.my_name.is_empty() { "jami" } else { &self.model.my_name }, self.model.account)))))
                .child(row(label("keys"), div().child(button("about-keys", G_BOLT, "show the keys · F1", t, false).on_click(cx.listener(|this, _, _, cx| {
                    this.keys_open = true;
                    cx.notify();
                })))))
                .child(note(if Joma::installed_with_mise() {
                    "Joma looks for releases on GitHub every six hours. An update is installed with mise, then Joma restarts."
                } else {
                    "Joma looks for releases on GitHub every six hours. This copy was not installed with mise, so it shows the command instead of updating itself."
                }))
                .child(note("Settings are kept in ~/.config/joma/quick.toml and in Jami's own configuration.")),
        };
        let nav_item = |i: usize, icon: Icon, name: &'static str| {
            let on = v.section == i;
            div()
                .id(SharedString::from(format!("set-nav-{i}")))
                .flex()
                .items_center()
                .gap(px(S2))
                .h(px(30.))
                .px(px(S2))
                .rounded(px(radius()))
                .text_color(if on { t.accent } else { t.text })
                .when(on, |d| d.bg(t.accent_bg))
                .cursor_pointer()
                .hover(|d| d.bg(t.raised))
                .child(icon)
                .when(!self.narrow, |d| d.child(name))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(s) = this.settings.as_mut() {
                        s.section = i;
                    }
                    cx.notify();
                }))
        };
        Some(
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .gap(px(GAP))
                .px(px(GAP))
                .pb(px(GAP))
                .child(
                    tile(t, false)
                        .flex_none()
                        .w(px(if self.narrow { 44. } else { 220. }))
                        .bg(t.panel)
                        .when(!self.narrow, |d| d.child(div().flex().flex_none().items_center().h(px(38.)).pl(px(S3)).child(caption(t, "settings"))))
                        .when(self.narrow, |d| d.pt(px(S1)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .px(px(S1))
                                .child(nav_item(0, G_COG, "interface"))
                                .child(nav_item(1, G_MIC, "audio"))
                                .child(nav_item(2, G_VIDEO, "video"))
                                .child(nav_item(3, G_DESKTOP, "desktop share"))
                                .child(nav_item(4, G_BOLT, "about")),
                        ),
                )
                .child(
                    tile(t, true)
                        .flex_1()
                        .bg(t.bg)
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .justify_end()
                                .h(px(38.))
                                .pr(px(S1))
                                .child(button("set-close", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                                    this.settings = None;
                                    cx.notify();
                                }))),
                        )
                        .child(div().id("settings-body").flex().flex_col().flex_1().min_h(px(0.)).overflow_y_scroll().px(px(20.)).pb(px(20.)).child(content)),
                )
                .into_any_element(),
        )
    }

    /// A picture of the conversation, as large as the window allows: before and after,
    /// the system's viewer, close (Esc, or a click beside it).
    pub(super) fn viewer_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = self.viewer.clone()?;
        let all = self.pictures();
        let at = all.iter().position(|(p, _)| *p == path);
        let name = at.map(|i| all[i].1.clone()).unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let count = match at {
            Some(i) if all.len() > 1 => format!("{} of {} · {name}", i + 1, all.len()),
            _ => name,
        };
        let outside = path.clone();
        let close = |this: &mut Joma, cx: &mut Context<Joma>| {
            this.viewer = None;
            cx.notify();
        };
        Some(
            div()
                .id("viewer")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .bg(gpui::black().opacity(0.88))
                .occlude()
                .on_click(cx.listener(move |this, _, _, cx| close(this, cx)))
                .child(
                    div()
                        .id("viewer-bar")
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(S1))
                        .h(px(40.))
                        .px(px(S3))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.text).child(count))
                        .when(all.len() > 1, |d| {
                            d.child(button("viewer-prev", G_BACK, "", t, false).on_click(cx.listener(|this, _, _, cx| this.viewer_step(-1, cx))))
                                .child(button("viewer-next", G_FORWARD, "", t, false).on_click(cx.listener(|this, _, _, cx| this.viewer_step(1, cx))))
                        })
                        .child(button("viewer-out", G_EXPAND, "open outside", t, false).on_click(move |_, _, _| Joma::open_outside(&outside)))
                        .child(button("viewer-close", G_TIMES, "", t, false).on_click(cx.listener(move |this, _, _, cx| close(this, cx)))),
                )
                .child(div().flex_1().min_h(px(0.)).p(px(S3)).child(gpui::img(path).size_full().object_fit(ObjectFit::Contain)))
                .into_any_element(),
        )
    }

    /// Our picture: choose one, draw one in pixels, or remove it.
    pub(super) fn profile_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.profile_edit.as_ref()?;
        let me = self.model.me.clone();
        let current = self.model.avatars.get(&me).cloned();
        let body: AnyElement = match &p.drawing {
            None => div()
                .flex()
                .items_center()
                .gap(px(18.))
                .p(px(16.))
                .child(avatar(t, current.clone(), &me, &self.model.my_name, 112., None))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(self.model.my_name.clone()))
                        .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child("Contacts see it next to your name."))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.))
                                .child(button("pf-pick", G_IMAGE, "picture…", t, false).on_click(cx.listener(|this, _, _, cx| this.choose_picture(cx))))
                                .child(button("pf-draw", G_PENCIL, "draw pixels", t, false).on_click(cx.listener(|this, _, _, cx| this.start_drawing(cx))))
                                .when(current.is_some(), |d| {
                                    d.child(button("pf-remove", G_TRASH, "remove", t, false).text_color(t.red).on_click(cx.listener(|this, _, _, cx| this.remove_avatar(cx))))
                                }),
                        ),
                )
                .into_any_element(),
            Some(art) => self.pixel_editor(t, p, art.clone(), cx),
        };
        Some(
            div()
                .id("profile-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .child(
                    div()
                        .id("profile-box")
                        .flex()
                        .flex_col()
                        .m(px(16.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.accent).rounded(px(radius()))
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
                                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(if p.drawing.is_some() { "Draw your picture" } else { "Your picture" }))
                                .child(button("pf-close", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                                    this.profile_edit = None;
                                    cx.notify();
                                }))),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }

    /// A 16×16 grid to paint on, the colour grid, the tools and a live preview.
    pub(super) fn pixel_editor(&self, t: &Theme, p: &ProfileEdit, art: crate::avatar::PixelArt, cx: &mut Context<Self>) -> AnyElement {
        use crate::avatar::GRID;
        let cell = 18.;
        let side = cell * GRID as f32;
        let bounds = p.grid.clone();
        let line = t.border.opacity(0.35);
        let art2 = art.clone();
        let grid = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                bounds.set(b);
                let c = f32::from(b.size.width) / GRID as f32;
                for y in 0..GRID {
                    for x in 0..GRID {
                        let rgb = art2.get(x, y).unwrap_or(art2.background);
                        let r = Bounds::new(point(b.origin.x + px(x as f32 * c), b.origin.y + px(y as f32 * c)), size(px(c), px(c)));
                        window.paint_quad(gpui::fill(r, gpui::rgb(rgb)));
                    }
                }
                for i in 0..=GRID {
                    let o = px(i as f32 * c);
                    window.paint_quad(gpui::fill(Bounds::new(point(b.origin.x + o, b.origin.y), size(px(1.), b.size.height)), line));
                    window.paint_quad(gpui::fill(Bounds::new(point(b.origin.x, b.origin.y + o), size(b.size.width, px(1.))), line));
                }
            },
        )
        .size(px(side));
        let grid = div()
            .id("pixel-grid")
            .size(px(side))
            .cursor_crosshair()
            .child(grid)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                if let Some(p) = this.profile_edit.as_mut() {
                    p.painting = p.tool != PaintTool::Fill;
                }
                this.paint_at(e.position, cx);
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let painting = this.profile_edit.as_ref().is_some_and(|p| p.painting);
                if painting && e.pressed_button == Some(MouseButton::Left) {
                    this.paint_at(e.position, cx);
                } else if painting {
                    if let Some(p) = this.profile_edit.as_mut() {
                        p.painting = false;
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                if let Some(p) = this.profile_edit.as_mut() {
                    p.painting = false;
                }
            }));
        let mut swatches = Vec::new();
        for (r, row) in crate::avatar::palette().into_iter().enumerate() {
            let mut line = div().flex();
            for (i, c) in row.into_iter().enumerate() {
                line = line.child(
                    div()
                        .id(SharedString::from(format!("sw-{r}-{i}")))
                        .size(px(20.))
                        .bg(gpui::rgb(c))
                        .border_1()
                        .border_color(if c == p.color { t.bright } else { gpui::black().opacity(0.25) }).rounded(px(radius()))
                        .cursor_pointer()
                        .hover(|d| d.border_color(gpui::white()))
                        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                            if let Some(p) = this.profile_edit.as_mut() {
                                // Shift picks the background behind empty cells.
                                if e.modifiers().shift {
                                    if let Some(a) = p.drawing.as_mut() {
                                        a.background = c;
                                    }
                                } else {
                                    p.color = c;
                                    if p.tool == PaintTool::Eraser {
                                        p.tool = PaintTool::Pen;
                                    }
                                }
                            }
                            cx.notify();
                        })),
                );
            }
            swatches.push(line);
        }
        fn tool(t: &Theme, p: &ProfileEdit, cx: &mut Context<Joma>, id: &'static str, g: Icon, label: &str, which: PaintTool) -> gpui::Stateful<gpui::Div> {
            button(id, g, label, t, p.tool == which).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(p) = this.profile_edit.as_mut() {
                    p.tool = which;
                }
                cx.notify();
            }))
        }
        let preview = div()
            .size(px(64.))
            .rounded(px(radius()))
            .overflow_hidden()
            .child({
                let a = art.clone();
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        let c = f32::from(b.size.width) / GRID as f32;
                        for y in 0..GRID {
                            for x in 0..GRID {
                                let r = Bounds::new(point(b.origin.x + px(x as f32 * c), b.origin.y + px(y as f32 * c)), size(px(c + 0.5), px(c + 0.5)));
                                window.paint_quad(gpui::fill(r, gpui::rgb(a.get(x, y).unwrap_or(a.background))));
                            }
                        }
                    },
                )
                .size_full()
            });
        div()
            .flex()
            .gap(px(16.))
            .p(px(16.))
            .child(grid)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(tool(t, p, cx, "pt-pen", G_PENCIL, "pen", PaintTool::Pen))
                            .child(tool(t, p, cx, "pt-erase", G_ERASER, "eraser", PaintTool::Eraser))
                            .child(tool(t, p, cx, "pt-fill", G_FILL, "fill", PaintTool::Fill)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(button("pt-mirror", G_MIRROR, "mirror", t, p.mirror).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.profile_edit.as_mut() {
                                    p.mirror = !p.mirror;
                                }
                                cx.notify();
                            })))
                            .child(button("pt-clear", G_TRASH, "clear", t, false).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(a) = this.profile_edit.as_mut().and_then(|p| p.drawing.as_mut()) {
                                    a.cells.iter_mut().for_each(|c| *c = None);
                                }
                                cx.notify();
                            }))),
                    )
                    .child(div().flex().items_center().gap(px(8.)).child(div().size(px(22.)).bg(gpui::rgb(p.color)).border_1().border_color(t.bright).rounded(px(radius()))).child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("#{:06x} · shift+click: background", p.color))))
                    .child(div().flex().flex_col().children(swatches))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .justify_between()
                            .gap(px(10.))
                            .child(preview)
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .child(button("pt-cancel", G_BACK, "back", t, false).on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = this.profile_edit.as_mut() {
                                            p.drawing = None;
                                        }
                                        cx.notify();
                                    })))
                                    .child(button("pt-save", G_CHECK, "use it", t, true).on_click(cx.listener(|this, _, _, cx| this.save_drawing(cx)))),
                            ),
                    ),
            )
            .into_any_element()
    }
}
