use crate::{Color, Rect, Transform, Vec2};
use std::ops::Range;

/// Which texture an instance samples. `Atlas` is the glyph atlas (also bound
/// for plain shapes); `User` is anything the host registers, e.g. a 3D viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureId {
    Atlas,
    User(u32),
}

use crate::render_contract::PrimitiveKind;

const KIND_SHAPE: f32 = PrimitiveKind::Shape.code();
const KIND_GLYPH: f32 = PrimitiveKind::Glyph.code();
const KIND_IMAGE: f32 = PrimitiveKind::Image.code();
const KIND_LINE: f32 = PrimitiveKind::Line.code();

/// One GPU instance = one quad. Shapes are rounded rects evaluated as an SDF
/// in the fragment shader, so fills, borders, and shadows are crisp at any DPI.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// x, y, w, h (logical px)
    pub rect: [f32; 4],
    /// u0, v0, u1, v1
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub border_color: [f32; 4],
    /// x0, y0, x1, y1
    pub clip: [f32; 4],
    /// radius, border width, softness, kind
    pub params: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Batch {
    pub texture: TextureId,
    pub range: Range<u32>,
}

#[derive(Default)]
pub struct DrawList {
    pub instances: Vec<Instance>,
    pub batches: Vec<Batch>,
    clips: Vec<Rect>,
    /// Canvas-to-window transforms, innermost last. Every rect and clip that
    /// goes through here is mapped, so nothing can draw untransformed by
    /// accident.
    xforms: Vec<Transform>,
    /// Text pixel-snapping, innermost last. Off inside a scroll area that is
    /// moving, so its text tracks the sub-pixel offset instead of shearing
    /// against the row boxes it sits in.
    snap_text: Vec<bool>,
}

impl DrawList {
    pub(crate) fn clear(&mut self, screen: Rect) {
        self.instances.clear();
        self.batches.clear();
        self.clips.clear();
        self.clips.push(screen);
        self.xforms.clear();
        self.snap_text.clear();
    }

    /// Whether text should snap its baseline to the physical pixel grid here.
    pub fn snap_text(&self) -> bool {
        *self.snap_text.last().unwrap_or(&true)
    }

    pub(crate) fn push_snap_text(&mut self, snap: bool) {
        self.snap_text.push(snap);
    }

    pub(crate) fn pop_snap_text(&mut self) {
        self.snap_text.pop();
    }

    /// Make room for `instances` instances, so a first frame does not grow.
    pub fn reserve(&mut self, instances: usize) {
        self.instances.reserve(instances.saturating_sub(self.instances.capacity()));
        self.batches.reserve(16);
    }

    pub fn clip(&self) -> Rect {
        *self.clips.last().unwrap_or(&Rect::default())
    }

    /// Transform from the current canvas's coordinates to the window.
    pub fn xform(&self) -> Transform {
        self.xforms.last().copied().unwrap_or(Transform::IDENTITY)
    }

    /// Enter a canvas. `t` maps the coordinates used inside it to the window,
    /// and composes with any canvas already entered.
    pub fn push_xform(&mut self, t: Transform) {
        let composed = t.then(self.xform());
        self.xforms.push(composed);
    }

    pub fn pop_xform(&mut self) {
        self.xforms.pop();
    }

    /// `r` is in the current canvas's coordinates.
    pub fn push_clip(&mut self, r: Rect) {
        let r = self.xform().rect(r);
        let c = self.clip().intersect(&r).unwrap_or_default();
        self.clips.push(c);
    }

    pub fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    /// Re-emit a recorded instance exactly as it was, batching as usual.
    /// Already clipped and transformed when it was recorded, so it goes in
    /// untouched — that is the whole point of having kept it.
    pub(crate) fn replay(&mut self, texture: TextureId, inst: Instance) {
        let idx = self.instances.len() as u32;
        self.instances.push(inst);
        match self.batches.last_mut() {
            Some(b) if b.texture == texture => b.range.end = idx + 1,
            _ => self.batches.push(Batch { texture, range: idx..idx + 1 }),
        }
    }

