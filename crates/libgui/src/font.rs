//! Pluggable font backend.
//!
//! libgui asks a [`FontRasterizer`] three things, and keeps everything else
//! itself (measurement caching, DPI/zoom fitting, caret positions, the glyph
//! atlas, pixel snapping):
//!
//! 1. [`FontRasterizer::line_metrics`]: ascent and descent at a pixel size;
//! 2. [`FontRasterizer::shape`]: a string to positioned *glyph ids*;
//! 3. [`FontRasterizer::rasterize`]: one glyph id to a coverage bitmap.
//!
//! [`FontStack`] composes several of these into a fallback chain, so a string
//! mixing scripts draws with whichever face has the glyphs.
//!
//! Shaping is by glyph id, not by character, so a real shaper (HarfBuzz,
//! CoreText, DirectWrite, rustybuzz) can produce ligatures, contextual forms
//! and combining marks. [`FontdueRasterizer`] (feature `fontdue`, on by
//! default) is the built-in one: one glyph per character plus kerning.
//! An engine that already ships FreeType/HarfBuzz implements this trait and
//! registers it with [`crate::Fonts::add_rasterizer`] or
//! [`crate::Ui::with_rasterizer`].

use crate::Vec2;

/// Vertical metrics at a pixel size, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetrics {
    /// Baseline to the top of the line (positive).
    pub ascent: f32,
    /// Baseline to the bottom of the line (zero or negative).
    pub descent: f32,
}

/// One glyph of shaped text, in logical order (left to right).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Face-specific glyph id, passed back to [`FontRasterizer::rasterize`].
    pub glyph: u32,
    /// Which face inside this rasterizer `glyph` belongs to, passed back to
    /// [`FontRasterizer::rasterize`] beside it. A single-face backend leaves
    /// it 0 and never reads it; a fallback chain ([`FontStack`]) uses it to
    /// say which of its faces owns the glyph, because a glyph id means
    /// nothing without the face it came from.
    pub face: u16,
    /// Byte offset in the source text of the first character this glyph
    /// represents. Non-decreasing; a ligature spans several characters.
    pub cluster: u32,
    /// Pen advance to the next glyph in pixels, including kerning.
    pub advance: f32,
    /// Offset of this glyph from the pen, in pixels (+y down), e.g. marks.
    pub offset: Vec2,
}

/// A rasterised glyph: an 8-bit coverage bitmap and where it sits relative to
/// the pen on the baseline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    /// Pen x to the bitmap's left edge, in pixels.
    pub left: f32,
    /// Baseline to the bitmap's *bottom* edge, in pixels, +y up (so a
    /// descender's bottom is negative).
    pub bottom: f32,
    /// `width * height` coverage values, row-major, top row first.
    pub coverage: Vec<u8>,
}

/// A font face that can shape and rasterise. All sizes are in pixels at the
/// size libgui asks for (already multiplied by DPI scale and canvas zoom).
pub trait FontRasterizer {
    fn line_metrics(&self, px: f32) -> LineMetrics;

    /// Append the glyphs for `text` to `out` (which the caller has cleared).
    /// Called often: for every measurement miss, caret query and draw.
    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>);

    /// Rasterise one glyph of one face. Called once per (face, glyph, px);
    /// the result is cached in libgui's atlas. Return a zero-sized bitmap for
    /// blank glyphs (space).
    fn rasterize(&self, face: u16, glyph: u32, px: f32) -> GlyphBitmap;

    /// Can this backend draw `ch` with something other than `.notdef`?
    ///
    /// Only [`FontStack`] asks, to decide where a run of text should go. The
    /// default says yes to everything, which is right for a single face: it
    /// is the last resort, and a box is better than nothing.
    fn covers(&self, ch: char) -> bool {
        let _ = ch;
        true
    }
}

/// The built-in backend: fontdue. No complex shaping (one glyph per
/// character, pair kerning), but pure Rust, small and fast.
#[cfg(feature = "fontdue")]
pub struct FontdueRasterizer {
    font: fontdue::Font,
}

#[cfg(feature = "fontdue")]
impl FontdueRasterizer {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::FontError> {
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map(|font| Self { font })
            .map_err(|e| crate::FontError(e.to_string()))
    }

