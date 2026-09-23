//! Triangles, for renderers that cannot do instancing.
//!
//! libgui's native output is one instance per primitive: 96 bytes, six vertex
//! attributes, six vertices generated in the vertex shader. That is the cheap
//! path and it is what [`crate::render_contract`] describes. It also assumes
//! something not every renderer has:
//!
//! - **bgfx** carries at most five `vec4`s of instance data (80 bytes). Six do
//!   not fit, whatever the backend underneath it is.
//! - **GLES2 and WebGL1** have no per-instance attributes at all.
//! - Plenty of engine RHIs expose a vertex+index draw and nothing else.
//!
//! Every one of those authors would otherwise reverse-engineer the instance
//! layout and write this file themselves, and get the anti-aliasing padding
//! wrong on the first try. So it lives here, tested against the renderer that
//! is the contract's reference.
//!
//! # What it produces
//!
//! One quad per primitive — four [`Vertex`]es and six indices — with every
//! value the fragment shader reads already interpolated into the vertices.
//! **The vertex mirrors the shader's varyings, not its inputs**: it is what
//! `vs_main` outputs, computed on the CPU. So the port of `ui.wgsl` to another
//! shading language is
//!
//! - a vertex shader that transforms [`Vertex::pos`] (logical px) into clip
//!   space and passes everything else through, which is four lines; and
//! - `fs_main` transliterated, reading the varyings it already reads.
//!
//! Nothing needs `@interpolate(flat)`: a value that is constant across a quad
//! interpolates to itself, so the dialects without a flat qualifier are fine.
//!
//! # What it costs
//!
//! [`VERTEX_STRIDE`](crate::render_contract::VERTEX_STRIDE) bytes per vertex against 96 per instance, so a quad is
//! about four and a half times the bandwidth. For a UI that is tens of
//! microseconds and a megabyte or two a frame — the same order as any
//! immediate-mode UI's vertex buffers, and the reason this is a fallback
//! rather than the default. A renderer that *can* instance should.
//!
//! ```ignore
//! let mut mesh = Mesh::new();                 // once
//! mesh.build(out.draw);                       // per frame, no allocation
//! for b in &mesh.batches {
//!     bind(b.texture);
//!     draw_indexed(&mesh.vertices, &mesh.indices[b.indices.clone()]);
//! }
//! ```

use std::ops::Range;

use crate::draw::{DrawList, Instance, TextureId};
use crate::render_contract::PrimitiveKind;

/// Vertices and indices per expanded primitive.
pub const VERTICES_PER_QUAD: usize = 4;
pub const INDICES_PER_QUAD: usize = 6;

/// One expanded vertex: `vs_main`'s output, computed on the CPU.
///
/// Field order is the attribute order in
/// [`VERTEX_ATTRIBUTES`](crate::render_contract::VERTEX_ATTRIBUTES); every
/// field is 32-bit floats, and the struct is `#[repr(C)]` with no padding, so
/// a slice of these uploads as-is.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Position in **logical pixels** — the `world` varying. The vertex shader
    /// turns this into clip space; it is left in pixels so the mesh does not
    /// depend on the target size and can be built before it is known.
    pub pos: [f32; 2],
    /// Position relative to the primitive's centre, for the SDF.
    pub local: [f32; 2],
    /// Texture coordinates, 0..1. Meaningless for shapes and lines.
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub border_color: [f32; 4],
    /// The scissor rect in logical px, `[x0, y0, x1, y1]`. Constant across the
    /// quad: the fragment shader discards outside it, which is how libgui
    /// clips without a scissor state per draw.
    pub clip: [f32; 4],
    /// `[corner radius, border width, softness, kind]` — see
    /// [`PrimitiveKind`]. Constant across the quad.
    pub params: [f32; 4],
    /// Half the primitive's size, for the SDF. Constant across the quad.
    pub half_size: [f32; 2],
    /// A line's endpoints, `[x0, y0, x1, y1]` in logical px. Constant across
    /// the quad, and read only by [`PrimitiveKind::Line`].
    pub seg: [f32; 4],
}

