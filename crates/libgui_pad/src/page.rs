//! The document: a ruler and a white sheet with the text area on it.
//!
//! The sheet is app drawing — a container with a fill and a shadow — and the
//! editor on it is [`Ui::text_area`] restyled through [`Ui::with_style`]: dark
//! ink on white, no border, no focus ring, set in the size the toolbar picked.
//! Nothing about the widget changes; only the theme it reads does.

use libgui::*;

use crate::theme::PAGE;
use crate::Pad;

/// Width of the sheet, and the margin printed inside it.
const SHEET_W: f32 = 700.0;
const MARGIN_X: f32 = 56.0;
const MARGIN_Y: f32 = 34.0;

pub fn page(ui: &mut Ui, pad: &mut Pad) {
    let t = ui.theme.clone();
    let desk = Layout::column()
        .width(Size::Grow(1.0))
        .height(Size::Grow(1.0))
        .padding(Insets { left: 0.0, right: 0.0, top: 10.0, bottom: 12.0 })
        .gap(8.0)
        .align(Align::Start, Align::Center);
    let frame = Frame { fill: t.palette.bg_app, clip: true, ..Frame::none() };
    ui.container_id(Id::new("desk"), desk, frame, |ui| {
        ruler(ui);
        sheet(ui, pad);
    });
}

/// The ruler over the page: inch ticks, and the margins shown as the darker
/// ends. It measures the sheet, so it is drawn in the sheet's coordinates
/// rather than the window's.
fn ruler(ui: &mut Ui) {
    let t = ui.theme.clone();
    let (face, tick, text) = (PAGE.ruler, PAGE.rule, PAGE.ink_faint);
    let margin = PAGE.ruler_margin;
    let size = t.metrics.font_size_small;
    // One inch of a 7-inch text column, which is what the sheet's width is
    // standing in for.
    let inch = (SHEET_W - MARGIN_X * 2.0) / 6.0;
    let numbers: Vec<FrameText> = (1..6).map(|i| ui.frame_text(&i.to_string())).collect();

    let id = ui.make_id("ruler");
    ui.add_leaf(id, Layout::leaf(Size::Fixed(SHEET_W), Size::Fixed(18.0)), Vec2::ZERO, false, move |p, r| {
        // The whole strip is the margin colour; the text column is lighter,
        // which is how a word processor shows where the text may go.
        p.rect(r, margin, 3.0);
        p.rect(Rect::new(r.x + MARGIN_X, r.y, r.w - MARGIN_X * 2.0, r.h), face, 0.0);
        let x0 = r.x + MARGIN_X;
        for i in 0..=6 {
            let x = x0 + i as f32 * inch;
            if i > 0 && i < 6 {
                let n = numbers[i - 1];
                let w = p.measure(size, n).x;
                p.text(Vec2::new(x - w * 0.5, r.y + (r.h - size) * 0.5 - 1.0), size, text, n);
            }
            // A tick halfway between each pair of numbers.
            if i < 6 {
                let h = p.hairline(x + inch * 0.5, r.y + r.h * 0.5 - 2.0, 1.0, 5.0);
                p.rect(h, tick, 0.0);
            }
        }
    });
}

fn sheet(ui: &mut Ui, pad: &mut Pad) {
    let sheet = Layout::column()
        .width(Size::Fixed(SHEET_W))
        .height(Size::Grow(1.0))
        .padding(Insets::xy(MARGIN_X, MARGIN_Y));
    let frame = Frame { fill: PAGE.sheet, radius: 2.0, shadow: true, clip: true, ..Frame::none() };
    ui.container_id(Id::new("sheet"), sheet, frame, |ui| {
        let size = pad.font_size();
        let numbers = pad.line_numbers;
        // The page's own look: ink on paper, and no chrome around the field —
        // on a sheet, a border and a focus ring would be the frame of a box
        // that is not there.
        let r = ui.with_style(
            |t| {
                t.metrics.font_size = size;
                let s = &mut t.text_input;
                s.fill = Color::TRANSPARENT;
                s.border = Color::TRANSPARENT;
                s.border_hover = Color::TRANSPARENT;
                s.border_focus = Color::TRANSPARENT;
                s.focus_ring = Color::TRANSPARENT;
                s.text = PAGE.ink;
                s.placeholder = PAGE.ink_faint;
                s.selection = PAGE.selection;
                s.caret = PAGE.ink;
                s.padding_x = 0.0;
            },
            |ui| {
                let opts = TextAreaOptions { height: Size::Grow(1.0), line_numbers: numbers, ..Default::default() };
                ui.text_area_with("document", &mut pad.text, opts)
            },
        );

        // What only the field knows, kept for the status bar and for the
        // commands that act on the caret's line or the selection.
        pad.caret = r.caret;
        pad.selection = r.selection;
        pad.editing = r.focused;
        if r.changed {
            pad.edited = true;
        }
    });
}
