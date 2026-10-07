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
//!
//! # Embedding your own renderer
//! `TextureId::User(n)` is opaque: map `n` to whatever your RHI uses. The host
//! owns the texture's lifetime, so:
//! - A `User` id the backend does not know must be **skipped silently**, not
//!   treated as an error. A resize that retires a texture mid-frame is normal.
//! - The image is sampled as **opaque sRGB**: `uv` selects a sub-rect, the
//!   texture's own alpha is ignored, and no colour conversion is applied.
//!   A linear or HDR target must be converted before it is handed over.
//!   The *instance's* colour still applies, premultiplied, so a viewport can
//!   be tinted or faded (dimmed behind a modal, cross-faded between two
//!   renderers) without the texture carrying an alpha channel.
//! - Whatever produced the texture must have completed before the UI pass
//!   samples it. Recording both into one command buffer gives that ordering;
//!   across queues or devices it is the host's to arrange.
//! - The glyph atlas is [`ATLAS_FORMAT`] coverage; user textures
//!   (`TextureId::User`) are [`USER_TEXTURE_FORMAT`], and how their alpha is
//!   read is the draw's [`ImageAlpha`], carried in `params[1]` of an
//!   [`PrimitiveKind::Image`] instance. The default, opaque, ignores it.
//! - Textures are read with texel loads and filtered in the shader: no sampler.

use crate::Instance;

/// Bumped whenever anything in this module changes meaning.
///
/// 3: an image's `params[1]` is its [`ImageAlpha`]. Zero is what it always
/// was, so a version-2 backend still draws every image — opaque.
///
/// 4: lines can be dashed (`params[1..3]` and `border_color[0]` of a
/// [`PrimitiveKind::Line`]), and images and glyphs can be rotated
/// (`border_color[0..2]`, see [`Rotation`]). Zero in every new field is what
/// a version-3 instance carries and means what it always did: a solid line,
/// an upright quad.
pub const CONTRACT_VERSION: u32 = 4;

/// How an image or glyph instance is rotated: about the centre of its `rect`,
/// clockwise on screen (y points down) for a positive angle.
///
/// Carried in `border_color[0..2]` — a field images and glyphs never used —
/// as `(cos θ − 1, sin θ)`, so the zeros an upright instance has always
/// carried mean "not rotated". The vertex stage turns the quad's corners by
/// it; `local` and `uv` stay those of the upright quad, so the fragment stage
/// is unchanged. Cosine and sine travel rather than the angle so that no
/// renderer evaluates a trigonometric function: every one of them rotates by
/// exactly the same amount ([`crate::sin_cos`] makes them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    pub sin: f32,
    pub cos: f32,
}

impl Rotation {
    pub const NONE: Rotation = Rotation { sin: 0.0, cos: 1.0 };

    /// A rotation by `radians`.
    pub fn new(radians: f32) -> Rotation {
        let (sin, cos) = crate::sin_cos(radians);
        Rotation { sin, cos }
    }

    pub fn is_none(&self) -> bool {
        self.sin == 0.0 && self.cos == 1.0
    }

    /// The two floats stored in `border_color[0..2]`.
    pub fn code(&self) -> [f32; 2] {
        [self.cos - 1.0, self.sin]
    }

    /// Read `border_color[0..2]` back.
    pub fn from_code(c: [f32; 2]) -> Rotation {
        Rotation { cos: 1.0 + c[0], sin: c[1] }
    }

    /// Turn `v` (relative to the centre) by this rotation.
    pub fn apply(&self, v: [f32; 2]) -> [f32; 2] {
        [v[0] * self.cos - v[1] * self.sin, v[0] * self.sin + v[1] * self.cos]
    }

    /// Turn `v` back: the inverse of [`Rotation::apply`].
    pub fn unapply(&self, v: [f32; 2]) -> [f32; 2] {
        [v[0] * self.cos + v[1] * self.sin, -v[0] * self.sin + v[1] * self.cos]
    }
}

