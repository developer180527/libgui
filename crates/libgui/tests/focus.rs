//! Keyboard focus, driven entirely by `UiAction`s.
//!
//! Not a single test here presses a key. libgui's widgets never look at keys —
//! which chord means "next" or "activate" is the keymap's, and a host may have
//! no keyboard at all — so the tests speak the same vocabulary a gamepad, a
//! foot pedal or an accessibility switch would: `InputEvent::Action`.
//!
//! The thing being proved is blunt: a form can be filled in and submitted
//! without the pointer ever existing.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    // Deliberately *not* a platform default: these tests are about the
    // mechanism, so they state the policy they are testing.
    ui.focus_policy = FocusPolicy::default();
    ui
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 2.0, dt: 1.0 / 60.0 }
}

#[derive(Default)]
struct Form {
    name: String,
    enabled: bool,
    level: f32,
    submitted: u32,
    cancelled: u32,
}

impl Form {
    fn frame(&mut self, ui: &mut Ui) {
        ui.begin_frame(info());
        ui.text_input("name", &mut self.name, "Name…");
        ui.checkbox("Enabled", &mut self.enabled);
        ui.slider("Level", &mut self.level, 0.0, 1.0);
        if ui.button("Apply").clicked {
            self.submitted += 1;
        }
        if ui.button("Cancel").clicked {
            self.cancelled += 1;
        }
        let _ = ui.end_frame();
    }
}

fn act(ui: &mut Ui, a: UiAction) {
    ui.push(InputEvent::Action(a));
}

/// Tab round the form and read back which widget the focus landed on.
fn focused_after(ui: &mut Ui, form: &mut Form, steps: usize) -> Vec<Option<Id>> {
    let mut seen = Vec::new();
    for _ in 0..steps {
        act(ui, UiAction::FocusNext);
        form.frame(ui);
        seen.push(ui.focused());
    }
    seen
}

#[test]
fn the_keyboard_reaches_every_control_and_comes_back_round() {
    let mut ui = ui();
    let mut form = Form::default();
    form.frame(&mut ui);

    // Five stops: the field, the checkbox, the slider and two buttons.
    let stops = focused_after(&mut ui, &mut form, 5);
    assert!(stops.iter().all(|s| s.is_some()), "focus fell off the form: {stops:?}");
    let unique: std::collections::HashSet<_> = stops.iter().collect();
    assert_eq!(unique.len(), 5, "Tab did not visit five distinct widgets: {stops:?}");

    // One more wraps back to the first.
    act(&mut ui, UiAction::FocusNext);
    form.frame(&mut ui);
    assert_eq!(ui.focused(), stops[0], "focus did not wrap round");
}

#[test]
fn a_form_can_be_filled_in_and_submitted_with_no_pointer_at_all() {
    let mut ui = ui();
    let mut form = Form::default();
    form.frame(&mut ui);

    // Into the text field, and type.
    act(&mut ui, UiAction::FocusNext);
    form.frame(&mut ui);
    ui.push(InputEvent::Text("Kick".into()));
    form.frame(&mut ui);
    assert_eq!(form.name, "Kick");

    // On to the checkbox, and turn it on.
    act(&mut ui, UiAction::FocusNext);
    form.frame(&mut ui);
    act(&mut ui, UiAction::Submit);
    form.frame(&mut ui);
    assert!(form.enabled, "the checkbox did not respond to the keyboard");

    // Past the slider to Apply, and press it.
    act(&mut ui, UiAction::FocusNext);
    form.frame(&mut ui);
    act(&mut ui, UiAction::FocusNext);
    form.frame(&mut ui);
    act(&mut ui, UiAction::Submit);
    form.frame(&mut ui);
    assert_eq!(form.submitted, 1, "Apply did not activate from the keyboard");
    assert_eq!(form.cancelled, 0, "the wrong button activated");
}

#[test]
fn shift_tab_walks_back() {
    let mut ui = ui();
    let mut form = Form::default();
    form.frame(&mut ui);
    let forward = focused_after(&mut ui, &mut form, 3);
    act(&mut ui, UiAction::FocusPrevious);
    form.frame(&mut ui);
    assert_eq!(ui.focused(), forward[1], "back did not undo forward");
}

#[test]
fn cancel_gives_focus_back() {
    let mut ui = ui();
    let mut form = Form::default();
    form.frame(&mut ui);
    act(&mut ui, UiAction::FocusNext);
    form.frame(&mut ui);
    assert!(ui.focused().is_some());
    act(&mut ui, UiAction::Cancel);
    form.frame(&mut ui);
    assert_eq!(ui.focused(), None, "Cancel left focus where it was");
}

/// The policy is the app's. A host following macOS's convention visits text
/// and lists and nothing else, and libgui obliges without knowing why.
#[test]
fn a_narrower_policy_skips_the_controls_it_excludes() {
    let mut ui = ui();
    ui.focus_policy = FocusPolicy::text_only();
    let mut form = Form::default();
    form.frame(&mut ui);

    let stops = focused_after(&mut ui, &mut form, 4);
    let unique: std::collections::HashSet<_> = stops.iter().flatten().collect();
    assert_eq!(unique.len(), 1, "a text-only policy visited more than the text field: {stops:?}");

    // And the buttons are still perfectly clickable.
    let mut ui2 = self::ui();
    ui2.focus_policy = FocusPolicy::text_only();
    let mut f2 = Form::default();
    f2.frame(&mut ui2);
    ui2.push(InputEvent::PointerMoved { pos: Vec2::new(30.0, 120.0) });
    f2.frame(&mut ui2);
    ui2.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    f2.frame(&mut ui2);
    ui2.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    f2.frame(&mut ui2);
    assert!(f2.submitted + f2.cancelled > 0 || f2.enabled, "an unfocusable widget stopped working entirely");
}

