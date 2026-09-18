//! Built-in widgets. Every widget follows the same recipe, so custom widgets
//! (gizmo toolbars, curve editors, node graphs) look exactly like these:
//!   1. `make_id`  2. `interact`  3. `animate` retained visual state
//!   4. copy its style from `self.theme` (so `with_style` scopes work)
//!   5. `add_leaf` with a layout + a paint closure that runs after layout.

use crate::{ButtonStyle, Color, Cursor, Insets, Layout, Painter, Rect, Response, Size, TextureId, Theme, Ui, Vec2};
use std::hash::Hash;

impl Ui {
    fn text_size(&self, size: f32, text: &str) -> Vec2 {
        self.fonts.measure(self.font, size, text)
    }

    /// Restyle everything built inside `body`, then restore the theme:
    /// `ui.with_style(|t| t.button.radius = 0.0, |ui| { ui.button("Square"); })`.
    pub fn with_style<R>(&mut self, edit: impl FnOnce(&mut Theme), body: impl FnOnce(&mut Ui) -> R) -> R {
        let saved = self.theme.clone();
        edit(&mut self.theme);
        let r = body(self);
        self.theme = saved;
        r
    }

    pub fn label(&mut self, text: &str) {
        let c = self.theme.palette.text;
        self.text_with(text, self.theme.metrics.font_size, c);
    }

    pub fn label_muted(&mut self, text: &str) {
        let c = self.theme.palette.text_muted;
        self.text_with(text, self.theme.metrics.font_size, c);
    }

    pub fn heading(&mut self, text: &str) {
        let c = self.theme.palette.text;
        self.text_with(text, self.theme.metrics.font_size_heading, c);
    }

    /// Small, uppercase section caption.
    pub fn section(&mut self, text: &str) {
        let c = self.theme.palette.text_faint;
        let text = text.to_uppercase();
        self.text_with(&text, self.theme.metrics.font_size_small, c);
    }

    pub fn text_with(&mut self, text: &str, size: f32, color: Color) {
        let id = self.make_id(("label", text));
        let m = self.text_size(size, text);
        let text = text.to_string();
        self.add_leaf(id, Layout::leaf(Size::Fit, Size::Fit), m, false, move |p, r| {
            p.text(Vec2::new(r.x, r.y), size, color, &text);
        });
    }

    pub fn space(&mut self, px: f32) {
        let id = self.make_id("space");
        self.add_leaf(id, Layout::leaf(Size::Fixed(px), Size::Fixed(px)), Vec2::ZERO, false, |_, _| {});
    }

