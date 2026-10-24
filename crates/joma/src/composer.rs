//! The message input: multiline, IME and clipboard aware. Drafts stay with their
//! conversation. Adapted from Vetro's chat composer.
use gpui::prelude::*;
use gpui::{
    canvas, div, px, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, MouseButton, Pixels, Point, Render, SharedString, UTF16Selection, Window,
};
use std::collections::HashMap;
use std::ops::Range;
use crate::theme::Theme;

pub struct Submit;
/// The text changed (for the "typing" indicator).
pub struct Edited;
pub struct Composer {
    focus: FocusHandle,
    text: String,
    selected: Range<usize>,
    reversed: bool,
    layout: Option<gpui::TextLayout>,
    marked: Option<Range<usize>>,
    key: String,
    drafts: HashMap<String, String>,
    theme: Option<std::rc::Rc<Theme>>,
    enabled: bool,
    bounds: Option<Bounds<Pixels>>,
    placeholder: String,
}
impl EventEmitter<Submit> for Composer {}
impl EventEmitter<Edited> for Composer {}
impl Focusable for Composer {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Composer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            selected: 0..0,
            reversed: false,
            layout: None,
            marked: None,
            key: String::new(),
            drafts: HashMap::new(),
            theme: None,
            enabled: false,
            bounds: None,
            placeholder: "Write a message…".into(),
        }
    }
    pub fn configure(&mut self, key: String, theme: std::rc::Rc<Theme>, enabled: bool) {
        if key != self.key {
            self.drafts.insert(self.key.clone(), std::mem::take(&mut self.text));
            self.text = self.drafts.remove(&key).unwrap_or_default();
            self.selected = self.text.len()..self.text.len();
            self.marked = None;
            self.reversed = false;
            self.key = key;
        }
        self.theme = Some(theme);
        self.enabled = enabled;
    }
    pub fn set_placeholder(&mut self, p: impl Into<String>) {
        self.placeholder = p.into();
    }
    pub fn text(&self) -> String {
        self.text.clone()
    }
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.text.clear();
        self.selected = 0..0;
        self.marked = None;
        cx.notify();
    }
    fn replace(&mut self, range: Option<Range<usize>>, text: &str, marked: bool, cx: &mut Context<Self>) {
        let range = range
            .map(|r| byte(&self.text, r.start)..byte(&self.text, r.end))
            .or_else(|| self.marked.take())
            .unwrap_or(self.selected.clone());
        self.text.replace_range(range.clone(), text);
        let end = range.start + text.len();
        self.selected = end..end;
        self.reversed = false;
        self.marked = (marked && !text.is_empty()).then_some(range.start..end);
        cx.emit(Edited);
        cx.notify();
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.enabled && self.marked.is_none() && !self.text.trim().is_empty() {
            cx.emit(Submit);
        }
    }
    fn key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control || k.modifiers.platform;
        if ctrl {
            match k.key.as_str() {
                "a" => self.selected = 0..self.text.len(),
                "c" | "x" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.text[self.selected.clone()].into()));
                    if k.key == "x" {
                        self.replace(None, "", false, cx);
                    }
                }
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                        self.replace(None, &text, false, cx);
                    }
                }
                _ => return,
            }
        } else {
            if self.marked.is_some() {
                return;
            } // Let the input method finish its composition.
            match k.key.as_str() {
                "enter" if k.modifiers.shift => self.replace(None, "\n", false, cx),
                "enter" => self.submit(cx),
                "backspace" | "delete" => {
                    if self.selected.is_empty() {
                        let at = self.selected.start;
                        self.selected =
                            if k.key == "backspace" { prev(&self.text, at)..at } else { at..next(&self.text, at) };
                    }
                    self.replace(None, "", false, cx);
                }
                "left" | "right" | "home" | "end" | "up" | "down" => {
                    let at = if self.reversed { self.selected.start } else { self.selected.end };
                    let to = match k.key.as_str() {
                        "left" => prev(&self.text, at),
                        "right" => next(&self.text, at),
                        "home" => self.text[..at].rfind('\n').map_or(0, |i| i + 1),
                        "end" => self.text[at..].find('\n').map_or(self.text.len(), |i| at + i),
                        _ => self
                            .layout
                            .as_ref()
                            .and_then(|l| {
                                l.position_for_index(at).map(|mut p| {
                                    p.y += l.line_height() * if k.key == "up" { -1. } else { 1. };
                                    l.index_for_position(p).unwrap_or_else(|i| i).min(self.text.len())
                                })
                            })
                            .unwrap_or(at),
                    };
                    let anchor = if self.reversed { self.selected.end } else { self.selected.start };
                    self.selected = if k.modifiers.shift { anchor.min(to)..anchor.max(to) } else { to..to };
                    self.reversed = k.modifiers.shift && to < anchor;
                }
                _ => return, // Text comes through the platform input handler, including IME.
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}
fn prev(s: &str, at: usize) -> usize {
    s[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}
fn next(s: &str, at: usize) -> usize {
    s[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}
fn byte(s: &str, utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
    }
    s.len()
}
fn units(s: &str, at: usize) -> usize {
    s[..at].encode_utf16().count()
}
impl EntityInputHandler for Composer {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = byte(&self.text, r.start)..byte(&self.text, r.end);
        *actual = Some(units(&self.text, r.start)..units(&self.text, r.end));
        Some(self.text[r].into())
    }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: units(&self.text, self.selected.start)..units(&self.text, self.selected.end),
            reversed: self.reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|r| units(&self.text, r.start)..units(&self.text, r.end))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(&mut self, r: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.replace(r, text, false, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(r, text, true, cx);
        if let Some(selected) = selected {
            let start = self.marked.as_ref().unwrap().start;
            self.selected = start + byte(text, selected.start)..start + byte(text, selected.end);
        }
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let l = self.layout.as_ref()?;
        let p = l.position_for_index(byte(&self.text, r.start))?;
        Some(Bounds::new(p, gpui::size(px(1.), l.line_height())))
    }
    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let at = self.layout.as_ref()?.index_for_position(p).unwrap_or_else(|i| i).min(self.text.len());
        Some(units(&self.text, at))
    }
}
impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(theme) = self.theme.clone() else {
            return div().into_any_element();
        };
        let focused = self.focus.is_focused(window);
        let shown = if self.text.is_empty() {
            if focused { " " } else { self.placeholder.as_str() }.to_string()
        } else {
            self.text.clone()
        };
        let highlight = self.marked.clone().unwrap_or_else(|| self.selected.clone());
        let text =
            gpui::StyledText::new(SharedString::from(shown)).with_highlights((!highlight.is_empty()).then_some((
                highlight,
                gpui::HighlightStyle { background_color: Some(theme.accent.opacity(0.25)), ..Default::default() },
            )));
        let layout = text.layout().clone();
        self.layout = Some(layout.clone());
        let (focus, entity) = (self.focus.clone(), cx.entity());
        let caret = if self.reversed { self.selected.start } else { self.selected.end };
        let accent = theme.accent;
        let input = canvas(
            move |b, _, cx| {
                entity.update(cx, |this, _| this.bounds = Some(b));
                b
            },
            {
                let entity = cx.entity();
                move |bounds, _, window, cx| {
                    window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                    if focused {
                        if let Some(p) = layout.position_for_index(caret) {
                            window.paint_quad(gpui::fill(
                                Bounds::new(p, gpui::size(px(1.), layout.line_height())),
                                accent,
                            ));
                        }
                    }
                }
            },
        )
        .absolute()
        .size_full();
        div()
            .id("composer")
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("composer-input")
                    .track_focus(&self.focus)
                    .relative()
                    .min_h(px(theme.size * 1.6 + 14.))
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .px(px(10.))
                    .py(px(7.))
                    .border_1()
                    .border_color(if focused { theme.accent } else { theme.border })
                    .bg(theme.panel)
                    .text_color(if self.text.is_empty() { theme.muted } else { theme.bright })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &gpui::MouseDownEvent, window, cx| {
                            window.focus(&this.focus);
                            if let Some(layout) = &this.layout {
                                let at =
                                    layout.index_for_position(ev.position).unwrap_or_else(|i| i).min(this.text.len());
                                this.selected = at..at;
                                this.reversed = false;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .on_key_down(cx.listener(Self::key))
                    .child(text)
                    .child(input),
            )
            .into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_composition_indices_stay_on_boundaries() {
        let s = "A🦊è\n文";
        assert_eq!(byte(s, 3), 5);
        assert_eq!(units(s, 7), 4);
        assert_eq!(prev(s, 5), 1);
        assert_eq!(next(s, 1), 5);
    }
}
