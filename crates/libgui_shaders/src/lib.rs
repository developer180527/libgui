//! The single UI shader shared by every libgui backend.
//!
//! Authored once in WGSL (`shaders/ui.wgsl`), validated and translated by naga
//! at build time. Pick the flavour your RHI consumes:
//!
//! | constant | target | entry points |
//! |---|---|---|
//! | [`WGSL`] | wgpu / WebGPU | `vs_main`, `fs_main` |
//! | [`HLSL`] | D3D11 / D3D12 (SM 5.1) | `vs_main`, `fs_main` |
//! | [`MSL`] | Metal 2.0 | `vs_main`, `fs_main` |
//! | [`GLSL_VERTEX`], [`GLSL_FRAGMENT`] | OpenGL 4.5 | `main` |
//! | [`SPIRV`] | Vulkan (one module) | `vs_main`, `fs_main` |
//!
//! ## Pipeline contract (same for every target)
//! - **Vertex input:** none per-vertex. One instance buffer, step rate = instance,
//!   stride = 96 bytes, six `float4` attributes at locations 0..=5
//!   (`rect, uv, color, border_color, clip, params`), matching `libgui::Instance`.
//!   Draw 6 vertices per instance, triangle list.
//! - **Group 0:** binding 0 = uniform `libgui::Globals` (16 bytes).
//! - **Group 1:** binding 0 = `texture2D<float>`: the R8 glyph atlas, or a
//!   user texture (e.g. a viewport) for `TextureId::User`. **No sampler**: the
//!   shader uses texel loads with its own bilinear filter.
//! - **Blend:** premultiplied alpha (`ONE`, `ONE_MINUS_SRC_ALPHA`). No depth test, no culling.
//! - **Output:** one colour target. Colours are sRGB-encoded values; render into a
//!   non-sRGB (UNORM) target, or convert in your post pass.
//!
//! In HLSL the groups become register spaces: `b0, space0` globals and
//! `t0, space1` texture. In SPIR-V they are descriptor set 0/1, binding 0.

pub const WGSL: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.wgsl"));
pub const HLSL: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.hlsl"));
pub const MSL: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.metal"));
pub const GLSL_VERTEX: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.vert.glsl"));
pub const GLSL_FRAGMENT: &str = include_str!(concat!(env!("OUT_DIR"), "/ui.frag.glsl"));
/// Little-endian SPIR-V words as bytes.
pub const SPIRV: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ui.spv"));

pub const VERTEX_ENTRY: &str = "vs_main";
pub const FRAGMENT_ENTRY: &str = "fs_main";

/// `libgui::render_contract::CONTRACT_VERSION` these shaders were generated for.
pub const CONTRACT_VERSION: u32 = include!(concat!(env!("OUT_DIR"), "/contract_version.rs"));

/// Every generated file as `(file name, bytes)`, for exporting into a C/C++
/// engine's shader pipeline.
pub fn files() -> [(&'static str, &'static [u8]); 6] {
    [
        ("ui.wgsl", WGSL.as_bytes()),
        ("ui.hlsl", HLSL.as_bytes()),
        ("ui.metal", MSL.as_bytes()),
        ("ui.vert.glsl", GLSL_VERTEX.as_bytes()),
        ("ui.frag.glsl", GLSL_FRAGMENT.as_bytes()),
        ("ui.spv", SPIRV),
    ]
}
