//! A gear icon that spins while a build runs.
//!
//! This is the whole answer to "how do I animate my own icon", and it needs
//! nothing libgui does not already have: the icon is drawn from points, the
//! points are yours, so you rotate them. `Ui::animate_with_speed` supplies an
//! angle that keeps climbing, and the paint closure turns it into geometry.
//!
//! The limit is worth knowing, and it is the reason this test exists: a
//! *textured* icon cannot do this. `Instance` is an axis-aligned rect with no
//! angle in it, so an image can be moved, scaled, tinted and masked, but not
//! turned. Vector icons can.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// A gear: teeth around a hub, as one closed polyline.
fn gear_points(centre: Vec2, radius: f32, teeth: usize, angle: f32, out: &mut Vec<Vec2>) {
    out.clear();
    let steps = teeth * 4;
    for i in 0..=steps {
        let t = i as f32 / steps as f32 * std::f32::consts::TAU + angle;
        // Alternate between the tooth tip and the root, four samples per tooth,
        // which is what gives the square-ish tooth profile.
        let r = if (i / 2) % 2 == 0 { radius } else { radius * 0.72 };
        out.push(Vec2::new(centre.x + r * t.cos(), centre.y + r * t.sin()));
    }
}

/// The button: a gear that turns while `building`.
fn compile_button(ui: &mut Ui, building: bool, phase: f32) -> Response {
    let id = ui.make_id("compile");
    let r = ui.interact_focusable(id, FocusKind::Control);
    // The angle is app state, not UI state. libgui's `animate_*` helpers ease
    // towards a target and stop, which is the opposite of a spin, so the phase
    // lives in the caller and advances by `dt`. `request_repaint` is what keeps
    // the frames coming on a host that skips idle ones.
    let spin = if building {
        ui.request_repaint();
        phase
    } else {
        0.0
    };
    let colour = ui.theme.palette.text;
    ui.add_leaf(id, Layout::leaf(Size::Fixed(24.0), Size::Fixed(24.0)), Vec2::ZERO, true, move |p, rect| {
        let mut pts = Vec::new();
        gear_points(rect.center(), 8.0, 6, spin * std::f32::consts::TAU, &mut pts);
        p.polyline(&pts, 1.5, colour);
    });
    r
}

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(200.0, 100.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// The instances a frame drew, as raw bytes, so "did the picture change" is a
/// question with a yes/no answer.
fn frame(ui: &mut Ui, building: bool, phase: f32) -> Vec<u8> {
    ui.begin_frame(info());
    compile_button(ui, building, phase);
    let out = ui.end_frame();
    let bytes = bytemuck::cast_slice(&out.draw.instances).to_vec();
    drop(out);
    bytes
}

#[test]
fn the_gear_turns_while_building_and_rests_when_it_is_not() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");

    // At rest: the same picture every frame, and no reason to draw another.
    let a = frame(&mut ui, false, 0.0);
    let b = frame(&mut ui, false, 0.0);
    assert_eq!(a, b, "a parked gear must be byte-identical between frames");
    assert!(!ui.needs_frame(1.0), "a parked gear must not hold the host awake");

    // Building: every frame differs, and the UI keeps asking for the next one.
    let mut phase = 0.0f32;
    let mut last = frame(&mut ui, true, phase);
    for i in 0..30 {
        assert!(ui.needs_frame(1.0), "frame {i}: a spinning gear must keep asking to be drawn");
        phase += (1.0 / 60.0) * 0.5;
        let next = frame(&mut ui, true, phase);
        assert_ne!(last, next, "frame {i}: the gear did not move");
        last = next;
    }

    // And it comes back to rest.
    frame(&mut ui, false, 0.0);
    let a = frame(&mut ui, false, 0.0);
    let b = frame(&mut ui, false, 0.0);
    assert_eq!(a, b, "the gear must settle once the build finishes");
}
