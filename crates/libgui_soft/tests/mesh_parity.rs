//! The expansion draws the same pixels as the instances.
//!
//! `libgui::mesh` exists so that a renderer without instancing — bgfx, GLES2,
//! WebGL1, an engine RHI with only a vertex+index draw — does not have to
//! reverse-engineer the instance layout. That promise is worth nothing unless
//! the two paths agree, so every golden scene is rendered both ways from the
//! *same frame* and compared byte for byte.
//!
//! The two share the fragment stage (`Varyings` in `libgui_soft`), which is
//! the point: what is under test is the **vertex** stage — the quad, the
//! anti-aliasing padding, and the interpolated varyings.

use libgui_soft::scenes;

/// The font the scenes are drawn with; the library does not embed one.
const SCENE_FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

use libgui::mesh::Mesh;
use libgui_soft::SoftRenderer;
use scenes::{SCALES, SCENES, THEMES};

#[test]
fn every_scene_renders_identically_from_triangles() {
    let mut compared = 0;
    for scene in SCENES {
        for (theme_name, theme) in THEMES {
            for scale in SCALES {
                scene.run(theme(), scale, SCENE_FONT, |out, (w, h)| {
                    let instanced = SoftRenderer::new().render_to_image(out, w, h);
                    let mut mesh = Mesh::new();
                    mesh.build(out.draw);
                    let expanded = SoftRenderer::new().render_mesh_to_image(out, &mesh, w, h);

                    assert_eq!(mesh.indices.len(), out.draw.instances.len() * 6);
                    let worst = instanced
                        .data
                        .iter()
                        .zip(&expanded.data)
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or(0);
                    let differing =
                        instanced.data.iter().zip(&expanded.data).filter(|(a, b)| a != b).count();
                    let total = instanced.data.len();

                    // The instanced path computes `local` as `world - centre`;
                    // the expanded one interpolates it across the quad, which
                    // is what a GPU does. At a fractional scale the two differ
                    // in the last bit on a handful of edge pixels, so the bar
                    // is the golden tests' own: one 8-bit step, and not one
                    // step beyond it. The *count* is bounded too, so a real
                    // disagreement cannot hide under the tolerance.
                    assert!(
                        worst <= 1,
                        "{}@{scale}x-{theme_name}: a channel differs by {worst} between the \
                         instanced and the expanded path — that is a bug, not rounding",
                        scene.name
                    );
                    assert!(
                        differing * 10_000 < total,
                        "{}@{scale}x-{theme_name}: {differing} of {total} channel values differ; \
                         rounding accounts for a few edge pixels, not for this many",
                        scene.name
                    );
                });
                compared += 1;
            }
        }
    }
    assert!(compared > 20, "only {compared} comparisons ran — the scene table did not load");
}

/// A frame of nothing expands to nothing, and still clears the target rather
/// than leaving it uninitialised.
#[test]
fn an_empty_frame_expands_to_nothing() {
    use libgui::*;
    let mut ui = Ui::new(Theme::dark(), include_bytes!("../../../assets/Inter.ttf")).expect("font");
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(64.0, 64.0), scale: 1.0, dt: 0.016 });
    let out = ui.end_frame();

    let mut mesh = Mesh::new();
    mesh.build(out.draw);
    assert!(mesh.vertices.is_empty() && mesh.indices.is_empty() && mesh.batches.is_empty());

    let img = SoftRenderer::new().render_mesh_to_image(&out, &mesh, 64, 64);
    let clear = img.pixel(0, 0);
    assert!(img.data.chunks_exact(4).all(|p| p == clear), "the empty target was not uniform");
}

/// The expansion is a fallback, not a free lunch. Pin the ratio the docs
/// quote, so it cannot drift without someone noticing.
#[test]
fn the_expansion_costs_what_the_docs_say() {
    let scene = SCENES.iter().max_by_key(|s| s.name.len()).expect("scenes");
    scene.run(scenes::THEMES[0].1(), 1.0, SCENE_FONT, |out, _| {
        let mut mesh = Mesh::new();
        mesh.build(out.draw);
        let instanced = out.draw.instances.len() * libgui::render_contract::INSTANCE_STRIDE;
        let expanded = mesh.vertices.len() * libgui::render_contract::VERTEX_STRIDE
            + mesh.indices.len() * std::mem::size_of::<u32>();
        let ratio = expanded as f32 / instanced as f32;
        assert!(
            (4.0..5.0).contains(&ratio),
            "expansion is {ratio:.2}x the instanced bytes; the docs say about four and a half"
        );
    });
}

