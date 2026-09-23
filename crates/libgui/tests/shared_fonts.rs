//! One atlas for every window.
//!
//! A docked application gives each window its own `Ui`, and each used to carry
//! its own font system: a panel torn into a new window rasterised every glyph
//! again, and the host uploaded a second copy of the same image. For a CJK
//! interface that is thousands of glyphs and megabytes of texture per window.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn info() -> FrameInfo {
    FrameInfo { screen_size: Vec2::new(400.0, 200.0), scale: 2.0, dt: 1.0 / 60.0 }
}

/// Draw a line of text and report what it cost.
fn frame(ui: &mut Ui, text: &str) -> libgui::testing::FrameCost {
    ui.audit = true;
    ui.begin_frame(info());
    ui.label(text);
    drop(ui.end_frame());
    ui.frame_cost()
}

#[test]
fn a_second_window_draws_from_the_first_windows_atlas() {
    const TEXT: &str = "Geometry Spreadsheet";

    let mut main = Ui::new(Theme::dark(), FONT).expect("font");
    let first = frame(&mut main, TEXT);
    assert!(first.glyphs_rasterized > 0, "the first window rasterised nothing at all");

    // A torn-off panel, sharing the font system.
    let mut torn = Ui::sharing_fonts(Theme::dark(), &main);
    assert!(torn.fonts.is_shared_with(&main.fonts), "sharing_fonts did not share");
    let shared = frame(&mut torn, TEXT);
    assert_eq!(
        shared.glyphs_rasterized, 0,
        "a shared window rasterised {} glyphs that were already in the atlas",
        shared.glyphs_rasterized
    );

    // What it used to do, and still does for a window given its own fonts.
    let mut separate = Ui::new(Theme::dark(), FONT).expect("font");
    let alone = frame(&mut separate, TEXT);
    assert!(
        alone.glyphs_rasterized > 0,
        "an unshared window should still rasterise its own glyphs"
    );

    // And it is one image, not two that happen to match: same version, and
    // the same allocation behind both.
    let a = main.fonts.atlas();
    let b = torn.fonts.atlas();
    assert_eq!(a.version, b.version, "the two windows disagree about the atlas");
    assert_eq!(a.data.as_ptr(), b.data.as_ptr(), "the atlas was copied rather than shared");
    assert_ne!(
        a.data.as_ptr(),
        separate.fonts.atlas().data.as_ptr(),
        "an unshared window must have its own atlas"
    );
}

/// A glyph the first window has never drawn is rasterised once, by whichever
/// window needs it, and is then there for both.
#[test]
fn a_glyph_is_rasterised_once_whichever_window_asks_first() {
    let mut main = Ui::new(Theme::dark(), FONT).expect("font");
    frame(&mut main, "shared");
    let mut torn = Ui::sharing_fonts(Theme::dark(), &main);

    // Text only the second window shows.
    let new_to_both = frame(&mut torn, "Wireframe");
    assert!(new_to_both.glyphs_rasterized > 0, "the new glyphs came from nowhere");

    // Now the first window shows it, and pays nothing.
    let already = frame(&mut main, "Wireframe");
    assert_eq!(already.glyphs_rasterized, 0, "the first window rasterised the second's glyphs again");
}

/// Windows on displays of different scales share the atlas without fighting
/// over it: a glyph is keyed by the size it is rasterised at, so both sizes
/// live in the same image.
#[test]
fn two_windows_at_different_scales_share_one_atlas() {
    let mut a = Ui::new(Theme::dark(), FONT).expect("font");
    let mut b = Ui::sharing_fonts(Theme::dark(), &a);

    let at = |ui: &mut Ui, scale: f32| {
        ui.audit = true;
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 200.0), scale, dt: 1.0 / 60.0 });
        ui.label("Retina");
        drop(ui.end_frame());
        ui.frame_cost()
    };

    at(&mut a, 2.0);
    // A different scale means different physical sizes, so these are new.
    assert!(at(&mut b, 1.0).glyphs_rasterized > 0, "the 1x glyphs should not already exist");
    // But each window keeps its own answer, and neither undoes the other.
    assert_eq!(at(&mut a, 2.0).glyphs_rasterized, 0, "the 2x glyphs were evicted by the 1x window");
    assert_eq!(at(&mut b, 1.0).glyphs_rasterized, 0, "the 1x glyphs were evicted by the 2x window");
    assert!(a.fonts.is_shared_with(&b.fonts));
}
