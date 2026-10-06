//! Notifications: what shows, when it goes, and what it costs to wait.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const SCREEN: Vec2 = Vec2::new(800.0, 600.0);

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// One frame of `dt` seconds with the stack drawn last.
fn frame(ui: &mut Ui, dt: f32) -> (ToastResponse, Option<f32>) {
    frame_with(ui, dt, ToastOptions::default())
}

fn frame_with(ui: &mut Ui, dt: f32, opts: ToastOptions) -> (ToastResponse, Option<f32>) {
    ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt });
    ui.label("The app");
    let r = ui.show_toasts_with(opts);
    let out = ui.end_frame();
    (r, out.platform.repaint_after)
}

fn card(n: u64) -> Id {
    Id::new(("libgui_toast", n)).with("card")
}

fn run(ui: &mut Ui, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        frame(ui, 1.0 / 60.0);
    }
}

fn click(ui: &mut Ui, at: Vec2) -> ToastResponse {
    ui.push(InputEvent::PointerMoved { pos: at });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    let mut r = frame(ui, 1.0 / 60.0).0;
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r2 = frame(ui, 1.0 / 60.0).0;
    r.action = r.action.or(r2.action);
    r.closed = r.closed.or(r2.closed);
    r
}

#[test]
fn a_toast_slides_in_at_the_corner_and_leaves_on_time() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    let id = ui.toast(Toast::success("Saved bracket_v3.step").duration(2.0));
    assert_eq!(id, ToastId(1));
    run(&mut ui, 0.5);
    let r = ui.rect_of(card(1)).expect("the toast was not drawn");
    assert!(r.right() > SCREEN.x - 40.0 && r.bottom() > SCREEN.y - 40.0, "not at the bottom-right corner: {r:?}");
    run(&mut ui, 1.0);
    assert_eq!(ui.toast_count(), 1, "it left before its two seconds were up");
    run(&mut ui, 1.5);
    assert_eq!(ui.toast_count(), 0, "it outstayed its two seconds");
    run(&mut ui, 1.0);
    assert!(ui.rect_of(card(1)).is_none(), "it was still drawn after sliding out");
}

/// Waiting for a notification to expire must not keep the host drawing: the
/// frame says when it is due, and one frame with the real elapsed time —
/// what the host passes after sleeping that long — is enough to expire it.
#[test]
fn a_waiting_toast_lets_the_host_sleep_until_it_is_due() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    ui.toast(Toast::info("Export finished").duration(3.0));
    run(&mut ui, 1.0); // slid in (a third of a second), every animation settled
    let (_, after) = frame(&mut ui, 1.0 / 60.0);
    let due = after.expect("a waiting toast let the host sleep forever");
    assert!(due > 2.0 && due < 2.7, "the host was asked to wake in {due} s, not when the toast is due");
    // The host sleeps that long, and says so.
    frame(&mut ui, due);
    assert_eq!(ui.toast_count(), 0, "one frame after sleeping until it was due, it had not expired");
}

#[test]
fn the_pointer_over_a_toast_holds_it() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    ui.toast(Toast::info("Hover me").duration(1.0));
    run(&mut ui, 0.5);
    let r = ui.rect_of(card(1)).unwrap();
    ui.push(InputEvent::PointerMoved { pos: r.center() });
    run(&mut ui, 5.0);
    assert_eq!(ui.toast_count(), 1, "it left while the pointer was on it");
    ui.push(InputEvent::PointerMoved { pos: Vec2::new(10.0, 10.0) });
    run(&mut ui, 1.5);
    assert_eq!(ui.toast_count(), 0, "it never left once the pointer did");
}

#[test]
fn an_error_stays_until_closed_and_closing_says_so() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    let id = ui.toast(Toast::error("Export failed: disk full"));
    run(&mut ui, 30.0);
    assert_eq!(ui.toast_count(), 1, "an error went away on its own");
    let x = ui.rect_of(Id::new(("libgui_toast", 1u64)).with("close")).expect("no close button");
    let r = click(&mut ui, x.center());
    assert_eq!(r.closed, Some(id));
    assert_eq!(ui.toast_count(), 0);
}

