//! Level meters: an audio channel, a CPU load, a tank, a signal strength.
//!
//! A bar from the bottom of a range to a value, lit in zones — ordinary,
//! warning, over — with the highest recent value held as a tick that falls
//! back after a moment, and a light that latches when the value goes off the
//! top of the scale until someone clicks it. What the numbers mean (dB, %,
//! litres) is the app's; the meter only places them on its range.
//!
//! Its state is the held peak and the latched light, kept by libgui under the
//! meter's id, so the app passes only this frame's value. Holding and falling
//! are timed by the real elapsed time ([`crate::FrameInfo::dt`]), and a meter
//! at rest asks for no frames: a held peak wakes the window once, when it is
//! due to fall.

use crate::{Axis, Layout, Rect, Response, Size, Ui, Vec2};

/// How a [`Ui::meter`] reads and draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterOptions {
    /// The bottom and top of the scale, in the app's units.
    pub range: (f32, f32),
    /// Where the warning zone and the over zone start, in the same units.
    /// `None`: one colour, the theme's `low`.
    pub zones: Option<(f32, f32)>,
    /// [`Axis::X`] fills left to right; [`Axis::Y`] bottom to top.
    pub axis: Axis,
    /// Along the bar. `Size::Grow` to fill what the container gives.
    pub length: Size,
    /// Across the bar, logical px.
    pub thickness: f32,
    /// Seconds a peak is held before it falls. Zero: no peak tick.
    pub hold: f32,
    /// How fast a held peak falls once its time is up, in fractions of the
    /// range per second.
    pub fall: f32,
    /// Draw as this many separate cells, an LED ladder; zero for a solid bar.
    pub cells: u32,
    /// A tick across the bar every this many units. `None`: no ticks.
    pub tick_step: Option<f32>,
    /// Show the over-range light at the top of the scale.
    pub clip_light: bool,
}

impl Default for MeterOptions {
    fn default() -> Self {
        Self {
            range: (0.0, 1.0),
            zones: None,
            axis: Axis::X,
            length: Size::Grow(1.0),
            thickness: 8.0,
            hold: 1.5,
            fall: 0.5,
            cells: 0,
            tick_step: None,
            clip_light: false,
        }
    }
}

impl MeterOptions {
    /// An audio channel in dBFS: −60 to 0, warning from −18, over from −6,
    /// vertical, an LED ladder, a clip light.
    pub fn audio_db() -> Self {
        Self {
            range: (-60.0, 0.0),
            zones: Some((-18.0, -6.0)),
            axis: Axis::Y,
            length: Size::Fixed(160.0),
            thickness: 10.0,
            cells: 30,
            tick_step: Some(6.0),
            clip_light: true,
            ..Self::default()
        }
    }
}

/// What a [`Ui::meter`] reports.
#[derive(Clone, Copy, Debug)]
pub struct MeterResponse {
    pub response: Response,
    /// The held peak, in the app's units.
    pub peak: f32,
    /// The value has reached the top of the scale since the light was last
    /// cleared. A click on the meter clears it and the peak.
    pub clipped: bool,
}

/// Slots under the meter's id.
const PEAK: u8 = 0;
const HELD: u8 = 1;
const CLIP: u8 = 2;

impl Ui {
    /// A level meter showing `value` on `opts.range`.
    ///
    /// ```ignore
    /// let opts = MeterOptions::audio_db();
    /// ui.row(|ui| {
    ///     ui.meter("L", left_db, &opts);
    ///     ui.meter("R", right_db, &opts);
    /// });
    /// ```
    pub fn meter(&mut self, key: &str, value: f32, opts: &MeterOptions) -> MeterResponse {
        self.meter_impl(key, value, None, opts)
    }

    /// [`Ui::meter`] showing an average under the instantaneous value — RMS
    /// under peak, load over the last second under load now. The average is
    /// the solid bar; the value reaches past it, dimmer.
    pub fn meter_with_average(&mut self, key: &str, value: f32, average: f32, opts: &MeterOptions) -> MeterResponse {
        self.meter_impl(key, value, Some(average), opts)
    }

