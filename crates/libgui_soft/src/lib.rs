//! CPU reference backend for libgui.
//!
//! [`SoftRenderer`] draws a [`FrameOutput`] into an RGBA8 [`Target`] by
//! evaluating the shared shader, `libgui_shaders/shaders/ui.wgsl`, per pixel on
//! the CPU: the same SDFs, the same bilinear filter, the same premultiplied
//! blend into an 8-bit UNORM target. It exists for three things:
//!
//! - **Golden-image tests.** It uses only IEEE-exact float operations (add,
//!   multiply, divide, `sqrt`, `floor`; no transcendental functions and no
//!   fused multiply-add), so a frame renders to the same bytes on every OS and
//!   CPU. A GPU cannot promise that across drivers.
//! - **A reference** that a hand-written backend for your own RHI can be
//!   compared against, pixel by pixel.
//! - **Headless rendering**: thumbnails, CI screenshots, servers.
//!
//! It is written for clarity and exactness, not speed: a UI frame at 1080p
//! takes milliseconds, which is fine for all three.
//!
//! ```ignore
//! let mut soft = SoftRenderer::new();
//! let out = ui.end_frame();
//! let mut target = Target::new(width_px, height_px, out.clear_color);
//! soft.prepare(&out);
//! soft.render(&mut target, &out);
//! ```
//!
//! If this and the shader disagree, the shader is right: this file follows it
//! line by line, and `libgui_wgpu`'s parity test checks the two against each
//! other on a real GPU.
pub mod scenes;


use libgui::render_contract::{PrimitiveKind, Rotation, CONTRACT_VERSION};
use libgui::{Backend, Color, FrameOutput, Globals, Instance, TextureId};
use std::collections::HashMap;
use std::ops::Range;

// This file is a port of the shader for one version of the contract.
const _: () = assert!(CONTRACT_VERSION == 4, "the contract changed: update libgui_soft to match ui.wgsl");

/// An RGBA8 image: what the UI is drawn into. Pixels are premultiplied, like
/// a GPU framebuffer after the UI pass; with an opaque clear colour (the usual
/// case) every pixel stays opaque and premultiplied equals straight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, row-major, top row first.
    pub data: Vec<u8>,
}

impl Target {
    /// A target cleared to `clear` (sRGB-encoded, straight alpha, as libgui's
    /// colours are).
    pub fn new(width: u32, height: u32, clear: Color) -> Self {
        let [r, g, b, a] = clear.to_array();
        let px = [unorm8(r * a), unorm8(g * a), unorm8(b * a), unorm8(a)];
        let data = px.iter().copied().cycle().take((width * height * 4) as usize).collect();
        Self { width, height, data }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]]
    }
}

/// A texture sampled by `TextureId::User`: RGBA8, sRGB-encoded values, as the
/// render contract specifies. Whether its alpha counts is the draw's
/// [`libgui::ImageAlpha`]: ignored by default, as a 3D view wants.
#[derive(Clone, Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, row-major, top row first.
    pub data: Vec<u8>,
}

/// One channel or four, as stored; values in 0..1 once loaded.
enum Texels<'a> {
    R8(&'a [u8]),
    Rgba8(&'a [u8]),
}

struct TexRef<'a> {
    width: i32,
    height: i32,
    texels: Texels<'a>,
}