/// A run of indices that share one texture — the same partition
/// [`crate::Batch`] describes, in index space.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshBatch {
    pub texture: TextureId,
    /// Half-open range into its chunk's slice of [`Mesh::indices`] — that is,
    /// relative to [`MeshChunk::indices`]`.start`, which is what a host needs
    /// after uploading that chunk. Unchunked there is one chunk starting at
    /// zero, so these are positions in [`Mesh::indices`] directly.
    pub indices: Range<u32>,
}

/// One upload's worth of a [`Mesh`]: the slices to put in a vertex and an
/// index buffer, and the draws to make from them.
///
/// A renderer that streams into a per-frame buffer has a ceiling — bgfx's
/// transient buffer is 6 MB by default, about thirteen thousand quads — and a
/// frame that goes over it is not a slow frame, it is a dropped draw call.
/// [`Mesh::build_limited`] cuts the frame into chunks that each fit.
///
/// `indices` holds values relative to `vertices.start`, and each batch's range
/// is relative to `indices.start`, so a host uploads the two slices and draws
/// the batches with no arithmetic of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshChunk {
    /// Into [`Mesh::vertices`].
    pub vertices: Range<u32>,
    /// Into [`Mesh::indices`].
    pub indices: Range<u32>,
    /// Into [`Mesh::batches`].
    pub batches: Range<u32>,
}

/// An expanded [`DrawList`]: triangles, ready to upload.
///
/// Keep one and call [`Mesh::build`] each frame; the buffers are reused, so a
/// steady frame allocates nothing.
#[derive(Default)]
pub struct Mesh {
    /// One chunk unless [`Mesh::build_limited`] had to split the frame.
    pub chunks: Vec<MeshChunk>,
    pub vertices: Vec<Vertex>,
    /// Two triangles per quad, `0,1,2, 2,1,3` from each quad's base vertex.
    /// 32-bit because a UI frame passes 16,384 quads sooner than people
    /// expect; [`Mesh::fits_u16`] says when the narrow form is safe.
    pub indices: Vec<u32>,
    pub batches: Vec<MeshBatch>,
}

impl Mesh {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve for `quads` primitives, to keep the first frame allocation-free
    /// too.
    pub fn reserve(&mut self, quads: usize) {
        self.vertices.reserve(quads * VERTICES_PER_QUAD);
        self.indices.reserve(quads * INDICES_PER_QUAD);
    }

    /// True when every index fits in a `u16`, for a renderer whose index
    /// buffers are 16-bit (GLES2, WebGL1, bgfx's default).
    ///
    /// Indices are relative to their own chunk, so this asks about the largest
    /// chunk: passing a vertex limit of 65,536 or less to
    /// [`Mesh::build_limited`] makes it true whatever the frame contains.
    pub fn fits_u16(&self) -> bool {
        self.chunks.iter().all(|c| (c.vertices.end - c.vertices.start) as usize <= u16::MAX as usize + 1)
    }

    /// Expand `draw` into triangles, replacing whatever was here before, as
    /// one chunk however large the frame is.
    pub fn build(&mut self, draw: &DrawList) {
        self.build_limited(draw, u32::MAX, u32::MAX);
    }

