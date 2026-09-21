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
//! document while someone is mid-word destroys their work. The history here
//! covers characters typed into a field before they are committed, and it dies
//! with the focus.
//!
//! So the rule is the same one the clipboard already follows: **focused field
//! first, app second**. Nothing about your document's undo changes, and you do
//! not have to record every keystroke in it to make Cmd+Z behave.
//!
//! # What it stores
//!
//! Whole snapshots of the string, coalesced into runs — a burst of typing is
//! one step, not forty — and bounded, because this is retained state hanging
//! off a widget id. Snapshots rather than diffs is the wrong trade for a large
//! document and the right one for the fields an app actually has; the caps
//! keep the failure mode "the oldest steps are forgotten" rather than "the
//! editor eats memory".

/// Steps kept per field.
const MAX_STEPS: usize = 64;
/// And a ceiling on the text they hold, so one enormous field cannot grow
/// without bound.
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

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

/// One field's undo history.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    past: Vec<Snapshot>,
    future: Vec<Snapshot>,
    /// The text as this field last left it. If the app's string differs when
    /// the field next runs, something else wrote it — the document's own undo,
    /// a reload, a sibling widget — and this history describes a buffer that
    /// no longer exists.
    last_seen: String,
    run: Run,
    /// Set once the field has seen the string at least once, so the first
    /// frame does not look like an external change.
    started: bool,
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

impl History {
    /// Called before a field touches the text. Returns true if the app changed
    /// the string behind the field's back, in which case the history has been
    /// cleared: it described a different buffer.
    pub fn sync(&mut self, text: &str) -> bool {
        if !self.started {
            self.started = true;
            self.last_seen = text.to_string();
            return false;
        }
        if self.last_seen == text {
            return false;
        }
        self.past.clear();
        self.future.clear();
        self.last_seen = text.to_string();
        self.run = Run::None;
        true
    }

    /// Remember the state *before* an edit of `kind`. Call with the text as it
    /// is about to be changed.
    /// Returns whether a new step was pushed, so a caller whose edit turns
    /// out to be a no-op (Backspace at the start) can take it back.
    pub fn record(&mut self, kind: Edited, text: &str, cursor: usize, anchor: usize) -> bool {
        let joins = matches!((kind, self.run), (Edited::Typing, Run::Typing) | (Edited::Deleting, Run::Deleting));
        self.run = match kind {
            Edited::Typing => Run::Typing,
            Edited::Deleting => Run::Deleting,
            Edited::Discrete => Run::None,
        };
        // A new edit invalidates anything that was undone: the future only
        // exists as long as nothing was written over it.
        self.future.clear();
        if joins {
            return false;
        }
        self.past.push(Snapshot { text: text.to_string(), cursor, anchor });
        self.trim();
        true
    }

    /// Drop the step `record` just pushed: the edit it was taken for did
    /// nothing, and an undo that changes nothing is worse than no undo.
    pub fn forget_last(&mut self) {
        self.past.pop();
        self.run = Run::None;
    }

    /// The text changed; remember what it became, so an external change is
    /// told apart from the field's own writing.
    pub fn committed(&mut self, text: &str) {
        self.last_seen.clear();
        self.last_seen.push_str(text);
    }

    /// End the run in progress: the next edit starts a new step. Caret moves,
    /// clicks and losing focus all break a run, the way a text control does.
    pub fn break_run(&mut self) {
        self.run = Run::None;
    }

    /// Step back. Returns the text, caret and anchor to restore.
    pub fn undo(&mut self, now: &str, cursor: usize, anchor: usize) -> Option<(String, usize, usize)> {
        let prev = self.past.pop()?;
        self.future.push(Snapshot { text: now.to_string(), cursor, anchor });
        self.run = Run::None;
        self.last_seen.clear();
        self.last_seen.push_str(&prev.text);
        Some((prev.text, prev.cursor, prev.anchor))
    }

    pub fn redo(&mut self, now: &str, cursor: usize, anchor: usize) -> Option<(String, usize, usize)> {
        let next = self.future.pop()?;
        self.past.push(Snapshot { text: now.to_string(), cursor, anchor });
        self.run = Run::None;
        self.last_seen.clear();
        self.last_seen.push_str(&next.text);
        Some((next.text, next.cursor, next.anchor))
    }

    /// Oldest steps first, by count and by weight.
    fn trim(&mut self) {
        while self.past.len() > MAX_STEPS {
            self.past.remove(0);
        }
        let mut bytes: usize = self.past.iter().map(|s| s.text.len()).sum();
        while bytes > MAX_BYTES && self.past.len() > 1 {
            bytes -= self.past[0].text.len();
            self.past.remove(0);
        }
    }

