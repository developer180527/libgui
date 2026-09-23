//! The contract between libgui and a GPU API. The exact data layout, bindings
//! and pixel rules are in [`crate::render_contract`].
//!
//! libgui never talks to a GPU. Each frame it produces a [`FrameOutput`]:
//! a list of POD [`Instance`]s grouped into [`Batch`]es by texture, a glyph
//! [`Atlas`], and [`Globals`]. A backend uploads that and issues one instanced
//! draw per batch using the shared shader from the `libgui_shaders` crate.
//!
//! Implementing [`Backend`] for your own RHI (D3D12, Vulkan, Metal, a console
//! API, or an engine abstraction over them) is typically a few hundred lines:
//!
//! 1. Create one pipeline from `libgui_shaders` (HLSL / MSL / SPIR-V / GLSL / WGSL):
//!    instance-rate vertex buffer of six `float4`s (96-byte stride), triangle list,
//!    premultiplied-alpha blend, no depth.
//! 2. [`Backend::prepare`]: write [`Globals`] to a 16-byte uniform buffer, copy
//!    `frame.draw.instances` into a (per-frame-in-flight) instance buffer, and
//!    re-upload the R8 atlas when [`Atlas::version`] changed.
//! 3. [`Backend::begin`]: bind pipeline, globals, instance buffer.
//! 4. [`Backend::draw`]: bind the texture for `TextureId` (atlas or your own,
//!    e.g. a viewport render target), then draw `6` vertices × `instances`.
//!
//! [`Backend::render`] drives 3–4 for a whole frame.

use crate::{Atlas, Batch, FrameOutput, Instance, TextureId};
use std::ops::Range;

pub use crate::render_contract::{INSTANCE_STRIDE, VERTICES_PER_INSTANCE};

/// Uniform block at group 0, binding 0.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Globals {
    /// Target size in logical pixels.
    pub screen_size: [f32; 2],
    /// Physical pixels per logical pixel (DPI scale).
    pub scale: f32,
    pub _pad: f32,
}

impl FrameOutput<'_> {
    pub fn globals(&self) -> Globals {
        Globals { screen_size: [self.screen_size.x, self.screen_size.y], scale: self.scale, _pad: 0.0 }
    }

    pub fn instances(&self) -> &[Instance] {
        &self.draw.instances
    }

    pub fn batches(&self) -> &[Batch] {
        &self.draw.batches
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }
}

/// Implement this for your RHI. See the module docs for the full contract.
pub trait Backend {
    /// What draw commands are recorded into: `wgpu::RenderPass`, a D3D12
    /// command-list wrapper, a Vulkan command buffer, your RHI's context…
    type Pass<'p>;

    /// Upload globals, instances and (if its version changed) the atlas.
    /// Called once per frame, before `render`.
    fn prepare(&mut self, frame: &FrameOutput);

    /// Bind pipeline, globals and instance buffer.
    fn begin(&mut self, pass: &mut Self::Pass<'_>);

    /// Bind `texture` at group 1 and draw `VERTICES_PER_INSTANCE` vertices for
    /// each instance in `instances`. Skip silently if the texture is unknown.
    fn draw(&mut self, pass: &mut Self::Pass<'_>, texture: TextureId, instances: Range<u32>);

    /// Record the whole UI into `pass`. The default is right for most backends.
    fn render(&mut self, pass: &mut Self::Pass<'_>, frame: &FrameOutput) {
        self.render_batches(pass, frame.batches());
    }

    /// Draw batches the backend has already prepared, without a
    /// [`FrameOutput`].
    ///
    /// For the host that skipped a UI frame because
    /// [`Ui::needs_frame`](crate::Ui::needs_frame) said nothing changed: the
    /// instance buffer and the atlas are still the ones `prepare` uploaded, so
    /// the only thing missing is the list of draws. Keep a copy of
    /// [`FrameOutput::batches`] from the last frame that *did* run and hand it
    /// back here. Nothing else needs re-uploading — including a user texture
    /// the app is still redrawing into, which is how a viewport keeps
    /// animating while the UI around it costs nothing.
    fn render_batches(&mut self, pass: &mut Self::Pass<'_>, batches: &[crate::Batch]) {
        if batches.is_empty() {
            return;
        }
        self.begin(pass);
        for b in batches {
            self.draw(pass, b.texture, b.range.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Theme, Ui};

    /// A backend that records calls: the smallest possible implementation,
    /// and a template for a custom RHI.
    #[derive(Default)]
    struct Recorder {
        atlas_version: u64,
        uploads: usize,
        instances: usize,
    }

    impl Backend for Recorder {
        type Pass<'p> = Vec<String>;

        fn prepare(&mut self, frame: &FrameOutput) {
            if frame.atlas().version != self.atlas_version {
                self.atlas_version = frame.atlas().version;
                self.uploads += 1;
            }
            self.instances = frame.instances().len();
        }

        fn begin(&mut self, pass: &mut Vec<String>) {
            pass.push("begin".into());
        }

        fn draw(&mut self, pass: &mut Vec<String>, texture: TextureId, instances: Range<u32>) {
            pass.push(format!("{texture:?} {instances:?}"));
        }
    }

    #[test]
    fn backend_receives_batched_draws() {
        let font = include_bytes!("../../../assets/Inter.ttf");
        let mut ui = Ui::new(Theme::dark(), font).unwrap();
        let mut backend = Recorder::default();
        for frame in 0..2 {
            ui.begin_frame(Default::default());
            ui.label("hello");
            let _ = ui.button("OK");
            ui.viewport("vp", TextureId::User(3), |_, _| {});
            ui.label("after viewport");
            let out = ui.end_frame();
            backend.prepare(&out);
            let mut pass = Vec::new();
            backend.render(&mut pass, &out);
            assert_eq!(out.globals().scale, 1.0);
            // Texture switches split batches: atlas → user viewport → atlas.
            assert_eq!(pass.len(), 4, "frame {frame}: {pass:?}");
            assert!(pass[2].starts_with("User(3)"));
            let last = out.batches().last().unwrap();
            assert_eq!(last.range.end as usize, backend.instances);
        }
        // The atlas is only re-uploaded when glyphs were added.
        assert_eq!(backend.uploads, 1);
    }
}
