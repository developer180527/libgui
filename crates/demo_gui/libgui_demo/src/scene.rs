//! The "engine" side of the demo. Everything here is yours, not the UI's: the UI
//! only ever sees the resulting texture view.

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct SceneParams {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub scale: f32,
    pub spin: f32,
}

pub struct Scene {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    vertex_count: u32,
    pub color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    pub size: (u32, u32),
}

fn geometry() -> Vec<[f32; 7]> {
    let mut v = Vec::new();
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ([-1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ([0., 1., 0.], [1., 0., 0.], [0., 0., 1.]),
        ([0., -1., 0.], [1., 0., 0.], [0., 0., 1.]),
        ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]),
        ([0., 0., -1.], [1., 0., 0.], [0., 1., 0.]),
    ];
    for (n, a, b) in faces {
        let corner = |s: f32, t: f32| {
            [n[0] + a[0] * s + b[0] * t, n[1] + a[1] * s + b[1] * t, n[2] + a[2] * s + b[2] * t]
        };
        for (s, t) in [(-1., -1.), (1., -1.), (1., 1.), (-1., -1.), (1., 1.), (-1., 1.)] {
            let p = corner(s, t);
            v.push([p[0], p[1], p[2], n[0], n[1], n[2], 0.0]);
        }
    }
    let e = 60.0;
    for (x, z) in [(-e, -e), (e, -e), (e, e), (-e, -e), (e, e), (-e, e)] {
        v.push([x, 0.0, z, 0.0, 1.0, 0.0, 1.0]);
    }
    v
}

impl Scene {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::util::DeviceExt;
        let shader = device.create_shader_module(wgpu::include_wgsl!("scene.wgsl"));
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 28,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniforms"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let geo = geometry();
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scene vertices"),
            contents: bytemuck::cast_slice(&geo),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let (color_view, depth_view) = Self::targets(device, 1, 1);
        Self {
            pipeline,
            uniforms,
            bind_group,
            vertices,
            vertex_count: geo.len() as u32,
            color_view,
            depth_view,
            size: (1, 1),
        }
    }

    fn targets(device: &wgpu::Device, w: u32, h: u32) -> (wgpu::TextureView, wgpu::TextureView) {
        let make = |format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("scene target"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        (
            make(FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING),
            make(DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT),
        )
    }

    /// Returns true if the colour target was recreated.
    pub fn resize(&mut self, device: &wgpu::Device, w: u32, h: u32) -> bool {
        let (w, h) = (w.clamp(1, 8192), h.clamp(1, 8192));
        if (w, h) == self.size {
            return false;
        }
        (self.color_view, self.depth_view) = Self::targets(device, w, h);
        self.size = (w, h);
        true
    }

    pub fn render(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, p: &SceneParams) {
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let u = [p.yaw, p.pitch, p.distance, p.scale, aspect, p.spin, 0.0, 0.0];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.075, g: 0.075, b: 0.082, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.vertex_count, 0..1);
    }
}
