//! [`FontStack`]: a string that mixes scripts draws with whichever registered
//! face has the glyphs, instead of coming out blank.
//!
//! The faces here are stand-ins rather than real fonts: each declares exactly
//! which characters it covers and rasterises a box of its own width, so a test
//! can say *which face drew this glyph* by looking at the quad — which no
//! assertion about real font bytes could do without shipping a CJK font.

use libgui::*;

/// A face that covers one set of characters, rasterises every glyph as a box
/// `boxw` wide, and shapes one glyph per character.
struct Face {
    covers: &'static str,
    /// Advance and bitmap width, in px, so the drawn quad names the face.
    boxw: f32,
}

impl FontRasterizer for Face {
    fn line_metrics(&self, px: f32) -> LineMetrics {
        LineMetrics { ascent: px * 0.8, descent: -px * 0.2 }
    }

    fn covers(&self, ch: char) -> bool {
        self.covers.contains(ch)
    }

    fn shape(&self, text: &str, _px: f32, out: &mut Vec<ShapedGlyph>) {
        for (byte, ch) in text.char_indices() {
            // Every face issues the *same* glyph ids, which is the point:
            // an id means nothing without the face that issued it.
            let glyph = (ch as u32) % 8;
            out.push(ShapedGlyph {
                glyph,
                face: 0,
                cluster: byte as u32,
                advance: self.boxw,
                offset: Vec2::ZERO,
            });
        }
    }

    fn rasterize(&self, _face: u16, _glyph: u32, _px: f32) -> GlyphBitmap {
        let w = self.boxw as u32;
        GlyphBitmap { width: w, height: 4, left: 0.0, bottom: 0.0, coverage: vec![255; (w * 4) as usize] }
    }
}

fn face(covers: &'static str, boxw: f32) -> Box<dyn FontRasterizer> {
    Box::new(Face { covers, boxw })
}

/// The widths of the quads one line of text drew, in order.
fn drawn(ui: &mut Ui, text: &str) -> Vec<f32> {
    ui.begin_frame(FrameInfo::default());
    ui.text_with(text, 20.0, Color::WHITE);
    let out = ui.end_frame();
    out.draw.instances.iter().map(|i| i.rect[2]).collect()
}

fn stack(faces: Vec<Box<dyn FontRasterizer>>) -> Ui {
    Ui::with_rasterizer(Theme::dark(), Box::new(FontStack::new(faces)))
}

/// **The point of the whole thing.** Without a chain, text in a script the UI
/// font has no outlines for is blank. With one, each character goes to the
/// first face that can draw it.
#[test]
fn each_character_goes_to_the_first_face_that_covers_it() {
    // Latin is 10 wide, "CJK" is 30 wide: a quad's width says which drew it.
    let mut ui = stack(vec![face("abc ", 10.0), face("字漢 ", 30.0)]);
    assert_eq!(drawn(&mut ui, "ab字"), vec![10.0, 10.0, 30.0]);
    assert_eq!(drawn(&mut ui, "字a漢"), vec![30.0, 10.0, 30.0]);

    // And the measured width agrees with what was drawn, so layout reserves
    // room for the fallback rather than for a missing glyph.
    let font = ui.font;
    assert_eq!(ui.fonts.measure(font, 20.0, "ab字").x, 50.0);
}

/// The regression the face index exists for. Both faces issue glyph id 1 for
/// their own character; if the atlas keyed on the id alone, the second would
/// find the first's bitmap and draw the wrong script at the wrong width.
#[test]
fn two_faces_may_issue_the_same_glyph_id() {
    // 'a' and '安' both hash to glyph 1 under Face::shape.
    assert_eq!(('a' as u32) % 8, ('安' as u32) % 8);
    let mut ui = stack(vec![face("a", 10.0), face("安", 30.0)]);
    let widths = drawn(&mut ui, "a安");
    assert_eq!(widths, vec![10.0, 30.0], "one glyph id was shared between two faces");
}

