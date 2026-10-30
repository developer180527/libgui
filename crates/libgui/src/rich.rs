//! Rich text: one paragraph in several faces, sizes and colours, wrapped as
//! one. What a Markdown paragraph, a chat message with a bold name, or a
//! label with a link in it needs, and what [`crate::TextSpan`] (one face, many
//! colours) cannot do.
//!
//! The runs are joined and broken by the same rules as a plain paragraph
//! (`wrap`), so "**bold**text" does not break between its halves, then each
//! word is cut back into its runs to be measured and drawn in its own face.
//! A line's runs share a baseline, so a larger or a code run sits on the line
//! rather than hanging from its top.
//!
//! Each run is shaped by itself: no kerning or ligature crosses a run, and a
//! right-to-left word is reordered within its run, not across runs. For a
//! paragraph that is mostly one direction with a few styled words, which is
//! what rich text in a UI is, that is invisible.

use crate::{Align, Color, FontId, Fonts, Painter, Rect, Vec2};
use std::rc::Rc;

/// One stretch of rich text in one style.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub font: FontId,
    pub size: f32,
    pub color: Color,
    /// Drawn behind the run's line box, as inline code is; transparent for
    /// none.
    pub background: Color,
    pub underline: bool,
    pub strike: bool,
    /// The app's number for the run, reported when it is hovered or clicked
    /// (a link's index); 0 for none, and a run with tag 0 is not interactive.
    pub tag: u32,
}

impl TextRun {
    pub fn new(text: impl Into<String>, font: FontId, size: f32, color: Color) -> Self {
        Self { text: text.into(), font, size, color, background: Color::TRANSPARENT, underline: false, strike: false, tag: 0 }
    }
}

/// A paragraph of [`TextRun`]s. Build it once and share it (`Rc`) with
/// [`crate::Ui::rich_text`]; it is laid out to whatever width it is given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichText {
    pub runs: Vec<TextRun>,
}

impl RichText {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, run: TextRun) -> &mut Self {
        if !run.text.is_empty() {
            self.runs.push(run);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.is_empty())
    }

    /// All the text, without its styling.
    pub fn plain(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// A piece of one run on one line, placed. Positions are from the
/// paragraph's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichPiece {
    pub run: u32,
    /// Bytes of the run's text.
    pub start: u32,
    pub end: u32,
    /// Where the piece's text is drawn (its own line box's top-left) and how
    /// wide it is.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    /// The line it is on: its top and height, for backgrounds and hits.
    pub line_top: f32,
    pub line_h: f32,
    /// From `y` to the baseline, for underlines.
    pub ascent: f32,
}

/// A [`RichText`] laid out to a width.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichLayout {
    pub pieces: Vec<RichPiece>,
    pub size: Vec2,
}

impl RichLayout {
    /// The tag of the run under `p` (from the paragraph's top-left), or 0.
    pub fn tag_at(&self, rich: &RichText, p: Vec2) -> u32 {
        self.pieces
            .iter()
            .find(|q| p.x >= q.x && p.x < q.x + q.w && p.y >= q.line_top && p.y < q.line_top + q.line_h)
            .map_or(0, |q| rich.runs[q.run as usize].tag)
    }
}

/// The widest unbreakable stretch: the narrowest the paragraph can be laid
/// out without cutting a word.
pub fn min_width(fonts: &Fonts, rich: &RichText) -> f32 {
    let (full, starts) = join(rich);
    let mut ops = Vec::new();
    crate::wrap::opportunities(&full, &mut ops);
    let mut best = 0.0f32;
    let mut from = 0;
    for (trim, at) in ops.iter().map(|o| (o.trim, o.at)).chain(std::iter::once((full.len(), full.len()))) {
        best = best.max(width(fonts, rich, &starts, from, trim));
        from = at;
    }
    best
}

