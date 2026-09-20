//! The handful of pieces a dense editor needs that are not in the library
//! yet: a tab strip, a panel frame, a toolbar icon, a breadcrumb. All built
//! from `Ui`'s own parts, which is the point — none of them reach inside it.

use libgui::*;

pub fn panel_frame(t: &Theme) -> Frame {
    Frame { fill: t.palette.bg_panel, border: t.palette.border, border_width: 1.0, radius: 0.0, shadow: false, clip: true }
}

/// The strip of tabs every panel in a tool like this wears, with the `+` that
/// adds another.
pub fn tabs(ui: &mut Ui, key: &str, selected: &mut usize, labels: &[&str]) {
    let t = ui.theme.clone();
    let h = t.tab.height;
    let bar = Layout::row().width(Size::Grow(1.0)).height(Size::Fixed(h)).gap(0.0);
    let __id = ui.make_id(("tabs", key));
        ui.container_id(__id, bar, Frame { fill: t.tab.bar_fill, clip: true, ..Frame::none() }, |ui| {
        for (i, label) in labels.iter().enumerate() {
            let on = *selected == i;
            let id = ui.make_id(("tab", key, i));
            let r = ui.interact_focusable(id, FocusKind::Control);
            if r.clicked {
                *selected = i;
            }
            let hot = ui.animate_bool(id, 0, r.hovered);
            let size = t.metrics.font_size;
            let w = ui.fonts.measure(ui.font, size, label).x + 16.0;
            let text = ui.frame_text(label);
            let s = t.tab;
            ui.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Grow(1.0)), Vec2::ZERO, true, move |p, rect| {
                let fill = if on { s.fill_active } else { s.fill_hover.with_alpha(s.fill_hover.a * hot) };
                if fill.a > 0.0 {
                    p.rect(rect, fill, 0.0);
                }
                let fg = if on { s.text_active } else { s.text };
                p.text_centered(rect, size, fg, text);
                if on {
                    let u = p.snap_rect(Rect::new(rect.x, rect.bottom() - s.accent_height, rect.w, s.accent_height));
                    p.rect(u, s.accent, 0.0);
                }
            });
        }
        plus(ui, key);
        ui.flex();
    });
}

fn plus(ui: &mut Ui, key: &str) {
    let t = ui.theme.clone();
    let id = ui.make_id(("plus", key));
    let r = ui.interact(id);
    let hot = ui.animate_bool(id, 0, r.hovered);
    let c = t.palette.text_faint.lerp(t.palette.text, hot);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(16.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, rect| {
        let m = rect.center();
        p.rect(p.hairline(m.x - 4.0, m.y, 1.0, 1.0).translate(0.0, 0.0), c, 0.0);
        p.rect(Rect::new(m.x - 4.0, m.y - 0.5, 8.0, 1.0), c, 0.0);
        p.rect(Rect::new(m.x - 0.5, m.y - 4.0, 1.0, 8.0), c, 0.0);
    });
}

/// The path field under a panel's tabs: a small icon, a name, and the arrows
/// that walk the history.
pub fn breadcrumb(ui: &mut Ui, key: &str, path: &str) {
    let t = ui.theme.clone();
    let row = Layout::row()
        .width(Size::Grow(1.0))
        .height(Size::Fixed(19.0))
        .padding(Insets::xy(3.0, 0.0))
        .gap(3.0)
        .align(Align::Start, Align::Center);
    let __id = ui.make_id(("crumb", key));
        ui.container_id(__id, row, Frame { fill: t.palette.bg_panel, ..Frame::none() }, |ui| {
        for (i, dir) in [Chevron::Left, Chevron::Right].into_iter().enumerate() {
            let id = ui.make_id(("crumbnav", key, i));
            let c = t.palette.text_faint;
            ui.add_leaf(id, Layout::leaf(Size::Fixed(12.0), Size::Fixed(14.0)), Vec2::ZERO, true, move |p, r| {
                p.chevron(r, 9.0, dir, c);
            });
        }
        let field = Layout::row()
            .width(Size::Grow(1.0))
            .height(Size::Fixed(15.0))
            .padding(Insets::xy(5.0, 0.0))
            .gap(4.0)
            .align(Align::Start, Align::Center);
        let f = Frame { fill: t.palette.bg_inset, border: t.palette.border, border_width: 1.0, radius: 2.0, ..Frame::none() };
        let __id = ui.make_id(("crumbfield", key));
        ui.container_id(__id, field, f, |ui| {
            swatch(ui, ("crumbicon", key), t.palette.accent);
            ui.label(path);
        });
    });
}

