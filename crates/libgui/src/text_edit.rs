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

/// Retained per-field state. **Indices are byte offsets** into the app's
/// string, so the field never converts between counts and offsets — see
/// `Edit`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TextState {
    cursor: usize,
    anchor: usize,
    scroll: f32,
    /// Vertical scroll, for a text area, as an **anchor**: the first visible
    /// line's number, its byte offset, and how far into it the view starts.
    ///
    /// Not a pixel offset, because turning one into "which line is at the top"
    /// means counting newlines from the start of the document — on every
    /// frame, with no input. Anchored, the field reads what it draws.
    top_line: usize,
    top_byte: usize,
    top_frac: f32,
    /// Time of the last caret move; the caret stays solid for a moment after it.
    last_move: f64,
    /// The x Up/Down aim for. Walking past a short line and back must return
    /// to where you started, so the *intended position* is remembered rather
    /// than recomputed from the caret each time.
    goal: Goal,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TextResponse {
    pub response: Response,
    /// Text was modified this frame.
    pub changed: bool,
    /// Enter was pressed (focus is released).
    pub submitted: bool,
    /// The cancel action (Escape, in the default keymap) was pressed: focus
    /// is released and the app should put back what the field held before.
    /// Losing focus any *other* way — a click elsewhere, Tab — is neither this
    /// nor `submitted`, and a field that commits on blur treats it as commit.
    pub cancelled: bool,
    pub focused: bool,
    /// Where the caret is, as `(line, column)`, both zero-based. The column is
    /// in **characters**, because that is what a status bar means by "Col 4";
    /// a single-line field is always on line 0.
    ///
    /// Only the field knows this, and a status bar, a line-scoped command, or
    /// a transform over the selection all need it.
    pub caret: (usize, usize),
    /// The selected range as `(start, end)` **byte offsets**, ordered, equal
    /// when nothing is selected. Slice the same `String` you passed in with
    /// it directly — `&text[r.selection.0..r.selection.1]` — with no
    /// conversion, which is the point of them being bytes.
    pub selection: (usize, usize),
    /// Has typing of its own to take back. When this is false the undo chord
    /// is released to the app, so an Edit menu can say whose undo it is
    /// instead of guessing from focus.
    pub can_undo: bool,
    pub can_redo: bool,
}

impl TextState {
    /// Select everything in a field holding `len` bytes, caret at the end: a
    /// numeric field does this on focus, so typing replaces the value.
    pub(crate) fn select_all(&mut self, len: usize) {
        self.anchor = 0;
        self.cursor = len;
    }

    /// Put the caret at byte `at` of `text`, pulled back to a character
    /// boundary, with nothing selected: where a validator said the problem is.
    pub(crate) fn place_caret(&mut self, text: &str, at: usize) {
        self.cursor = floor_boundary(text, at);
        self.anchor = self.cursor;
    }
}

/// Pure editing operations on a `String` + caret/selection.
///
/// **Offsets are bytes**, not chars, and that is the point: a byte offset is
/// what slices a `String`, so every operation here is local. Counting in
/// characters meant converting one to a byte offset by walking from the start
/// of the document, so a caret move in a large file paid for the whole file.
/// Every index below is on a char boundary, kept there by the motions.
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

/// The column Up and Down aim for, as an **x position in logical pixels**.
///
/// Not a character column: in a proportional font the thirtieth `i` and the
/// thirtieth `W` are nowhere near each other, so a caret walking down a page
/// would slide sideways. What a person means by "the same place on the next
/// line" is a position, and only the widget knows how to map one — hence
/// [`Columns`].
pub(crate) type Goal = Option<f32>;

/// Maps between an x position within one line and a byte offset in it.
///
/// `Edit` does not know about fonts, and vertical motion cannot be done
/// without them. The text area passes its own; a single-line field has no
/// line to move to, so it passes one that is never asked.
pub(crate) trait Columns {
    /// x of the caret before `byte` in `line`.
    fn x_at(&self, line: &str, byte: usize) -> f32;
    /// The byte offset in `line` whose caret is nearest `x`.
    fn byte_at(&self, line: &str, x: f32) -> usize;
}

/// A stand-in for tests of the motions that never leave a line.
#[cfg(test)]
pub(crate) struct NoColumns;

