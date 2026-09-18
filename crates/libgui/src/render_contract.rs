//! The renderer contract: everything a backend (GPU or CPU) must agree on
//! with libgui to draw its output correctly. It is the single source of truth:
//! `libgui_shaders` generates the shader's constants from this module and
//! checks the shader's inputs and bindings against it at build time, so the
//! core and the shader cannot drift apart. A backend with a hand-written
//! shader should check [`CONTRACT_VERSION`] and use these values too.
//!
//! # Per frame
//! - [`crate::Globals`] (16 bytes) at group [`GLOBALS_GROUP`], binding [`GLOBALS_BINDING`].
//! - One instance buffer of [`crate::Instance`] ([`INSTANCE_STRIDE`] bytes each,
//!   attributes in [`INSTANCE_ATTRIBUTES`]), step rate *instance*.
//! - Per [`crate::Batch`]: bind its texture at group [`TEXTURE_GROUP`], binding
//!   [`TEXTURE_BINDING`], then draw [`VERTICES_PER_INSTANCE`] vertices (triangle
//!   list, no index buffer) × the batch's instance range.
//!
//! # Pixels
//! - Positions are logical px; the vertex shader divides by `Globals::screen_size`.
//! - Colours are **sRGB-encoded, straight alpha** floats; the shader outputs
//!   **premultiplied** colour, blended with [`BLEND`]. Render into a UNORM (not
//!   sRGB) target, or convert in a later pass. No depth test, no culling.
//! - Shapes and lines do not read a texture, so the atlas may stay bound.
//! - The glyph atlas is [`ATLAS_FORMAT`] coverage; user textures
//!   (`TextureId::User`) are [`USER_TEXTURE_FORMAT`] and composite as opaque RGB.
//! - Textures are read with texel loads and filtered in the shader: no sampler.

use crate::Instance;

/// Bumped whenever anything in this module changes meaning.
pub const CONTRACT_VERSION: u32 = 2;

/// What an instance draws, stored in `Instance::params[3]` as a float code.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimitiveKind {
    /// Rounded-rect SDF: fills, borders, soft shadows.
    /// `params = [corner radius, border width, softness/blur, kind]`.
    Shape = 0,
    /// Glyph: atlas coverage (`.r`) × colour. `uv` = atlas rect (0..1).
    Glyph = 1,
    /// Image with a rounded-corner mask: texture `.rgb` × colour.
    /// `params = [corner radius, 0, 0, kind]`, `uv` = texture rect (0..1).
    Image = 2,
    /// Line segment with round caps, evaluated as a capsule SDF.
    /// `uv` = `[x0, y0, x1, y1]`, the endpoints in the same space as `rect`;
    /// `params = [half width, 0, 0, kind]`. `rect` is the segment's bounding
    /// box, already grown for the width and for anti-aliasing.
    ///
    /// Polylines and curves are many of these: overlapping round caps make a
    /// round join, so no join geometry is needed. (Overlap double-blends where
    /// two segments meet, which shows only on translucent strokes.)
    Line = 3,
}

impl PrimitiveKind {
    pub const ALL: [PrimitiveKind; 4] =
        [PrimitiveKind::Shape, PrimitiveKind::Glyph, PrimitiveKind::Image, PrimitiveKind::Line];

    /// Value stored in `Instance::params[3]`.
    pub const fn code(self) -> f32 {
        self as u32 as f32
    }

    /// Decode `Instance::params[3]`.
    pub fn from_code(code: f32) -> Option<PrimitiveKind> {
        match code.round() as i64 {
            0 => Some(PrimitiveKind::Shape),
            1 => Some(PrimitiveKind::Glyph),
            2 => Some(PrimitiveKind::Image),
            3 => Some(PrimitiveKind::Line),
            _ => None,
        }
    }

    /// Name of the WGSL constant for this kind.
    pub const fn shader_name(self) -> &'static str {
        match self {
            PrimitiveKind::Shape => "KIND_SHAPE",
            PrimitiveKind::Glyph => "KIND_GLYPH",
            PrimitiveKind::Image => "KIND_IMAGE",
            PrimitiveKind::Line => "KIND_LINE",
        }
    }
}