/// Lay `rich` out in lines no wider than `max`, aligned within it.
pub fn layout(fonts: &Fonts, rich: &RichText, max: f32, align: Align) -> RichLayout {
    let (full, starts) = join(rich);
    let mut ops = Vec::new();
    crate::wrap::opportunities(&full, &mut ops);
    let max = max.max(1.0);

    // Words: the text up to a break (drawn), then what the break consumes
    // (whitespace, measured but not drawn), and whether it is a newline.
    let mut words: Vec<(usize, usize, usize, bool)> = Vec::new();
    let mut from = 0;
    for o in &ops {
        words.push((from, o.trim, o.at, full.as_bytes().get(o.trim) == Some(&b'\n')));
        from = o.at;
    }
    words.push((from, full.len(), full.len(), false));

    let mut out = RichLayout::default();
    let mut lines: Vec<(usize, f32)> = Vec::new(); // (first piece, visible width)
    let mut line_start = 0usize;
    let mut x = 0.0f32;
    let mut visible = 0.0f32;
    let place = |out: &mut RichLayout, a: usize, b: usize, x: &mut f32| {
        for (run, ra, rb) in split(&starts, a, b) {
            let r = &rich.runs[run];
            let w = fonts.measure(r.font, r.size, &r.text[ra..rb]).x;
            out.pieces.push(RichPiece { run: run as u32, start: ra as u32, end: rb as u32, x: *x, y: 0.0, w, line_top: 0.0, line_h: 0.0, ascent: 0.0 });
            *x += w;
        }
    };
    for &(a, trim, at, hard) in &words {
        let w = width(fonts, rich, &starts, a, trim);
        if out.pieces.len() > line_start && x + w > max {
            lines.push((line_start, visible));
            line_start = out.pieces.len();
            x = 0.0;
        }
        if w > max && out.pieces.len() == line_start {
            // A word wider than the line: cut it between characters.
            let mut cut = a;
            let mut cw = 0.0;
            for (i, c) in full[a..trim].char_indices() {
                let p = a + i;
                let cwid = width(fonts, rich, &starts, p, p + c.len_utf8());
                if cw + cwid > max && p > cut {
                    place(&mut out, cut, p, &mut x);
                    lines.push((line_start, x));
                    line_start = out.pieces.len();
                    x = 0.0;
                    cut = p;
                    cw = 0.0;
                }
                cw += cwid;
            }
            place(&mut out, cut, trim, &mut x);
        } else {
            place(&mut out, a, trim, &mut x);
        }
        visible = x;
        if hard {
            lines.push((line_start, visible));
            line_start = out.pieces.len();
            x = 0.0;
            visible = 0.0;
        } else {
            x += width(fonts, rich, &starts, trim, at);
        }
    }
    lines.push((line_start, visible));

    // Vertical: each line as tall as its tallest run, on one baseline.
    let mut top = 0.0f32;
    let mut widest = 0.0f32;
    for (k, &(first, vis)) in lines.iter().enumerate() {
        let end = lines.get(k + 1).map_or(out.pieces.len(), |l| l.0);
        let ps = &mut out.pieces[first..end];
        let metrics = |p: &RichPiece| {
            let r = &rich.runs[p.run as usize];
            (fonts.ascent(r.font, r.size), fonts.line_height(r.font, r.size))
        };
        let (mut asc, mut h) = (0.0f32, 0.0f32);
        for p in ps.iter() {
            let (a, lh) = metrics(p);
            asc = asc.max(a);
            h = h.max(lh);
        }
        if ps.is_empty() {
            // An empty line (two newlines): as tall as the text around it.
            let r = rich.runs.last();
            h = r.map_or(0.0, |r| fonts.line_height(r.font, r.size));
        }
        // The deepest descent below the shared baseline sets the height.
        for p in ps.iter() {
            let (a, lh) = metrics(p);
            h = h.max(asc - a + lh);
        }
        let shift = match align {
            Align::Start => 0.0,
            Align::Center => ((max - vis) * 0.5).max(0.0),
            Align::End => (max - vis).max(0.0),
        };
        for p in ps.iter_mut() {
            let (a, _) = metrics(p);
            p.ascent = a;
            p.y = top + asc - a;
            p.x += shift;
            p.line_top = top;
            p.line_h = h;
        }
        widest = widest.max(vis);
        top += h;
    }
    out.size = Vec2::new(widest, top);
    out
}

/// The runs' text joined, and where each run starts in it.
fn join(rich: &RichText) -> (String, Vec<usize>) {
    let mut full = String::with_capacity(rich.runs.iter().map(|r| r.text.len()).sum());
    let mut starts = Vec::with_capacity(rich.runs.len() + 1);
    for r in &rich.runs {
        starts.push(full.len());
        full.push_str(&r.text);
    }
    starts.push(full.len());
    (full, starts)
}

/// `a..b` of the joined text as (run, start, end) in each run's own bytes.
fn split(starts: &[usize], a: usize, b: usize) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    (0..starts.len() - 1).filter_map(move |i| {
        let (s, e) = (starts[i], starts[i + 1]);
        let (lo, hi) = (a.max(s), b.min(e));
        (lo < hi).then(|| (i, lo - s, hi - s))
    })
}

fn width(fonts: &Fonts, rich: &RichText, starts: &[usize], a: usize, b: usize) -> f32 {
    split(starts, a, b).map(|(run, ra, rb)| {
        let r = &rich.runs[run];
        fonts.measure(r.font, r.size, &r.text[ra..rb]).x
    }).sum()
}

