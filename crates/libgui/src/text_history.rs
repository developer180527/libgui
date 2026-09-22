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
/// The default for [`crate::Ui::undo_run_pause`]: a pause this long closes the
/// run in progress, so an uninterrupted stretch of typing is not one undo
/// step. Every editor does this; two seconds is the usual figure, and it is a
/// default rather than a rule because it is taste, not mechanism.
pub const DEFAULT_RUN_PAUSE: f64 = 2.0;
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
/// **Offsets are bytes**, like the rest of the field since it stopped counting
/// characters. They used to be chars here and bytes there, which meant undo
/// silently addressed the wrong place the moment anything before the caret was
/// not ASCII — an accent, a curly quote, an emoji.
#[derive(Clone, Debug, PartialEq)]
struct Step {
    at: usize,
    removed: String,
    inserted: String,
    /// Where the caret was before the edit, to put it back.
    cursor: usize,
    anchor: usize,
    /// The document as this step leaves it: what `undo` expects to find.
    after: Fingerprint,
    /// The document as undoing it leaves it: what `redo` expects to find.
    before: Fingerprint,
}

/// Enough of the document to notice that the app rewrote it.
///
/// Checking only that `inserted` is still at `at` is no check at all for a
/// deletion, which inserted nothing: every string starts with the empty
/// string, so the test always passed, the field went on claiming it could undo
/// and spliced the removed text into whatever the app had put there instead.
///
/// So a step also remembers the document's length and a hash of the bytes
/// immediately around the edit. Both are O(1) to take and to check — the whole
/// point of storing edits rather than snapshots was not to touch the document's
/// length on every keystroke, and this keeps that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fingerprint {
    /// Total length of the document, in bytes.
    len: usize,
    /// Hash of up to [`CONTEXT`] bytes on each side of the edit. The context
    /// is the same before and after the edit — the edit is what sits between
    /// the two — so one hash serves both fingerprints.
    around: u64,
}

/// Bytes hashed on each side of an edit. Enough that a rewrite which happens
/// to preserve the length is still caught, small enough to be free.
const CONTEXT: usize = 64;

/// Hash the bytes just before `at` and just after `at + len`.
fn around(text: &str, at: usize, len: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let b = text.as_bytes();
    // Clamped rather than asserted: `at` comes from a step that may describe a
    // document the app has since replaced, so it can point anywhere.
    let start = at.min(b.len());
    let end = (at + len).min(b.len());
    let mut h = crate::hash::FxHasher::default();
    b[start.saturating_sub(CONTEXT)..start].hash(&mut h);
    b[end..(end + CONTEXT).min(b.len())].hash(&mut h);
    h.finish()
}

/// The document as it stands, around the range `at..at + len`.
fn fingerprint(text: &str, at: usize, len: usize) -> Fingerprint {
    Fingerprint { len: text.len(), around: around(text, at, len) }
}

impl Step {
    fn weight(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }

    /// Take both fingerprints from `text`, the document as this step leaves
    /// it. The context on each side of the edit is the same either way — it is
    /// only the piece between them that differs — so the two fingerprints
    /// share a hash and differ by the lengths of what was swapped.
    fn stamp(&mut self, text: &str) {
        let around = around(text, self.at, self.inserted.len());
        self.after = Fingerprint { len: text.len(), around };
        self.before = Fingerprint {
            len: text.len() + self.removed.len() - self.inserted.len().min(text.len()),
            around,
        };
    }
}

/// What one edit did to the text, as the field's [`Edit`](crate::text_edit)
/// reports it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Change {
    /// Byte offset where the replacement starts.
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

/// Replace `at..at + n` **bytes** of `text` with `with`.
fn splice(text: &mut String, at: usize, n: usize, with: &str) {
    text.replace_range(at..at + n, with);
}

/// Is `needle` exactly at byte offset `at` in `text`?
///
/// Compared as bytes rather than by slicing, so an offset into a document the
/// app has replaced — past the end, or mid-character — answers false instead
/// of panicking.
fn sits_at(text: &str, at: usize, needle: &str) -> bool {
    let b = text.as_bytes();
    at <= b.len() && b[at..].starts_with(needle.as_bytes())
}