    /// fontdue reports glyph 0 (`.notdef`) for a character the font has no
    /// outline for, which is exactly the question `covers` asks.
    fn has(&self, ch: char) -> bool {
        self.font.lookup_glyph_index(ch) != 0
    }
}

#[cfg(feature = "fontdue")]
impl FontRasterizer for FontdueRasterizer {
    fn covers(&self, ch: char) -> bool {
        self.has(ch)
    }

    fn line_metrics(&self, px: f32) -> LineMetrics {
        match self.font.horizontal_line_metrics(px) {
            Some(m) => LineMetrics { ascent: m.ascent, descent: m.descent },
            None => LineMetrics { ascent: px * 0.8, descent: -px * 0.2 },
        }
    }

    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
        let mut prev: Option<u16> = None;
        for (byte, ch) in text.char_indices() {
            let glyph = self.font.lookup_glyph_index(ch);
            // Kerning adjusts the gap *before* this glyph: fold it into the
            // previous glyph's advance.
            if let (Some(p), Some(last)) = (prev, out.last_mut()) {
                last.advance += self.font.horizontal_kern_indexed(p, glyph, px).unwrap_or(0.0);
            }
            out.push(ShapedGlyph {
                glyph: glyph as u32,
                face: 0,
                cluster: byte as u32,
                advance: self.font.metrics_indexed(glyph, px).advance_width,
                offset: Vec2::ZERO,
            });
            prev = Some(glyph);
        }
    }

    fn rasterize(&self, _face: u16, glyph: u32, px: f32) -> GlyphBitmap {
        let (m, coverage) = self.font.rasterize_indexed(glyph as u16, px);
        GlyphBitmap {
            width: m.width as u32,
            height: m.height as u32,
            left: m.xmin as f32,
            bottom: m.ymin as f32,
            coverage,
        }
    }
}

/// A fallback chain: several faces, tried in order, so a string that mixes
/// scripts draws with whichever face has the glyphs.
///
/// The bundled UI font has no CJK, Arabic, Indic or emoji outlines. Without a
/// chain those characters draw as `.notdef` boxes — or, in a font whose
/// `.notdef` is blank, as nothing at all, which is the failure that makes a
/// general-purpose toolkit unusable outside Latin. Registering the chain is
/// the host's job, because only the host knows which system fonts it may ship
/// or load:
///
/// ```no_run
/// # use libgui::{FontStack, Fonts};
/// # fn f(ui_font: &[u8], cjk: &[u8], emoji: &[u8]) -> Result<(), libgui::FontError> {
/// let stack = FontStack::from_fonts(&[ui_font, cjk, emoji])?;
/// let mut fonts = Fonts::new();
/// let id = fonts.add_rasterizer(Box::new(stack));
/// # Ok(()) }
/// ```
///
/// **Line metrics come from the first face**, not from whichever face a
/// particular glyph landed in. A line of Latin text must not change height
/// because a CJK font is registered behind it, and a paragraph must not grow
/// taller the moment one emoji appears in it. A fallback glyph taller than the
/// primary's ascent therefore overhangs its line rather than pushing it open.
pub struct FontStack {
    faces: Vec<Box<dyn FontRasterizer>>,
    /// Scratch for shaping one run, since the sub-face appends face-0 glyphs
    /// that have to be rewritten before they join the output.
    run: std::cell::RefCell<Vec<ShapedGlyph>>,
}

impl FontStack {
    /// A chain over faces you have built yourself. The first is the primary
    /// and the last resort: a character no face covers is shaped by it, so it
    /// draws whatever that font shows for the unknown.
    ///
    /// At most 65,536 faces, because a face index is a `u16` in
    /// [`ShapedGlyph`]; extra ones are dropped.
    ///
    /// # Panics
    ///
    /// If `faces` is empty. A chain with nothing in it has no primary, so it
    /// could not answer for line metrics or for an uncovered character, and
    /// every one of those questions would fail later and further away.
    pub fn new(faces: Vec<Box<dyn FontRasterizer>>) -> Self {
        assert!(!faces.is_empty(), "a FontStack needs at least one face: the first is the primary");
        let mut faces = faces;
        faces.truncate(u16::MAX as usize + 1);
        Self { faces, run: std::cell::RefCell::new(Vec::new()) }
    }

