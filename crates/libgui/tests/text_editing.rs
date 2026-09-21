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
        .bind(ctrl(Key::A), UiAction::SelectAll)
        .bind(ctrl(Key::Z), UiAction::Undo)
        .bind(Shortcut::plain(Key::Z).ctrl().shift(), UiAction::Redo);
    b
}

struct World {
    ui: Ui,
    text: String,
    /// Set when the *app's* undo shortcut fired, which must not happen while a
    /// field has focus.
    app_undo: usize,
}

impl World {
    fn new(text: &str) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        ui.set_key_bindings(bindings());
        Self { ui, text: text.to_string(), app_undo: 0 }
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
