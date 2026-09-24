//! A CAD dimension box, driven the way a user drives one.
//!
//! The evaluator has its own tests in `number.rs`. These are about the field:
//! when the caller's value changes, and — more often the question — when it
//! must not.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// Enter commits, Escape cancels. libgui binds neither; an app gets these from
/// `libgui_keymap`.
fn bindings() -> KeyBindings {
    let mut b = KeyBindings::new();
    b.bind(Shortcut::plain(Key::Enter), UiAction::InsertNewline)
        .bind(Shortcut::plain(Key::Escape), UiAction::Cancel)
        .bind(Shortcut::plain(Key::Backspace), UiAction::Delete(Motion::Left));
    b
}

struct World {
    ui: Ui,
    units: Units,
    value: f64,
    min: f64,
    vars: Vec<(String, f64, i32)>,
    last: NumberResponse,
    /// Every frame's `committed`, so a commit on a frame between the ones a
    /// test looks at is still seen.
    commits: usize,
    field: Rect,
    elsewhere: Rect,
}

impl World {
    fn new(value: f64) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        ui.set_key_bindings(bindings());
        let mut w = Self {
            ui,
            units: Units::length_mm(),
            value,
            min: f64::NEG_INFINITY,
            vars: Vec::new(),
            last: NumberResponse::default(),
            commits: 0,
            field: Rect::default(),
            elsewhere: Rect::default(),
        };
        for _ in 0..3 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) -> NumberResponse {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 });
        let vars: Vec<Var> = self.vars.iter().map(|(n, v, d)| Var { name: n, value: *v, dim: *d }).collect();
        let opts = NumberOptions { min: self.min, vars: &vars, ..NumberOptions::default() };
        let r = self.ui.number_input_with("depth", &mut self.value, &self.units, &opts);
        let other = self.ui.button("elsewhere");
        let _ = self.ui.end_frame();
        self.field = r.response.rect;
        self.elsewhere = other.rect;
        self.commits += r.committed as usize;
        self.last = r.clone();
        r
    }

    fn click(&mut self, at: Vec2) {
        self.ui.push(InputEvent::PointerMoved { pos: at });
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame();
    }

    fn click_field(&mut self) {
        let c = self.field.center();
        self.click(c);
        assert!(self.last.focused, "clicking the field did not focus it");
    }

    fn type_text(&mut self, s: &str) -> NumberResponse {
        self.ui.push(InputEvent::Text(s.into()));
        self.frame()
    }

    fn key(&mut self, key: Key) -> NumberResponse {
        self.ui.push(InputEvent::Key { key, pressed: true, repeat: false });
        let r = self.frame();
        self.ui.push(InputEvent::Key { key, pressed: false, repeat: false });
        self.frame();
        r
    }

    /// Focus the field, type, press Enter: what a user does nine times in ten.
    fn enter(&mut self, text: &str) -> NumberResponse {
        self.click_field();
        self.type_text(text);
        self.key(Key::Enter)
    }
}

#[test]
fn focus_selects_the_value_so_typing_replaces_it() {
    let mut w = World::new(25.4);
    let r = w.enter("10");
    // Had the caret landed where the click was, this would read "25.4 mm10"
    // and fail to evaluate.
    assert!(r.error.is_none(), "typing was appended, not a replacement: {:?}", r.error);
    assert_eq!(w.value, 10.0);
}

#[test]
fn the_value_changes_on_commit_not_per_keystroke() {
    let mut w = World::new(25.4);
    w.click_field();
    let r = w.type_text("5");
    assert!(r.changed, "the keystroke was not reported");
    assert_eq!(w.value, 25.4, "a half-typed value reached the model");
    assert!(!r.committed);
    w.type_text("0");
    assert_eq!(w.value, 25.4);

    let r = w.key(Key::Enter);
    assert!(r.committed, "Enter did not commit");
    assert_eq!(w.value, 50.0);
    assert_eq!(w.commits, 1, "one commit, reported once");
}

