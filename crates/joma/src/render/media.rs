//! Voice and video messages in the chat (D-26). A voice message looks like the Jami
//! app's: a round play button, the bars of its sound (a click on one jumps there) and its
//! length. A video message shows a still picture and plays inside the bubble, with its
//! transcript as subtitles; the corner button makes it fill the window (Telegram's).

use super::*;
use crate::model::FileRef;
use crate::player::{clock, BARS};

const BAR_W: f32 = 3.;
const BAR_GAP: f32 = 2.;
const BAR_H: f32 = 26.;

/// A round button with an icon: play, pause, expand.
fn round(id: impl Into<SharedString>, icon: Icon, side: f32, bg: Hsla, fg: Hsla) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .child(icon.sized((side * 0.45).round()))
}

/// A small dark label over a video.
fn chip(text: String) -> gpui::Div {
    div().px(px(6.)).py(px(1.)).bg(gpui::black().opacity(0.55)).text_color(gpui::white()).text_size(px(11.)).child(text)
}

/// The current picture of the playing video, fitted into its box.
fn frame_canvas(image: Arc<RenderImage>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let s = image.size(0);
            let r = fit(bounds, f32::from(s.width.0 as f32), f32::from(s.height.0 as f32));
            let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
        },
    )
    .size_full()
}

impl Joma {
    /// The voice or video message of `msg` (`file`), in its bubble.
    pub(super) fn media_bubble(&self, t: &Theme, conv: &str, msg: &str, file: &FileRef, inner: f32, cx: &mut Context<Self>) -> AnyElement {
        let saved = self.saved_file(conv, &file.id);
        let view = saved.as_ref().and_then(|_| self.media.get(&file.id).cloned().flatten());
        let video = match &view {
            Some(v) => v.info.video.is_some(),
            None => crate::media::is_video(std::path::Path::new(&file.name)),
        };
        if video {
            self.video_bubble(t, conv, msg, file, saved, view, inner, cx)
        } else {
            self.voice_bubble(t, conv, msg, file, saved, view, cx)
        }
    }

