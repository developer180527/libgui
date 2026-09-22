//! Built-in widgets. Every widget follows the same recipe, so custom widgets
//! (gizmo toolbars, curve editors, node graphs) look exactly like these:
//!   1. `make_id`  2. `interact`  3. `animate` retained visual state
//!   4. copy its style from `self.theme` (so `with_style` scopes work)
//!   5. `add_leaf` with a layout + a paint closure that runs after layout.

use crate::{
    Align, Axis, ButtonStyle, Chevron, Color, Cursor, FocusKind, Id, Insets, Layout, Painter, Rect, Response, Size, TextureId, Theme, Ui,
    Vec2,
};
use std::hash::Hash;

/// Whether a tree row can be expanded, and whether it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Branch {
    /// No children: no arrow, nothing to toggle.
    #[default]
    Leaf,
    Collapsed,
    Expanded,
}

/// Result of [`Ui::tree_row`].
#[derive(Clone, Copy, Debug, Default)]
pub struct TreeResponse {
    pub response: Response,
    /// The disclosure arrow was clicked. Mutually exclusive with
    /// `response.clicked`, so a toggle never also selects.
    pub toggled: bool,
}


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

    /// Text that wraps to the width it is given, and grows downwards.
    ///
    /// Its height follows from its width, which layout only knows after it has
    /// run — so a frame containing one solves twice. That is paid once, when a
    /// paragraph's width changes; a steady frame costs a walk of the
    /// paragraphs and nothing more.
    ///
    /// Its *minimum* width is its longest unbreakable word, not its full
    /// length: a paragraph never forces the panel around it wider. The
    /// consequence is that a paragraph inside a `Size::Fit` container collapses
    /// to that longest word, because a `Fit` container asks its children how
    /// wide they want to be and a paragraph has no answer. Give the container
    /// a width.
    pub fn paragraph(&mut self, text: &str) {
        let (size, color) = (self.theme.metrics.font_size, self.theme.palette.text);
        self.paragraph_with(text, size, color, Align::Start);
    }

    /// [`Ui::paragraph`] with an explicit size, colour and alignment.
    pub fn paragraph_with(&mut self, text: &str, size: f32, color: Color, align: Align) {
        let id = self.make_id(("paragraph", text));
        // Last frame's width, so the first frame of a stable paragraph is
        // already right and the second solve has nothing to correct.
        let last = self.rect_of(id).map(|r| r.w).filter(|w| *w > 0.0);
        let min_w = self.fonts.min_wrap_width(self.font, size, text);
        let h = match last {
            Some(w) => self.fonts.measure_wrapped(self.font, size, text, w).y,
            None => self.fonts.line_height(self.font, size),
        };
        let ft = self.frame_text(text);
        let node = self.nodes.len() as u32;
        self.wrapping.push((node, ft, size));
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fit);
        self.add_leaf(id, layout, Vec2::new(min_w, h), false, move |p, r| {
            p.text_wrapped(r, size, color, align, ft);
        });
    }

    pub fn text_with(&mut self, text: &str, size: f32, color: Color) {
        let id = self.make_id(("label", text));
        let m = self.text_size(size, text);
        let text = self.frame_text(text);
        self.add_leaf(id, Layout::leaf(Size::Fit, Size::Fit), m, false, move |p, r| {
            p.text(Vec2::new(r.x, r.y), size, color, text);
        });
    }

    /// A 2px accent line along the leading (or trailing) edge of `over`'s rect:
    /// the "it goes here" marker for a reorderable list, a tab strip, or a
    /// timeline. Built as an absolute leaf inside the current container, so a
    /// scroll area clips it like any other content.
    pub fn insertion_line(&mut self, over: Id, axis: Axis, after: bool) {
        let Some(r) = self.rect_of(over) else { return };
        let w = 2.0;
        let rect = match (axis, after) {
            (Axis::Y, false) => Rect::new(r.x, r.y - w * 0.5, r.w, w),
            (Axis::Y, true) => Rect::new(r.x, r.bottom() - w * 0.5, r.w, w),
            (Axis::X, false) => Rect::new(r.x - w * 0.5, r.y, w, r.h),
            (Axis::X, true) => Rect::new(r.right() - w * 0.5, r.y, w, r.h),
        };
        let color = self.theme.palette.accent;
        let id = self.make_id(("insertion_line", over));
        self.add_leaf_at(id, rect, crate::LeafOptions::default(), move |p, r| p.rect(r, color, w * 0.5));
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
        let resp = self.interact_focusable(id, FocusKind::Control);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let down = self.animate_bool(id, 1, resp.active && resp.hovered);
        let label = self.frame_text(label);
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
            p.text_centered(r.translate(0.0, down * s.press_offset), size, s.text.at(hover, down), label);
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
        let resp = self.interact_focusable(id, FocusKind::Control);
        if resp.clicked {
            *value = !*value;
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let on = self.animate_bool(id, 0, *value);
        let hover = self.animate_bool(id, 1, resp.hovered);
        let label = self.frame_text(label);
        let content = Vec2::new(m.x + 12.0 + s.width, m.y.max(s.height));
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h.max(s.height))), content, true, move |p, r| {
            p.text_left(r, size, s.label.lerp(muted, 0.25 * (1.0 - hover)), label);
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

    /// A box you tick. Unlike [`Ui::toggle`], which is an inspector row with the
    /// switch pushed to the right, this sits inline with its label.
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> Response {
        self.checkbox_keyed(label, label, value)
    }

    /// [`Ui::checkbox`] with an explicit key. See [`Ui::with_key`].
    pub fn checkbox_keyed(&mut self, key: impl Hash, label: &str, value: &mut bool) -> Response {
        let s = self.theme.toggle;
        let sel = self.theme.selectable;
        let id = self.make_id(("checkbox", key));
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact_focusable(id, FocusKind::Control);
        if resp.clicked {
            *value = !*value;
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let on = self.animate_bool(id, 0, *value);
        let hover = self.animate_bool(id, 1, resp.hovered);
        let box_side = (size + 4.0).round();
        let gap = 8.0;
        let label = self.frame_text(label);
        let h = self.theme.metrics.control_height.max(box_side);
        let content = Vec2::new(box_side + gap + m.x, m.y.max(box_side));
        let layout = Layout::leaf(Size::Fit, Size::Fixed(h));
        self.add_leaf(id, layout, content, true, move |p, r| {
            let b = Rect::new(r.x, r.center().y - box_side * 0.5, box_side, box_side);
            let fill = s.track_off.lerp(s.track_on, on);
            let border = s.border_off.lerp(s.border_on, on).lerp(s.knob, hover * 0.3);
            p.rect_bordered(b, fill, sel.radius * 0.8, 1.0, border);
            if on > 0.01 {
                // A tick drawn as two strokes, scaled in as it turns on.
                let c = b.center();
                let k = box_side * 0.5 * on;
                let w = (box_side * 0.14).max(1.2);
                let a = Vec2::new(c.x - k * 0.55, c.y + k * 0.02);
                let bend = Vec2::new(c.x - k * 0.15, c.y + k * 0.45);
                let e = Vec2::new(c.x + k * 0.6, c.y - k * 0.5);
                p.line(a, bend, w, s.knob);
                p.line(bend, e, w, s.knob);
            }
            p.text_left(r.shrink(box_side + gap, 0.0, 0.0, 0.0), size, sel.text_hover, label);
        });
        resp
    }

    /// One of a set. `value` is set to `choice` when it is clicked.
    pub fn radio<T: PartialEq + Copy>(&mut self, label: &str, value: &mut T, choice: T) -> Response {
        let s = self.theme.toggle;
        let sel = self.theme.selectable;
        let id = self.make_id(("radio", label));
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact_focusable(id, FocusKind::Control);
        if resp.clicked {
            *value = choice;
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let on = self.animate_bool(id, 0, *value == choice);
        let hover = self.animate_bool(id, 1, resp.hovered);
        let d = (size + 4.0).round();
        let gap = 8.0;
        let label = self.frame_text(label);
        let h = self.theme.metrics.control_height.max(d);
        let content = Vec2::new(d + gap + m.x, m.y.max(d));
        self.add_leaf(id, Layout::leaf(Size::Fit, Size::Fixed(h)), content, true, move |p, r| {
            let b = Rect::new(r.x, r.center().y - d * 0.5, d, d);
            let fill = s.track_off.lerp(s.track_on, on);
            let border = s.border_off.lerp(s.border_on, on).lerp(s.knob, hover * 0.3);
            p.rect_bordered(b, fill, d * 0.5, 1.0, border);
            if on > 0.01 {
                let dot = d * 0.42 * on;
                let c = b.center();
                p.rect(Rect::new(c.x - dot, c.y - dot, dot * 2.0, dot * 2.0), s.knob, dot);
            }
            p.text_left(r.shrink(d + gap, 0.0, 0.0, 0.0), size, sel.text_hover, label);
        });
        resp
    }

    /// Progress from 0 to 1, or an indeterminate sweep when `value` is `None`.
    pub fn progress(&mut self, label: &str, value: Option<f32>) {
        let s = self.theme.slider;
        let id = self.make_id(("progress", label));
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let show_label = !label.is_empty();
        let text = match value {
            Some(v) => format!("{:.0}%", v.clamp(0.0, 1.0) * 100.0),
            None => String::new(),
        };
        // An indeterminate bar animates, so it has to ask for frames.
        if value.is_none() {
            self.request_repaint();
        }
        let time = self.time as f32;
        let label = self.frame_text(label);
        let track_h = s.track_height.max(6.0);
        let h = if show_label { m.y + 6.0 + track_h } else { track_h };
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::new(80.0, h), false, move |p, r| {
            if show_label {
                let top = Rect::new(r.x, r.y, r.w, m.y);
                p.text_left(top, size, s.label, label);
                p.text_right(top, size, s.value, &text);
            }
            let track = Rect::new(r.x, r.bottom() - track_h, r.w, track_h);
            p.rect(track, s.track, track_h * 0.5);
            match value {
                Some(v) => {
                    let w = track.w * v.clamp(0.0, 1.0);
                    if w > 0.5 {
                        p.rect(Rect::new(track.x, track.y, w, track_h), s.fill, track_h * 0.5);
                    }
                }
                None => {
                    // A chunk sweeping back and forth, easing at each end.
                    let span = track.w * 0.3;
                    let t = (time * 0.9).sin() * 0.5 + 0.5;
                    let x = track.x + (track.w - span) * t;
                    p.rect(Rect::new(x, track.y, span, track_h), s.fill, track_h * 0.5);
                }
            }
        });
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
        let resp = self.interact_focusable_drag(id, FocusKind::Control);
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
        let label = self.frame_text(label);
        let value_text = format!("{:.2}", *value);
        let h = m.y + 8.0 + 2.0 * kr0;
        self.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(h)), Vec2::new(m.x + 40.0, h), true, move |p, r| {
            let top = Rect::new(r.x, r.y, r.w, m.y);
            p.text_left(top, size, s.label, label);
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

    /// Drag left and right to change a number — the control every inspector is
    /// mostly made of.
    ///
    /// `speed` is units per pixel dragged. Hold Alt (Option) for fine
    /// control, Shift for coarse.
    /// Unbounded unless you pass a range to [`Ui::drag_value_range`].
    pub fn drag_value(&mut self, label: &str, value: &mut f32, speed: f32) -> Response {
        self.drag_value_range(label, value, speed, f32::NEG_INFINITY..=f32::INFINITY)
    }

    /// [`Ui::drag_value`] clamped to a range.
    pub fn drag_value_range(
        &mut self,
        label: &str,
        value: &mut f32,
        speed: f32,
        range: std::ops::RangeInclusive<f32>,
    ) -> Response {
        let s = self.theme.text_input;
        let sl = self.theme.slider;
        let id = self.make_id(("drag_value", label));
        let size = self.theme.metrics.font_size;
        let h = self.theme.metrics.control_height;
        let m = self.text_size(size, label);
        let resp = self.interact_focusable_drag(id, FocusKind::Control);

        if resp.active && resp.drag_delta.x != 0.0 {
            let mods = self.input.modifiers;
            // Alt (Option) for fine, Shift for coarse: the same physical keys
            // on every platform, as DCC tools do.
            let scale = if mods.alt { 0.1 } else if mods.shift { 10.0 } else { 1.0 };
            *value = (*value + resp.drag_delta.x * speed * scale).clamp(*range.start(), *range.end());
        }
        if resp.hovered || resp.active {
            self.cursor = Cursor::ResizeHorizontal;
        }
        if resp.active {
            // Lock the pointer so a drag can keep going past the screen edge
            // instead of stopping when the cursor runs out of room.
            self.request_pointer_lock();
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let drag = self.animate_bool(id, 1, resp.active);
        // Show enough decimals to see a change at this speed.
        let decimals = if speed >= 1.0 { 0 } else if speed >= 0.1 { 1 } else if speed >= 0.01 { 2 } else { 3 };
        let text = format!("{:.*}", decimals, *value);
        let show_label = !label.is_empty();
        let label = self.frame_text(label);
        let content = Vec2::new(m.x + 64.0, m.y);
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(h)).padding(Insets::xy(s.padding_x, 0.0));
        self.add_leaf(id, layout, content, true, move |p, r| {
            let border = s.border.lerp(s.border_hover, hover).lerp(s.border_focus, drag);
            p.rect_bordered(r, s.fill, s.radius, 1.0, border);
            if show_label {
                p.text_left(r.shrink(s.padding_x, 0.0, 0.0, 0.0), size, s.placeholder, label);
                p.text_right(r.shrink(0.0, 0.0, s.padding_x, 0.0), size, sl.value.lerp(sl.value_active, drag), &text);
            } else {
                p.text_centered(r, size, sl.value.lerp(sl.value_active, drag), &text);
            }
        });
        resp
    }

    /// A vertical fader: the mixer control, and the shape a level wants.
    pub fn slider_vertical(&mut self, label: &str, value: &mut f32, min: f32, max: f32, height: f32) -> Response {
        let s = self.theme.slider;
        let id = self.make_id(("vslider", label));
        let resp = self.interact_focusable_drag(id, FocusKind::Control);
        let kr0 = s.knob_radius;
        if resp.active && resp.rect.h > 2.0 * kr0 {
            // Up is more, which is the opposite of the y axis.
            let frac = 1.0 - ((resp.mouse_pos.y - resp.rect.y - kr0) / (resp.rect.h - 2.0 * kr0)).clamp(0.0, 1.0);
            *value = min + frac * (max - min);
        }
        if resp.hovered || resp.active {
            self.cursor = if resp.active { Cursor::Grabbing } else { Cursor::Grab };
        }
        let frac = ((*value - min) / (max - min)).clamp(0.0, 1.0);
        let hover = self.animate_bool(id, 0, resp.hovered || resp.active);
        let drag = self.animate_bool(id, 1, resp.active);
        let _ = label;
        let w = (kr0 * 2.0 + 8.0).ceil();
        self.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Fixed(height)), Vec2::new(w, height), true, move |p, r| {
            let th = s.track_height;
            let cx = r.center().x;
            let track = Rect::new(cx - th * 0.5, r.y + kr0, th, r.h - 2.0 * kr0);
            p.rect(track, s.track, th * 0.5);
            let ky = track.bottom() - frac * track.h;
            p.rect(Rect::new(track.x, ky, th, track.bottom() - ky), s.fill, th * 0.5);
            let ring = kr0 + 5.0 * hover;
            p.rect(Rect::new(cx - ring, ky - ring, ring * 2.0, ring * 2.0), s.ring.with_alpha(s.ring.a * hover), ring);
            let kr = kr0 + drag;
            p.shadow(Rect::new(cx - kr, ky - kr + 1.0, kr * 2.0, kr * 2.0), kr, 2.0, p.theme.palette.shadow);
            p.rect(Rect::new(cx - kr, ky - kr, kr * 2.0, kr * 2.0), s.knob, kr);
        });
        resp
    }

    /// A dropdown: shows the chosen option, opens a menu of them.
    pub fn combo(&mut self, label: &str, selected: &mut usize, options: &[&str]) -> Response {
        let s = self.theme.text_input;
        let menu = self.theme.menu;
        let id = self.make_id(("combo", label));
        let popup_id = id.with("menu");
        let size = self.theme.metrics.font_size;
        let h = self.theme.metrics.control_height;
        let idx = (*selected).min(options.len().saturating_sub(1));
        let shown = options.get(idx).copied().unwrap_or("");
        let m = self.text_size(size, shown);
        let shown = self.frame_text(shown);
        let resp = self.interact_focusable(id, FocusKind::Control);
        let open = self.popup_open(popup_id);
        if resp.opened() {
            if open {
                self.close_popups();
            } else {
                let anchor = self.xform().rect(resp.rect);
                self.open_popup(popup_id, anchor);
            }
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let hot = self.animate_bool(id, 1, open);
        let arrow_w = h;
        let content = Vec2::new(m.x + arrow_w + s.padding_x * 2.0, m.y);
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(h)).padding(Insets::xy(s.padding_x, 0.0));
        self.add_leaf(id, layout, content, true, move |p, r| {
            let border = s.border.lerp(s.border_hover, hover).lerp(s.border_focus, hot);
            p.rect_bordered(r, s.fill, s.radius, 1.0, border);
            p.text_left(r.shrink(s.padding_x, 0.0, arrow_w, 0.0), size, s.text, shown);
            let a = Rect::new(r.right() - arrow_w, r.y, arrow_w, r.h);
            p.chevron(a, size, Chevron::Down, menu.shortcut);
        });

        // The menu is a popup, so it escapes any clipping the combo sits in.
        let opts: Vec<String> = options.iter().map(|o| o.to_string()).collect();
        let width = resp.rect.w.max(120.0);
        let mut picked = None;
        self.popup(popup_id, width, |ui| {
            for (i, o) in opts.iter().enumerate() {
                if ui.menu_item_ex(o, None, true).clicked {
                    picked = Some(i);
                }
            }
        });
        if let Some(i) = picked {
            *selected = i;
        }
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
        let resp = self.interact_focusable(id, FocusKind::Collection);
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let sel = self.animate_bool(id, 1, selected);
        let label = self.frame_text(label);
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
            p.text_left(r.shrink(s.padding_x + 4.0 * sel, 0.0, s.padding_x, 0.0), size, fg, label);
        });
        resp
    }

    /// One row of a tree: indentation, a disclosure arrow, and a label styled
    /// like [`Ui::selectable`].
    ///
    /// libgui does not own your tree. You keep the nodes and the set of
    /// expanded ones, flatten the visible nodes into a list each frame, and
    /// feed that to [`Ui::virtual_list`] — so a tree costs only what is on
    /// screen, however deep or wide it is:
    ///
    /// ```ignore
    /// let rows = flatten(&tree, &expanded);            // Vec<(node, depth)>
    /// ui.virtual_list("tree", rows.len(), row_h, |ui, i| {
    ///     let (node, depth) = rows[i];
    ///     let branch = if tree[node].children.is_empty() { Branch::Leaf }
    ///                  else if expanded.contains(&node) { Branch::Expanded }
    ///                  else { Branch::Collapsed };
    ///     let r = ui.tree_row(node, depth, branch, &tree[node].name, node == selected);
    ///     if r.toggled { toggle(&mut expanded, node); }
    ///     if r.response.clicked { selected = node; }
    /// });
    /// ```
    ///
    /// Clicking the arrow toggles and does *not* select: `toggled` and
    /// `response.clicked` are never both true.
    pub fn tree_row(&mut self, key: impl Hash, depth: usize, branch: Branch, label: &str, selected: bool) -> TreeResponse {
        let s = self.theme.selectable;
        let indent = self.theme.metrics.indent;
        let size = self.theme.metrics.font_size;
        let faint = self.theme.palette.text_faint;
        let id = self.make_id(("tree_row", key));
        let m = self.text_size(size, label);
        let mut resp = self.interact_focusable(id, FocusKind::Collection);

        // The arrow is a region of the row rather than its own widget: one hit
        // rect, and the row still highlights as a whole under the pointer.
        let arrow_x = resp.rect.x + s.padding_x + depth as f32 * indent;
        let on_arrow = branch != Branch::Leaf
            && resp.mouse_pos.x >= arrow_x
            && resp.mouse_pos.x < arrow_x + indent;
        let toggled = resp.clicked && on_arrow;
        if toggled {
            resp.clicked = false;
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hover = self.animate_bool(id, 0, resp.hovered);
        let sel = self.animate_bool(id, 1, selected);
        let arrow_hot = self.animate_bool(id, 2, resp.hovered && on_arrow);

        let label = self.frame_text(label);
        let text_x = s.padding_x + depth as f32 * indent + indent;
        let content = Vec2::new(text_x + m.x, m.y);
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(s.height));
        self.add_leaf(id, layout, content, true, move |p, r| {
            let bg = s.fill_hover.with_alpha(s.fill_hover.a * hover).lerp(s.fill_selected, sel);
            p.rect(r, bg, s.radius);
            if sel > 0.01 && s.indicator_width > 0.0 {
                let bar_h = (r.h * 0.55) * sel;
                let w = s.indicator_width;
                p.rect(Rect::new(r.x + 3.0, r.center().y - bar_h * 0.5, w, bar_h), s.indicator, w * 0.5);
            }
            let fg = s.text.lerp(s.text_hover, hover).lerp(s.text_selected, sel);
            // Switched, not cross-faded: two overlapping arrows read as a smudge.
            if branch != Branch::Leaf {
                let dir = if branch == Branch::Expanded { Chevron::Down } else { Chevron::Right };
                let c = faint.lerp(fg, arrow_hot.max(sel));
                let a = Rect::new(r.x + s.padding_x + depth as f32 * indent, r.y, indent, r.h);
                p.chevron(a, size, dir, c);
            }
            p.text_left(r.shrink(text_x, 0.0, s.padding_x, 0.0), size, fg, label);
        });
        TreeResponse { response: resp, toggled }
    }

    // ---- menus -----------------------------------------------------------

    /// A menu bar button. Click to open its menu; while any menu on the bar is
    /// open, moving across the others opens them, as a menu bar should.
    ///
    /// ```ignore
    /// ui.row(|ui| {
    ///     ui.menu_button("File", |ui| {
    ///         if ui.menu_item_shortcut("Save", &keymap.label(Save)).clicked { save(); }
    ///         ui.menu_separator();
    ///         if ui.menu_item("Quit").clicked { quit(); }
    ///     });
    /// });
    /// ```
    pub fn menu_button<R>(&mut self, label: &str, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let id = self.make_id(("menu_button", label));
        let menu_id = id.with("menu");
        self.menu_button_face(label, id, menu_id);
        self.popup(menu_id, 160.0, body)
    }

    /// The button part of a menu: the label, its hover, and the open/close
    /// logic — everything except the panel it opens.
    fn menu_button_face(&mut self, label: &str, id: Id, menu_id: Id) {
        let s = self.theme.menu;
        let size = self.theme.metrics.font_size;
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        let open = self.popup_open(menu_id);

        // While a menu is open the sheet swallows the pointer, so hovering is
        // judged against this button's own rect: that is what lets you slide
        // from one menu to the next.
        let sliding = self.any_popup_open() && resp.rect.contains(resp.mouse_pos);
        // Popups live at the root in window coordinates, but a response is in
        // whatever space its widget was built in: map the anchor across, or a
        // menu inside a canvas would open at its canvas coordinates.
        let anchor = self.xform().rect(resp.rect);
        if sliding && !open {
            self.open_popup(menu_id, anchor);
        } else if resp.opened() {
            if open {
                self.close_popups();
            } else {
                self.open_popup(menu_id, anchor);
            }
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hot = self.animate_bool(id, 0, resp.hovered || sliding || open);
        let label = self.frame_text(label);
        let layout = Layout::leaf(Size::Fit, Size::Fixed(s.item_height)).padding(Insets::xy(s.item_padding_x, 0.0));
        self.add_leaf(id, layout, m, true, move |p, r| {
            p.rect(r, s.item_fill_hover.with_alpha(s.item_fill_hover.a * hot), s.item_radius);
            p.text_centered(r, size, s.text, label);
        });
    }

    /// [`Ui::menu_button`] without a closure: draws the button and opens the
    /// menu's panel, returning whether it is open. Build the items only when
    /// it returns true, and then call [`Ui::close_menu`].
    pub fn open_menu(&mut self, label: &str) -> bool {
        let id = self.make_id(("menu_button", label));
        let menu_id = id.with("menu");
        self.menu_button_face(label, id, menu_id);
        self.open_popup_body(menu_id, 160.0)
    }

    pub fn close_menu(&mut self) {
        self.close_popup_body();
    }

    /// One row of a menu. Returns a [`Response`]; check `clicked`.
    pub fn menu_item(&mut self, label: &str) -> Response {
        self.menu_item_ex(label, None, true)
    }

    /// A menu row with a right-aligned shortcut hint (`⌘S`, `Ctrl+S`).
    ///
    /// The hint is only text: how a chord is spelled on each platform is the
    /// keymap's (`libgui_keymap::Keymap::label`), and showing it here does not
    /// bind it; handle the chord with [`Ui::consume_shortcut`] as usual.
    pub fn menu_item_shortcut(&mut self, label: &str, hint: &str) -> Response {
        self.menu_item_ex(label, Some(hint), true)
    }

    /// A menu row that can be greyed out.
    pub fn menu_item_ex(&mut self, label: &str, hint: Option<&str>, enabled: bool) -> Response {
        let s = self.theme.menu;
        let size = self.theme.metrics.font_size;
        let id = self.make_id(("menu_item", label));
        let m = self.text_size(size, label);
        let hint = hint.unwrap_or_default();
        let hint_w = if hint.is_empty() { 0.0 } else { self.text_size(size, hint).x + s.item_padding_x };
        let hint = self.frame_text(hint);
        let mut resp = self.interact(id);
        if !enabled {
            resp.clicked = false;
            resp.hovered = false;
        }
        if resp.clicked {
            // Choosing an item dismisses the whole chain, submenus included.
            self.close_popups();
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
            // Moving onto a plain item closes any submenu the pointer left.
            self.close_sibling_submenus(id);
        }
        let hot = self.animate_bool(id, 0, resp.hovered);
        let label = self.frame_text(label);
        let content = Vec2::new(s.gutter + m.x + hint_w + s.item_padding_x, m.y);
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(s.item_height)).padding(Insets::xy(s.item_padding_x, 0.0));
        self.add_leaf(id, layout, content, enabled, move |p, r| {
            if hot > 0.01 {
                p.rect(r, s.item_fill_hover.with_alpha(s.item_fill_hover.a * hot), s.item_radius);
            }
            let fg = if enabled { s.text.lerp(s.text_hover, hot) } else { s.text_disabled };
            p.text_left(r.shrink(s.gutter, 0.0, 0.0, 0.0), size, fg, label);
            if !hint.is_empty() {
                p.text_right(r, size, s.shortcut, hint);
            }
        });
        resp
    }

    /// A horizontal rule between groups of menu items.
    pub fn menu_separator(&mut self) {
        let s = self.theme.menu;
        let id = self.make_id("menu_sep");
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(s.separator_height));
        self.add_leaf(id, layout, Vec2::ZERO, false, move |p, r| {
            let y = r.center().y.round();
            p.rect(Rect::new(r.x, y, r.w, 1.0), s.separator, 0.0);
        });
    }

    /// A menu row that opens a further menu beside it, on hover.
    pub fn submenu<R>(&mut self, label: &str, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let s = self.theme.menu;
        let size = self.theme.metrics.font_size;
        let id = self.make_id(("submenu", label));
        let child = id.with("menu");
        let m = self.text_size(size, label);
        let resp = self.interact(id);
        let open = self.popup_open(child);
        if resp.hovered && !open {
            // Anchored to the row's right edge, so the child sits beside it.
            let a = self.xform().rect(resp.rect);
            let anchor = Rect::new(a.right() - 4.0, a.y - s.padding.top - 1.0, 0.0, 0.0);
            if let Some(parent) = self.enclosing_popup() {
                self.open_child_popup(parent, child, anchor);
            }
        }
        if resp.hovered {
            self.cursor = Cursor::Pointer;
        }
        let hot = self.animate_bool(id, 0, resp.hovered || open);
        let label = self.frame_text(label);
        let arrow_w = s.item_padding_x * 1.5;
        let content = Vec2::new(s.gutter + m.x + arrow_w + s.item_padding_x, m.y);
        let layout = Layout::leaf(Size::Grow(1.0), Size::Fixed(s.item_height)).padding(Insets::xy(s.item_padding_x, 0.0));
        self.add_leaf(id, layout, content, true, move |p, r| {
            if hot > 0.01 {
                p.rect(r, s.item_fill_hover.with_alpha(s.item_fill_hover.a * hot), s.item_radius);
            }
            let fg = s.text.lerp(s.text_hover, hot);
            p.text_left(r.shrink(s.gutter, 0.0, 0.0, 0.0), size, fg, label);
            p.chevron(Rect::new(r.right() - arrow_w, r.y, arrow_w, r.h), size, Chevron::Right, s.shortcut);
        });
        self.popup(child, 140.0, body)
    }

    /// Right-click menu for the widget `resp` came from.
    ///
    /// ```ignore
    /// let r = ui.selectable(&name, selected);
    /// ui.context_menu(&r, |ui| {
    ///     if ui.menu_item("Rename").clicked { rename(); }
    /// });
    /// ```
    pub fn context_menu<R>(&mut self, resp: &Response, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        let id = resp.id.with("context_menu");
        if resp.secondary_pressed {
            // Anchored to the pointer, so it opens where you clicked — in
            // window coordinates, since the popup itself is not in the canvas.
            let p = self.xform().point(resp.mouse_pos);
            self.open_popup(id, Rect::new(p.x, p.y, 0.0, 0.0));
        }
        self.popup(id, 160.0, body)
    }

    /// One-of-N picker (density, tool modes, view modes).
    pub fn segmented(&mut self, key: &str, selected: &mut usize, options: &[&str]) -> Response {
        let s = self.theme.segmented;
        let id = self.make_id(("segmented", key));
        let size = self.theme.metrics.font_size;
        let widths: Vec<f32> = options.iter().map(|o| self.text_size(size, o).x + 24.0).collect();
        let total: f32 = widths.iter().sum();
        let resp = self.interact_focusable(id, FocusKind::Control);
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
        let options: Vec<crate::FrameText> = options.iter().map(|o| self.frame_text(o)).collect();
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
    ///
    /// Render your scene at [`Ui::physical_px`] of the returned rect and hand
    /// the texture back through `texture`; anything else is resampled and looks
    /// soft. The rect is one frame old, like every [`Response`], so resize your
    /// target when it changes rather than every frame.
    ///
    /// The image is composited as **opaque sRGB**: its alpha is ignored and no
    /// colour conversion happens, so a linear or HDR target must be converted
    /// before it gets here. See `libgui::render_contract`.
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

    /// A draggable divider that resizes a pane, outside the dock.
    ///
    /// The dock has its own splitters, but a UI that is not docked — three
    /// fixed panels around a viewport, a sidebar, an inspector — has no way to
    /// let the user move the boundary. This is that: a thin handle that moves
    /// one number, in logical px, which the caller then feeds back into a
    /// [`Size::Fixed`] on whatever it sizes.
    ///
    /// ```no_run
    /// # use libgui::*;
    /// # fn f(ui: &mut Ui, inspector_w: &mut f32) {
    /// // An inspector docked to the RIGHT: its handle is on its leading edge,
    /// // so dragging left must make it *wider*. That is what `inverted` is.
    /// let opts = SplitterOptions::vertical_rule(180.0, 520.0).inverted();
    /// if ui.splitter("inspector", inspector_w, opts).double_clicked {
    ///     *inspector_w = 280.0;
    /// }
    /// # }
    /// ```
    ///
    /// Returns the [`Response`], so a double click resets the pane to whatever
    /// the app thinks its default is. libgui does not pick that default: a
    /// widget that silently restored a number it invented would be policy.
    pub fn splitter(&mut self, key: &str, value: &mut f32, opts: SplitterOptions) -> Response {
        let id = self.make_id(("splitter", key));
        // The handle is laid out *after* the pane it sizes, so it must survive
        // a frame in which the pane's own id changes; the dock's splitter does
        // the same.
        self.keep_id(id);
        let resp = self.interact_drag(id);
        let (lo, hi) = opts.range;
        // A range worked out from a size that is not known yet — `(w * 0.1, w
        // * 0.9)` on the first frame — arrives as NaN, and `f32::clamp` panics
        // on a NaN bound as readily as on a reversed one. A host's bad number
        // must not take the app down, so the drag is simply ignored until the
        // range is a range again. Everything else about the widget still
        // works: it draws, it reports, the cursor still changes.
        let usable = lo.is_finite() && hi.is_finite();
        // A value that is not finite is repaired straight away, not on the
        // next drag: a NaN width breaks the layout it feeds, so the handle
        // lands nowhere and there is no drag left to recover on. NaN is never
        // a width someone meant, unlike a finite value outside the range,
        // which is left alone until a drag brings it in.
        if usable && !value.is_finite() {
            *value = lo.min(hi);
        }
        if resp.active && usable {
            let d = match opts.axis {
                // A vertical rule is dragged horizontally: the axis names the
                // rule's own direction, the way a person describes the line
                // they see, not the direction it travels.
                Axis::Y => resp.drag_delta.x,
                Axis::X => resp.drag_delta.y,
            };
            let d = if opts.invert { -d } else { d };
            // `min` before `max`: an inverted range would otherwise clamp to
            // the wrong end and the pane would jump across the window.
            let (lo, hi) = (lo.min(hi), lo.max(hi));
            *value = (*value + d).clamp(lo, hi);
        }
        if resp.hovered || resp.active {
            self.cursor = match opts.axis {
                Axis::Y => Cursor::ResizeHorizontal,
                Axis::X => Cursor::ResizeVertical,
            };
        }
        let hot = self.animate_bool(id, 0, resp.hovered || resp.active);
        let style = self.theme.splitter;
        let axis = opts.axis;
        let layout = match axis {
            Axis::Y => Layout::leaf(Size::Fixed(style.size), Size::Grow(1.0)),
            Axis::X => Layout::leaf(Size::Grow(1.0), Size::Fixed(style.size)),
        };
        // The visible rule is thin and the grab area is not: a 4 px line is
        // hard to hit and a person aiming at it misses low as often as high.
        let leaf = crate::LeafOptions { interactive: true, hit_pad: opts.hit_pad, hit_top: true };
        self.add_leaf_ex(id, layout, Vec2::ZERO, leaf, move |p, r| {
            if hot > 0.01 {
                let line = match axis {
                    Axis::Y => Rect::new(r.center().x - 1.0, r.y, 2.0, r.h),
                    Axis::X => Rect::new(r.x, r.center().y - 1.0, r.w, 2.0),
                };
                p.rect(line, style.line_hover.with_alpha(style.line_hover.a * hot), 1.0);
            }
        });
        resp
    }
}

