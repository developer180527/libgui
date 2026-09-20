//! Text layout on top of a pluggable [`FontRasterizer`]: measurement (cached),
//! DPI and zoom fitting, caret positions, the glyph atlas and pixel snapping.
//! Shaping and rasterising belong to the rasterizer (see `font.rs`).

use crate::font::{FontRasterizer, ShapedGlyph};
use crate::{Color, DrawList, Rect, Vec2};
use crate::hash::FxMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::fmt;

/// Distinct strings cached per (font, size) before the cache is dropped. Keeps
/// a label that changes every frame (a frame-time readout) from growing it
/// without bound; a UI with this many live strings wants a virtualised list.
const MEASURE_CAP: usize = 50_000;

// Shaping *is* cached, per (font, px, string): re-shaping every visible label
// every frame made `label` ~70% slower with fontdue, and a real shaper
// (HarfBuzz) costs more still. Kerning pairs are not cached separately: that
// measured 8-15% slower than recomputing them inside the shaper.

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
    /// How large it may grow before it starts evicting instead. A single
    /// channel, so 4096 is 16 MB — enough for CJK at several sizes, or a
    /// zooming canvas asking for hundreds of them.
    pub max_size: u32,
    pub data: Vec<u8>,
    pub version: u64,
    cursor: (u32, u32),
    row_h: u32,
}

impl Atlas {
    fn new(size: u32) -> Self {
        Self { size, max_size: 4096, data: vec![0; (size * size) as usize], version: 1, cursor: (1, 1), row_h: 0 }
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

    /// Double it, if it is allowed to get any bigger. Everything in it is lost
    /// — the callers clear their caches — but it happens once, where resetting
    /// the same size over and over happens every frame forever.
    fn grow(&mut self) -> bool {
        let next = self.size.saturating_mul(2);
        if next > self.max_size {
            return false;
        }
        self.size = next;
        self.data = vec![0; (next * next) as usize];
        self.cursor = (1, 1);
        self.row_h = 0;
        self.version += 1;
        true
    }
}

#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    w: f32,
    h: f32,
    /// Pen to bitmap left, px.
    left: f32,
    /// Baseline to bitmap bottom, px, +y up.
    bottom: f32,
}

/// A shaped string: its glyphs and total advance, in physical px.
#[derive(Clone)]
struct Run {
    glyphs: Rc<[ShapedGlyph]>,
    width: f32,
}

pub struct Fonts {
    fonts: Vec<Box<dyn FontRasterizer>>,
    /// (font, glyph id, px) -> atlas entry.
    glyphs: FxMap<(u16, u32, u32), Glyph>,
    /// Scratch for shaping a string the run cache has not seen yet.
    shaped: RefCell<Vec<ShapedGlyph>>,
    /// (font, px) -> text -> shaped run, in physical px. Keyed *before*
    /// dividing by `scale`, so one entry stays correct across DPI changes.
    runs: RefCell<FxMap<(u16, u32), FxMap<String, Run>>>,
    /// (font, px) -> (ascent, descent), in physical px.
    lines: RefCell<FxMap<(u16, u32), (f32, f32)>>,
    atlas: Atlas,
    scale: f32,
    /// Extra resolution for text inside a zoomed canvas.
    zoom: f32,
    /// Glyphs rasterised since the counter was last taken. A steady frame
    /// rasterises none; a frame that does is doing work it will not repeat.
    rasterized: u32,
    /// Strings shaped since the counter was last taken (run-cache misses).
    /// `Cell`, because shaping happens through `&self` on the measure path.
    shaped_runs: std::cell::Cell<u32>,
    /// Strings drawn since the counter was last taken.
    text_draws: u32,
    /// (font, px, width in whole px) -> text -> the lines it breaks into.
    /// Wrapping is asked for twice a frame — once to measure, once to draw —
    /// and the answer only changes when the width does.
    wraps: RefCell<WrapCache>,
    /// Scratch for the break opportunities of the string being wrapped.
    breaks: RefCell<Vec<crate::wrap::Opportunity>>,
}

/// (font, px, width in whole px) -> text -> its lines.
type WrapCache = FxMap<(u16, u32, u32), FxMap<String, Rc<[Line]>>>;

/// One line of wrapped text: the byte range to draw, and how wide it is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line {
    pub start: u32,
    pub end: u32,
    /// Raster px.
    pub width: f32,
}

impl Fonts {
    /// Glyphs rasterised since the last call, and reset. [`Ui`](crate::Ui)
    /// takes it once a frame for [`FrameCost`](crate::testing::FrameCost).
    pub fn take_rasterized(&mut self) -> u32 {
        std::mem::take(&mut self.rasterized)
    }

