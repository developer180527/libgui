//! The CPU renderer (`libgui_soft`) against the real shader on a real GPU.
//!
//! Golden images are rendered on the CPU, so they are only worth trusting if
//! the CPU renderer draws what the GPU draws. This renders every golden scene
//! both ways and compares. GPUs differ slightly from each other and from the
//! CPU (interpolation precision, rounding at pixel centres), so the check is a
//! tolerance, not equality: nearly every pixel within a couple of steps, and no
//! region where they disagree outright.
//!
//! Skipped, with a note, when no GPU adapter is available.

use libgui_soft::scenes;

/// The font the scenes are drawn with; the library does not embed one.
const SCENE_FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

use libgui::Backend;
use libgui_soft::SoftRenderer;
use scenes::{SCALES, SCENES, THEMES};

/// A pixel "agrees" if no channel differs by more than this.
const CLOSE: u8 = 3;
/// At most this fraction of pixels may disagree.
const MAX_DISAGREE: f64 = 0.002;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> Option<Gpu> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let desc = wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() };
    let (device, queue) = pollster::block_on(adapter.request_device(&desc)).ok()?;
    Some(Gpu { device, queue })
}

/// Render one frame through `libgui_wgpu` into an offscreen RGBA8 target and
/// read it back, rows tightly packed.
fn render_gpu(g: &Gpu, out: &libgui::FrameOutput, (w, h): (u32, u32)) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("parity target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut renderer = libgui_wgpu::Renderer::new(&g.device, &g.queue, format);
    renderer.prepare(out);

    let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("parity readback"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = g.device.create_command_encoder(&Default::default());
    {
        let c = out.clear_color;
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("parity"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // As libgui_soft::Target::new: premultiplied clear colour.
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: (c.r * c.a) as f64,
                        g: (c.g * c.a) as f64,
                        b: (c.b * c.a) as f64,
                        a: c.a as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.render(&mut pass, out);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    readback.map_async(wgpu::MapMode::Read, .., |r| r.expect("map readback"));
    g.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    let data = readback.get_mapped_range(..).expect("mapped range");
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let start = (y * row) as usize;
        px.extend_from_slice(&data[start..start + (w * 4) as usize]);
    }
    px
}

#[test]
fn the_cpu_renderer_matches_the_gpu() {
    let Some(g) = gpu() else {
        eprintln!("parity: no GPU adapter, skipped");
        return;
    };
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for scene in SCENES {
        for (theme_name, theme) in THEMES {
            for scale in SCALES {
                let (gpu_px, cpu) = scene.run(theme(), scale, SCENE_FONT, |out, size| {
                    (render_gpu(&g, out, size), SoftRenderer::new().render_to_image(out, size.0, size.1))
                });
                let (mut disagree, mut worst, mut exact) = (0usize, 0u8, 0usize);
                for (a, b) in gpu_px.chunks(4).zip(cpu.data.chunks(4)) {
                    let d = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
                    worst = worst.max(d);
                    exact += (d == 0) as usize;
                    disagree += (d > CLOSE) as usize;
                }
                let n = (cpu.width * cpu.height) as usize;
                let frac = disagree as f64 / n as f64;
                let line = format!(
                    "{:>15}@{scale}x-{theme_name:5}  exact {:6.2}%  off>{CLOSE} {:6.3}%  worst {worst}",
                    scene.name,
                    100.0 * exact as f64 / n as f64,
                    100.0 * frac,
                );
                if frac > MAX_DISAGREE {
                    failures.push(line.clone());
                }
                report.push(line);
            }
        }
    }
    eprintln!("{}", report.join("\n"));
    assert!(failures.is_empty(), "the CPU renderer disagrees with the GPU:\n{}", failures.join("\n"));
}
