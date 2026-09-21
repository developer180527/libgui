//! Text fields: [`Ui::text_input`] for one line, [`Ui::text_area`] for many.
//!
//! Both are the same editor. `Edit` is the whole of it — a `String`, a caret,
//! an anchor and the motions between them — and it is line-aware, so a
//! single-line field is simply one whose text has no newlines in it. The
//! widgets differ in how they lay it out, scroll it and hit-test it.
//!
//! Undo lives in `text_history`, which explains why a UI library has one at
//! all: it is the *field's* undo, not the app's document history, and the two
//! never meet.

use crate::input::UiEvent as Event;
use crate::text_history::{Change, Edited, History};
use crate::{FrameText, Id};
use crate::{Color, Cursor, Insets, Layout, Motion, Rect, Response, Size, Ui, UiAction, Vec2};

/// Retained per-field state. Indices are in chars, not bytes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TextState {
    cursor: usize,
    anchor: usize,
    scroll: f32,
    /// Vertical scroll, for a text area.
    scroll_y: f32,
    /// Time of the last caret move; the caret stays solid for a moment after it.
    last_move: f64,
    /// The column Up/Down aim for. Walking past a short line and back must
    /// return to the column you started in, so the *intended* column is
    /// remembered rather than recomputed from the caret each time.
    goal: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextResponse {
    pub response: Response,
    /// Text was modified this frame.
    pub changed: bool,
    /// Enter was pressed (focus is released).
    pub submitted: bool,
    pub focused: bool,
    /// Where the caret is, as `(line, column)` — both zero-based, both counted
    /// in `char`s, not bytes. A single-line field is always on line 0.
    ///
    /// Only the field knows this, and a status bar ("Ln 12, Col 4"), a
    /// line-scoped command, or a transform over the selection all need it.
    pub caret: (usize, usize),
    /// The selected range as `(start, end)` char indices, ordered, equal when
    /// nothing is selected. Slice the same `String` you passed in with it.
    pub selection: (usize, usize),
    /// Has typing of its own to take back. When this is false the undo chord
    /// is released to the app, so an Edit menu can say whose undo it is
    /// instead of guessing from focus.
    pub can_undo: bool,
    pub can_redo: bool,
}