/// How a [`Ui::splitter`] behaves. See that method.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitterOptions {
    /// The direction of the *rule itself*: [`Axis::Y`] is a vertical line
    /// between two side-by-side panes, dragged horizontally.
    pub axis: Axis,
    /// Smallest and largest the value may become, logical px.
    pub range: (f32, f32),
    /// The value grows as the pointer moves *against* the drag direction, for
    /// a pane whose handle is on its leading edge: an inspector docked right,
    /// or a console docked to the bottom. Without it, dragging the inspector's
    /// left edge to the left makes it narrower, which is backwards.
    pub invert: bool,
    /// Extra grab area on each side of the visible rule, logical px.
    pub hit_pad: f32,
}

impl SplitterOptions {
    /// A vertical rule between two side-by-side panes, dragged horizontally.
    pub fn vertical_rule(min: f32, max: f32) -> Self {
        Self { axis: Axis::Y, range: (min, max), invert: false, hit_pad: 3.0 }
    }

    /// A horizontal rule between two stacked panes, dragged vertically.
    pub fn horizontal_rule(min: f32, max: f32) -> Self {
        Self { axis: Axis::X, range: (min, max), invert: false, hit_pad: 3.0 }
    }

    /// For a pane whose handle is on its leading edge: docked right, or
    /// docked to the bottom. See [`SplitterOptions::invert`].
    pub fn inverted(mut self) -> Self {
        self.invert = true;
        self
    }