impl TexRef<'_> {
    fn load(&self, x: i32, y: i32) -> [f32; 4] {
        let i = (y * self.width + x) as usize;
        match self.texels {
            Texels::R8(d) => [d[i] as f32 / 255.0, 0.0, 0.0, 1.0],
            Texels::Rgba8(d) => {
                let p = &d[i * 4..i * 4 + 4];
                [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
            }
        }
    }

    /// `sample_bilinear` in ui.wgsl: manual bilinear with clamp-to-edge.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.width, self.height);
        let px = u * w as f32 - 0.5;
        let py = v * h as f32 - 0.5;
        let (fx, fy) = (fract(px), fract(py));
        let (ix, iy) = (px.floor() as i32, py.floor() as i32);
        let cx = |x: i32| x.clamp(0, w - 1);
        let cy = |y: i32| y.clamp(0, h - 1);
        let a = self.load(cx(ix), cy(iy));
        let b = self.load(cx(ix + 1), cy(iy));
        let c = self.load(cx(ix), cy(iy + 1));
        let d = self.load(cx(ix + 1), cy(iy + 1));
        let mut out = [0.0; 4];
        for k in 0..4 {
            out[k] = mix(mix(a[k], b[k], fx), mix(c[k], d[k], fx), fy);
        }
        out
    }

    /// `sample_bilinear_premul` in `ui.wgsl`: each texel premultiplied before
    /// it is interpolated, so a straight-alpha texture's transparent texels
    /// cannot bleed their colour into its edges.
    fn sample_premul(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.width, self.height);
        let px = u * w as f32 - 0.5;
        let py = v * h as f32 - 0.5;
        let (fx, fy) = (fract(px), fract(py));
        let (ix, iy) = (px.floor() as i32, py.floor() as i32);
        let cx = |x: i32| x.clamp(0, w - 1);
        let cy = |y: i32| y.clamp(0, h - 1);
        let a = premul(self.load(cx(ix), cy(iy)));
        let b = premul(self.load(cx(ix + 1), cy(iy)));
        let c = premul(self.load(cx(ix), cy(iy + 1)));
        let d = premul(self.load(cx(ix + 1), cy(iy + 1)));
        let mut out = [0.0; 4];
        for k in 0..4 {
            out[k] = mix(mix(a[k], b[k], fx), mix(c[k], d[k], fx), fy);
        }
        out
    }
}

/// The CPU backend. Holds a copy of the frame's instances and atlas between
/// [`Backend::prepare`] and [`Backend::render`], like a GPU backend holds
/// buffers, plus any user textures you register.
#[derive(Default)]
pub struct SoftRenderer {
    globals: Globals,
    instances: Vec<Instance>,
    atlas: Vec<u8>,
    atlas_size: u32,
    atlas_version: u64,
    user: HashMap<u64, Texture>,
    next_user: u64,
}

impl SoftRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an image for `ui.viewport` / `Painter::image`.
    pub fn register_texture(&mut self, texture: Texture) -> TextureId {
        let id = TextureId::User(self.next_user);
        self.next_user += 1;
        self.update_texture(id, texture);
        id
    }

    pub fn update_texture(&mut self, id: TextureId, texture: Texture) {
        assert_eq!(texture.data.len(), (texture.width * texture.height * 4) as usize, "texture data is not width*height*4 bytes");
        if let TextureId::User(n) = id {
            self.user.insert(n, texture);
        }
    }

    pub fn unregister_texture(&mut self, id: TextureId) {
        if let TextureId::User(n) = id {
            self.user.remove(&n);
        }
    }

    /// Render an **expanded** frame — [`libgui::mesh::Mesh`] triangles rather
    /// than instances — into a new target, otherwise exactly as
    /// [`SoftRenderer::render_to_image`] does.
    ///
    /// This is the reference for a renderer that cannot instance (bgfx, GLES2,
    /// WebGL1). It shares the fragment stage with the instanced path, so the
    /// two agreeing is a statement about the expansion; `mesh_parity.rs`
    /// asserts they agree byte for byte on real frames.
    pub fn render_mesh_to_image(
        &mut self,
        frame: &FrameOutput,
        mesh: &libgui::mesh::Mesh,
        width: u32,
        height: u32,
    ) -> Target {
        let mut target = Target::new(width, height, frame.clear_color);
        self.prepare(frame);
        // Exactly what a host does: for each chunk, take its two slices and
        // draw its batches out of them. Indices count from the chunk's first
        // vertex, and a batch's range from the chunk's first index.
        for chunk in &mesh.chunks {
            let (v0, i0) = (chunk.vertices.start as usize, chunk.indices.start as usize);
            let vertices = &mesh.vertices[v0..chunk.vertices.end as usize];
            let indices = &mesh.indices[i0..chunk.indices.end as usize];
            for batch in &mesh.batches[chunk.batches.start as usize..chunk.batches.end as usize] {
                let Some(tex) = self.texture(batch.texture) else { continue };
                if tex.width == 0 || tex.height == 0 {
                    continue;
                }
                // Two triangles per quad, and the first index of each pair
                // names its top-left vertex — the layout `Mesh::build`
                // documents.
                let range = batch.indices.start as usize..batch.indices.end as usize;
                for tri in indices[range].chunks_exact(6) {
                    let base = tri[0] as usize;
                    draw_quad(&mut target, &self.globals, &vertices[base..base + 4], &tex);
                }
            }
        }
        target
    }

    /// Prepare and render a whole frame into a new target of `width` x
    /// `height` physical pixels, cleared to the frame's clear colour.
    pub fn render_to_image(&mut self, frame: &FrameOutput, width: u32, height: u32) -> Target {
        let mut target = Target::new(width, height, frame.clear_color);
        self.prepare(frame);
        self.render(&mut target, frame);
        target
    }

    fn texture(&self, id: TextureId) -> Option<TexRef<'_>> {
        match id {
            TextureId::Atlas => Some(TexRef {
                width: self.atlas_size as i32,
                height: self.atlas_size as i32,
                texels: Texels::R8(&self.atlas),
            }),
            TextureId::User(n) => self.user.get(&n).map(|t| TexRef {
                width: t.width as i32,
                height: t.height as i32,
                texels: Texels::Rgba8(&t.data),
            }),
        }
    }
}

