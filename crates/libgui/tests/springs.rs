//! Springs through `Ui`: what an app sees.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

/// One frame at `hz`, reading the spring toward `target`.
fn frame(ui: &mut Ui, hz: f32, target: f32, spring: Spring) -> f32 {
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(200.0, 100.0), scale: 1.0, dt: 1.0 / hz });
    let id = Id::new("drawer");
    ui.keep_id(id);
    let v = ui.animate_spring_with(id, 0, target, spring);
    drop(ui.end_frame());
    v
}

/// Nothing animates into existence: the first value is the target.
#[test]
fn a_spring_starts_where_it_is_first_asked_to_be() {
    let mut ui = ui();
    assert_eq!(frame(&mut ui, 60.0, 240.0, Spring::SNAPPY), 240.0);
    assert!(!ui.needs_frame(0.0), "a spring at rest asked for a frame");
}

/// It asks for frames exactly while it moves, then lets the host sleep.
#[test]
fn it_asks_for_frames_until_it_rests_and_not_after() {
    let mut ui = ui();
    frame(&mut ui, 60.0, 0.0, Spring::SNAPPY);
    let mut frames = 0;
    loop {
        let v = frame(&mut ui, 60.0, 100.0, Spring::SNAPPY);
        frames += 1;
        if v == 100.0 {
            break;
        }
        assert!(ui.needs_frame(0.0), "frame {frames}: moving at {v} but not asking to be drawn");
        assert!(frames < 120, "never came to rest");
    }
    assert!(!ui.needs_frame(0.0), "at rest and still asking for frames");
}

/// The same motion at 30 and 144 Hz: the frames sample one curve.
#[test]
fn it_moves_the_same_at_any_frame_rate() {
    let at = |hz: f32, frames: usize| {
        let mut ui = ui();
        frame(&mut ui, hz, 0.0, Spring::BOUNCY);
        let mut v = 0.0;
        for _ in 0..frames {
            v = frame(&mut ui, hz, 1.0, Spring::BOUNCY);
        }
        v
    };
    // 1/6 s: 5 frames at 30 Hz, 24 at 144 Hz.
    let (slow, fast) = (at(30.0, 5), at(144.0, 24));
    assert!((slow - fast).abs() < 1e-3, "30 Hz reached {slow}, 144 Hz reached {fast}");
}

/// Told to go back mid-flight, it carries on for a moment and turns — the
/// ease reverses on the spot. Measured over a few short frames, which is how
/// the eye sees it.
#[test]
fn an_interrupted_spring_turns_rather_than_reversing() {
    let mut ui = ui();
    frame(&mut ui, 240.0, 0.0, Spring::SMOOTH);
    let mut v = 0.0;
    for _ in 0..24 {
        v = frame(&mut ui, 240.0, 1.0, Spring::SMOOTH);
    }
    let turned_at = v;
    // Now back to 0. The next frame is still further along.
    let next = frame(&mut ui, 240.0, 0.0, Spring::SMOOTH);
    assert!(next > turned_at, "it reversed on the spot: {turned_at} -> {next}");
}

/// Reduced motion: the value arrives at once, and nothing asks for frames.
#[test]
fn reduced_motion_arrives_at_once() {
    let mut ui = ui();
    ui.theme.metrics.reduced_motion = true;
    frame(&mut ui, 60.0, 0.0, Spring::BOUNCY);
    assert_eq!(frame(&mut ui, 60.0, 300.0, Spring::BOUNCY), 300.0);
    assert!(!ui.needs_frame(0.0));
}

/// A thrown thing keeps going: handed a release velocity toward its target,
/// a springy spring sails past and comes back.
#[test]
fn a_release_velocity_carries_through() {
    let mut ui = ui();
    let id = Id::new("drawer");
    frame(&mut ui, 60.0, 0.0, Spring::BOUNCY);
    ui.set_spring(id, 0, 0.0, 3000.0);
    assert_eq!(ui.spring_velocity(id, 0), 3000.0);
    let mut furthest = 0.0f32;
    for _ in 0..90 {
        furthest = furthest.max(frame(&mut ui, 60.0, 100.0, Spring::BOUNCY));
    }
    assert!(furthest > 120.0, "thrown at 3000 px/s it only reached {furthest}");
    assert_eq!(frame(&mut ui, 60.0, 100.0, Spring::BOUNCY), 100.0, "it did not come home");
}