    fn meter_impl(&mut self, key: &str, value: f32, average: Option<f32>, opts: &MeterOptions) -> MeterResponse {
        let s = self.theme.meter;
        let id = self.make_id(("meter", key));
        let response = self.interact(id);
        let (lo, hi) = crate::scope::sane_range(opts.range);
        let v = if value.is_finite() { value } else { lo };

        // The held peak: up at once, held, then falling.
        let dt = self.elapsed;
        let (mut peak, mut held) = match (self.anim_get(id, PEAK), self.anim_get(id, HELD)) {
            (Some(p), Some(h)) => (p, h),
            _ => (v, 0.0),
        };
        let mut clipped = self.anim_get(id, CLIP).unwrap_or(0.0) > 0.5;
        if response.clicked {
            peak = v;
            held = 0.0;
            clipped = false;
        }
        if v >= peak {
            peak = v;
            held = 0.0;
        } else {
            held += dt;
            if held > opts.hold {
                peak = (peak - opts.fall * (hi - lo) * dt.min(held - opts.hold)).max(v);
            }
        }
        if v >= hi {
            clipped = true;
        }
        if opts.hold > 0.0 && peak > v {
            if held < opts.hold {
                self.request_repaint_in(opts.hold - held);
            } else {
                self.request_repaint();
            }
        }
        self.set_anim(id, PEAK, peak);
        self.set_anim(id, HELD, held);
        self.set_anim(id, CLIP, clipped as u8 as f32);

        let vertical = opts.axis == Axis::Y;
        let layout = if vertical {
            Layout::leaf(Size::Fixed(opts.thickness), opts.length)
        } else {
            Layout::leaf(opts.length, Size::Fixed(opts.thickness))
        };
        let opts = *opts;
        let show_peak = opts.hold > 0.0 && peak > v;
        self.add_leaf(id, layout, Vec2::ZERO, true, move |p, r| {
            // The clip light takes the far end; the bar has the rest.
            let light = opts.clip_light.then_some(if vertical { r.w } else { r.h });
            let gap = 2.0;
            let bar = match light {
                Some(l) if vertical => Rect::new(r.x, r.y + l + gap, r.w, r.h - l - gap),
                Some(l) => Rect::new(r.x, r.y, r.w - l - gap, r.h),
                None => r,
            };
            if let Some(l) = light {
                let lr = if vertical { Rect::new(r.x, r.y, r.w, l) } else { Rect::new(r.right() - l, r.y, l, r.h) };
                let c = if clipped { s.clip } else { s.clip.with_alpha(s.clip.a * 0.18) };
                p.rect(p.snap_rect(lr), c, s.radius);
            }
            // Fraction of the bar at a value, and the rect from a to b.
            let f = |x: f32| ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
            let span = |a: f32, b: f32| -> Rect {
                if vertical {
                    Rect::new(bar.x, bar.bottom() - bar.h * b, bar.w, bar.h * (b - a))
                } else {
                    Rect::new(bar.x + bar.w * a, bar.y, bar.w * (b - a), bar.h)
                }
            };
            let zone_color = |x: f32| match opts.zones {
                Some((_, over)) if x >= over => s.high,
                Some((warn, _)) if x >= warn => s.mid,
                _ => s.low,
            };
            p.rect(bar, s.track, s.radius);
            let solid = f(average.unwrap_or(v));
            let reach = f(v);
            if opts.cells > 0 {
                let n = opts.cells;
                let cell_gap = 1.0;
                for i in 0..n {
                    let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
                    let mid = lo + (hi - lo) * (a + b) * 0.5;
                    let c = zone_color(mid);
                    let lit = if (a + b) * 0.5 <= solid {
                        c
                    } else if (a + b) * 0.5 <= reach {
                        c.with_alpha(c.a * 0.55)
                    } else {
                        c.with_alpha(c.a * 0.12)
                    };
                    let cell = span(a, b);
                    let cell = if vertical {
                        Rect::new(cell.x, cell.y + cell_gap * 0.5, cell.w, (cell.h - cell_gap).max(0.5))
                    } else {
                        Rect::new(cell.x + cell_gap * 0.5, cell.y, (cell.w - cell_gap).max(0.5), cell.h)
                    };
                    p.rect(p.snap_rect(cell), lit, 0.0);
                }
            } else {
                // Lit in zone colours up to the value; past the average dimmer.
                let mut edges = vec![0.0];
                if let Some((warn, over)) = opts.zones {
                    edges.push(f(warn));
                    edges.push(f(over));
                }
                edges.push(1.0);
                for w in edges.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let c = zone_color(lo + (hi - lo) * (a + b) * 0.5);
                    if solid > a {
                        p.rect(p.snap_rect(span(a, solid.min(b))), c, 0.0);
                    }
                    if reach > solid.max(a) {
                        p.rect(p.snap_rect(span(solid.max(a), reach.min(b))), c.with_alpha(c.a * 0.55), 0.0);
                    }
                }
            }
            if let Some(step) = opts.tick_step.filter(|s| *s > 0.0 && (hi - lo) / *s <= 200.0) {
                let mut t = (lo / step).ceil() * step;
                while t <= hi {
                    let k = f(t);
                    if k > 0.0 && k < 1.0 {
                        let tr = if vertical {
                            Rect::new(bar.x, bar.bottom() - bar.h * k, bar.w, 0.0)
                        } else {
                            Rect::new(bar.x + bar.w * k, bar.y, 0.0, bar.h)
                        };
                        let px = 1.0 / p.scale.max(0.01);
                        let tr = if vertical { Rect::new(tr.x, tr.y, tr.w * 0.35, px) } else { Rect::new(tr.x, tr.y, px, tr.h * 0.35) };
                        p.rect(p.snap_rect(tr), s.tick, 0.0);
                    }
                    t += step;
                }
            }
            if show_peak {
                let k = f(peak);
                let px = 2.0 / p.scale.max(0.01);
                let pr = if vertical {
                    Rect::new(bar.x, bar.bottom() - bar.h * k - px * 0.5, bar.w, px)
                } else {
                    Rect::new(bar.x + bar.w * k - px * 0.5, bar.y, px, bar.h)
                };
                p.rect(p.snap_rect(pr), s.peak, 0.0);
            }
        });
        MeterResponse { response, peak, clipped }
    }
}
