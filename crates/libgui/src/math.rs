use std::ops::{Add, AddAssign, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}

/// Axis-aligned rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn size(&self) -> Vec2 {
        Vec2::new(self.w, self.h)
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn shrink(&self, l: f32, t: f32, r: f32, b: f32) -> Rect {
        Rect::new(self.x + l, self.y + t, (self.w - l - r).max(0.0), (self.h - t - b).max(0.0))
    }

    pub fn expand(&self, v: f32) -> Rect {
        Rect::new(self.x - v, self.y - v, self.w + 2.0 * v, self.h + 2.0 * v)
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// Straight-alpha colour in sRGB space, components 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgba(0.0, 0.0, 0.0, 1.0);

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// `0xRRGGBB`
    pub const fn hex(v: u32) -> Self {
        Self::rgba(
            ((v >> 16) & 0xff) as f32 / 255.0,
            ((v >> 8) & 0xff) as f32 / 255.0,
            (v & 0xff) as f32 / 255.0,
            1.0,
        )
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self::rgba(self.r, self.g, self.b, a)
    }

    pub fn lerp(self, o: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        Color::rgba(
            self.r + (o.r - self.r) * t,
            self.g + (o.g - self.g) * t,
            self.b + (o.b - self.b) * t,
            self.a + (o.a - self.a) * t,
        )
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// Pan and uniform zoom: the mapping between a canvas's own coordinates and
/// the window. There is no rotation — the shader draws axis-aligned quads.
///
/// `window = canvas * zoom + pan`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform { pan: Vec2::ZERO, zoom: 1.0 };

    pub fn new(pan: Vec2, zoom: f32) -> Self {
        Self { pan, zoom: if zoom.is_finite() && zoom > 1e-6 { zoom } else { 1e-6 } }
    }

    pub fn is_identity(&self) -> bool {
        self.zoom == 1.0 && self.pan == Vec2::ZERO
    }

    pub fn point(&self, p: Vec2) -> Vec2 {
        Vec2::new(p.x * self.zoom + self.pan.x, p.y * self.zoom + self.pan.y)
    }

    pub fn inv_point(&self, p: Vec2) -> Vec2 {
        Vec2::new((p.x - self.pan.x) / self.zoom, (p.y - self.pan.y) / self.zoom)
    }

    pub fn rect(&self, r: Rect) -> Rect {
        Rect::new(r.x * self.zoom + self.pan.x, r.y * self.zoom + self.pan.y, r.w * self.zoom, r.h * self.zoom)
    }

    pub fn inv_rect(&self, r: Rect) -> Rect {
        Rect::new((r.x - self.pan.x) / self.zoom, (r.y - self.pan.y) / self.zoom, r.w / self.zoom, r.h / self.zoom)
    }

    /// `self` then `outer`.
    pub fn then(&self, outer: Transform) -> Transform {
        Transform {
            pan: Vec2::new(self.pan.x * outer.zoom + outer.pan.x, self.pan.y * outer.zoom + outer.pan.y),
            zoom: self.zoom * outer.zoom,
        }
    }
}
