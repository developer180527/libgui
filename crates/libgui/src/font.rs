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
    /// Font-specific glyph id, passed back to [`FontRasterizer::rasterize`].
    pub glyph: u32,
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

    /// Rasterise one glyph. Called once per (glyph, px); the result is cached
    /// in libgui's atlas. Return a zero-sized bitmap for blank glyphs (space).
    fn rasterize(&self, glyph: u32, px: f32) -> GlyphBitmap;
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
}

#[cfg(feature = "fontdue")]
impl FontRasterizer for FontdueRasterizer {
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
                cluster: byte as u32,
                advance: self.font.metrics_indexed(glyph, px).advance_width,
                offset: Vec2::ZERO,
            });
            prev = Some(glyph);
        }
    }

    fn rasterize(&self, glyph: u32, px: f32) -> GlyphBitmap {
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
                    out.push(ShapedGlyph { glyph: LIGATURE_FI, cluster, advance: px * 0.75, offset: Vec2::ZERO });
                } else if ch == '\u{301}' {
                    out.push(ShapedGlyph { glyph: ch as u32, cluster, advance: 0.0, offset: Vec2::new(-px * 0.4, -px * 0.3) });
                } else {
                    out.push(ShapedGlyph { glyph: ch as u32, cluster, advance: px * 0.5, offset: Vec2::ZERO });
                }
            }
        }

        fn rasterize(&self, glyph: u32, px: f32) -> GlyphBitmap {
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
