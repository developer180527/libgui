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

/// The default for [`Fonts::set_tab_width`]. Four is the common figure, and it
/// is a default rather than a rule: tab width is an app's preference, and
/// often a per-language one (Go is eight, plenty of web repositories are two).
pub const DEFAULT_TAB_WIDTH: usize = 4;

/// One page of the glyph atlas: a single-channel coverage image, `size` by
/// `size`. `version` bumps whenever its pixels change, so a renderer uploads a
/// page again only when that page changed.
#[derive(Clone)]
pub struct AtlasPage {
    pub size: u32,
    /// Behind an `Rc` so the atlas is cheap to *clone*, which is what lets a
    /// finished frame carry a snapshot of it instead of borrowing the font
    /// system — and so several `Ui`s can share one. Writing to it clones the
    /// image only while a snapshot is outstanding, which a host that uploads
    /// and drops its frame output never causes.
    pub data: Rc<Vec<u8>>,
    pub version: u64,
    cursor: (u32, u32),
    row_h: u32,
}

impl AtlasPage {
    fn new(size: u32) -> Self {
        Self { size, data: Rc::new(vec![0; (size * size) as usize]), version: 1, cursor: (1, 1), row_h: 0 }
    }

    /// Could a `w` x `h` bitmap fit on an empty page of this size?
    fn fits(&self, w: u32, h: u32) -> bool {
        // A texel of padding each side.
        w + 2 <= self.size && h + 2 <= self.size
    }

    /// Shelf packer. None when full or the bitmap is too large for the page.
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

    /// Empty it for reuse. Its version bumps, so a renderer uploads it again.
    fn clear(&mut self) {
        Rc::make_mut(&mut self.data).fill(0);
        self.cursor = (1, 1);
        self.row_h = 0;
        self.version += 1;
    }
}

/// The glyph atlas: pages of coverage that glyphs and filled paths are packed
/// into, drawn as [`TextureId::Atlas`](crate::TextureId::Atlas)`(page)`.
///
/// **Nothing on screen is ever lost to a full atlas.** Every page records the
/// frame it was last drawn from. When the pages are full and the budget allows
/// no more, the page used longest ago is emptied and reused at once, mid-frame
/// — safe, because nothing drawn this frame points into it. Only a single frame
/// that needs more glyphs than every page together holds can miss one, and it
/// is counted ([`FrameCost::atlas_overflows`](crate::testing::FrameCost)).
#[derive(Clone)]
pub struct Atlas {
    /// Behind an `Rc` like each page's texels, so a frame's snapshot of the
    /// atlas is a reference count, not a copy; the list is copied only when a
    /// glyph is placed while a snapshot is held.
    pub pages: Rc<Vec<AtlasPage>>,
    /// The memory budget, as the side of one square: all pages together hold at
    /// most `max_size²` texels. The default is 4096 (16 MB, four pages).
    pub max_size: u32,
    /// The side of an ordinary page. A glyph too big for one gets a page of
    /// its own, sized to fit, inside the same budget.
    pub page_size: u32,
    /// Bumped whenever a page is emptied for reuse: anything holding uv
    /// coordinates into the atlas from before may be stale.
    pub repacks: u64,
    /// The page new bitmaps go to.
    open: usize,
}

impl Atlas {
    fn new(page_size: u32) -> Self {
        Self { pages: Rc::new(vec![AtlasPage::new(page_size)]), max_size: 4096, page_size, repacks: 0, open: 0 }
    }

    /// The first page: where everything goes until it fills.
    pub fn first(&self) -> &AtlasPage {
        &self.pages[0]
    }

    /// Total texels held, all pages.
    pub fn texels(&self) -> u64 {
        self.pages.iter().map(|p| p.size as u64 * p.size as u64).sum()
    }

    fn budget(&self) -> u64 {
        self.max_size as u64 * self.max_size as u64
    }

    /// Could a `w` x `h` bitmap ever be placed within the budget?
    fn could_fit(&self, w: u32, h: u32) -> bool {
        let side = (w.max(h) + 2).next_power_of_two().max(self.page_size);
        side <= self.max_size
    }
}