/// Pure editing operations on a `String` + caret/selection.
struct Edit<'a> {
    text: &'a mut String,
    cursor: usize,
    anchor: usize,
    /// What the last mutation did, for the undo history. Every edit goes
    /// through `replace`, so this is the one place a change is described —
    /// and describing it here is what lets the history store the edit rather
    /// than a copy of the whole document.
    change: Option<Change>,
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
        if ba == bb && with.is_empty() {
            return; // nothing happened: backspace at the start, delete at the end
        }
        self.change = Some(Change { at: a, removed: self.text[ba..bb].to_string(), inserted: with.to_string() });
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

    /// First char of the line `pos` is on.
    fn line_start(&self, pos: usize) -> usize {
        let mut start = 0;
        for (i, c) in self.text.chars().enumerate().take(pos) {
            if c == '\n' {
                start = i + 1;
            }
        }
        start
    }

    /// The newline that ends `pos`'s line, or the end of the text.
    fn line_end(&self, pos: usize) -> usize {
        self.text.chars().enumerate().skip(pos).find(|(_, c)| *c == '\n').map_or(self.len(), |(i, _)| i)
    }

    /// The same column on the line above, clamped to its length. `None` when
    /// there is no line above — a single-line field is always in that case.
    fn line_above(&self, pos: usize, col: usize) -> Option<usize> {
        let start = self.line_start(pos);
        if start == 0 {
            return None;
        }
        let prev_start = self.line_start(start - 1);
        Some((prev_start + col).min(start - 1))
    }

    fn line_below(&self, pos: usize, col: usize) -> Option<usize> {
        let end = self.line_end(pos);
        if end >= self.len() {
            return None;
        }
        let next_start = end + 1;
        let next_end = self.line_end(next_start);
        Some((next_start + col).min(next_end))
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.cursor = to.min(self.len());
        if !extend {
            self.anchor = self.cursor;
        }
    }

    /// Where `motion` takes the caret. `goal` is the column Up and Down aim
    /// for; with no line to move to they fall back to the ends of the text,
    /// which is what a single-line field wants from them.
    fn target(&self, motion: Motion, goal: Option<usize>) -> usize {
        let n = self.len();
        let col = || goal.unwrap_or_else(|| self.cursor - self.line_start(self.cursor));
        match motion {
            Motion::Left => self.cursor.saturating_sub(1),
            Motion::Right => (self.cursor + 1).min(n),
            Motion::WordLeft => self.word_left(self.cursor),
            Motion::WordRight => self.word_right(self.cursor),
            Motion::LineStart => self.line_start(self.cursor),
            Motion::LineEnd => self.line_end(self.cursor),
            Motion::Up => self.line_above(self.cursor, col()).unwrap_or(0),
            Motion::Down => self.line_below(self.cursor, col()).unwrap_or(n),
            Motion::DocStart => 0,
            Motion::DocEnd => n,
        }
    }

    /// Apply one action. Returns true if the text changed.
    ///
    /// `goal` is the column Up/Down aim for: kept by the caller across
    /// actions, set by a vertical move and cleared by anything else.
    fn action(&mut self, action: UiAction, goal: &mut Option<usize>) -> bool {
        // Only vertical motion keeps a goal column; everything else decides a
        // new one by where the caret lands.
        let vertical = matches!(action, UiAction::Move { motion: Motion::Up | Motion::Down, .. });
        if !vertical {
            *goal = None;
        }
        match action {
            UiAction::Move { motion, select } => {
                // A plain arrow with a selection lands on its near edge
                // rather than moving from the caret.
                let to = match motion {
                    Motion::Left if self.has_selection() && !select => self.selection().0,
                    Motion::Right if self.has_selection() && !select => self.selection().1,
                    m => self.target(m, *goal),
                };
                if vertical && goal.is_none() {
                    *goal = Some(self.cursor - self.line_start(self.cursor));
                }
                self.move_to(to, select);
                false
            }
            UiAction::Delete(_) if self.has_selection() => {
                self.insert("");
                true
            }
            UiAction::Delete(motion) => {
                let to = self.target(motion, None);
                if to == self.cursor {
                    return false;
                }
                self.replace(self.cursor.min(to), self.cursor.max(to), "");
                true
            }
            UiAction::SelectAll => {
                self.anchor = 0;
                self.cursor = self.len();
                false
            }
            _ => false,
        }
    }
}

/// Control characters a field cannot show. A single-line field turns newlines
/// and tabs into spaces; a text area keeps newlines and drops the rest, since
/// it has somewhere to put a line break and nowhere to put a bell.
/// Strip what a field cannot hold. A text area keeps newlines and **tabs** —
/// flattening a tab to a space destroys the indentation of any pasted code,
/// and turns a pasted Makefile into one that does not build. A single-line
/// field has nowhere to put either, so both become a space rather than
/// vanishing and joining two words.
fn sanitize(s: &str, multiline: bool) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\n' | '\t' if multiline => Some(c),
            '\r' if multiline => None,
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Apply one edit and record it. The caret before the edit is what undo puts
/// back, and an action that changed nothing (backspace at the start) records
/// nothing — there is no step to take back.
fn edited(e: &mut Edit, hist: &mut History, kind: Edited, now: f64, body: impl FnOnce(&mut Edit)) -> bool {
    let (cursor, anchor) = (e.cursor, e.anchor);
    e.change = None;
    body(e);
    match e.change.take() {
        Some(change) => {
            hist.record(kind, change, cursor, anchor, now);
            true
        }
        None => false,
    }
}

/// What a frame of editing did.
struct Edited2 {
    changed: bool,
    submitted: bool,
    can_undo: bool,
    can_redo: bool,
}

