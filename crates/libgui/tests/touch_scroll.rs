//! Touch scrolling, frame by frame, without a window: a finger is a list of
//! `InputEvent::Touch`es, and where the content went is the first button's rect.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

struct World {
    ui: Ui,
    /// Buttons clicked in the last frame.
    clicked: Vec<usize>,
}

impl World {
    fn new() -> Self {
        Self { ui: Ui::new(Theme::dark(), FONT).expect("font"), clicked: Vec::new() }
    }

    /// Builds one frame; returns where the first button is: the scroll offset,
    /// negated. A `Response`'s rect is the previous frame's layout, so this
    /// is one frame behind.
    fn frame(&mut self) -> f32 {
        self.ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 400.0), scale: 2.0, dt: 1.0 / 60.0 });
        let mut top = 0.0;
        let mut clicked = Vec::new();
        self.ui.scroll_area("list", |ui| {
            for i in 0..200 {
                let r = ui.button(&format!("Item {i}"));
                if i == 0 {
                    top = r.rect.y;
                }
                if r.clicked {
                    clicked.push(i);
                }
            }
        });
        let _ = self.ui.end_frame();
        self.clicked = clicked;
        top
    }

    fn touch(&mut self, phase: TouchPhase, y: f32) -> f32 {
        self.ui.push(InputEvent::Touch { id: 1, phase, pos: Vec2::new(20.0, y) });
        self.frame()
    }

    /// A quick upward flick, released mid-motion: the content keeps going.
    fn fling(&mut self) {
        self.frame();
        self.touch(TouchPhase::Start, 300.0);
        for k in 1..=5 {
            self.touch(TouchPhase::Move, 300.0 - 30.0 * k as f32);
        }
        self.touch(TouchPhase::End, 150.0);
    }
}

#[test]
fn a_flick_coasts_after_the_finger_lifts() {
    let mut w = World::new();
    w.fling();
    let a = w.frame();
    let b = w.frame();
    assert!(b < a, "still coasting: {a} then {b}");
}

/// The bug: a finger held still on a coasting list never got past the touch
/// slop, so the list was not "being touched" and kept coasting under it.
#[test]
fn a_finger_landing_on_a_coasting_list_stops_it() {
    let mut w = World::new();
    w.fling();
    w.frame();
    w.touch(TouchPhase::Start, 200.0);
    let caught = w.frame();
    let mut held = Vec::new();
    for _ in 0..10 {
        held.push(w.frame());
    }
    assert!(held.iter().all(|&y| y == caught), "moved under a still finger: caught at {caught}, then {held:?}");
    w.touch(TouchPhase::End, 200.0);
    let clicked = std::mem::take(&mut w.clicked);
    let after = [w.frame(), w.frame()];
    assert_eq!(after, [caught; 2], "set off again after the finger lifted");
    assert!(clicked.is_empty() && w.clicked.is_empty(), "catching it clicked {clicked:?} {:?}", w.clicked);
}

/// Catching does not lock the list: the same finger can then drag it, and
/// without first waiting out the slop.
#[test]
fn a_caught_list_follows_the_finger_at_once() {
    let mut w = World::new();
    w.fling();
    w.frame();
    w.touch(TouchPhase::Start, 200.0);
    let caught = w.frame();
    w.touch(TouchPhase::Move, 205.0);
    let dragged = w.frame();
    assert_eq!(dragged, caught + 5.0);
}

/// A list at rest is untouched by all this: a tap on it is still a tap.
#[test]
fn a_tap_on_a_list_at_rest_still_clicks() {
    let mut w = World::new();
    w.frame();
    w.touch(TouchPhase::Start, 15.0);
    w.touch(TouchPhase::End, 15.0);
    assert_eq!(w.clicked, [0]);
}
