//! Single-line text field: caret, selection, word/line movement, clipboard,
//! focus, placeholder, and horizontal auto-scroll. Editing logic (`Edit`) is
//! kept separate from the widget so it can be unit-tested and reused for a
//! future multi-line editor.

use crate::input::UiEvent as Event;
use crate::{Color, Cursor, Insets, Key, Layout, Modifiers, Rect, Response, Size, Ui, Vec2};

/// Retained per-field state. Indices are in chars, not bytes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TextState {
    cursor: usize,
    anchor: usize,
    scroll: f32,
    /// Time of the last caret move; the caret stays solid for a moment after it.
    last_move: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextResponse {
    pub response: Response,
    /// Text was modified this frame.
    pub changed: bool,
    /// Enter was pressed (focus is released).
    pub submitted: bool,
    pub focused: bool,
}

/// Pure editing operations on a `String` + caret/selection.
struct Edit<'a> {
    text: &'a mut String,
    cursor: usize,
    anchor: usize,
}

fn byte_at(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

impl Edit<'_> {
    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    fn has_selection(&self) -> bool {
        self.cursor != self.anchor
    }

    fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.text[byte_at(self.text, a)..byte_at(self.text, b)].to_string()
    }

    fn replace(&mut self, a: usize, b: usize, with: &str) {
        let (ba, bb) = (byte_at(self.text, a), byte_at(self.text, b));
        self.text.replace_range(ba..bb, with);
        self.cursor = a + with.chars().count();
        self.anchor = self.cursor;
    }

    fn insert(&mut self, s: &str) {
        let (a, b) = self.selection();
        self.replace(a, b, s);
    }

    fn word_left(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = from;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn word_right(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = from;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.cursor = to.min(self.len());
        if !extend {
            self.anchor = self.cursor;
        }
    }

    /// Apply one key. Returns true if the text changed.
    fn key(&mut self, key: Key, m: Modifiers) -> bool {
        let n = self.len();
        match key {
            Key::Backspace | Key::Delete if self.has_selection() => {
                self.insert("");
                true
            }
            Key::Backspace if self.cursor > 0 => {
                let from = if m.command { 0 } else if m.word { self.word_left(self.cursor) } else { self.cursor - 1 };
                self.replace(from, self.cursor, "");
                true
            }
            Key::Delete if self.cursor < n => {
                let to = if m.command { n } else if m.word { self.word_right(self.cursor) } else { self.cursor + 1 };
                self.replace(self.cursor, to, "");
                true
            }
            Key::ArrowLeft => {
                let to = if self.has_selection() && !m.shift {
                    self.selection().0
                } else if m.command {
                    0
                } else if m.word {
                    self.word_left(self.cursor)
                } else {
                    self.cursor.saturating_sub(1)
                };
                self.move_to(to, m.shift);
                false
            }
            Key::ArrowRight => {
                let to = if self.has_selection() && !m.shift {
                    self.selection().1
                } else if m.command {
                    n
                } else if m.word {
                    self.word_right(self.cursor)
                } else {
                    self.cursor + 1
                };
                self.move_to(to, m.shift);
                false
            }
            Key::Home | Key::ArrowUp => {
                self.move_to(0, m.shift);
                false
            }
            Key::End | Key::ArrowDown => {
                self.move_to(n, m.shift);
                false
            }
            Key::A if m.command => {
                self.anchor = 0;
                self.cursor = n;
                false
            }
            _ => false,
        }
    }
}

/// Single-line fields turn newlines/tabs into spaces and drop other control chars.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