/// Apply this frame's text, clipboard and action events to `text`.
///
/// Shared by both fields: the single-line one differs only in refusing
/// newlines, and in treating Enter as "commit" because it has nowhere to put
/// a line break.
fn apply_events(ui: &mut Ui, id: Id, text: &mut String, st: &mut TextState, multiline: bool) -> Edited2 {
    let mut out = Edited2 { changed: false, submitted: false, can_undo: false, can_redo: false };
    let mut hist = ui.text_history.remove(&id).unwrap_or_default();
    // No whole-document comparison here to spot the app writing the string
    // behind the field's back: a step is checked against the buffer when it is
    // applied instead, which costs the length of the edit rather than the
    // length of the document. See `text_history`.
    let now = ui.time;
    let events = ui.input.events.clone();
    let mut e = Edit { text, cursor: st.cursor, anchor: st.anchor, change: None };
    for ev in events {
        match ev {
            Event::Text(s) => {
                let s = sanitize(&s, multiline);
                if !s.is_empty() {
                    out.changed |= edited(&mut e, &mut hist, Edited::Typing, now, |e| e.insert(&s));
                }
            }
            Event::Paste(s) => {
                let s = sanitize(&s, multiline);
                if !s.is_empty() {
                    // A paste is one step whatever it lands next to.
                    out.changed |= edited(&mut e, &mut hist, Edited::Discrete, now, |e| e.insert(&s));
                }
            }
            Event::Action(UiAction::Copy) if e.has_selection() => ui.copied = Some(e.selected_text()),
            Event::Action(UiAction::Cut) if e.has_selection() => {
                ui.copied = Some(e.selected_text());
                out.changed |= edited(&mut e, &mut hist, Edited::Discrete, now, |e| e.insert(""));
            }
            Event::Action(UiAction::InsertNewline) => {
                if multiline {
                    out.changed |= edited(&mut e, &mut hist, Edited::Typing, now, |e| e.insert("\n"));
                } else {
                    // Nowhere to put a line break: Enter commits instead.
                    out.submitted = true;
                    ui.focused = None;
                }
            }
            Event::Action(UiAction::Submit) => {
                out.submitted = true;
                ui.focused = None;
            }
            Event::Action(UiAction::Cancel) => ui.focused = None,
            // Undo and redo the field cannot serve are *released*: the action
            // is reported as unclaimed, so `consume_shortcut` lets the chord
            // through to the app. A caret resting in a search box must not
            // make the app's own undo unreachable.
            Event::Action(UiAction::Undo) => {
                match hist.undo(e.text, e.cursor, e.anchor) {
                    Some((c, a)) => {
                        e.cursor = c.min(e.len());
                        e.anchor = a.min(e.len());
                        out.changed = true;
                    }
                    None => ui.release_action(UiAction::Undo),
                }
            }
            Event::Action(UiAction::Redo) => {
                match hist.redo(e.text, e.cursor, e.anchor) {
                    Some((c, a)) => {
                        e.cursor = c.min(e.len());
                        e.anchor = a.min(e.len());
                        out.changed = true;
                    }
                    None => ui.release_action(UiAction::Redo),
                }
            }
            Event::Action(a @ UiAction::Delete(_)) => {
                out.changed |= edited(&mut e, &mut hist, Edited::Deleting, now, |e| {
                    e.action(a, &mut None);
                });
            }
            Event::Action(a @ (UiAction::Move { .. } | UiAction::SelectAll)) => {
                // A caret move ends the run: what is typed next is its own step.
                hist.break_run();
                out.changed |= e.action(a, &mut st.goal);
            }
            _ => continue,
        }
        st.last_move = ui.time;
    }
    st.cursor = e.cursor;
    st.anchor = e.anchor;
    out.can_undo = hist.can_undo();
    out.can_redo = hist.can_redo();
    ui.text_history.insert(id, hist);
    out
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
            // Clicking ends the typing run: the next characters are their own
            // undo step, wherever the caret just went.
            if let Some(h) = self.text_history.get_mut(&id) {
                h.break_run();
            }
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
        let (mut can_undo, mut can_redo) = (false, false);
        if self.focused == Some(id) {
            let out = apply_events(self, id, text, &mut st, false);
            changed = out.changed;
            submitted = out.submitted;
            (can_undo, can_redo) = (out.can_undo, out.can_redo);
            if changed {
                carets = self.fonts.carets(self.font, size, text);
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
        // What an input method is composing sits at the caret, without being
        // part of the field's value: it belongs to the IME until it commits.
        let composing = if focused { self.preedit().map(|(t, c)| (t.to_string(), c)) } else { None };
        let (pre_w, pre_caret, pre_text) = match &composing {
            Some((t, c)) => (
                self.fonts.measure(self.font, size, t).x,
                self.fonts.measure(self.font, size, &t[..*c]).x,
                Some(self.frame_text(t)),
            ),
            None => (0.0, 0.0, None),
        };
        if focused {
            // The IME wants the rect of what it is composing, so its candidate
            // window can sit under it rather than under the caret it started at.
            let x = resp.rect.x + pad + cx - st.scroll;
            self.ime_rect = Some(Rect::new(x, resp.rect.center().y - line_h * 0.5, pre_w.max(1.0), line_h));
        }

        let focus_t = self.animate_bool(id, 0, focused);
        let hover_t = self.animate_bool(id, 1, resp.hovered);
        let caret_on = focused && (((self.time - st.last_move) * 1.8) as i64 % 2 == 0);
        let (sa, sb) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        let sel = (carets[sa], carets[sb]);
        let shown = text.clone();
        // Split at the caret, so the composing text can be drawn between them.
        let head = shown[..byte_at(&shown, st.cursor)].to_string();
        let tail = shown[byte_at(&shown, st.cursor)..].to_string();
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
            if shown.is_empty() && pre_text.is_none() {
                p.text(Vec2::new(inner.x, ty), size, s.placeholder, &placeholder);
            } else if let Some(pre) = pre_text {
                // Three runs: what is committed before the caret, what the IME
                // is composing, and what is committed after it.
                p.text(Vec2::new(x0, ty), size, s.text, &head);
                let px = x0 + sel.0;
                p.text(Vec2::new(px, ty), size, s.text, pre);
                p.text(Vec2::new(px + pre_w, ty), size, s.text, &tail);
                // Underlined, which is how every platform says "not committed".
                let u = p.hairline(px, ty + line_h - 2.0, 1.0, 1.0);
                p.rect(Rect::new(u.x, u.y, pre_w, u.w), s.text, 0.0);
            } else {
                p.text(Vec2::new(x0, ty), size, s.text, &shown);
            }
            if caret_on {
                // While composing, the caret belongs to the IME's own cursor
                // inside the composing text, not to the field's.
                let at = x0 + cx + pre_caret;
                p.rect(Rect::new(at.round() - 0.75, ty - 1.0, 1.5, line_h + 2.0), s.caret, 0.75);
            }
            p.draw.pop_clip();
        });

        TextResponse { response: resp, changed, submitted, focused, caret: (0, st.cursor), selection: (sa, sb), can_undo, can_redo }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(s: &str, cursor: usize) -> (String, usize, usize) {
        (s.to_string(), cursor, cursor)
    }

    fn run(state: (String, usize, usize), actions: &[UiAction]) -> (String, usize, usize) {
        let (mut text, cursor, anchor) = state;
        let mut e = Edit { text: &mut text, cursor, anchor, change: None };
        for &a in actions {
            e.action(a, &mut None);
        }
        let (c, a) = (e.cursor, e.anchor);
        (text, c, a)
    }

    const fn mv(motion: Motion) -> UiAction {
        UiAction::Move { motion, select: false }
    }

    const fn sel(motion: Motion) -> UiAction {
        UiAction::Move { motion, select: true }
    }

    use Motion::*;
    use UiAction::Delete;

    #[test]
    fn backspace_and_delete() {
        assert_eq!(run(edit("hello", 5), &[Delete(Left)]).0, "hell");
        assert_eq!(run(edit("hello", 0), &[Delete(Right)]).0, "ello");
        assert_eq!(run(edit("hello", 0), &[Delete(Left)]).0, "hello");
    }

    #[test]
    fn word_movement_and_deletion() {
        let r = run(edit("move the cube", 13), &[Delete(WordLeft)]);
        assert_eq!(r.0, "move the ");
        let r = run(edit("move the cube", 0), &[mv(WordRight), mv(WordRight)]);
        assert_eq!(r.1, 8);
        let r = run(edit("move the cube", 9), &[Delete(LineStart)]);
        assert_eq!(r.0, "cube");
        let r = run(edit("move the cube", 4), &[Delete(LineEnd)]);
        assert_eq!(r.0, "move");
    }

    #[test]
    fn selection_replace_and_unicode() {
        // Select "wörld" with a selecting word-left, then type over it.
        let (mut text, cursor, anchor) = run(edit("héllo wörld", 11), &[sel(WordLeft)]);
        assert_eq!((cursor, anchor), (6, 11));
        let mut e = Edit { text: &mut text, cursor, anchor, change: None };
        assert_eq!(e.selected_text(), "wörld");
        e.insert("libgui");
        assert_eq!(text, "héllo libgui");
    }

    #[test]
    fn arrows_collapse_selection() {
        let r = run(edit("abcdef", 2), &[sel(Right), sel(Right), mv(Left)]);
        assert_eq!((r.1, r.2), (2, 2));
        let r = run(edit("abcdef", 3), &[UiAction::SelectAll]);
        assert_eq!((r.1, r.2), (6, 0));
        // Deleting with a selection removes the selection, whatever the motion.
        let r = run(edit("abcdef", 1), &[sel(Right), sel(Right), Delete(WordRight)]);
        assert_eq!(r.0, "adef");
    }

    /// Drives the real widget through `Ui` with host-style events.
    #[test]
    fn ui_focus_typing_clipboard_and_tab() {
        use crate::{FrameInfo, InputEvent as IE, Key, PlatformOutput, PointerButton, Theme};
        let mut ui = Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).unwrap();
        ui.set_key_bindings(crate::input::test_bindings());
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
        // Ends the closure's mutable borrow of `a` so it can be read.
        #[allow(clippy::drop_non_drop)]
        drop(frame);
        assert_eq!(a, "pasted");
    }

    #[test]
    fn sanitize_single_line() {
        assert_eq!(sanitize("a\nb\tc\u{7}", false), "a b c", "a single-line field flattens a newline");
        assert_eq!(sanitize("a\nb\r\tc\u{7}", true), "a\nb\tc", "a text area keeps newlines and tabs");
    }
}