    /// Play (or fetch, then play) from the button.
    fn play_click(&self, conv: &str, msg: &str, file: &FileRef, saved: Option<std::path::PathBuf>, cx: &mut Context<Self>) -> Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static> {
        let (conv, msg, file) = (conv.to_owned(), msg.to_owned(), file.clone());
        Box::new(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
            cx.stop_propagation();
            match &saved {
                Some(path) => this.toggle_play(&file.id, path, cx),
                None => {
                    this.play_after = Some(file.id.clone());
                    this.file_clicked(&conv, &msg, &file, cx);
                }
            }
            cx.notify();
        }))
    }

    fn voice_bubble(
        &self,
        t: &Theme,
        conv: &str,
        msg: &str,
        file: &FileRef,
        saved: Option<std::path::PathBuf>,
        view: Option<MediaView>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (playing, pos) = self.media_position(&file.id);
        let duration = view.as_ref().map_or(0., |v| v.info.duration);
        let done = if duration > 0. { (pos / duration) as f32 } else { 0. };
        let arriving = saved.is_none() && self.downloads.contains_key(&file.id);
        let bars: Vec<f32> = view.as_ref().map(|v| v.bars.clone()).filter(|b| !b.is_empty()).unwrap_or_else(|| vec![0.; BARS]);
        let n = bars.len();
        let time = if pos > 0. { clock(pos) } else if duration > 0. { clock(duration) } else if arriving { "arriving…".into() } else { String::new() };
        let mut wave = div().flex().items_center().gap(px(BAR_GAP)).h(px(BAR_H));
        for (i, v) in bars.into_iter().enumerate() {
            let played = (i as f32 + 0.5) / n as f32 <= done && (playing || pos > 0.);
            let (fid, path) = (file.id.clone(), saved.clone());
            wave = wave.child(
                div()
                    .id(SharedString::from(format!("bar-{msg}-{i}")))
                    .h_full()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(div().w(px(BAR_W)).h(px(3. + v * (BAR_H - 3.))).bg(if played { t.accent } else { t.text.opacity(0.3) }))
                    .when_some(path, |d, path| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.seek_media(&fid, &path, i as f64 / n as f64, cx);
                        }))
                    }),
            );
        }
        let player = div()
            .flex()
            .items_center()
            .gap(px(S2))
            .py(px(2.))
            .child(round(SharedString::from(format!("play-{msg}")), if playing { G_PAUSE } else { G_PLAY }, 32., t.bright, t.bar).on_click(self.play_click(conv, msg, file, saved.clone(), cx)))
            .child(wave)
            .child(div().min_w(px(34.)).text_size(px(t.size - 2.)).text_color(t.muted).child(time));
        div().flex().flex_col().child(player).children(self.transcript_line(t, msg, &file.id, saved.as_deref(), 300., cx)).into_any_element()
    }

    /// The words under a voice message: being made, made, or why not (with a retry).
    fn transcript_line(&self, t: &Theme, msg: &str, file_id: &str, saved: Option<&std::path::Path>, width: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = saved?.to_owned();
        let small = |d: gpui::Div| d.mt(px(S1)).w(px(width)).text_size(px(t.size - 2.));
        match self.transcripts.get(file_id) {
            Some(None) => Some(small(div()).text_color(t.muted).italic().child("transcribing…").into_any_element()),
            Some(Some(Ok(text))) => Some(div().mt(px(S1)).w(px(width)).text_color(t.bright).child(text.clone()).into_any_element()),
            Some(Some(Err(e))) => {
                let fid = file_id.to_owned();
                Some(
                    small(div())
                        .flex()
                        .items_center()
                        .gap(px(S1))
                        .child(div().flex_1().text_color(t.red).child(format!("not transcribed: {e}")))
                        .child(button(SharedString::from(format!("tr-{msg}")), G_COMMENT, "again", t, false).on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.transcribe(&fid, &path, cx);
                        })))
                        .into_any_element(),
                )
            }
            None if crate::media::transcriber(&self.quick.transcribe).is_some() => None,
            None => None,
        }
    }

    /// The subtitle at `pos` of a message.
    fn subtitle(&self, file_id: &str, pos: f64) -> Option<String> {
        crate::media::cue_at(self.cues.get(file_id)?, pos).map(str::to_owned)
    }

    #[allow(clippy::too_many_arguments)]
    fn video_bubble(
        &self,
        t: &Theme,
        conv: &str,
        msg: &str,
        file: &FileRef,
        saved: Option<std::path::PathBuf>,
        view: Option<MediaView>,
        inner: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (playing, pos) = self.media_position(&file.id);
        let (vw, vh) = view.as_ref().and_then(|v| v.info.video).map_or((16., 9.), |(w, h)| (w as f32, h as f32));
        let mut w = inner.min(300.);
        let mut h = w * vh / vw;
        if h > 380. {
            h = 380.;
            w = h * vw / vh;
        }
        let duration = view.as_ref().map_or(0., |v| v.info.duration);
        let ours = self.player.as_ref().is_some_and(|p| p.file_id == file.id);
        let frame = self.player_frame.as_ref().filter(|_| ours && (playing || pos > 0.)).map(|f| f.image.clone());
        let thumb = view.as_ref().and_then(|v| v.thumb.clone());
        let arriving = saved.is_none() && self.downloads.contains_key(&file.id);
        let time = match (pos > 0., duration > 0.) {
            (true, true) => format!("{} / {}", clock(pos), clock(duration)),
            (false, true) => clock(duration),
            _ if arriving => "arriving…".into(),
            _ => String::new(),
        };
        let sub = if ours { self.subtitle(&file.id, pos) } else { None };
        let (fid, path) = (file.id.clone(), saved.clone());
        let video = div()
            .id(SharedString::from(format!("video-{msg}")))
            .relative()
            .w(px(w))
            .h(px(h))
            
            .overflow_hidden()
            .bg(gpui::black())
            .cursor_pointer()
            .map(|d| match (frame, thumb) {
                (Some(image), _) => d.child(frame_canvas(image)),
                (None, Some(thumb)) => d.child(gpui::img(thumb).size_full().object_fit(ObjectFit::Cover)),
                (None, None) => d,
            })
            .on_click(self.play_click(conv, msg, file, saved.clone(), cx))
            // The big play button while it is not playing.
            .when(!playing, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(round(SharedString::from(format!("vplay-{msg}")), G_PLAY, 48., gpui::black().opacity(0.55), gpui::white()).on_click(self.play_click(conv, msg, file, saved.clone(), cx))),
                )
            })
            // The corner button: fill the window.
            .when_some(path, |d, path| {
                d.child(
                    div().absolute().top(px(6.)).right(px(6.)).child(
                        round(SharedString::from(format!("vbig-{msg}")), G_EXPAND, 26., gpui::black().opacity(0.55), gpui::white()).on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            let ours = this.player.as_ref().is_some_and(|p| p.file_id == fid);
                            if !ours || !this.player.as_ref().is_some_and(|p| p.playing()) {
                                this.toggle_play(&fid, &path, cx);
                            }
                            this.video_big = true;
                            cx.notify();
                        })),
                    ),
                )
            })
            .when(!time.is_empty(), |d| d.child(div().absolute().top(px(6.)).left(px(6.)).child(chip(time))))
            .when_some(sub, |d, s| {
                d.child(
                    div().absolute().bottom(px(10.)).left_0().right_0().flex().justify_center().px(px(8.)).child(
                        div().px(px(6.)).py(px(2.)).bg(gpui::black().opacity(0.65)).text_color(gpui::white()).text_size(px(t.size - 1.)).text_center().child(s),
                    ),
                )
            })
            // How far it is, along the bottom.
            .when(duration > 0. && pos > 0., |d| {
                d.child(div().absolute().bottom_0().left_0().h(px(3.)).w(px(w * (pos / duration).min(1.) as f32)).bg(t.accent))
            });
        let status = self.transcripts.get(&file.id).and_then(|r| match r {
            None => Some(div().mt(px(S1)).text_size(px(t.size - 2.)).text_color(t.muted).italic().child("transcribing…")),
            Some(Err(e)) => Some(div().mt(px(S1)).w(px(w)).text_size(px(t.size - 2.)).text_color(t.red).child(format!("not transcribed: {e}"))),
            Some(Ok(_)) => None,
        });
        div().flex().flex_col().child(video).children(status).into_any_element()
    }

    /// The playing video filling the window: bigger subtitles, play and pause, a bar to
    /// jump with, back to the chat (Esc, or the corner button).
    pub(super) fn video_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.video_big {
            return None;
        }
        let p = self.player.as_ref().filter(|p| p.info.video.is_some())?;
        let (fid, path, playing, pos, duration) = (p.file_id.clone(), p.path.clone(), p.playing(), p.position(), p.info.duration);
        let frame = self.player_frame.as_ref().map(|f| f.image.clone());
        let thumb = self.media.get(&fid).cloned().flatten().and_then(|v| v.thumb);
        let sub = self.subtitle(&fid, pos);
        let close = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            cx.stop_propagation();
            this.video_big = false;
            cx.notify();
        });
        let (fid2, path2) = (fid.clone(), path.clone());
        let mut bar = div().flex_1().flex().h(px(18.)).items_center().cursor_pointer();
        const STEPS: usize = 100;
        for i in 0..STEPS {
            let (f, pa) = (fid.clone(), path.clone());
            let lit = duration > 0. && (i as f64 + 0.5) / STEPS as f64 <= pos / duration;
            bar = bar.child(
                div()
                    .id(SharedString::from(format!("vseek-{i}")))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(div().w_full().h(px(4.)).bg(if lit { t.accent } else { gpui::white().opacity(0.25) }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.seek_media(&f, &pa, i as f64 / STEPS as f64, cx);
                    })),
            );
        }
        Some(
            div()
                .id("video-big")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .bg(gpui::black())
                .occlude()
                .child(
                    div()
                        .id("video-big-picture")
                        .relative()
                        .flex_1()
                        .min_h(px(0.))
                        .m(px(S3))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_play(&fid2, &path2, cx)))
                        .map(|d| match (frame, thumb) {
                            (Some(image), _) => d.child(frame_canvas(image)),
                            (None, Some(thumb)) => d.child(gpui::img(thumb).size_full().object_fit(ObjectFit::Contain)),
                            (None, None) => d,
                        })
                        .when_some(sub, |d, s| {
                            d.child(
                                div().absolute().bottom(px(24.)).left_0().right_0().flex().justify_center().child(
                                    div().max_w(px(900.)).px(px(10.)).py(px(4.)).bg(gpui::black().opacity(0.7)).text_color(gpui::white()).text_size(px(t.size + 6.)).text_center().child(s),
                                ),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(S2))
                        .h(px(48.))
                        .px(px(S3))
                        .child(round("video-big-play", if playing { G_PAUSE } else { G_PLAY }, 34., t.accent, t.bg).on_click(cx.listener(move |this, _, _, cx| this.toggle_play(&fid, &path, cx))))
                        .child(div().text_color(gpui::white()).text_size(px(t.size - 1.)).child(format!("{} / {}", clock(pos), clock(duration))))
                        .child(bar)
                        .child(round("video-big-close", G_COMPRESS, 30., gpui::white().opacity(0.15), gpui::white()).on_click(close)),
                )
                .into_any_element(),
        )
    }
}