    /// Strings shaped since the last call, and reset.
    pub fn take_shaped_runs(&mut self) -> u32 {
        self.shaped_runs.replace(0)
    }

    /// Strings drawn since the last call, cache hits included, and reset.
    pub fn take_text_draws(&mut self) -> u32 {
        std::mem::take(&mut self.text_draws)
    }

    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            glyphs: FxMap::default(),
            shaped: RefCell::new(Vec::new()),
            lines: RefCell::new(FxMap::default()),
            runs: RefCell::new(FxMap::default()),
            atlas: Atlas::new(2048),
            scale: 1.0,
            zoom: 1.0,
            rasterized: 0,
            shaped_runs: std::cell::Cell::new(0),
            text_draws: 0,
            wraps: RefCell::new(FxMap::default()),
            breaks: RefCell::new(Vec::new()),
        }
    }

    /// Parse and register a font with the built-in fontdue backend. Fails
    /// rather than panicking, so a host loading a user-chosen font can fall
    /// back to a built-in one.
    #[cfg(feature = "fontdue")]
    pub fn add_font(&mut self, bytes: &[u8]) -> Result<FontId, FontError> {
        let r = crate::font::FontdueRasterizer::from_bytes(bytes)?;
        Ok(self.add_rasterizer(Box::new(r)))
    }

    /// Register a font face backed by your own rasterizer (FreeType +
    /// HarfBuzz, CoreText, DirectWrite, an engine's text system…).
    pub fn add_rasterizer(&mut self, rasterizer: Box<dyn FontRasterizer>) -> FontId {
        self.fonts.push(rasterizer);
        FontId(self.fonts.len() as u16 - 1)
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
        let m = self.fonts[font.0 as usize].line_metrics(px);
        let v = (m.ascent, m.descent);
        self.lines.borrow_mut().insert(key, v);
        v
    }

    /// The shaped run for `text`, memoised. Interactive widgets measure their
    /// label twice a frame (once to size the node, once to align it in the
    /// paint closure), draw it, and do it all again next frame: after the first
    /// frame each of those is one hash lookup.
    fn run(&self, font: FontId, px: f32, text: &str) -> Run {
        let key = (font.0, px as u32);
        if let Some(r) = self.runs.borrow().get(&key).and_then(|m| m.get(text)) {
            return r.clone();
        }
        self.shaped_runs.set(self.shaped_runs.get() + 1);
        let run = {
            let mut shaped = self.shaped.borrow_mut();
            shaped.clear();
            self.fonts[font.0 as usize].shape(text, px, &mut shaped);
            Run { width: shaped.iter().map(|g| g.advance).sum(), glyphs: Rc::from(shaped.as_slice()) }
        };
        let mut cache = self.runs.borrow_mut();
        let m = cache.entry(key).or_default();
        if m.len() >= MEASURE_CAP {
            m.clear();
        }
        m.insert(text.to_string(), run.clone());
        run
    }

    fn width_px(&self, font: FontId, px: f32, text: &str) -> f32 {
        // Hot path (every measure): read the width in place, no run clone.
        if let Some(r) = self.runs.borrow().get(&(font.0, px as u32)).and_then(|m| m.get(text)) {
            return r.width;
        }
        self.run(font, px, text).width
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
    /// A caret sits at the pen position of the first glyph whose cluster starts
    /// at or after its character, so characters inside a ligature share the
    /// ligature's end. Snapped to physical pixels the way [`Fonts::draw`] snaps
    /// its pen, so a caret lines up with the glyph it precedes.
    pub fn carets(&self, font: FontId, size: f32, text: &str) -> Vec<f32> {
        let px = self.px(size);
        let r = self.fit(size, px);
        let s = self.text_scale();
        let run = self.run(font, px, text);
        let shaped = &run.glyphs;
        let mut out = Vec::with_capacity(text.len() + 1);
        let (mut j, mut pen) = (0usize, 0.0f32);
        for (byte, _) in text.char_indices() {
            while j < shaped.len() && (shaped[j].cluster as usize) < byte {
                pen += shaped[j].advance;
                j += 1;
            }
            out.push((pen * r).round() / s);
        }
        out.push((run.width * r).round() / s);
        out
    }

    /// Break `text` into lines no wider than `max` logical px.
    ///
    /// Greedy: each line takes as much as fits. That is what every UI toolkit
    /// does — the alternative, minimising raggedness across the paragraph, is
    /// for typesetting, and it makes a line's contents depend on lines after
    /// it, which is not something a caret wants.
    pub(crate) fn wrap(&self, font: FontId, size: f32, text: &str, max: f32) -> Rc<[Line]> {
        let px = self.px(size);
        let k = px / size.max(0.01);
        let max_px = (max * k).max(1.0);
        let key = (font.0, px as u32, max_px as u32);
        if let Some(l) = self.wraps.borrow().get(&key).and_then(|m| m.get(text)) {
            return l.clone();
        }
        let run = self.run(font, px, text);
        let mut ops = self.breaks.borrow_mut();
        crate::wrap::opportunities(text, &mut ops);

        let mut lines: Vec<Line> = Vec::new();
        let mut start = 0usize;
        let mut next_op = 0usize;
        loop {
            // A newline is mandatory: nothing after it may share this line,
            // however much room is left.
            let hard = ops[next_op..].iter().find(|o| o.at > start && text.as_bytes().get(o.trim) == Some(&b'\n'));
            let limit = hard.map_or(text.len(), |o| o.trim);

            let fits = width_between(&run, start, limit);
            if fits <= max_px {
                lines.push(Line { start: start as u32, end: limit as u32, width: fits });
                match hard {
                    Some(o) => start = o.at,
                    None => break,
                }
            } else {
                // Greedy: the furthest soft break that still fits. Widths grow
                // with the offset, so the first that does not fit ends the
                // search.
                let mut best: Option<crate::wrap::Opportunity> = None;
                for o in ops[next_op..].iter() {
                    if o.at <= start {
                        continue;
                    }
                    if o.trim > limit {
                        break;
                    }
                    if width_between(&run, start, o.trim) > max_px {
                        break;
                    }
                    best = Some(*o);
                }
                match best {
                    Some(o) => {
                        lines.push(Line {
                            start: start as u32,
                            end: o.trim as u32,
                            width: width_between(&run, start, o.trim),
                        });
                        start = o.at;
                    }
                    // Nothing fits and nowhere to break: cut the word rather
                    // than let it overflow.
                    None => {
                        let cut = cut_to_fit(&run, text, start, limit, max_px);
                        lines.push(Line {
                            start: start as u32,
                            end: cut as u32,
                            width: width_between(&run, start, cut),
                        });
                        start = cut;
                    }
                }
            }
            while next_op < ops.len() && ops[next_op].at <= start {
                next_op += 1;
            }
            if start >= text.len() {
                break;
            }
        }
        if lines.is_empty() {
            lines.push(Line { start: 0, end: 0, width: 0.0 });
        }
        let lines: Rc<[Line]> = Rc::from(lines.as_slice());
        let mut cache = self.wraps.borrow_mut();
        let m = cache.entry(key).or_default();
        if m.len() >= MEASURE_CAP {
            m.clear();
        }
        m.insert(text.to_string(), lines.clone());
        lines
    }

    /// The wrapped lines as strings, for tests.
    #[doc(hidden)]
    pub fn wrap_lines_for_test(&self, font: FontId, size: f32, text: &str, max: f32) -> Vec<String> {
        self.wrap(font, size, text, max).iter().map(|l| text[l.start as usize..l.end as usize].to_string()).collect()
    }

    /// Size of `text` wrapped to `max` logical px.
    pub fn measure_wrapped(&self, font: FontId, size: f32, text: &str, max: f32) -> Vec2 {
        let lines = self.wrap(font, size, text, max);
        let px = self.px(size);
        let k = size / px;
        let widest = lines.iter().fold(0.0f32, |a, l| a.max(l.width));
        let lh = self.line_height(font, size);
        Vec2::new((widest * k).ceil(), lh * lines.len() as f32)
    }

    /// The narrowest `max` at which `text` wraps without cutting a word: the
    /// width of its longest unbreakable run. A wrapping paragraph reports this
    /// as its minimum, so a container can always be narrower than the text.
    pub fn min_wrap_width(&self, font: FontId, size: f32, text: &str) -> f32 {
        let px = self.px(size);
        let k = size / px;
        let run = self.run(font, px, text);
        let mut ops = self.breaks.borrow_mut();
        crate::wrap::opportunities(text, &mut ops);
        let mut widest = 0.0f32;
        let mut start = 0usize;
        for o in ops.iter() {
            widest = widest.max(width_between(&run, start, o.trim));
            start = o.at;
        }
        widest = widest.max(width_between(&run, start, text.len()));
        (widest * k).ceil()
    }

    /// Height of one line of text in logical px.
    pub fn line_height(&self, font: FontId, size: f32) -> f32 {
        let px = self.px(size);
        let (asc, desc) = self.line(font, px);
        ((asc - desc) * size / px).ceil()
    }

    fn glyph(&mut self, font: FontId, id: u32, px: f32) -> Glyph {
        let key = (font.0, id, px as u32);
        if let Some(g) = self.glyphs.get(&key) {
            return *g;
        }
        self.rasterized += 1;
        let m = self.fonts[font.0 as usize].rasterize(id, px);
        let bitmap = &m.coverage;
        let (w, h) = (m.width, m.height);
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
                        // Full. Growing costs one re-rasterisation of
                        // everything, once; resetting the same size costs the
                        // same re-rasterisation *every frame*, because the
                        // working set that overflowed it is still there next
                        // frame. A torture test with 400 font sizes found this
                        // the expensive way: 1,704 glyphs rasterised, every
                        // frame, forever.
                        if !self.atlas.grow() {
                            self.atlas.reset();
                        }
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
            left: m.left,
            bottom: m.bottom,
        };
        self.glyphs.insert(key, g);
        g
    }

    /// Draw `text` wrapped to `r`'s width, from its top-left down.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_wrapped(
        &mut self,
        dl: &mut DrawList,
        font: FontId,
        size: f32,
        r: Rect,
        color: Color,
        align: crate::Align,
        text: &str,
    ) {
        let lines = self.wrap(font, size, text, r.w);
        let lh = self.line_height(font, size);
        let px = self.px(size);
        let k = size / px;
        for (i, l) in lines.iter().enumerate() {
            let slice = &text[l.start as usize..l.end as usize];
            if slice.is_empty() {
                continue;
            }
            let w = l.width * k;
            let x = match align {
                crate::Align::End => r.x + r.w - w,
                crate::Align::Center => r.x + (r.w - w) * 0.5,
                _ => r.x,
            };
            self.draw(dl, font, size, Vec2::new(x, r.y + lh * i as f32), color, slice);
        }
    }

    /// Draw one line of text with its top-left at `pos` (logical px). Glyphs
    /// are rasterised at physical resolution and pixel-snapped.
    pub fn draw(&mut self, dl: &mut DrawList, font: FontId, size: f32, pos: Vec2, color: Color, text: &str) {
        let s = self.text_scale();
        let px = self.px(size);
        let r = self.fit(size, px);
        let (asc, _) = self.line(font, px);
        // Snapping to whole physical pixels keeps text crisp, and is what
        // the scroll offset is snapped to match. A scroll area that is moving
        // turns it off for its content: a trackpad's first frames move less
        // than a pixel each, and rounding them away is what makes the start of
        // a scroll stutter. Moving text is resampled (the shader filters the
        // atlas bilinearly), which is invisible in motion and exact again the
        // moment the scroll comes to rest.
        self.text_draws += 1;
        let snap = dl.snap_text();
        let round = |v: f32| if snap { v.round() } else { v };
        let x0 = round(pos.x * s);
        // Pen advances in raster px, placed at the exact size (`r`); glyphs are
        // drawn at their native raster size, so at rest they are texel-exact.
        let mut x = 0.0;
        let baseline = round(pos.y * s + asc * r);
        // The run is reference-counted, so holding it while rasterising glyphs
        // (which needs `&mut self`) costs no copy.
        let run = self.run(font, px, text);
        for sg in run.glyphs.iter() {
            let g = self.glyph(font, sg.glyph, px);
            if g.w > 0.0 {
                let gx = round(x0 + (x + sg.offset.x) * r) + g.left;
                let gy = baseline + round(sg.offset.y * r) - (g.bottom + g.h);
                dl.glyph(Rect::new(gx / s, gy / s, g.w / s, g.h / s), g.uv, color);
            }
            x += sg.advance;
        }
    }
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

/// Width of `text[a..b]` in raster px, from an already-shaped run.
fn width_between(run: &Run, a: usize, b: usize) -> f32 {
    run.glyphs
        .iter()
        .filter(|g| (g.cluster as usize) >= a && (g.cluster as usize) < b)
        .map(|g| g.advance)
        .sum()
}

/// The furthest end offset in `start..limit` that still fits in `max`, and
/// never `start` itself: a line holding nothing would not terminate.
fn cut_to_fit(run: &Run, text: &str, start: usize, limit: usize, max: f32) -> usize {
    let mut w = 0.0;
    let mut end = start;
    for g in run.glyphs.iter() {
        let at = g.cluster as usize;
        if at < start || at >= limit {
            continue;
        }
        if w + g.advance > max && end > start {
            return end;
        }
        w += g.advance;
        // The glyph's cluster is where it *starts*, so the line ends after it.
        end = text[at..limit].char_indices().nth(1).map_or(limit, |(i, _)| at + i);
    }
    // One glyph is wider than the whole line: take a single character, or the
    // caller would ask again with the same `start` forever.
    if end <= start {
        return text[start..limit].char_indices().nth(1).map_or(limit, |(i, _)| start + i);
    }
    end
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