/// How a [`Ui::text_area`] is laid out.
#[derive(Clone, Copy, Debug)]
pub struct TextAreaOptions {
    /// Height in lines, when `height` is `Size::Fit`.
    pub rows: usize,
    pub height: Size,
    /// Show a gutter of line numbers, as a script editor does.
    pub line_numbers: bool,
}

impl Default for TextAreaOptions {
    fn default() -> Self {
        Self { rows: 6, height: Size::Fit, line_numbers: false }
    }
}

/// Where one line of the text starts and ends, in chars.
struct Line {
    start: usize,
    end: usize,
}

/// One visible line, ready to paint: its y, its glyphs, the part of it that is
/// selected, and its number for the gutter.
struct Row {
    y: f32,
    text: FrameText,
    selection: Option<(f32, f32)>,
    number: FrameText,
}

/// The lines `text` is made of. One pass; a real editor keeps this index
/// between frames, which is the next thing to do here if a document gets big
/// enough to notice.
fn lines_of(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            out.push(Line { start, end: i });
            start = i + 1;
        }
    }
    out.push(Line { start, end: text.chars().count() });
    out
}

impl Ui {
    /// A multi-line text field: a console, a script, a note, a description.
    ///
    /// Enter breaks a line and Cmd/Ctrl+Enter commits, which is the
    /// convention everywhere a text box has more than one line. Undo is the
    /// field's own: it takes back typing, and never your document's history.
    /// While no field has focus the chord does not reach libgui at all, so
    /// your app's undo is what runs.
    ///
    /// Lines are hard: there is no word wrapping yet, and a long line scrolls
    /// sideways rather than folding.
    pub fn text_area(&mut self, key: &str, text: &mut String, rows: usize) -> TextResponse {
        self.text_area_with(key, text, TextAreaOptions { rows, ..Default::default() })
    }