/// A small coloured tile, which is what most of a shelf's icons read as at
/// this size.
pub fn swatch(ui: &mut Ui, key: impl std::hash::Hash, color: Color) {
    let id = ui.make_id(key);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(10.0), Size::Fixed(10.0)), Vec2::ZERO, false, move |p, r| {
        p.rect(Rect::new(r.x, r.y + (r.h - 9.0) * 0.5, 9.0, 9.0), color, 2.0);
    });
}

/// One tool on the shelf: an icon over a two-line label, the way every 3D
/// application lays a shelf out.
pub fn shelf_tool(ui: &mut Ui, key: impl std::hash::Hash + Copy, label: &str, color: Color, w: f32) {
    let t = ui.theme.clone();
    let id = ui.make_id(("shelf", key));
    let r = ui.interact(id);
    let hot = ui.animate_bool(id, 0, r.hovered);
    let size = t.metrics.font_size_small;
    let text = ui.frame_text(label);
    let fg = t.palette.text_muted;
    let fill = t.palette.surface.with_alpha(0.7 * hot);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(w), Size::Grow(1.0)), Vec2::ZERO, true, move |p, rect| {
        if fill.a > 0.0 {
            p.rect(rect, fill, 2.0);
        }
        let c = rect.center();
        p.rect(Rect::new(c.x - 7.0, rect.y + 4.0, 14.0, 14.0), color, 3.0);
        p.rect(Rect::new(c.x - 3.0, rect.y + 8.0, 6.0, 6.0), color.lerp(Color::WHITE, 0.45), 1.5);
        let words = Rect::new(rect.x, rect.y + 20.0, rect.w, rect.h - 20.0);
        p.text_wrapped(words, size, fg, Align::Center, text);
    });
}

/// An icon-only button, for the toolbars that run down the sides of a
/// viewport and across the top of a network editor.
pub fn tool_icon(ui: &mut Ui, key: impl std::hash::Hash, shape: u8, size: f32) {
    let t = ui.theme.clone();
    let id = ui.make_id(key);
    let r = ui.interact(id);
    let hot = ui.animate_bool(id, 0, r.hovered);
    let fg = t.palette.text_faint.lerp(t.palette.text, hot);
    let bg = t.palette.surface.with_alpha(0.8 * hot);
    ui.add_leaf(id, Layout::leaf(Size::Fixed(size), Size::Fixed(size)), Vec2::ZERO, true, move |p, rect| {
        if bg.a > 0.0 {
            p.rect(rect, bg, 2.0);
        }
        let c = rect.center();
        let k = size * 0.28;
        match shape % 6 {
            0 => p.rect_bordered(Rect::new(c.x - k, c.y - k, k * 2.0, k * 2.0), Color::TRANSPARENT, 1.0, 1.0, fg),
            1 => p.rect(Rect::new(c.x - k, c.y - k, k * 2.0, k * 2.0), fg, k),
            2 => {
                p.line(Vec2::new(c.x - k, c.y + k), Vec2::new(c.x + k, c.y - k), 1.0, fg);
                p.line(Vec2::new(c.x - k, c.y - k), Vec2::new(c.x + k, c.y + k), 1.0, fg);
            }
            3 => {
                p.rect(Rect::new(c.x - k, c.y - 0.5, k * 2.0, 1.0), fg, 0.0);
                p.rect(Rect::new(c.x - 0.5, c.y - k, 1.0, k * 2.0), fg, 0.0);
            }
            4 => p.rect_bordered(Rect::new(c.x - k, c.y - k * 0.6, k * 2.0, k * 1.2), Color::TRANSPARENT, 1.0, 1.0, fg),
            _ => p.chevron(rect, size * 0.5, Chevron::Down, fg),
        }
    });
}

/// A row of them.
pub fn tool_row(ui: &mut Ui, key: &str, n: usize, size: f32) {
    for i in 0..n {
        tool_icon(ui, (key, i), i as u8, size);
    }
}
