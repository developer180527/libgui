//! Built-in widgets. Every widget follows the same recipe, so custom widgets
//! (gizmo toolbars, curve editors, node graphs) look exactly like these:
//!   1. `make_id`  2. `interact`  3. `animate` retained visual state
//!   4. `add_leaf` with a layout + a paint closure that runs after layout.

use crate::{Color, Cursor, Insets, Layout, Painter, Rect, Response, Size, TextureId, Ui, Vec2};

impl Ui {
    fn text_size(&self, size: f32, text: &str) -> Vec2 {
        self.fonts.measure(self.font, size, text)
    }

    pub fn label(&mut self, text: &str) {
        let c = self.theme.text;
        self.text_with(text, self.theme.font_size, c);
    }

    pub fn label_muted(&mut self, text: &str) {
        let c = self.theme.text_muted;
        self.text_with(text, self.theme.font_size, c);
    }

    pub fn heading(&mut self, text: &str) {
        let c = self.theme.text;
        self.text_with(text, self.theme.font_size_heading, c);
    }

    /// Small, uppercase section caption.
    pub fn section(&mut self, text: &str) {
        let c = self.theme.text_faint;
        let text = text.to_uppercase();
        self.text_with(&text, self.theme.font_size_small, c);
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
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(1.0)), Vec2::ZERO, false, |p, r| {
            let c = p.theme.border;
            p.rect(r, c, 0.0);
        });
    }

    pub fn button(&mut self, label: &str) -> Response {
        self.button_impl(label, false)
    }

    pub fn button_primary(&mut self, label: &str) -> Response {
        self.button_impl(label, true)
    }

    fn button_impl(&mut self, label: &str, primary: bool) -> Response {
        let id = self.make_id(("button", label));
        let (size, h, pad) = (self.theme.font_size, self.theme.control_height, self.theme.space * 1.75);
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let down = self.animate_bool(id, 1, resp.active && resp.hovered);
        let label = label.to_string();
        let layout = Layout::leaf(Size::Fit, Size::Fixed(h)).padding(Insets::xy(pad, 0.0));
        self.add_leaf(id, layout, m, true, move |p, r| {
            let t = p.theme;
            let (base, hov, act, fg) = if primary {
                (t.accent, t.accent_hover, t.accent_active, t.text_on_accent)
            } else {
                (t.surface, t.surface_hover, t.surface_active, t.text)
            };
            let fill = base.lerp(hov, hover).lerp(act, down);
            let border = if primary { fill.lerp(Color::WHITE, 0.12) } else { t.border.lerp(t.border_strong, hover) };
            let shadow = t.shadow.with_alpha(0.45 * (1.0 - down));
            let radius = t.radius;
            p.shadow(r.translate(0.0, 1.5), radius, 3.0, shadow);
            p.rect_bordered(r, fill, radius, 1.0, border);
            // Subtle top highlight gives a hand-finished look without bevels.
            p.rect(Rect::new(r.x + radius, r.y + 1.0, r.w - 2.0 * radius, 1.0), Color::WHITE.with_alpha(0.05 + 0.04 * hover), 0.0);
            p.text_centered(r.translate(0.0, down * 0.5), size, fg, &label);
        });
        resp
    }

    /// Inspector-style row: label on the left, animated switch on the right.
    pub fn toggle(&mut self, label: &str, value: &mut bool) -> Response {
        let id = self.make_id(("toggle", label));
        let (size, h) = (self.theme.font_size, self.theme.control_height);
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
        let content = Vec2::new(m.x + 12.0 + 34.0, m.y);
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), content, true, move |p, r| {
            let t = p.theme;
            p.text_left(r, size, t.text.lerp(t.text_muted, 0.25 * (1.0 - hover)), &label);
            let track = Rect::new(r.right() - 34.0, r.center().y - 9.0, 34.0, 18.0);
            let fill = t.bg_inset.lerp(t.accent, on);
            let border = t.border_strong.lerp(t.accent_hover, on).lerp(t.text_faint, hover * (1.0 - on) * 0.5);
            p.rect_bordered(track, fill, 9.0, 1.0, border);
            let kx = track.x + 2.0 + on * 16.0;
            let knob = Rect::new(kx, track.y + 2.0, 14.0, 14.0);
            p.shadow(knob.translate(0.0, 1.0), 7.0, 2.0, t.shadow);
            p.rect(knob, Color::hex(0xf2f4f8), 7.0);
        });
        resp
    }

    /// Labelled horizontal slider. Drag anywhere on it.
    pub fn slider(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> Response {
        let id = self.make_id(("slider", label));
        let size = self.theme.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        let knob_r = 7.0;
        if resp.active && resp.rect.w > 2.0 * knob_r {
            let frac = ((resp.mouse_pos.x - resp.rect.x - knob_r) / (resp.rect.w - 2.0 * knob_r)).clamp(0.0, 1.0);
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
        let h = m.y + 22.0;
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::new(m.x + 40.0, h), true, move |p, r| {
            let t = p.theme;
            let top = Rect::new(r.x, r.y, r.w, m.y);
            p.text_left(top, size, t.text_muted, &label);
            p.text_right(top, size, t.text.lerp(t.accent_hover, drag), &value_text);
            let cy = r.bottom() - 9.0;
            let track = Rect::new(r.x + knob_r, cy - 2.0, r.w - 2.0 * knob_r, 4.0);
            p.rect(track, t.bg_inset, 2.0);
            let kx = track.x + frac * track.w;
            p.rect(Rect::new(track.x, track.y, kx - track.x, track.h), t.accent, 2.0);
            let ring = 7.0 + 5.0 * hover;
            p.rect(Rect::new(kx - ring, cy - ring, ring * 2.0, ring * 2.0), t.accent.with_alpha(0.18 * hover), ring);
            let kr = knob_r + drag;
            p.shadow(Rect::new(kx - kr, cy - kr + 1.0, kr * 2.0, kr * 2.0), kr, 2.0, t.shadow);
            p.rect(Rect::new(kx - kr, cy - kr, kr * 2.0, kr * 2.0), Color::hex(0xf2f4f8), kr);
        });
        resp
    }

    /// List / tree row with selection highlight.
    pub fn selectable(&mut self, label: &str, selected: bool) -> Response {
        let id = self.make_id(("selectable", label));
        let (size, h, pad) = (self.theme.font_size, self.theme.row_height, self.theme.space * 1.25);
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let sel = self.animate_bool(id, 1, selected);
        let label = label.to_string();
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(h)).padding(Insets::xy(pad, 0.0));
        self.add_leaf(id, layout, m, true, move |p, r| {
            let t = p.theme;
            let bg = t.surface.with_alpha(0.6 * hover).lerp(t.accent.with_alpha(0.16), sel);
            p.rect(r, bg, t.radius);
            if sel > 0.01 {
                let bar_h = (r.h - 12.0) * sel;
                p.rect(Rect::new(r.x + 3.0, r.center().y - bar_h * 0.5, 3.0, bar_h), t.accent, 1.5);
            }
            let fg = t.text_muted.lerp(t.text, hover.max(sel));
            p.text_left(r.shrink(pad + 4.0 * sel, 0.0, pad, 0.0), size, fg, &label);
        });
        resp
    }

    /// Bar plot for debug stats (frame times, memory, …).
    pub fn plot(&mut self, label: &str, values: &[f32], max: f32, height: f32) {
        let id = self.make_id(("plot", label));
        let values = values.to_vec();
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(height)), Vec2::ZERO, false, move |p, r| {
            let t = p.theme;
            p.rect_bordered(r, t.bg_inset, t.radius, 1.0, t.border);
            if values.is_empty() {
                return;
            }
            let inner = r.shrink(4.0, 4.0, 4.0, 4.0);
            let bw = inner.w / values.len() as f32;
            for (k, v) in values.iter().enumerate() {
                let f = (v / max).clamp(0.0, 1.0);
                let bh = (inner.h * f).max(1.0);
                let c = t.accent.lerp(Color::hex(0xf59e0b), ((f - 0.5) * 2.0).max(0.0));
                p.rect(Rect::new(inner.x + k as f32 * bw, inner.bottom() - bh, (bw - 1.0).max(1.0), bh), c.with_alpha(0.85), 1.0);
            }
        });
    }

    /// Engine viewport: shows a host-rendered texture and gives you input over it.
    /// `overlay` runs in the immediate paint layer on top of the image
    /// (gizmo labels, stats, selection boxes).
    pub fn viewport(&mut self, key: &str, texture: TextureId, overlay: impl FnOnce(&mut Painter, Rect) + 'static) -> Response {
        let id = self.make_id(("viewport", key));
        let resp = self.interact(id);
        if resp.active {
            self.cursor = Cursor::Grabbing;
        }
        let focus = self.animate_bool(id, 0, resp.hovered || resp.active);
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, r| {
            let t = p.theme;
            let radius = t.radius_large;
            p.image(r, texture, radius);
            p.draw.push_clip(r);
            overlay(p, r);
            p.draw.pop_clip();
            let border = t.border.lerp(t.border_strong, focus);
            p.rect_bordered(r, Color::TRANSPARENT, radius, 1.0, border);
        });
        resp
    }
}