    pub fn text_area_with(&mut self, key: &str, text: &mut String, opts: TextAreaOptions) -> TextResponse {
        let id = self.make_id(("text_area", key));
        let s = self.theme.text_input;
        let size = self.theme.metrics.font_size;
        let pad = s.padding_x;
        let lh = self.fonts.line_height(self.font, size);
        let resp = self.interact_drag(id);
        if resp.hovered {
            self.cursor = Cursor::Text;
        }
        let mut st = self.text_states.get(&id).copied().unwrap_or_default();

        // ---- geometry ----------------------------------------------------
        let gutter = if opts.line_numbers { (size * 2.6).ceil() } else { 0.0 };
        let view = Rect::new(resp.rect.x + pad + gutter, resp.rect.y + pad, (resp.rect.w - pad * 2.0 - gutter).max(1.0), (resp.rect.h - pad * 2.0).max(1.0));
        let lines = lines_of(text);
        let total = lines.len();
        let content_h = total as f32 * lh;

        // ---- pointer -------------------------------------------------------
        // Char index under a window point, which is what a click and a drag
        // both need.
        let index_at = |ui: &mut Ui, p: Vec2, lines: &[Line], text: &str| -> usize {
            let row = (((p.y - view.y + st.scroll_y) / lh).floor().max(0.0) as usize).min(total.saturating_sub(1));
            let line = &lines[row];
            let src: String = text.chars().skip(line.start).take(line.end - line.start).collect();
            let carets = ui.fonts.carets(ui.font, size, &src);
            let local = p.x - view.x + st.scroll;
            let mut best = 0;
            for (i, c) in carets.iter().enumerate() {
                if (c - local).abs() < (carets[best] - local).abs() {
                    best = i;
                }
            }
            line.start + best
        };

        if resp.pressed {
            self.focused = Some(id);
            if let Some(h) = self.text_history.get_mut(&id) {
                h.break_run();
            }
            let i = index_at(self, resp.mouse_pos, &lines, text);
            st.cursor = i;
            if !self.input.modifiers.shift {
                st.anchor = i;
            }
            st.goal = None;
            st.last_move = self.time;
        } else if resp.active && (self.mouse_delta.x != 0.0 || self.mouse_delta.y != 0.0) {
            st.cursor = index_at(self, resp.mouse_pos, &lines, text);
            st.last_move = self.time;
        }

        // ---- keyboard ------------------------------------------------------
        let mut changed = false;
        let mut submitted = false;
        let (mut can_undo, mut can_redo) = (false, false);
        if self.focused == Some(id) {
            let out = apply_events(self, id, text, &mut st, true);
            changed = out.changed;
            submitted = out.submitted;
            (can_undo, can_redo) = (out.can_undo, out.can_redo);
        }
        let focused = self.focused == Some(id);
        self.focus_order.push(id);

        // Re-index after an edit, and clamp a caret that a shorter text left
        // past the end.
        let lines = if changed { lines_of(text) } else { lines };
        let n = text.chars().count();
        st.cursor = st.cursor.min(n);
        st.anchor = st.anchor.min(n);
        let total = lines.len();

        // ---- scrolling -----------------------------------------------------
        let wheel = if resp.hovered { self.input.scroll.y } else { 0.0 };
        let max_y = (total as f32 * lh - view.h).max(0.0);
        st.scroll_y = (st.scroll_y - wheel).clamp(0.0, max_y);

        let row = lines.iter().position(|l| st.cursor <= l.end).unwrap_or(0);
        let col = st.cursor - lines[row].start;
        // Keep the caret in view, vertically and sideways.
        let caret_y = row as f32 * lh;
        if focused {
            if caret_y < st.scroll_y {
                st.scroll_y = caret_y;
            } else if caret_y + lh > st.scroll_y + view.h {
                st.scroll_y = caret_y + lh - view.h;
            }
            st.scroll_y = st.scroll_y.clamp(0.0, max_y);
        }
        let line_src: String = text.chars().skip(lines[row].start).take(lines[row].end - lines[row].start).collect();
        let caret_carets = self.fonts.carets(self.font, size, &line_src);
        let caret_x = caret_carets[col.min(caret_carets.len() - 1)];
        if focused {
            if caret_x - st.scroll > view.w - 2.0 {
                st.scroll = caret_x - view.w + 2.0;
            }
            if caret_x - st.scroll < 0.0 {
                st.scroll = caret_x;
            }
            st.scroll = st.scroll.max(0.0);
        }

        // ---- what to draw --------------------------------------------------
        // Only the visible lines: a thousand-line script costs what a screenful
        // does, the same rule the virtual list follows.
        let first = (st.scroll_y / lh).floor().max(0.0) as usize;
        let visible = ((view.h / lh).ceil() as usize + 2).min(total.saturating_sub(first));
        let (sel_a, sel_b) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        let mut rows: Vec<Row> = Vec::with_capacity(visible);
        for (r, l) in lines.iter().enumerate().skip(first).take(visible) {
            let src: String = text.chars().skip(l.start).take(l.end - l.start).collect();
            let carets = self.fonts.carets(self.font, size, &src);
            let y = r as f32 * lh - st.scroll_y;
            // This line's share of the selection, if any.
            let sel = if sel_b > sel_a && sel_b > l.start && sel_a <= l.end {
                let a = sel_a.max(l.start) - l.start;
                let b = sel_b.min(l.end) - l.start;
                let (x0, x1) = (carets[a.min(carets.len() - 1)], carets[b.min(carets.len() - 1)]);
                // A selected newline shows as a sliver past the last glyph.
                let x1 = if sel_b > l.end { x1 + size * 0.35 } else { x1 };
                Some((x0, x1))
            } else {
                None
            };
            let number = if opts.line_numbers { self.frame_text(&(r + 1).to_string()) } else { self.frame_text("") };
            rows.push(Row { y, text: self.frame_text(&src), selection: sel, number });
        }

        let caret_on = focused && (((self.time - st.last_move) * 1.8) as i64 % 2 == 0);
        let focus_t = self.animate_bool(id, 0, focused);
        let hover_t = self.animate_bool(id, 1, resp.hovered);
        self.text_states.insert(id, st);
        self.animating |= focused;

        let height = match opts.height {
            Size::Fit => Size::Fixed(opts.rows as f32 * lh + pad * 2.0),
            h => h,
        };
        let (scroll_x, scroll_y) = (st.scroll, st.scroll_y);
        let caret_pos = Vec2::new(caret_x, caret_y);
        let layout = Layout::leaf(Size::Grow(1.0), height);
        self.add_leaf(id, layout, Vec2::new(80.0, lh + pad * 2.0), true, move |p, r| {
            let radius = s.radius;
            if focus_t > 0.01 {
                p.rect_bordered(r.expand(3.0), Color::TRANSPARENT, radius + 3.0, 2.0, s.focus_ring.with_alpha(s.focus_ring.a * focus_t));
            }
            let border = s.border.lerp(s.border_hover, hover_t).lerp(s.border_focus, focus_t);
            p.rect_bordered(r, s.fill, radius, 1.0, border);

            let view = Rect::new(r.x + pad + gutter, r.y + pad, (r.w - pad * 2.0 - gutter).max(0.0), (r.h - pad * 2.0).max(0.0));
            p.draw.push_clip(Rect::new(r.x + 1.0, view.y, r.w - 2.0, view.h));
            for row in &rows {
                let ly = view.y + row.y;
                if let Some((x0, x1)) = row.selection {
                    let c = if focus_t > 0.5 { s.selection } else { s.selection.with_alpha(s.selection.a * 0.5) };
                    p.rect(Rect::new(view.x - scroll_x + x0, ly, (x1 - x0).max(1.0), lh), c, 2.0);
                }
                if gutter > 0.0 {
                    let g = Rect::new(r.x + pad, ly, gutter - 6.0, lh);
                    p.text_right(g, size, s.placeholder, row.number);
                }
                p.text(Vec2::new(view.x - scroll_x, ly), size, s.text, row.text);
            }
            if caret_on {
                let x = (view.x - scroll_x + caret_pos.x).round();
                p.rect(Rect::new(x - 0.75, view.y + caret_pos.y - 1.0, 1.5, lh + 2.0), s.caret, 0.75);
            }
            p.draw.pop_clip();
        });

        if focused {
            let x = resp.rect.x + pad + gutter - scroll_x + caret_pos.x;
            let y = resp.rect.y + pad + caret_pos.y - scroll_y;
            self.ime_rect = Some(Rect::new(x, y, 1.0, lh));
        }
        let _ = content_h;
        TextResponse { response: resp, changed, submitted, focused, caret: (row, col), selection: (sel_a, sel_b), can_undo, can_redo }
    }
}