    pub fn hit_pad(mut self, pad: f32) -> Self {
        self.hit_pad = pad;
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn ui() -> Ui {
        let mut ui = Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).unwrap();
        ui.set_key_bindings(crate::input::test_bindings());
        ui
    }

    fn click_at(ui: &mut Ui, x: f32, y: f32, down: bool) {
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(x, y) });
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: down });
    }

    /// Checkbox and radio toggle their own value; radio is exclusive.
    #[test]
    fn checkbox_and_radio_set_their_values() {
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum Mode {
            A,
            B,
        }
        let mut ui = ui();
        let (mut checked, mut mode) = (false, Mode::A);
        let frame = |ui: &mut Ui, checked: &mut bool, mode: &mut Mode| -> (Rect, Rect) {
            ui.begin_frame(FrameInfo::default());
            let c = ui.checkbox("Visible", checked).rect;
            let r = ui.radio("Mode B", mode, Mode::B).rect;
            let _ = ui.end_frame();
            (c, r)
        };
        frame(&mut ui, &mut checked, &mut mode);
        let (cbox, rbox) = frame(&mut ui, &mut checked, &mut mode);

        // Click the checkbox: on, then off again.
        for expected in [true, false] {
            click_at(&mut ui, cbox.x + 8.0, cbox.center().y, true);
            frame(&mut ui, &mut checked, &mut mode);
            click_at(&mut ui, cbox.x + 8.0, cbox.center().y, false);
            frame(&mut ui, &mut checked, &mut mode);
            assert_eq!(checked, expected, "checkbox did not toggle to {expected}");
        }

        // Radio sets its choice and, unlike a checkbox, does not unset it.
        for _ in 0..2 {
            click_at(&mut ui, rbox.x + 8.0, rbox.center().y, true);
            frame(&mut ui, &mut checked, &mut mode);
            click_at(&mut ui, rbox.x + 8.0, rbox.center().y, false);
            frame(&mut ui, &mut checked, &mut mode);
            assert_eq!(mode, Mode::B, "radio should stay on its choice");
        }
    }

    /// Dragging a number field changes it by distance times speed, with the
    /// modifiers scaling it, and a range clamping it.
    #[test]
    fn drag_value_tracks_the_pointer_and_clamps() {
        let mut ui = ui();
        let mut v = 0.0f32;
        let frame = |ui: &mut Ui, v: &mut f32| -> Rect {
            ui.begin_frame(FrameInfo::default());
            let r = ui.drag_value_range("X", v, 1.0, -50.0..=50.0).rect;
            let _ = ui.end_frame();
            r
        };
        frame(&mut ui, &mut v);
        let r = frame(&mut ui, &mut v);
        let y = r.center().y;

        // Press, then drag 30px right: +30 at speed 1.
        click_at(&mut ui, r.x + 20.0, y, true);
        frame(&mut ui, &mut v);
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(r.x + 50.0, y) });
        frame(&mut ui, &mut v);
        assert!((v - 30.0).abs() < 0.01, "expected 30, got {v}");

        // Keep going past the range: clamped, not run away.
        for i in 1..20 {
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(r.x + 50.0 + i as f32 * 20.0, y) });
            frame(&mut ui, &mut v);
        }
        assert_eq!(v, 50.0, "range did not clamp");
        click_at(&mut ui, r.x + 400.0, y, false);
        frame(&mut ui, &mut v);

        // Releasing stops it following the pointer.
        let held = v;
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(r.x + 10.0, y) });
        frame(&mut ui, &mut v);
        assert_eq!(v, held, "value moved after the drag ended");
    }

    /// A vertical fader is high at the top, unlike the y axis it is drawn on.
    #[test]
    fn a_vertical_slider_is_high_at_the_top() {
        let mut ui = ui();
        let mut v = 0.5f32;
        let frame = |ui: &mut Ui, v: &mut f32| -> Rect {
            ui.begin_frame(FrameInfo::default());
            let r = ui.slider_vertical("Gain", v, 0.0, 1.0, 200.0).rect;
            let _ = ui.end_frame();
            r
        };
        frame(&mut ui, &mut v);
        let r = frame(&mut ui, &mut v);

        click_at(&mut ui, r.center().x, r.y + 2.0, true);
        frame(&mut ui, &mut v);
        assert!(v > 0.95, "dragging to the top should be the maximum, got {v}");
        click_at(&mut ui, r.center().x, r.bottom() - 2.0, true);
        frame(&mut ui, &mut v);
        assert!(v < 0.05, "dragging to the bottom should be the minimum, got {v}");
    }

    /// A combo opens a menu and picking an item sets the index.
    #[test]
    fn a_combo_picks_from_its_menu() {
        let mut ui = ui();
        let mut sel = 0usize;
        let frame = |ui: &mut Ui, sel: &mut usize| -> (Rect, bool) {
            ui.begin_frame(FrameInfo::default());
            let r = ui.combo("Blend", sel, &["Normal", "Add", "Multiply"]).rect;
            let open = ui.any_popup_open();
            let _ = ui.end_frame();
            (r, open)
        };
        frame(&mut ui, &mut sel);
        let (r, _) = frame(&mut ui, &mut sel);

        click_at(&mut ui, r.center().x, r.center().y, true);
        frame(&mut ui, &mut sel);
        click_at(&mut ui, r.center().x, r.center().y, false);
        assert!(frame(&mut ui, &mut sel).1, "the combo did not open");
        frame(&mut ui, &mut sel);

        // Pick the third item.
        let menu = ui.rect_of(Id::new("root").with(("combo", "Blend")).with("menu")).expect("no menu");
        let item_y = menu.y + menu.h - 14.0;
        click_at(&mut ui, menu.x + 20.0, item_y, true);
        frame(&mut ui, &mut sel);
        click_at(&mut ui, menu.x + 20.0, item_y, false);
        let (_, open) = frame(&mut ui, &mut sel);
        assert_eq!(sel, 2, "picked the wrong option");
        assert!(!open, "the menu stayed open after picking");
    }

    /// The core of a menu: it opens on a click, sizes itself to its items,
    /// sits above the content, and stops that content being clicked through.
    #[test]
    fn a_menu_opens_sizes_itself_and_blocks_what_is_under_it() {
        let mut ui = ui();
        // (menu button response, a button under where the menu will appear)
        let frame = |ui: &mut Ui| -> (bool, Response, Vec<&'static str>) {
            let mut chosen = Vec::new();
            ui.begin_frame(FrameInfo::default());
            ui.menu_button("File", |ui| {
                if ui.menu_item("Open").clicked {
                    chosen.push("Open");
                }
                ui.menu_separator();
                if ui.menu_item("Save As Something Rather Long").clicked {
                    chosen.push("Save");
                }
            });
            let under = ui.button("Underneath");
            let open = ui.any_popup_open();
            let _ = ui.end_frame();
            (open, under, chosen)
        };
        frame(&mut ui);
        frame(&mut ui);
        assert!(!frame(&mut ui).0, "a menu should start closed");

        // Click the File button: press then release.
        click_at(&mut ui, 20.0, 10.0, true);
        frame(&mut ui);
        click_at(&mut ui, 20.0, 10.0, false);
        assert!(frame(&mut ui).0, "clicking the menu button did not open it");

        // It must size itself to its widest item rather than collapsing.
        frame(&mut ui);
        let rect = ui.rect_of(Id::new("root").with(("menu_button", "File")).with("menu")).expect("no popup rect");
        assert!(rect.h > 40.0, "the menu is {}px tall: it did not fit its items", rect.h);
        assert!(rect.w > 160.0, "the menu is {}px wide: it did not fit its widest item", rect.w);

        // The button underneath the open menu must not be hoverable.
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(rect.x + 20.0, rect.y + 20.0) });
        let (_, under, _) = frame(&mut ui);
        assert!(!under.hovered, "a widget under an open menu was still hovered");
    }

    /// Choosing an item reports the click and dismisses the menu; clicking away
    /// dismisses without choosing anything.
    #[test]
    fn a_menu_item_reports_its_click_and_closes() {
        let mut ui = ui();
        let frame = |ui: &mut Ui| -> (bool, Vec<&'static str>) {
            let mut chosen = Vec::new();
            ui.begin_frame(FrameInfo::default());
            ui.menu_button("File", |ui| {
                if ui.menu_item("Open").clicked {
                    chosen.push("Open");
                }
                if ui.menu_item("Quit").clicked {
                    chosen.push("Quit");
                }
            });
            let open = ui.any_popup_open();
            let _ = ui.end_frame();
            (open, chosen)
        };
        let open_menu = |ui: &mut Ui, frame: &dyn Fn(&mut Ui) -> (bool, Vec<&'static str>)| {
            click_at(ui, 20.0, 10.0, true);
            frame(ui);
            click_at(ui, 20.0, 10.0, false);
            frame(ui);
            frame(ui);
        };
        frame(&mut ui);
        open_menu(&mut ui, &frame);
        let menu = ui.rect_of(Id::new("root").with(("menu_button", "File")).with("menu")).unwrap();

        // Click the first item.
        let item_y = menu.y + 14.0;
        click_at(&mut ui, menu.x + 30.0, item_y, true);
        frame(&mut ui);
        click_at(&mut ui, menu.x + 30.0, item_y, false);
        let (open, chosen) = frame(&mut ui);
        assert_eq!(chosen, vec!["Open"], "the item did not report its click");
        assert!(!open, "choosing an item left the menu open");

        // Reopen, then press far away: dismissed, nothing chosen.
        open_menu(&mut ui, &frame);
        click_at(&mut ui, 600.0, 500.0, true);
        let (open, chosen) = frame(&mut ui);
        assert!(chosen.is_empty(), "clicking away chose an item");
        assert!(!open, "clicking away left the menu open");
    }

    /// A context menu opened from inside a canvas must appear under the
    /// pointer on screen, not at the canvas coordinates the widget reported.
    #[test]
    fn a_context_menu_inside_a_canvas_opens_under_the_pointer() {
        let mut ui = ui();
        let mut st = CanvasState { zoom: 2.0, pan: Vec2::new(40.0, 30.0), wheel_zooms: false, ..Default::default() };
        let run = |ui: &mut Ui, st: &mut CanvasState| {
            ui.begin_frame(FrameInfo::default());
            ui.canvas("c", st, |ui, _| {
                let id = ui.make_id("node");
                let opts = LeafOptions { interactive: true, ..Default::default() };
                ui.add_leaf_at(id, Rect::new(0.0, 0.0, 400.0, 400.0), opts, |_, _| {});
                let r = ui.interact(id);
                ui.context_menu(&r, |ui| {
                    let _ = ui.menu_item("Delete");
                });
            });
            let _ = ui.end_frame();
        };
        run(&mut ui, &mut st);
        run(&mut ui, &mut st);

        let pointer = Vec2::new(300.0, 220.0);
        ui.push(InputEvent::PointerMoved { pos: pointer });
        ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: true });
        run(&mut ui, &mut st);
        ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: false });
        run(&mut ui, &mut st);
        run(&mut ui, &mut st);

        // The menu's id is derived from the widget it belongs to.
        let node = Id::new("root").with(("canvas", "c")).with("node");
        let r = ui.rect_of(node.with("context_menu")).expect("context menu has no rect");
        assert!(
            (r.x - pointer.x).abs() < 8.0 && r.y >= pointer.y - 1.0,
            "menu opened at {r:?} but the pointer was at {pointer:?}"
        );
    }

    /// Right-click opens a menu where the pointer is, not where the widget is.
    #[test]
    fn a_context_menu_opens_at_the_pointer() {
        let mut ui = ui();
        let frame = |ui: &mut Ui| -> bool {
            ui.begin_frame(FrameInfo::default());
            let r = ui.selectable("Object", false);
            ui.context_menu(&r, |ui| {
                let _ = ui.menu_item("Rename");
                let _ = ui.menu_item("Delete");
            });
            let open = ui.any_popup_open();
            let _ = ui.end_frame();
            open
        };
        frame(&mut ui);
        frame(&mut ui);
        assert!(!frame(&mut ui));

        let (px, py) = (120.0, 12.0);
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(px, py) });
        ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: true });
        assert!(frame(&mut ui), "right-click did not open a context menu");
        ui.push(InputEvent::PointerButton { button: PointerButton::Secondary, pressed: false });
        frame(&mut ui);
        frame(&mut ui);

        let id = Id::new("root").with(("selectable", "Object")).with("context_menu");
        let r = ui.rect_of(id).expect("no context menu rect");
        assert!((r.x - px).abs() < 8.0, "menu at x {} but the pointer was at {px}", r.x);
        assert!(r.y >= py, "menu at y {} should be at or below the pointer {py}", r.y);
    }

    /// Escape backs out of a menu.
    #[test]
    fn escape_closes_a_menu() {
        let mut ui = ui();
        let frame = |ui: &mut Ui| -> bool {
            ui.begin_frame(FrameInfo::default());
            ui.menu_button("File", |ui| {
                let _ = ui.menu_item("Open");
            });
            let open = ui.any_popup_open();
            let _ = ui.end_frame();
            open
        };
        frame(&mut ui);
        click_at(&mut ui, 20.0, 10.0, true);
        frame(&mut ui);
        click_at(&mut ui, 20.0, 10.0, false);
        assert!(frame(&mut ui));

        ui.push(InputEvent::Key { key: Key::Escape, pressed: true, repeat: false });
        assert!(!frame(&mut ui), "Escape did not close the menu");
    }

    /// The arrow toggles, the rest of the row selects, and the two never fire
    /// together — otherwise expanding a node would also change the selection.
    #[test]
    fn tree_row_separates_the_arrow_from_the_row() {
        let mut ui = ui();
        let indent = ui.theme.metrics.indent;
        let pad = ui.theme.selectable.padding_x;
        let row_h = ui.theme.selectable.height;

        let frame = |ui: &mut Ui| -> Vec<TreeResponse> {
            let mut out = Vec::new();
            ui.begin_frame(FrameInfo::default());
            // depth 0 branch, depth 1 leaf.
            out.push(ui.tree_row(0u32, 0, Branch::Collapsed, "Group", false));
            out.push(ui.tree_row(1u32, 1, Branch::Leaf, "Child", false));
            let _ = ui.end_frame();
            out
        };
        frame(&mut ui);
        frame(&mut ui);

        // Click the arrow of row 0 (depth 0, so it sits at pad..pad+indent).
        let arrow_x = pad + indent * 0.5;
        let row0_y = row_h * 0.5;
        click_at(&mut ui, arrow_x, row0_y, true);
        frame(&mut ui);
        click_at(&mut ui, arrow_x, row0_y, false);
        let r = frame(&mut ui);
        assert!(r[0].toggled, "clicking the arrow did not toggle");
        assert!(!r[0].response.clicked, "clicking the arrow also selected the row");

        // Click the label area of row 0.
        let label_x = pad + indent * 3.0;
        click_at(&mut ui, label_x, row0_y, true);
        frame(&mut ui);
        click_at(&mut ui, label_x, row0_y, false);
        let r = frame(&mut ui);
        assert!(r[0].response.clicked, "clicking the label did not select");
        assert!(!r[0].toggled, "clicking the label also toggled");

        // A leaf has no arrow: a click where its arrow would be still selects.
        let leaf_arrow_x = pad + indent * 1.5;
        let row1_y = row_h * 1.5;
        click_at(&mut ui, leaf_arrow_x, row1_y, true);
        frame(&mut ui);
        click_at(&mut ui, leaf_arrow_x, row1_y, false);
        let r = frame(&mut ui);
        assert!(!r[1].toggled, "a leaf toggled");
        assert!(r[1].response.clicked, "a leaf did not select");
    }

    /// Depth must move the arrow, so a click lands on the right node's arrow
    /// rather than on an ancestor's indentation.
    #[test]
    fn tree_row_arrow_follows_depth() {
        let mut ui = ui();
        let indent = ui.theme.metrics.indent;
        let pad = ui.theme.selectable.padding_x;
        let row_h = ui.theme.selectable.height;
        let frame = |ui: &mut Ui| -> TreeResponse {
            ui.begin_frame(FrameInfo::default());
            let r = ui.tree_row(0u32, 2, Branch::Expanded, "Deep", false);
            let _ = ui.end_frame();
            r
        };
        frame(&mut ui);
        frame(&mut ui);

        // Where a depth-0 arrow would be: that is indentation now, so it selects.
        click_at(&mut ui, pad + indent * 0.5, row_h * 0.5, true);
        frame(&mut ui);
        click_at(&mut ui, pad + indent * 0.5, row_h * 0.5, false);
        let r = frame(&mut ui);
        assert!(!r.toggled && r.response.clicked, "indentation behaved like an arrow");

        // The depth-2 arrow does toggle.
        click_at(&mut ui, pad + indent * 2.5, row_h * 0.5, true);
        frame(&mut ui);
        click_at(&mut ui, pad + indent * 2.5, row_h * 0.5, false);
        let r = frame(&mut ui);
        assert!(r.toggled && !r.response.clicked, "the depth-2 arrow did not toggle");
    }
}