    /// [`Mesh::build`], cut into chunks that each stay within `max_vertices`
    /// and `max_indices`.
    ///
    /// For a renderer streaming into a fixed per-frame buffer: bgfx's
    /// transient buffer holds about thirteen thousand quads by default, and
    /// going over it drops the draw rather than slowing it down. A dense table
    /// or node graph passes that sooner than people expect.
    ///
    /// The limits are rounded down to whole quads, and a limit smaller than
    /// one quad is treated as one — a chunk that could hold nothing would
    /// never finish.
    pub fn build_limited(&mut self, draw: &DrawList, max_vertices: u32, max_indices: u32) {
        self.vertices.clear();
        self.indices.clear();
        self.batches.clear();
        self.chunks.clear();

        // Whole quads only: half a quad in one buffer and half in the next is
        // not a thing a draw call can express.
        let by_vertices = max_vertices as usize / VERTICES_PER_QUAD;
        let by_indices = max_indices as usize / INDICES_PER_QUAD;
        let quads_per_chunk = by_vertices.min(by_indices).max(1);

        // Where the chunk being filled starts.
        let (mut v0, mut i0, mut b0) = (0usize, 0usize, 0usize);
        let mut quads = 0usize;

        for batch in &draw.batches {
            let mut first_index = (self.indices.len() - i0) as u32;
            for inst in &draw.instances[batch.range.start as usize..batch.range.end as usize] {
                if quads == quads_per_chunk {
                    // Close the part of this batch that fits, then the chunk.
                    let indices = first_index..(self.indices.len() - i0) as u32;
                    if !indices.is_empty() {
                        self.batches.push(MeshBatch { texture: batch.texture, indices });
                    }
                    self.close_chunk(v0, i0, b0);
                    (v0, i0, b0) = (self.vertices.len(), self.indices.len(), self.batches.len());
                    quads = 0;
                    first_index = 0;
                }
                // Indices count from the chunk's first vertex, not the mesh's.
                let before = self.vertices.len();
                self.push_quad(inst, v0);
                if self.vertices.len() != before {
                    quads += 1;
                }
            }
            let indices = first_index..(self.indices.len() - i0) as u32;
            // A batch that expanded to nothing (an unknown kind) would
            // otherwise become an empty draw call for every backend to skip.
            if !indices.is_empty() {
                self.batches.push(MeshBatch { texture: batch.texture, indices });
            }
        }
        self.close_chunk(v0, i0, b0);
        // An empty frame still describes itself: one chunk, drawing nothing.
        if self.chunks.is_empty() {
            self.chunks.push(MeshChunk { vertices: 0..0, indices: 0..0, batches: 0..0 });
        }
    }

    /// Record the chunk that started at these marks, unless it is empty.
    fn close_chunk(&mut self, v0: usize, i0: usize, b0: usize) {
        if self.vertices.len() == v0 {
            return;
        }
        self.chunks.push(MeshChunk {
            vertices: v0 as u32..self.vertices.len() as u32,
            indices: i0 as u32..self.indices.len() as u32,
            batches: b0 as u32..self.batches.len() as u32,
        });
    }