    /// Pushes following siblings to the end of the row/column.
    pub fn flex(&mut self) {
        let id = self.make_id("flex");
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, |_, _| {});
    }

    pub fn separator(&mut self) {
        let id = self.make_id("sep");
        let c = self.theme.palette.border;
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(1.0)), Vec2::ZERO, false, move |p, r| p.rect(r, c, 0.0));
    }

    pub fn button(&mut self, label: &str) -> Response {
        let style = self.theme.button;
        self.button_styled(label, &style)
    }

    /// [`Ui::button`] with an identity that does not depend on the label or on
    /// build order. Use it when two buttons in one container share a label, or
    /// when one of them is conditional. See [`Ui::with_key`].
    pub fn button_keyed(&mut self, key: impl Hash, label: &str) -> Response {
        let style = self.theme.button;
        self.button_styled_keyed(key, label, &style)
    }

    pub fn button_primary(&mut self, label: &str) -> Response {
        let style = self.theme.button_primary;
        self.button_styled(label, &style)
    }

    /// Button with an explicit style (e.g. a one-off destructive button).
    pub fn button_styled(&mut self, label: &str, style: &ButtonStyle) -> Response {
        self.button_styled_keyed(label, label, style)
    }

    /// [`Ui::button_styled`] with an explicit key.
    pub fn button_styled_keyed(&mut self, key: impl Hash, label: &str, style: &ButtonStyle) -> Response {
        let s = *style;
        let id = self.make_id(("button", key));
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let down = self.animate_bool(id, 1, resp.active && resp.hovered);
        let label = label.to_string();
        let layout = Layout::leaf(Size::Fit, Size::Fixed(s.height)).padding(Insets::xy(s.padding_x, 0.0));
        self.add_leaf(id, layout, m, true, move |p, r| {
            let shadow = s.shadow.color.with_alpha(s.shadow.color.a * (1.0 - down));
            if shadow.a > 0.0 {
                p.shadow(r.translate(0.0, s.shadow.offset_y), s.radius, s.shadow.blur, shadow);
            }
            p.rect_bordered(r, s.fill.at(hover, down), s.radius, s.border_width, s.border.at(hover, down));
            if s.highlight > 0.0 {
                // 1px top highlight: a hand-finished look without bevels.
                let hl = Rect::new(r.x + s.radius, r.y + s.border_width, r.w - 2.0 * s.radius, 1.0);
                p.rect(hl, Color::WHITE.with_alpha(s.highlight * (1.0 + 0.8 * hover)), 0.0);
            }
            p.text_centered(r.translate(0.0, down * s.press_offset), size, s.text.at(hover, down), &label);
        });
        resp
    }

    /// Inspector-style row: label on the left, animated switch on the right.
    pub fn toggle(&mut self, label: &str, value: &mut bool) -> Response {
        self.toggle_keyed(label, label, value)
    }

    /// [`Ui::toggle`] with an explicit key. See [`Ui::with_key`].
    pub fn toggle_keyed(&mut self, key: impl Hash, label: &str, value: &mut bool) -> Response {
        let s = self.theme.toggle;
        let id = self.make_id(("toggle", key));
        let (size, h) = (self.theme.metrics.font_size, self.theme.metrics.control_height);
        let muted = self.theme.palette.text_muted;
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        if resp.clicked {
            *value = !*value;
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let on = self.animate_bool(id, 0, *value);
        let hover = self.animate_bool(id, 1, resp.hovered);
        let label = label.to_string();
        let content = Vec2::new(m.x + 12.0 + s.width, m.y.max(s.height));
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h.max(s.height))), content, true, move |p, r| {
            p.text_left(r, size, s.label.lerp(muted, 0.25 * (1.0 - hover)), &label);
            let track = Rect::new(r.right() - s.width, r.center().y - s.height * 0.5, s.width, s.height);
            let fill = s.track_off.lerp(s.track_on, on);
            let border = s.border_off.lerp(s.border_on, on);
            p.rect_bordered(track, fill, s.height * 0.5, 1.0, border);
            let d = s.height - 4.0;
            let kx = track.x + 2.0 + on * (s.width - s.height);
            let knob = Rect::new(kx, track.y + 2.0, d, d);
            p.shadow(knob.translate(0.0, 1.0), d * 0.5, 2.0, p.theme.palette.shadow);
            p.rect(knob, s.knob, d * 0.5);
        });
        resp
    }

    /// Labelled horizontal slider. Drag anywhere on it.
    pub fn slider(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> Response {
        self.slider_keyed(label, label, value, min, max)
    }

    /// [`Ui::slider`] with an explicit key. See [`Ui::with_key`].
    pub fn slider_keyed(&mut self, key: impl Hash, label: &str, value: &mut f32, min: f32, max: f32) -> Response {
        let s = self.theme.slider;
        let id = self.make_id(("slider", key));
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact_drag(id);
        let kr0 = s.knob_radius;
        if resp.active && resp.rect.w > 2.0 * kr0 {
            let frac = ((resp.mouse_pos.x - resp.rect.x - kr0) / (resp.rect.w - 2.0 * kr0)).clamp(0.0, 1.0);
            *value = min + frac * (max - min);
        }
        if resp.hovered || resp.active {
            self.cursor = if resp.active { Cursor::Grabbing } else { Cursor::Grab };
        }
        let frac = ((*value - min) / (max - min)).clamp(0.0, 1.0);
        let hover = self.animate_bool(id, 0, resp.hovered || resp.active);
        let drag = self.animate_bool(id, 1, resp.active);
        let label = label.to_string();
        let value_text = format!("{:.2}", *value);
        let h = m.y + 8.0 + 2.0 * kr0;
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::new(m.x + 40.0, h), true, move |p, r| {
            let top = Rect::new(r.x, r.y, r.w, m.y);
            p.text_left(top, size, s.label, &label);
            p.text_right(top, size, s.value.lerp(s.value_active, drag), &value_text);
            let cy = r.bottom() - kr0 - 2.0;
            let th = s.track_height;
            let track = Rect::new(r.x + kr0, cy - th * 0.5, r.w - 2.0 * kr0, th);
            p.rect(track, s.track, th * 0.5);
            let kx = track.x + frac * track.w;
            p.rect(Rect::new(track.x, track.y, kx - track.x, th), s.fill, th * 0.5);
            let ring = kr0 + 5.0 * hover;
            p.rect(Rect::new(kx - ring, cy - ring, ring * 2.0, ring * 2.0), s.ring.with_alpha(s.ring.a * hover), ring);
            let kr = kr0 + drag;
            p.shadow(Rect::new(kx - kr, cy - kr + 1.0, kr * 2.0, kr * 2.0), kr, 2.0, p.theme.palette.shadow);
            p.rect(Rect::new(kx - kr, cy - kr, kr * 2.0, kr * 2.0), s.knob, kr);
        });
        resp
    }

    /// List / tree row with selection highlight.
    pub fn selectable(&mut self, label: &str, selected: bool) -> Response {
        self.selectable_keyed(label, label, selected)
    }

    /// [`Ui::selectable`] with an explicit key: the right call for list and
    /// tree rows, whose labels are often duplicated. See [`Ui::with_key`].
    pub fn selectable_keyed(&mut self, key: impl Hash, label: &str, selected: bool) -> Response {
        let s = self.theme.selectable;
        let id = self.make_id(("selectable", key));
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let sel = self.animate_bool(id, 1, selected);
        let label = label.to_string();
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(s.height)).padding(Insets::xy(s.padding_x, 0.0));
        self.add_leaf(id, layout, m, true, move |p, r| {
            let bg = s.fill_hover.with_alpha(s.fill_hover.a * hover).lerp(s.fill_selected, sel);
            p.rect(r, bg, s.radius);
            if sel > 0.01 && s.indicator_width > 0.0 {
                let bar_h = (r.h * 0.55) * sel;
                let w = s.indicator_width;
                p.rect(Rect::new(r.x + 3.0, r.center().y - bar_h * 0.5, w, bar_h), s.indicator, w * 0.5);
            }
            let fg = s.text.lerp(s.text_hover, hover).lerp(s.text_selected, sel);
            p.text_left(r.shrink(s.padding_x + 4.0 * sel, 0.0, s.padding_x, 0.0), size, fg, &label);
        });
        resp
    }

    /// One-of-N picker (density, tool modes, view modes).
    pub fn segmented(&mut self, key: &str, selected: &mut usize, options: &[&str]) -> Response {
        let s = self.theme.segmented;
        let id = self.make_id(("segmented", key));
        let size = self.theme.metrics.font_size;
        let widths: Vec<f32> = options.iter().map(|o| self.text_size(size, o).x + 24.0).collect();
        let total: f32 = widths.iter().sum();
        let resp = self.interact(id);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        if resp.pressed && resp.rect.w > 0.0 {
            let scale = resp.rect.w / total;
            let mut x = resp.rect.x;
            for (i, w) in widths.iter().enumerate() {
                if resp.mouse_pos.x < x + w * scale {
                    *selected = i;
                    break;
                }
                x += w * scale;
            }
        }
        let idx = (*selected).min(options.len().saturating_sub(1));
        let pos = self.animate(id, 0, idx as f32);
        let options: Vec<String> = options.iter().map(|o| o.to_string()).collect();
        let min_w = total * 0.6;
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(s.height)), Vec2::new(min_w, 0.0), true, move |p, r| {
            p.rect_bordered(r, s.fill, s.radius, 1.0, s.border);
            if widths.is_empty() {
                return;
            }
            let scale = r.w / total;
            // Narrow pane: shrink the label font rather than overlapping.
            let size = if scale < 1.0 { (size * (0.35 + 0.65 * scale)).max(size * 0.75) } else { size };
            let xs: Vec<f32> = widths.iter().scan(r.x, |x, w| {
                let start = *x;
                *x += w * scale;
                Some(start)
            }).collect();
            // Sliding thumb interpolated between segment positions. `pos` is
            // retained across frames, so it can still point past the end after
            // the caller shortens `options`: clamp before indexing.
            let pos = pos.clamp(0.0, (xs.len() - 1) as f32);
            let (i0, t) = (pos.floor() as usize, pos.fract());
            let i1 = (i0 + 1).min(xs.len() - 1);
            let x = xs[i0] + (xs[i1] - xs[i0]) * t;
            let w = widths[i0] * scale + (widths[i1] - widths[i0]) * scale * t;
            let thumb = Rect::new(x + 2.0, r.y + 2.0, w - 4.0, r.h - 4.0);
            p.shadow(thumb.translate(0.0, 1.0), s.radius - 1.0, 2.0, p.theme.palette.shadow.with_alpha(0.25));
            p.rect(thumb, s.fill_selected, (s.radius - 2.0).max(0.0));
            for (i, o) in options.iter().enumerate() {
                let seg = Rect::new(xs[i], r.y, widths[i] * scale, r.h);
                let on = (1.0 - (pos - i as f32).abs()).clamp(0.0, 1.0);
                p.text_centered(seg, size, s.text.lerp(s.text_selected, on), o);
            }
        });
        resp
    }

    /// Bar plot for debug stats (frame times, memory, …).
    pub fn plot(&mut self, label: &str, values: &[f32], max: f32, height: f32) {
        let s = self.theme.plot;
        let radius = self.theme.metrics.radius;
        let id = self.make_id(("plot", label));
        let values = values.to_vec();
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(height)), Vec2::ZERO, false, move |p, r| {
            p.rect_bordered(r, s.fill, radius, 1.0, s.border);
            if values.is_empty() {
                return;
            }
            let inner = r.shrink(4.0, 4.0, 4.0, 4.0);
            let bw = inner.w / values.len() as f32;
            for (k, v) in values.iter().enumerate() {
                let f = (v / max).clamp(0.0, 1.0);
                let bh = (inner.h * f).max(1.0);
                let c = s.bar.lerp(s.bar_high, ((f - 0.5) * 2.0).max(0.0));
                p.rect(Rect::new(inner.x + k as f32 * bw, inner.bottom() - bh, (bw - 1.0).max(1.0), bh), c.with_alpha(0.85), 1.0);
            }
        });
    }

    /// Engine viewport: shows a host-rendered texture and gives you input over it.
    /// `overlay` runs in the immediate paint layer on top of the image
    /// (gizmo labels, stats, selection boxes).
    pub fn viewport(&mut self, key: &str, texture: TextureId, overlay: impl FnOnce(&mut Painter, Rect) + 'static) -> Response {
        let s = self.theme.viewport;
        let id = self.make_id(("viewport", key));
        let resp = self.interact_drag(id);
        if resp.active {
            self.cursor = Cursor::Grabbing;
        }
        let focus = self.animate_bool(id, 0, resp.hovered || resp.active);
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, r| {
            p.image(r, texture, s.radius);
            p.draw.push_clip(r);
            overlay(p, r);
            p.draw.pop_clip();
            p.rect_bordered(r, Color::TRANSPARENT, s.radius, 1.0, s.border.lerp(s.border_hover, focus));
        });
        resp
    }
}
