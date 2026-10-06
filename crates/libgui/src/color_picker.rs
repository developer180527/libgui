//! A colour picker, and a swatch that opens one.
//!
//! Built from nothing a custom widget does not also have: ids, drag
//! interaction, leaves with paint closures, [`Painter::gradient`], and a
//! [`Ui::validated_input`] for the hex field. It is meant to be read as an
//! example as much as used.
//!
//! The one thing it keeps for itself is the **hue and saturation it was
//! showing**. Your colour is RGB, and RGB cannot hold them where they stop
//! mattering: grey has no hue, black has no saturation. Converted fresh each
//! frame, dragging to the bottom of the square would throw the hue away and
//! snap its handle to red. So the picker remembers what it last showed, and
//! only re-derives it when your colour changes from outside — an undo, another
//! panel, a script.
//!
//! HSV is computed on the colour as stored, sRGB-encoded, which is what every
//! common picker does and what the gradients blend in, so the square shows
//! exactly the colours the handle picks.

use crate::{Axis, Color, FocusKind, Layout, Painter, Rect, Response, Size, Ui, ValidatedOptions, Vec2};
use crate::{Cursor, FieldError, Frame};

/// How a [`Ui::color_picker`] looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPickerOptions {
    /// Offer an alpha strip. Off for a colour that has no business being
    /// translucent — a layer colour that is always drawn opaque.
    pub alpha: bool,
    /// Height of the saturation/value square, logical px. Its width is
    /// whatever the picker is given.
    pub square_height: f32,
    /// Show the hex field.
    pub hex: bool,
}

impl Default for ColorPickerOptions {
    fn default() -> Self {
        Self { alpha: true, square_height: 160.0, hex: true }
    }
}

/// What a picker did this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct ColorPickerResponse {
    /// The colour changed this frame — every frame of a drag. For a live
    /// preview.
    pub changed: bool,
    /// An edit finished: a drag was let go, or a hex value committed. The
    /// one to push an undo step on, so a drag is one step and not sixty.
    pub finished: bool,
    /// Something in the picker is being dragged.
    pub dragging: bool,
}

/// Retained per picker.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PickerState {
    h: f32,
    s: f32,
    v: f32,
    /// The colour these were derived from or produced; when the caller's
    /// colour differs, it changed from outside and they are re-derived.
    seen: [f32; 4],
    known: bool,
    dragging: bool,
}