/// A character neither face covers still draws: it goes to the primary, which
/// shows whatever it shows for the unknown. Silently dropping it would hide
/// the gap instead of showing it.
#[test]
fn an_uncovered_character_falls_back_to_the_primary() {
    let mut ui = stack(vec![face("a", 10.0), face("字", 30.0)]);
    assert_eq!(drawn(&mut ui, "a\u{10FFFD}"), vec![10.0, 10.0]);
}

/// A run is not split by a character the current face already covers. A space
/// between two CJK words belongs to the CJK run: splitting there would shape
/// each word separately for no reason, and re-splitting on every space is
/// what makes a naive chain slow on ordinary text.
#[test]
fn a_shared_character_stays_in_the_run_it_is_already_in() {
    let mut ui = stack(vec![face("abc ", 10.0), face("字漢 ", 30.0)]);
    // The space after 字 is covered by *both*. It stays with face 1.
    assert_eq!(drawn(&mut ui, "字 漢"), vec![30.0, 30.0, 30.0]);
    // And leading Latin keeps its own space.
    assert_eq!(drawn(&mut ui, "a 字"), vec![10.0, 10.0, 30.0]);
}

/// A combining mark goes wherever its base went, whatever the coverage says.
/// Shaped into a different face it would be positioned against nothing, and
/// an accent would land beside its letter instead of over it.
#[test]
fn a_combining_mark_never_leaves_its_base() {
    // Face 0 has the mark but not the base; face 1 has the base but not the
    // mark. Coverage alone would put them in different runs.
    let mut ui = stack(vec![face("a\u{301}", 10.0), face("字", 30.0)]);
    let widths = drawn(&mut ui, "字\u{301}");
    assert_eq!(widths, vec![30.0, 30.0], "the mark was shaped away from its base");
}

/// The chain does not change how tall a line is. A paragraph of Latin must not
/// grow because a CJK font is registered behind it, or because one emoji
/// appeared in it.
#[test]
fn the_primary_sets_the_line_height() {
    let one = stack(vec![face("a", 10.0)]);
    let many = stack(vec![face("a", 10.0), face("字", 30.0)]);
    let h = |ui: &Ui| ui.fonts.line_height(ui.font, 20.0);
    assert_eq!(h(&one), h(&many));
}

/// A stack of one behaves exactly like the face inside it, so a host can build
/// the chain unconditionally and add fallbacks later.
#[test]
fn a_stack_of_one_is_the_face_itself() {
    const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
    let mut plain = Ui::new(Theme::dark(), FONT).expect("font");
    let mut wrapped =
        Ui::with_rasterizer(Theme::dark(), Box::new(FontStack::from_fonts(&[FONT]).expect("stack")));
    for text in ["Hello", "Inspector", "a b c", "fi\tx"] {
        assert_eq!(drawn(&mut plain, text), drawn(&mut wrapped, text), "{text:?}");
        assert_eq!(
            plain.fonts.measure(plain.font, 14.0, text),
            wrapped.fonts.measure(wrapped.font, 14.0, text),
            "{text:?}"
        );
    }
}

/// Real font bytes: Inter has no CJK outlines, so a chain is the only thing
/// standing between a Japanese label and a row of boxes.
#[test]
fn inter_does_not_cover_cjk_which_is_why_the_chain_exists() {
    const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
    let inter = FontdueRasterizer::from_bytes(FONT).expect("font");
    assert!(inter.covers('a'));
    assert!(inter.covers('é'));
    assert!(!inter.covers('字'), "Inter grew CJK: pick another missing script for this test");
    assert!(!inter.covers('\u{1F600}'), "Inter grew emoji");

    // Behind a face that does have them, the label draws.
    let mut ui = stack(vec![Box::new(inter), face("字\u{1F600}", 30.0)]);
    assert_eq!(drawn(&mut ui, "字\u{1F600}"), vec![30.0, 30.0]);
}
