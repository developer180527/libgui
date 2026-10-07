//! Dashed lines and turned images and text, rendered by the reference
//! renderer and read back pixel by pixel: what the pattern and the rotation
//! *mean*, which the GPU parity test cannot say (it only says both agree).

use libgui::*;
use libgui_soft::{SoftRenderer, Target, Texture};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const WHITE: Color = Color::WHITE;

/// Paint on black into a `w` x `h` logical target at 1x, with `tex`
/// registered first if given.
fn shot(w: f32, h: f32, tex: Option<Texture>, f: impl Fn(&mut Painter, Option<TextureId>) + 'static) -> Target {
    let mut soft = SoftRenderer::new();
    let id = tex.map(|t| soft.register_texture(t));
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let f = std::rc::Rc::new(f);
    let mut out = None;
    for _ in 0..2 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(w, h), scale: 1.0, dt: 1.0 / 60.0 });
        let f = f.clone();
        ui.container(Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)), Frame { fill: Color::BLACK, ..Frame::none() }, |ui| {
            ui.add_leaf(Id::new("c"), Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, _| f(p, id));
        });
        out = Some(soft.render_to_image(&ui.end_frame(), w as u32, h as u32));
    }
    out.expect("a frame")
}

fn lit(t: &Target, x: u32, y: u32) -> u8 {
    t.pixel(x, y)[0]
}

/// 4 on, 4 off from x = 10: every pixel centre inside a dash is full and
/// every one inside a gap is empty.
#[test]
fn a_dashed_line_is_on_and_off_where_the_pattern_says() {
    let t = shot(60.0, 20.0, None, |p, _| p.dashed_line(Vec2::new(10.0, 10.0), Vec2::new(50.0, 10.0), 2.0, WHITE, Dash::even(4.0)));
    for x in 11..49u32 {
        let s = x as f32 + 0.5 - 10.0; // distance along the line
        let m = s % 8.0;
        if (0.5..3.5).contains(&m) {
            assert_eq!(lit(&t, x, 9), 255, "x={x} is inside a dash");
        } else if (4.5..7.5).contains(&m) {
            assert_eq!(lit(&t, x, 9), 0, "x={x} is inside a gap");
        }
    }
}

/// The phase slides the pattern along: half a period swaps dashes and gaps.
#[test]
fn the_phase_slides_the_pattern() {
    let line = |phase: f32| shot(60.0, 20.0, None, move |p, _| p.dashed_line(Vec2::new(10.0, 10.0), Vec2::new(50.0, 10.0), 2.0, WHITE, Dash::even(4.0).phase(phase)));
    let (a, b) = (line(0.0), line(4.0));
    for x in [12u32, 13, 20, 21, 28, 29] {
        assert_eq!(lit(&a, x, 9), 255, "unshifted dash at {x}");
        assert_eq!(lit(&b, x, 9), 0, "a half-period phase left a dash at {x}");
    }
}

/// A polyline's dashes run on across its joins: the second segment starts
/// where the first left off, not with a fresh dash. 6 on, 6 off; the first
/// segment is 9 long, so the second starts 3 into a gap.
#[test]
fn a_dashed_polyline_carries_its_phase_across_joins() {
    let t = shot(60.0, 40.0, None, |p, _| {
        let end = p.dashed_polyline(&[Vec2::new(10.0, 10.0), Vec2::new(19.0, 10.0), Vec2::new(19.0, 35.0)], 2.0, WHITE, Dash::even(6.0));
        assert_eq!(end, 34.0, "the returned phase is the length drawn");
    });
    // Down the second segment, from y = 10: still in the gap until y = 13,
    // then a dash to y = 19.
    assert_eq!(lit(&t, 18, 12), 0, "the second segment restarted the pattern instead of continuing it");
    assert_eq!(lit(&t, 18, 16), 255);
}

/// Zero for either length is a solid line, identical to `line`.
#[test]
fn an_empty_pattern_is_a_solid_line() {
    let solid = shot(60.0, 20.0, None, |p, _| p.line(Vec2::new(10.0, 10.0), Vec2::new(50.0, 10.0), 2.0, WHITE));
    let zero = shot(60.0, 20.0, None, |p, _| p.dashed_line(Vec2::new(10.0, 10.0), Vec2::new(50.0, 10.0), 2.0, WHITE, Dash::new(0.0, 4.0)));
    assert_eq!(solid, zero);
}

