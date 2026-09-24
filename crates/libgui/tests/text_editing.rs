//! Multi-line editing and the field's own undo, driven through `Ui` the way a
//! user drives them.
//!
//! The test that matters most is `undo_belongs_to_the_field_only_while_it_has_focus`:
//! a UI library has no business undoing an app's document, and the whole
//! design rests on the chord reaching one or the other, never both.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// The chords these tests press. An app would get these from `libgui_keymap`;
/// spelled out here because the point is that libgui itself binds nothing.
fn bindings() -> KeyBindings {
    let mut b = KeyBindings::new();
    let ctrl = |k| Shortcut::plain(k).ctrl();
    b.bind(Shortcut::plain(Key::Backspace), UiAction::Delete(Motion::Left))
        .bind(Shortcut::plain(Key::Delete), UiAction::Delete(Motion::Right))
        .bind(Shortcut::plain(Key::ArrowLeft), UiAction::Move { motion: Motion::Left, select: false })
        .bind(Shortcut::plain(Key::ArrowRight), UiAction::Move { motion: Motion::Right, select: false })
        .bind(Shortcut::plain(Key::ArrowUp), UiAction::Move { motion: Motion::Up, select: false })
        .bind(Shortcut::plain(Key::ArrowDown), UiAction::Move { motion: Motion::Down, select: false })
        .bind(Shortcut::plain(Key::ArrowUp).shift(), UiAction::Move { motion: Motion::Up, select: true })
        .bind(Shortcut::plain(Key::ArrowDown).shift(), UiAction::Move { motion: Motion::Down, select: true })
        .bind(Shortcut::plain(Key::Home), UiAction::Move { motion: Motion::LineStart, select: false })
        .bind(Shortcut::plain(Key::End), UiAction::Move { motion: Motion::LineEnd, select: false })
        .bind(Shortcut::plain(Key::Enter), UiAction::InsertNewline)
        .bind(Shortcut::plain(Key::Enter).ctrl(), UiAction::Submit)
        .bind(Shortcut::plain(Key::Escape), UiAction::Cancel)
        .bind(ctrl(Key::A), UiAction::SelectAll)
        .bind(ctrl(Key::Z), UiAction::Undo)
        .bind(Shortcut::plain(Key::Z).ctrl().shift(), UiAction::Redo);
    b
}

struct World {
    ui: Ui,
    text: String,
    /// The last frame's caret, as `(line, column)`.
    caret: (usize, usize),
    /// Set when the *app's* undo shortcut fired, which must not happen while a
    /// field has focus.
    app_undo: usize,
}

impl World {
    fn new(text: &str) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        ui.set_key_bindings(bindings());
        Self { ui, text: text.to_string(), caret: (0, 0), app_undo: 0 }
    }

    /// One frame: a text area, and the app's own Undo command after it — the
    /// order a real app builds in, panels first and globals last.
    fn frame(&mut self) -> TextResponse {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(420.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 });
        let r = self.ui.text_area("notes", &mut self.text, 6);
        if self.ui.consume_shortcut(Shortcut::plain(Key::Z).ctrl()) {
            self.app_undo += 1;
        }
        let _ = self.ui.end_frame();
        self.caret = r.caret;
        r
    }

    fn warm(&mut self) -> TextResponse {
        for _ in 0..3 {
            self.frame();
        }
        self.frame()
    }

    fn click(&mut self, at: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame();
    }

    fn type_text(&mut self, s: &str) {
        self.ui.push(InputEvent::Text(s.into()));
        self.frame();
    }

    fn key(&mut self, key: Key, mods: &[Key]) -> TextResponse {
        for &m in mods {
            self.ui.push(InputEvent::Key { key: m, pressed: true, repeat: false });
        }
        self.ui.push(InputEvent::Key { key, pressed: true, repeat: false });
        let out = self.frame();
        self.ui.push(InputEvent::Key { key, pressed: false, repeat: false });
        for &m in mods {
            self.ui.push(InputEvent::Key { key: m, pressed: false, repeat: false });
        }
        self.frame();
        out
    }
}

