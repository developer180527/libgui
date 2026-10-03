//! Filled paths through the whole pipeline: built in a `Ui`, rasterised into
//! the atlas, drawn as glyphs, and rendered by the reference renderer.

use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// A play triangle in a 24 x 24 icon box, the commonest icon there is.
fn play() -> Path {
    Path::new(24.0, 24.0)
        .move_to(Vec2::new(6.0, 4.0))
        .line_to(Vec2::new(20.0, 12.0))
        .line_to(Vec2::new(6.0, 20.0))
        .close()
}

/// Draw `path` into a `size` box at (20, 20) on black, `frames` times, and
/// return the last image and what that frame cost.
fn shot(path: Path, size: f32, scale: f32, color: Color, frames: usize) -> (libgui_soft::Target, libgui::testing::FrameCost) {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.audit = true;
    let info = FrameInfo { screen_size: Vec2::new(200.0, 200.0), scale, dt: 1.0 / 60.0 };
    let mut img = None;
    for _ in 0..frames {
        ui.begin_frame(info);
        let p = path.clone();
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame { fill: Color::BLACK, ..Frame::none() }, |ui| {
            ui.add_leaf(Id::new("icon"), Layout::leaf(Size::Fixed(size), Size::Fixed(size)), Vec2::ZERO, false, move |pt, r| {
                pt.fill_path(&p, r, color);
            });
        });
        let out = ui.end_frame();
        let (w, h) = ((200.0 * scale) as u32, (200.0 * scale) as u32);
        img = Some(SoftRenderer::new().render_to_image(&out, w, h));
    }
    (img.expect("a frame"), ui.frame_cost())
}

/// The shape lands where it should, in the colour it was drawn in: inside is
/// the colour, outside is what was behind it.
#[test]
fn a_filled_path_draws_its_shape_in_its_colour() {
    let red = Color::rgba(1.0, 0.0, 0.0, 1.0);
    let (img, _) = shot(play(), 96.0, 1.0, red, 3);
    // The box is at (20, 20) and 96 px: 4 px per icon unit.
    let at = |ux: f32, uy: f32| img.pixel((20.0 + ux * 4.0) as u32, (20.0 + uy * 4.0) as u32);
    let inside = at(10.0, 12.0);
    assert!(inside[0] > 250 && inside[1] < 5 && inside[2] < 5, "inside the triangle is not red: {inside:?}");
    for (ux, uy) in [(2.0, 2.0), (22.0, 4.0), (22.0, 20.0), (3.0, 12.0)] {
        let px = at(ux, uy);
        assert!(px[0] < 5, "outside the triangle at ({ux}, {uy}) was painted: {px:?}");
    }
}

/// The point of putting it in the atlas: drawn every frame, rasterised once.
#[test]
fn a_path_is_rasterised_once_and_reused() {
    let (_, cost) = shot(play(), 48.0, 2.0, Color::WHITE, 4);
    assert_eq!(cost.glyphs_rasterized, 0, "a steady frame rasterised the icon again");
}

/// Rasterised at the size it is shown, so it is as sharp at 2x as at 1x:
/// the same icon at double the scale has its edge in the same place, not a
/// blurred enlargement.
#[test]
fn an_icon_is_crisp_at_every_scale() {
    // An axis-aligned square on the icon grid: its edges are exact pixels at
    // any whole scale, so any blur is a resampling artefact.
    let square = Path::polygon(24.0, 24.0, &[Vec2::new(6.0, 6.0), Vec2::new(18.0, 6.0), Vec2::new(18.0, 18.0), Vec2::new(6.0, 18.0)]);
    for scale in [1.0f32, 2.0, 3.0] {
        let (img, _) = shot(square.clone(), 48.0, scale, Color::WHITE, 3);
        let partial = img
            .data
            .chunks_exact(4)
            .filter(|p| p[0] > 3 && p[0] < 252)
            .count();
        assert_eq!(partial, 0, "at {scale}x the square has {partial} blurred pixels");
    }
}

/// A hole is a hole: a ring drawn as two circles, inner reversed, shows the
/// background in the middle.
#[test]
fn a_ring_shows_the_background_through_its_hole() {
    let ring = Path::new(24.0, 24.0).add_circle(Vec2::new(12.0, 12.0), 10.0, false).add_circle(Vec2::new(12.0, 12.0), 5.0, true);
    let (img, _) = shot(ring, 96.0, 1.0, Color::WHITE, 3);
    let centre = img.pixel(20 + 48, 20 + 48);
    let band = img.pixel(20 + 48 + 30, 20 + 48);
    assert!(centre[0] < 5, "the hole was filled: {centre:?}");
    assert!(band[0] > 250, "the ring itself is missing: {band:?}");
}

/// The tint is the colour, alpha included, so one icon can be faded.
#[test]
fn a_faded_icon_is_faded() {
    let (img, _) = shot(play(), 96.0, 1.0, Color::WHITE.with_alpha(0.5), 3);
    let inside = img.pixel(20 + 40, 20 + 48);
    assert!((120..=135).contains(&inside[0]), "half-faded white on black is not mid-grey: {inside:?}");
}

/// A path in a zoomed canvas is rasterised at the zoomed size, so zooming in
/// keeps the edge crisp instead of enlarging a small raster.
#[test]
fn a_zoomed_canvas_rasterises_at_the_zoomed_size() {
    let square = Path::polygon(24.0, 24.0, &[Vec2::new(6.0, 6.0), Vec2::new(18.0, 6.0), Vec2::new(18.0, 18.0), Vec2::new(6.0, 18.0)]);
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let info = FrameInfo { screen_size: Vec2::new(300.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 };
    let mut out_img = None;
    for _ in 0..3 {
        ui.begin_frame(info);
        let p = square.clone();
        ui.with_transform(Id::new("zoom"), Transform::new(Vec2::new(10.0, 10.0), 4.0), |ui| {
            ui.add_leaf(Id::new("icon"), Layout::leaf(Size::Fixed(24.0), Size::Fixed(24.0)), Vec2::ZERO, false, move |pt, r| {
                pt.fill_path(&p, r, Color::WHITE);
            });
        });
        let out = ui.end_frame();
        out_img = Some(SoftRenderer::new().render_to_image(&out, 300, 300));
    }
    let img = out_img.expect("a frame");
    // Nothing is drawn behind it, so the background is the theme's clear
    // colour: anything that is neither that nor the fill is an edge pixel.
    let bg = img.pixel(299, 299);
    let painted = img.data.chunks_exact(4).filter(|p| p[0] > 250).count();
    let blurred = img.data.chunks_exact(4).filter(|p| p[0] <= 250 && p[0].abs_diff(bg[0]) > 3).count();
    // 12 icon units at 1 px each, zoomed 4x: a 48 px square.
    assert!((2200..=2400).contains(&painted), "a 48 px square painted {painted} pixels");
    assert_eq!(blurred, 0, "the zoomed square is a blurred enlargement: {blurred} soft pixels");
}
