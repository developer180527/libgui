//! Numbers a real app will eventually hand the UI: a NaN dimension from a CAD
//! computation, a zero-size window during a resize, a font size from a zoomed
//! canvas, a clock that hiccupped across a suspend.
//!
//! Two rules are tested here.
//!
//! 1. **Nothing panics.** A UI library that aborts the process because a value
//!    was odd is not one a tool can be built on.
//! 2. **Nothing sticks.** libgui's own retained state — animation values,
//!    scroll offsets — never keeps a non-finite number, so one bad frame
//!    cannot corrupt a widget for the rest of the session. Geometry the app
//!    passes straight to a draw call is its own business: it draws nothing.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn ok_info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 }
}

/// A frame of a typical panel, as bytes.
fn panel(ui: &mut Ui, info: FrameInfo) -> Vec<u8> {
    ui.begin_frame(info);
    ui.heading("Inspector");
    let mut v = 0.35;
    let _ = ui.slider("Scale", &mut v, 0.0, 1.0);
    let _ = ui.button("Apply");
    ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(80.0)), |ui| {
        for i in 0..20 {
            let _ = ui.selectable_keyed(i, &format!("Row {i}"), false);
        }
    });
    let out = ui.end_frame();
    bytemuck::cast_slice(&out.draw.instances).to_vec()
}

/// Every one of these used to be a panic, a NaN in retained state, or both.
#[test]
fn pathological_values_never_panic() {
    let bad = [
        ("zero window", FrameInfo { screen_size: Vec2::ZERO, ..ok_info() }),
        ("negative window", FrameInfo { screen_size: Vec2::new(-100.0, -50.0), ..ok_info() }),
        ("nan window", FrameInfo { screen_size: Vec2::new(f32::NAN, f32::NAN), ..ok_info() }),
        ("huge window", FrameInfo { screen_size: Vec2::new(1e9, 1e9), ..ok_info() }),
        ("zero scale", FrameInfo { scale: 0.0, ..ok_info() }),
        ("nan scale", FrameInfo { scale: f32::NAN, ..ok_info() }),
        ("infinite scale", FrameInfo { scale: f32::INFINITY, ..ok_info() }),
        ("nan dt", FrameInfo { dt: f32::NAN, ..ok_info() }),
        ("negative dt", FrameInfo { dt: -1.0, ..ok_info() }),
        ("suspended dt", FrameInfo { dt: 1e6, ..ok_info() }),
    ];
    for (name, info) in bad {
        let mut ui = ui();
        for _ in 0..3 {
            panel(&mut ui, info);
        }
        println!("{name}: survived");
    }

    // App data, through every widget that turns a number into geometry.
    let mut ui = ui();
    for _ in 0..3 {
        ui.begin_frame(ok_info());
        let mut nan = f32::NAN;
        let _ = ui.slider("s", &mut nan, 0.0, 1.0);
        let mut half = 0.5;
        let _ = ui.slider("range", &mut half, f32::NAN, f32::NAN);
        let _ = ui.slider("inverted", &mut half, 1.0, 0.0);
        let _ = ui.drag_value("d", &mut nan, f32::NAN);
        ui.progress("p", Some(f32::NAN));
        ui.text_with("huge", 1e6, Color::WHITE);
        ui.text_with("nan", f32::NAN, Color::WHITE);
        ui.text_with("negative", -20.0, Color::WHITE);
        let id = ui.make_id("leaf");
        ui.add_leaf(id, Layout::leaf(Size::Fixed(f32::NAN), Size::Fixed(f32::NAN)), Vec2::ZERO, false, |_, _| {});
        ui.container(Layout::column().padding(Insets::all(-50.0)), Frame::none(), |ui| ui.label("negative padding"));
        let mut st = CanvasState { zoom: f32::NAN, pan: Vec2::new(f32::NAN, f32::NAN), ..CanvasState::default() };
        ui.canvas("c", &mut st, |ui, _| ui.label("in a broken canvas"));
        let _ = ui.end_frame();
    }
}

/// A glyph too big for the atlas is skipped, not rasterised: fontdue indexes
/// its coverage buffer with an `i32` and overflows on the way there.
#[test]
fn an_absurd_font_size_draws_nothing_and_costs_nothing() {
    let mut ui = ui();
    ui.begin_frame(ok_info());
    ui.text_with("visible", 14.0, Color::WHITE);
    ui.text_with("far too big", 200_000.0, Color::WHITE);
    let out = ui.end_frame();
    let n = out.draw.instances.len();
    assert_eq!(n, "visible".len(), "only the readable text should have been drawn, got {n} instances");
    assert!(ui.frame_cost().glyphs_rasterized <= 7, "the absurd size was rasterised anyway");
}

/// One bad frame from the host must not outlive itself: the frame after it is
/// the frame a UI that never saw it would have drawn.
#[test]
fn a_bad_frame_from_the_host_does_not_stick() {
    for bad in [
        FrameInfo { dt: f32::NAN, ..ok_info() },
        FrameInfo { scale: f32::NAN, ..ok_info() },
        FrameInfo { screen_size: Vec2::new(f32::NAN, f32::NAN), ..ok_info() },
        FrameInfo { dt: f32::INFINITY, ..ok_info() },
    ] {
        let mut hurt = ui();
        let mut clean = ui();
        // Same history, except one frame.
        panel(&mut hurt, ok_info());
        panel(&mut clean, ok_info());
        panel(&mut hurt, bad);
        panel(&mut clean, ok_info());
        for f in 0..6 {
            let (a, b) = (panel(&mut hurt, ok_info()), panel(&mut clean, ok_info()));
            assert_eq!(a, b, "{bad:?} was still showing {f} frames later");
        }
    }
}

/// A scroll area handed a NaN height keeps working once the app stops: its
/// offset is retained, so a NaN there would stay for good.
#[test]
fn a_scroll_area_recovers_from_a_nan_size() {
    let mut hurt = ui();
    let mut clean = ui();
    let frame = |ui: &mut Ui, h: f32| -> Vec<u8> {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(40.0, 40.0) });
        ui.push(InputEvent::Wheel { delta: Vec2::new(0.0, -40.0), unit: WheelUnit::Pixel });
        ui.begin_frame(ok_info());
        ui.scroll_area_with("list", ScrollOptions::new(Size::Fixed(h)), |ui| {
            for i in 0..30 {
                let _ = ui.selectable_keyed(i, &format!("Row {i}"), false);
            }
        });
        let out = ui.end_frame();
        bytemuck::cast_slice(&out.draw.instances).to_vec()
    };
    frame(&mut hurt, 100.0);
    frame(&mut clean, 100.0);
    frame(&mut hurt, f32::NAN);
    frame(&mut clean, 100.0);
    for f in 0..6 {
        let (a, b) = (frame(&mut hurt, 100.0), frame(&mut clean, 100.0));
        assert_eq!(a, b, "the scroll area never recovered, frame {f}");
    }
}