/// Like every other retained state, a spring whose widget stops being built
/// is forgotten, and starts fresh at its target when it comes back.
#[test]
fn a_spring_is_forgotten_with_its_widget() {
    let mut ui = ui();
    frame(&mut ui, 60.0, 0.0, Spring::SNAPPY);
    frame(&mut ui, 60.0, 100.0, Spring::SNAPPY);
    // A frame without it.
    ui.begin_frame(FrameInfo::default());
    drop(ui.end_frame());
    assert_eq!(frame(&mut ui, 60.0, 50.0, Spring::SNAPPY), 50.0, "a forgotten spring remembered its motion");
}

/// The pointer's velocity is the speed it is actually moving at.
#[test]
fn the_pointer_velocity_is_its_speed() {
    let mut ui = ui();
    let mut x = 0.0;
    for _ in 0..30 {
        x += 10.0; // 10 px a frame at 60 Hz = 600 px/s
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, 50.0) });
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(800.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        drop(ui.end_frame());
    }
    let v = ui.pointer_velocity();
    assert!((v.x - 600.0).abs() < 10.0 && v.y.abs() < 1.0, "a 600 px/s drag measured {v:?}");
}

/// Theme files written before springs existed still load, and get the
/// default spring.
#[cfg(feature = "theme-toml")]
#[test]
fn a_theme_file_from_before_springs_still_loads() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes");
    for entry in std::fs::read_dir(&dir).expect("themes/") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read");
        let theme = Theme::from_toml(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(theme.metrics.spring, Spring::SNAPPY, "{}", path.display());
        assert!(!theme.metrics.reduced_motion, "{}", path.display());
    }
}

/// A spring on an id no widget is built with — the natural thing to write for
/// a value that is not a widget — still moves. Retained state is dropped for
/// ids not seen in a frame, and this one used to be dropped every frame and
/// restarted at its target, so the motion silently never happened.
#[test]
fn a_spring_on_a_bare_id_keeps_its_motion() {
    let mut ui = ui();
    let id = Id::new("not a widget");
    let step = |ui: &mut Ui, target: f32| {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(200.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        let v = ui.animate_spring_with(id, 0, target, Spring::SMOOTH);
        drop(ui.end_frame());
        v
    };
    step(&mut ui, 0.0);
    let a = step(&mut ui, 1.0);
    let b = step(&mut ui, 1.0);
    assert!(a > 0.0 && a < 1.0, "it did not start moving: {a}");
    assert!(b > a && b < 1.0, "it restarted instead of carrying on: {a} -> {b}");
}

/// A spring moved by hand asks for the frame that shows it moving.
///
/// A drag hands over on the frame it is let go, after the spring has already
/// been read that frame. With reduced motion the spring had just reported
/// itself at rest, so nothing asked for another frame: the host slept with
/// the thing drawn where the pointer let go, and it only jumped home when
/// some unrelated input — a mouse move — woke the window.
#[test]
fn a_hand_off_asks_for_the_frame_that_shows_it() {
    for reduced in [false, true] {
        let mut ui = ui();
        ui.theme.metrics.reduced_motion = reduced;
        let id = Id::new("puck");
        let frame = |ui: &mut Ui, release: bool| {
            ui.begin_frame(FrameInfo { screen_size: Vec2::new(200.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
            let v = ui.animate_spring_with(id, 0, 52.0, Spring::BOUNCY);
            if release {
                ui.set_spring(id, 0, 148.0, 900.0);
            }
            drop(ui.end_frame());
            v
        };
        frame(&mut ui, false);
        frame(&mut ui, false);
        frame(&mut ui, true);
        assert!(ui.needs_frame(0.0), "reduced={reduced}: the hand-off did not ask to be drawn");
        let next = frame(&mut ui, false);
        if reduced {
            assert_eq!(next, 52.0, "with reduced motion it should arrive on the next frame");
            assert!(!ui.needs_frame(0.0), "arrived, and still asking for frames");
        } else {
            assert!(next > 148.0, "it should be carried on by the throw: {next}");
        }
    }
}