impl History {
    /// Record an edit that has just been applied.
    /// `caret` is where the cursor and anchor were *before* the edit, which is
    /// what undo puts back. `text` is the document as the edit has just left
    /// it: a step fingerprints itself against it.
    pub fn record(&mut self, kind: Edited, change: Change, caret: (usize, usize), now: f64, pause: f64, text: &str) {
        let (cursor, anchor) = caret;
        // A new edit invalidates anything that was undone: the future only
        // exists as long as nothing was written over it.
        self.future.clear();
        if now - self.last_edit > pause {
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
                    // The merged step covers a different range, so its
                    // fingerprints are taken again rather than inherited.
                    last.stamp(text);
                    self.trim();
                    return;
                }
            }
        }
        let mut step = Step {
            at: change.at,
            removed: change.removed,
            inserted: change.inserted,
            cursor,
            anchor,
            after: Fingerprint::default(),
            before: Fingerprint::default(),
        };
        step.stamp(text);
        self.past.push(step);
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
        if !sits_at(text, step.at, &step.inserted) || fingerprint(text, step.at, step.inserted.len()) != step.after {
            self.clear(); // the app rewrote the buffer under us
            return None;
        }
        splice(text, step.at, step.inserted.len(), &step.removed);
        let caret = (step.cursor, step.anchor);
        self.future.push(Step { cursor, anchor, ..step });
        self.run = Run::None;
        Some(caret)
    }

    pub fn redo(&mut self, text: &mut String, cursor: usize, anchor: usize) -> Option<(usize, usize)> {
        let step = self.future.pop()?;
        if !sits_at(text, step.at, &step.removed) || fingerprint(text, step.at, step.removed.len()) != step.before {
            self.clear();
            return None;
        }
        splice(text, step.at, step.removed.len(), &step.inserted);
        // Redo leaves the caret after what it put back.
        let caret = step.at + step.inserted.len();
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
    let inserted_len = last.inserted.len();
    // Typing on: the new text starts where the last insertion ended.
    if change.removed.is_empty() && change.at == last.at + inserted_len {
        last.inserted.push_str(&change.inserted);
        return true;
    }
    if change.inserted.is_empty() {
        // Backspace: removes the chars just before this step's range.
        if change.at + change.removed.len() == last.at + inserted_len && inserted_len == 0 {
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

    /// Apply `with` over `at..at + n` and hand the history the change, the
    /// way the field does. **Offsets are bytes**, as they are in the field.
    fn edit(h: &mut History, text: &mut String, kind: Edited, at: usize, n: usize, with: &str) {
        let removed = text[at..at + n].to_string();
        let cursor = at + n;
        splice(text, at, n, with);
        h.record(kind, Change { at, removed, inserted: with.to_string() }, (cursor, cursor), 0.0, DEFAULT_RUN_PAUSE, text);
    }

    /// Type `s` one character at a time, advancing by each one's *byte*
    /// length, so the helper is right for text that is not all ASCII.
    fn type_text(h: &mut History, text: &mut String, at: usize, s: &str) {
        let mut at = at;
        for c in s.chars() {
            edit(h, text, Edited::Typing, at, 0, &c.to_string());
            at += c.len_utf8();
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
        // The edit is applied first: a step fingerprints the document as the
        // edit leaves it, so `record` is handed the text afterwards.
        let at = |h: &mut History, text: &mut String, s: &str, now: f64| {
            let at = text.len();
            text.push_str(s);
            h.record(Edited::Typing, Change { at, removed: String::new(), inserted: s.into() }, (at, at), now, DEFAULT_RUN_PAUSE, text);
        };
        at(&mut h, &mut text, "one", 0.0);
        at(&mut h, &mut text, "two", 1.0);
        assert_eq!(h.depth().0, 1, "typing a second later should join the run");

        at(&mut h, &mut text, "!", 10.0);
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