#[cfg(test)]
impl Columns for NoColumns {
    fn x_at(&self, _line: &str, _byte: usize) -> f32 {
        0.0
    }
    fn byte_at(&self, _line: &str, _x: f32) -> usize {
        0
    }
}

/// The char boundary at or before `at`. A field is handed the app's string
/// every frame and the app may have rewritten it, so an offset kept from last
/// frame can land mid-character; slicing there would panic.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

impl Edit<'_> {
    /// Length in **bytes**.
    fn len(&self) -> usize {
        self.text.len()
    }

    fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }

    fn has_selection(&self) -> bool {
        self.cursor != self.anchor
    }

    fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.text[a..b].to_string()
    }

    fn replace(&mut self, a: usize, b: usize, with: &str) {
        if a == b && with.is_empty() {
            return; // nothing happened: backspace at the start, delete at the end
        }
        self.change = Some(Change { at: a, removed: self.text[a..b].to_string(), inserted: with.to_string() });
        self.text.replace_range(a..b, with);
        self.cursor = a + with.len();
        self.anchor = self.cursor;
    }

    fn insert(&mut self, s: &str) {
        let (a, b) = self.selection();
        self.replace(a, b, s);
    }

    /// The char boundary before `pos`, or `pos` at the start.
    fn prev_char(&self, pos: usize) -> usize {
        self.text[..pos].char_indices().next_back().map_or(0, |(i, _)| i)
    }

    /// The char boundary after `pos`, or `pos` at the end.
    fn next_char(&self, pos: usize) -> usize {
        self.text[pos..].chars().next().map_or(pos, |c| pos + c.len_utf8())
    }

    fn word_left(&self, from: usize) -> usize {
        let mut i = from;
        let head = &self.text[..from];
        for (at, c) in head.char_indices().rev() {
            if c.is_whitespace() {
                i = at;
            } else {
                break;
            }
        }
        for (at, c) in self.text[..i].char_indices().rev() {
            if c.is_whitespace() {
                return at + c.len_utf8();
            }
            i = at;
        }
        i
    }

    fn word_right(&self, from: usize) -> usize {
        let mut i = from;
        for (at, c) in self.text[from..].char_indices() {
            if c.is_whitespace() {
                i = from + at + c.len_utf8();
            } else {
                i = from + at;
                break;
            }
        }
        for (at, c) in self.text[i..].char_indices() {
            if c.is_whitespace() {
                return i + at;
            }
            let _ = c;
        }
        self.len()
    }

    /// First byte of the line `pos` is on. Scans back to the previous newline
    /// only — the length of one line, not of the document.
    fn line_start(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map_or(0, |i| i + 1)
    }

    /// The newline that ends `pos`'s line, or the end of the text.
    fn line_end(&self, pos: usize) -> usize {
        self.text[pos..].find('\n').map_or(self.len(), |i| pos + i)
    }

    /// The line `pos` is on, as a string slice.
    fn line_at(&self, pos: usize) -> &str {
        &self.text[self.line_start(pos)..self.line_end(pos)]
    }

    /// The same x on the line above. `None` when there is no line above — a
    /// single-line field is always in that case.
    fn line_above(&self, pos: usize, x: f32, cols: &dyn Columns) -> Option<usize> {
        let start = self.line_start(pos);
        if start == 0 {
            return None;
        }
        let prev_start = self.line_start(start - 1);
        let prev = &self.text[prev_start..start - 1];
        Some(prev_start + cols.byte_at(prev, x))
    }

    fn line_below(&self, pos: usize, x: f32, cols: &dyn Columns) -> Option<usize> {
        let end = self.line_end(pos);
        if end >= self.len() {
            return None;
        }
        let next_start = end + 1;
        let next_end = self.line_end(next_start);
        let next = &self.text[next_start..next_end];
        Some(next_start + cols.byte_at(next, x))
    }

    fn move_to(&mut self, to: usize, extend: bool) {
        self.cursor = to.min(self.len());
        if !extend {
            self.anchor = self.cursor;
        }
    }

    /// Where `motion` takes the caret. `goal` is the x Up and Down aim for;
    /// with no line to move to they fall back to the ends of the text, which
    /// is what a single-line field wants from them.
    fn target(&self, motion: Motion, goal: Goal, cols: &dyn Columns) -> usize {
        let n = self.len();
        let x = || goal.unwrap_or_else(|| self.caret_x(cols));
        match motion {
            Motion::Left => self.prev_char(self.cursor),
            Motion::Right => self.next_char(self.cursor),
            Motion::WordLeft => self.word_left(self.cursor),
            Motion::WordRight => self.word_right(self.cursor),
            Motion::LineStart => self.line_start(self.cursor),
            Motion::LineEnd => self.line_end(self.cursor),
            Motion::Up => self.line_above(self.cursor, x(), cols).unwrap_or(0),
            Motion::Down => self.line_below(self.cursor, x(), cols).unwrap_or(n),
            Motion::DocStart => 0,
            Motion::DocEnd => n,
        }
    }

    /// The caret's x within its own line.
    fn caret_x(&self, cols: &dyn Columns) -> f32 {
        let start = self.line_start(self.cursor);
        cols.x_at(self.line_at(self.cursor), self.cursor - start)
    }

    /// Apply one action. Returns true if the text changed.
    ///
    /// `goal` is the x Up/Down aim for: kept by the caller across actions, set
    /// by a vertical move and cleared by anything else.
    fn action(&mut self, action: UiAction, goal: &mut Goal, cols: &dyn Columns) -> bool {
        // Only vertical motion keeps a goal; everything else decides a new one
        // by where the caret lands.
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
                    m => {
                        if vertical && goal.is_none() {
                            *goal = Some(self.caret_x(cols));
                        }
                        self.target(m, *goal, cols)
                    }
                };
                self.move_to(to, select);
                false
            }
            UiAction::Delete(_) if self.has_selection() => {
                self.insert("");
                true
            }
            UiAction::Delete(motion) => {
                let to = self.target(motion, None, cols);
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
fn edited(e: &mut Edit, hist: &mut History, kind: Edited, now: f64, pause: f64, body: impl FnOnce(&mut Edit)) -> bool {
    let (cursor, anchor) = (e.cursor, e.anchor);
    e.change = None;
    body(e);
    match e.change.take() {
        Some(change) => {
            // The text as the edit has just left it: a step fingerprints the
            // document around itself so it can tell, later, whether the app
            // replaced what it describes.
            hist.record(kind, change, (cursor, anchor), now, pause, e.text);
            true
        }
        None => false,
    }
}

/// What a frame of editing did.
struct Edited2 {
    changed: bool,
    submitted: bool,
    cancelled: bool,
    can_undo: bool,
    can_redo: bool,
}

/// Apply this frame's text, clipboard and action events to `text`.
///
/// Shared by both fields: the single-line one differs only in refusing
/// newlines, and in treating Enter as "commit" because it has nowhere to put
/// a line break.
fn apply_events(ui: &mut Ui, id: Id, text: &mut String, st: &mut TextState, multiline: bool) -> Edited2 {
    let mut out = Edited2 { changed: false, submitted: false, cancelled: false, can_undo: false, can_redo: false };
    let mut hist = ui.text_history.remove(&id).unwrap_or_default();
    // No whole-document comparison here to spot the app writing the string
    // behind the field's back: a step is checked against the buffer when it is
    // applied instead, which costs the length of the edit rather than the
    // length of the document. See `text_history`.
    let now = ui.time;
    let pause = ui.undo_run_pause;
    // Vertical motion needs to measure text, and only the font can: the x a
    // caret is at on one line decides where it lands on the next.
    let size = ui.theme.metrics.font_size;
    let events = ui.input.events.clone();
    let mut e = Edit { text, cursor: st.cursor, anchor: st.anchor, change: None };
    for ev in events {
        match ev {
            Event::Text(s) => {
                let s = sanitize(&s, multiline);
                if !s.is_empty() {
                    out.changed |= edited(&mut e, &mut hist, Edited::Typing, now, pause, |e| e.insert(&s));
                }
            }
            Event::Paste(s) => {
                let s = sanitize(&s, multiline);
                if !s.is_empty() {
                    // A paste is one step whatever it lands next to.
                    out.changed |= edited(&mut e, &mut hist, Edited::Discrete, now, pause, |e| e.insert(&s));
                }
            }
            Event::Action(UiAction::Copy) if e.has_selection() => ui.copied = Some(e.selected_text()),
            Event::Action(UiAction::Cut) if e.has_selection() => {
                ui.copied = Some(e.selected_text());
                out.changed |= edited(&mut e, &mut hist, Edited::Discrete, now, pause, |e| e.insert(""));
            }
            Event::Action(UiAction::InsertNewline) => {
                if multiline {
                    out.changed |= edited(&mut e, &mut hist, Edited::Typing, now, pause, |e| e.insert("\n"));
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
            Event::Action(UiAction::Cancel) => {
                out.cancelled = true;
                ui.focused = None;
            }
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
                let cols = LineColumns { fonts: &ui.fonts, font: ui.font, size };
                out.changed |= edited(&mut e, &mut hist, Edited::Deleting, now, pause, |e| {
                    e.action(a, &mut None, &cols);
                });
            }
            Event::Action(a @ (UiAction::Move { .. } | UiAction::SelectAll)) => {
                // A caret move ends the run: what is typed next is its own step.
                hist.break_run();
                let cols = LineColumns { fonts: &ui.fonts, font: ui.font, size };
                out.changed |= e.action(a, &mut st.goal, &cols);
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
        // An offset the app's own write left inside a character, or past the
        // end, is pulled back to a boundary rather than panicking on the next
        // slice.
        st.cursor = floor_boundary(text, st.cursor);
        st.anchor = floor_boundary(text, st.anchor);
        let text_x = resp.rect.x + pad - st.scroll;
        let hit = |ui: &Ui, x: f32, text: &str| -> usize {
            ui.fonts.byte_at_x(ui.font, size, text, x - text_x)
        };

        if resp.pressed {
            self.focused = Some(id);
            // Clicking ends the typing run: the next characters are their own
            // undo step, wherever the caret just went.
            if let Some(h) = self.text_history.get_mut(&id) {
                h.break_run();
            }
            let i = hit(self, resp.mouse_pos.x, text);
            st.cursor = i;
            if !self.input.modifiers.shift {
                st.anchor = i;
            }
            st.last_move = self.time;
        } else if resp.active && self.mouse_delta.x != 0.0 {
            st.cursor = hit(self, resp.mouse_pos.x, text); // drag-select
            st.last_move = self.time;
        }

        let mut changed = false;
        let mut submitted = false;
        let mut cancelled = false;
        let (mut can_undo, mut can_redo) = (false, false);
        if self.focused == Some(id) {
            let out = apply_events(self, id, text, &mut st, false);
            changed = out.changed;
            submitted = out.submitted;
            cancelled = out.cancelled;
            (can_undo, can_redo) = (out.can_undo, out.can_redo);
        }
        let focused = self.focused == Some(id);
        self.focus_order.push(id);
        if !focused {
            st.anchor = st.cursor; // collapse selection when focus leaves
        }

        // Keep the caret in view.
        let visible_w = (resp.rect.w - 2.0 * pad).max(0.0);
        let total_w = self.fonts.caret_x(self.font, size, text, text.len());
        let cx = self.fonts.caret_x(self.font, size, text, st.cursor);
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
        let sel = (self.fonts.caret_x(self.font, size, text, sa), self.fonts.caret_x(self.font, size, text, sb));
        // Handles into the frame's text arena rather than owned copies: the
        // paint closure runs after layout, so it needs the text to outlive
        // this call, and three `String`s per field per frame is exactly the
        // kind of churn the arena exists to avoid.
        let empty = text.is_empty();
        let shown = self.frame_text(text);
        // Split at the caret, so the composing text can be drawn between them.
        let head = self.frame_text(&text[..st.cursor]);
        let tail = self.frame_text(&text[st.cursor..]);
        let placeholder = self.frame_text(placeholder);
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
            if empty && pre_text.is_none() {
                p.text(Vec2::new(inner.x, ty), size, s.placeholder, placeholder);
            } else if let Some(pre) = pre_text {
                // Three runs: what is committed before the caret, what the IME
                // is composing, and what is committed after it.
                p.text(Vec2::new(x0, ty), size, s.text, head);
                let px = x0 + sel.0;
                p.text(Vec2::new(px, ty), size, s.text, pre);
                p.text(Vec2::new(px + pre_w, ty), size, s.text, tail);
                // Underlined, which is how every platform says "not committed".
                let u = p.hairline(px, ty + line_h - 2.0, 1.0, 1.0);
                p.rect(Rect::new(u.x, u.y, pre_w, u.w), s.text, 0.0);
            } else {
                p.text(Vec2::new(x0, ty), size, s.text, shown);
            }
            if caret_on {
                // While composing, the caret belongs to the IME's own cursor
                // inside the composing text, not to the field's.
                let at = x0 + cx + pre_caret;
                p.rect(Rect::new(at.round() - 0.75, ty - 1.0, 1.5, line_h + 2.0), s.caret, 0.75);
            }
            p.draw.pop_clip();
        });

        TextResponse { response: resp, changed, submitted, cancelled, focused, caret: (0, st.cursor), selection: (sa, sb), can_undo, can_redo }
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
            e.action(a, &mut None, &NoColumns);
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
        // Offsets are bytes: "héllo " is 7 bytes, and "wörld" is 6.
        let (mut text, cursor, anchor) = run(edit("héllo wörld", 13), &[sel(WordLeft)]);
        assert_eq!((cursor, anchor), (7, 13));
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

/// One visible line, ready to paint: its y, its glyphs, the part of it that is
/// selected, and its number for the gutter.
struct Row {
    y: f32,
    text: FrameText,
    selection: Option<(f32, f32)>,
    number: FrameText,
}

/// Newlines in `s`. Bytes, not chars: the only thing a line boundary is.
fn count_lines(s: &str) -> usize {
    s.as_bytes().iter().filter(|&&b| b == b'\n').count()
}

/// Move `start` (a line start) by `delta` whole lines, staying a line start.
/// Walks only the lines it crosses, which is what makes a scroll or a caret
/// nudge cost the distance moved rather than the size of the document.
fn advance_lines(text: &str, start: usize, delta: isize) -> usize {
    let mut at = start;
    for _ in 0..delta.unsigned_abs() {
        if delta > 0 {
            match text[at..].find('\n') {
                Some(i) => at += i + 1,
                None => return at,
            }
        } else {
            if at == 0 {
                return 0;
            }
            at = text[..at - 1].rfind('\n').map_or(0, |i| i + 1);
        }
    }
    at
}

/// Move the anchor by `delta` whole lines, keeping its line number in step.
fn move_anchor(text: &str, st: &mut TextState, delta: isize) {
    let to = advance_lines(text, st.top_byte, delta);
    // `advance_lines` stops at the ends, so the line number follows what it
    // actually did rather than what it was asked to do.
    let moved = if to >= st.top_byte {
        count_lines(&text[st.top_byte..to]) as isize
    } else {
        -(count_lines(&text[to..st.top_byte]) as isize)
    };
    st.top_byte = to;
    st.top_line = (st.top_line as isize + moved).max(0) as usize;
}

/// Fold whole lines out of the fractional offset, and stop at the ends.
///
/// Scrolling down stops with the last line at the bottom of the view, which
/// needs to know only whether `rows` more lines exist — a look ahead of one
/// screen, not a count of the document.
fn normalise_anchor(text: &str, st: &mut TextState, lh: f32, rows: usize) {
    while st.top_frac < 0.0 {
        if st.top_byte == 0 {
            st.top_frac = 0.0;
            break;
        }
        move_anchor(text, st, -1);
        st.top_frac += lh;
    }
    while st.top_frac >= lh {
        // At the bottom when a screenful from here reaches the end.
        let end_of_screen = advance_lines(text, st.top_byte, rows as isize);
        if text[end_of_screen..].find('\n').is_none() && end_of_screen >= text.len().saturating_sub(1) {
            st.top_frac = st.top_frac.min(lh - 1.0).max(0.0);
            break;
        }
        let before = st.top_byte;
        move_anchor(text, st, 1);
        if st.top_byte == before {
            st.top_frac = 0.0;
            break;
        }
        st.top_frac -= lh;
    }
}

/// The line number and line start of `at`, counted from the beginning. The one
/// place the field reads the whole document — when the anchor is lost because
/// the app replaced the string underneath it.
fn locate(text: &str, at: usize) -> (usize, usize) {
    let head = &text[..at];
    (count_lines(head), head.rfind('\n').map_or(0, |i| i + 1))
}

/// The text area's [`Columns`]: x within a line, measured in the field's own
/// font and size.
struct LineColumns<'a> {
    fonts: &'a crate::Fonts,
    font: crate::FontId,
    size: f32,
}

impl Columns for LineColumns<'_> {
    fn x_at(&self, line: &str, byte: usize) -> f32 {
        self.fonts.caret_x(self.font, self.size, line, byte)
    }
    fn byte_at(&self, line: &str, x: f32) -> usize {
        self.fonts.byte_at_x(self.font, self.size, line, x)
    }
}

impl Ui {
    /// A multi-line text field: a console, a script, a note, a description.
    ///
    /// Enter breaks a line and Cmd/Ctrl+Enter commits, which is the
    /// convention everywhere a text box has more than one line. Undo is the
    /// field's own: it takes back typing, and never your document's history.
    /// While no field has focus the chord does not reach libgui at all — and
    /// a focused field with nothing left to take back releases it — so your
    /// app's undo is what runs in both cases.
    ///
    /// **It reads what it draws.** The scroll position is an anchor (the first
    /// visible line and where it starts), not a pixel offset, so nothing has
    /// to count newlines from the beginning of the document to find out what
    /// is on screen. A focused, idle frame over a multi-megabyte file costs
    /// the same as one over a short note.
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
        let rows_visible = ((view.h / lh).ceil() as usize).max(1);

        // ---- the anchor ----------------------------------------------------
        // Everything below is measured from the first visible line, so an idle
        // frame reads the lines it draws and nothing else. The anchor is
        // checked first, because the app owns the string and may have replaced
        // it between frames: a byte offset that is no longer a line start
        // means the document underneath moved.
        st.top_byte = st.top_byte.min(text.len());
        if st.top_byte > 0 && text.as_bytes().get(st.top_byte - 1) != Some(&b'\n') {
            st.top_byte = 0;
            st.top_line = 0;
            st.top_frac = 0.0;
        }
        st.cursor = floor_boundary(text, st.cursor);
        st.anchor = floor_boundary(text, st.anchor);

        // ---- pointer -------------------------------------------------------
        // Byte offset under a window point. The point is inside the view, so
        // the line it names is reached from the anchor, not from the start of
        // the document.
        let row_at = |ui: &Ui, p: Vec2, text: &str, st: &TextState| -> usize {
            let delta = ((p.y - view.y + st.top_frac) / lh).floor().max(0.0) as isize;
            let start = advance_lines(text, st.top_byte, delta);
            let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
            start + ui.fonts.byte_at_x(ui.font, size, &text[start..end], p.x - view.x + st.scroll)
        };

        if resp.pressed {
            self.focused = Some(id);
            if let Some(h) = self.text_history.get_mut(&id) {
                h.break_run();
            }
            let i = row_at(self, resp.mouse_pos, text, &st);
            st.cursor = i;
            if !self.input.modifiers.shift {
                st.anchor = i;
            }
            st.goal = None;
            st.last_move = self.time;
        } else if resp.active && (self.mouse_delta.x != 0.0 || self.mouse_delta.y != 0.0) {
            st.cursor = row_at(self, resp.mouse_pos, text, &st);
            st.last_move = self.time;
        }

        // ---- keyboard ------------------------------------------------------
        let mut changed = false;
        let mut submitted = false;
        let mut cancelled = false;
        let (mut can_undo, mut can_redo) = (false, false);
        if self.focused == Some(id) {
            let out = apply_events(self, id, text, &mut st, true);
            changed = out.changed;
            submitted = out.submitted;
            cancelled = out.cancelled;
            (can_undo, can_redo) = (out.can_undo, out.can_redo);
        }
        let focused = self.focused == Some(id);
        self.focus_order.push(id);
        st.cursor = floor_boundary(text, st.cursor);
        st.anchor = floor_boundary(text, st.anchor);

        // An edit before the anchor moves it: the same line now starts
        // somewhere else, and may not be the same line number either. Both
        // follow from the edit itself, so neither needs a search.
        if st.top_byte > text.len() || (st.top_byte > 0 && text.as_bytes().get(st.top_byte - 1) != Some(&b'\n')) {
            self.text_scanned += st.cursor;
            let (line, byte) = locate(text, st.cursor);
            st.top_line = line;
            st.top_byte = byte;
            st.top_frac = 0.0;
        }

        // ---- where the caret is, relative to the anchor ---------------------
        let line_start = text[..st.cursor].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[st.cursor..].find('\n').map_or(text.len(), |i| st.cursor + i);
        let caret_row = if st.cursor >= st.top_byte {
            self.text_scanned += st.cursor - st.top_byte;
            st.top_line + count_lines(&text[st.top_byte..st.cursor])
        } else {
            self.text_scanned += st.top_byte - st.cursor;
            st.top_line.saturating_sub(count_lines(&text[st.cursor..st.top_byte]))
        };

        // ---- scrolling -----------------------------------------------------
        let wheel = if resp.hovered { self.input.scroll.y } else { 0.0 };
        st.top_frac -= wheel;
        normalise_anchor(text, &mut st, lh, rows_visible);

        // Keep the caret in view — but only on the frames the caret actually
        // moved. Doing it on every frame means a wheel scroll is undone before
        // it is drawn: the view snaps back to a caret that has not moved,
        // which is what "the text area will not scroll while it has focus"
        // looked like.
        let caret_moved = st.last_move == self.time;
        if focused && caret_moved {
            let delta = if caret_row < st.top_line {
                caret_row as isize - st.top_line as isize
            } else if caret_row + 1 > st.top_line + rows_visible {
                (caret_row + 1 - rows_visible) as isize - st.top_line as isize
            } else {
                0
            };
            if delta != 0 {
                move_anchor(text, &mut st, delta);
                st.top_frac = 0.0;
            }
        }

        let line_src = &text[line_start..line_end];
        let caret_x = self.fonts.caret_x(self.font, size, line_src, st.cursor - line_start);
        // The column, in chars, for whoever draws a status bar.
        let col = line_src[..st.cursor - line_start].chars().count();
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
        // From the anchor forward: a five-thousand-line script draws what a
        // screenful draws, and reads no more of the document than that.
        let (sel_a, sel_b) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        let mut rows: Vec<Row> = Vec::with_capacity(rows_visible + 1);
        let mut at = st.top_byte;
        for r in st.top_line..st.top_line + rows_visible + 1 {
            let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
            let src = &text[at..end];
            let y = (r - st.top_line) as f32 * lh - st.top_frac;
            // This line's share of the selection, if any. Measured only when
            // there is one: a caret table per visible line is what made an
            // idle text area allocate on every frame.
            let sel = if sel_b > sel_a && sel_b > at && sel_a <= end {
                let x0 = self.fonts.caret_x(self.font, size, src, sel_a.max(at) - at);
                let x1 = self.fonts.caret_x(self.font, size, src, sel_b.min(end) - at);
                // A selected newline shows as a sliver past the last glyph.
                let x1 = if sel_b > end { x1 + size * 0.35 } else { x1 };
                Some((x0, x1))
            } else {
                None
            };
            let number = if opts.line_numbers { self.frame_text(&(r + 1).to_string()) } else { self.frame_text("") };
            self.text_scanned += end - at;
            rows.push(Row { y, text: self.frame_text(src), selection: sel, number });
            if end >= text.len() {
                break;
            }
            at = end + 1;
        }
        let caret_y = (caret_row as f32 - st.top_line as f32) * lh - st.top_frac;
        let row = caret_row;

        let caret_on = focused && (((self.time - st.last_move) * 1.8) as i64 % 2 == 0);
        let focus_t = self.animate_bool(id, 0, focused);
        let hover_t = self.animate_bool(id, 1, resp.hovered);
        self.text_states.insert(id, st);
        self.animating |= focused;

        let height = match opts.height {
            Size::Fit => Size::Fixed(opts.rows as f32 * lh + pad * 2.0),
            h => h,
        };
        let scroll_x = st.scroll;
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
            let y = resp.rect.y + pad + caret_pos.y;
            self.ime_rect = Some(Rect::new(x, y, 1.0, lh));
        }
        TextResponse { response: resp, changed, submitted, cancelled, focused, caret: (row, col), selection: (sel_a, sel_b), can_undo, can_redo }
    }
}
