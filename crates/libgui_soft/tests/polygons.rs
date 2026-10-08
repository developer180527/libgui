//! Filled polygons through the whole pipeline (WO-3): no seams, the right
//! area, and no CPU rasterisation for a shape that changes every frame.

use libgui::*;
use libgui_soft::{SoftRenderer, Target};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn shot(w: f32, h: f32, scale: f32, f: impl Fn(&mut Painter, Rect) + 'static) -> Target {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(w, h), scale, dt: 1.0 / 60.0 });
    ui.container(Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)), Frame { fill: Color::BLACK, ..Frame::none() }, |ui| {
        ui.add_leaf(Id::new("c"), Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| f(p, r));
    });
    let out = ui.end_frame();
    SoftRenderer::new().render_to_image(&out, (w * scale) as u32, (h * scale) as u32)
}

fn circle(c: Vec2, r: f32, n: usize) -> Vec<Vec2> {
    (0..n).map(|i| {
        let a = i as f32 * std::f32::consts::TAU / n as f32;
        Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect()
}

fn star(c: Vec2, n: usize, r0: f32, r1: f32) -> Vec<Vec2> {
    (0..n * 2).map(|i| {
        let a = i as f32 * std::f32::consts::PI / n as f32;
        let r = if i % 2 == 0 { r0 } else { r1 };
        Vec2::new(c.x + r * a.cos(), c.y + r * a.sin())
    }).collect()
}

/// Half-transparent white over black is exactly 128 wherever it is one
/// layer. A pixel drawn by two triangles of the same shape comes out brighter
/// (a seam that overlaps); one drawn by neither comes out black (a gap).
/// Inside, away from the outline, every pixel must be exactly 128.
#[test]
fn a_translucent_fill_has_no_seams() {
    let half = Color::rgba(1.0, 1.0, 1.0, 0.5);
    for scale in [1.0f32, 1.5, 2.0] {
        let t = shot(320.0, 220.0, scale, move |p, _| {
            p.fill_polygon(&circle(Vec2::new(70.0, 70.0), 60.0, 300), half);
            p.fill_polygon(&star(Vec2::new(200.0, 70.0), 9, 60.0, 25.0), half);
            let outer = [Vec2::new(10.0, 140.0), Vec2::new(150.0, 140.0), Vec2::new(150.0, 210.0), Vec2::new(10.0, 210.0)];
            let hole = [Vec2::new(50.0, 160.0), Vec2::new(110.0, 160.0), Vec2::new(110.0, 190.0), Vec2::new(50.0, 190.0)];
            p.fill_polygon_with_holes(&outer, &[&hole], half);
            // A mesh with a shared diagonal.
            let pts = [Vec2::new(180.0, 140.0), Vec2::new(300.0, 140.0), Vec2::new(300.0, 210.0), Vec2::new(180.0, 210.0)];
            p.fill_mesh(&pts, &[0, 1, 2, 0, 2, 3], half);
        });
        let brightest = t.data.chunks(4).map(|p| p[0]).max().unwrap();
        assert_eq!(brightest, 128, "@{scale}x: a pixel was covered twice");
        // Well inside each shape: one layer, exactly.
        let s = scale;
        for (x, y) in [(70.0, 70.0), (40.0, 50.0), (200.0, 70.0), (205.0, 60.0), (30.0, 150.0), (130.0, 200.0), (240.0, 175.0), (200.0, 150.0), (290.0, 200.0)] {
            assert_eq!(t.pixel((x * s) as u32, (y * s) as u32)[0], 128, "@{scale}x: ({x}, {y}) is not one layer");
        }
        // The hole is a hole.
        assert_eq!(t.pixel((80.0 * s) as u32, (175.0 * s) as u32)[0], 0, "@{scale}x: the hole was filled");
    }
}

/// Anti-aliased by area: the ink adds up to the polygon's area, so edges are
/// neither fattened nor thinned.
#[test]
fn the_ink_is_the_area() {
    let pts = star(Vec2::new(100.0, 100.0), 7, 80.0, 35.0);
    let area: f32 = {
        let mut a = 0.0;
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            a += p.x * q.y - q.x * p.y;
        }
        (a * 0.5).abs()
    };
    let shape = pts.clone();
    let t = shot(200.0, 200.0, 1.0, move |p, _| p.fill_polygon(&shape, Color::WHITE));
    let ink: f32 = t.data.chunks(4).map(|p| p[0] as f32 / 255.0).sum();
    assert!((ink - area).abs() / area < 0.003, "ink {ink:.1} against an area of {area:.1}");
}

/// A shape that changes every frame is triangles, not a rasterised path: no
/// glyph-atlas work, ever, and instances in proportion to its corners.
#[test]
fn a_changing_shape_costs_no_rasterisation() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let mut texels = 0;
    for f in 0..20 {
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 });
        let phase = f as f32 * 0.3;
        ui.add_leaf(Id::new("blob"), Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, _| {
            let blob: Vec<Vec2> = (0..2000)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 2000.0;
                    let r = 150.0 + 20.0 * (a * 7.0 + phase).sin();
                    Vec2::new(200.0 + r * a.cos(), 200.0 + r * a.sin())
                })
                .collect();
            p.fill_polygon(&blob, Color::WHITE);
        });
        let out = ui.end_frame();
        let tris = out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Triangle.code()).count();
        assert_eq!(tris, 1998, "frame {f}: a 2,000-corner shape is 1,998 triangles");
        if f == 0 {
            texels = out.atlas.texels();
        }
        assert_eq!(out.atlas.texels(), texels);
        drop(out);
        assert_eq!(ui.frame_cost().glyphs_rasterized, 0, "frame {f}: the shape was rasterised on the CPU");
    }
}

/// Shared edges exactly on pixel centres — the case the tie rule exists for:
/// a 4 x 4 grid of cells whose inner edges run along x.5 and y.5, each cell
/// two triangles. Every pixel inside is one layer: none twice, none missed.
#[test]
fn edges_on_pixel_centres_belong_to_exactly_one_triangle() {
    let half = Color::rgba(1.0, 1.0, 1.0, 0.5);
    let t = shot(120.0, 120.0, 1.0, move |p, _| {
        let mut pts = Vec::new();
        for j in 0..5 {
            for i in 0..5 {
                pts.push(Vec2::new(10.5 + i as f32 * 20.0, 10.5 + j as f32 * 20.0));
            }
        }
        let mut idx = Vec::new();
        for j in 0..4u32 {
            for i in 0..4u32 {
                let (a, b, c, d) = (j * 5 + i, j * 5 + i + 1, (j + 1) * 5 + i + 1, (j + 1) * 5 + i);
                // Alternate the diagonal, so shared edges run every way.
                if (i + j) % 2 == 0 {
                    idx.extend([a, b, c, a, c, d]);
                } else {
                    idx.extend([a, b, d, b, c, d]);
                }
            }
        }
        p.fill_mesh(&pts, &idx, half);
    });
    for y in 13..88 {
        for x in 13..88 {
            assert_eq!(t.pixel(x, y)[0], 128, "({x}, {y}) is not exactly one layer");
        }
    }
}
