//! wgpu implementation of [`libgui::Backend`]. Also the reference for writing
//! a backend for your own RHI: the whole thing is one pipeline, one uniform
//! buffer, one instance buffer and a texture per `TextureId`.

use libgui::{Backend, FrameOutput, TextureId, INSTANCE_STRIDE, VERTICES_PER_INSTANCE};

// The shaders must have been generated for the contract this crate is built against.
const _: () = assert!(libgui_shaders::CONTRACT_VERSION == libgui::render_contract::CONTRACT_VERSION);
use std::collections::HashMap;
use std::ops::Range;

const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4
];

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    atlas: wgpu::Texture,
    atlas_bg: wgpu::BindGroup,
    atlas_size: u32,
    atlas_version: u64,
    instances: wgpu::Buffer,
    capacity: usize,
    user: HashMap<u32, wgpu::BindGroup>,
    next_user: u32,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("libgui globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("libgui texture"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("libgui ui.wgsl"),
            source: wgpu::ShaderSource::Wgsl(libgui_shaders::WGSL.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("libgui"),
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("libgui"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(libgui_shaders::VERTEX_ENTRY),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_STRIDE as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRS,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(libgui_shaders::FRAGMENT_ENTRY),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libgui globals"),
            size: std::mem::size_of::<libgui::Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("libgui globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });

        let (atlas, atlas_bg) = Self::create_atlas(device, &tex_layout, 1);
        let capacity = 4096;
        let instances = Self::create_instances(device, capacity);

        Self {
            device: device.clone(),
            queue: queue.clone(),
            pipeline,
            globals,
            globals_bg,
            tex_layout,
            atlas,
            atlas_bg,
            atlas_size: 1,
            atlas_version: 0,
            instances,
            capacity,
            user: HashMap::new(),
            next_user: 0,
        }
    }

    fn create_atlas(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: u32) -> (wgpu::Texture, wgpu::BindGroup) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("libgui atlas"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("libgui atlas"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
        });
        (tex, bg)
    }

    fn create_instances(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libgui instances"),
            size: (capacity * INSTANCE_STRIDE) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Register a texture (e.g. your viewport's colour target) for `ui.viewport`.
    pub fn register_texture(&mut self, view: &wgpu::TextureView) -> TextureId {
        let id = TextureId::User(self.next_user);
        self.next_user += 1;
        self.update_texture(id, view);
        id
    }

    /// Point an existing id at a new view (after resizing a render target).
    pub fn update_texture(&mut self, id: TextureId, view: &wgpu::TextureView) {
        if let TextureId::User(n) = id {
            let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("libgui user texture"),
                layout: &self.tex_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }],
            });
            self.user.insert(n, bg);
        }
    }

    pub fn unregister_texture(&mut self, id: TextureId) {
        if let TextureId::User(n) = id {
            self.user.remove(&n);
        }
    }
}

impl Backend for Renderer {
    type Pass<'p> = wgpu::RenderPass<'p>;

    fn prepare(&mut self, frame: &FrameOutput) {
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&frame.globals()));

        let atlas = frame.atlas();
        if atlas.size != self.atlas_size {
            let (t, bg) = Self::create_atlas(&self.device, &self.tex_layout, atlas.size);
            self.atlas = t;
            self.atlas_bg = bg;
            self.atlas_size = atlas.size;
            self.atlas_version = 0;
        }
        if atlas.version != self.atlas_version {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &atlas.data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(atlas.size), rows_per_image: Some(atlas.size) },
                wgpu::Extent3d { width: atlas.size, height: atlas.size, depth_or_array_layers: 1 },
            );
            self.atlas_version = atlas.version;
        }

        let inst = frame.instances();
        if inst.len() > self.capacity {
            self.capacity = inst.len().next_power_of_two();
            self.instances = Self::create_instances(&self.device, self.capacity);
        }
        if !inst.is_empty() {
            self.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(inst));
        }
    }

    fn begin(&mut self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals_bg, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
    }

    fn draw(&mut self, pass: &mut wgpu::RenderPass<'_>, texture: TextureId, instances: Range<u32>) {
        let bg = match texture {
            TextureId::Atlas => &self.atlas_bg,
            TextureId::User(n) => match self.user.get(&n) {
                Some(bg) => bg,
                None => return,
            },
        };
        pass.set_bind_group(1, bg, &[]);
        pass.draw(0..VERTICES_PER_INSTANCE, instances);
    }
}