/// Instance attributes in location order: `(location, name, byte offset)`.
/// Every attribute is four 32-bit floats.
pub const INSTANCE_ATTRIBUTES: [(u32, &str, usize); 6] = [
    (0, "rect", 0),
    (1, "uv", 16),
    (2, "color", 32),
    (3, "border_color", 48),
    (4, "clip", 64),
    (5, "params", 80),
];

/// Byte size of one [`Instance`]; also the instance buffer stride.
pub const INSTANCE_STRIDE: usize = 96;
const _: () = assert!(std::mem::size_of::<Instance>() == INSTANCE_STRIDE);

/// Vertices per instance (two triangles, generated in the vertex shader).
pub const VERTICES_PER_INSTANCE: u32 = 6;

pub const GLOBALS_GROUP: u32 = 0;
pub const GLOBALS_BINDING: u32 = 0;
/// Byte size of the uniform block ([`crate::Globals`]).
pub const GLOBALS_SIZE: usize = 16;
const _: () = assert!(std::mem::size_of::<crate::Globals>() == GLOBALS_SIZE);

pub const TEXTURE_GROUP: u32 = 1;
pub const TEXTURE_BINDING: u32 = 0;

pub const VERTEX_ENTRY: &str = "vs_main";
pub const FRAGMENT_ENTRY: &str = "fs_main";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// `out = src + dst * (1 - src.a)` for colour and alpha.
    PremultipliedAlpha,
}

pub const BLEND: Blend = Blend::PremultipliedAlpha;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexelFormat {
    /// One 8-bit unorm channel.
    R8Unorm,
    /// Four 8-bit unorm channels, sRGB-encoded values (not an sRGB view).
    Rgba8Unorm,
}

pub const ATLAS_FORMAT: TexelFormat = TexelFormat::R8Unorm;
pub const USER_TEXTURE_FORMAT: TexelFormat = TexelFormat::Rgba8Unorm;

/// Whether the render target should be an sRGB view. libgui's colours are
/// already sRGB-encoded, so the target is UNORM.
pub const TARGET_IS_SRGB: bool = false;

/// WGSL declarations of the contract constants a shader uses. `libgui_shaders`
/// prepends this to `ui.wgsl`.
pub fn wgsl_prelude() -> String {
    let mut s = format!(
        "// Generated from libgui::render_contract. Do not edit.\nconst CONTRACT_VERSION: u32 = {CONTRACT_VERSION}u;\n"
    );
    for k in PrimitiveKind::ALL {
        s += &format!("const {}: u32 = {}u;\n", k.shader_name(), k as u32);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_layout_matches_the_attribute_table() {
        let offsets = [
            std::mem::offset_of!(Instance, rect),
            std::mem::offset_of!(Instance, uv),
            std::mem::offset_of!(Instance, color),
            std::mem::offset_of!(Instance, border_color),
            std::mem::offset_of!(Instance, clip),
            std::mem::offset_of!(Instance, params),
        ];
        for (i, &(loc, _, off)) in INSTANCE_ATTRIBUTES.iter().enumerate() {
            assert_eq!(loc, i as u32);
            assert_eq!(off, offsets[i], "attribute {i}");
        }
    }

    /// A line's endpoints ride in `uv`, which shapes and lines do not otherwise
    /// use — so adding paths changed the kind enum but not the instance layout,
    /// the stride, or the attribute table a backend binds.
    #[test]
    fn adding_lines_did_not_change_the_instance_layout() {
        assert_eq!(INSTANCE_STRIDE, 96);
        assert_eq!(INSTANCE_ATTRIBUTES.len(), 6);
        assert_eq!(std::mem::size_of::<Instance>(), INSTANCE_STRIDE);
        assert_eq!(CONTRACT_VERSION, 2, "bump this when the contract changes meaning");
    }

    #[test]
    fn kinds_round_trip_through_their_float_code() {
        for k in PrimitiveKind::ALL {
            assert_eq!(PrimitiveKind::from_code(k.code()), Some(k));
        }
        assert_eq!(PrimitiveKind::from_code(7.0), None);
        assert!(wgsl_prelude().contains("const KIND_GLYPH: u32 = 1u;"));
        assert!(wgsl_prelude().contains("const KIND_LINE: u32 = 3u;"));
    }
}