impl Backend for SoftRenderer {
    type Pass<'p> = Target;

    fn prepare(&mut self, frame: &FrameOutput) {
        self.globals = frame.globals();
        self.instances.clear();
        self.instances.extend_from_slice(frame.instances());
        let atlas = frame.atlas();
        if atlas.size != self.atlas_size || atlas.version != self.atlas_version {
            self.atlas.clear();
            self.atlas.extend_from_slice(&atlas.data);
            self.atlas_size = atlas.size;
            self.atlas_version = atlas.version;
        }
    }

    fn begin(&mut self, _target: &mut Target) {}

    fn draw(&mut self, target: &mut Target, texture: TextureId, instances: Range<u32>) {
        // Unknown user textures are skipped silently (render contract).
        let Some(tex) = self.texture(texture) else { return };
        if tex.width == 0 || tex.height == 0 {
            return;
        }
        for inst in &self.instances[instances.start as usize..instances.end as usize] {
            draw_instance(target, &self.globals, inst, &tex);
        }
    }
}

/// Rasterise one instance: the quad from `vs_main`, then `fs_main` at every
/// pixel centre inside it, blended into the target.
fn draw_instance(target: &mut Target, g: &Globals, inst: &Instance, tex: &TexRef) {
    let Some(kind) = PrimitiveKind::from_code(inst.params[3]) else { return };
    let [rx, ry, rw, rh] = inst.rect;

    // vs_main: the quad, grown for softness + AA on shapes only.
    let pad = if kind == PrimitiveKind::Shape { inst.params[2] + 1.0 } else { 0.0 };
    let half = [rw * 0.5, rh * 0.5];
    let center = [rx + half[0], ry + half[1]];
    let ext = [half[0] + pad, half[1] + pad];
    let (qx0, qx1) = (center[0] - ext[0], center[0] + ext[0]);
    let (qy0, qy1) = (center[1] - ext[1], center[1] + ext[1]);

    // A rotated image or glyph: the quad `vs_main` turns, corner by corner,
    // exactly as `Mesh` builds it, so the two paths share one rasteriser.
    if matches!(kind, PrimitiveKind::Image | PrimitiveKind::Glyph) {
        let rot = Rotation::from_code([inst.border_color[0], inst.border_color[1]]);
        if !rot.is_none() {
            // A turned image gets a pixel of room to anti-alias its edge,
            // with the texture mapping carried past it, as `vs_main` does.
            let image = kind == PrimitiveKind::Image;
            let ext = if image { [half[0] + 1.0, half[1] + 1.0] } else { ext };
            let mut corners = [Corner::default(); 4];
            for (k, (cx, cy)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].into_iter().enumerate() {
                let local = [(cx * 2.0f32 - 1.0) * ext[0], (cy * 2.0f32 - 1.0) * ext[1]];
                let turned = rot.apply(local);
                let [ux, uy] = if image { libgui::mesh::uv_fraction(local, half) } else { [cx, cy] };
                corners[k] = Corner {
                    pos: [center[0] + turned[0], center[1] + turned[1]],
                    local,
                    uv: [mix(inst.uv[0], inst.uv[2], ux), mix(inst.uv[1], inst.uv[3], uy)],
                };
            }
            let flat = Flat { color: inst.color, border_color: inst.border_color, clip: inst.clip, params: inst.params, half_size: half, seg: inst.uv };
            draw_turned(target, g, kind, &corners, &flat, tex);
            return;
        }
    }

    // Logical to physical pixels, as the viewport transform does it.
    let (w, h) = (target.width as f32, target.height as f32);
    let (sx, sy) = (w / g.screen_size[0], h / g.screen_size[1]);

    // Pixels whose centre is inside the quad: left/top edges inclusive,
    // right/bottom exclusive (the top-left rule every GPU API uses).
    let span = |a: f32, b: f32, n: u32| -> Range<u32> {
        let lo = (a - 0.5).ceil().max(0.0);
        let hi = (b - 0.5).ceil().min(n as f32);
        if hi <= lo {
            0..0
        } else {
            lo as u32..hi as u32
        }
    };
    let xs = span(qx0 * sx, qx1 * sx, target.width);
    let ys = span(qy0 * sy, qy1 * sy, target.height);

    let clip = inst.clip;
    let aa = 1.0 / g.scale;
    for py in ys {
        // Pixel centre back in logical px: the interpolated `world` varying.
        let wy = (py as f32 + 0.5) / sy;
        if wy < clip[1] || wy > clip[3] {
            continue;
        }
        let row = (py * target.width) as usize * 4;
        for px in xs.clone() {
            let wx = (px as f32 + 0.5) / sx;
            if wx < clip[0] || wx > clip[2] {
                continue; // discard
            }
            let local = [wx - center[0], wy - center[1]];
            let vary = Varyings {
                world: [wx, wy],
                local,
                uv: quad_uv(inst.uv, local, ext),
                color: inst.color,
                border_color: inst.border_color,
                params: inst.params,
                half_size: half,
                seg: inst.uv,
            };
            let src = fragment(kind, &vary, aa, tex);
            if src[3] <= 0.0 && src[0] <= 0.0 && src[1] <= 0.0 && src[2] <= 0.0 {
                continue; // blending zero leaves the pixel as it is
            }
            blend(&mut target.data[row + px as usize * 4..row + px as usize * 4 + 4], src);
        }
    }
}

