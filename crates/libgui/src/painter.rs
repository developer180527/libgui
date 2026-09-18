use crate::{Color, DrawList, FontId, Fonts, Rect, TextureId, Theme, Vec2};

/// Handed to paint callbacks after layout is solved. This is the
/// "immediate" drawing layer: widgets and custom overlays (gizmo labels, graphs,
/// debug text) draw here with final rects.
pub struct Painter<'a> {
    pub draw: &'a mut DrawList,
    pub fonts: &'a mut Fonts,
    pub theme: &'a Theme,
    pub font: FontId,
}

impl Painter<'_> {
    pub fn rect(&mut self, r: Rect, fill: Color, radius: f32) {
        self.draw.rect(r, fill, radius, 0.0, Color::TRANSPARENT);
    }

    pub fn rect_bordered(&mut self, r: Rect, fill: Color, radius: f32, border: f32, border_color: Color) {
        self.draw.rect(r, fill, radius, border, border_color);
    }

    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, color: Color) {
        self.draw.shadow(r, radius, blur, color);
    }

    pub fn image(&mut self, r: Rect, tex: TextureId, radius: f32) {
        self.draw.image(r, tex, radius, Color::WHITE);
    }

    pub fn measure(&self, size: f32, text: &str) -> Vec2 {
        self.fonts.measure(self.font, size, text)
    }

    pub fn text(&mut self, pos: Vec2, size: f32, color: Color, text: &str) {
        self.fonts.draw(self.draw, self.font, size, pos, color, text);
    }

    /// Left-aligned, vertically centred in `r`.
    pub fn text_left(&mut self, r: Rect, size: f32, color: Color, text: &str) {
        let m = self.measure(size, text);
        self.text(Vec2::new(r.x, r.y + (r.h - m.y) * 0.5), size, color, text);
    }

    /// Right-aligned, vertically centred in `r`.
    pub fn text_right(&mut self, r: Rect, size: f32, color: Color, text: &str) {
        let m = self.measure(size, text);
        self.text(Vec2::new(r.right() - m.x, r.y + (r.h - m.y) * 0.5), size, color, text);
    }

    pub fn text_centered(&mut self, r: Rect, size: f32, color: Color, text: &str) {
        let m = self.measure(size, text);
        let c = r.center();
        self.text(Vec2::new(c.x - m.x * 0.5, c.y - m.y * 0.5), size, color, text);
    }
}
