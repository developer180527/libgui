use crate::{Color, Rect};
use std::ops::Range;

/// Which texture an instance samples. `Atlas` is the glyph atlas (also bound
/// for plain shapes); `User` is anything the host registers, e.g. a 3D viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureId {
    Atlas,
    User(u32),
}

pub(crate) const KIND_SHAPE: f32 = 0.0;
pub(crate) const KIND_GLYPH: f32 = 1.0;
pub(crate) const KIND_IMAGE: f32 = 2.0;

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
}

impl DrawList {
    pub(crate) fn clear(&mut self, screen: Rect) {
        self.instances.clear();
        self.batches.clear();
        self.clips.clear();
        self.clips.push(screen);
    }

    pub fn clip(&self) -> Rect {
        *self.clips.last().unwrap_or(&Rect::default())
    }

    pub fn push_clip(&mut self, r: Rect) {
        let c = self.clip().intersect(&r).unwrap_or_default();
        self.clips.push(c);
    }

    pub fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
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

    /// Rounded rectangle with optional border.
    pub fn rect(&mut self, r: Rect, fill: Color, radius: f32, border: f32, border_color: Color) {
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

    /// Textured quad with rounded-corner mask, e.g. an engine viewport.
    pub fn image(&mut self, r: Rect, texture: TextureId, radius: f32, tint: Color) {
        self.push(
            texture,
            r,
            Instance {
                rect: [r.x, r.y, r.w, r.h],
                uv: [0.0, 0.0, 1.0, 1.0],
                color: tint.to_array(),
                border_color: [0.0; 4],
                clip: [0.0; 4],
                params: [radius, 0.0, 0.0, KIND_IMAGE],
            },
        );
    }
}
