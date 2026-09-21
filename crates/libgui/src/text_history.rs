//! Undo for a text field — and *only* for a text field.
//!
//! # Why a UI library has an undo stack at all
//!
//! It has two, and they belong to different people.
//!
//! **Your document's undo is yours.** libgui does not know what an extrude, a
//! keyframe or a transaction is, and it never touches your model. While no
//! text field has focus, the undo chord does not reach libgui at all:
//! [`Ui::consume_shortcut`](crate::Ui::consume_shortcut) hands it to your app,
//! which runs its own history with its own granularity.
//!
//! **A focused field's undo is the field's.** The moment a caret is in a text
//! box, Cmd/Ctrl+Z has to take back the *typing*, not reverse your last CAD
//! operation — that is what every OS text control does (NSTextView, the Win32
//! edit control, GTK's entry), and an app that routes the chord to its
//! document while someone is mid-word destroys their work.
//!
//! So the rule is: **focused field first, app second** — but only while the
//! field has something of its own to take back. An empty history hands the
//! chord on, the way an `NSTextView` shares its window's undo manager, so a
//! caret resting in a search box never makes your app's undo unreachable.
//!
//! # What it stores
//!
//! **Edits, not copies of the document.** A step is the range that changed,
//! the text that was there and the text that replaced it — so a keystroke in a
//! 4 MB file costs a few bytes, and the caps below bound the *edits* rather
//! than throwing away history because the document is large. (They once stored
//! whole snapshots, which meant five edits to a 140 KB file left one undoable
//! step and said nothing about it.)
//!
//! A run of typing coalesces into one step, and deleting is its own run.
//!
//! # Staleness
//!
//! The app may write the string itself between frames — its own undo, a
//! reload, a value shared with another widget — and then a step describes text
//! that is no longer there. Rather than compare the whole document every frame
//! to find out, a step is **checked against the buffer when it is applied**:
//! the text it claims to have inserted has to still be where it says. If it is
//! not, the history is stale, it is dropped, and the chord goes to the app. It
//! costs the length of the edit instead of the length of the document, and it
//! is exact rather than a guess.

/// Steps kept per field.
const MAX_STEPS: usize = 64;
/// A pause this long closes the run in progress, so an uninterrupted stretch
/// of typing is not one undo step. Every editor does this; two seconds is the
/// usual figure.
const RUN_PAUSE: f64 = 2.0;
/// And a ceiling on the *edited* text they hold. Reached by editing a lot, not
/// by editing a large file.
const MAX_BYTES: usize = 256 * 1024;

/// What kind of edit is currently being coalesced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Run {
    /// Nothing in progress: the next edit starts a new step.
    #[default]
    None,
    Typing,
    Deleting,
}

/// One undoable edit: `removed` was at `at`, and `inserted` replaced it.
/// Indices are in chars, like the rest of the field.
#[derive(Clone, Debug, PartialEq)]
struct Step {
    at: usize,
    removed: String,
    inserted: String,
    /// Where the caret was before the edit, to put it back.
    cursor: usize,
    anchor: usize,
}

impl Step {
    fn weight(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

/// What one edit did to the text, as the field's [`Edit`](crate::text_edit)
/// reports it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Change {
    /// Char index where the replacement starts.
    pub at: usize,
    pub removed: String,
    pub inserted: String,
}

/// One field's undo history.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    past: Vec<Step>,
    future: Vec<Step>,
    run: Run,
    /// When the run in progress was last extended.
    last_edit: f64,
}

/// How an edit should join the history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edited {
    /// Typing: coalesces with the run before it.
    Typing,
    /// Deleting: coalesces with other deletes, not with typing.
    Deleting,
    /// Everything else — a paste, a cut, select-all-then-replace: its own step.
    Discrete,
}

/// Replace `at..at + n` chars of `text` with `with`.
fn splice(text: &mut String, at: usize, n: usize, with: &str) {
    let start = byte_at(text, at);
    let end = byte_at(text, at + n);
    text.replace_range(start..end, with);
}