/// Enter breaks the line; Ctrl+Enter commits. One binding does both, because a
/// single-line field has nowhere to put a newline.
#[test]
fn enter_breaks_a_line_and_ctrl_enter_commits() {
    let mut w = World::new("first");
    w.warm();
    w.click(Vec2::new(200.0, 20.0));
    w.key(Key::End, &[]);
    w.key(Key::Enter, &[]);
    w.type_text("second");
    assert_eq!(w.text, "first\nsecond");

    let r = w.key(Key::Enter, &[Key::ControlLeft]);
    assert!(r.submitted, "Ctrl+Enter did not commit");
    assert_eq!(w.text, "first\nsecond", "committing changed the text");
}

/// Escape is reported as its own way of leaving, distinct from committing and
/// from clicking away, so a field that reverts on cancel can tell them apart.
#[test]
fn escape_leaves_the_field_and_says_so() {
    let mut w = World::new("draft");
    w.warm();
    w.click(Vec2::new(200.0, 20.0));
    let r = w.key(Key::Escape, &[]);
    assert!(r.cancelled, "Escape was not reported as a cancel");
    assert!(!r.submitted, "Escape was reported as a commit");
    assert!(!r.focused, "Escape did not release focus");
    let r = w.frame();
    assert!(!r.cancelled, "the cancel was reported again on the next frame");
}

/// Up and Down keep the column you started in, even walking over a short line
/// — the thing that makes arrow navigation feel right rather than lossy.
#[test]
fn vertical_motion_keeps_its_goal_column() {
    let mut w = World::new("aaaaaaaaaa\nbb\ncccccccccc");
    w.warm();
    w.click(Vec2::new(200.0, 20.0));
    w.key(Key::End, &[]); // end of the long first line, column 10
    w.key(Key::ArrowDown, &[]); // short line: clamps to its end
    w.key(Key::ArrowDown, &[]); // long again: back to column 10
    w.type_text("!");
    assert_eq!(w.text, "aaaaaaaaaa\nbb\ncccccccccc!", "the goal column was lost on the short line");
}

/// A burst of typing is one undo step, and undo puts the caret back where the
/// run began rather than leaving it stranded.
#[test]
fn typing_is_one_undo_step() {
    let mut w = World::new("");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.type_text("hello");
    w.type_text(" there");
    assert_eq!(w.text, "hello there");

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "", "one undo did not take back the whole run");

    w.key(Key::Z, &[Key::ControlLeft, Key::ShiftLeft]);
    assert_eq!(w.text, "hello there", "redo did not put it back");

    // Typing after an undo drops the redo: that future no longer follows.
    w.key(Key::Z, &[Key::ControlLeft]);
    w.type_text("x");
    w.key(Key::Z, &[Key::ControlLeft, Key::ShiftLeft]);
    assert_eq!(w.text, "x", "a stale redo came back");
}

/// Deleting is its own run, and a caret move between edits starts a new step.
#[test]
fn deletes_and_moves_break_the_run() {
    let mut w = World::new("");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.type_text("abcdef");
    w.key(Key::Backspace, &[]);
    w.key(Key::Backspace, &[]);
    assert_eq!(w.text, "abcd");

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "abcdef", "the two deletes were not one step");
    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "", "the typing before them was not a separate step");
}

/// **The design, as a test.** While the caret is in a field, Ctrl+Z is the
/// field's; the app never sees it. With nothing focused, the app gets it and
/// the field's history is not consulted at all.
#[test]
fn undo_belongs_to_the_field_only_while_it_has_focus() {
    let mut w = World::new("");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.type_text("typed");

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "", "the field did not undo its own typing");
    assert_eq!(w.app_undo, 0, "the app's undo fired while a field had focus — it would have undone the document");

    // Click away: nothing is focused, so the chord is the app's.
    w.click(Vec2::new(400.0, 280.0));
    assert!(!w.ui.wants_keyboard(), "the field kept focus");
    let before = w.text.clone();
    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.app_undo, 1, "the app's undo did not fire once the field lost focus");
    assert_eq!(w.text, before, "the field undid something while it was not focused");
}