    /// The instances emitted since `from`, with the texture each went to.
    pub(crate) fn since(&self, from: u32) -> impl Iterator<Item = (TextureId, Instance)> + '_ {
        let from = from as usize;
        self.batches.iter().flat_map(move |b| {
            let s = (b.range.start as usize).max(from);
            let e = b.range.end as usize;
            (s..e).map(move |i| (b.texture, self.instances[i]))
        })
    }

    pub(crate) fn instance_count(&self) -> u32 {
        self.instances.len() as u32
    }

    fn push(&mut self, texture: TextureId, bounds: Rect, mut inst: Instance) {
        let clip = self.clip();
        if clip.intersect(&bounds).is_none() {
            return; // culled
        }
        inst.clip = [clip.x, clip.y, clip.right(), clip.bottom()];
        let idx = self.instances.len() as u32;
        self.instances.push(inst);
        match self.batches.last_mut() {
            Some(b) if b.texture == texture => b.range.end = idx + 1,
            _ => self.batches.push(Batch { texture, range: idx..idx + 1 }),
        }
    }

    /// Rounded rectangle with optional border. `r` is in the current canvas's
    /// coordinates; corner radius and border width scale with its zoom.
    pub fn rect(&mut self, r: Rect, fill: Color, radius: f32, border: f32, border_color: Color) {
        let t = self.xform();
        let (r, radius, border) = (t.rect(r), radius * t.zoom, border * t.zoom);
        self.push(
            TextureId::Atlas,
            r.expand(1.0),
            Instance {
                rect: [r.x, r.y, r.w, r.h],
                uv: [0.0; 4],
                color: fill.to_array(),
                border_color: border_color.to_array(),
                clip: [0.0; 4],
                params: [radius, border, 0.0, KIND_SHAPE],
            },
        );
    }

    /// Soft, blurred rounded rect, used for drop shadows and glows.
    pub fn shadow(&mut self, r: Rect, radius: f32, blur: f32, color: Color) {
        let t = self.xform();
        let (r, radius, blur) = (t.rect(r), radius * t.zoom, blur * t.zoom);
        self.push(
            TextureId::Atlas,
            r.expand(blur + 1.0),
            Instance {
                rect: [r.x, r.y, r.w, r.h],
                uv: [0.0; 4],
                color: color.to_array(),
                border_color: [0.0; 4],
                clip: [0.0; 4],
                params: [radius, 0.0, blur, KIND_SHAPE],
            },
        );
    }

    pub(crate) fn glyph(&mut self, r: Rect, uv: [f32; 4], color: Color) {
        let r = self.xform().rect(r);
        self.push(
            TextureId::Atlas,
            r,
            Instance {
                rect: [r.x, r.y, r.w, r.h],
                uv,
                color: color.to_array(),
                border_color: [0.0; 4],
                clip: [0.0; 4],
                params: [0.0, 0.0, 0.0, KIND_GLYPH],
            },
        );
    }

    /// Line segment with round caps, in the current canvas's coordinates.
    ///
    /// Overlapping caps give a round join, so a polyline is simply several of
    /// these. `width` scales with the canvas zoom like every other dimension;
    /// divide by the zoom for a hairline that stays one pixel wide.
    pub fn line(&mut self, a: Vec2, b: Vec2, width: f32, color: Color) {
        let t = self.xform();
        let (a, b) = (t.point(a), t.point(b));
        let hw = (width * t.zoom * 0.5).max(0.05);
        // A segment's quad is its bounding box, which for a long diagonal is
        // enormous next to the line itself: an 800x600 diagonal rasterises
        // ~480k fragments to draw a 2px line, nearly all of them discarded.
        // Splitting it into k strips divides that area by k, and the extra
        // instances cost far less than the fragments they save.
        let (dx, dy) = ((a.x - b.x).abs(), (a.y - b.y).abs());
        let length = dx.hypot(dy);
        let useful = (length * hw * 2.0).max(1.0);
        let k = ((dx * dy) / (4.0 * useful)).ceil().clamp(1.0, 32.0) as usize;
        // Bounding box grown for the width and for anti-aliasing, so the
        // vertex shader needs no padding of its own.
        let pad = hw + 2.0;
        if k == 1 {
            let bounds = Rect::new(a.x.min(b.x) - pad, a.y.min(b.y) - pad, dx + pad * 2.0, dy + pad * 2.0);
            self.segment(a, b, hw, color, bounds);
            return;
        }

        // Strips side by side across the longer axis, each evaluating the
        // distance to the *whole* segment. Every pixel is then drawn exactly
        // once, so the result is the same as one quad. (Splitting into shorter
        // capsules instead overlapped their round caps, and a translucent line
        // showed a darker blob wherever two met.)
        //
        // Neighbouring strips must share a bit-identical edge, or the
        // rasteriser could cover a pixel twice or skip it along the seam. The
        // shader rebuilds each edge as `center ± half`; with edges on a 1/64 px
        // grid that arithmetic is exact (for coordinates under 65536 px), so
        // both strips land on the same value.
        let along_x = dx >= dy;
        let split = |p: Vec2| if along_x { (p.x, p.y) } else { (p.y, p.x) };
        let ((am, an), (bm, bn)) = (split(a), split(b));
        let (lo, hi) = (am.min(bm) - pad, am.max(bm) + pad);
        let edge = |i: usize| -> f32 {
            match i {
                0 => (lo * 64.0).floor() / 64.0,
                _ if i == k => (hi * 64.0).ceil() / 64.0,
                _ => ((lo + (hi - lo) * i as f32 / k as f32) * 64.0).round() / 64.0,
            }
        };
        // The segment's cross-axis position at `m` along it, clamped to its ends.
        let across = |m: f32| an + (bn - an) * ((m - am) / (bm - am)).clamp(0.0, 1.0);
        let mut e0 = edge(0);
        for i in 1..=k {
            let e1 = edge(i);
            if e1 <= e0 {
                continue;
            }
            // Anything within `pad` of the segment and inside this strip lies
            // over the part of the segment within `pad` of the strip.
            let (n0, n1) = (across(e0 - pad), across(e1 + pad));
            let (c0, c1) = (n0.min(n1) - pad, n0.max(n1) + pad);
            let bounds = if along_x { Rect::new(e0, c0, e1 - e0, c1 - c0) } else { Rect::new(c0, e0, c1 - c0, e1 - e0) };
            self.segment(a, b, hw, color, bounds);
            e0 = e1;
        }
    }

    /// One `Line` instance for the segment `a`-`b`, drawn over `bounds`
    /// (already in window coordinates).
    fn segment(&mut self, a: Vec2, b: Vec2, hw: f32, color: Color, bounds: Rect) {
        self.push(
            TextureId::Atlas,
            bounds,
            Instance {
                rect: [bounds.x, bounds.y, bounds.w, bounds.h],
                uv: [a.x, a.y, b.x, b.y],
                color: color.to_array(),
                border_color: [0.0; 4],
                clip: [0.0; 4],
                params: [hw, 0.0, 0.0, KIND_LINE],
            },
        );
    }

    /// Textured quad with rounded-corner mask, e.g. an engine viewport.
    pub fn image(&mut self, r: Rect, texture: TextureId, radius: f32, tint: Color) {
        self.image_uv(r, texture, [0.0, 0.0, 1.0, 1.0], radius, tint);
    }

    /// [`DrawList::image`] showing only part of the texture. `uv` is
    /// `[u0, v0, u1, v1]` in 0..1, with v increasing downwards — a frame from a
    /// thumbnail sheet, one icon from an atlas, a tile from a sprite page.
    pub fn image_uv(&mut self, r: Rect, texture: TextureId, uv: [f32; 4], radius: f32, tint: Color) {
        let t = self.xform();
        let (r, radius) = (t.rect(r), radius * t.zoom);
        self.push(
            texture,
            r,
            Instance {
                rect: [r.x, r.y, r.w, r.h],
                uv,
                color: tint.to_array(),
                border_color: [0.0; 4],
                clip: [0.0; 4],
                params: [radius, 0.0, 0.0, KIND_IMAGE],
            },
        );
    }
}