    /// A chain from font files, each read with the built-in fontdue backend.
    /// Fails on the first one that will not parse, naming its position, so a
    /// host loading user-chosen fallbacks can say which file was bad.
    #[cfg(feature = "fontdue")]
    pub fn from_fonts(fonts: &[&[u8]]) -> Result<Self, crate::FontError> {
        let mut faces: Vec<Box<dyn FontRasterizer>> = Vec::with_capacity(fonts.len());
        for (i, bytes) in fonts.iter().enumerate() {
            let face = FontdueRasterizer::from_bytes(bytes)
                .map_err(|e| crate::FontError(format!("fallback font {i}: {}", e.0)))?;
            faces.push(Box::new(face));
        }
        if faces.is_empty() {
            return Err(crate::FontError("a font stack needs at least one face".into()));
        }
        Ok(Self::new(faces))
    }

    pub fn faces(&self) -> usize {
        self.faces.len()
    }

    /// Which face should draw `ch`: the first that covers it, else the
    /// primary, which then draws its own `.notdef`.
    fn face_for(&self, ch: char) -> u16 {
        self.faces.iter().position(|f| f.covers(ch)).unwrap_or(0) as u16
    }
}

/// Does `ch` have to stay with the character before it, whatever the coverage
/// says?
///
/// A combining mark positions itself against the base it follows, and a
/// zero-width joiner exists only to bind its neighbours: shaped apart from
/// what they attach to, an accent lands on nothing and an emoji sequence
/// becomes its unjoined pieces. So these never start a new run — they go
/// wherever their base went, and if that face lacks them the shaper there
/// drops or boxes them, which is still better than detaching them.
///
/// This is a range check, not a Unicode general-category lookup: libgui has no
/// Unicode tables and will not grow one for this. It covers the combining
/// blocks, the joiners and the variation selectors, which is the set that
/// actually breaks. A backend with real Unicode data can do better by
/// implementing [`FontRasterizer`] itself.
fn joins_previous(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036F   // combining diacritical marks
        | 0x0483..=0x0489 // Cyrillic combining
        | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7 // Hebrew points
        | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 // Arabic marks
        | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0951..=0x0957 // Devanagari matras/signs
        | 0x1AB0..=0x1AFF // combining diacritical marks extended
        | 0x1DC0..=0x1DFF // combining diacritical marks supplement
        | 0x200C..=0x200D // ZWNJ, ZWJ
        | 0x20D0..=0x20F0 // combining diacritical marks for symbols
        | 0xFE00..=0xFE0F // variation selectors
        | 0xFE20..=0xFE2F // combining half marks
        | 0xE0100..=0xE01EF // variation selectors supplement
    )
}

impl FontRasterizer for FontStack {
    /// The primary's, always. See the note on [`FontStack`].
    fn line_metrics(&self, px: f32) -> LineMetrics {
        self.faces[0].line_metrics(px)
    }

    fn covers(&self, ch: char) -> bool {
        self.faces.iter().any(|f| f.covers(ch))
    }

    /// Split `text` into maximal runs that one face can draw, shape each with
    /// that face, and concatenate.
    ///
    /// A run is shaped as its own string, so no ligature or contextual form
    /// crosses a face boundary — which is what should happen: a script change
    /// is a shaping boundary anyway. A character the *current* face already
    /// covers stays in the current run even if an earlier face covers it too,
    /// so a space or a digit between two CJK words does not split the run in
    /// three.
    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
        let mut run = self.run.borrow_mut();
        let mut start = 0usize;
        let mut face: Option<u16> = None;

        // `flush` is written out at each site rather than closed over: it
        // needs `&self`, `out` and `run` at once.
        for (byte, ch) in text.char_indices() {
            let want = match face {
                // Already in a run: stay in it if this face can draw the
                // character, or if the character must not be detached.
                Some(f) if joins_previous(ch) || self.faces[f as usize].covers(ch) => f,
                _ => self.face_for(ch),
            };
            if face == Some(want) {
                continue;
            }
            if let Some(f) = face {
                shape_run(&*self.faces[f as usize], &text[start..byte], px, start, f, &mut run, out);
            }
            (start, face) = (byte, Some(want));
        }
        if let Some(f) = face {
            shape_run(&*self.faces[f as usize], &text[start..], px, start, f, &mut run, out);
        }
    }

    fn rasterize(&self, face: u16, glyph: u32, px: f32) -> GlyphBitmap {
        // The sub-face is a single-face backend and issued this id as its own
        // face 0, so that is what it is asked for.
        match self.faces.get(face as usize) {
            Some(f) => f.rasterize(0, glyph, px),
            None => GlyphBitmap::default(),
        }
    }
}