/// The app wrote the buffer itself — its own undo, a reload, a value bound to
/// something else. The field's history described text that no longer exists,
/// so it is dropped rather than resurrecting it.
#[test]
fn an_external_write_drops_the_fields_history() {
    let mut w = World::new("original");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.type_text("!");
    assert_eq!(w.text, "original!");

    // The document's undo put something else in the same string.
    w.text = "replaced by the app".into();
    w.frame();

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "replaced by the app", "the field undid its way back over the app's own write");
}

/// Selection across lines, by dragging, and what Ctrl+A covers.
#[test]
fn selection_spans_lines() {
    let mut w = World::new("one\ntwo\nthree");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.key(Key::A, &[Key::ControlLeft]);
    w.type_text("gone");
    assert_eq!(w.text, "gone", "select-all did not cover every line");

    // Shift+Down extends a line at a time.
    w.text = "one\ntwo\nthree".into();
    w.frame();
    w.click(Vec2::new(20.0, 20.0));
    w.key(Key::Home, &[]); // wherever the click landed, start of line one
    w.key(Key::ArrowDown, &[Key::ShiftLeft]);
    w.type_text("X");
    assert_eq!(w.text, "Xtwo\nthree", "shift+down did not select through the first line break");
}

/// A long document builds what fits on screen, not what it holds — the same
/// rule the virtual list follows.
#[test]
fn a_long_document_costs_what_a_screenful_costs() {
    let small: String = (0..8).map(|i| format!("line {i}\n")).collect();
    let huge: String = (0..5_000).map(|i| format!("line {i}\n")).collect();

    let cost = |text: &str| {
        let mut w = World::new(text);
        w.warm();
        w.ui.frame_cost()
    };
    let a = cost(&small);
    let b = cost(&huge);
    assert!(
        b.instances < a.instances + 40,
        "5,000 lines drew {} instances against {} for eight",
        b.instances,
        a.instances
    );
    assert!(b.nodes <= a.nodes + 2, "5,000 lines built {} nodes against {}", b.nodes, a.nodes);
}

// ---- regressions from review ----------------------------------------------

/// A field's history must survive an ordinary document. Whole-text snapshots
/// hit the byte cap after a few edits of a 200 KB file and silently threw the
/// older steps away — on the widget that advertises five thousand lines.
#[test]
fn a_large_document_keeps_its_undo_steps() {
    let big: String = (0..4_000).map(|i| format!("line {i} of a document that is not small\n")).collect();
    assert!(big.len() > 150_000, "the fixture is not big enough to test the cap");

    let mut w = World::new(&big);
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    for i in 0..5 {
        w.type_text(&format!("{i}"));
        w.key(Key::ArrowRight, &[]); // break the run: five separate steps
    }
    assert_ne!(w.text, big, "the edits did not land");

    // Five edits, five undos. With whole-document snapshots the byte cap threw
    // all but the last away and said nothing about it.
    for i in 0..5 {
        let before = w.text.clone();
        w.key(Key::Z, &[Key::ControlLeft]);
        assert_ne!(w.text, before, "undo {i} did nothing — a step was trimmed away");
    }
    assert_eq!(w.text, big, "five undos did not get back to the original document");
}