impl Ui {
    /// Single-line text field. `key` must be unique within the container.
    pub fn text_input(&mut self, key: &str, text: &mut String, placeholder: &str) -> TextResponse {
        let id = self.make_id(("text_input", key));
        let st_style = self.theme.text_input;
        let (size, h, pad) = (self.theme.metrics.font_size, st_style.height, st_style.padding_x);
        let resp = self.interact_drag(id);
        if resp.hovered {
            self.cursor = Cursor::Text;
        }

        let mut st = self.text_states.get(&id).copied().unwrap_or_default();
        let mut carets = self.fonts.carets(self.font, size, text);
        let n = carets.len() - 1;
        st.cursor = st.cursor.min(n);
        st.anchor = st.anchor.min(n);
        let text_x = resp.rect.x + pad - st.scroll;
        let hit = |carets: &[f32], x: f32| -> usize {
            let local = x - text_x;
            let mut best = 0;
            for (i, c) in carets.iter().enumerate() {
                if (c - local).abs() < (carets[best] - local).abs() {
                    best = i;
                }
            }
            best
        };

        if resp.pressed {
            self.focused = Some(id);
            let i = hit(&carets, resp.mouse_pos.x);
            st.cursor = i;
            if !self.input.modifiers.shift {
                st.anchor = i;
            }
            st.last_move = self.time;
        } else if resp.active && self.mouse_delta.x != 0.0 {
            st.cursor = hit(&carets, resp.mouse_pos.x); // drag-select
            st.last_move = self.time;
        }

        let mut changed = false;
        let mut submitted = false;
        if self.focused == Some(id) {
            let events = self.input.events.clone();
            let mut e = Edit { text, cursor: st.cursor, anchor: st.anchor };
            for ev in events {
                match ev {
                    Event::Text(s) | Event::Paste(s) => {
                        let s = sanitize(&s);
                        if !s.is_empty() {
                            e.insert(&s);
                            changed = true;
                        }
                    }
                    Event::Copy if e.has_selection() => self.copied = Some(e.selected_text()),
                    Event::Cut if e.has_selection() => {
                        self.copied = Some(e.selected_text());
                        e.insert("");
                        changed = true;
                    }
                    Event::Key(Key::Enter, _) => {
                        submitted = true;
                        self.focused = None;
                    }
                    Event::Key(Key::Escape, _) => self.focused = None,
                    Event::Key(k, m) => changed |= e.key(k, m),
                    _ => continue,
                }
                st.last_move = self.time;
            }
            st.cursor = e.cursor;
            st.anchor = e.anchor;
            if changed {
                carets = self.fonts.carets(self.font, size, e.text);
            }
        }
        let focused = self.focused == Some(id);
        self.focus_order.push(id);
        if !focused {
            st.anchor = st.cursor; // collapse selection when focus leaves
        }

        // Keep the caret in view.
        let visible_w = (resp.rect.w - 2.0 * pad).max(0.0);
        let total_w = *carets.last().unwrap();
        let cx = carets[st.cursor];
        if resp.rect.w > 0.0 {
            if cx - st.scroll > visible_w {
                st.scroll = cx - visible_w;
            }
            if cx - st.scroll < 0.0 {
                st.scroll = cx;
            }
            st.scroll = st.scroll.clamp(0.0, (total_w - visible_w + 1.0).max(0.0));
        }
        self.text_states.insert(id, st);

        let line_h = self.fonts.line_height(self.font, size);
        if focused {
            let x = resp.rect.x + pad + cx - st.scroll;
            self.ime_rect = Some(Rect::new(x, resp.rect.center().y - line_h * 0.5, 1.0, line_h));
        }

        let focus_t = self.animate_bool(id, 0, focused);
        let hover_t = self.animate_bool(id, 1, resp.hovered);
        let caret_on = focused && (((self.time - st.last_move) * 1.8) as i64 % 2 == 0);
        let (sa, sb) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        let sel = (carets[sa], carets[sb]);
        let shown = text.clone();
        let placeholder = placeholder.to_string();
        let scroll = st.scroll;

        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(h)).padding(Insets::xy(pad, 0.0));
        self.add_leaf(id, layout, Vec2::new(40.0, line_h), true, move |p, r| {
            let s = st_style;
            let radius = s.radius;
            if focus_t > 0.01 {
                p.rect_bordered(r.expand(3.0), Color::TRANSPARENT, radius + 3.0, 2.0, s.focus_ring.with_alpha(s.focus_ring.a * focus_t));
            }
            let border = s.border.lerp(s.border_hover, hover_t).lerp(s.border_focus, focus_t);
            p.rect_bordered(r, s.fill, radius, 1.0, border);

            let inner = r.shrink(pad, 0.0, pad, 0.0);
            let ty = r.center().y - line_h * 0.5;
            let x0 = inner.x - scroll;
            p.draw.push_clip(inner.expand(1.0));
            if sel.1 > sel.0 {
                let c = if focus_t > 0.5 { s.selection } else { s.selection.with_alpha(s.selection.a * 0.5) };
                p.rect(Rect::new(x0 + sel.0, ty, sel.1 - sel.0, line_h), c, 2.0);
            }
            if shown.is_empty() {
                p.text(Vec2::new(inner.x, ty), size, s.placeholder, &placeholder);
            } else {
                p.text(Vec2::new(x0, ty), size, s.text, &shown);
            }
            if caret_on {
                p.rect(Rect::new((x0 + cx).round() - 0.75, ty - 1.0, 1.5, line_h + 2.0), s.caret, 0.75);
            }
            p.draw.pop_clip();
        });

        TextResponse { response: resp, changed, submitted, focused }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(s: &str, cursor: usize) -> (String, usize, usize) {
        (s.to_string(), cursor, cursor)
    }

    fn run(state: (String, usize, usize), keys: &[(Key, Modifiers)]) -> (String, usize, usize) {
        let (mut text, cursor, anchor) = state;
        let mut e = Edit { text: &mut text, cursor, anchor };
        for &(k, m) in keys {
            e.key(k, m);
        }
        let (c, a) = (e.cursor, e.anchor);
        (text, c, a)
    }

    const NONE: Modifiers = Modifiers { shift: false, ctrl: false, alt: false, logo: false, command: false, word: false };
    const SHIFT: Modifiers = Modifiers { shift: true, ..NONE };
    const WORD: Modifiers = Modifiers { word: true, ..NONE };
    const CMD: Modifiers = Modifiers { command: true, ..NONE };

