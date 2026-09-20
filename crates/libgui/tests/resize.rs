//! Resizing is not input.
//!
//! No `InputEvent` describes a resize: the new size reaches the UI only through
//! `FrameInfo`. That makes it the one change a frame-skipping host can miss
//! entirely — it reuses the batches it built at the old size, so the window
//! edge moves and the UI inside it does not follow until some unrelated event
//! wakes it up. These tests pin the signal that tells a host otherwise.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn info(w: f32, h: f32, scale: f32) -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(w, h), scale, dt: 1.0 / 60.0 }
}

/// A panel with something in it, so the frame has geometry that depends on size.
fn build(ui: &mut Ui, info: FrameInfo) {
    ui.begin_frame(info);
    ui.label("Render Geometry Settings");
    let _ = ui.button("Apply");
    drop(ui.end_frame());
}

#[test]
fn a_resize_asks_for_a_frame_even_though_no_input_arrived() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let start = info(1800.0, 1000.0, 2.0);

    // Settle: run until the UI stops asking for frames of its own accord.
    for _ in 0..8 {
        build(&mut ui, start);
    }
    assert!(!ui.needs_frame(1.0), "the UI has settled, so nothing is pending");
    assert!(!ui.needs_frame_for(&start, 1.0), "and the same size still needs nothing");

    // Drag the window edge. Not one event is pushed — this is the whole point.
    let smaller = info(1799.0, 1000.0, 2.0);
    assert!(
        !ui.needs_frame(1.0),
        "the trap this exists for: the size-blind check still says no, which is \
         why a host must not gate a resizable window on it"
    );
    assert!(
        ui.needs_frame_for(&smaller, 1.0),
        "a single pixel of resize must force a rebuild: the batches on the GPU \
         are the wrong size now"
    );

    // Moving to a different display changes the scale, not the logical size.
    let rescaled = info(1800.0, 1000.0, 1.5);
    assert!(ui.needs_frame_for(&rescaled, 1.0), "a scale change is a resize too");
}

/// The cheap check must stay cheap: once the new size is built, it goes quiet.
#[test]
fn it_stops_asking_once_the_new_size_is_built() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    for _ in 0..8 {
        build(&mut ui, info(1800.0, 1000.0, 2.0));
    }
    let dragged = info(1400.0, 900.0, 2.0);
    assert!(ui.needs_frame_for(&dragged, 1.0), "the new size needs a frame");
    build(&mut ui, dragged);
    for _ in 0..4 {
        build(&mut ui, dragged);
    }
    assert!(
        !ui.needs_frame_for(&dragged, 1.0),
        "a resize that has been built and settled must not pin the UI awake"
    );
}