    /// `chunk_v0` is the vertex the chunk being filled starts at, so the
    /// indices this writes are relative to it.
    fn push_quad(&mut self, inst: &Instance, chunk_v0: usize) {
        // Unknown kinds are dropped rather than drawn as something else: the
        // contract says a backend may meet an instance from a newer libgui.
        let Some(kind) = PrimitiveKind::from_code(inst.params[3]) else { return };

        // `vs_main`, exactly: shapes grow by their softness plus a pixel of
        // room for anti-aliasing, and nothing else does. Getting this wrong is
        // how a hand-written expansion clips the edge off every shadow.
        let pad = if kind == PrimitiveKind::Shape { inst.params[2] + 1.0 } else { 0.0 };
        let [rx, ry, rw, rh] = inst.rect;
        let half_size = [rw * 0.5, rh * 0.5];
        let center = [rx + half_size[0], ry + half_size[1]];
        let ext = [half_size[0] + pad, half_size[1] + pad];

        let base = (self.vertices.len() - chunk_v0) as u32;
        // Corners in the order the indices below assume: top-left, top-right,
        // bottom-left, bottom-right.
        for (cx, cy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let local = [(cx * 2.0 - 1.0) * ext[0], (cy * 2.0 - 1.0) * ext[1]];
            self.vertices.push(Vertex {
                pos: [center[0] + local[0], center[1] + local[1]],
                local,
                uv: [mix(inst.uv[0], inst.uv[2], cx), mix(inst.uv[1], inst.uv[3], cy)],
                color: inst.color,
                border_color: inst.border_color,
                clip: inst.clip,
                params: inst.params,
                half_size,
                seg: inst.uv,
            });
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_contract::{VERTEX_ATTRIBUTES, VERTEX_STRIDE};
    use crate::{Color, Rect, Vec2};

    fn shape(rect: Rect, softness: f32) -> Instance {
        Instance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            uv: [0.0; 4],
            color: [1.0, 0.0, 0.0, 1.0],
            border_color: [0.0; 4],
            clip: [0.0, 0.0, 1000.0, 1000.0],
            params: [4.0, 0.0, softness, PrimitiveKind::Shape.code()],
        }
    }

    fn list(instances: Vec<Instance>) -> DrawList {
        let mut dl = DrawList::default();
        let n = instances.len() as u32;
        dl.instances = instances;
        dl.batches = vec![crate::Batch { texture: TextureId::Atlas, range: 0..n }];
        dl
    }

    #[test]
    fn the_vertex_matches_the_attribute_table() {
        let offsets = [
            std::mem::offset_of!(Vertex, pos),
            std::mem::offset_of!(Vertex, local),
            std::mem::offset_of!(Vertex, uv),
            std::mem::offset_of!(Vertex, color),
            std::mem::offset_of!(Vertex, border_color),
            std::mem::offset_of!(Vertex, clip),
            std::mem::offset_of!(Vertex, params),
            std::mem::offset_of!(Vertex, half_size),
            std::mem::offset_of!(Vertex, seg),
        ];
        assert_eq!(VERTEX_ATTRIBUTES.len(), offsets.len());
        let mut expected = 0;
        for (i, &(loc, _, off, floats)) in VERTEX_ATTRIBUTES.iter().enumerate() {
            assert_eq!(loc, i as u32, "attribute {i} is out of order");
            assert_eq!(off, offsets[i], "attribute {i} is at the wrong offset");
            expected += floats * 4;
        }
        assert_eq!(VERTEX_STRIDE, std::mem::size_of::<Vertex>());
        assert_eq!(VERTEX_STRIDE, expected, "the table's floats do not add up to the stride");
    }

    #[test]
    fn every_primitive_becomes_one_quad() {
        let mut mesh = Mesh::new();
        mesh.build(&list(vec![shape(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0); 3]));
        assert_eq!(mesh.vertices.len(), 3 * VERTICES_PER_QUAD);
        assert_eq!(mesh.indices.len(), 3 * INDICES_PER_QUAD);
        assert_eq!(mesh.batches.len(), 1);
        assert_eq!(mesh.batches[0].indices, 0..18);
        // Both triangles of the second quad address the second quad.
        for &i in &mesh.indices[6..12] {
            assert!((4..8).contains(&i), "index {i} left its own quad");
        }
    }

    /// The padding rule, which is the thing a hand-written expansion gets
    /// wrong: a shape grows by its softness plus one pixel, and a glyph — same
    /// rect, same softness field — does not grow at all.
    #[test]
    fn only_shapes_grow_for_softness_and_anti_aliasing() {
        let mut mesh = Mesh::new();
        mesh.build(&list(vec![shape(Rect::new(10.0, 10.0, 20.0, 20.0), 8.0)]));
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.pos[0]).collect();
        assert_eq!(xs[0], 10.0 - 9.0, "a soft shape did not grow by softness + 1");
        assert_eq!(xs[1], 30.0 + 9.0);

        let mut glyph = shape(Rect::new(10.0, 10.0, 20.0, 20.0), 8.0);
        glyph.params[3] = PrimitiveKind::Glyph.code();
        mesh.build(&list(vec![glyph]));
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.pos[0]).collect();
        assert_eq!(xs[0], 10.0, "a glyph grew, and would sample outside its atlas rect");
        assert_eq!(xs[1], 30.0);
    }

    /// uv interpolates across the *padded* quad, so a soft shape's corners are
    /// outside 0..1 — which is what the shader's `mix` does too, and what
    /// keeps a glyph's texels lined up with its pixels.
    #[test]
    fn uv_spans_the_quad_that_is_actually_drawn() {
        let mut glyph = shape(Rect::new(0.0, 0.0, 8.0, 8.0), 0.0);
        glyph.params[3] = PrimitiveKind::Glyph.code();
        glyph.uv = [0.25, 0.5, 0.75, 1.0];
        let mut mesh = Mesh::new();
        mesh.build(&list(vec![glyph]));
        assert_eq!(mesh.vertices[0].uv, [0.25, 0.5]);
        assert_eq!(mesh.vertices[3].uv, [0.75, 1.0]);
        // And the raw endpoints ride along untouched, for lines.
        assert_eq!(mesh.vertices[0].seg, [0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn batches_become_index_ranges_over_one_buffer() {
        let mut dl = DrawList::default();
        dl.instances = vec![shape(Rect::new(0.0, 0.0, 1.0, 1.0), 0.0); 5];
        dl.batches = vec![
            crate::Batch { texture: TextureId::Atlas, range: 0..2 },
            crate::Batch { texture: TextureId::User(7), range: 2..5 },
        ];
        let mut mesh = Mesh::new();
        mesh.build(&dl);
        assert_eq!(mesh.batches.len(), 2);
        assert_eq!(mesh.batches[0], MeshBatch { texture: TextureId::Atlas, indices: 0..12 });
        assert_eq!(mesh.batches[1], MeshBatch { texture: TextureId::User(7), indices: 12..30 });
        assert_eq!(mesh.indices.len(), 30);
    }

    /// An instance from a newer libgui is dropped, not drawn as some other
    /// kind — and its batch does not become an empty draw call.
    #[test]
    fn an_unknown_kind_is_skipped_with_its_batch() {
        let mut odd = shape(Rect::new(0.0, 0.0, 4.0, 4.0), 0.0);
        odd.params[3] = 99.0;
        let mut mesh = Mesh::new();
        mesh.build(&list(vec![odd]));
        assert!(mesh.vertices.is_empty());
        assert!(mesh.batches.is_empty(), "an empty batch survived");
    }

    /// The buffers are reused: a steady frame expands without allocating.
    #[test]
    fn rebuilding_reuses_its_buffers() {
        let dl = list(vec![shape(Rect::new(0.0, 0.0, 4.0, 4.0), 0.0); 64]);
        let mut mesh = Mesh::new();
        mesh.build(&dl);
        let (v, i) = (mesh.vertices.capacity(), mesh.indices.capacity());
        for _ in 0..8 {
            mesh.build(&dl);
        }
        assert_eq!(mesh.vertices.capacity(), v, "the vertex buffer reallocated");
        assert_eq!(mesh.indices.capacity(), i, "the index buffer reallocated");
        assert!(mesh.fits_u16());
    }

    /// `pos` is the `world` varying: the quad the instance covers, in logical
    /// pixels, with the centre where the rect's centre is.
    #[test]
    fn positions_are_logical_pixels_around_the_rect() {
        let mut mesh = Mesh::new();
        mesh.build(&list(vec![shape(Rect::new(100.0, 40.0, 60.0, 20.0), 0.0)]));
        let v = &mesh.vertices;
        assert_eq!(v[0].pos, [99.0, 39.0]);
        assert_eq!(v[3].pos, [161.0, 61.0]);
        assert_eq!(v[0].half_size, [30.0, 10.0]);
        // `local` is measured from the centre, and the centre is at the middle
        // of the *rect*, padding or not.
        let centre = Vec2::new(130.0, 50.0);
        for vert in v {
            assert_eq!(vert.local, [vert.pos[0] - centre.x, vert.pos[1] - centre.y]);
        }
        assert_eq!(v[0].color, Color::rgba(1.0, 0.0, 0.0, 1.0).to_array());
    }
}