/// How an image draw reads its texture's alpha, stored in `params[1]` of a
/// [`PrimitiveKind::Image`] instance.
///
/// Opaque is the default because the commonest image is a 3D view, and engine
/// render targets often leave junk in alpha: honouring it would punch holes in
/// the scene. An icon needs the opposite, so it is the draw's choice rather
/// than the library's.
///
/// **Where the premultiply happens is the whole difference between a clean
/// edge and a dark halo.** A straight-alpha texture's transparent texels are
/// usually black; filter first and premultiply after, and that black bleeds
/// into every edge. Premultiplying each texel *before* filtering is correct.
/// libgui's shader filters by hand and does that for [`ImageAlpha::Straight`].
/// A backend that filters in hardware cannot, so it must premultiply on upload
/// and draw with [`ImageAlpha::Premultiplied`].
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageAlpha {
    /// Alpha ignored; the texture covers its rect. 3D views, video, render
    /// targets whose alpha means nothing.
    #[default]
    Opaque = 0,
    /// Colour already multiplied by alpha. GPU render targets with real
    /// alpha, and icons premultiplied on upload.
    Premultiplied = 1,
    /// Ordinary straight alpha, as a PNG stores it. Premultiplied per texel
    /// before filtering, so edges stay clean.
    Straight = 2,
}

impl ImageAlpha {
    /// Value stored in `Instance::params[1]`.
    pub const fn code(self) -> f32 {
        self as u32 as f32
    }

    /// Decode `Instance::params[1]`. Anything unrecognised is opaque, which is
    /// the one reading that never makes part of an image disappear.
    pub fn from_code(c: f32) -> ImageAlpha {
        match c.round() as i64 {
            1 => ImageAlpha::Premultiplied,
            2 => ImageAlpha::Straight,
            _ => ImageAlpha::Opaque,
        }
    }
}

/// What an instance draws, stored in `Instance::params[3]` as a float code.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimitiveKind {
    /// Rounded-rect SDF: fills, borders, soft shadows.
    /// `params = [corner radius, border width, softness/blur, kind]`.
    Shape = 0,
    /// Glyph: atlas coverage (`.r`) × colour. `uv` = atlas rect (0..1).
    /// `border_color[0..2]` is its [`Rotation`].
    Glyph = 1,
    /// Image with a rounded-corner mask: texture `.rgb` × colour.
    /// `params = [corner radius, ImageAlpha, 0, kind]`, `uv` = texture rect
    /// (0..1), `border_color[0..2]` its [`Rotation`].
    Image = 2,
    /// Line segment with round caps, evaluated as a capsule SDF.
    /// `uv` = `[x0, y0, x1, y1]`, the endpoints in the same space as `rect`;
    /// `params = [half width, dash, gap, kind]`. `rect` is the segment's bounding
    /// box, already grown for the width and for anti-aliasing.
    ///
    /// **Dashes.** With `dash` and `gap` both above zero the line is drawn
    /// `dash` on, `gap` off, measured along it from `uv.xy` starting
    /// `border_color[0]` into the pattern (the phase, so a polyline's dashes
    /// run on across its joins). Dash ends are square. Zero for either is a
    /// solid line.
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

/// Attributes of an expanded [`crate::mesh::Vertex`], in location order:
/// `(location, name, byte offset, floats)`.
///
/// This is the *other* way to draw libgui: one quad per primitive, for a
/// renderer with no per-instance attributes (GLES2, WebGL1) or too few of them
/// (bgfx carries five `vec4`s; an instance needs six). See [`crate::mesh`],
/// which produces these and is tested to draw the same pixels as the instance
/// path.
pub const VERTEX_ATTRIBUTES: [(u32, &str, usize, usize); 9] = [
    (0, "pos", 0, 2),
    (1, "local", 8, 2),
    (2, "uv", 16, 2),
    (3, "color", 24, 4),
    (4, "border_color", 40, 4),
    (5, "clip", 56, 4),
    (6, "params", 72, 4),
    (7, "half_size", 88, 2),
    (8, "seg", 96, 4),
];

/// Byte size of one [`crate::mesh::Vertex`]; also the vertex buffer stride.
pub const VERTEX_STRIDE: usize = 112;
const _: () = assert!(std::mem::size_of::<crate::mesh::Vertex>() == VERTEX_STRIDE);

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
        assert_eq!(CONTRACT_VERSION, 4, "bump this when the contract changes meaning");
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