/// With the caret in a field that has nothing left to undo, the chord belongs
/// to the app again — the way an NSTextView shares its window's undo manager.
/// Otherwise a focused field swallows Cmd+Z forever and the app's own undo is
/// unreachable without clicking away first.
#[test]
fn an_empty_field_history_lets_the_chord_through() {
    let mut w = World::new("hello");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    assert!(w.ui.wants_keyboard(), "the field did not take focus");

    // Nothing has been typed, so the field has nothing of its own to undo.
    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.app_undo, 1, "a focused field with an empty history swallowed the app's undo");
    assert_eq!(w.text, "hello", "the field changed the text with an empty history");

    // Type: now the field owns the chord again.
    w.type_text(" there");
    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "hello", "the field did not take back its own typing");
    assert_eq!(w.app_undo, 1, "the app's undo fired while the field had something to undo");

    // And once that is spent, it falls through again.
    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.app_undo, 2, "the chord did not fall through once the field was spent");
}

/// Pasted tabs survive. Flattening them to one space destroys the indentation
/// of any pasted code, and a pasted Makefile stops working.
#[test]
fn a_pasted_tab_is_not_flattened_to_a_space() {
    let mut w = World::new("");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.ui.push(InputEvent::Paste("build:\n\tcc -o out main.c\n".into()));
    w.frame();
    assert!(w.text.contains('\t'), "the tab was flattened: {:?}", w.text);
    assert_eq!(w.text, "build:\n\tcc -o out main.c\n");
}

/// A text area must read **what it draws**, not the document.
///
/// It used to build a `Vec` of every line from a `char` walk, count the
/// characters again for the caret clamp, and convert char indices to byte
/// offsets by walking from the start a third time — on every frame, with no
/// input. The scroll position is now an anchor (which line is at the top, and
/// where it starts) rather than a pixel offset, so nothing has to count
/// newlines from the beginning to find out what is on screen.
#[test]
fn a_frame_reads_what_it_draws_not_the_document() {
    let small: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let large: String = (0..200_000).map(|i| format!("line {i}\n")).collect();
    assert!(large.len() > 2_000_000, "the fixture is not big enough to tell the two apart");

    let read = |doc: &str, focus: bool| -> usize {
        let mut w = World::new(doc);
        w.warm();
        if focus {
            w.click(Vec2::new(60.0, 20.0));
            w.frame();
        }
        w.frame();
        w.ui.frame_cost().text_scanned
    };

    let (a, b) = (read(&small, false), read(&large, false));
    assert_eq!(a, b, "an idle frame read {a} bytes of a small document and {b} of a large one");
    assert!(a < 4_000, "an idle frame read {a} bytes to draw a screenful");

    // Focused, with a caret to place: still bounded by the window.
    let (a, b) = (read(&small, true), read(&large, true));
    assert_eq!(a, b, "a focused frame's reading followed the document's size");
}

/// Up and Down aim for a **position**, not a character column. In a
/// proportional font the thirtieth `i` and the thirtieth `W` are nowhere near
/// each other, so a caret walking down a page of mixed text used to slide
/// sideways.
#[test]
fn vertical_motion_holds_its_x_not_its_column() {
    // Line 1 is wide characters, line 2 narrow, line 3 wide again. Landing on
    // the same *column* in line 2 would be a third of the way along it; the
    // same x is most of the way along.
    let mut w = World::new("WWWWWWWWWW\niiiiiiiiiiiiiiiiiiiiiiiiiiiiii\nWWWWWWWWWW");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.key(Key::Home, &[]);
    w.key(Key::ArrowRight, &[]);
    w.key(Key::ArrowRight, &[]); // after two Ws
    w.key(Key::ArrowDown, &[]);
    w.type_text("|");

    let line2 = w.text.lines().nth(1).expect("second line");
    let at = line2.find('|').expect("the marker did not land on the second line");
    assert!(
        at > 4,
        "two Ws wide landed at character {at} of the narrow line — that is a column, not a position"
    );

    // And coming back up returns to where it started, rather than to wherever
    // the narrow line's column happened to be.
    let mut w = World::new("WWWWWWWWWW\niiii\nWWWWWWWWWW");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    w.key(Key::Home, &[]);
    for _ in 0..5 {
        w.key(Key::ArrowRight, &[]);
    }
    w.key(Key::ArrowDown, &[]); // the short line clamps
    w.key(Key::ArrowDown, &[]); // back to a wide line at the original x
    w.type_text("|");
    let line3 = w.text.lines().nth(2).expect("third line");
    assert_eq!(line3.find('|'), Some(5), "the goal position was lost crossing the short line");
}

