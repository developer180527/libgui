//! Glyph rasterisation + atlas. fontdue handles rasterising; there is no
//! complex shaping yet (see README: swap in cosmic-text/swash or HarfBuzz for
//! ligatures, bidi, and fallback fonts).

use crate::{Color, DrawList, Rect, Vec2};
use crate::hash::FxMap;
use std::cell::RefCell;
use std::fmt;

/// Distinct strings cached per (font, size) before the cache is dropped. Keeps
/// a label that changes every frame (a frame-time readout) from growing it
/// without bound; a UI with this many live strings wants a virtualised list.
const MEASURE_CAP: usize = 50_000;

// Not cached: kerning. `fontdue::Font::horizontal_kern` turned out to be
// cheaper than a map lookup on a (font, left, right, px) key, and caching it
// measured 8% slower on `label` and 15% slower on `button`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontId(pub u16);

/// The supplied bytes are not a font this library can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontError(pub String);

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid font data: {}", self.0)
    }
}

impl std::error::Error for FontError {}

/// Single-channel coverage atlas. `version` bumps whenever pixels change so the
/// renderer knows when to re-upload.
pub struct Atlas {
    pub size: u32,
    pub data: Vec<u8>,
    pub version: u64,
    cursor: (u32, u32),
    row_h: u32,
}

impl Atlas {
    fn new(size: u32) -> Self {
        Self { size, data: vec![0; (size * size) as usize], version: 1, cursor: (1, 1), row_h: 0 }
    }

    /// Could a `w` x `h` glyph ever fit, even in a freshly reset atlas?
    fn fits(&self, w: u32, h: u32) -> bool {
        let pad = 1;
        1 + w + pad <= self.size && 1 + h + pad <= self.size
    }

    /// Shelf packer. Returns None when full (or when the glyph is too large,
    /// in which case no amount of resetting would help).
    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let pad = 1;
        if !self.fits(w, h) {
            return None;
        }
        if self.cursor.0 + w + pad > self.size {
            self.cursor = (1, self.cursor.1 + self.row_h + pad);
            self.row_h = 0;
        }
        if self.cursor.1 + h + pad > self.size {
            return None;
        }
        let pos = self.cursor;
        self.cursor.0 += w + pad;
        self.row_h = self.row_h.max(h);
        Some(pos)
    }

    fn reset(&mut self) {
        self.data.fill(0);
        self.cursor = (1, 1);
        self.row_h = 0;
        self.version += 1;
    }
}

#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    w: f32,
    h: f32,
    xmin: f32,
    ymin: f32,
    advance: f32,
}

pub struct Fonts {
    fonts: Vec<fontdue::Font>,
    glyphs: FxMap<(u16, char, u32), Glyph>,
    /// (font, px) -> (ascent, descent), in physical px.
    lines: RefCell<FxMap<(u16, u32), (f32, f32)>>,
    /// (font, px) -> text -> advance width, in physical px. Cached *before*
    /// dividing by `scale`, so one entry stays correct across DPI changes.
    widths: RefCell<FxMap<(u16, u32), FxMap<String, f32>>>,
    atlas: Atlas,
    scale: f32,
    /// Extra resolution for text inside a zoomed canvas.
    zoom: f32,
}

