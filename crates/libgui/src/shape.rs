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
//! # Locale
//!
//! Shaping depends on more than the characters. A font's `locl` feature
//! substitutes different letterforms per language over the *same* codepoints:
//! Turkish wants the dotless i treated as its own letter, and Serbian Cyrillic
//! italics differ from Russian ones. Only the app knows which it is, so
//! [`ShapeRasterizer::with_language`] takes it and nothing here guesses it.
//!
//! Script and direction *are* derived from the text when they are not given,
//! which is a content-level inference rather than a locale one — it reads only
//! the characters in the string, never the process environment. Override
//! either with [`ShapeRasterizer::with_script`] and
//! [`ShapeRasterizer::with_direction`] when you know better than the text
//! does.
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

/// Which way a run of text runs. See [`ShapeRasterizer::with_direction`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

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
    /// What the app knows about the text that the text itself does not say.
    /// `None` leaves it to `guess_segment_properties`, which reads only the
    /// characters.
    language: Option<rustybuzz::Language>,
    script: Option<rustybuzz::Script>,
    direction: Option<rustybuzz::Direction>,
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
        Ok(Self {
            face,
            raster,
            buffer: RefCell::new(Some(rustybuzz::UnicodeBuffer::new())),
            upem,
            language: None,
            script: None,
            direction: None,
        })
    }

    /// The language this text is in, as a BCP-47 tag: `"tr"`, `"sr"`,
    /// `"zh-Hant"`.
    ///
    /// Nothing else can supply this. A font's `locl` feature picks different
    /// letterforms for the same codepoints depending on the language — the
    /// dotted and dotless i in Turkish, Serbian Cyrillic italics against
    /// Russian ones — and no amount of looking at the characters reveals which
    /// language they are. Left unset, the font's default forms are used, which
    /// is right for most text and quietly wrong for those.
    ///
    /// libgui does not read the process locale to fill this in. Reading the
    /// environment is the host's job, and a UI that has to render one document
    /// in Turkish and another in English cannot be served by a process-wide
    /// answer anyway.
    pub fn with_language(mut self, tag: &str) -> Result<Self, crate::FontError> {
        let lang = tag
            .parse::<rustybuzz::Language>()
            .map_err(|e| crate::FontError(format!("language `{tag}`: {e}")))?;
        self.language = Some(lang);
        Ok(self)
    }

    /// The script, as an ISO 15924 tag: `"Latn"`, `"Arab"`, `"Deva"`.
    ///
    /// Derived from the text when unset, which is usually right. Worth setting
    /// for a run whose characters do not say — digits and punctuation alone
    /// belong to no script — or where the app knows the surrounding context
    /// that a single run has lost.
    pub fn with_script(mut self, tag: &str) -> Result<Self, crate::FontError> {
        let bytes: [u8; 4] = tag
            .as_bytes()
            .try_into()
            .map_err(|_| crate::FontError(format!("script `{tag}`: an ISO 15924 tag is four characters")))?;
        let script = rustybuzz::Script::from_iso15924_tag(rustybuzz::ttf_parser::Tag::from_bytes(&bytes))
            .ok_or_else(|| crate::FontError(format!("script `{tag}`: not an ISO 15924 tag")))?;
        self.script = Some(script);
        Ok(self)
    }

    /// The language tag in force, if one was set.
    pub fn language(&self) -> Option<&str> {
        self.language.as_ref().map(|l| l.as_str())
    }

    /// Which way the text runs. Derived from the script when unset.
    ///
    /// Note what this does and does not do: it decides how the *shaper* treats
    /// the run, so an RTL run gets its joined forms either way. It does not
    /// lay the run out right to left — see the note on right-to-left above.
    pub fn with_direction(mut self, dir: TextDirection) -> Self {
        self.direction = Some(match dir {
            TextDirection::LeftToRight => rustybuzz::Direction::LeftToRight,
            TextDirection::RightToLeft => rustybuzz::Direction::RightToLeft,
        });
        self
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
        // Whatever the app told us, first: `clear` resets all three, so they
        // are set per call rather than once at construction.
        if let Some(l) = &self.language {
            buf.set_language(l.clone());
        }
        if let Some(s) = self.script {
            buf.set_script(s);
        }
        if let Some(d) = self.direction {
            buf.set_direction(d);
        }
        // Then fill the rest from the characters. This only sets what is still
        // unset, and reads nothing outside the string — no process locale, no
        // environment. Language it leaves alone entirely, which is why
        // `with_language` exists.
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
