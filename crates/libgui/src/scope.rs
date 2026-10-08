//! Traces and scopes: a waveform, a sensor history, a frame-time graph.
//!
//! **The cost follows the screen, not the data.** A trace of a million
//! samples across 600 pixels is drawn as 600 vertical strokes, one per
//! physical pixel column, each spanning the lowest to the highest value that
//! column covers — the min/max envelope every oscilloscope and audio editor
//! draws. Nothing is lost by it: a one-sample spike still lights its column.
//! A trace with fewer samples than columns is drawn as the line through them.
//!
//! [`Painter::trace`] and [`Painter::trace_fill`] draw one in a custom widget;
//! [`Ui::scope`] is the widget: a framed grid, several traces, automatic or
//! fixed range, and a readout under the pointer.

use crate::{Color, Layout, Painter, Rect, Response, Size, Ui, Vec2};

/// Samples to draw, oldest first. Borrowed: nothing is copied to make one.
///
/// A ring buffer is drawn in place with [`Trace::ring`] — the newest sample
/// written at `head - 1`, the oldest at `head` — so a streaming scope keeps
/// one fixed buffer and never shifts it.
#[derive(Clone, Copy, Debug)]
pub struct Trace<'a> {
    samples: &'a [f32],
    start: usize,
}

impl<'a> Trace<'a> {
    pub fn new(samples: &'a [f32]) -> Self {
        Self { samples, start: 0 }
    }

    /// A ring buffer whose oldest sample is at `start` (the next one to be
    /// overwritten).
    pub fn ring(samples: &'a [f32], start: usize) -> Self {
        let start = if samples.is_empty() { 0 } else { start % samples.len() };
        Self { samples, start }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Sample `i`, counting from the oldest.
    pub fn get(&self, i: usize) -> f32 {
        let n = self.samples.len();
        let k = self.start + i;
        self.samples[if k >= n { k - n } else { k }]
    }

    /// The lowest and highest finite sample, or `None` if there is none.
    pub fn bounds(&self) -> Option<(f32, f32)> {
        let mut out: Option<(f32, f32)> = None;
        for &v in self.samples {
            if v.is_finite() {
                out = Some(out.map_or((v, v), |(a, b)| (a.min(v), b.max(v))));
            }
        }
        out
    }
}

impl<'a> From<&'a [f32]> for Trace<'a> {
    fn from(s: &'a [f32]) -> Self {
        Trace::new(s)
    }
}

/// One `[min, max]` per column, in value units: the trace as `cols` columns
/// see it. A column with no finite value is `[NaN, NaN]`, a gap.
///
/// Sample `i` sits at `i / (n - 1)` across the width. Each column takes every
/// sample inside it and the trace's value at both of its edges, so
/// neighbouring columns share an edge value and the strokes join up.
pub(crate) fn envelope(t: Trace, cols: usize, out: &mut Vec<[f32; 2]>) {
    out.clear();
    let n = t.len();
    if n == 0 || cols == 0 {
        return;
    }
    if n == 1 {
        out.resize(cols, [t.get(0), t.get(0)]);
        return;
    }
    let last = (n - 1) as f64;
    // The trace at a fraction `u` of the way across, interpolated.
    let at = |u: f64| -> f32 {
        let x = u * last;
        let i = (x.floor() as usize).min(n - 2);
        let f = (x - i as f64) as f32;
        let (a, b) = (t.get(i), t.get(i + 1));
        if f <= 0.0 {
            a
        } else if f >= 1.0 {
            b
        } else {
            a + (b - a) * f
        }
    };
    let mut i = 0usize;
    for c in 0..cols {
        let (u0, u1) = (c as f64 / cols as f64, (c + 1) as f64 / cols as f64);
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut take = |v: f32| {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        };
        take(at(u0));
        take(at(u1));
        let end = u1 * last;
        while i < n && (i as f64) <= end {
            if (i as f64) >= u0 * last {
                take(t.get(i));
            }
            i += 1;
        }
        // The column's right edge is the next one's left: step back so the
        // sample sitting on it is seen by both.
        i = i.saturating_sub(1);
        out.push(if lo <= hi { [lo, hi] } else { [f32::NAN, f32::NAN] });
    }
}

/// `(lo, hi)` made usable: ordered, finite and not empty.
pub(crate) fn sane_range((lo, hi): (f32, f32)) -> (f32, f32) {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if !(lo.is_finite() && hi.is_finite()) {
        return (0.0, 1.0);
    }
    if hi - lo < 1e-12 {
        (lo - 0.5, hi + 0.5)
    } else {
        (lo, hi)
    }
}