/// HSV of an sRGB-encoded colour: hue in 0..1, saturation and value in 0..1.
/// `None` for the hue where there is none (grey), so the caller can keep its
/// own.
fn to_hsv(c: Color) -> (Option<f32>, f32, f32) {
    let (r, g, b) = (c.r.clamp(0.0, 1.0), c.g.clamp(0.0, 1.0), c.b.clamp(0.0, 1.0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max;
    let s = if max > 0.0 { d / max } else { 0.0 };
    if d <= 0.0 {
        return (None, s, v);
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (Some(h / 6.0), s, v)
}

/// The fully saturated, full-value colour at hue `h` in 0..1.
fn hue_rgb(h: f32) -> [f32; 3] {
    // Piecewise linear between the six primaries and secondaries.
    let x = (h.rem_euclid(1.0)) * 6.0;
    let seg = x.floor() as i32;
    let t = x - seg as f32;
    match seg.rem_euclid(6) {
        0 => [1.0, t, 0.0],
        1 => [1.0 - t, 1.0, 0.0],
        2 => [0.0, 1.0, t],
        3 => [0.0, 1.0 - t, 1.0],
        4 => [t, 0.0, 1.0],
        _ => [1.0, 0.0, 1.0 - t],
    }
}

fn from_hsv(h: f32, s: f32, v: f32, a: f32) -> Color {
    let [r, g, b] = hue_rgb(h);
    // v · lerp(white, hue, s): the square's construction exactly.
    let mix = |c: f32| v * (1.0 - s + s * c);
    Color::rgba(mix(r), mix(g), mix(b), a)
}

/// The six corners of the hue circle, for drawing the strip.
const PRIMARIES: [[f32; 3]; 7] = [
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 1.0, 1.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [1.0, 0.0, 0.0],
];

fn opaque(c: [f32; 3]) -> Color {
    Color::rgba(c[0], c[1], c[2], 1.0)
}

/// A handle that reads on any colour: a dark ring inside a light one.
fn ring(p: &mut Painter, at: Vec2, radius: f32) {
    let r = Rect::new(at.x - radius, at.y - radius, radius * 2.0, radius * 2.0);
    p.shadow(r, radius, 3.0, Color::rgba(0.0, 0.0, 0.0, 0.35));
    p.rect_bordered(r, Color::TRANSPARENT, radius, 2.0, Color::WHITE);
    p.rect_bordered(r.shrink(2.0, 2.0, 2.0, 2.0), Color::TRANSPARENT, radius - 2.0, 1.0, Color::rgba(0.0, 0.0, 0.0, 0.5));
}

/// A vertical bar handle across a strip.
fn bar(p: &mut Painter, r: Rect, x: f32) {
    let b = Rect::new(x - 3.0, r.y - 2.0, 6.0, r.h + 4.0);
    p.shadow(b, 3.0, 3.0, Color::rgba(0.0, 0.0, 0.0, 0.35));
    p.rect_bordered(b, Color::TRANSPARENT, 3.0, 2.0, Color::WHITE);
}

/// The light-and-dark squares translucency is shown over.
fn checker(p: &mut Painter, r: Rect) {
    let cell = (r.h * 0.5).max(3.0);
    p.rect(r, Color::rgba(0.85, 0.85, 0.85, 1.0), 0.0);
    let dark = Color::rgba(0.6, 0.6, 0.6, 1.0);
    let (cols, rows) = ((r.w / cell).ceil() as i32, (r.h / cell).ceil() as i32);
    p.draw.push_clip(r);
    for j in 0..rows {
        for i in 0..cols {
            if (i + j) % 2 == 1 {
                p.rect(Rect::new(r.x + i as f32 * cell, r.y + j as f32 * cell, cell, cell), dark, 0.0);
            }
        }
    }
    p.draw.pop_clip();
}

impl Ui {
    /// A colour picker: a saturation/value square, a hue strip, an alpha
    /// strip, and a hex field, editing `color` in place.
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # fn f(ui: &mut Ui, layer: &mut Color, push_undo: impl FnOnce()) {
    /// if ui.color_picker("layer", layer).finished {
    ///     push_undo();
    /// }
    /// # }
    /// ```
    pub fn color_picker(&mut self, key: &str, color: &mut Color) -> ColorPickerResponse {
        self.color_picker_with(key, color, ColorPickerOptions::default())
    }

    /// [`Ui::color_picker`] with options.
    pub fn color_picker_with(&mut self, key: &str, color: &mut Color, opts: ColorPickerOptions) -> ColorPickerResponse {
        let id = self.make_id(("color_picker", key));
        self.mark_seen(id);
        let mut st = self.picker_states.get(&id).copied().unwrap_or_default();

        // Changed from outside, or never seen: derive HSV from the colour,
        // keeping the hue where grey has none and the saturation where black
        // has none.
        if !st.known || st.seen != color.to_array() {
            let (h, s, v) = to_hsv(*color);
            if let Some(h) = h {
                st.h = h;
            }
            if v > 0.0 {
                st.s = s;
            }
            st.v = v;
            st.known = true;
        }

        let mut out = ColorPickerResponse::default();
        let (sq_id, hue_id, alpha_id) = (id.with("sv"), id.with("hue"), id.with("alpha"));
        let border = self.theme.palette.border;
        let (mut h, mut s, mut v, mut a) = (st.h, st.s, st.v, color.a);
        // From the responses, as any widget would know it: `released` is the
        // frame the press ends, a frame before `active` clears.
        let mut dragging = false;
        let mut released = false;
        // A keyboard step is a whole edit: it changes the colour and finishes.
        let mut keyed = false;

        let gap = self.theme.metrics.space;
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Fit).gap(gap);
        self.container_id(id.with("body"), layout, Frame::none(), |ui| {
            // Saturation across, value down.
            let r = ui.interact_focusable_drag(sq_id, FocusKind::Control);
            dragging |= r.active;
            released |= r.released;
            if r.active && r.rect.w > 0.0 && r.rect.h > 0.0 {
                s = ((r.mouse_pos.x - r.rect.x) / r.rect.w).clamp(0.0, 1.0);
                v = 1.0 - ((r.mouse_pos.y - r.rect.y) / r.rect.h).clamp(0.0, 1.0);
            }
            // The keyboard: left and right are saturation, up and down value,
            // a hundredth a step; Home and End are white and the pure hue.
            let n = ui.take_nudge(r.focused);
            if n.any() {
                (s, v) = if n.home {
                    (0.0, 1.0)
                } else if n.end {
                    (1.0, 1.0)
                } else {
                    ((s + n.x * 0.01).clamp(0.0, 1.0), (v + n.y * 0.01).clamp(0.0, 1.0))
                };
                keyed = true;
            }
            hover_cursor(ui, &r);
            let (hh, ss, vv) = (h, s, v);
            ui.add_leaf(sq_id, Layout::leaf(Size::Grow(1.0), Size::Fixed(opts.square_height)), Vec2::new(120.0, 0.0), true, move |p, r| {
                p.gradient(r, Color::WHITE, opaque(hue_rgb(hh)), Axis::X);
                p.gradient(r, Color::TRANSPARENT, Color::BLACK, Axis::Y);
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
                // Not clipped to the square: at full value — the top edge,
                // where every saturated colour is — half of it would vanish.
                ring(p, Vec2::new(r.x + ss * r.w, r.y + (1.0 - vv) * r.h), 6.0);
            });

            // Hue.
            let r = ui.interact_focusable_drag(hue_id, FocusKind::Control);
            dragging |= r.active;
            released |= r.released;
            if r.active && r.rect.w > 0.0 {
                h = ((r.mouse_pos.x - r.rect.x) / r.rect.w).clamp(0.0, 1.0);
            }
            let n = ui.take_nudge(r.focused);
            if n.any() {
                h = n.apply(h, 1.0 / 360.0, 0.0, 1.0);
                keyed = true;
            }
            hover_cursor(ui, &r);
            let hh = h;
            ui.add_leaf(hue_id, Layout::leaf(Size::Grow(1.0), Size::Fixed(14.0)), Vec2::new(120.0, 0.0), true, move |p, r| {
                let n = PRIMARIES.len() - 1;
                for k in 0..n {
                    // Cut on the pixel grid, so neighbouring segments meet
                    // exactly rather than overlapping a pixel's coverage.
                    let x0 = r.x + r.w * k as f32 / n as f32;
                    let x1 = r.x + r.w * (k + 1) as f32 / n as f32;
                    let seg = p.snap_rect(Rect::new(x0, r.y, x1 - x0, r.h));
                    p.gradient(seg, opaque(PRIMARIES[k]), opaque(PRIMARIES[k + 1]), Axis::X);
                }
                p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
                bar(p, r, r.x + hh * r.w);
            });

            // Alpha.
            if opts.alpha {
                let r = ui.interact_focusable_drag(alpha_id, FocusKind::Control);
                dragging |= r.active;
                released |= r.released;
                if r.active && r.rect.w > 0.0 {
                    a = ((r.mouse_pos.x - r.rect.x) / r.rect.w).clamp(0.0, 1.0);
                }
                let n = ui.take_nudge(r.focused);
                if n.any() {
                    a = n.apply(a, 0.01, 0.0, 1.0);
                    keyed = true;
                }
                hover_cursor(ui, &r);
                let solid = from_hsv(h, s, v, 1.0);
                let aa = a;
                ui.add_leaf(alpha_id, Layout::leaf(Size::Grow(1.0), Size::Fixed(14.0)), Vec2::new(120.0, 0.0), true, move |p, r| {
                    checker(p, r);
                    p.gradient(r, Color::TRANSPARENT, solid, Axis::X);
                    p.rect_bordered(r, Color::TRANSPARENT, 0.0, 1.0, border);
                    bar(p, r, r.x + aa * r.w);
                });
            } else {
                ui.keep_id(alpha_id);
            }
        });

        let next = from_hsv(h, s, v, a);
        if next.to_array() != color.to_array() && (dragging || st.dragging || keyed) {
            *color = next;
            out.changed = true;
        }
        if released || (keyed && out.changed) {
            out.finished = true;
        }
        st.dragging = dragging;
        out.dragging = dragging;
        (st.h, st.s, st.v) = (h, s, v);

        if opts.hex {
            let mut text = color.to_hex();
            let mut parsed = None;
            let field = self.validated_input_with(&format!("{key}#hex"), &mut text, &ValidatedOptions::default(), |t| {
                let t = t.trim();
                let c = Color::parse_hex(t).or_else(|| Color::parse_hex(&format!("#{t}")));
                match c {
                    Some(c) => {
                        parsed = Some(c);
                        Ok(())
                    }
                    None => Err(FieldError::new("a colour is #rgb, #rrggbb or #rrggbbaa")),
                }
            });
            if let Some(c) = parsed {
                // Accepted. Even text that differs only in spelling (`#FFF`
                // for `#ffffff`) is not a change unless the colour is.
                if c.to_array() != color.to_array() {
                    *color = c;
                    out.changed = true;
                }
                out.finished |= field.committed || out.changed;
            }
        }

        st.seen = color.to_array();
        self.picker_states.insert(id, st);
        out
    }

    /// A swatch of `color` that opens a [`Ui::color_picker`] in a popup when
    /// clicked: what an inspector row shows.
    pub fn color_button(&mut self, key: &str, color: &mut Color) -> ColorPickerResponse {
        self.color_button_with(key, color, ColorPickerOptions::default())
    }

    /// [`Ui::color_button`] with options for the picker it opens.
    pub fn color_button_with(&mut self, key: &str, color: &mut Color, opts: ColorPickerOptions) -> ColorPickerResponse {
        let id = self.make_id(("color_button", key));
        let popup = id.with("popup");
        let r = self.interact_focusable(id, FocusKind::Control);
        if r.clicked {
            if self.popup_open(popup) {
                self.close_popup(popup);
            } else {
                self.open_popup(popup, r.rect);
            }
        }
        let hot = self.animate_bool(id, 0, r.hovered);
        let (c, border, hover) = (*color, self.theme.palette.border, self.theme.palette.border_strong);
        let h = self.theme.metrics.control_height;
        let radius = self.theme.metrics.radius;
        self.add_leaf(id, Layout::leaf(Size::Fixed(h * 1.6), Size::Fixed(h)), Vec2::ZERO, true, move |p, r| {
            let inner = r.shrink(3.0, 3.0, 3.0, 3.0);
            if c.a < 1.0 {
                checker(p, inner);
            }
            p.rect(inner, c, 0.0);
            p.rect_bordered(r, Color::TRANSPARENT, radius, 1.0, border.lerp(hover, hot));
        });
        let width = 240.0;
        let key = format!("{key}#picker");
        self.popup(popup, width, |ui| ui.color_picker_with(&key, color, opts)).unwrap_or_default()
    }
}

fn hover_cursor(ui: &mut Ui, r: &Response) {
    // The slider's convention, for the same gesture.
    if r.hovered || r.active {
        ui.cursor = if r.active { Cursor::Grabbing } else { Cursor::Grab };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips() {
        for c in [
            Color::rgba(1.0, 0.0, 0.0, 1.0),
            Color::rgba(0.2, 0.6, 0.9, 1.0),
            Color::rgba(0.95, 0.85, 0.1, 0.5),
            Color::rgba(0.3, 0.3, 0.3, 1.0),
            Color::rgba(0.0, 0.0, 0.0, 1.0),
        ] {
            let (h, s, v) = to_hsv(c);
            let back = from_hsv(h.unwrap_or(0.0), s, v, c.a);
            for (a, b) in c.to_array().iter().zip(back.to_array()) {
                assert!((a - b).abs() < 1e-5, "{c:?} came back as {back:?}");
            }
        }
    }

    #[test]
    fn grey_has_no_hue() {
        assert_eq!(to_hsv(Color::rgba(0.4, 0.4, 0.4, 1.0)).0, None);
        assert!(to_hsv(Color::rgba(0.4, 0.5, 0.4, 1.0)).0.is_some());
    }

    #[test]
    fn the_hue_strip_is_the_six_primaries() {
        for (k, p) in PRIMARIES.iter().enumerate() {
            let c = hue_rgb(k as f32 / 6.0);
            assert_eq!(&c, p, "hue {k}/6");
        }
    }
}