/// What the vertex stage hands the fragment stage: `VOut` in `ui.wgsl`, and
/// field for field the same thing [`libgui::mesh::Vertex`] carries. Both paths
/// below fill one of these and call the same `fragment`, which is what makes
/// "the expanded mesh draws the same pixels" a claim about the vertex stage
/// rather than about two copies of a shader.
struct Varyings {
    world: [f32; 2],
    local: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    border_color: [f32; 4],
    params: [f32; 4],
    half_size: [f32; 2],
    seg: [f32; 4],
}

/// `fs_main`, for one pixel. Returns premultiplied colour.
fn fragment(kind: PrimitiveKind, v: &Varyings, aa: f32, tex: &TexRef) -> [f32; 4] {
    let (local, half, p) = (v.local, v.half_size, v.params);
    match kind {
        PrimitiveKind::Line => {
            let [x0, y0, x1, y1] = v.seg;
            let mut d = sd_segment(v.world, [x0, y0], [x1, y1]) - p[0];
            let (on, off) = (p[1], p[2]);
            if on > 0.0 && off > 0.0 {
                let ba = [x1 - x0, y1 - y0];
                let len = length(ba).max(1e-6);
                let s = ((v.world[0] - x0) * ba[0] + (v.world[1] - y0) * ba[1]) / len + v.border_color[0];
                let period = on + off;
                let m = s - (s / period).floor() * period;
                let along = if m < on { -(m.min(on - m)) } else { (m - on).min(period - m) };
                d = d.max(along);
            }
            let m = clamp01(0.5 - d / aa);
            scale4(premul(v.color), m)
        }
        PrimitiveKind::Image => {
            let r = p[0].min(half[0].min(half[1]));
            let d = sd_round_rect(local, half, r);
            let m = clamp01(0.5 - d / aa);
            let c = v.color;
            match libgui::ImageAlpha::from_code(p[1]) {
                // The texture's own alpha ignored: a 3D view whose alpha
                // means nothing. The tint's alpha multiplies colour too.
                libgui::ImageAlpha::Opaque => {
                    let s = tex.sample(v.uv[0], v.uv[1]);
                    scale4(premul([s[0] * c[0], s[1] * c[1], s[2] * c[2], c[3]]), m)
                }
                alpha => {
                    let s = if alpha == libgui::ImageAlpha::Straight {
                        tex.sample_premul(v.uv[0], v.uv[1])
                    } else {
                        tex.sample(v.uv[0], v.uv[1])
                    };
                    // Both premultiplied now: the tint multiplies alike.
                    scale4([s[0] * c[0] * c[3], s[1] * c[1] * c[3], s[2] * c[2] * c[3], s[3] * c[3]], m)
                }
            }
        }
        PrimitiveKind::Glyph => scale4(premul(v.color), tex.sample(v.uv[0], v.uv[1])[0]),
        PrimitiveKind::Shape => {
            let r = p[0].min(half[0].min(half[1]));
            let soft = p[2] + aa;
            let d = sd_round_rect(local, half, r);
            let fill = 1.0 - smoothstep(-soft * 0.5, soft * 0.5, d);
            let mut col = premul(v.color);
            let bw = p[1];
            if bw > 0.0 {
                let inner = sd_round_rect(local, [half[0] - bw, half[1] - bw], (r - bw).max(0.0));
                let b = smoothstep(-aa * 0.5, aa * 0.5, inner);
                let bc = premul(v.border_color);
                for k in 0..4 {
                    col[k] = mix(col[k], bc[k], b);
                }
            }
            scale4(col, fill)
        }
    }
}