/// Value to y in `r`, clamped to it: a value off the scale rails at the edge,
/// as a real scope's trace does.
fn y_of(r: Rect, (lo, hi): (f32, f32), v: f32) -> f32 {
    r.bottom() - ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * r.h
}

impl Painter<'_> {
    /// How many physical pixel columns `r` covers here, through any canvas
    /// zoom: what a trace decimates to.
    pub fn columns(&self, r: Rect) -> usize {
        (r.w * self.scale * self.draw.xform().zoom).round().max(1.0) as usize
    }

    /// `t` drawn across `r`, `range.0` at the bottom and `range.1` at the top,
    /// as a line `width` wide. Values off the scale rail at the edge;
    /// non-finite samples leave a gap.
    ///
    /// Up to one sample per pixel column it is the line through the samples;
    /// past that, one vertical stroke per column from the lowest to the
    /// highest value in it. Either way the instances drawn are bounded by the
    /// width of `r`, not by the number of samples.
    pub fn trace(&mut self, r: Rect, t: Trace, range: (f32, f32), width: f32, color: Color) {
        let range = sane_range(range);
        let cols = self.columns(r);
        if t.len() <= cols {
            self.trace_points(r, t, range, width, color);
        } else {
            let mut env = Vec::with_capacity(cols);
            envelope(t, cols, &mut env);
            self.trace_envelope(r, &env, range, width, color);
        }
    }

    /// The area between `t` and `baseline` across `r`, filled: under a level
    /// history, inside a waveform. One crisp rect per pixel column.
    pub fn trace_fill(&mut self, r: Rect, t: Trace, range: (f32, f32), baseline: f32, color: Color) {
        let range = sane_range(range);
        let cols = self.columns(r);
        if t.len() <= cols {
            self.fill_points(r, t, range, baseline, color);
            return;
        }
        let mut env = Vec::new();
        envelope(t, cols, &mut env);
        self.fill_envelope(r, &env, range, baseline, color);
    }

    /// The area between the line through the samples and the baseline, as
    /// triangles: smooth along the line, where columns would step. Each span
    /// between two samples is a trapezoid, cut in two where the line crosses
    /// the baseline. The line and the baseline are the outline; the cuts
    /// between spans are inner edges, so the fill has no seams.
    pub(crate) fn fill_points(&mut self, r: Rect, t: Trace, range: (f32, f32), baseline: f32, color: Color) {
        let n = t.len();
        if n < 2 {
            return;
        }
        let x_of = |i: usize| r.x + r.w * i as f32 / (n - 1) as f32;
        let yb = y_of(r, range, baseline);
        for i in 0..n - 1 {
            let (v0, v1) = (t.get(i), t.get(i + 1));
            if !(v0.is_finite() && v1.is_finite()) {
                continue;
            }
            // A side is outline where the fill starts or stops: the ends, and
            // either side of a gap.
            let first = i == 0 || !t.get(i - 1).is_finite();
            let last = i + 2 == n || !t.get(i + 2).is_finite();
            let (x0, x1) = (x_of(i), x_of(i + 1));
            let (y0, y1) = (y_of(r, range, v0), y_of(r, range, v1));
            let (d0, d1) = (y0 - yb, y1 - yb);
            if d0 == 0.0 && d1 == 0.0 {
                continue;
            }
            if (d0 > 0.0 && d1 < 0.0) || (d0 < 0.0 && d1 > 0.0) {
                // Crosses the baseline: a triangle each side of the crossing.
                let xc = x0 + (x1 - x0) * (d0 / (d0 - d1));
                let (p0, c, p1) = (Vec2::new(x0, y0), Vec2::new(xc, yb), Vec2::new(x1, y1));
                // p0→c on the line (outline), c→(x0,yb) on the baseline
                // (outline), (x0,yb)→p0 the left side.
                self.draw.triangle(p0, c, Vec2::new(x0, yb), 1 | 2 | ((first as u8) << 2), color);
                self.draw.triangle(c, p1, Vec2::new(x1, yb), 1 | ((last as u8) << 1) | 4, color);
            } else {
                let (p0, p1, b1, b0) = (Vec2::new(x0, y0), Vec2::new(x1, y1), Vec2::new(x1, yb), Vec2::new(x0, yb));
                // p0→p1 the line, p1→b1 the right side, b1→p0 the diagonal.
                self.draw.triangle(p0, p1, b1, 1 | ((last as u8) << 1), color);
                // p0→b1 the diagonal, b1→b0 the baseline, b0→p0 the left side.
                self.draw.triangle(p0, b1, b0, 2 | ((first as u8) << 2), color);
            }
        }
    }

    pub(crate) fn trace_points(&mut self, r: Rect, t: Trace, range: (f32, f32), width: f32, color: Color) {
        let n = t.len();
        let x_of = |i: usize| if n > 1 { r.x + r.w * i as f32 / (n - 1) as f32 } else { r.x + r.w * 0.5 };
        let mut prev: Option<Vec2> = None;
        for i in 0..n {
            let v = t.get(i);
            if !v.is_finite() {
                prev = None;
                continue;
            }
            let p = Vec2::new(x_of(i), y_of(r, range, v));
            match prev {
                Some(q) => self.draw.line(q, p, width, color),
                // A lone finite sample between gaps is still a dot.
                None if i + 1 >= n || !t.get(i + 1).is_finite() => self.draw.line(p, p, width, color),
                None => {}
            }
            prev = Some(p);
        }
    }

    pub(crate) fn trace_envelope(&mut self, r: Rect, env: &[[f32; 2]], range: (f32, f32), width: f32, color: Color) {
        let cols = env.len().max(1) as f32;
        let w = r.w / cols;
        for (c, &[lo, hi]) in env.iter().enumerate() {
            if !lo.is_finite() {
                continue;
            }
            let x = r.x + (c as f32 + 0.5) * w;
            self.draw.line(Vec2::new(x, y_of(r, range, hi)), Vec2::new(x, y_of(r, range, lo)), width, color);
        }
    }

    pub(crate) fn fill_envelope(&mut self, r: Rect, env: &[[f32; 2]], range: (f32, f32), baseline: f32, color: Color) {
        let cols = env.len().max(1) as f32;
        let w = r.w / cols;
        let base = y_of(r, range, baseline);
        for (c, &[lo, hi]) in env.iter().enumerate() {
            if !lo.is_finite() {
                continue;
            }
            // Everything the column's values reach on either side of the
            // baseline, and the baseline itself.
            let top = y_of(r, range, hi).min(base);
            let bottom = y_of(r, range, lo).max(base);
            if bottom - top <= 0.0 {
                continue;
            }
            self.draw.rect(Rect::new(r.x + c as f32 * w, top, w, bottom - top), color, 0.0, 0.0, Color::TRANSPARENT);
        }
    }
}