/// A texture whose left half is red and right half blue, turned a quarter
/// turn clockwise, has red on top and blue below.
#[test]
fn a_quarter_turn_puts_the_left_edge_on_top() {
    let mut data = Vec::new();
    for _y in 0..4 {
        for x in 0..4 {
            data.extend_from_slice(if x < 2 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
        }
    }
    let tex = Texture { width: 4, height: 4, data };
    let t = shot(80.0, 80.0, Some(tex), |p, id| {
        p.image_rotated(Rect::new(20.0, 30.0, 40.0, 20.0), id.unwrap(), [0.0, 0.0, 1.0, 1.0], 0.0, WHITE, ImageAlpha::Opaque, std::f32::consts::FRAC_PI_2);
    });
    // Turned, the 40 x 20 rect stands 20 wide and 40 tall about (40, 40).
    assert_eq!(t.pixel(40, 25), [255, 0, 0, 255], "the left half did not turn to the top");
    assert_eq!(t.pixel(40, 55), [0, 0, 255, 255], "the right half did not turn to the bottom");
    assert_eq!(t.pixel(25, 40), [0, 0, 0, 255], "the turned image still covers its old width");
    // The edges are anti-aliased rather than cut: something between the
    // image and the background along the turned edge at a slant.
    let slanted = shot(80.0, 80.0, Some(Texture { width: 1, height: 1, data: vec![255, 255, 255, 255] }), |p, id| {
        p.image_rotated(Rect::new(20.0, 20.0, 40.0, 40.0), id.unwrap(), [0.0, 0.0, 1.0, 1.0], 0.0, WHITE, ImageAlpha::Opaque, 0.4);
    });
    let partial = slanted.data.chunks(4).filter(|p| p[0] > 20 && p[0] < 235).count();
    assert!(partial > 40, "a turned edge has only {partial} partly covered pixels: it is aliased");
    // And faded symmetrically about the true edge: the ink adds up to the
    // square's area. Cut at the quad instead, the outer half of every edge's
    // fade is lost and the square comes out small.
    let ink: f64 = slanted.data.chunks(4).map(|p| p[0] as f64 / 255.0).sum();
    assert!((ink - 1600.0).abs() < 4.0, "a turned 40 x 40 square has {ink:.1} px of ink, not 1600");
}

/// Text turned a quarter turn stands tall instead of lying wide, and keeps
/// its ink: the same glyphs, so roughly the same amount lit.
#[test]
fn turned_text_stands_up_and_keeps_its_ink() {
    let extent = |t: &Target| {
        let (mut x0, mut x1, mut y0, mut y1, mut ink) = (u32::MAX, 0, u32::MAX, 0, 0u32);
        for y in 0..t.height {
            for x in 0..t.width {
                let v = t.pixel(x, y)[0] as u32;
                if v > 64 {
                    (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
                }
                ink += v;
            }
        }
        (x1 - x0, y1 - y0, ink)
    };
    let upright = shot(160.0, 160.0, None, |p, _| p.text_rotated(Vec2::new(80.0, 80.0), 16.0, WHITE, "Height (mm)", 0.0));
    let turned = shot(160.0, 160.0, None, |p, _| p.text_rotated(Vec2::new(80.0, 80.0), 16.0, WHITE, "Height (mm)", -std::f32::consts::FRAC_PI_2));
    let (uw, uh, uink) = extent(&upright);
    let (tw, th, tink) = extent(&turned);
    assert!(uw > uh * 3 && th > tw * 3, "upright {uw}x{uh}, turned {tw}x{th}: it did not stand up");
    assert!(th.abs_diff(uw) <= 3, "the turned line is {th} long, the upright one {uw}");
    let ratio = tink as f64 / uink as f64;
    assert!((0.85..1.15).contains(&ratio), "turning changed the ink by {ratio}: glyphs lost or doubled");
}