/// Shape one same-face run and append it, moving the sub-face's byte offsets
/// back into the whole string's coordinates and stamping the face on each
/// glyph.
fn shape_run(
    face: &dyn FontRasterizer,
    slice: &str,
    px: f32,
    at: usize,
    index: u16,
    scratch: &mut Vec<ShapedGlyph>,
    out: &mut Vec<ShapedGlyph>,
) {
    scratch.clear();
    face.shape(slice, px, scratch);
    out.extend(scratch.iter().map(|g| ShapedGlyph {
        face: index,
        cluster: g.cluster + at as u32,
        ..*g
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, FrameInfo, Theme, Ui};

    /// A stand-in for a real shaper: every glyph is a box `px/2` wide, "fi"
    /// becomes one ligature glyph, and U+0301 (combining acute) becomes a
    /// zero-advance mark drawn above the previous glyph.
    struct FakeShaper;

    const LIGATURE_FI: u32 = 0xFB01;

    impl FontRasterizer for FakeShaper {
        fn line_metrics(&self, px: f32) -> LineMetrics {
            LineMetrics { ascent: px * 0.8, descent: -px * 0.2 }
        }

        fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
            let mut it = text.char_indices().peekable();
            while let Some((byte, ch)) = it.next() {
                let cluster = byte as u32;
                if ch == 'f' && it.peek().map(|&(_, c)| c) == Some('i') {
                    it.next();
                    out.push(ShapedGlyph { glyph: LIGATURE_FI, face: 0, cluster, advance: px * 0.75, offset: Vec2::ZERO });
                } else if ch == '\u{301}' {
                    out.push(ShapedGlyph { glyph: ch as u32, face: 0, cluster, advance: 0.0, offset: Vec2::new(-px * 0.4, -px * 0.3) });
                } else {
                    out.push(ShapedGlyph { glyph: ch as u32, face: 0, cluster, advance: px * 0.5, offset: Vec2::ZERO });
                }
            }
        }

        fn rasterize(&self, _face: u16, glyph: u32, px: f32) -> GlyphBitmap {
            if glyph == ' ' as u32 {
                return GlyphBitmap::default();
            }
            let (w, h) = ((px * 0.4) as u32, (px * 0.6) as u32);
            GlyphBitmap { width: w, height: h, left: 1.0, bottom: 0.0, coverage: vec![255; (w * h) as usize] }
        }
    }

    #[test]
    fn a_custom_rasterizer_drives_measure_carets_and_drawing() {
        let mut ui = Ui::with_rasterizer(Theme::dark(), Box::new(FakeShaper));
        let (font, px) = (ui.font, 20.0);

        // "fix": ligature (15) + x (10). Measurement is the shaper's advances.
        assert_eq!(ui.fonts.measure(font, px, "fix").x, 25.0);
        assert_eq!(ui.fonts.measure(font, px, "abc").x, 30.0);

        // Carets: f | i share the ligature's end; then x.
        assert_eq!(ui.fonts.carets(font, px, "fix"), vec![0.0, 15.0, 15.0, 25.0]);
        // A combining mark adds a character but no width.
        let e = "e\u{301}x";
        assert_eq!(ui.fonts.carets(font, px, e), vec![0.0, 10.0, 10.0, 20.0]);

        // Drawing: one quad per visible glyph (the ligature is one), marks at
        // their offset, and nothing for a blank glyph.
        ui.begin_frame(FrameInfo::default());
        ui.text_with("fix e\u{301}", px, Color::WHITE);
        let out = ui.end_frame();
        let quads: Vec<[f32; 4]> = out.draw.instances.iter().map(|i| i.rect).collect();
        assert_eq!(quads.len(), 4, "fi, x, e, mark (space is blank): {quads:?}");
        let (e_quad, mark) = (quads[2], quads[3]);
        assert!(mark[1] < e_quad[1], "the mark sits above its base");
        assert!(mark[0] < e_quad[0] + 10.0, "and over it, not after it");
    }
}