/// Narrowing the policy while something excluded has focus must not strand it.
#[test]
fn focus_does_not_get_stuck_on_a_widget_the_policy_stops_visiting() {
    let mut ui = ui();
    let mut form = Form::default();
    form.frame(&mut ui);
    for _ in 0..4 {
        act(&mut ui, UiAction::FocusNext);
        form.frame(&mut ui);
    }
    assert!(ui.focused().is_some());
    ui.focus_policy = FocusPolicy::text_only();
    form.frame(&mut ui);
    form.frame(&mut ui);
    let f = ui.focused();
    assert!(f.is_none() || f == Some(Id::new("root").with(("text", "name"))), "focus was stranded on {f:?}");
}

/// A ring under the mouse is noise; a ring after Tab is the only way to know
/// where you are. So it follows *how* focus arrived, not merely that it did.
#[test]
fn the_focus_ring_appears_for_the_keyboard_and_not_for_a_click() {
    // Is there an instance exactly where a ring around the focused widget
    // would be? Precise, and it says what it is looking for.
    let ring_drawn = |ui: &mut Ui, form: &mut Form| -> bool {
        ui.begin_frame(info());
        ui.text_input("name", &mut form.name, "Name…");
        ui.checkbox("Enabled", &mut form.enabled);
        ui.slider("Level", &mut form.level, 0.0, 1.0);
        let _ = ui.button("Apply");
        let _ = ui.button("Cancel");
        let out = ui.end_frame();
        let instances = out.draw.instances.clone();
        drop(out);
        let Some(f) = ui.focused() else { return false };
        let Some(r) = ui.rect_of(f) else { return false };
        let w = ui.theme.metrics.focus_ring_width;
        let want = r.expand(w * 0.5);
        instances.iter().any(|i| {
            (i.rect[0] - want.x).abs() < 0.01
                && (i.rect[1] - want.y).abs() < 0.01
                && (i.rect[2] - want.w).abs() < 0.01
                && (i.rect[3] - want.h).abs() < 0.01
                && (i.params[1] - w).abs() < 0.01
        })
    };

    let mut ui = ui();
    let mut form = Form::default();
    ring_drawn(&mut ui, &mut form);
    assert!(!ring_drawn(&mut ui, &mut form), "a ring appeared with nothing focused");

    // Tab onto the checkbox. Focus moves at the end of a frame, after paint,
    // so the ring lands on the next one — the library's usual one-frame rule.
    act(&mut ui, UiAction::FocusNext);
    ring_drawn(&mut ui, &mut form);
    act(&mut ui, UiAction::FocusNext);
    ring_drawn(&mut ui, &mut form);
    assert!(ring_drawn(&mut ui, &mut form), "Tab drew no focus ring");

    // Click the same checkbox. Focus stays on it, the ring goes away: it is
    // there to say where the *keyboard* is.
    let checkbox = ui.focused().expect("focused");
    let at = ui.rect_of(checkbox).expect("checkbox rect").center();
    ui.push(InputEvent::PointerMoved { pos: at });
    ring_drawn(&mut ui, &mut form);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    ring_drawn(&mut ui, &mut form);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    ring_drawn(&mut ui, &mut form);
    assert_eq!(ui.focused(), Some(checkbox), "the click moved focus off the widget it pressed");
    assert!(!ring_drawn(&mut ui, &mut form), "a click drew a focus ring");
}

/// Two clicks in the same place, close together in time, are a double click —
/// and a third starts a new pair rather than reporting one every frame.
#[test]
fn a_double_click_is_two_clicks_on_one_widget() {
    use libgui::*;
    const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");

    let at = Vec2::new(40.0, 20.0);
    // Returns (clicked, double_clicked) for the frame, rather than writing to
    // a captured variable the assertions would have to borrow around.
    fn frame(ui: &mut Ui, dt: f32, events: Vec<InputEvent>) -> (bool, bool) {
        for e in events {
            ui.push(e);
        }
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(200.0, 100.0), scale: 1.0, dt });
        let r = ui.button("target");
        let out = (r.clicked, r.double_clicked);
        let _ = ui.end_frame();
        out
    }
    let down = InputEvent::PointerButton { button: PointerButton::Primary, pressed: true };
    let up = InputEvent::PointerButton { button: PointerButton::Primary, pressed: false };
    // One click: press on one frame, release on the next.
    let click = |ui: &mut Ui| {
        frame(ui, 0.016, vec![InputEvent::PointerButton { button: PointerButton::Primary, pressed: true }]);
        frame(ui, 0.016, vec![InputEvent::PointerButton { button: PointerButton::Primary, pressed: false }])
    };

    frame(&mut ui, 0.016, vec![InputEvent::PointerMoved { pos: at }]);
    assert_eq!(click(&mut ui), (true, false), "the first click was a double click");
    assert_eq!(click(&mut ui), (true, true), "the second click was not a double click");
    // A third starts a new pair: a run of clicks goes single, double, single,
    // double — not double for every one after the first.
    assert_eq!(click(&mut ui), (true, false), "the third click reported a double as well");

    // Too slow is two single clicks.
    assert_eq!(click(&mut ui), (true, true));
    frame(&mut ui, 2.0, vec![]);
    assert_eq!(click(&mut ui), (true, false), "clicks a second apart counted as a double");

    // And so is clicking somewhere else in between.
    frame(&mut ui, 0.016, vec![InputEvent::PointerMoved { pos: Vec2::new(180.0, 90.0) }]);
    let _ = click(&mut ui);
    frame(&mut ui, 0.016, vec![InputEvent::PointerMoved { pos: at }]);
    assert_eq!(click(&mut ui), (true, false), "a click elsewhere did not end the pair");
    let _ = (down, up);
}