/// Rasterise one expanded quad: the varyings are interpolated across it, the
/// way a GPU interpolates them, and the same `fragment` runs.
///
/// The quad is axis-aligned and `w` is 1 everywhere, so the interpolation is
/// plain bilinear — there is no perspective divide to get wrong, and the
/// values at the pixel centres are the ones the instanced path computes
/// directly.
fn draw_quad(target: &mut Target, g: &Globals, verts: &[libgui::mesh::Vertex], tex: &TexRef) {
    let Some(kind) = PrimitiveKind::from_code(verts[0].params[3]) else { return };
    // Corners in `Mesh::build`'s order: top-left, top-right, bottom-left,
    // bottom-right.
    let (v0, v3) = (&verts[0], &verts[3]);
    // Turned: not an upright axis-aligned rectangle any more. The order counts
    // as well as the alignment: a half turn's corners line up again, but the
    // first one is then the bottom-right.
    let upright = verts[1].pos[1] == v0.pos[1]
        && verts[2].pos[0] == v0.pos[0]
        && verts[1].pos[0] > v0.pos[0]
        && verts[2].pos[1] > v0.pos[1];
    if !upright {
        let corner = |v: &libgui::mesh::Vertex| Corner { pos: v.pos, local: v.local, uv: v.uv };
        let corners = [corner(&verts[0]), corner(&verts[1]), corner(&verts[2]), corner(&verts[3])];
        let flat = Flat { color: v0.color, border_color: v0.border_color, clip: v0.clip, params: v0.params, half_size: v0.half_size, seg: v0.seg };
        draw_turned(target, g, kind, &corners, &flat, tex);
        return;
    }
    let (qx0, qy0) = (v0.pos[0], v0.pos[1]);
    let (qx1, qy1) = (v3.pos[0], v3.pos[1]);
    let (dx, dy) = (qx1 - qx0, qy1 - qy0);
    if dx <= 0.0 || dy <= 0.0 {
        return;
    }

    let (w, h) = (target.width as f32, target.height as f32);
    let (sx, sy) = (w / g.screen_size[0], h / g.screen_size[1]);
    let span = |a: f32, b: f32, n: u32| -> Range<u32> {
        let lo = (a - 0.5).ceil().max(0.0);
        let hi = (b - 0.5).ceil().min(n as f32);
        if hi <= lo {
            0..0
        } else {
            lo as u32..hi as u32
        }
    };
    let xs = span(qx0 * sx, qx1 * sx, target.width);
    let ys = span(qy0 * sy, qy1 * sy, target.height);

    let clip = v0.clip;
    let aa = 1.0 / g.scale;
    for py in ys {
        let wy = (py as f32 + 0.5) / sy;
        if wy < clip[1] || wy > clip[3] {
            continue;
        }
        let ty = (wy - qy0) / dy;
        let row = (py * target.width) as usize * 4;
        for px in xs.clone() {
            let wx = (px as f32 + 0.5) / sx;
            if wx < clip[0] || wx > clip[2] {
                continue; // discard
            }
            let tx = (wx - qx0) / dx;
            let vary = Varyings {
                world: [wx, wy],
                local: [mix(v0.local[0], v3.local[0], tx), mix(v0.local[1], v3.local[1], ty)],
                uv: [mix(v0.uv[0], v3.uv[0], tx), mix(v0.uv[1], v3.uv[1], ty)],
                color: v0.color,
                border_color: v0.border_color,
                params: v0.params,
                half_size: v0.half_size,
                seg: v0.seg,
            };
            let src = fragment(kind, &vary, aa, tex);
            if src[3] <= 0.0 && src[0] <= 0.0 && src[1] <= 0.0 && src[2] <= 0.0 {
                continue;
            }
            blend(&mut target.data[row + px as usize * 4..row + px as usize * 4 + 4], src);
        }
    }
}

