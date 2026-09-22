//! [`ShapeRasterizer`]: what a real shaper does that a character-to-glyph map
//! cannot.
//!
//! Every assertion here is a difference against the built-in
//! [`FontdueRasterizer`] over the *same font bytes*, so nothing turns on which
//! font is bundled — only on the fact that one backend reads the font's
//! layout tables and the other does not.

#![cfg(feature = "shape")]

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const PX: f32 = 20.0;

fn shaped(r: &dyn FontRasterizer, text: &str) -> Vec<ShapedGlyph> {
    let mut out = Vec::new();
    r.shape(text, PX, &mut out);
    out
}

fn both(text: &str) -> (Vec<ShapedGlyph>, Vec<ShapedGlyph>) {
    let sh = ShapeRasterizer::from_bytes(FONT).expect("shaper");
    let fd = FontdueRasterizer::from_bytes(FONT).expect("fontdue");
    (shaped(&sh, text), shaped(&fd, text))
}

fn width(g: &[ShapedGlyph]) -> f32 {
    g.iter().map(|g| g.advance).sum()
}

/// **Kerning that is actually in the font.** Inter puts its kerning in GPOS,
/// which is a layout table; fontdue reads only the legacy `kern` table, so it
/// finds none and sets "AV" as if the two letters had nothing to do with each
/// other. Every heading in a UI is slightly wrong because of it.
#[test]
fn gpos_kerning_is_applied_where_fontdue_finds_none() {
    let (sh, fd) = both("AV");
    assert_eq!(fd[0].advance, fd[1].advance, "fontdue grew a kern table; pick another pair");
    assert!(sh[0].advance < fd[0].advance, "AV was not kerned: {} vs {}", sh[0].advance, fd[0].advance);
    assert!(width(&sh) < width(&fd));
}

/// **A combining mark is composed with its base.** "e" + U+0301 is one glyph,
/// the font's own é, rather than an unattached acute floating at the origin of
/// the next character — which is what a character-to-glyph map produces, since
/// it has no way to know the two belong together.
#[test]
fn a_base_and_its_mark_become_one_glyph() {
    let (sh, fd) = both("e\u{301}");
    assert_eq!(sh.len(), 1, "the mark was not composed: {sh:?}");
    assert_eq!(fd.len(), 2, "fontdue composed it after all; this test is moot");
    // fontdue's second glyph has no advance *and* no offset: it is drawn at
    // the pen, on top of nothing.
    assert_eq!((fd[1].advance, fd[1].offset), (0.0, Vec2::ZERO));

    // The composed glyph spans both characters, so a caret between them lands
    // at the end of the pair rather than inside it.
    assert_eq!(sh[0].cluster, 0);
    let ui = Ui::with_rasterizer(Theme::dark(), Box::new(ShapeRasterizer::from_bytes(FONT).unwrap()));
    let (f, s) = (ui.font, 20.0);
    let carets = ui.fonts.carets(f, s, "e\u{301}x");
    assert_eq!(carets[1], carets[2], "the caret stopped inside a single glyph");
}

/// **Contextual substitution.** The first `f` of "ffi" is set narrower than a
/// standalone `f`: the font has a rule for the sequence, and running it is the
/// whole job of a shaper.
#[test]
fn a_letter_changes_shape_because_of_its_neighbours() {
    let (sh, fd) = both("ffi");
    assert!(sh[0].advance < sh[1].advance, "the leading f was not narrowed: {sh:?}");
    assert_eq!(fd[0].advance, fd[1].advance, "fontdue set both f's identically, as expected");
}

/// Clusters stay non-decreasing and land on character boundaries, whatever the
/// shaper did. Everything downstream — [`Fonts::carets`], hit testing, the
/// width of a byte range — reads them that way.
#[test]
fn clusters_are_ordered_byte_offsets_into_the_source() {
    for text in ["Hello, world", "e\u{301}ffi", "a\tb", "→ ✓ ×", ""] {
        let (sh, _) = both(text);
        let mut last = 0u32;
        for g in &sh {
            assert!(g.cluster >= last, "{text:?} went backwards: {sh:?}");
            assert!(text.is_char_boundary(g.cluster as usize), "{text:?} cluster {} is mid-character", g.cluster);
            last = g.cluster;
        }
    }
}

/// An RTL run is put back into logical order before it leaves the shaper.
/// rustybuzz emits it visually, right-hand glyph first, which would leave
/// clusters running backwards and break every caret in the library.
///
/// This is *not* bidi: the glyphs are laid out left to right. See the module
/// docs on `ShapeRasterizer`. The test pins the ordering so that when bidi
/// does land, it lands deliberately.
#[test]
fn a_right_to_left_run_comes_back_in_logical_order() {
    let (sh, _) = both("שלום");
    assert_eq!(sh.len(), 4);
    let clusters: Vec<u32> = sh.iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 2, 4, 6], "an RTL run leaked out in visual order");
}

/// The shaper drives the whole pipeline, not just the shape call: measurement,
/// drawing and the atlas all read its output.
#[test]
fn the_shaper_drives_measurement_and_drawing() {
    let mut sh = Ui::with_rasterizer(Theme::dark(), Box::new(ShapeRasterizer::from_bytes(FONT).unwrap()));
    let mut fd = Ui::new(Theme::dark(), FONT).expect("font");

    let w = |ui: &Ui| ui.fonts.measure(ui.font, 40.0, "AVATAR").x;
    assert!(w(&sh) < w(&fd), "the label measured the same either way: {} vs {}", w(&sh), w(&fd));

    let quads = |ui: &mut Ui, text: &str| {
        ui.begin_frame(FrameInfo::default());
        ui.text_with(text, 40.0, Color::WHITE);
        ui.end_frame().draw.instances.len()
    };
    // One quad per *glyph*, so the composed é is one where fontdue draws two.
    assert_eq!(quads(&mut sh, "e\u{301}"), 1);
    assert_eq!(quads(&mut fd, "e\u{301}"), 2);
}

/// Shaping is cached per (font, size, string) like any other backend's, so a
/// steady frame does not pay for it. rustybuzz costs more per call than
/// fontdue, which makes the cache matter more, not less.
#[test]
fn a_steady_frame_does_not_reshape() {
    let mut ui = Ui::with_rasterizer(Theme::dark(), Box::new(ShapeRasterizer::from_bytes(FONT).unwrap()));
    for _ in 0..3 {
        ui.begin_frame(FrameInfo::default());
        ui.label("Inspector");
        ui.label("Hierarchy");
        let _ = ui.end_frame();
    }
    assert_eq!(ui.frame_cost().text_shaped, 0, "a settled frame re-shaped its labels");
}

/// It composes with the fallback chain: a stack can hold shaping faces, and
/// the face index still names which one drew each glyph.
#[test]
fn a_shaping_face_works_inside_a_fallback_chain() {
    let faces: Vec<Box<dyn FontRasterizer>> = vec![
        Box::new(ShapeRasterizer::from_bytes(FONT).unwrap()),
        Box::new(ShapeRasterizer::from_bytes(FONT).unwrap()),
    ];
    let stack = FontStack::new(faces);
    let g = shaped(&stack, "AV");
    assert!(g.iter().all(|g| g.face == 0), "Inter covers both, so nothing should fall back");
    // The kerning survived the trip through the chain.
    assert!(g[0].advance < g[1].advance);
}