#[test]
fn an_action_is_reported_and_dismisses() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    let id = ui.toast(Toast::info("Deleted 3 parts").action("Undo"));
    run(&mut ui, 0.5);
    let b = ui.rect_of(Id::new(("libgui_toast", 1u64)).with("action")).expect("no action button");
    let r = click(&mut ui, b.center());
    assert_eq!(r.action, Some(id), "the action was not reported");
    assert_eq!(r.closed, None, "pressing the action was also reported as closing");
    assert_eq!(ui.toast_count(), 0, "the action did not dismiss it");
}

#[test]
fn too_many_wait_their_turn_and_their_clocks_wait_too() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    for i in 0..6 {
        ui.toast(Toast::info(format!("Note {i}")).duration(2.0));
    }
    run(&mut ui, 0.5);
    let drawn = (1..=6).filter(|n| ui.rect_of(card(*n)).is_some()).count();
    assert_eq!(drawn, 4, "{drawn} drawn at once");
    // The first four go (in, two seconds, out); the last two then get their
    // full two seconds.
    run(&mut ui, 2.5);
    assert!(ui.rect_of(card(5)).is_some(), "the fifth did not take a freed place");
    run(&mut ui, 1.0);
    assert!(ui.rect_of(card(6)).is_some() && ui.toast_count() == 2, "the waiting ones had their time used up while they waited");
}

#[test]
fn every_corner() {
    for (corner, check) in [
        (ToastCorner::TopLeft, (true, true)),
        (ToastCorner::TopRight, (false, true)),
        (ToastCorner::BottomLeft, (true, false)),
        (ToastCorner::BottomRight, (false, false)),
    ] {
        let mut ui = ui();
        let opts = ToastOptions { corner, ..ToastOptions::default() };
        frame_with(&mut ui, 1.0 / 60.0, opts);
        ui.toast(Toast::info("Corner"));
        for _ in 0..30 {
            frame_with(&mut ui, 1.0 / 60.0, opts);
        }
        let r = ui.rect_of(card(1)).expect("not drawn");
        let (left, top) = check;
        assert_eq!(r.x < SCREEN.x / 2.0, left, "{corner:?}: {r:?}");
        assert_eq!(r.y < SCREEN.y / 2.0, top, "{corner:?}: {r:?}");
    }
}

#[test]
fn a_toast_pushed_between_frames_wakes_an_idle_window() {
    let mut ui = ui();
    run(&mut ui, 1.0);
    assert!(!ui.needs_frame(0.0), "the window was not idle to begin with");
    ui.toast(Toast::info("From an event handler"));
    assert!(ui.needs_frame(0.0), "a new toast did not ask for the frame that shows it");
}

#[test]
fn the_kind_is_the_themes_colour() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    ui.toast(Toast::success("Saved"));
    run(&mut ui, 0.5);
    ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
    ui.show_toasts();
    let green = ui.theme.palette.success.to_array();
    let out = ui.end_frame();
    assert!(out.draw.instances.iter().any(|i| i.color == green), "no part of a success toast was the theme's success colour");
}

/// The clock starts once the toast has fully slid in, so a short one is read
/// for its whole duration rather than spending part of it arriving.
#[test]
fn the_clock_starts_once_it_is_in() {
    let mut ui = ui();
    frame(&mut ui, 1.0 / 60.0);
    ui.toast(Toast::info("Brief").duration(0.3));
    run(&mut ui, 0.35);
    assert_eq!(ui.toast_count(), 1, "the slide-in used up its time");
}

/// A host that slept passes the whole sleep as `dt`. Animations are spared the
/// leap, but timing is not: two clicks two seconds apart are two clicks.
#[test]
fn a_long_dt_is_real_time_for_clicks() {
    let mut ui = ui();
    let id = Id::new("target");
    let mut double = false;
    for (dt, pressed) in [(1.0 / 60.0, true), (1.0 / 60.0, false), (1.0, true), (1.0, false)] {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(5.0, 5.0) });
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed });
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt });
        let r = ui.interact(id);
        ui.add_leaf(id, Layout::leaf(Size::Fixed(50.0), Size::Fixed(50.0)), Vec2::ZERO, true, |_, _| {});
        double |= r.double_clicked;
        drop(ui.end_frame());
    }
    assert!(!double, "two clicks two seconds apart read as a double-click");
}