/// One corner of a quad that is not axis-aligned: where it is, and the
/// varyings that interpolate across it.
#[derive(Clone, Copy, Default)]
struct Corner {
    pos: [f32; 2],
    local: [f32; 2],
    uv: [f32; 2],
}

/// The varyings that are the same at every corner.
struct Flat {
    color: [f32; 4],
    border_color: [f32; 4],
    clip: [f32; 4],
    params: [f32; 4],
    half_size: [f32; 2],
    seg: [f32; 4],
}

/// Rasterise a turned quad — corners top-left, top-right, bottom-left,
/// bottom-right of the upright quad — interpolating `local` and `uv` the way
/// a GPU does across a parallelogram: as an affine function of position.
///
/// A pixel is drawn when its centre is inside, with the two edges leaving the
/// first corner inclusive and the other two exclusive, which is the top-left
/// rule restated for edges that are not horizontal or vertical.
fn draw_turned(target: &mut Target, g: &Globals, kind: PrimitiveKind, c: &[Corner; 4], f: &Flat, tex: &TexRef) {
    let p0 = c[0].pos;
    let e1 = [c[1].pos[0] - p0[0], c[1].pos[1] - p0[1]];
    let e2 = [c[2].pos[0] - p0[0], c[2].pos[1] - p0[1]];
    let det = e1[0] * e2[1] - e1[1] * e2[0];
    if det == 0.0 || !det.is_finite() {
        return;
    }
    let (w, h) = (target.width as f32, target.height as f32);
    let (sx, sy) = (w / g.screen_size[0], h / g.screen_size[1]);
    let xs_all = [c[0].pos[0], c[1].pos[0], c[2].pos[0], c[3].pos[0]];
    let ys_all = [c[0].pos[1], c[1].pos[1], c[2].pos[1], c[3].pos[1]];
    let fold = |v: [f32; 4], min: bool| v.into_iter().fold(v[0], |a, b| if min { a.min(b) } else { a.max(b) });
    let span = |a: f32, b: f32, n: u32| -> Range<u32> {
        let lo = (a - 0.5).ceil().max(0.0);
        let hi = (b - 0.5).ceil().min(n as f32);
        if hi <= lo {
            0..0
        } else {
            lo as u32..hi as u32
        }
    };
    let xs = span(fold(xs_all, true) * sx, fold(xs_all, false) * sx, target.width);
    let ys = span(fold(ys_all, true) * sy, fold(ys_all, false) * sy, target.height);
    let clip = f.clip;
    let aa = 1.0 / g.scale;
    for py in ys {
        let wy = (py as f32 + 0.5) / sy;
        if wy < clip[1] || wy > clip[3] {
            continue;
        }
        let row = (py * target.width) as usize * 4;
        for px in xs.clone() {
            let wx = (px as f32 + 0.5) / sx;
            if wx < clip[0] || wx > clip[2] {
                continue;
            }
            // Where the pixel is in the quad's own frame: world - p0 = a e1 + b e2.
            let (dx, dy) = (wx - p0[0], wy - p0[1]);
            let a = (dx * e2[1] - dy * e2[0]) / det;
            let b = (e1[0] * dy - e1[1] * dx) / det;
            if !(0.0..1.0).contains(&a) || !(0.0..1.0).contains(&b) {
                continue;
            }
            let lerp2 = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| [p[0] + a * (q[0] - p[0]) + b * (r[0] - p[0]), p[1] + a * (q[1] - p[1]) + b * (r[1] - p[1])];
            let vary = Varyings {
                world: [wx, wy],
                local: lerp2(c[0].local, c[1].local, c[2].local),
                uv: lerp2(c[0].uv, c[1].uv, c[2].uv),
                color: f.color,
                border_color: f.border_color,
                params: f.params,
                half_size: f.half_size,
                seg: f.seg,
            };
            let src = fragment(kind, &vary, aa, tex);
            if src[3] <= 0.0 && src[0] <= 0.0 && src[1] <= 0.0 && src[2] <= 0.0 {
                continue;
            }
            blend(&mut target.data[row + px as usize * 4..row + px as usize * 4 + 4], src);
        }
    }
}