#[derive(Clone, Copy)]
struct Glyph {
    /// The atlas page it is on.
    page: u32,
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

/// Everything the font system owns. Private: `Fonts` is the handle to it, so
/// that several `Ui`s can be given the same one.
struct FontsInner {
    fonts: Vec<Box<dyn FontRasterizer>>,
    /// (font, face, glyph id, px) -> atlas entry. The face is part of the key
    /// because a glyph id only means something inside the face that issued it:
    /// glyph 42 of a fallback CJK face is not glyph 42 of the UI font.
    glyphs: FxMap<(u16, u16, u32, u32), Glyph>,
    /// Scratch for shaping a string the run cache has not seen yet.
    shaped: RefCell<Vec<ShapedGlyph>>,
    /// The blank glyph, its face and the space advance per (font, px), for
    /// laying out tabs.
    shaped_space: RefCell<SpaceCache>,
    /// (font, px) -> text -> shaped run, in physical px. Keyed *before*
    /// dividing by `scale`, so one entry stays correct across DPI changes.
    runs: RefCell<FxMap<(u16, u32), FxMap<String, Run>>>,
    /// (font, px) -> (ascent, descent), in physical px.
    lines: RefCell<FxMap<(u16, u32), (f32, f32)>>,
    atlas: Atlas,
    /// Filled paths in the atlas: their key at a size -> (page, uv), `None`
    /// when it can never fit. Dropped with the glyphs of a page it shared when
    /// that page is reused.
    masks: FxMap<u64, Option<(u32, [f32; 4])>>,
    /// Reused between rasterisations, so a path costs no allocation.
    mask_scratch: Vec<u8>,
    scale: f32,
    /// Extra resolution for text inside a zoomed canvas.
    zoom: f32,
    /// Glyphs rasterised since the counter was last taken. A steady frame
    /// rasterises none; a frame that does is doing work it will not repeat.
    rasterized: u32,
    /// A bitmap could not be placed this frame: every page was in use by it.
    /// It is retried next frame, so the host is asked for one.
    repack_pending: bool,
    /// Frames begun on this font system, by every `Ui` sharing it: what a
    /// page's `last_used` is compared with.
    frame: u64,
    /// Per atlas page, the frame something on it was last drawn: a page drawn
    /// from this frame is never reused during it. Kept here, not in the page,
    /// because it changes every frame and the pages are shared with snapshots.
    page_used: Vec<u64>,
    /// Bitmaps that could not be placed, and pages emptied for reuse, since
    /// the counters were last taken.
    overflows: u32,
    evictions: u32,
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
    /// How many spaces wide a tab lays out. Baked into a shaped run, so
    /// changing it drops the caches that hold one.
    tab_width: usize,
}

/// (font, px) -> (the blank glyph, the face that owns it, a space's advance).
type SpaceCache = FxMap<(u16, u32), (u32, u16, f32)>;

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

impl FontsInner {
    /// Glyphs rasterised since the last call, and reset. [`Ui`](crate::Ui)
    /// takes it once a frame for [`FrameCost`](crate::testing::FrameCost).
    /// True while a bitmap is waiting to be placed: the host needs one more
    /// frame before the text is complete.
    pub(crate) fn repack_pending(&self) -> bool {
        self.repack_pending
    }

    /// A new frame starts: pages drawn from before now may be reused. Called
    /// by every `Ui` sharing this font system, from `begin_frame`.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
        // The last frame could not place something: every page was in use by
        // it. Page-at-a-time reuse cannot help then — a page holding one glyph
        // still drawn is pinned however much else on it is stale — so start
        // every page afresh now, between frames, and let this frame rasterise
        // only what it draws. One frame of missing glyphs, and only when a
        // frame outgrew the whole budget.
        if self.repack_pending {
            for pg in Rc::make_mut(&mut self.atlas.pages).iter_mut() {
                if pg.size > 0 {
                    pg.clear();
                }
            }
            self.atlas.repacks += 1;
            self.atlas.open = 0;
            self.glyphs.clear();
            self.masks.clear();
        }
        self.repack_pending = false;
    }