/// The scroll position is an anchor now, so the things a pixel offset gave for
/// free have to be checked: that the wheel moves the view, that it stops at
/// both ends, and that the caret drags the view along with it.
#[test]
fn the_view_scrolls_and_stops_at_both_ends() {
    let doc: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let mut w = World::new(&doc);
    w.warm();

    // Which line is at the top: click the first row and ask.
    let top_line = |w: &mut World| -> usize {
        w.click(Vec2::new(30.0, 12.0));
        w.frame();
        w.caret.0
    };
    assert_eq!(top_line(&mut w), 0, "the view did not start at the top");

    let wheel = |w: &mut World, dy: f32| {
        // Hover first, in its own frame: a widget learns it is hovered from
        // the rect layout gave it last frame.
        w.ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 40.0) });
        w.frame();
        w.ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, dy), unit: WheelUnit::Pixel });
        w.frame();
    };

    wheel(&mut w, -100.0); // down
    let scrolled = top_line(&mut w);
    assert!(scrolled > 0, "the wheel did not scroll the view");

    wheel(&mut w, 1000.0); // far up
    assert_eq!(top_line(&mut w), 0, "scrolling up did not stop at the first line");

    // Far down: the last line stays in view rather than scrolling off.
    for _ in 0..40 {
        wheel(&mut w, -1000.0);
    }
    let bottom = top_line(&mut w);
    assert!(bottom > 100, "scrolling down barely moved: top line {bottom}");
    assert!(bottom < 200, "the view scrolled past the end of the document: top line {bottom}");

    // The caret drags the view with it: back to the top, then down past the
    // bottom of the window.
    wheel(&mut w, 100_000.0);
    w.click(Vec2::new(30.0, 12.0));
    w.key(Key::Home, &[]);
    let start = w.caret.0;
    for _ in 0..12 {
        w.key(Key::ArrowDown, &[]);
    }
    assert_eq!(w.caret.0, start + 12, "the caret did not move twelve lines");
    w.type_text("X");
    let line = w.text.lines().nth(start + 12).expect("that line");
    assert!(line.contains('X'), "the caret and the view disagree about which line is which");
}

/// **Undo across non-ASCII text.** Offsets in the field are bytes; the history
/// read them as character counts. `é` is two bytes, so a step recorded after
/// it pointed past where it meant, the staleness check failed, and undo
/// silently did nothing *and* threw the history away.
///
/// The ASCII tests above could never catch this: where every character is one
/// byte, the two readings agree.
#[test]
fn undo_works_with_an_accent_before_the_edit() {
    for subject in ["café au lait", "naïve", "“quoted”", "emoji 🙂 here", "日本語のテキスト"] {
        let mut w = World::new(subject);
        w.warm();
        w.click(Vec2::new(400.0, 20.0));
        w.key(Key::End, &[]);
        w.type_text("!");
        assert_eq!(w.text, format!("{subject}!"), "typing failed for {subject:?}");

        let r = w.key(Key::Z, &[Key::ControlLeft]);
        assert_eq!(w.text, subject, "undo did nothing for {subject:?}");
        assert_eq!(w.app_undo, 0, "the field released a chord it could serve");
        let _ = r;

        // And redo puts it back, at the right offset.
        w.key(Key::Z, &[Key::ControlLeft, Key::ShiftLeft]);
        assert_eq!(w.text, format!("{subject}!"), "redo landed wrong for {subject:?}");
    }
}

/// Deleting across non-ASCII too: the step's range is bytes at both ends.
#[test]
fn undo_restores_a_deletion_that_spans_multibyte_characters() {
    let mut w = World::new("héllo wörld");
    w.warm();
    w.click(Vec2::new(400.0, 20.0));
    w.key(Key::End, &[]);
    for _ in 0..5 {
        w.key(Key::Backspace, &[]);
    }
    assert_eq!(w.text, "héllo ");
    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "héllo wörld", "undoing a multibyte deletion did not restore it");
}

