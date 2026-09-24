//! The field whose value only changes when the app accepts it.
//!
//! libgui owns the behaviour — commit, cancel, keep the refused text, point at
//! the problem — and the app owns the question. These tests use validators
//! that stand in for an app's own: a parametric CAD's expression language
//! with a lazily resolved parameter table, and a parser that cannot say where
//! it failed.

use libgui::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn bindings() -> KeyBindings {
    let mut b = KeyBindings::new();
    b.bind(Shortcut::plain(Key::Enter), UiAction::InsertNewline)
        .bind(Shortcut::plain(Key::Escape), UiAction::Cancel)
        .bind(Shortcut::plain(Key::Backspace), UiAction::Delete(Motion::Left));
    b
}

/// A stand-in for an app's evaluator: `name * n` or `name + n` over a
/// parameter table the app owns and may change between frames. Resolved when
/// asked, never listed up front.
fn evaluate(text: &str, params: &HashMap<String, f64>) -> Result<f64, FieldError> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [name, op, n] = parts.as_slice() else {
        return Err(FieldError::new("expected `name op number`"));
    };
    let Some(&base) = params.get(*name) else {
        let at = text.find(name).unwrap_or(0);
        return Err(FieldError::new(format!("no parameter called {name}")).at(at));
    };
    let n: f64 = n.parse().map_err(|_| FieldError::new("not a number").at(text.rfind(n).unwrap_or(0)))?;
    match *op {
        "*" => Ok(base * n),
        "+" => Ok(base + n),
        _ => Err(FieldError::new("unknown operator").at(text.find(op).unwrap_or(0))),
    }
}

struct World {
    ui: Ui,
    /// The app's source of truth: the expression, not what it evaluates to.
    source: String,
    params: RefCell<HashMap<String, f64>>,
    select_on_focus: bool,
    /// Every text the validator was asked about.
    asked: RefCell<Vec<String>>,
    /// When set, the validator refuses without saying where.
    no_position: Cell<bool>,
    last: ValidatedResponse,
    commits: usize,
    field: Rect,
    elsewhere: Rect,
}

impl World {
    fn new(source: &str) -> Self {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        ui.set_key_bindings(bindings());
        let mut params = HashMap::new();
        params.insert("width".to_string(), 20.0);
        let mut w = Self {
            ui,
            source: source.to_string(),
            params: RefCell::new(params),
            select_on_focus: true,
            asked: RefCell::new(Vec::new()),
            no_position: Cell::new(false),
            last: ValidatedResponse::default(),
            commits: 0,
            field: Rect::default(),
            elsewhere: Rect::default(),
        };
        for _ in 0..3 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) -> ValidatedResponse {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 });
        // What a parametric app shows while nobody is editing: the value.
        let shown = match evaluate(&self.source, &self.params.borrow()) {
            Ok(v) => format!("{v} mm"),
            Err(_) => self.source.clone(),
        };
        let opts = ValidatedOptions { display: Some(&shown), select_on_focus: self.select_on_focus, ..Default::default() };
        let (params, asked, no_position) = (&self.params, &self.asked, &self.no_position);
        let r = self.ui.validated_input_with("height", &mut self.source, &opts, |t| {
            asked.borrow_mut().push(t.to_string());
            evaluate(t, &params.borrow()).map(|_| ()).map_err(|e| if no_position.get() { FieldError::new(e.message) } else { e })
        });
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

    fn type_text(&mut self, s: &str) -> ValidatedResponse {
        self.ui.push(InputEvent::Text(s.into()));
        self.frame()
    }

    fn key(&mut self, key: Key) -> ValidatedResponse {
        self.ui.push(InputEvent::Key { key, pressed: true, repeat: false });
        let r = self.frame();
        self.ui.push(InputEvent::Key { key, pressed: false, repeat: false });
        self.frame();
        r
    }
}

/// The reviewer's first point, and the whole of parametric CAD: the text is
/// the truth. Editing starts from `width * 2`, not the `40 mm` on show, and
/// committing it untouched keeps the expression.
#[test]
fn editing_starts_from_the_source_not_the_display() {
    let mut w = World::new("width * 2");
    w.click_field();
    let r = w.key(Key::Enter);
    assert_eq!(w.asked.borrow().last().map(String::as_str), Some("width * 2"), "the validator was handed the display, not the source");
    assert_eq!(w.source, "width * 2", "the expression was replaced by what it evaluated to");
    assert!(!r.committed, "committing the unchanged source reported a change");
}

#[test]
fn an_accepted_commit_writes_the_text_the_user_typed() {
    let mut w = World::new("width * 2");
    w.click_field();
    w.type_text("width + 5");
    assert_eq!(w.source, "width * 2", "the source changed before commit");
    let r = w.key(Key::Enter);
    assert!(r.committed);
    assert_eq!(w.source, "width + 5", "the committed text is not what was typed");
    assert_eq!(w.commits, 1);
}

/// The reviewer's third point: names resolve when asked, against whatever the
/// app's table holds then — not a list fixed when the field was built.
#[test]
fn names_resolve_when_the_app_is_asked() {
    let mut w = World::new("width * 2");
    w.click_field();
    w.type_text("depth * 3");
    let r = w.key(Key::Enter);
    assert!(r.error.is_some(), "an unknown parameter was accepted");
    assert_eq!(w.source, "width * 2");

    w.params.borrow_mut().insert("depth".into(), 7.0);
    let c = w.elsewhere.center();
    w.click(c);
    assert_eq!(w.source, "depth * 3", "a name the app defined later was not resolved");
    assert!(w.last.error.is_none(), "the error outlived the fix");
}

