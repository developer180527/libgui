//! Real text shaping, through rustybuzz (a Rust port of HarfBuzz).
//!
//! The built-in [`FontdueRasterizer`](crate::FontdueRasterizer) maps one
//! character to one glyph and adds pair kerning. That is enough for Latin and
//! wrong for most of the world: Devanagari reorders a vowel sign in front of
//! the consonant it follows in the text, Arabic picks a different form of a
//! letter depending on its neighbours, Thai stacks marks above and below, and
//! a good Latin font has ligatures and mark attachment of its own. All of that
//! is table-driven work inside the font, and [`ShapeRasterizer`] is what runs
//! those tables.
//!
//! `ShapeRasterizer` shapes with rustybuzz and rasterises with fontdue, over
//! the same bytes: both read the font's own glyph ids, so the ids rustybuzz
//! produces are the ones fontdue draws.
//!
//! # Cost
//!
//! Shaping a short label costs about **10.5 µs** here against **0.26 µs** with
//! fontdue — forty times as much, measured over 10,000 strings at 14 px. That
//! is why the feature is off by default, and why it does not matter much when
//! it is on: [`Fonts`](crate::Fonts) caches a shaped run per (font, size,
//! string), so the price is paid once per distinct label and a settled frame
//! shapes nothing at all. A first frame with two hundred fresh labels pays
//! about 2 ms for them.
//!
//! # Right-to-left
//!
//! Letters are **shaped** correctly in RTL scripts — an Arabic letter gets its
//! initial, medial, final or isolated form, and joins its neighbours — but the
//! glyphs are emitted in logical order and so are laid out left to right.
//! Reordering them is bidi, which is a property of the paragraph rather than
//! of the font, and which every caret, hit test and selection rectangle in the
//! library would have to understand. It is not done here, and this module does
//! not pretend otherwise.

use crate::font::{FontRasterizer, GlyphBitmap, LineMetrics, ShapedGlyph};
use crate::Vec2;
use std::cell::RefCell;

self_cell::self_cell!(
    /// rustybuzz borrows the font bytes for the life of the face, so the two
    /// are kept together rather than leaked or re-parsed per call.
    struct OwnedFace {
        owner: Vec<u8>,
        #[covariant]
        dependent: BorrowedFace,
    }
);

type BorrowedFace<'a> = rustybuzz::Face<'a>;

/// A [`FontRasterizer`] that shapes with rustybuzz and rasterises with
/// fontdue. See the docs at the top of this module.
pub struct ShapeRasterizer {
    face: OwnedFace,
    raster: fontdue::Font,
    /// rustybuzz takes its buffer by value and hands it back; keeping it here
    /// means a shaped string does not allocate one. Shaping runs through
    /// `&self` (the measure path), hence the cell.
    buffer: RefCell<Option<rustybuzz::UnicodeBuffer>>,
    /// Font design units per em, for turning rustybuzz's integer positions
    /// into pixels.
    upem: f32,
}

impl ShapeRasterizer {
    /// Parse `bytes` twice, once for each half of the job. Fails rather than
    /// panicking, so a host loading a user-chosen font can fall back.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::FontError> {
        let raster = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| crate::FontError(e.to_string()))?;
        let face = OwnedFace::try_new(bytes.to_vec(), |owned| {
            rustybuzz::Face::from_slice(owned, 0).ok_or_else(|| crate::FontError("rustybuzz cannot read this font".into()))
        })?;
        let upem = face.borrow_dependent().units_per_em() as f32;
        Ok(Self { face, raster, buffer: RefCell::new(Some(rustybuzz::UnicodeBuffer::new())), upem })
    }
}

impl FontRasterizer for ShapeRasterizer {
    fn line_metrics(&self, px: f32) -> LineMetrics {
        match self.raster.horizontal_line_metrics(px) {
            Some(m) => LineMetrics { ascent: m.ascent, descent: m.descent },
            None => LineMetrics { ascent: px * 0.8, descent: -px * 0.2 },
        }
    }

    fn covers(&self, ch: char) -> bool {
        self.raster.lookup_glyph_index(ch) != 0
    }

    fn shape(&self, text: &str, px: f32, out: &mut Vec<ShapedGlyph>) {
        let mut slot = self.buffer.borrow_mut();
        let mut buf = slot.take().unwrap_or_default();
        buf.clear();
        buf.push_str(text);
        // Script, language and direction from the text itself. A caller that
        // knows better — a UI that knows its own locale — would pass them in;
        // libgui does not know the locale and will not guess one.
        buf.guess_segment_properties();
        let rtl = buf.direction() == rustybuzz::Direction::RightToLeft;

        let face = self.face.borrow_dependent();
        let glyphs = rustybuzz::shape(face, &[], buf);
        let k = px / self.upem;

        let (infos, pos) = (glyphs.glyph_infos(), glyphs.glyph_positions());
        // rustybuzz emits an RTL run in visual order, which would leave
        // clusters running backwards. Everything downstream — caret positions,
        // hit testing, the width of a byte range — reads clusters as
        // non-decreasing, so the run is put back into logical order. That is
        // why RTL lays out left to right; see the module docs.
        let n = infos.len();
        out.reserve(n);
        for i in 0..n {
            let i = if rtl { n - 1 - i } else { i };
            let (info, p) = (&infos[i], &pos[i]);
            out.push(ShapedGlyph {
                glyph: info.glyph_id,
                face: 0,
                cluster: info.cluster,
                advance: p.x_advance as f32 * k,
                // rustybuzz's y is up, libgui's offset is y-down.
                offset: Vec2::new(p.x_offset as f32 * k, -(p.y_offset as f32) * k),
            });
        }

        *slot = Some(glyphs.clear());
    }

    fn rasterize(&self, _face: u16, glyph: u32, px: f32) -> GlyphBitmap {
        let (m, coverage) = self.raster.rasterize_indexed(glyph as u16, px);
        GlyphBitmap {
            width: m.width as u32,
            height: m.height as u32,
            left: m.xmin as f32,
            bottom: m.ymin as f32,
            coverage,
        }
    }
}