/// Cutting the frame into chunks must not change a pixel.
///
/// A renderer streaming into a fixed per-frame buffer has a ceiling — bgfx's
/// transient buffer is about thirteen thousand quads — and going over it drops
/// the draw rather than slowing it. So the mesh can be split; the split has to
/// be invisible, and the chunks have to be inside the limits that caused it.
#[test]
fn chunking_changes_nothing_but_the_number_of_uploads() {
    // Small enough that every scene splits many times over.
    const MAX_VERTICES: u32 = 64; // 16 quads
    const MAX_INDICES: u32 = 96; // the same 16 quads

    let mut split_scenes = 0;
    for scene in SCENES {
        let (theme_name, theme) = THEMES[0];
        scene.run(theme(), 1.0, SCENE_FONT, |out, (w, h)| {
            let mut whole = Mesh::new();
            whole.build(out.draw);
            let mut cut = Mesh::new();
            cut.build_limited(out.draw, MAX_VERTICES, MAX_INDICES);

            // The same triangles, in the same order, however they are grouped.
            assert_eq!(cut.vertices, whole.vertices, "{}: chunking moved a vertex", scene.name);
            assert_eq!(cut.indices.len(), whole.indices.len(), "{}: chunking changed the index count", scene.name);

            // Every chunk is inside the limits it was cut for, and describes
            // itself consistently.
            for (n, chunk) in cut.chunks.iter().enumerate() {
                let verts = chunk.vertices.end - chunk.vertices.start;
                let idx = chunk.indices.end - chunk.indices.start;
                assert!(verts <= MAX_VERTICES, "{}: chunk {n} has {verts} vertices", scene.name);
                assert!(idx <= MAX_INDICES, "{}: chunk {n} has {idx} indices", scene.name);
                assert_eq!(idx, verts / 4 * 6, "{}: chunk {n} is not whole quads", scene.name);
                // Its indices address its own vertices, which is what lets a
                // host upload the slice and draw it untouched.
                for &i in &cut.indices[chunk.indices.start as usize..chunk.indices.end as usize] {
                    assert!(i < verts, "{}: chunk {n} indexes vertex {i} of {verts}", scene.name);
                }
                // And each of its batches addresses its own indices.
                for b in &cut.batches[chunk.batches.start as usize..chunk.batches.end as usize] {
                    assert!(b.indices.end <= idx, "{}: a batch runs past chunk {n}", scene.name);
                }
            }
            if cut.chunks.len() > 1 {
                split_scenes += 1;
            }
            assert!(cut.fits_u16(), "{}: chunks this small must fit 16-bit indices", scene.name);

            // The pixels: identical, not merely close.
            let one = SoftRenderer::new().render_mesh_to_image(out, &whole, w, h);
            let many = SoftRenderer::new().render_mesh_to_image(out, &cut, w, h);
            let differing = one.data.iter().zip(&many.data).filter(|(a, b)| a != b).count();
            assert_eq!(
                differing, 0,
                "{}@{theme_name}: {differing} bytes differ between one upload and {} chunks",
                scene.name,
                cut.chunks.len()
            );
        });
    }
    assert!(split_scenes > 3, "only {split_scenes} scenes were big enough to split; the test proves little");
}

/// A limit too small for one quad would describe a chunk that can hold
/// nothing, and a loop that never finishes.
#[test]
fn a_limit_below_one_quad_still_makes_progress() {
    let scene = scenes::SCENES.iter().find(|s| s.name == "widgets").expect("scene");
    scene.run(THEMES[0].1(), 1.0, SCENE_FONT, |out, _| {
        let mut mesh = Mesh::new();
        mesh.build_limited(out.draw, 1, 1);
        assert_eq!(mesh.indices.len(), out.draw.instances.len() * 6, "quads went missing");
        for chunk in &mesh.chunks {
            assert_eq!(chunk.vertices.end - chunk.vertices.start, 4, "a chunk holds one quad");
        }
        assert_eq!(mesh.chunks.len(), out.draw.instances.len(), "one quad per chunk");
    });
}
