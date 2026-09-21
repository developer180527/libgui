//! Pad driven the way a person drives it, to pin the thing the demo exists to
//! show: **the caret decides whose undo Cmd+Z is.**

use libgui::*;
use libgui_keymap::{Keymap, Platform};
use libgui_pad::{Cmd, Pad};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
/// Big enough that the sheet lands where the look test shows it.
const SIZE: Vec2 = Vec2::new(1320.0, 900.0);
/// A point inside the first line of the page, and one on the empty desk
/// beside it.
const IN_PAGE: Vec2 = Vec2::new(300.0, 150.0);
const OFF_PAGE: Vec2 = Vec2::new(960.0, 600.0);

struct World {
    ui: Ui,
    pad: Pad,
}

impl World {
    fn text(&self) -> &str {
        &self.pad.text
    }
}

impl World {
    fn new(text: &str) -> Self {
        // Pinned to one platform so the chord the test presses is the chord
        // the app bound, on whatever machine this runs.
        let mut ui = Ui::new(libgui_pad::theme(false), FONT).expect("font");
        Keymap::<u8>::new(Platform::Mac).install(&mut ui);
        let mut pad = Pad::new(Platform::Mac);
        pad.text = text.to_string();
        let mut w = Self { ui, pad };
        for _ in 0..4 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) {
        self.ui.begin_frame(FrameInfo { screen_size: SIZE, scale: 1.0, dt: 1.0 / 60.0 });
        self.pad.ui(&mut self.ui);
        let _ = self.ui.end_frame();
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

    /// Cmd+Z, or Cmd+Shift+Z. Two frames, then one more: a command is run at
    /// the top of the frame after the one that raised it.
    fn undo_chord(&mut self, shift: bool) {
        let mods: &[Key] = if shift { &[Key::SuperLeft, Key::ShiftLeft] } else { &[Key::SuperLeft] };
        for &m in mods {
            self.ui.push(InputEvent::Key { key: m, pressed: true, repeat: false });
        }
        self.ui.push(InputEvent::Key { key: Key::Z, pressed: true, repeat: false });
        self.frame();
        self.ui.push(InputEvent::Key { key: Key::Z, pressed: false, repeat: false });
        for &m in mods {
            self.ui.push(InputEvent::Key { key: m, pressed: false, repeat: false });
        }
        self.frame();
        self.frame();
    }

    /// A menu or toolbar click, without hunting for the control: both do
    /// exactly this.
    fn command(&mut self, cmd: Cmd) {
        self.pad.raise(cmd);
        self.frame();
        self.frame();
    }
}

/// **The demo, as a test.** The same chord reaches two different histories,
/// and which one it is depends only on where the caret is.
#[test]
fn the_caret_decides_whose_undo_the_chord_is() {
    let mut w = World::new("beta\nalpha\n");
    w.click(IN_PAGE);
    assert!(w.pad.editing, "the click did not land in the page");

    // A command first, so there is something in the document's history.
    w.command(Cmd::SortLines);
    assert_eq!(w.pad.text, "\nalpha\nbeta", "sort did not run");
    assert!(w.pad.can_undo());

    // Caret in the page: the chord is the field's, and it takes back typing.
    w.click(IN_PAGE);
    w.type_text("X");
    assert!(w.pad.text.contains('X'));
    w.undo_chord(false);
    assert!(!w.pad.text.contains('X'), "the field did not take back the typing");
    assert_eq!(w.pad.text, "\nalpha\nbeta", "the app's undo ran too, and undid the command as well");
    assert!(w.pad.can_undo(), "the document's history was consumed by a chord meant for the field");

    // Click off the page: nothing is focused, so the app gets the chord.
    w.click(OFF_PAGE);
    assert!(!w.pad.editing, "the page kept the keyboard");
    w.undo_chord(false);
    assert_eq!(w.pad.text, "beta\nalpha\n", "the app's undo did not take back the command");
    assert!(!w.pad.can_undo());

    // And redo, from the same side.
    w.undo_chord(true);
    assert_eq!(w.pad.text, "\nalpha\nbeta", "redo did not put the command back");
}

/// Commands act on the caret's line, which is only knowable because the field
/// reports it.
#[test]
fn a_command_acts_where_the_caret_is() {
    let mut w = World::new("one\ntwo\nthree");
    w.click(IN_PAGE); // the first line
    assert_eq!(w.pad.caret.0, 0);
    w.command(Cmd::DuplicateLine);
    assert_eq!(w.pad.text, "one\none\ntwo\nthree");

    w.command(Cmd::Upper);
    assert_eq!(w.pad.text, "ONE\none\ntwo\nthree", "upper-casing did not stay on the caret's line");

    w.command(Cmd::DeleteLine);
    assert_eq!(w.pad.text, "one\ntwo\nthree");
}

/// Replace all is one step in the document's history, not one per match.
#[test]
fn replace_all_is_a_single_undoable_step() {
    let mut w = World::new("red fish, Red fish, blue fish");
    w.pad.find = "red".into();
    w.pad.replace = "one".into();
    w.frame();
    assert_eq!(w.pad.matches().len(), 2, "find is not case-insensitive");

    w.command(Cmd::ReplaceAll);
    assert_eq!(w.pad.text, "one fish, one fish, blue fish");

    w.click(OFF_PAGE);
    w.undo_chord(false);
    assert_eq!(w.pad.text, "red fish, Red fish, blue fish", "one undo did not take the whole replace back");
}

/// A command writes the document from outside the field, so the field's typing
/// history describes text that is no longer there. The field notices when the
/// chord arrives, drops that history and **releases the chord**, so Pad's own
/// undo takes the command back — with the caret still in the page.
///
/// Before that release existed, the focused field swallowed the chord with
/// nothing to undo and Cmd+Z after a command silently did nothing.
#[test]
fn the_chord_falls_through_once_the_field_has_nothing_to_undo() {
    let mut w = World::new("hello");
    w.click(IN_PAGE);
    w.type_text(" there");
    assert_eq!(w.text(), "hello there");

    w.command(Cmd::Upper);
    assert_eq!(w.text(), "HELLO THERE");
    assert!(w.pad.editing, "the page lost the caret");

    // Caret still in the page. The field's step described " there", which is
    // not in the buffer any more, so the chord is the app's.
    w.undo_chord(false);
    assert_eq!(w.text(), "hello there", "Cmd+Z after a command did nothing");

    // And again: the app keeps unwinding its own history from here.
    w.undo_chord(false);
    assert_eq!(w.text(), "hello there", "a second undo took back typing the app never recorded");
    assert!(!w.pad.can_undo(), "the document's history should be spent");
}

/// The other half of the same rule: while the field *does* have something of
/// its own, the chord stays with it and the app never sees it.
#[test]
fn the_field_keeps_the_chord_while_it_has_typing_to_take_back() {
    let mut w = World::new("hello");
    w.click(IN_PAGE);
    w.command(Cmd::Upper); // something in the app's history
    assert!(w.pad.can_undo());

    w.type_text("!");
    w.undo_chord(false);
    assert!(!w.text().contains('!'), "the field did not take back its own typing");
    assert!(w.pad.can_undo(), "the app's undo ran while the field had typing to take back");
}
