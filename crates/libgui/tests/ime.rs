//! Composing text: what an input method shows before it commits.
//!
//! Typing Japanese, Korean or Chinese goes through a composition the user can
//! see and edit before accepting it. Without it, a text field shows nothing at
//! all until the moment a word commits, which is not a field anyone can type
//! into. The composing text belongs to the IME, not to the field: it is drawn
//! inline, and the `&mut String` does not change until the host commits.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 200.0), scale: 2.0, dt: 1.0 / 60.0 }
}

struct Field {
    text: String,
    instances: usize,
    ime_rect: Option<Rect>,
}

impl Field {
    fn new() -> Self {
        Self { text: String::new(), instances: 0, ime_rect: None }
    }

    fn frame(&mut self, ui: &mut Ui) {
        ui.begin_frame(info());
        ui.text_input("field", &mut self.text, "Type…");
        let out = ui.end_frame();
        self.instances = out.draw.instances.len();
        self.ime_rect = out.platform.text_input;
    }
}

fn focus(ui: &mut Ui, f: &mut Field) {
    f.frame(ui);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 15.0) });
    f.frame(ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    f.frame(ui);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    f.frame(ui);
    assert!(ui.focused().is_some(), "the field did not take focus");
}

/// Latin, not kana: the bundled font has no CJK glyphs, so kana would draw
/// nothing and the test would be measuring the missing fallback rather than
/// the composition. A Japanese IME shows romaji at this stage anyway, and the
/// mechanism does not care which script it is carrying.
fn preedit(ui: &mut Ui, text: &str, cursor: usize) {
    ui.push(InputEvent::ImePreedit { text: text.into(), cursor });
}

#[test]
fn composing_text_is_visible_without_being_part_of_the_value() {
    let mut ui = ui();
    let mut f = Field::new();
    focus(&mut ui, &mut f);
    let empty = f.instances;

    preedit(&mut ui, "nihon", 5);
    f.frame(&mut ui);
    assert!(f.instances > empty, "composing text drew nothing");
    assert_eq!(f.text, "", "composing text leaked into the field's value");
}

#[test]
fn committing_replaces_the_composition_and_changes_the_value() {
    let mut ui = ui();
    let mut f = Field::new();
    focus(&mut ui, &mut f);

    preedit(&mut ui, "nihon", 5);
    f.frame(&mut ui);
    ui.push(InputEvent::Text("Japan".into()));
    f.frame(&mut ui);
    assert_eq!(f.text, "Japan", "the commit did not reach the field");

    // Nothing may be left composing. Compared against a field that holds the
    // same value and never composed anything: a leftover composition would
    // draw "Japan" twice, once committed and once underlined.
    let mut plain = self::ui();
    let mut g = Field::new();
    focus(&mut plain, &mut g);
    plain.push(InputEvent::Text("Japan".into()));
    g.frame(&mut plain);
    g.frame(&mut plain);
    f.frame(&mut ui);
    assert_eq!(f.instances, g.instances, "the composition outlived its commit");
}

#[test]
fn abandoning_a_composition_leaves_the_field_as_it_was() {
    let mut ui = ui();
    let mut f = Field::new();
    focus(&mut ui, &mut f);
    ui.push(InputEvent::Text("abc".into()));
    f.frame(&mut ui);
    let settled = f.instances;

    preedit(&mut ui, "nihon", 5);
    f.frame(&mut ui);
    assert!(f.instances > settled);

    // An empty preedit is how a host says the user backed out.
    preedit(&mut ui, "", 0);
    f.frame(&mut ui);
    assert_eq!(f.instances, settled, "an abandoned composition left something behind");
    assert_eq!(f.text, "abc");
}

#[test]
fn the_window_losing_focus_abandons_the_composition() {
    let mut ui = ui();
    let mut f = Field::new();
    focus(&mut ui, &mut f);
    let empty = f.instances;
    preedit(&mut ui, "nihon", 5);
    f.frame(&mut ui);
    assert!(f.instances > empty);
    ui.push(InputEvent::FocusLost);
    f.frame(&mut ui);
    assert_eq!(f.instances, empty, "a composition survived the window losing focus");
}

#[test]
fn the_host_is_told_where_the_composition_is_so_its_candidates_can_follow() {
    let mut ui = ui();
    let mut f = Field::new();
    focus(&mut ui, &mut f);
    let caret = f.ime_rect.expect("a focused field reports an IME rect");
    assert_eq!(caret.w, 1.0, "with nothing composing the rect is the caret");

    preedit(&mut ui, "nihongo", 7);
    f.frame(&mut ui);
    let composing = f.ime_rect.expect("still focused");
    assert!(composing.w > caret.w * 4.0, "the IME rect did not cover the composing text: {composing:?}");
    assert_eq!(composing.x, caret.x, "the composition does not start at the caret");
}

#[test]
fn nothing_composes_into_a_field_that_is_not_focused() {
    let mut ui = ui();
    let mut f = Field::new();
    f.frame(&mut ui);
    let quiet = f.instances;
    preedit(&mut ui, "nihon", 5);
    f.frame(&mut ui);
    assert_eq!(f.instances, quiet, "an unfocused field drew someone else's composition");
    assert!(f.ime_rect.is_none());
}
