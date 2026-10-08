use crate::{Axis, Color, DrawList, FontId, Fonts, Rect, TextureId, Theme, Vec2, PaintText};

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
        let Some((page, uv)) = self.fonts.coverage_mask(key, w, h, |buf| path.rasterize(w, h, buf)) else { return };
        // Back into the coordinates the draw list expects; it applies the
        // transform itself.
        self.draw.glyph(t.inv_rect(window), page, uv, color);
    }

    /// A linear gradient from `from` to `to` across `r`: left to right for
    /// [`Axis::X`], top to bottom for [`Axis::Y`].
    ///
    /// Built from what every backend already draws, so nothing new is asked
    /// of one: `from` is a plain fill, and `to` is laid over it through a ramp
    /// of coverage in the glyph atlas — 0 at the start, 1 at the end — drawn
    /// the way text is. Over an opaque `from` that is exactly
    /// `from·(1−t) + to·t`. The ramp is 256 texels, rasterised once and
    /// stretched to any size by the same bilinear filter that samples glyphs,
    /// so a gradient costs two instances and no allocation.
    ///
    /// **Exact when `from` is opaque or fully transparent.** A transparent
    /// `from` gives `to` fading in over whatever is beneath — an alpha strip
    /// over a checkerboard. A `from` that is partly transparent is drawn
    /// first and then covered, which is close to an interpolation in alpha
    /// but not equal to one.
    ///
    /// Square corners: the ramp has no rounded edge to match a radius. Outside
    /// a canvas the rect is snapped to whole physical pixels, so the fill and
    /// the ramp over it end at the same pixel.
    pub fn gradient(&mut self, r: Rect, from: Color, to: Color, axis: Axis) {
        let r = if self.draw.xform().is_identity() { self.snap_rect(r) } else { r };
        if !(r.w > 0.0 && r.h > 0.0) {
            return;
        }
        if from.a > 0.0 {
            self.rect(r, from, 0.0);
        }
        if to.a <= 0.0 {
            return;
        }
        const N: u32 = 256;
        // Fixed keys: the ramp is the same at every size and in every window
        // that shares this atlas. Hashed from a name so they cannot collide
        // with a path's keys by accident.
        let (w, h, key) = match axis {
            Axis::X => (N, 1, ramp_key("gradient_ramp_x")),
            Axis::Y => (1, N, ramp_key("gradient_ramp_y")),
        };
        let fill = |buf: &mut Vec<u8>| {
            for (i, c) in buf.iter_mut().enumerate() {
                *c = ((i as u32 * 255 + (N - 1) / 2) / (N - 1)) as u8;
            }
        };
        let Some((page, uv)) = self.fonts.coverage_mask(key, w, h, fill) else { return };
        // Centre of the first texel to centre of the last, and the middle of
        // the one texel across: the ends are exactly 0 and 1, and bilinear
        // sampling never reaches a neighbour in the atlas.
        let (du, dv) = ((uv[2] - uv[0]) / w as f32, (uv[3] - uv[1]) / h as f32);
        let (u0, v0) = (uv[0] + du * 0.5, uv[1] + dv * 0.5);
        let uv = match axis {
            Axis::X => [u0, v0, uv[2] - du * 0.5, v0],
            Axis::Y => [u0, v0, u0, uv[3] - dv * 0.5],
        };
        self.draw.glyph(r, page, uv, to);
    }

    /// A filled polygon: `points` in order, either winding, concave or not.
    /// Edges are anti-aliased; the triangles inside it meet exactly, so a
    /// translucent fill has no seams. Drawn as triangles every call — nothing
    /// is rasterised on the CPU or cached in the atlas — so it is for shapes
    /// that change (a sketch region being dragged, an area chart, a selection
    /// lasso); for a fixed icon, [`Painter::fill_path`] is cheaper per frame.
    ///
    /// Simple polygons: an outline that crosses itself draws something, but
    /// not a defined fill rule.
    pub fn fill_polygon(&mut self, points: &[Vec2], color: Color) {
        self.fill_polygon_with_holes(points, &[], color);
    }

    /// [`Painter::fill_polygon`] with `holes` cut out of it.
    pub fn fill_polygon_with_holes(&mut self, outline: &[Vec2], holes: &[&[Vec2]], color: Color) {
        if color.a <= 0.0 {
            return;
        }
        let mut tris = Vec::new();
        crate::tess::triangulate(outline, holes, &mut tris);
        for (a, b, c, edges) in tris {
            self.draw.triangle(a, b, c, edges, color);
        }
    }

    /// Triangles you have already made — a CAD kernel's face, a mesh from a
    /// file: `indices` in threes into `points`. An edge used by one triangle
    /// is the outline and anti-aliased; an edge two triangles share is drawn
    /// exactly, so they meet with no seam.
    pub fn fill_mesh(&mut self, points: &[Vec2], indices: &[u32], color: Color) {
        if color.a <= 0.0 {
            return;
        }
        let key = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
        let mut uses: crate::hash::FxMap<(u32, u32), u8> = Default::default();
        let tris = indices.chunks_exact(3).filter(|t| t.iter().all(|&i| (i as usize) < points.len()));
        for t in tris.clone() {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *uses.entry(key(a, b)).or_insert(0) += 1;
            }
        }
        for t in tris {
            let edge = |a: u32, b: u32| (uses.get(&key(a, b)) == Some(&1)) as u8;
            let bits = edge(t[0], t[1]) | (edge(t[1], t[2]) << 1) | (edge(t[2], t[0]) << 2);
            let p = |i: u32| points[i as usize];
            self.draw.triangle(p(t[0]), p(t[1]), p(t[2]), bits, color);
        }
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

    /// A dashed straight line: `dash.on` drawn, `dash.off` skipped, from
    /// `dash.phase` into the pattern at `a`. Dash ends are square. One
    /// instance, like a solid line — the gaps are cut in the shader.
    ///
    /// ```ignore
    /// p.dashed_line(a, b, 1.0, ink, Dash::even(4.0));          // construction line
    /// p.dashed_line(a, b, 2.0, ink, Dash::dotted(2.0));        // dotted
    /// p.dashed_line(a, b, 1.0, ink, Dash::even(4.0).phase(t * 20.0)); // marching ants
    /// ```
    pub fn dashed_line(&mut self, a: Vec2, b: Vec2, width: f32, color: Color, dash: crate::Dash) {
        self.draw.dashed_line(a, b, width, color, dash);
    }

    /// [`Painter::polyline`] dashed, with the pattern running on across the
    /// joins rather than restarting at each point. Returns the phase the last
    /// point ends at, to continue the pattern into another call.
    pub fn dashed_polyline(&mut self, points: &[Vec2], width: f32, color: Color, dash: crate::Dash) -> f32 {
        let mut phase = dash.phase;
        for w in points.windows(2) {
            self.draw.dashed_line(w[0], w[1], width, color, dash.phase(phase));
            phase += (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
        }
        phase
    }

    /// An image turned by `radians` about the centre of `r`, clockwise on
    /// screen: a knob's pointer, a compass needle, a spinner. `r` is the
    /// upright rect; the turned image may reach outside it. The rounded
    /// corners turn with the image.
    #[allow(clippy::too_many_arguments)]
    pub fn image_rotated(
        &mut self,
        r: Rect,
        tex: TextureId,
        uv: [f32; 4],
        radius: f32,
        tint: Color,
        alpha: crate::ImageAlpha,
        radians: f32,
    ) {
        self.draw.image_rotated(r, tex, uv, radius, tint, alpha, crate::Rotation::new(radians));
    }

    /// One line of text centred on `center` and turned by `radians` about it,
    /// clockwise on screen: a vertical axis label (`-π/2` reads bottom to
    /// top), a dimension written along the line it measures.
    ///
    /// ```ignore
    /// // Along a line from a to b, readable from below:
    /// let mut angle = (b.y - a.y).atan2(b.x - a.x);
    /// if angle.abs() > FRAC_PI_2 { angle += PI; }
    /// p.text_rotated((a + b) * 0.5, 12.0, ink, "42.0 mm", angle);
    /// ```
    ///
    /// Upright text is snapped to the pixel grid; turned text cannot be, so it
    /// is a shade softer. At an angle of exactly zero this is
    /// [`Painter::text`] centred, and as crisp.
    pub fn text_rotated(&mut self, center: Vec2, size: f32, color: Color, text: impl PaintText, radians: f32) {
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        let rot = crate::Rotation::new(radians);
        if rot.is_none() {
            let m = self.fonts.measure(self.font, size, s);
            self.fonts.draw(self.draw, self.font, size, Vec2::new(center.x - m.x * 0.5, center.y - m.y * 0.5), color, s);
            return;
        }
        self.fonts.draw_rotated(self.draw, self.font, size, center, rot, color, s);
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

    /// The byte range `row` of a bidi `line`, its left edge at `pos`: a
    /// wrapped row laid out as part of its paragraph, so its direction is the
    /// paragraph's.
    pub(crate) fn text_in(&mut self, pos: Vec2, size: f32, color: Color, line: impl PaintText, row: (usize, usize)) {
        let arena: &'a [u8] = self.strs;
        let s = line.get(arena);
        self.fonts.draw_in(self.draw, self.font, size, pos, color, s, row);
    }

    /// Left-aligned, vertically centred in `r`.
    pub fn text_left(&mut self, r: Rect, size: f32, color: Color, text: impl PaintText) {
        let arena: &'a [u8] = self.strs;
        let s = text.get(arena);
        let m = self.fonts.measure(self.font, size, s);
        self.fonts.draw(self.draw, self.font, size, Vec2::new(r.x, r.y + (r.h - m.y) * 0.5), color, s);
    }

    /// A label at the left of `r` and a value at its right, as sliders and
    /// fields show them, never overlapping. The value is shown in full; the
    /// label gets the room left, cut short with "…" when it does not fit, and
    /// is left out when not even that does. Nothing is allocated.
    #[allow(clippy::too_many_arguments)]
    pub fn label_value(&mut self, r: Rect, size: f32, label_color: Color, label: impl PaintText, value_color: Color, value: impl PaintText) {
        let arena: &'a [u8] = self.strs;
        let (label, value) = (label.get(arena), value.get(arena));
        let font = self.font;
        let vw = self.fonts.measure(font, size, value).x;
        self.text_right(r, size, value_color, value);
        if label.is_empty() {
            return;
        }
        let room = r.w - vw - size * 0.5;
        let m = self.fonts.measure(font, size, label);
        let y = r.y + (r.h - m.y) * 0.5;
        if m.x <= room {
            self.fonts.draw(self.draw, font, size, Vec2::new(r.x, y), label_color, label);
            return;
        }
        const MORE: &str = "…";
        let room = room - self.fonts.measure(font, size, MORE).x;
        // The longest prefix that fits, by bisection over char boundaries.
        let ends: (usize, usize) = (0, label.chars().count());
        let (mut lo, mut hi) = ends;
        let at = |n: usize| label.char_indices().nth(n).map_or(label.len(), |(b, _)| b);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.fonts.measure(font, size, &label[..at(mid)]).x <= room {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        if lo == 0 {
            return;
        }
        let head = label[..at(lo)].trim_end();
        let w = self.fonts.measure(font, size, head).x;
        self.fonts.draw(self.draw, font, size, Vec2::new(r.x, y), label_color, head);
        self.fonts.draw(self.draw, font, size, Vec2::new(r.x + w, y), label_color, MORE);
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

fn ramp_key(name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = crate::id::StableHasher::new();
    name.hash(&mut h);
    h.finish()
}