    /// Bitmaps that could not be placed, and pages emptied for reuse, since
    /// the last call; reset.
    pub(crate) fn take_atlas_counts(&mut self) -> (u32, u32) {
        (std::mem::take(&mut self.overflows), std::mem::take(&mut self.evictions))
    }

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
            shaped_space: RefCell::new(Default::default()),
            lines: RefCell::new(FxMap::default()),
            runs: RefCell::new(FxMap::default()),
            atlas: Atlas::new(2048),
            masks: FxMap::default(),
            mask_scratch: Vec::new(),
            scale: 1.0,
            zoom: 1.0,
            rasterized: 0,
            repack_pending: false,
            frame: 1,
            page_used: vec![0],
            overflows: 0,
            evictions: 0,
            shaped_runs: std::cell::Cell::new(0),
            text_draws: 0,
            wraps: RefCell::new(FxMap::default()),
            breaks: RefCell::new(Vec::new()),
            tab_width: DEFAULT_TAB_WIDTH,
        }
    }

    /// Parse and register a font with the built-in fontdue backend. Fails
    /// rather than panicking, so a host loading a user-chosen font can fall
    /// back to a built-in one.
    ///
    /// This registers one *face*, which is what a widget's `FontId` names. To
    /// give that face fallbacks for the scripts it cannot draw, build a
    /// [`FontStack`](crate::FontStack) and register that instead: several
    /// faces behind one id is what makes a mixed-script string come out whole.
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

    /// How many spaces wide a tab lays out. Default [`DEFAULT_TAB_WIDTH`].
    pub fn tab_width(&self) -> usize {
        self.tab_width
    }

    /// Set how many spaces wide a tab lays out.
    ///
    /// Not a true tab *stop* aligned to a multiple — see the note on
    /// `lay_out_tabs` — which is what indentation in a text field wants either
    /// way. Clamped to at least 1: a zero-width tab would stack the characters
    /// after it on top of each other.
    ///
    /// This is per-`Ui`, not per-document: the width is baked into a cached
    /// shaped run, and a per-field width would have to be part of every cache
    /// key and threaded through every measurement. An editor that needs a
    /// different width per language wants one `Ui` per window, which it
    /// probably has anyway, or a re-set between documents.
    ///
    /// Changing it drops the shaped-run and wrap caches, because both hold
    /// advances computed with the old width. Do it when the preference
    /// changes, not per frame.
    pub fn set_tab_width(&mut self, spaces: usize) {
        let spaces = spaces.max(1);
        if spaces == self.tab_width {
            return;
        }
        self.tab_width = spaces;
        self.runs.borrow_mut().clear();
        self.wraps.borrow_mut().clear();
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The glyph atlas's memory budget, as the side of one square: all its
    /// pages together hold at most `max²` texels. The default is 4096 — 16 MB
    /// at one channel, four 2048 pages.
    ///
    /// Raise it for a UI with more live glyphs than that: CJK through a
    /// [`FontStack`](crate::FontStack) at many sizes, heavy caption work, a
    /// canvas zooming through many sizes. Past the budget, the page drawn from
    /// longest ago is emptied and reused, which costs re-rasterising what was
    /// on it if it is wanted again — never a missing glyph, unless a single
    /// frame needs more than the whole budget.
    ///
    /// A glyph bigger than an ordinary page gets a page of its own, sized to
    /// fit and inside the budget: raising the budget is what lets an enormous
    /// glyph be drawn at all.
    ///
    /// Lower it for a device where 16 MB of texture is not free. Pages
    /// already allocated are not freed; the budget applies to the next one.
    /// Rounded up to a power of two and clamped to at least the page size.
    pub fn set_atlas_limit(&mut self, max: u32) {
        self.atlas.max_size = max.max(self.atlas.page_size).next_power_of_two();
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
            if text.contains('\t') {
                self.lay_out_tabs(font, px, text, &mut shaped);
            }
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

    /// Give every tab a blank glyph and a fixed advance.
    ///
    /// A font maps `\t` to whatever it likes — Inter draws a `.notdef` box —
    /// so text carrying tabs has to be laid out here rather than left to the
    /// shaper. The advance is [`Fonts::tab_width`] spaces, not a true tab *stop*
    /// aligned to a multiple: indentation, which is what tabs in a text field
    /// are for, comes out right either way, and a stop would have to know
    /// where the line began.
    fn lay_out_tabs(&self, font: FontId, px: f32, text: &str, glyphs: &mut [crate::ShapedGlyph]) {
        // The space glyph is blank in every font, which saves inventing a
        // "draw nothing" glyph id that the rasteriser would have to know about.
        let mut space = self.shaped_space.borrow_mut();
        let key = (font.0, px as u32);
        let (blank, blank_face, width) = *space.entry(key).or_insert_with(|| {
            let mut out = Vec::new();
            self.fonts[font.0 as usize].shape(" ", px, &mut out);
            out.first().map_or((0, 0, px * 0.25), |g| (g.glyph, g.face, g.advance))
        });
        let bytes = text.as_bytes();
        for g in glyphs.iter_mut() {
            if bytes.get(g.cluster as usize) == Some(&b'\t') {
                g.glyph = blank;
                g.face = blank_face;
                g.advance = width * self.tab_width as f32;
                g.offset = Vec2::ZERO;
            }
        }
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

    /// x of the caret before byte offset `byte`, in logical px from the start
    /// of `text`.
    ///
    /// The same answer as [`Fonts::carets`] at that index, without building
    /// the vector: a text widget wants one or two of these per frame, and
    /// allocating a caret table per visible line is what made a text area
    /// allocate on every frame at rest.
    pub fn caret_x(&self, font: FontId, size: f32, text: &str, byte: usize) -> f32 {
        let px = self.px(size);
        let r = self.fit(size, px);
        let s = self.text_scale();
        let run = self.run(font, px, text);
        if byte >= text.len() {
            return (run.width * r).round() / s;
        }
        let mut pen = 0.0f32;
        for g in run.glyphs.iter() {
            if (g.cluster as usize) >= byte {
                break;
            }
            pen += g.advance;
        }
        (pen * r).round() / s
    }

    /// The byte offset in `text` whose caret is nearest `x` (logical px from
    /// the start of the text). Always a char boundary.
    ///
    /// This is the hit test behind a click and behind Up/Down: both ask "which
    /// character is under this position", which is a question about pixels and
    /// not about character counts.
    pub fn byte_at_x(&self, font: FontId, size: f32, text: &str, x: f32) -> usize {
        let px = self.px(size);
        let r = self.fit(size, px);
        let s = self.text_scale();
        let run = self.run(font, px, text);
        let (mut best, mut best_d) = (0usize, f32::INFINITY);
        let (mut j, mut pen) = (0usize, 0.0f32);
        for (byte, _) in text.char_indices() {
            while j < run.glyphs.len() && (run.glyphs[j].cluster as usize) < byte {
                pen += run.glyphs[j].advance;
                j += 1;
            }
            let d = ((pen * r).round() / s - x).abs();
            if d < best_d {
                (best, best_d) = (byte, d);
            }
        }
        if ((run.width * r).round() / s - x).abs() < best_d {
            return text.len();
        }
        best
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
        // One caret per character, not per byte: `len()` is bytes, and an
        // over-reservation is the kind of thing `perf_alloc` is watching.
        let mut out = Vec::with_capacity(text.chars().count() + 1);
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

    /// Put a `w` x `h` coverage bitmap in the atlas and return its page and
    /// uv rect, or `None` when it cannot go in this frame. Glyphs and filled
    /// paths both come through here.
    ///
    /// The open page first; then a new page while the budget allows; then the
    /// page drawn from longest ago, emptied — never one drawn from this frame,
    /// because instances already emitted point into it. A bitmap too big for
    /// an ordinary page gets a page of its own the same way.
    fn place(&mut self, w: u32, h: u32, bitmap: &[u8]) -> Option<(u32, [f32; 4])> {
        let big = !(w + 2 <= self.atlas.page_size && h + 2 <= self.atlas.page_size);
        let (page, pos) = if big {
            let side = (w.max(h) + 2).next_power_of_two();
            let p = self.new_page(side)?;
            (p, Rc::make_mut(&mut self.atlas.pages)[p].alloc(w, h)?)
        } else if let Some(pos) = Rc::make_mut(&mut self.atlas.pages)[self.atlas.open].alloc(w, h) {
            (self.atlas.open, pos)
        } else {
            let p = self.new_page(self.atlas.page_size)?;
            self.atlas.open = p;
            (p, Rc::make_mut(&mut self.atlas.pages)[p].alloc(w, h)?)
        };
        self.page_used[page] = self.frame;
        let pg = &mut Rc::make_mut(&mut self.atlas.pages)[page];
        let s = pg.size;
        let data = Rc::make_mut(&mut pg.data);
        for row in 0..h {
            let dst = ((pos.1 + row) * s + pos.0) as usize;
            let src = (row * w) as usize;
            data[dst..dst + w as usize].copy_from_slice(&bitmap[src..src + w as usize]);
        }
        pg.version += 1;
        let sf = s as f32;
        Some((page as u32, [pos.0 as f32 / sf, pos.1 as f32 / sf, (pos.0 + w) as f32 / sf, (pos.1 + h) as f32 / sf]))
    }

    /// An empty page of side `side`: a new one if the budget allows, else the
    /// page drawn from longest ago (of that size) emptied for reuse, else —
    /// every page is in use this frame — `None`, counted, and retried next
    /// frame.
    fn new_page(&mut self, side: u32) -> Option<usize> {
        let a = &self.atlas;
        if a.texels() + side as u64 * side as u64 <= a.budget() {
            Rc::make_mut(&mut self.atlas.pages).push(AtlasPage::new(side));
            self.page_used.push(0);
            return Some(self.atlas.pages.len() - 1);
        }
        let frame = self.frame;
        // Oldest first. A page of a different size is not reused for this one;
        // an oversized page that has gone unused is dropped to make room.
        let used = &self.page_used;
        let mut order: Vec<usize> = (0..a.pages.len()).filter(|&i| used[i] < frame).collect();
        order.sort_by_key(|&i| used[i]);
        if let Some(&i) = order.iter().find(|&&i| self.atlas.pages[i].size == side) {
            self.evict(i);
            return Some(i);
        }
        // No unused page of this size. Release unused pages of other sizes,
        // oldest first, until a new one fits the budget, then add it.
        let mut freed = false;
        for &i in &order {
            if self.atlas.pages[i].size != side && self.atlas.pages[i].size > 0 {
                self.evict(i);
                // A released page shrinks to nothing until its slot is reused;
                // its version keeps rising, so a renderer never mistakes the
                // next page in this slot for the one it uploaded.
                let v = self.atlas.pages[i].version + 1;
                Rc::make_mut(&mut self.atlas.pages)[i] = AtlasPage { version: v, ..AtlasPage::new(0) };
                freed = true;
                if self.atlas.texels() + side as u64 * side as u64 <= self.atlas.budget() {
                    break;
                }
            }
        }
        if freed && self.atlas.texels() + side as u64 * side as u64 <= self.atlas.budget() {
            // Reuse an emptied slot so page numbers stay small.
            if let Some(i) = (0..self.atlas.pages.len()).find(|&i| self.atlas.pages[i].size == 0) {
                let v = self.atlas.pages[i].version + 1;
                Rc::make_mut(&mut self.atlas.pages)[i] = AtlasPage { version: v, ..AtlasPage::new(side) };
                return Some(i);
            }
        }
        self.overflows += 1;
        self.repack_pending = true;
        None
    }

    /// Empty page `i` and forget everything that pointed into it.
    fn evict(&mut self, i: usize) {
        Rc::make_mut(&mut self.atlas.pages)[i].clear();
        self.atlas.repacks += 1;
        self.evictions += 1;
        let p = i as u32;
        self.glyphs.retain(|_, g| g.page != p || g.w == 0.0);
        self.masks.retain(|_, m| m.is_none_or(|(pg, _)| pg != p));
    }

    /// Mark `page` as drawn from this frame.
    fn touch(&mut self, page: u32) {
        if let Some(u) = self.page_used.get_mut(page as usize) {
            *u = self.frame;
        }
    }

    /// A filled path's coverage, from the atlas if it is already there and
    /// rasterised by `raster` into a `w` x `h` buffer if it is not. `key` names
    /// the path *at this size*: the caller hashes both.
    ///
    /// Cached the way glyphs are, and dropped with them when the atlas is
    /// repacked, so a steady frame of icons rasterises nothing.
    pub(crate) fn coverage_mask(&mut self, key: u64, w: u32, h: u32, raster: impl FnOnce(&mut Vec<u8>)) -> Option<(u32, [f32; 4])> {
        if let Some(m) = self.masks.get(&key).copied() {
            if let Some((page, _)) = m {
                self.touch(page);
            }
            return m;
        }
        // The same ceiling glyphs have: bigger than the budget could ever hold
        // is not worth rasterising.
        if w == 0 || h == 0 || !self.atlas.could_fit(w, h) {
            self.masks.insert(key, None);
            return None;
        }
        self.rasterized += 1;
        let mut buf = std::mem::take(&mut self.mask_scratch);
        buf.clear();
        buf.resize((w * h) as usize, 0);
        raster(&mut buf);
        let at = self.place(w, h, &buf);
        self.mask_scratch = buf;
        // Not placed because every page is in use this frame is not cached:
        // the next frame tries again.
        if at.is_some() {
            self.masks.insert(key, at);
        }
        at
    }

    fn glyph(&mut self, font: FontId, face: u16, id: u32, px: f32) -> Glyph {
        let key = (font.0, face, id, px as u32);
        if let Some(g) = self.glyphs.get(&key).copied() {
            if g.w > 0.0 {
                self.touch(g.page);
            }
            return g;
        }
        // A glyph taller than the atlas could never be stored anyway, and
        // asking a rasteriser for one is not merely wasteful: fontdue indexes
        // its coverage buffer with an `i32` and overflows somewhere past
        // 46,000 px, which a zoomed canvas or a theme file can ask for. Skip
        // it before it is rasterised; the glyph is not drawn and its advance
        // still counts, so nothing moves. A NaN is not a size either.
        if px > self.atlas.max_size as f32 || px.is_nan() {
            let g = Glyph { page: 0, uv: [0.0; 4], w: 0.0, h: 0.0, left: 0.0, bottom: 0.0 };
            self.glyphs.insert(key, g);
            return g;
        }
        self.rasterized += 1;
        let m = self.fonts[font.0 as usize].rasterize(face, id, px);
        let bitmap = &m.coverage;
        let (w, h) = (m.width, m.height);
        // A glyph too big for the whole budget is cached as invisible: its
        // advance still counts, so layout stays correct. One that could not
        // be placed only because every page is in use this frame is *not*
        // cached, and is placed next frame.
        let (mut page, mut uv, mut placed) = (0, [0.0; 4], (w, h));
        if w > 0 && h > 0 {
            if !self.atlas.could_fit(w, h) {
                placed = (0, 0);
            } else {
                match self.place(w, h, bitmap) {
                    Some((p, at)) => (page, uv) = (p, at),
                    None => {
                        return Glyph { page: 0, uv, w: 0.0, h: 0.0, left: m.left, bottom: m.bottom };
                    }
                }
            }
        }
        let g = Glyph { page, uv, w: placed.0 as f32, h: placed.1 as f32, left: m.left, bottom: m.bottom };
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
            let g = self.glyph(font, sg.face, sg.glyph, px);
            if g.w > 0.0 {
                let gx = round(x0 + (x + sg.offset.x) * r) + g.left;
                let gy = baseline + round(sg.offset.y * r) - (g.bottom + g.h);
                dl.glyph(Rect::new(gx / s, gy / s, g.w / s, g.h / s), g.page, g.uv, color);
            }
            x += sg.advance;
        }
    }

    /// One line of text centred on `center` and turned by `rot` about it.
    ///
    /// Each glyph is placed where it would sit in the upright line, that
    /// position is turned about the centre, and the glyph is drawn there
    /// turned by the same amount. Nothing is snapped: a turned glyph has no
    /// pixel grid to sit on, and is resampled by the atlas's bilinear filter,
    /// which reads well for labels but is a shade softer than upright text.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rotated(&mut self, dl: &mut DrawList, font: FontId, size: f32, center: Vec2, rot: crate::Rotation, color: Color, text: &str) {
        let s = self.text_scale();
        let px = self.px(size);
        let r = self.fit(size, px);
        let (asc, _) = self.line(font, px);
        let m = self.measure(font, size, text);
        self.text_draws += 1;
        // The upright box's top-left and baseline, in raster px from the centre.
        let x0 = -m.x * 0.5 * s;
        let baseline = -m.y * 0.5 * s + asc * r;
        let mut x = 0.0;
        let run = self.run(font, px, text);
        for sg in run.glyphs.iter() {
            let g = self.glyph(font, sg.face, sg.glyph, px);
            if g.w > 0.0 {
                let gx = x0 + (x + sg.offset.x) * r + g.left;
                let gy = baseline + sg.offset.y * r - (g.bottom + g.h);
                let (w, h) = (g.w / s, g.h / s);
                let at = rot.apply([gx / s + w * 0.5, gy / s + h * 0.5]);
                let c = Vec2::new(center.x + at[0], center.y + at[1]);
                // Half a texel more on every side, into the empty gutter the
                // atlas leaves around each glyph: a turned edge then fades to
                // nothing instead of being cut off mid-texel, and never
                // samples the glyph packed next to this one.
                let (du, dv) = ((g.uv[2] - g.uv[0]) / g.w * 0.5, (g.uv[3] - g.uv[1]) / g.h * 0.5);
                let uv = [g.uv[0] - du, g.uv[1] - dv, g.uv[2] + du, g.uv[3] + dv];
                let (w, h) = (w + 1.0 / s, h + 1.0 / s);
                dl.glyph_rotated(Rect::new(c.x - w * 0.5, c.y - h * 0.5, w, h), g.page, uv, color, rot);
            }
            x += sg.advance;
        }
    }
}

impl Default for FontsInner {
    fn default() -> Self {
        Self::new()
    }
}

/// The font system: the faces, the shaping and wrapping caches, and the glyph
/// atlas they are packed into.
///
/// A **handle**, so it can be shared. Every window in a docked application has
/// its own [`Ui`](crate::Ui), and each used to carry its own atlas: a panel
/// torn into a new window rasterised every glyph again and the host uploaded a
/// second copy of the same image. Give the new `Ui` [`Fonts::share`] of the
/// old one and there is a single atlas, rasterised once and uploaded once.
///
/// Sharing is by `Rc`: a `Ui` is not `Send`, and neither is this.
///
/// ```no_run
/// # use libgui::*;
/// # fn f(theme: Theme, font: &[u8]) -> Result<(), FontError> {
/// let main = Ui::new(theme.clone(), font)?;
/// // A torn-off window, drawing from the same atlas.
/// let panel = Ui::sharing_fonts(theme, &main);
/// # let _ = panel; Ok(()) }
/// ```
#[derive(Clone)]
pub struct Fonts(Rc<RefCell<FontsInner>>);

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Self {
        Fonts(Rc::new(RefCell::new(FontsInner::new())))
    }

    /// Another handle to the same faces, caches and atlas.
    ///
    /// What makes one atlas serve every window. The two are the same font
    /// system: a glyph either rasterises for both or for neither.
    pub fn share(&self) -> Fonts {
        Fonts(Rc::clone(&self.0))
    }

    /// Do these two handles name the same font system?
    pub fn is_shared_with(&self, other: &Fonts) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// The glyph atlas as it stands, to upload.
    ///
    /// A cheap snapshot, not a borrow: the image is behind an `Rc`, so this
    /// copies a handful of numbers. Holding one while text is drawn copies the
    /// image once rather than failing — a host that uploads its frame output
    /// and drops it never causes that.
    pub fn atlas(&self) -> Atlas {
        self.0.borrow().atlas().clone()
    }

    #[cfg(feature = "fontdue")]
    pub fn add_font(&self, bytes: &[u8]) -> Result<FontId, FontError> {
        self.0.borrow_mut().add_font(bytes)
    }

    pub fn add_rasterizer(&self, rasterizer: Box<dyn FontRasterizer>) -> FontId {
        self.0.borrow_mut().add_rasterizer(rasterizer)
    }

    pub fn tab_width(&self) -> usize {
        self.0.borrow().tab_width()
    }

    pub fn set_tab_width(&self, spaces: usize) {
        self.0.borrow_mut().set_tab_width(spaces);
    }

    pub fn set_atlas_limit(&self, max: u32) {
        self.0.borrow_mut().set_atlas_limit(max);
    }

    pub fn take_rasterized(&self) -> u32 {
        self.0.borrow_mut().take_rasterized()
    }

    pub fn take_shaped_runs(&self) -> u32 {
        self.0.borrow_mut().take_shaped_runs()
    }

    pub fn take_text_draws(&self) -> u32 {
        self.0.borrow_mut().take_text_draws()
    }

    pub fn measure(&self, font: FontId, size: f32, text: &str) -> Vec2 {
        self.0.borrow().measure(font, size, text)
    }

    pub fn caret_x(&self, font: FontId, size: f32, text: &str, byte: usize) -> f32 {
        self.0.borrow().caret_x(font, size, text, byte)
    }

    pub fn byte_at_x(&self, font: FontId, size: f32, text: &str, x: f32) -> usize {
        self.0.borrow().byte_at_x(font, size, text, x)
    }

    pub fn carets(&self, font: FontId, size: f32, text: &str) -> Vec<f32> {
        self.0.borrow().carets(font, size, text)
    }

    /// `text` broken into lines no wider than `max`, cached.
    pub(crate) fn wrap(&self, font: FontId, size: f32, text: &str, max: f32) -> Rc<[Line]> {
        self.0.borrow().wrap(font, size, text, max)
    }

    pub fn wrap_lines_for_test(&self, font: FontId, size: f32, text: &str, max: f32) -> Vec<String> {
        self.0.borrow().wrap_lines_for_test(font, size, text, max)
    }

    pub fn measure_wrapped(&self, font: FontId, size: f32, text: &str, max: f32) -> Vec2 {
        self.0.borrow().measure_wrapped(font, size, text, max)
    }

    pub fn min_wrap_width(&self, font: FontId, size: f32, text: &str) -> f32 {
        self.0.borrow().min_wrap_width(font, size, text)
    }

    pub fn line_height(&self, font: FontId, size: f32) -> f32 {
        self.0.borrow().line_height(font, size)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_wrapped(
        &self,
        dl: &mut DrawList,
        font: FontId,
        size: f32,
        r: Rect,
        color: Color,
        align: crate::Align,
        text: &str,
    ) {
        self.0.borrow_mut().draw_wrapped(dl, font, size, r, color, align, text);
    }

    pub fn draw(&self, dl: &mut DrawList, font: FontId, size: f32, pos: Vec2, color: Color, text: &str) {
        self.0.borrow_mut().draw(dl, font, size, pos, color, text);
    }

    /// One line of text centred on `center`, turned by `rot` about it.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rotated(&self, dl: &mut DrawList, font: FontId, size: f32, center: Vec2, rot: crate::Rotation, color: Color, text: &str) {
        self.0.borrow_mut().draw_rotated(dl, font, size, center, rot, color, text);
    }

    // ---- crate-internal ---------------------------------------------------

    pub(crate) fn coverage_mask(&self, key: u64, w: u32, h: u32, raster: impl FnOnce(&mut Vec<u8>)) -> Option<(u32, [f32; 4])> {
        self.0.borrow_mut().coverage_mask(key, w, h, raster)
    }

    pub(crate) fn repack_pending(&self) -> bool {
        self.0.borrow().repack_pending()
    }

    pub(crate) fn begin_frame(&self) {
        self.0.borrow_mut().begin_frame();
    }

    pub(crate) fn take_atlas_counts(&self) -> (u32, u32) {
        self.0.borrow_mut().take_atlas_counts()
    }

    pub(crate) fn set_scale(&self, scale: f32) {
        self.0.borrow_mut().set_scale(scale);
    }

    pub(crate) fn set_zoom(&self, zoom: f32) {
        self.0.borrow_mut().set_zoom(zoom);
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

    fn fonts(scale: f32) -> FontsInner {
        let mut f = FontsInner::new();
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

#[cfg(test)]
mod tab_tests {
    use super::*;

    const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

    fn fonts() -> (FontsInner, FontId) {
        let mut f = FontsInner::new();
        let id = f.add_font(FONT).expect("font");
        (f, id)
    }

    /// A tab is laid out as four spaces and draws nothing.
    ///
    /// Left to the shaper, Inter maps `\t` to a `.notdef` box: preserving a
    /// pasted tab would then show a box where the indentation should be, which
    /// is a worse bug than the space it used to be flattened into.
    #[test]
    fn a_tab_is_four_spaces_wide_and_blank() {
        let (f, id) = fonts();
        let space = f.measure(id, 14.0, " ").x;
        let tab = f.measure(id, 14.0, "\t").x;
        assert!(
            (tab - space * DEFAULT_TAB_WIDTH as f32).abs() <= 1.0,
            "a tab measured {tab}, four spaces measure {}",
            space * DEFAULT_TAB_WIDTH as f32
        );

        // The glyph is the space's, which every font draws as nothing.
        let mut shaped = Vec::new();
        f.fonts[id.0 as usize].shape(" ", 14.0, &mut shaped);
        let blank = shaped[0].glyph;
        let run = f.run(id, 14.0, "\tx");
        assert_eq!(run.glyphs[0].glyph, blank, "the tab kept the font's own tab glyph");
    }

    /// Carets land after the tab's full width, so a click in indented text
    /// picks the character it looks like it picked.
    #[test]
    fn carets_follow_the_tab_advance() {
        let (f, id) = fonts();
        let carets = f.carets(id, 14.0, "\tx");
        let space = f.measure(id, 14.0, " ").x;
        assert!(carets[1] >= space * 3.0, "the caret after a tab sat at {}", carets[1]);
    }
}
