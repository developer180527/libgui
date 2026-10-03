use crate::{Color, DrawList, FontId, Fonts, Rect, TextureId, Theme, Vec2, PaintText};

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
    /// This frame's text arena, which a [`FrameText`](crate::FrameText) names a range in.
    pub strs: &'a [u8],
    /// Physical px per logical px, for [`Painter::hairline`].
    pub scale: f32,
}

impl<'a> Painter<'a> {
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

    /// An image whose own alpha counts: an icon, a decal, anything that is
    /// not a rectangle all the way to its edges. `tint` multiplies it, so one
    /// white icon serves every state.
    ///
    /// For a PNG as loaded, [`ImageAlpha::Straight`](crate::ImageAlpha::Straight).
    /// If your renderer filters in hardware, premultiply on upload and pass
    /// [`ImageAlpha::Premultiplied`](crate::ImageAlpha::Premultiplied)
    /// instead, or every edge gets a dark halo — see `ImageAlpha`.
    pub fn image_with_alpha(
        &mut self,
        r: Rect,
        tex: TextureId,
        uv: [f32; 4],
        radius: f32,
        tint: Color,
        alpha: crate::ImageAlpha,
    ) {
        self.draw.image_alpha(r, tex, uv, radius, tint, alpha);
    }

    /// Fill `path`, scaled from its view box into `r`, in `color`.
    ///
    /// For icons and other static vector art. The outline is rasterised on
    /// the CPU at the size it is shown — through any canvas zoom — into an
    /// 8-bit coverage mask in the glyph atlas, and drawn the way text is: so
    /// it is anti-aliased by exact area, tinted by `color`, crisp at every
    /// DPI, rasterised **once** per size and reused every frame after, and
    /// needs nothing from a backend beyond what text already needs.
    ///
    /// A shape that changes every frame misses that cache every frame; it
    /// still draws, but it is not what this is for.
    ///
    /// The rect is snapped to the pixel grid, which is what lets one
    /// rasterisation serve every frame: a path at a fractional position would
    /// be a different raster each time it moved by a fraction.
    pub fn fill_path(&mut self, path: &crate::Path, r: Rect, color: Color) {
        if path.is_empty() || color.a <= 0.0 {
            return;
        }
        // The size it is actually shown at: through the canvas transform, then
        // to physical pixels.
        let t = self.draw.xform();
        let (window, w, h) = crate::path::pixel_box(t.rect(r), self.scale);
        if w == 0 || h == 0 {
            return;
        }
        let key = path.key(w, h);
        let Some(uv) = self.fonts.coverage_mask(key, w, h, |buf| path.rasterize(w, h, buf)) else { return };
        // Back into the coordinates the draw list expects; it applies the
        // transform itself.
        self.draw.glyph(t.inv_rect(window), uv, color);
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

    /// A rule `px` physical pixels wide, snapped to the pixel grid: the rect
    /// you would draw for a 1px line is 1.5 physical px at a 1.5x scale, and a
    /// blurry line is not a hairline. Returns the rect to draw.
    ///
    /// Grid rules, separators and column edges should go through this; a
    /// border that is part of a shape should not, because rounding it
    /// separately would part it from the shape.
    pub fn hairline(&self, x: f32, y: f32, px: f32, height: f32) -> Rect {
        let s = self.scale.max(0.01);
        let w = (px.max(1.0)).round() / s;
        Rect::new((x * s).round() / s, y, w, height)
    }

    /// `r` snapped out to whole physical pixels: a solid or translucent fill
    /// that should have a hard edge rather than an antialiased one. A row
    /// background, a selection band, a ruler cell — anything whose edge the
    /// eye reads as a boundary rather than as part of a shape.
    pub fn snap_rect(&self, r: Rect) -> Rect {
        let s = self.scale.max(0.01);
        let (x, y) = ((r.x * s).round(), (r.y * s).round());
        let (x1, y1) = (((r.x + r.w) * s).round(), ((r.y + r.h) * s).round());
        Rect::new(x / s, y / s, (x1 - x) / s, (y1 - y) / s)
    }

    pub fn measure(&self, size: f32, text: impl PaintText) -> Vec2 {
        self.fonts.measure(self.font, size, text.get(self.strs))
    }

    /// `text` is a `&str`, a `String`, or a [`FrameText`](crate::FrameText) handle into the
    /// frame's arena — which is what the built-in widgets pass, because it
    /// costs no allocation to carry one into a paint closure.
    pub fn text(&mut self, pos: Vec2, size: f32, color: Color, text: impl PaintText) {
        // The arena is borrowed from the frame, not from `self`, so resolving
        // first leaves `&mut self` free for the draw.
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        self.fonts.draw(self.draw, self.font, size, pos, color, s);
    }

    /// Left-aligned, vertically centred in `r`.
    pub fn text_left(&mut self, r: Rect, size: f32, color: Color, text: impl PaintText) {
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        let m = self.fonts.measure(self.font, size, s);
        self.fonts.draw(self.draw, self.font, size, Vec2::new(r.x, r.y + (r.h - m.y) * 0.5), color, s);
    }

    /// Right-aligned, vertically centred in `r`.
    pub fn text_right(&mut self, r: Rect, size: f32, color: Color, text: impl PaintText) {
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        let m = self.fonts.measure(self.font, size, s);
        let pos = Vec2::new(r.right() - m.x, r.y + (r.h - m.y) * 0.5);
        self.fonts.draw(self.draw, self.font, size, pos, color, s);
    }

    /// Text wrapped to `r`'s width, laid out from its top. Lines break where
    /// [`crate::Ui::paragraph`] would break them: the wrapping is cached, so
    /// measuring and drawing the same paragraph costs one pass, not two.
    pub fn text_wrapped(&mut self, r: Rect, size: f32, color: Color, align: crate::Align, text: impl PaintText) {
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        self.fonts.draw_wrapped(self.draw, self.font, size, r, color, align, s);
    }

    pub fn text_centered(&mut self, r: Rect, size: f32, color: Color, text: impl PaintText) {
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        let m = self.fonts.measure(self.font, size, s);
        let c = r.center();
        self.fonts.draw(self.draw, self.font, size, Vec2::new(c.x - m.x * 0.5, c.y - m.y * 0.5), color, s);
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