    #[cfg(test)]
    pub fn depth(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A burst of typing is one step, not one per character — otherwise undo
    /// is useless and the history is enormous.
    #[test]
    fn typing_coalesces_into_one_step() {
        let mut h = History::default();
        let mut text = String::new();
        h.sync(&text);
        for (i, c) in "hello".chars().enumerate() {
            let _ = h.record(Edited::Typing, &text, i, i);
            text.push(c);
            h.committed(&text);
        }
        assert_eq!(h.depth(), (1, 0), "five characters became {} steps", h.depth().0);
        let (undone, cursor, _) = h.undo(&text, 5, 5).expect("nothing to undo");
        assert_eq!(undone, "");
        assert_eq!(cursor, 0, "undo did not put the caret back where the run started");
    }

    /// Moving the caret ends the run: what is typed next is a separate step,
    /// which is what makes undo land where a user expects.
    #[test]
    fn a_caret_move_breaks_the_run() {
        let mut h = History::default();
        let mut text = String::from("ab");
        h.sync(&text);
        let _ = h.record(Edited::Typing, &text, 2, 2);
        text.push('c');
        h.committed(&text);
        h.break_run();
        let _ = h.record(Edited::Typing, &text, 3, 3);
        text.push('d');
        h.committed(&text);
        assert_eq!(h.depth(), (2, 0));
        assert_eq!(h.undo(&text, 4, 4).unwrap().0, "abc");
    }

    /// Typing and deleting are different runs, and redo survives until
    /// something is written over it.
    #[test]
    fn deleting_is_its_own_run_and_redo_is_dropped_on_a_new_edit() {
        let mut h = History::default();
        let mut text = String::from("word");
        h.sync(&text);
        let _ = h.record(Edited::Deleting, &text, 4, 4);
        text.pop();
        h.committed(&text);
        let _ = h.record(Edited::Deleting, &text, 3, 3);
        text.pop();
        h.committed(&text);
        assert_eq!(h.depth(), (1, 0), "two deletes should coalesce");

        let (back, _, _) = h.undo(&text, 2, 2).unwrap();
        assert_eq!(back, "word");
        assert_eq!(h.depth(), (0, 1));
        let (forward, _, _) = h.redo(&back, 4, 4).unwrap();
        assert_eq!(forward, "wo");

        // Undo, then type: the redo is gone, because it described a future
        // that no longer follows from here.
        let (back, _, _) = h.undo(&forward, 2, 2).unwrap();
        let _ = h.record(Edited::Typing, &back, 4, 4);
        assert_eq!(h.depth().1, 0, "a new edit left a stale redo behind");
    }

    /// The app changed the string itself — its own undo, a reload, a value
    /// bound to something else — so the field's history describes a buffer
    /// that is gone.
    #[test]
    fn an_external_change_clears_the_history() {
        let mut h = History::default();
        let mut text = String::from("mine");
        h.sync(&text);
        let _ = h.record(Edited::Typing, &text, 4, 4);
        text.push('!');
        h.committed(&text);
        assert_eq!(h.depth(), (1, 0));

        // The document's undo put something else in the same buffer.
        let theirs = String::from("something else");
        assert!(h.sync(&theirs), "an external write was not noticed");
        assert_eq!(h.depth(), (0, 0), "stale steps survived an external change");
        assert!(!h.sync(&theirs), "the same text twice is not a change");
    }

    /// It is retained state on a widget id, so it has a ceiling.
    #[test]
    fn the_history_is_bounded() {
        let mut h = History::default();
        let mut text = String::new();
        h.sync(&text);
        for i in 0..(MAX_STEPS * 3) {
            let _ = h.record(Edited::Discrete, &text, 0, 0);
            text.push_str(&format!("{i} "));
            h.committed(&text);
        }
        assert!(h.depth().0 <= MAX_STEPS, "history grew to {}", h.depth().0);

        // And by weight: one huge field cannot hold megabytes of snapshots.
        let mut h = History::default();
        let mut big = "x".repeat(200 * 1024);
        h.sync(&big);
        for _ in 0..8 {
            let _ = h.record(Edited::Discrete, &big, 0, 0);
            big.push('y');
            h.committed(&big);
        }
        let bytes: usize = h.past.iter().map(|s| s.text.len()).sum();
        assert!(bytes <= MAX_BYTES + big.len(), "history holds {bytes} bytes");
    }
}
