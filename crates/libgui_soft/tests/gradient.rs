//! `Painter::gradient`: two instances, no new primitive, and the exact
//! interpolation at every pixel.

use libgui::mesh::Mesh;
use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn unorm(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Draw one gradient filling a `w` x `h` window and return the frame, the
/// image and the instance count.
fn draw(scale: f32, w: f32, h: f32, bg: Color, from: Color, to: Color, axis: Axis) -> (libgui_soft::Target, libgui_soft::Target, usize, u32) {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut out_n = 0;
    let mut rasterized = 0;
    let mut images = None;
    for frame in 0..3 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(w, h), scale, dt: 1.0 / 60.0 });
        let id = ui.make_id("g");
        ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| {
            p.rect(r, bg, 0.0);
            p.gradient(r, from, to, axis);
        });
        let out = ui.end_frame();
        out_n = out.draw.instances.len();
        if frame == 2 {
            let (pw, ph) = ((w * scale).round() as u32, (h * scale).round() as u32);
            let a = SoftRenderer::new().render_to_image(&out, pw, ph);
            let mut mesh = Mesh::new();
            mesh.build(out.draw);
            let b = SoftRenderer::new().render_mesh_to_image(&out, &mesh, pw, ph);
            images = Some((a, b));
        }
        drop(out);
        rasterized = ui.frame_cost().glyphs_rasterized;
    }
    let (a, b) = images.unwrap();
    (a, b, out_n, rasterized)
}

/// Every pixel against `from + (to - from) * t`, `t` at the pixel's centre.
fn check(scale: f32, axis: Axis, bg: Color, from: Color, to: Color, what: &str) {
    let (w, h) = (300.0, 120.0);
    let (a, b, instances, rasterized) = draw(scale, w, h, bg, from, to, axis);
    // Background, fill and ramp; no fill when `from` is transparent.
    let want = if from.a > 0.0 { 3 } else { 2 };
    assert_eq!(instances, want, "{what}: a gradient is not two instances over its background");
    assert_eq!(rasterized, 0, "{what}: a steady frame rasterised its ramp again");
    for (path, img) in [("instanced", &a), ("triangles", &b)] {
        let mut worst = 0u8;
        for y in (0..img.height).step_by(7) {
            for x in (0..img.width).step_by(5) {
                let t = match axis {
                    Axis::X => (x as f32 + 0.5) / img.width as f32,
                    Axis::Y => (y as f32 + 0.5) / img.height as f32,
                };
                // Over the background: the fill (if any), then `to` by t.
                let base = if from.a > 0.0 { from } else { bg };
                let k = t * to.a;
                let want = [base.r + (to.r - base.r) * k, base.g + (to.g - base.g) * k, base.b + (to.b - base.b) * k];
                let got = img.pixel(x, y);
                for c in 0..3 {
                    worst = worst.max(got[c].abs_diff(unorm(want[c])));
                }
            }
        }
        assert!(worst <= 2, "{what} at {scale}x, {path}: a pixel is {worst} steps from the interpolation");
    }
}

#[test]
fn a_gradient_is_the_interpolation_at_every_pixel() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let blue = Color::rgba(0.0, 0.2, 1.0, 1.0);
    let grey = Color::rgba(0.3, 0.3, 0.3, 1.0);
    for scale in [1.0, 1.5, 2.0] {
        check(scale, Axis::X, grey, red, blue, "red to blue, across");
        check(scale, Axis::Y, grey, red, blue, "red to blue, down");
        check(scale, Axis::X, grey, Color::TRANSPARENT, blue, "fading in over a background");
        check(scale, Axis::Y, grey, Color::WHITE, Color::rgba(0.0, 0.0, 0.0, 1.0), "white to black, down");
    }
}

/// The ramp is shared: many gradients, one rasterisation, ever.
#[test]
fn many_gradients_share_one_ramp() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut first = 0;
    for frame in 0..2 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        for i in 0..40 {
            let id = ui.make_id(("g", i));
            ui.add_leaf(id, Layout::leaf(Size::Fixed(10.0 + i as f32 * 7.0), Size::Fixed(8.0)), Vec2::ZERO, false, |p, r| {
                p.gradient(r, Color::WHITE, Color::rgba(0.0, 0.0, 1.0, 1.0), Axis::X);
                p.gradient(r, Color::TRANSPARENT, Color::BLACK, Axis::Y);
            });
        }
        let _ = ui.end_frame();
        if frame == 0 {
            first = ui.frame_cost().glyphs_rasterized;
        }
    }
    assert!(first <= 2, "40 gradients of 40 sizes rasterised {first} ramps; two are all there are");
    assert_eq!(ui.frame_cost().glyphs_rasterized, 0);
}

/// Inside a zoomed canvas the gradient is drawn through the transform like
/// everything else, and still interpolates end to end.
#[test]
fn a_gradient_inside_a_canvas_follows_the_zoom() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut st = CanvasState { zoom: 2.0, ..CanvasState::default() };
    let mut out_img = None;
    for _ in 0..3 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 100.0), scale: 1.0, dt: 1.0 / 60.0 });
        ui.canvas("c", &mut st, |ui, _| {
            let id = ui.make_id("g");
            ui.add_leaf_at(id, Rect::new(10.0, 10.0, 100.0, 20.0), LeafOptions::default(), |p, r| {
                p.gradient(r, Color::BLACK, Color::WHITE, Axis::X);
            });
        });
        let out = ui.end_frame();
        out_img = Some(SoftRenderer::new().render_to_image(&out, 300, 100));
    }
    let img = out_img.unwrap();
    // Canvas (10..110, 10..30) at 2x from the canvas origin: window 20..220, 20..60.
    let (l, m, r) = (img.pixel(21, 40)[0], img.pixel(120, 40)[0], img.pixel(218, 40)[0]);
    assert!(l < 8 && r > 247, "the ends are not black and white: {l} .. {r}");
    assert!(m.abs_diff(128) <= 3, "the middle is not half way: {m}");
}