/// The `uv` varying: `mix(uv.xy, uv.zw, c)` where `c` is the corner position
/// 0..1 across the quad.
fn quad_uv(uv: [f32; 4], local: [f32; 2], ext: [f32; 2]) -> [f32; 2] {
    let cx = (local[0] / ext[0] + 1.0) * 0.5;
    let cy = (local[1] / ext[1] + 1.0) * 0.5;
    [mix(uv[0], uv[2], cx), mix(uv[1], uv[3], cy)]
}

/// Premultiplied-alpha blend into a UNORM8 pixel: `src + dst * (1 - src.a)`,
/// rounded back to 8 bits, as the GPU does after every draw.
fn blend(dst: &mut [u8], src: [f32; 4]) {
    let a = clamp01(src[3]);
    for k in 0..4 {
        let d = dst[k] as f32 / 255.0;
        dst[k] = unorm8(clamp01(src[k]) + d * (1.0 - a));
    }
}

fn unorm8(v: f32) -> u8 {
    (clamp01(v) * 255.0 + 0.5).floor() as u8
}

// ---- WGSL built-ins and the shader's helpers, exactly as ui.wgsl has them ----

fn sd_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let pa = [p[0] - a[0], p[1] - a[1]];
    let ba = [b[0] - a[0], b[1] - a[1]];
    let h = ((pa[0] * ba[0] + pa[1] * ba[1]) / (ba[0] * ba[0] + ba[1] * ba[1]).max(1e-6)).clamp(0.0, 1.0);
    length([pa[0] - ba[0] * h, pa[1] - ba[1] * h])
}

fn sd_round_rect(p: [f32; 2], b: [f32; 2], r: f32) -> f32 {
    let q = [p[0].abs() - b[0] + r, p[1].abs() - b[1] + r];
    length([q[0].max(0.0), q[1].max(0.0)]) + q[0].max(q[1]).min(0.0) - r
}

fn length(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn premul(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

fn scale4(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3] * k]
}

#[cfg(test)]
mod tests {
    use super::*;
    use libgui::{FrameInfo, Rect, Theme, Ui, Vec2};

    const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
    const BLACK: Color = Color::rgba(0.0, 0.0, 0.0, 1.0);