#[test]
fn the_validator_is_asked_only_on_commit() {
    let mut w = World::new("width * 2");
    let before = w.asked.borrow().len();
    w.click_field();
    w.type_text("width");
    w.type_text(" * 9");
    assert_eq!(w.asked.borrow().len(), before, "the validator ran on keystrokes");
    w.key(Key::Enter);
    assert_eq!(w.asked.borrow().len(), before + 1, "one commit, one question");
}

/// Refused on Enter, the user stays, and the caret is where the app said the
/// problem is — so the next keystroke is the fix.
#[test]
fn a_refused_enter_keeps_focus_at_the_problem() {
    let mut w = World::new("width * 2");
    w.click_field();
    w.type_text("width * x");
    let r = w.key(Key::Enter);
    let e = r.error.expect("x was accepted as a number");
    assert_eq!(e.at, Some(8));
    assert!(r.focused, "a refused Enter threw the user out");
    assert_eq!(w.source, "width * 2");

    // The caret is at the x, so a keystroke lands before it. Had it been at
    // the end, the validator would now be asked about "width * x3".
    w.type_text("3");
    w.key(Key::Enter);
    assert_eq!(w.asked.borrow().last().map(String::as_str), Some("width * 3x"), "the keystroke did not land at the error");
}

/// A parser that cannot say where it failed still works; the caret stays
/// where the user left it.
#[test]
fn an_error_without_a_position_is_fine() {
    let mut w = World::new("width * 2");
    w.no_position.set(true);
    w.click_field();
    w.type_text("width * x");
    let r = w.key(Key::Enter);
    let e = r.error.expect("refused");
    assert_eq!(e.at, None);
    assert!(r.focused);
    // The caret was at the end, after the x, and still is.
    w.ui.push(InputEvent::Key { key: Key::Backspace, pressed: true, repeat: false });
    w.frame();
    w.ui.push(InputEvent::Key { key: Key::Backspace, pressed: false, repeat: false });
    w.frame();
    w.type_text("4");
    w.no_position.set(false);
    w.key(Key::Enter);
    assert_eq!(w.source, "width * 4");
}

/// Refused on a click elsewhere: the user meant to leave, and focus is not
/// pulled back — but the refused text and its reason stay.
#[test]
fn a_refused_click_away_does_not_steal_focus_back() {
    let mut w = World::new("width * 2");
    w.click_field();
    w.type_text("nonsense");
    let c = w.elsewhere.center();
    w.click(c);
    assert!(!w.last.focused, "focus was pulled back into the field");
    assert!(w.last.error.is_some(), "the reason was lost");
    assert_eq!(w.source, "width * 2");
}

#[test]
fn escape_throws_the_edit_away() {
    let mut w = World::new("width * 2");
    w.click_field();
    w.type_text("width + 100");
    let r = w.key(Key::Escape);
    assert!(r.cancelled);
    assert!(!r.committed);
    assert_eq!(w.source, "width * 2");

    // And the next edit starts from the source again, not the thrown-away text.
    w.click_field();
    w.key(Key::Enter);
    assert_eq!(w.asked.borrow().last().map(String::as_str), Some("width * 2"));
}

#[test]
fn without_select_on_focus_typing_appends() {
    let mut w = World::new("width * 2");
    w.select_on_focus = false;
    w.click_field();
    w.type_text("0");
    w.key(Key::Enter);
    assert_eq!(w.source, "width * 20");
}

#[test]
fn source_changed_by_the_app_is_what_editing_starts_from() {
    let mut w = World::new("width * 2");
    w.source = "width + 1".into(); // an undo, another panel, a script
    w.frame();
    w.click_field();
    w.key(Key::Enter);
    assert_eq!(w.asked.borrow().last().map(String::as_str), Some("width + 1"));
}

#[test]
fn the_state_is_forgotten_when_the_field_goes() {
    let mut w = World::new("width * 2");
    w.click_field();
    w.type_text("nonsense");
    let c = w.elsewhere.center();
    w.click(c);
    assert!(w.last.error.is_some());

    w.ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 });
    let _ = w.ui.end_frame();

    let r = w.frame();
    assert!(r.error.is_none(), "a closed panel's refused edit came back with it");
}

/// The display is what is drawn while nobody is editing, and the source is
/// what is drawn once someone is. Counted in glyphs: a long display beside a
/// one-letter source cannot draw the same number of them.
#[test]
fn the_display_is_drawn_until_editing_begins() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.set_key_bindings(bindings());
    let info = FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 };
    let mut source = String::from("w");
    let mut draw = |ui: &mut Ui, display: Option<&str>| -> (usize, Rect) {
        ui.begin_frame(info);
        let opts = ValidatedOptions { display, ..Default::default() };
        let r = ui.validated_input_with("f", &mut source, &opts, |_| Ok(()));
        let out = ui.end_frame();
        (out.draw.instances.len(), r.response.rect)
    };
    for _ in 0..3 {
        draw(&mut ui, None);
    }
    let (plain, _) = draw(&mut ui, None);
    let (with_display, rect) = draw(&mut ui, Some("123456789"));
    assert_eq!(with_display, plain + 8, "the display was not what the idle field drew");

    // Focus it: now the source is on screen, whatever the display says.
    ui.push(InputEvent::PointerMoved { pos: rect.center() });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    draw(&mut ui, Some("123456789"));
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    draw(&mut ui, Some("123456789"));
    let (editing, _) = draw(&mut ui, Some("123456789"));
    // The caret and the selection are drawn while editing; the text is `w`.
    assert!(editing < with_display, "the display was still drawn while editing ({editing} vs {with_display})");
}
