//! What the picker's square shows is what clicking it gives you.
//!
//! The square is drawn with two gradients and the picked colour is computed
//! with HSV arithmetic; if those disagreed, a user would click one colour and
//! get another. So at a grid of points this renders the square, reads the
//! pixel, clicks there, and compares.

use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

fn frame(ui: &mut Ui, color: &mut Color, render: bool) -> Option<(libgui_soft::Target, Rect)> {
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(300.0, 320.0), scale: 1.0, dt: 1.0 / 60.0 });
    ui.container(Layout::column().width(Size::Fixed(240.0)).height(Size::Fit), Frame::none(), |ui| {
        ui.color_picker_with("c", color, ColorPickerOptions { alpha: false, ..Default::default() });
    });
    let out = ui.end_frame();
    if !render {
        return None;
    }
    let sq = out.draw.instances.iter().find(|i| i.rect[3] == 160.0 && i.color == Color::WHITE.to_array())?.rect;
    Some((SoftRenderer::new().render_to_image(&out, 300, 320), Rect::new(sq[0], sq[1], sq[2], sq[3])))
}

#[test]
fn the_square_shows_the_colour_a_click_picks() {
    for start in [Color::rgba(1.0, 0.0, 0.0, 1.0), Color::rgba(0.1, 0.6, 0.9, 1.0), Color::rgba(0.9, 0.8, 0.1, 1.0)] {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut color = start;
        for _ in 0..3 {
            frame(&mut ui, &mut color, false);
        }
        let mut worst = 0u8;
        for (fx, fy) in [(0.2, 0.2), (0.5, 0.5), (0.8, 0.3), (0.3, 0.8), (0.9, 0.9), (0.6, 0.1)] {
            // Draw the square with the handle out of the way, read the pixel.
            let (img, sq) = frame(&mut ui, &mut color, true).expect("no square");
            let at = Vec2::new((sq.x + sq.w * fx).floor() + 0.5, (sq.y + sq.h * fy).floor() + 0.5);
            let shown = img.pixel(at.x as u32, at.y as u32);
            // Click exactly there.
            ui.push(InputEvent::PointerMoved { pos: at });
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
            frame(&mut ui, &mut color, false);
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
            frame(&mut ui, &mut color, false);
            let picked = color.to_array().map(|v| (v * 255.0).round() as u8);
            for c in 0..3 {
                worst = worst.max(shown[c].abs_diff(picked[c]));
            }
            // Move the handle away again before the next read.
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(sq.x + 2.0, sq.y + 2.0) });
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
            frame(&mut ui, &mut color, false);
            ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
            frame(&mut ui, &mut color, false);
        }
        assert!(worst <= 3, "from {start:?}: the square showed one colour and a click picked another, {worst} steps apart");
    }
}
