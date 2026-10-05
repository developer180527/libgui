//! The demo driven the way a person drives it.

use libgui::*;
use libgui_springs::{Springs, SLOTS};

const FONT: &[u8] = include_bytes!("../../../../assets/Inter.ttf");

struct World {
    ui: Ui,
    app: Springs,
}

impl World {
    fn new() -> Self {
        let mut w = Self { ui: Ui::new(Theme::dark(), FONT).expect("font"), app: Springs::default() };
        for _ in 0..4 {
            w.frame();
        }
        w
    }

    fn frame(&mut self) {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(720.0, 760.0), scale: 1.0, dt: 1.0 / 60.0 });
        self.app.ui(&mut self.ui);
        drop(self.ui.end_frame());
    }

    fn track(&self) -> Rect {
        self.ui.rect_of(Id::new("puck")).expect("the puck's track was not built")
    }

    /// Press on the puck, drag it by `step` px a frame for `frames` frames,
    /// and let go. Returns where it was let go, in track space.
    fn throw(&mut self, step: f32, frames: usize) -> f32 {
        let track = self.track();
        let y = track.y + track.h * 0.5;
        let mut x = track.x + SLOTS[self.app.slot] * track.w;
        self.ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, y) });
        self.frame();
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        self.frame();
        for _ in 0..frames {
            x += step;
            self.ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, y) });
            self.frame();
        }
        self.ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        self.frame();
        x - track.x
    }
}

/// A hard throw to the right travels past where it was let go, to the far
/// slot — the velocity is what decides, not the release point.
#[test]
fn a_hard_throw_carries_to_the_far_slot() {
    let mut w = World::new();
    assert_eq!(w.app.slot, 0);
    // 24 px a frame at 60 Hz is 1440 px/s, let go a quarter of the way along.
    let released = w.throw(24.0, 4);
    assert!(released < SLOTS[1] * w.track().w, "the test let go too late to prove anything: {released}");
    assert_eq!(w.app.slot, 2, "a 1440 px/s throw did not carry to the far slot");
}

/// A gentle drag lands where it was put: the nearest slot.
#[test]
fn a_gentle_drag_lands_where_it_was_put() {
    let mut w = World::new();
    // 3 px a frame for 70 frames: 180 px/s, ending near the middle slot.
    w.throw(3.0, 70);
    assert_eq!(w.app.slot, 1, "a slow drag to the middle did not settle in the middle");
}

/// After a throw the demo keeps asking for frames while the puck moves, and
/// then lets the window sleep.
#[test]
fn a_thrown_puck_settles_and_the_window_can_sleep() {
    let mut w = World::new();
    w.throw(24.0, 4);
    assert!(w.ui.needs_frame(0.0), "the puck was thrown but no frame was asked for");
    let mut frames = 0;
    while w.ui.needs_frame(0.0) {
        w.frame();
        frames += 1;
        assert!(frames < 600, "still moving after ten seconds");
    }
}

/// Flipping the switches twice in quick succession: the ease is already
/// heading back, the springs are still going forward.
#[test]
fn the_springs_turn_where_the_ease_reverses() {
    let mut w = World::new();
    let flip = |w: &mut World| w.app.on = !w.app.on;
    flip(&mut w);
    for _ in 0..8 {
        w.frame();
    }
    // Read each value as it is mid-flight, then flip back.
    let before = |w: &mut World| {
        w.ui.begin_frame(FrameInfo { screen_size: Vec2::new(720.0, 760.0), scale: 1.0, dt: 1.0 / 240.0 });
        let target = if w.app.on { 1.0 } else { 0.0 };
        let ease = w.ui.animate_with_speed(Id::new("ease"), 0, target, 9.0);
        let spring = w.ui.animate_spring_with(Id::new("critical"), 0, target, Spring::new(w.app.response, 1.0));
        drop(w.ui.end_frame());
        (ease, spring)
    };
    let (ease0, spring0) = before(&mut w);
    flip(&mut w);
    let (ease1, spring1) = before(&mut w);
    assert!(ease1 < ease0, "the ease did not reverse on the spot: {ease0} -> {ease1}");
    assert!(spring1 > spring0, "the spring reversed on the spot: {spring0} -> {spring1}");
}


/// With reduced motion a throw still picks its slot by velocity, and the puck
/// is drawn there on the very next frame — which the window is asked for. It
/// used to stay drawn where it was let go until a mouse move woke the window.
#[test]
fn with_reduced_motion_a_throw_arrives_on_the_next_frame() {
    let mut w = World::new();
    w.app.reduced_motion = true;
    w.frame();
    let released = w.throw(24.0, 4);
    assert_eq!(w.app.slot, 2, "the throw's velocity should still choose the slot");
    assert!((w.app.shown_x - released).abs() < 1.0, "the release frame shows it where it was let go");
    assert!(w.ui.needs_frame(0.0), "nothing asked for the frame that puts it in its slot");
    w.frame();
    let home = SLOTS[2] * w.track().w;
    assert_eq!(w.app.shown_x, home, "with reduced motion it should be in its slot a frame later");
}