fn byte_at(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

/// Is `needle` exactly at char index `at` in `text`?
fn sits_at(text: &str, at: usize, needle: &str) -> bool {
    let start = byte_at(text, at);
    text[start..].starts_with(needle)
}

impl History {
    /// Record an edit that has just been applied. `cursor`/`anchor` are where
    /// the caret was *before* it.
    pub fn record(&mut self, kind: Edited, change: Change, cursor: usize, anchor: usize, now: f64) {
        // A new edit invalidates anything that was undone: the future only
        // exists as long as nothing was written over it.
        self.future.clear();
        if now - self.last_edit > RUN_PAUSE {
            self.run = Run::None;
        }
        self.last_edit = now;

        let joins = matches!((kind, self.run), (Edited::Typing, Run::Typing) | (Edited::Deleting, Run::Deleting));
        self.run = match kind {
            Edited::Typing => Run::Typing,
            Edited::Deleting => Run::Deleting,
            Edited::Discrete => Run::None,
        };
        // A newline ends a typing run, so undo lands line by line rather than
        // taking back a whole paragraph in one go.
        if change.inserted.contains('\n') {
            self.run = Run::None;
        }

        if joins {
            if let Some(last) = self.past.last_mut() {
                if merge(last, &change) {
                    self.trim();
                    return;
                }
            }
        }
        self.past.push(Step {
            at: change.at,
            removed: change.removed,
            inserted: change.inserted,
            cursor,
            anchor,
        });
        self.trim();
    }

    /// End the run in progress: the next edit starts a new step. Caret moves,
    /// clicks and losing focus all break a run, the way a text control does.
    pub fn break_run(&mut self) {
        self.run = Run::None;
    }

    /// Anything to take back? A field with nothing of its own lets the undo
    /// chord through to the app.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Step back, editing `text` in place. Returns the caret and anchor to
    /// restore, or `None` — nothing to undo, or the buffer no longer matches
    /// what the step describes, in which case the history is dropped.
    pub fn undo(&mut self, text: &mut String, cursor: usize, anchor: usize) -> Option<(usize, usize)> {
        let step = self.past.pop()?;
        if !sits_at(text, step.at, &step.inserted) {
            self.clear(); // the app rewrote the buffer under us
            return None;
        }
        splice(text, step.at, step.inserted.chars().count(), &step.removed);
        let caret = (step.cursor, step.anchor);
        self.future.push(Step { cursor, anchor, ..step });
        self.run = Run::None;
        Some(caret)
    }

    pub fn redo(&mut self, text: &mut String, cursor: usize, anchor: usize) -> Option<(usize, usize)> {
        let step = self.future.pop()?;
        if !sits_at(text, step.at, &step.removed) {
            self.clear();
            return None;
        }
        splice(text, step.at, step.removed.chars().count(), &step.inserted);
        // Redo leaves the caret after what it put back.
        let caret = step.at + step.inserted.chars().count();
        self.past.push(Step { cursor, anchor, ..step });
        self.run = Run::None;
        Some((caret, caret))
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.run = Run::None;
    }

    /// Oldest steps first, by count and by weight.
    fn trim(&mut self) {
        while self.past.len() > MAX_STEPS {
            self.past.remove(0);
        }
        let mut bytes: usize = self.past.iter().map(Step::weight).sum();
        while bytes > MAX_BYTES && self.past.len() > 1 {
            bytes -= self.past[0].weight();
            self.past.remove(0);
        }
    }

    #[cfg(test)]
    pub fn depth(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

/// Fold `change` into `last` if they are adjacent parts of one run. Typing
/// extends the insertion; backspace grows the removal leftwards; forward
/// delete grows it rightwards.
fn merge(last: &mut Step, change: &Change) -> bool {
    let inserted_len = last.inserted.chars().count();
    // Typing on: the new text starts where the last insertion ended.
    if change.removed.is_empty() && change.at == last.at + inserted_len {
        last.inserted.push_str(&change.inserted);
        return true;
    }
    if change.inserted.is_empty() {
        // Backspace: removes the chars just before this step's range.
        if change.at + change.removed.chars().count() == last.at + inserted_len && inserted_len == 0 {
            let mut removed = change.removed.clone();
            removed.push_str(&last.removed);
            last.removed = removed;
            last.at = change.at;
            return true;
        }
        // Forward delete: removes the chars just after it.
        if change.at == last.at + inserted_len && inserted_len == 0 {
            last.removed.push_str(&change.removed);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply `inserted` at `at` and hand the history the change, the way the
    /// field does.
    fn edit(h: &mut History, text: &mut String, kind: Edited, at: usize, n: usize, with: &str) {
        let start = byte_at(text, at);
        let end = byte_at(text, at + n);
        let removed = text[start..end].to_string();
        let cursor = at + n;
        splice(text, at, n, with);
        h.record(kind, Change { at, removed, inserted: with.to_string() }, cursor, cursor, 0.0);
    }

    fn type_text(h: &mut History, text: &mut String, at: usize, s: &str) {
        for (i, c) in s.chars().enumerate() {
            edit(h, text, Edited::Typing, at + i, 0, &c.to_string());
        }
    }

    /// A burst of typing is one step, not one per character.
    #[test]
    fn typing_coalesces_into_one_step() {
        let mut h = History::default();
        let mut text = String::new();
        type_text(&mut h, &mut text, 0, "hello");
        assert_eq!(h.depth(), (1, 0), "five characters became {} steps", h.depth().0);

        let (cursor, _) = h.undo(&mut text, 5, 5).expect("nothing to undo");
        assert_eq!(text, "");
        assert_eq!(cursor, 0, "undo did not put the caret back where the run started");
    }

    /// **The point of storing edits.** A big document does not cost history:
    /// the cap bounds what was *edited*, so a hundred keystrokes in a 4 MB
    /// file are all still undoable.
    #[test]
    fn a_large_document_does_not_evict_its_history() {
        let mut text = "x".repeat(4 * 1024 * 1024);
        let mut h = History::default();
        for i in 0..40 {
            edit(&mut h, &mut text, Edited::Discrete, i * 10, 0, "!");
        }
        assert_eq!(h.depth().0, 40, "steps were evicted from a document that was merely large");
        for _ in 0..40 {
            assert!(h.undo(&mut text, 0, 0).is_some(), "a step vanished");
        }
        assert_eq!(text.len(), 4 * 1024 * 1024, "undo did not restore the document");
        assert!(text.chars().all(|c| c == 'x'));
    }

    /// Moving the caret ends the run: what is typed next is a separate step.
    #[test]
    fn a_caret_move_breaks_the_run() {
        let mut h = History::default();
        let mut text = String::from("ab");
        type_text(&mut h, &mut text, 2, "c");
        h.break_run();
        type_text(&mut h, &mut text, 3, "d");
        assert_eq!(h.depth(), (2, 0));
        h.undo(&mut text, 4, 4);
        assert_eq!(text, "abc");
    }

    /// A newline ends a run too, so undo lands line by line instead of taking
    /// back ten minutes of typing at once.
    #[test]
    fn a_newline_ends_the_run() {
        let mut h = History::default();
        let mut text = String::new();
        type_text(&mut h, &mut text, 0, "one");
        type_text(&mut h, &mut text, 3, "\n");
        type_text(&mut h, &mut text, 4, "two");
        // The newline closes the run it ends up in, so the step is "one\n"
        // and what follows is its own: undo lands line by line rather than
        // taking back ten minutes of typing at once.
        assert_eq!(h.depth().0, 2, "the line break did not close its run");
        h.undo(&mut text, 7, 7);
        assert_eq!(text, "one\n");
    }

    /// Typing and deleting are different runs, and redo survives until
    /// something is written over it.
    #[test]
    fn deleting_is_its_own_run_and_redo_is_dropped_on_a_new_edit() {
        let mut h = History::default();
        let mut text = String::from("word");
        edit(&mut h, &mut text, Edited::Deleting, 3, 1, ""); // backspace 'd'
        edit(&mut h, &mut text, Edited::Deleting, 2, 1, ""); // backspace 'r'
        assert_eq!(text, "wo");
        assert_eq!(h.depth(), (1, 0), "two backspaces should coalesce");

        h.undo(&mut text, 2, 2).expect("undo");
        assert_eq!(text, "word");
        assert_eq!(h.depth(), (0, 1));
        h.redo(&mut text, 4, 4).expect("redo");
        assert_eq!(text, "wo");

        // Undo, then type: the redo is gone, because it described a future
        // that no longer follows from here.
        h.undo(&mut text, 2, 2).expect("undo");
        type_text(&mut h, &mut text, 4, "!");
        assert_eq!(h.depth().1, 0, "a new edit left a stale redo behind");
    }

    /// The app changed the string itself, so the step describes text that is
    /// no longer there. It is caught when the step is applied — not by
    /// comparing the whole document every frame — and the history is dropped.
    #[test]
    fn an_outside_write_is_caught_when_undo_is_attempted() {
        let mut h = History::default();
        let mut text = String::from("mine");
        type_text(&mut h, &mut text, 4, "!");
        assert_eq!(h.depth(), (1, 0));

        let mut theirs = String::from("something else");
        assert_eq!(h.undo(&mut theirs, 0, 0), None, "undo was applied to a buffer it never edited");
        assert_eq!(theirs, "something else", "the field rewrote the app's own text");
        assert_eq!(h.depth(), (0, 0), "stale steps survived");
    }

    /// It is retained state on a widget id, so it has a ceiling — on the edits.
    #[test]
    fn the_history_is_bounded_by_what_was_edited() {
        let mut h = History::default();
        let mut text = String::new();
        for i in 0..(MAX_STEPS * 3) {
            edit(&mut h, &mut text, Edited::Discrete, 0, 0, &format!("{i} "));
        }
        assert!(h.depth().0 <= MAX_STEPS, "history grew to {}", h.depth().0);

        let mut h = History::default();
        let mut big = String::new();
        for _ in 0..8 {
            let blob = "y".repeat(100 * 1024);
            edit(&mut h, &mut big, Edited::Discrete, 0, 0, &blob);
        }
        let bytes: usize = h.past.iter().map(Step::weight).sum();
        assert!(bytes <= MAX_BYTES + 100 * 1024, "history holds {bytes} bytes");
    }

    /// A pause closes the run: coming back after a break starts a new step,
    /// so a long stretch of typing is not one undo.
    #[test]
    fn a_pause_closes_the_run() {
        let mut h = History::default();
        let mut text = String::new();
        h.record(Edited::Typing, Change { at: 0, removed: String::new(), inserted: "one".into() }, 0, 0, 0.0);
        text.push_str("one");
        h.record(Edited::Typing, Change { at: 3, removed: String::new(), inserted: "two".into() }, 3, 3, 1.0);
        text.push_str("two");
        assert_eq!(h.depth().0, 1, "typing a second later should join the run");

        h.record(Edited::Typing, Change { at: 6, removed: String::new(), inserted: "!".into() }, 6, 6, 10.0);
        text.push('!');
        assert_eq!(h.depth().0, 2, "typing after a long pause joined the previous run");
        h.undo(&mut text, 7, 7);
        assert_eq!(text, "onetwo");
    }

    /// An empty history hands the chord to the app.
    #[test]
    fn an_empty_history_cannot_undo() {
        let mut h = History::default();
        assert!(!h.can_undo() && !h.can_redo());
        let mut text = String::from("x");
        type_text(&mut h, &mut text, 1, "y");
        assert!(h.can_undo());
        h.undo(&mut text, 2, 2);
        assert!(!h.can_undo() && h.can_redo());
    }
}
