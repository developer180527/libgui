use crate::{Color, DrawList, FontId, Fonts, Rect, TextureId, Theme, Vec2};

/// Handed to paint callbacks after layout is solved. This is the
/// "immediate" drawing layer: widgets and custom overlays (gizmo labels, graphs,
/// debug text) draw here with final rects.
/// Which way a [`Painter::chevron`] points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chevron {
    Up,
    Down,
    Left,
    Right,
}

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

    /// Part of a texture: `uv` is `[u0, v0, u1, v1]` in 0..1, v downwards.
    /// A video thumbnail sheet, an icon atlas, a sprite page.
    pub fn image_uv(&mut self, r: Rect, tex: TextureId, uv: [f32; 4], radius: f32) {
        self.draw.image_uv(r, tex, uv, radius, Color::WHITE);
    }

    /// A tinted image, for icons drawn from a single-channel or white sheet.
    pub fn image_tinted(&mut self, r: Rect, tex: TextureId, uv: [f32; 4], radius: f32, tint: Color) {
        self.draw.image_uv(r, tex, uv, radius, tint);
    }

    /// Straight line with round caps.
    pub fn line(&mut self, a: Vec2, b: Vec2, width: f32, color: Color) {
        self.draw.line(a, b, width, color);
    }

    /// Connected line segments. Round caps make the joins round for free.
    pub fn polyline(&mut self, points: &[Vec2], width: f32, color: Color) {
        for w in points.windows(2) {
            self.draw.line(w[0], w[1], width, color);
        }
    }

    /// Cubic bezier, flattened to segments. The number of segments follows the
    /// curve's size *on screen*, so it stays smooth when zoomed in and does not
    /// waste instances when zoomed out.
    pub fn bezier(&mut self, p0: Vec2, c0: Vec2, c1: Vec2, p1: Vec2, width: f32, color: Color) {
        let n = Self::bezier_steps(p0, c0, c1, p1, self.draw.xform().zoom);
        let mut prev = p0;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let p = cubic(p0, c0, c1, p1, t);
            self.draw.line(prev, p, width, color);
            prev = p;
        }
    }

    /// A left-to-right wire between two points, the shape a node graph uses:
    /// the tangents leave horizontally, so it reads as a cable.
    pub fn wire(&mut self, from: Vec2, to: Vec2, width: f32, color: Color) {
        let dx = ((to.x - from.x).abs() * 0.5).max(24.0);
        self.bezier(from, Vec2::new(from.x + dx, from.y), Vec2::new(to.x - dx, to.y), to, width, color);
    }

    fn bezier_steps(p0: Vec2, c0: Vec2, c1: Vec2, p1: Vec2, zoom: f32) -> usize {
        let len = |a: Vec2, b: Vec2| (b.x - a.x).hypot(b.y - a.y);
        // Control polygon length is an upper bound on the arc length.
        let screen_len = (len(p0, c0) + len(c0, c1) + len(c1, p1)) * zoom;
        (screen_len.max(1.0).sqrt() * 1.2) as usize + 2
    }

    /// A small open arrow (⌄ ›) centred in `r`, sized for text of `size`:
    /// disclosure triangles, combo boxes, submenus.
    ///
    /// Drawn as two strokes rather than a glyph, so it never depends on the
    /// font having arrow characters (Inter, for one, has none of ▾▸). The two
    /// strokes overlap at the tip, so a translucent colour is slightly
    /// stronger there.
    pub fn chevron(&mut self, r: Rect, size: f32, dir: Chevron, color: Color) {
        let c = r.center();
        // Half the span across the arrow, and how far it points.
        let half = size * 0.26;
        let depth = size * 0.14;
        let stroke = (size * 0.11).max(1.0);
        let (a, tip, b) = match dir {
            Chevron::Down => (
                Vec2::new(c.x - half, c.y - depth),
                Vec2::new(c.x, c.y + depth),
                Vec2::new(c.x + half, c.y - depth),
            ),
            Chevron::Up => (
                Vec2::new(c.x - half, c.y + depth),
                Vec2::new(c.x, c.y - depth),
                Vec2::new(c.x + half, c.y + depth),
            ),
            Chevron::Right => (
                Vec2::new(c.x - depth, c.y - half),
                Vec2::new(c.x + depth, c.y),
                Vec2::new(c.x - depth, c.y + half),
            ),
            Chevron::Left => (
                Vec2::new(c.x + depth, c.y - half),
                Vec2::new(c.x - depth, c.y),
                Vec2::new(c.x + depth, c.y + half),
            ),
        };
        self.line(a, tip, stroke, color);
        self.line(tip, b, stroke, color);
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

fn cubic(p0: Vec2, c0: Vec2, c1: Vec2, p1: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Vec2::new(
        p0.x * a + c0.x * b + c1.x * c + p1.x * d,
        p0.y * a + c0.y * b + c1.y * c + p1.y * d,
    )
}