    /// Draw with a painter into a `w` x `h` logical target at `scale`.
    fn paint(w: f32, h: f32, scale: f32, f: impl FnOnce(&mut libgui::Painter, Rect) + 'static) -> Target {
        let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(w, h), scale, dt: 1.0 });
        let id = ui.make_id("canvas");
        ui.add_leaf(id, libgui::Layout::leaf(libgui::Size::Grow(1.0), libgui::Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| f(p, r));
        let out = ui.end_frame();
        let mut t = Target::new((w * scale).round() as u32, (h * scale).round() as u32, BLACK);
        let mut soft = SoftRenderer::new();
        soft.prepare(&out);
        soft.render(&mut t, &out);
        t
    }

    /// A rect on whole pixels covers exactly those pixels: full inside,
    /// untouched outside. Anything else would blur every panel edge.
    #[test]
    fn a_pixel_aligned_rect_has_crisp_edges() {
        let t = paint(40.0, 30.0, 1.0, |p, _| p.rect(Rect::new(10.0, 5.0, 10.0, 8.0), Color::rgba(1.0, 1.0, 1.0, 1.0), 0.0));
        for y in 0..30 {
            for x in 0..40 {
                let inside = (10..20).contains(&x) && (5..13).contains(&y);
                let want = if inside { [255, 255, 255, 255] } else { [0, 0, 0, 255] };
                assert_eq!(t.pixel(x, y), want, "pixel ({x},{y})");
            }
        }
        // At 2x the same rect is 20x16 physical pixels.
        let t = paint(40.0, 30.0, 2.0, |p, _| p.rect(Rect::new(10.0, 5.0, 10.0, 8.0), Color::rgba(1.0, 1.0, 1.0, 1.0), 0.0));
        assert_eq!(t.pixel(20, 10), [255; 4]);
        assert_eq!(t.pixel(39, 25), [255; 4]);
        assert_eq!(t.pixel(19, 10), [0, 0, 0, 255]);
        assert_eq!(t.pixel(40, 26), [0, 0, 0, 255]);
    }

    /// Half-transparent white over black is half grey: premultiplied blend,
    /// rounded to 8 bits.
    #[test]
    fn translucent_fills_blend_premultiplied() {
        let t = paint(8.0, 8.0, 1.0, |p, _| p.rect(Rect::new(0.0, 0.0, 8.0, 8.0), Color::rgba(1.0, 1.0, 1.0, 0.5), 0.0));
        assert_eq!(t.pixel(4, 4), [128, 128, 128, 255]);
    }

    /// A clip rect discards what falls outside it, like the shader's `discard`.
    #[test]
    fn clipping_discards_outside_pixels() {
        let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
        ui.begin_frame(FrameInfo { screen_size: Vec2::new(20.0, 20.0), scale: 1.0, dt: 1.0 });
        let id = ui.make_id("c");
        ui.add_leaf(id, libgui::Layout::leaf(libgui::Size::Grow(1.0), libgui::Size::Grow(1.0)), Vec2::ZERO, false, |p, _| {
            p.draw.push_clip(Rect::new(0.0, 0.0, 10.0, 20.0));
            p.rect(Rect::new(0.0, 0.0, 20.0, 20.0), Color::rgba(1.0, 0.0, 0.0, 1.0), 0.0);
            p.draw.pop_clip();
        });
        let out = ui.end_frame();
        let t = SoftRenderer::new().render_to_image(&out, 20, 20);
        assert_eq!(t.pixel(5, 5)[0], 255);
        assert_eq!(t.pixel(15, 5)[0], (out.clear_color.to_array()[0] * 255.0 + 0.5).floor() as u8);
    }

    /// Text draws: glyph coverage lands in the target.
    #[test]
    fn glyphs_render() {
        let t = paint(60.0, 24.0, 1.0, |p, r| p.text_left(r, 16.0, Color::rgba(1.0, 1.0, 1.0, 1.0), "Hig"));
        let lit = t.data.chunks(4).filter(|p| p[0] > 128).count();
        assert!(lit > 40, "only {lit} lit pixels for three glyphs");
    }

    /// A user texture shows through an image instance; an unknown one is
    /// skipped silently rather than failing (render contract).
    #[test]
    fn user_textures_sample_and_unknown_ones_are_skipped() {
        let mut soft = SoftRenderer::new();
        let red = soft.register_texture(Texture { width: 2, height: 2, data: [255, 0, 0, 255].repeat(4) });
        for (tex, want) in [(red, 255u8), (TextureId::User(99), 0)] {
            let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
            ui.begin_frame(FrameInfo { screen_size: Vec2::new(10.0, 10.0), scale: 1.0, dt: 1.0 });
            let id = ui.make_id("c");
            ui.add_leaf(id, libgui::Layout::leaf(libgui::Size::Grow(1.0), libgui::Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| p.image(r, tex, 0.0));
            let out = ui.end_frame();
            let mut t = Target::new(10, 10, BLACK);
            soft.prepare(&out);
            soft.render(&mut t, &out);
            assert_eq!(t.pixel(5, 5)[0], want, "{tex:?}");
        }
    }

    /// A long line is drawn as several strips, and a translucent one must
    /// still blend once per pixel: anything brighter than one 50% blend over
    /// black is a pixel drawn twice where two strips overlap.
    #[test]
    fn a_split_translucent_line_blends_once() {
        let t = paint(200.0, 160.0, 1.5, |p, _| {
            p.line(Vec2::new(10.0, 10.0), Vec2::new(190.0, 150.0), 8.0, Color::rgba(1.0, 1.0, 1.0, 0.5))
        });
        let brightest = t.data.chunks(4).map(|p| p[0]).max().unwrap();
        assert_eq!(brightest, 128, "a pixel was blended more than once");
        // And the middle of the line really is covered.
        assert_eq!(t.pixel(150, 120)[0], 128);
    }

    /// Lines are capsules: the centre of a thick line is fully covered and a
    /// point well off it is untouched.
    #[test]
    fn lines_are_capsules() {
        let t = paint(40.0, 40.0, 1.0, |p, _| p.line(Vec2::new(5.0, 5.0), Vec2::new(35.0, 35.0), 4.0, Color::rgba(1.0, 1.0, 1.0, 1.0)));
        assert_eq!(t.pixel(20, 20)[0], 255);
        assert_eq!(t.pixel(30, 10)[0], 0);
    }
}