impl Painter<'_> {
    /// Draw `rich`, laid out as `layout`, its top-left at `origin`. `hot` is a
    /// tag to draw as hovered (its colour lightened), or 0.
    pub fn rich(&mut self, origin: Vec2, rich: &RichText, layout: &RichLayout, hot: u32) {
        // Backgrounds first, one per run per line: inline code of two words
        // is one box, not two with a gap at the space.
        let mut k = 0;
        while k < layout.pieces.len() {
            let p = layout.pieces[k];
            let r = &rich.runs[p.run as usize];
            let mut right = p.x + p.w;
            let mut j = k + 1;
            while j < layout.pieces.len() && layout.pieces[j].run == p.run && layout.pieces[j].line_top == p.line_top {
                right = layout.pieces[j].x + layout.pieces[j].w;
                j += 1;
            }
            if r.background.a > 0.0 {
                let pad = (r.size * 0.2).round();
                let x = origin.x + p.x;
                self.rect(Rect::new(x - pad, origin.y + p.line_top + 1.0, right - p.x + pad * 2.0, p.line_h - 2.0), r.background, 3.0);
            }
            k = j;
        }
        let s = self.scale.max(0.01);
        // A horizontal rule `t` thick at `y`, on the pixel grid.
        let rule = |x: f32, y: f32, w: f32, t: f32| Rect::new(x, (y * s).round() / s, w, (t * s).round().max(1.0) / s);
        for (k, p) in layout.pieces.iter().enumerate() {
            let r = &rich.runs[p.run as usize];
            let (x, y) = (origin.x + p.x, origin.y + p.y);
            let color = if hot != 0 && r.tag == hot { r.color.lerp(Color::WHITE, 0.25) } else { r.color };
            let text = &r.text[p.start as usize..p.end as usize];
            self.fonts.draw(self.draw, r.font, r.size, Vec2::new(x, y), color, text);
            if r.underline || r.strike {
                // Through the space to the next piece of the same run, so a
                // link or a strike of two words is one line.
                let next = layout.pieces.get(k + 1).filter(|n| n.run == p.run && n.line_top == p.line_top);
                let w = next.map_or(p.w, |n| n.x - p.x);
                let t = (r.size / 14.0).max(1.0);
                if r.underline {
                    let u = rule(x, y + p.ascent + t * 1.5, w, t);
                    self.rect(u, color, 0.0);
                }
                if r.strike {
                    let u = rule(x, y + p.ascent * 0.68, w, t);
                    self.rect(u, color, 0.0);
                }
            }
        }
    }
}

/// What [`crate::Ui::rich_text`] reports: the widget, and the tagged run the
/// pointer is over and the one clicked this frame (0 for none).
#[derive(Clone, Copy, Debug, Default)]
pub struct RichResponse {
    pub response: crate::Response,
    pub hovered_tag: u32,
    pub clicked_tag: u32,
}

impl crate::Ui {
    /// A wrapped paragraph of [`RichText`]: bold, italic and code words, links,
    /// in the faces of [`crate::FontRole`]. Grows to the width it is given,
    /// as [`crate::Ui::paragraph`] does. A run with a nonzero `tag` is
    /// clickable, and the response says which was hovered or clicked.
    pub fn rich_text(&mut self, key: impl std::hash::Hash, rich: Rc<RichText>) -> RichResponse {
        self.rich_text_with(key, rich, Align::Start)
    }

    /// [`crate::Ui::rich_text`] aligned within its width.
    pub fn rich_text_with(&mut self, key: impl std::hash::Hash, rich: Rc<RichText>, align: Align) -> RichResponse {
        let id = self.make_id(("rich_text", key));
        let tagged = rich.runs.iter().any(|r| r.tag != 0);
        let response = if tagged { self.interact(id) } else { crate::Response { id, ..Default::default() } };
        let last = self.rect_of(id).filter(|r| r.w > 0.0);
        let mut hovered_tag = 0;
        let h = match last {
            Some(r) => {
                let l = layout(&self.fonts, &rich, r.w, align);
                if tagged && response.hovered {
                    hovered_tag = l.tag_at(&rich, response.mouse_pos - Vec2::new(r.x, r.y));
                }
                l.size.y
            }
            None => rich.runs.first().map_or(0.0, |r| self.fonts.line_height(r.font, r.size)),
        };
        if hovered_tag != 0 {
            self.cursor = crate::Cursor::Pointer;
        }
        let clicked_tag = if response.clicked { hovered_tag } else { 0 };
        let min_w = min_width(&self.fonts, &rich);
        let node = self.nodes.len() as u32;
        self.wrapping.push((node, crate::ui::Wrap::Rich(rich.clone(), align)));
        let paint_rich = rich;
        self.add_leaf(id, crate::Layout::leaf(crate::Size::Grow(1.0), crate::Size::Fit), Vec2::new(min_w, h), tagged, move |p, r| {
            let l = layout(p.fonts, &paint_rich, r.w, align);
            p.rich(Vec2::new(r.x, r.y), &paint_rich, &l, hovered_tag);
        });
        RichResponse { response, hovered_tag, clicked_tag }
    }
}