/// **Undoing a deletion must not splice into a document the app replaced.**
///
/// A step used to be checked by confirming the text it *inserted* was still
/// where it said. A deletion inserts nothing, and every string starts with the
/// empty string, so the check passed against any buffer at all: the field
/// spliced the removed text into whatever the app had put there. Deleting
/// `gamma`, then having the app write `zzz`, then undoing, produced
/// `zzzgamma`.
#[test]
fn undoing_a_deletion_notices_that_the_app_rewrote_the_document() {
    let mut w = World::new("alpha beta gamma");
    w.warm();
    w.click(Vec2::new(400.0, 20.0));
    w.key(Key::End, &[]);
    for _ in 0..5 {
        w.key(Key::Backspace, &[]);
    }
    assert_eq!(w.text, "alpha beta ");

    // The app rewrites the document behind the field's back: a command, a
    // reload, a file opened.
    w.text = "zzz".into();
    w.frame();

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "zzz", "undo spliced into a document it no longer described");
    // And having nothing left to serve, the field hands the chord to the app.
    assert!(w.app_undo > 0, "the field kept claiming a chord it could not serve");
}

/// A rewrite to something *longer* is caught only by the fingerprint: the
/// offset is still inside the new document, and a deletion's empty `inserted`
/// is a prefix of anything, so neither the bounds check nor `starts_with` has
/// anything to object to.
#[test]
fn a_longer_rewrite_is_caught_too() {
    let mut w = World::new("alpha beta gamma");
    w.warm();
    w.click(Vec2::new(400.0, 20.0));
    w.key(Key::End, &[]);
    for _ in 0..5 {
        w.key(Key::Backspace, &[]);
    }
    assert_eq!(w.text, "alpha beta ");

    w.text = "a much longer document than the field ever saw".into();
    let after = w.text.clone();
    w.frame();

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, after, "undo spliced into a longer document it no longer described");
    assert!(w.app_undo > 0, "the field kept the chord");
}

/// The same check has to survive a rewrite that keeps the length, which a
/// length comparison alone would miss.
#[test]
fn a_same_length_rewrite_is_caught_too() {
    let mut w = World::new("alpha beta gamma");
    w.warm();
    w.click(Vec2::new(400.0, 20.0));
    w.key(Key::End, &[]);
    for _ in 0..5 {
        w.key(Key::Backspace, &[]);
    }
    assert_eq!(w.text, "alpha beta ");

    // Same length, different text.
    w.text = "ALPHA BETA ".into();
    assert_eq!(w.text.len(), 11);
    w.frame();

    w.key(Key::Z, &[Key::ControlLeft]);
    assert_eq!(w.text, "ALPHA BETA ", "a same-length rewrite slipped past the check");
    assert!(w.app_undo > 0, "the field kept the chord");
}

/// The check must not fire on the field's *own* edits, or undo would break
/// wherever the document happened to look unfamiliar. A long run of typing,
/// deleting and re-typing unwinds completely.
#[test]
fn an_undisturbed_field_unwinds_its_whole_history() {
    let mut w = World::new("");
    w.warm();
    w.click(Vec2::new(60.0, 20.0));
    for word in ["one ", "two ", "three "] {
        w.type_text(word);
        // Break the run, so each word is its own step.
        w.key(Key::ArrowLeft, &[]);
        w.key(Key::End, &[]);
    }
    assert_eq!(w.text, "one two three ");

    let mut guard = 0;
    while !w.text.is_empty() && guard < 20 {
        w.key(Key::Z, &[Key::ControlLeft]);
        guard += 1;
    }
    assert_eq!(w.text, "", "the field could not unwind its own edits: {:?}", w.text);
    assert_eq!(w.app_undo, 0, "the field released a chord while it still had work");
}
