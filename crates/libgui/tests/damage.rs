//! Frame skipping: what a host that redraws for its *own* reasons is allowed
//! to leave undone.
//!
//! The rule `needs_frame` promises is narrow and worth stating exactly: if it
//! says no, the frame you would have built is byte-identical to the one you
//! already have. So every test here checks both halves — that a static UI
//! says no, and that anything a user can actually perceive says yes *before*
//! the frame that would show it.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const STEP: f32 = 1.0 / 120.0;

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(500.0, 400.0), scale: 2.0, dt: STEP }
}

/// A panel with everything that might want a frame: hover, focus, a scroll
/// area, a tooltip.
fn panel(ui: &mut Ui, text: &mut String) {
    ui.heading("Inspector");
    let b = ui.button("Apply");
    ui.tooltip(&b, "Applies the change");
    ui.text_input("field", text, "Name…");
    ui.scroll_area("list", |ui| {
        for i in 0..200 {
            ui.with_key(i, |ui| {
                let _ = ui.selectable("Row", false);
            });
        }
    });
}

/// Run frames while they are asked for, and report how many actually ran out
/// of `frames` opportunities.
fn run(ui: &mut Ui, text: &mut String, frames: usize) -> usize {
    let mut ran = 0;
    let mut waited = 0.0;
    for _ in 0..frames {
        waited += STEP;
        if !ui.needs_frame(waited) {
            continue;
        }
        ui.begin_frame(FrameInfo { dt: waited, ..info() });
        panel(ui, text);
        let _ = ui.end_frame();
        ran += 1;
        waited = 0.0;
    }
    ran
}

/// The bytes a frame produces, for proving a skipped frame would have been
/// identical rather than merely close.
fn snapshot(ui: &mut Ui, text: &mut String) -> Vec<u8> {
    ui.begin_frame(info());
    panel(ui, text);
    let out = ui.end_frame();
    bytemuck::cast_slice(&out.draw.instances).to_vec()
}

#[test]
fn a_static_ui_stops_asking_for_frames() {
    let mut ui = ui();
    let mut text = String::new();
    // Settle: hover fades and the scroll area's bar fade have to finish.
    run(&mut ui, &mut text, 400);
    assert_eq!(run(&mut ui, &mut text, 600), 0, "a UI with nothing happening kept redrawing");
}

#[test]
fn the_frame_a_skip_would_have_produced_is_the_one_you_already_have() {
    let mut ui = ui();
    let mut text = String::new();
    run(&mut ui, &mut text, 400);
    assert!(!ui.needs_frame(10.0), "still busy after settling");

    let a = snapshot(&mut ui, &mut text);
    let b = snapshot(&mut ui, &mut text);
    assert_eq!(a, b, "two frames of a settled UI differ, so skipping one would be visible");
    assert!(!a.is_empty());
}

#[test]
fn any_queued_input_asks_for_a_frame() {
    let mut ui = ui();
    let mut text = String::new();
    run(&mut ui, &mut text, 400);
    assert!(!ui.needs_frame(10.0));

    ui.push(InputEvent::PointerMoved { pos: Vec2::new(10.0, 10.0) });
    assert!(ui.needs_frame(0.0), "a queued event did not ask for a frame");
}

#[test]
fn hovering_asks_until_the_animation_finishes_and_then_stops() {
    let mut ui = ui();
    let mut text = String::new();
    run(&mut ui, &mut text, 400);

    // Onto the button (y 19..49): the hover fade has to run.
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(40.0, 34.0) });
    let during = run(&mut ui, &mut text, 20);
    assert!(during > 3, "a hover fade ran in {during} frames, so it was not animated");

    // Well inside the tooltip delay, and it must keep asking: on a host that
    // sleeps between frames, a tooltip that stops asking never appears.
    assert!(ui.needs_frame(10.0), "a pending tooltip stopped asking for frames");
    run(&mut ui, &mut text, 400);
    assert_eq!(run(&mut ui, &mut text, 200), 0, "a shown tooltip kept asking forever");
}

#[test]
fn a_focused_caret_asks_only_as_often_as_it_blinks() {
    let mut ui = ui();
    let mut text = String::new();
    run(&mut ui, &mut text, 400);

    // Click into the text field, which sits at y 49..79.
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 64.0) });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    run(&mut ui, &mut text, 2);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    run(&mut ui, &mut text, 400);
    assert!(ui.focused().is_some(), "the click did not focus the field");

    // A blink is half a second: over two seconds at 120 Hz that is a handful
    // of frames, not 240.
    let ran = run(&mut ui, &mut text, 240);
    assert!(ran > 0 && ran <= 8, "a blinking caret ran {ran} frames in two seconds");
}

#[test]
fn a_fresh_ui_always_wants_its_first_frame() {
    let ui = ui();
    assert!(ui.needs_frame(0.0), "a Ui that has never drawn thought it had nothing to do");
}

#[test]
fn a_scroll_asks_until_it_settles() {
    let mut ui = ui();
    let mut text = String::new();
    run(&mut ui, &mut text, 400);
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(250.0, 300.0) });
    run(&mut ui, &mut text, 400);

    ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -1.0), unit: WheelUnit::Line });
    let ran = run(&mut ui, &mut text, 120);
    assert!(ran > 3, "a wheel notch eased in {ran} frames, so it did not animate");
    assert_eq!(run(&mut ui, &mut text, 200), 0, "a settled scroll kept asking");
}