/// One trace in a [`Ui::scope`].
#[derive(Clone, Copy, Debug)]
pub struct ScopeTrace<'a> {
    pub trace: Trace<'a>,
    /// `None` takes the theme's next trace colour.
    pub color: Option<Color>,
    pub width: f32,
    /// Fill between the trace and the scope's baseline.
    pub fill: bool,
    /// Shown in the readout.
    pub label: Option<&'a str>,
}

impl<'a> ScopeTrace<'a> {
    pub fn new(trace: impl Into<Trace<'a>>) -> Self {
        Self { trace: trace.into(), color: None, width: 1.5, fill: false, label: None }
    }
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
    pub fn filled(mut self) -> Self {
        self.fill = true;
        self
    }
    pub fn label(mut self, l: &'a str) -> Self {
        self.label = Some(l);
        self
    }
}

/// How [`Ui::scope`] draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScopeOptions {
    pub height: f32,
    /// `None` fits the traces' own values, with a little room.
    pub range: Option<(f32, f32)>,
    /// Grid divisions across and up. Zero for none.
    pub grid: (u32, u32),
    /// What fills reach to; clamped into range. Zero by default.
    pub baseline: f32,
    /// Show each trace's value under the pointer.
    pub readout: bool,
}

impl Default for ScopeOptions {
    fn default() -> Self {
        Self { height: 120.0, range: None, grid: (10, 4), baseline: 0.0, readout: true }
    }
}

/// What a [`Ui::scope`] reports.
#[derive(Clone, Copy, Debug)]
pub struct ScopeResponse {
    pub response: Response,
    /// The range drawn: the one given, or the one fitted.
    pub range: (f32, f32),
    /// Where the pointer is across the scope, 0 at the oldest sample and 1 at
    /// the newest, while it is over it.
    pub at: Option<f32>,
}

impl ScopeResponse {
    /// The sample under the pointer, for a trace of `len` samples.
    pub fn index(&self, len: usize) -> Option<usize> {
        let at = self.at?;
        (len > 0).then(|| ((at * (len - 1) as f32).round() as usize).min(len - 1))
    }
}