    #[test]
    fn backspace_and_delete() {
        assert_eq!(run(edit("hello", 5), &[(Key::Backspace, NONE)]).0, "hell");
        assert_eq!(run(edit("hello", 0), &[(Key::Delete, NONE)]).0, "ello");
        assert_eq!(run(edit("hello", 0), &[(Key::Backspace, NONE)]).0, "hello");
    }

    #[test]
    fn word_movement_and_deletion() {
        let r = run(edit("move the cube", 13), &[(Key::Backspace, WORD)]);
        assert_eq!(r.0, "move the ");
        let r = run(edit("move the cube", 0), &[(Key::ArrowRight, WORD), (Key::ArrowRight, WORD)]);
        assert_eq!(r.1, 8);
        let r = run(edit("move the cube", 9), &[(Key::Backspace, CMD)]);
        assert_eq!(r.0, "cube");
    }

    #[test]
    fn selection_replace_and_unicode() {
        // Select "wörld" with shift+word-left, then type over it.
        let (mut text, cursor, anchor) = run(edit("héllo wörld", 11), &[(Key::ArrowLeft, Modifiers { shift: true, word: true, ..NONE })]);
        assert_eq!((cursor, anchor), (6, 11));
        let mut e = Edit { text: &mut text, cursor, anchor };
        assert_eq!(e.selected_text(), "wörld");
        e.insert("libgui");
        assert_eq!(text, "héllo libgui");
    }

    #[test]
    fn arrows_collapse_selection() {
        let r = run(edit("abcdef", 2), &[(Key::ArrowRight, SHIFT), (Key::ArrowRight, SHIFT), (Key::ArrowLeft, NONE)]);
        assert_eq!((r.1, r.2), (2, 2));
        let r = run(edit("abcdef", 3), &[(Key::A, CMD)]);
        assert_eq!((r.1, r.2), (6, 0));
    }

    /// Drives the real widget through `Ui` with host-style events.
    #[test]
    fn ui_focus_typing_clipboard_and_tab() {
        use crate::{FrameInfo, InputEvent as IE, PlatformOutput, PointerButton, Theme};
        let mut ui = Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).unwrap();
        ui.set_mac_shortcuts(true);
        let (mut a, mut b) = (String::new(), String::from("second"));
        let mut frame = |ui: &mut Ui, events: Vec<IE>| -> (TextResponse, TextResponse, PlatformOutput) {
            for e in events {
                ui.push(e);
            }
            ui.begin_frame(FrameInfo::default());
            let ra = ui.text_input("a", &mut a, "type here");
            let rb = ui.text_input("b", &mut b, "");
            let out = ui.end_frame().platform;
            (ra, rb, out)
        };
        let key = |key: Key, pressed: bool| IE::Key { key, pressed, repeat: false };
        let at = Vec2::new(40.0, 15.0);
        frame(&mut ui, vec![IE::PointerMoved { pos: at }]); // layout pass so rects exist

        // Click field A (a press and release inside one frame still counts) and type.
        let click = vec![
            IE::PointerButton { button: PointerButton::Primary, pressed: true },
            IE::PointerButton { button: PointerButton::Primary, pressed: false },
        ];
        let (ra, _, out) = frame(&mut ui, click);
        assert!(ra.focused && out.wants_keyboard && out.text_input.is_some());
        let (ra, _, _) = frame(&mut ui, vec![IE::Text("hello libgui".into())]);
        assert!(ra.changed);

        // Cmd+A, Cmd+X from physical keys: the cut text is handed to the host.
        let (_, _, out) = frame(
            &mut ui,
            vec![key(Key::SuperLeft, true), key(Key::A, true), key(Key::A, false), key(Key::X, true), key(Key::X, false)],
        );
        assert_eq!(out.copied_text.as_deref(), Some("hello libgui"));

        // Cmd+V asks the host for the clipboard; the host answers with Paste.
        let (_, _, out) = frame(&mut ui, vec![key(Key::V, true), key(Key::V, false), key(Key::SuperLeft, false)]);
        assert!(out.paste_requested);
        frame(&mut ui, vec![IE::Paste("pasted".into())]);

        // Text typed while Cmd is held is a shortcut, not input.
        frame(&mut ui, vec![key(Key::SuperLeft, true), IE::Text("s".into()), key(Key::SuperLeft, false)]);

        // Tab moves focus to B; Enter submits and releases focus.
        frame(&mut ui, vec![key(Key::Tab, true)]);
        let (ra, rb, _) = frame(&mut ui, vec![key(Key::Tab, false)]);
        assert!(!ra.focused && rb.focused);
        let (_, rb, out) = frame(&mut ui, vec![key(Key::Enter, true)]);
        assert!(rb.submitted && !out.wants_keyboard);
        drop(frame);
        assert_eq!(a, "pasted");
    }

    #[test]
    fn sanitize_single_line() {
        assert_eq!(sanitize("a\nb\tc\u{7}"), "a b c");
    }
}