#[test]
fn an_expression_in_units_arrives_in_base_units() {
    let mut w = World::new(0.0);
    w.enter("3/8\"");
    assert!((w.value - 9.525).abs() < 1e-9, "3/8\" is {} mm, not 9.525", w.value);

    w.vars = vec![("w".into(), 40.0, 1)];
    w.enter("w/2 + 1cm");
    assert!((w.value - 30.0).abs() < 1e-9, "got {}", w.value);
}

#[test]
fn escape_puts_back_what_was_there() {
    let mut w = World::new(25.4);
    w.click_field();
    w.type_text("99");
    let r = w.key(Key::Escape);
    assert!(!r.committed, "Escape committed");
    assert!(r.error.is_none());
    assert_eq!(w.value, 25.4);
    assert_eq!(w.commits, 0);

    // And the field shows the value again: committing it untouched changes
    // nothing. Had "99" stayed in the field, this would write 99.
    w.click_field();
    let r = w.key(Key::Enter);
    assert_eq!(w.value, 25.4, "the cancelled text was still in the field");
    assert!(!r.committed, "committing the unchanged value reported a change");
}

#[test]
fn clicking_elsewhere_commits() {
    let mut w = World::new(25.4);
    w.click_field();
    w.type_text("7");
    let c = w.elsewhere.center();
    w.click(c);
    assert!(!w.last.focused, "the field kept focus");
    assert_eq!(w.value, 7.0, "leaving the field threw the edit away");
    assert_eq!(w.commits, 1);
}

#[test]
fn text_that_does_not_evaluate_stays_and_says_why() {
    let mut w = World::new(25.4);
    let r = w.enter("2w");
    let e = r.error.expect("no error for 2w");
    assert!(e.message.contains("use *"), "{e}");
    assert_eq!(w.value, 25.4, "a failed expression changed the value");
    assert!(!r.committed);

    // It stays, frame after frame, without the user doing anything.
    for _ in 0..5 {
        w.frame();
    }
    assert!(w.last.error.is_some(), "the error disappeared on its own");

    // Fixing it is the way out, and it commits normally.
    let r = w.enter("3");
    assert!(r.error.is_none(), "{:?}", r.error);
    assert_eq!(w.value, 3.0);
}

#[test]
fn a_value_out_of_range_is_refused_not_clamped() {
    let mut w = World::new(25.4);
    w.min = 0.0;
    let r = w.enter("-5");
    let e = r.error.expect("a negative depth was accepted");
    assert!(e.message.contains("at least 0 mm"), "{e}");
    assert_eq!(w.value, 25.4, "the value was clamped instead of refused");
}

#[test]
fn a_value_changed_by_the_app_shows_in_the_field() {
    let mut w = World::new(25.4);
    w.value = 12.0; // an undo, a constraint solve, another panel
    w.frame();
    w.click_field();
    let r = w.key(Key::Enter);
    assert_eq!(w.value, 12.0, "the field still held the old value and wrote it back");
    assert!(!r.committed);
}

#[test]
fn the_state_is_forgotten_when_the_field_goes() {
    let mut w = World::new(25.4);
    w.enter("2w");
    assert!(w.last.error.is_some());

    // A frame without the field, as when its panel closes.
    w.ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 });
    let _ = w.ui.end_frame();

    let r = w.frame();
    assert!(r.error.is_none(), "a closed panel's failed edit came back with it");
}

/// Text can become valid without being edited: it named something the app had
/// not defined yet. Committing it then must clear the error, or a correct
/// value sits under a red border saying it is wrong.
#[test]
fn text_that_becomes_valid_commits_and_clears_its_error() {
    let mut w = World::new(25.4);
    let r = w.enter("w/2");
    assert!(r.error.is_some(), "an unknown name was accepted");

    w.vars = vec![("w".into(), 40.0, 1)];
    w.click_field();
    let r = w.key(Key::Enter); // no typing: the same text, now meaningful
    assert_eq!(w.value, 20.0);
    assert!(r.error.is_none(), "a value that committed is still marked wrong");
}
