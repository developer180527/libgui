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

/// **Locale is the app's, and only the app has it.** A font's `locl` feature
/// substitutes different letterforms for the same codepoints depending on the
/// language, and no amount of looking at the characters reveals which language
/// they are. The shaper takes the tag rather than guessing one.
///
/// What this pins is that the tag is validated and kept — and no more than
/// that. Inter has no `locl` rules, so the substitution itself is not
/// observable here: **deleting the `set_language` call in the shaper does not
/// fail this test**, and no assertion over this font could make it. Verified
/// by trying it. The direction and script tests below do not have that
/// problem, because both change a glyph id.
///
/// Said plainly rather than papered over: a test that asserted "the glyphs
/// come out the same either way" would pass just as happily with the language
/// thrown away, and would read like coverage while being none. Closing this
/// needs a font with `locl` rules in the test assets.
#[test]
fn a_language_tag_is_validated_and_kept() {
    let turkish = ShapeRasterizer::from_bytes(FONT).expect("shaper").with_language("tr").expect("tr");
    assert_eq!(turkish.language(), Some("tr"));
    // Normalised to lower case, the way a tag is compared.
    let mixed = ShapeRasterizer::from_bytes(FONT).unwrap().with_language("zh-Hant").unwrap();
    assert_eq!(mixed.language(), Some("zh-hant"));
    // Unset unless the app sets it: nothing here reads a process locale.
    assert_eq!(ShapeRasterizer::from_bytes(FONT).unwrap().language(), None);
    // A tag that is not one is refused rather than silently ignored.
    assert!(ShapeRasterizer::from_bytes(FONT).unwrap().with_language("").is_err());
}

/// **Direction reaches the shaper**, provably: an RTL run mirrors its brackets,
/// so `(` is shaped as `)`. That is a different glyph id, not a different
/// position, so no amount of reordering downstream could produce it — it can
/// only come from the shaper having been told the direction.
#[test]
fn a_forced_direction_mirrors_brackets() {
    let ltr = ShapeRasterizer::from_bytes(FONT).expect("shaper");
    let rtl = ShapeRasterizer::from_bytes(FONT)
        .expect("shaper")
        .with_direction(TextDirection::RightToLeft);

    let open_ltr = shaped(&ltr, "(")[0].glyph;
    let open_rtl = shaped(&rtl, "(")[0].glyph;
    assert_ne!(open_ltr, open_rtl, "an RTL run did not mirror its bracket: the direction never arrived");
    // And it is the *closing* bracket it became, not some third thing.
    assert_eq!(open_rtl, shaped(&ltr, ")")[0].glyph);

    // Clusters still ascend: the run is put back into logical order.
    let clusters: Vec<u32> = shaped(&rtl, "(a)").iter().map(|g| g.cluster).collect();
    assert_eq!(clusters, vec![0, 1, 2], "a forced-RTL run did not come back in logical order");
}

/// **Script reaches the shaper too.** Arabic implies a right-to-left run, so
/// forcing the script mirrors brackets the same way — observable proof the tag
/// was applied rather than dropped.
#[test]
fn a_forced_script_changes_how_a_run_is_shaped() {
    let latn = ShapeRasterizer::from_bytes(FONT).expect("shaper").with_script("Latn").expect("Latn");
    let arab = ShapeRasterizer::from_bytes(FONT).expect("shaper").with_script("Arab").expect("Arab");
    assert_ne!(
        shaped(&latn, "(")[0].glyph,
        shaped(&arab, "(")[0].glyph,
        "forcing the script had no effect: the tag never reached the shaper"
    );

    for bad in ["Lat", "Latin", "", "Latnx"] {
        assert!(ShapeRasterizer::from_bytes(FONT).unwrap().with_script(bad).is_err(), "script tag {bad:?} was accepted");
    }
}

/// The locale survives the buffer being reused. rustybuzz's `clear` resets
/// script, language and direction, so they have to be applied on every call —
/// a shaper that set them once in its constructor would work for exactly one
/// string and then quietly stop.
#[test]
fn the_locale_survives_the_reused_buffer() {
    let rtl = ShapeRasterizer::from_bytes(FONT)
        .expect("shaper")
        .with_direction(TextDirection::RightToLeft);
    let mirrored = shaped(&rtl, "(")[0].glyph;
    for i in 0..4 {
        assert_eq!(shaped(&rtl, "(")[0].glyph, mirrored, "the direction was lost on call {}", i + 1);
        // Shape something else in between, so the buffer really is recycled.
        let _ = shaped(&rtl, "Hello");
    }
}