impl Fonts {
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            glyphs: FxMap::default(),
            lines: RefCell::new(FxMap::default()),
            widths: RefCell::new(FxMap::default()),
            atlas: Atlas::new(2048),
            scale: 1.0,
            zoom: 1.0,
        }
    }

    /// Parse and register a font. Fails rather than panicking, so a host
    /// loading a user-chosen font can fall back to a built-in one.
    pub fn add_font(&mut self, bytes: &[u8]) -> Result<FontId, FontError> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| FontError(e.to_string()))?;
        self.fonts.push(font);
        Ok(FontId(self.fonts.len() as u16 - 1))
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    pub(crate) fn set_scale(&mut self, scale: f32) {
        self.scale = scale.max(0.5);
    }

    /// Rasterise text inside a zoomed canvas at that resolution, so it stays
    /// crisp instead of being a scaled-up 1x bitmap.
    ///
    /// Quantised to quarter steps: a continuous zoom would otherwise rasterise
    /// a new size every frame and thrash the atlas. Glyphs are rasterised at the
    /// rounded pixel size but *spaced* at the exact requested size (see
    /// [`Fonts::fit`]), so [`Fonts::measure`] and the drawn width are the same at
    /// any zoom or fractional DPI, and layout does not change with zoom.
    pub(crate) fn set_zoom(&mut self, zoom: f32) {
        let q = if zoom >= 1.0 { (zoom * 4.0).round() / 4.0 } else { (zoom * 16.0).round() / 16.0 };
        self.zoom = q.clamp(0.05, 16.0);
    }

    /// Physical pixel size, rounded so the glyph cache stays small.
    fn px(&self, size: f32) -> f32 {
        (size * self.scale * self.zoom).round().max(1.0)
    }

    /// Physical pixels per logical pixel for the text being drawn now.
    fn text_scale(&self) -> f32 {
        self.scale * self.zoom
    }

    /// Wanted physical size over the rasterised (rounded) size, ≈1. Advances and
    /// vertical metrics of the `px` raster are multiplied by this so text lays
    /// out at exactly `size`, whatever the rounding. Bitmaps stay unscaled, so
    /// glyphs remain pixel-crisp.
    fn fit(&self, size: f32, px: f32) -> f32 {
        size * self.text_scale() / px
    }

    fn line(&self, font: FontId, px: f32) -> (f32, f32) {
        let key = (font.0, px as u32);
        if let Some(v) = self.lines.borrow().get(&key) {
            return *v;
        }
        let m = self.fonts[font.0 as usize].horizontal_line_metrics(px);
        let v = m.map(|m| (m.ascent, m.descent)).unwrap_or((px * 0.8, -px * 0.2));
        self.lines.borrow_mut().insert(key, v);
        v
    }

    /// Advance width of one line in *physical* px, memoised. Interactive
    /// widgets measure their label twice a frame (once to size the node, once
    /// to centre or align the text inside the paint closure), and every widget
    /// measures the same strings again next frame.
    fn width_px(&self, font: FontId, px: f32, text: &str) -> f32 {
        let key = (font.0, px as u32);
        if let Some(w) = self.widths.borrow().get(&key).and_then(|m| m.get(text)) {
            return *w;
        }
        let f = &self.fonts[font.0 as usize];
        let mut w = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            if let Some(p) = prev {
                w += f.horizontal_kern(p, ch, px).unwrap_or(0.0);
            }
            w += f.metrics(ch, px).advance_width;
            prev = Some(ch);
        }
        let mut cache = self.widths.borrow_mut();
        let m = cache.entry(key).or_default();
        if m.len() >= MEASURE_CAP {
            m.clear();
        }
        m.insert(text.to_string(), w);
        w
    }

    /// Size of a single line of text in logical px.
    pub fn measure(&self, font: FontId, size: f32, text: &str) -> Vec2 {
        let px = self.px(size);
        let w = self.width_px(font, px, text);
        let (asc, desc) = self.line(font, px);
        // Logical px per raster px: exact `size`, independent of rounding and zoom.
        let k = size / px;
        Vec2::new((w * k).ceil(), ((asc - desc) * k).ceil())
    }

    /// Caret x positions (logical px from the text start) before each char and
    /// after the last one: `len == chars + 1`.
    ///
    /// Snapped to physical pixels the same way [`Fonts::draw`] snaps its pen, so
    /// a caret lines up with the glyph it precedes instead of drifting from it
    /// along a long line. (The kerning between a pair is applied to the second
    /// glyph, not to the caret between them, which is what you want: the caret
    /// sits on the advance boundary.)
    pub fn carets(&self, font: FontId, size: f32, text: &str) -> Vec<f32> {
        let px = self.px(size);
        let r = self.fit(size, px);
        let f = &self.fonts[font.0 as usize];
        let mut out = Vec::with_capacity(text.chars().count() + 1);
        let mut x = 0.0;
        let mut prev = None;
        out.push(0.0);
        for ch in text.chars() {
            if let Some(p) = prev {
                x += f.horizontal_kern(p, ch, px).unwrap_or(0.0);
            }
            x += f.metrics(ch, px).advance_width;
            out.push((x * r).round() / self.text_scale());
            prev = Some(ch);
        }
        out
    }

    /// Height of one line of text in logical px.
    pub fn line_height(&self, font: FontId, size: f32) -> f32 {
        let px = self.px(size);
        let (asc, desc) = self.line(font, px);
        ((asc - desc) * size / px).ceil()
    }

    fn glyph(&mut self, font: FontId, ch: char, px: f32) -> Glyph {
        let key = (font.0, ch, px as u32);
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        let (m, bitmap) = self.fonts[font.0 as usize].rasterize(ch, px);
        let (w, h) = (m.width as u32, m.height as u32);
        let mut uv = [0.0; 4];
        // Zero when the glyph could not be placed: it is then skipped by `draw`
        // (invisible) while its advance still counts, so layout stays correct.
        // Better than aborting on a font size larger than the atlas, which a
        // theme file or a zoomed canvas can ask for.
        let mut placed = (w, h);
        if w > 0 && h > 0 {
            let pos = if !self.atlas.fits(w, h) {
                None
            } else {
                match self.atlas.alloc(w, h) {
                    Some(p) => Some(p),
                    None => {
                        // Full: start over. Glyphs already emitted this frame may
                        // flicker for one frame; a real implementation would use
                        // multiple pages or LRU eviction.
                        self.atlas.reset();
                        self.glyphs.clear();
                        self.atlas.alloc(w, h)
                    }
                }
            };
            match pos {
                Some(pos) => {
                    let s = self.atlas.size;
                    for row in 0..h {
                        let dst = ((pos.1 + row) * s + pos.0) as usize;
                        let src = (row * w) as usize;
                        self.atlas.data[dst..dst + w as usize].copy_from_slice(&bitmap[src..src + w as usize]);
                    }
                    self.atlas.version += 1;
                    let sf = s as f32;
                    uv = [
                        pos.0 as f32 / sf,
                        pos.1 as f32 / sf,
                        (pos.0 + w) as f32 / sf,
                        (pos.1 + h) as f32 / sf,
                    ];
                }
                None => placed = (0, 0),
            }
        }
        let g = Glyph {
            uv,
            w: placed.0 as f32,
            h: placed.1 as f32,
            xmin: m.xmin as f32,
            ymin: m.ymin as f32,
            advance: m.advance_width,
        };
        self.glyphs.insert(key, g);
        g
    }

    /// Draw one line of text with its top-left at `pos` (logical px). Glyphs
    /// are rasterised at physical resolution and pixel-snapped.
    pub fn draw(&mut self, dl: &mut DrawList, font: FontId, size: f32, pos: Vec2, color: Color, text: &str) {
        let s = self.text_scale();
        let px = self.px(size);
        let r = self.fit(size, px);
        let (asc, _) = self.line(font, px);
        let x0 = (pos.x * s).round();
        // Pen advances in raster px, placed at the exact size (`r`); glyphs are
        // drawn at their native raster size and snapped, so they stay crisp.
        let mut x = 0.0;
        let baseline = (pos.y * s + asc * r).round();
        let mut prev = None;
        for ch in text.chars() {
            if let Some(p) = prev {
                x += self.fonts[font.0 as usize].horizontal_kern(p, ch, px).unwrap_or(0.0);
            }
            let g = self.glyph(font, ch, px);
            if g.w > 0.0 {
                let gx = (x0 + x * r).round() + g.xmin;
                let gy = baseline - (g.ymin + g.h);
                dl.glyph(Rect::new(gx / s, gy / s, g.w / s, g.h / s), g.uv, color);
            }
            x += g.advance;
            prev = Some(ch);
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts(scale: f32) -> Fonts {
        let mut f = Fonts::new();
        f.add_font(include_bytes!("../../../assets/Inter.ttf")).unwrap();
        f.set_scale(scale);
        f
    }

    /// Text is laid out at zoom 1 but drawn inside a zoomed canvas at a rounded
    /// raster size. Width must not depend on that rounding, or zoomed text
    /// overflows (or falls short of) the box it was laid out in.
    #[test]
    fn zoom_and_fractional_dpi_do_not_change_text_width() {
        let label = "Amount · Enabled · 0.50 Wavy";
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut f = fonts(scale);
            let font = FontId(0);
            for size in [10.5, 11.0, 13.0, 16.0] {
                f.set_zoom(1.0);
                let laid_out = f.measure(font, size, label);
                for zoom in [0.125, 0.3125, 0.5, 0.75, 1.25, 1.75, 2.5, 4.0] {
                    f.set_zoom(zoom);
                    let m = f.measure(font, size, label);
                    assert!((m.x - laid_out.x).abs() <= 1.0, "scale {scale} size {size} zoom {zoom}: {} vs {}", m.x, laid_out.x);
                    assert!((m.y - laid_out.y).abs() <= 1.0, "height at scale {scale} size {size} zoom {zoom}");

                    // The glyphs actually drawn stay within the laid-out width.
                    let mut dl = DrawList::default();
                    dl.clear(Rect::new(0.0, 0.0, 10_000.0, 10_000.0));
                    f.draw(&mut dl, font, size, Vec2::ZERO, Color::WHITE, label);
                    let right = dl.instances.iter().map(|i| i.rect[0] + i.rect[2]).fold(0.0f32, f32::max);
                    // Allowed: two *screen* pixels (pixel snapping plus a glyph's
                    // bitmap overhanging its advance), whatever the zoom.
                    let slack = 1.0 + 2.0 / (scale * f.zoom);
                    assert!(
                        right <= laid_out.x + slack,
                        "scale {scale} size {size} zoom {zoom}: drawn {right} > laid out {} (+{slack})",
                        laid_out.x
                    );
                }
            }
        }
    }
}