/// A trace as the paint closure carries it: the samples themselves when there
/// are few, the column envelope when there are many. Either is about a screen
/// width of data, however long the trace.
enum Shape {
    Points(Vec<f32>),
    Envelope(Vec<[f32; 2]>),
}

/// Digits after the point that tell values `span` apart, without a logarithm
/// (whose last bit differs between platforms, and would move pixels).
pub(crate) fn digits_for(span: f32) -> usize {
    let mut d = 0;
    let mut step = 100.0f32;
    while d < 6 && span < step {
        d += 1;
        step /= 10.0;
    }
    d
}

impl Ui {
    /// A scope: `traces` drawn over a grid, newest sample at the right.
    ///
    /// ```ignore
    /// let r = ui.scope("frame time", &[ScopeTrace::new(Trace::ring(&ms, head)).filled().label("ms")],
    ///     &ScopeOptions { range: Some((0.0, 33.3)), ..Default::default() });
    /// ```
    ///
    /// The samples are reduced to one min/max pair per pixel column while the
    /// frame is built, so a trace of any length costs about a screen width of
    /// work and memory to draw. Hold the data in a ring buffer and pass
    /// [`Trace::ring`]: nothing is shifted or copied.
    pub fn scope(&mut self, key: &str, traces: &[ScopeTrace], opts: &ScopeOptions) -> ScopeResponse {
        let s = self.theme.scope;
        let id = self.make_id(("scope", key));
        let response = self.interact(id);
        let range = sane_range(opts.range.unwrap_or_else(|| {
            let b = traces.iter().filter_map(|t| t.trace.bounds()).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)));
            let (lo, hi) = b.unwrap_or((0.0, 1.0));
            let pad = ((hi - lo) * 0.08).max(1e-6);
            (lo - pad, hi + pad)
        }));
        // Columns by last frame's width: this frame's is not known until
        // layout, and a change of a few pixels only stretches one frame.
        let cols = self.rect_of(id).map_or(512, |r| ((r.w - 2.0).max(1.0) * self.input.scale).round().max(1.0) as usize);
        let palette = [s.trace, s.trace_2, s.trace_3];
        let shapes: Vec<(Shape, Color, f32, bool)> = traces
            .iter()
            .enumerate()
            .map(|(k, t)| {
                let shape = if t.trace.len() <= cols {
                    Shape::Points((0..t.trace.len()).map(|i| t.trace.get(i)).collect())
                } else {
                    let mut env = Vec::with_capacity(cols);
                    envelope(t.trace, cols, &mut env);
                    Shape::Envelope(env)
                };
                (shape, t.color.unwrap_or(palette[k % 3]), t.width, t.fill)
            })
            .collect();

        let at = response.hovered.then(|| {
            let r = response.rect;
            ((response.mouse_pos.x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0)
        });
        // The readout: each trace's value under the pointer, in its colour.
        let readout: Vec<(String, Color)> = match at {
            Some(at) if opts.readout => {
                let digits = digits_for(range.1 - range.0);
                traces
                    .iter()
                    .zip(&shapes)
                    .filter(|(t, _)| !t.trace.is_empty())
                    .map(|(t, (_, c, _, _))| {
                        let n = t.trace.len();
                        let v = t.trace.get(((at * (n - 1) as f32).round() as usize).min(n - 1));
                        let text = match t.label {
                            Some(l) => format!("{l} {v:.digits$}"),
                            None => format!("{v:.digits$}"),
                        };
                        (text, *c)
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let (grid, baseline) = (opts.grid, opts.baseline);
        let size = self.theme.metrics.font_size * 0.85;
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(opts.height)), Vec2::ZERO, true, move |p, r| {
            p.rect_bordered(r, s.fill, s.radius, 1.0, s.border);
            // Inside the border by whole physical pixels: a clip edge on a
            // pixel centre (1 logical px at 1.5x) is a tie that renderers
            // break differently, and a column of trace comes and goes.
            let inset = p.scale.max(0.01).ceil() / p.scale.max(0.01);
            let plot = r.shrink(inset, inset, inset, inset);
            p.draw.push_clip(plot);
            for i in 1..grid.0 {
                let x = plot.x + plot.w * i as f32 / grid.0 as f32;
                let h = p.hairline(x, plot.y, 1.0, plot.h);
                p.rect(h, s.grid, 0.0);
            }
            for j in 1..grid.1 {
                let y = plot.y + plot.h * j as f32 / grid.1 as f32;
                let px = p.scale.max(0.01);
                p.rect(Rect::new(plot.x, (y * px).round() / px, plot.w, 1.0 / px), s.grid, 0.0);
            }
            if range.0 < 0.0 && range.1 > 0.0 {
                let y = y_of(plot, range, 0.0);
                let px = p.scale.max(0.01);
                p.rect(Rect::new(plot.x, (y * px).round() / px, plot.w, 1.0 / px), s.axis, 0.0);
            }
            for (shape, color, width, fill) in &shapes {
                match shape {
                    Shape::Points(v) => {
                        let t = Trace::new(v);
                        if *fill {
                            p.fill_points(plot, t, range, baseline, color.with_alpha(color.a * 0.25));
                        }
                        p.trace_points(plot, t, range, *width, *color);
                    }
                    Shape::Envelope(env) => {
                        if *fill {
                            p.fill_envelope(plot, env, range, baseline, color.with_alpha(color.a * 0.25));
                        }
                        p.trace_envelope(plot, env, range, *width, *color);
                    }
                }
            }
            if let Some(at) = at {
                let x = plot.x + plot.w * at;
                let h = p.hairline(x, plot.y, 1.0, plot.h);
                p.rect(h, s.cursor.with_alpha(s.cursor.a * 0.6), 0.0);
                let mut y = plot.y + 4.0;
                for (text, c) in &readout {
                    let m = p.measure(size, text.as_str());
                    // Beside the cursor, on whichever side has room.
                    let tx = if x + 6.0 + m.x < plot.right() { x + 6.0 } else { x - 6.0 - m.x };
                    p.rect(Rect::new(tx - 3.0, y - 1.0, m.x + 6.0, m.y + 2.0), s.fill.with_alpha(0.85), 3.0);
                    p.text(Vec2::new(tx, y), size, *c, text.as_str());
                    y += m.y + 4.0;
                }
            }
            p.draw.pop_clip();
        });
        ScopeResponse { response, range, at }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample in a million still lights its column: decimation keeps the
    /// extremes, which is the whole reason to draw an envelope rather than
    /// every Nth sample.
    #[test]
    fn a_single_spike_survives_decimation() {
        let mut s = vec![0.0f32; 1_000_000];
        s[500_123] = 1.0;
        s[999_999] = -2.0;
        let mut env = Vec::new();
        envelope(Trace::new(&s), 600, &mut env);
        assert_eq!(env.len(), 600);
        assert!(env.iter().any(|c| c[1] == 1.0), "the spike was decimated away");
        assert_eq!(env[599][0], -2.0, "the last sample is not in the last column");
        assert_eq!(env.iter().filter(|c| c[1] == 1.0).count(), 1, "the spike spread over several columns");
    }

    /// Neighbouring columns share an edge value, so the strokes join: no
    /// column's range is disjoint from the next one's.
    #[test]
    fn columns_join_up() {
        let s: Vec<f32> = (0..10_000).map(|i| ((i as f32) * 0.0031).sin()).collect();
        let mut env = Vec::new();
        envelope(Trace::new(&s), 300, &mut env);
        for w in env.windows(2) {
            assert!(w[0][1] >= w[1][0] && w[1][1] >= w[0][0], "a gap between columns: {:?} then {:?}", w[0], w[1]);
        }
    }

    /// Non-finite samples are gaps, not zeros; a column of nothing but gaps is
    /// a gap.
    #[test]
    fn non_finite_samples_are_gaps() {
        let mut s = vec![1.0f32; 1000];
        for v in &mut s[400..600] {
            *v = f32::NAN;
        }
        let mut env = Vec::new();
        envelope(Trace::new(&s), 10, &mut env);
        assert!(env[5][0].is_nan(), "a column of gaps drew something: {:?}", env[5]);
        assert!(env.iter().filter(|c| c[0].is_finite()).all(|c| *c == [1.0, 1.0]), "a gap pulled a column to some other value");
    }

    #[test]
    fn a_ring_reads_oldest_first() {
        let buf = [5.0, 6.0, 1.0, 2.0, 3.0, 4.0];
        let t = Trace::ring(&buf, 2);
        assert_eq!((0..6).map(|i| t.get(i)).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(Trace::ring(&buf, 8).get(0), 1.0, "a start past the end did not wrap");
        assert_eq!(Trace::ring(&[], 3).len(), 0);
    }

    #[test]
    fn digits_follow_the_span() {
        assert_eq!(digits_for(1000.0), 0);
        assert_eq!(digits_for(33.3), 1);
        assert_eq!(digits_for(2.0), 2);
        assert_eq!(digits_for(0.01), 4);
    }
}
