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
fn render_gpu(g: &Gpu, out: &libgui::FrameOutput, size: (u32, u32)) -> Vec<u8> {
    render_gpu_with(g, out, size, &[])
}

/// [`render_gpu`] with user textures registered first, under the ids the
/// frame refers to them by.
fn render_gpu_with(
    g: &Gpu,
    out: &libgui::FrameOutput,
    (w, h): (u32, u32),
    textures: &[(libgui::TextureId, &libgui_soft::Texture)],
) -> Vec<u8> {
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
    // Kept alive until the frame is drawn: the renderer holds only a view.
    let mut keep = Vec::new();
    for &(id, t) in textures {
        let size = wgpu::Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 };
        let tex = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("parity user texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        g.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &t.data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.width * 4), rows_per_image: Some(t.height) },
            size,
        );
        renderer.update_texture(id, &tex.create_view(&Default::default()));
        keep.push(tex);
    }
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


/// The image alpha modes, through the real shader, against the CPU reference.
///
/// The CPU side has its own tests for what each mode means; this is what
/// says the shader agrees, including the per-texel premultiply that keeps a
/// straight-alpha icon's edges clean. Every edge is filtered: an 8x8 texture
/// drawn into 100 px.
#[test]
fn the_image_alpha_modes_match_the_gpu() {
    let Some(g) = gpu() else {
        eprintln!("parity: no GPU adapter, skipped");
        return;
    };
    // A coloured square on a transparent field whose texels are black: what
    // decides whether the filter order is right.
    let icon = {
        let mut data = Vec::new();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let core = (2..6).contains(&x) && (2..6).contains(&y);
                data.extend_from_slice(if core { &[40, 160, 220, 255] } else { &[0, 0, 0, 0] });
            }
        }
        libgui_soft::Texture { width: 8, height: 8, data }
    };

    for alpha in [libgui::ImageAlpha::Opaque, libgui::ImageAlpha::Premultiplied, libgui::ImageAlpha::Straight] {
        for scale in [1.0f32, 1.5] {
            let mut soft = SoftRenderer::new();
            let id = soft.register_texture(icon.clone());
            let mut ui = libgui::Ui::new(libgui::Theme::light(), SCENE_FONT).expect("font");
            let size = libgui::Vec2::new(160.0, 160.0);
            let info = libgui::FrameInfo { screen_size: size, scale, dt: 1.0 };
            let build = |ui: &mut libgui::Ui| {
                ui.begin_frame(info);
                ui.add_leaf(
                    libgui::Id::new("icon"),
                    libgui::Layout::leaf(libgui::Size::Fixed(100.0), libgui::Size::Fixed(100.0)),
                    libgui::Vec2::ZERO,
                    false,
                    move |p, r| {
                        // Faded, so the tint's alpha is part of what is compared.
                        let tint = libgui::Color::WHITE.with_alpha(0.75);
                        p.image_with_alpha(r, id, [0.0, 0.0, 1.0, 1.0], 6.0, tint, alpha);
                    },
                );
            };
            build(&mut ui);
            drop(ui.end_frame());
            build(&mut ui);
            let out = ui.end_frame();
            let px = ((size.x * scale) as u32, (size.y * scale) as u32);
            let cpu = soft.render_to_image(&out, px.0, px.1).data;
            let gpu_px = render_gpu_with(&g, &out, px, &[(id, &icon)]);

            let total = (px.0 * px.1) as usize;
            let (cpu_px, _) = cpu.as_chunks::<4>();
            let (gpu_chunks, _) = gpu_px.as_chunks::<4>();
            let disagree = cpu_px
                .iter()
                .zip(gpu_chunks)
                .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > CLOSE))
                .count();
            assert!(
                (disagree as f64) <= total as f64 * MAX_DISAGREE,
                "{alpha:?}@{scale}x: {disagree} of {total} pixels differ between the shader and the CPU reference"
            );
        }
    }
}

/// Turned images, through the real shader, against the CPU reference: the
/// vertex stage's rotation, the anti-aliasing pixel a turned edge is given,
/// and the texture mapping carried past it, at angles that put edges on
/// pixel centres (a quarter turn) and between them.
#[test]
fn rotated_images_match_the_gpu() {
    let Some(g) = gpu() else {
        eprintln!("parity: no GPU adapter, skipped");
        return;
    };
    // A gradient with a hard bar, so a texture mapped wrongly shows.
    let tex = {
        let mut data = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                let bar = (6..10).contains(&x);
                data.extend_from_slice(&if bar { [240, 240, 40, 255] } else { [(x * 16) as u8, (y * 16) as u8, 160, 255] });
            }
        }
        libgui_soft::Texture { width: 16, height: 16, data }
    };
    for angle in [0.3f32, -std::f32::consts::FRAC_PI_2, 2.5] {
        for (alpha, radius) in [(libgui::ImageAlpha::Opaque, 0.0f32), (libgui::ImageAlpha::Straight, 10.0)] {
            for scale in [1.0f32, 1.5] {
                let mut soft = SoftRenderer::new();
                let id = soft.register_texture(tex.clone());
                let mut ui = libgui::Ui::new(libgui::Theme::dark(), SCENE_FONT).expect("font");
                let size = libgui::Vec2::new(160.0, 160.0);
                let info = libgui::FrameInfo { screen_size: size, scale, dt: 1.0 };
                let build = |ui: &mut libgui::Ui| {
                    ui.begin_frame(info);
                    ui.add_leaf(
                        libgui::Id::new("knob"),
                        libgui::Layout::leaf(libgui::Size::Fixed(160.0), libgui::Size::Fixed(160.0)),
                        libgui::Vec2::ZERO,
                        false,
                        move |p, _| {
                            let r = libgui::Rect::new(40.3, 30.0, 80.0, 100.0);
                            p.image_rotated(r, id, [0.0, 0.0, 1.0, 1.0], radius, libgui::Color::WHITE.with_alpha(0.9), alpha, angle);
                        },
                    );
                };
                build(&mut ui);
                drop(ui.end_frame());
                build(&mut ui);
                let out = ui.end_frame();
                let px = ((size.x * scale) as u32, (size.y * scale) as u32);
                let cpu = soft.render_to_image(&out, px.0, px.1).data;
                let gpu_px = render_gpu_with(&g, &out, px, &[(id, &tex)]);
                let total = (px.0 * px.1) as usize;
                let (cpu_px, _) = cpu.as_chunks::<4>();
                let (gpu_chunks, _) = gpu_px.as_chunks::<4>();
                let disagree = cpu_px
                    .iter()
                    .zip(gpu_chunks)
                    .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > CLOSE))
                    .count();
                assert!(
                    (disagree as f64) <= total as f64 * MAX_DISAGREE,
                    "{angle} rad {alpha:?}@{scale}x: {disagree} of {total} pixels differ between the shader and the CPU reference"
                );
            }
        }
    }
}

/// No seams on the GPU itself: a translucent mesh whose shared edges run
/// along pixel centres (at 1x) and between them (at 1.5x and 2x) is exactly
/// one layer everywhere inside. The CPU reference cannot see the failure this
/// guards against — two triangles interpolating a pixel's position a last bit
/// apart and both claiming it, or neither — so it is checked here.
#[test]
fn a_translucent_mesh_has_no_seams_on_the_gpu() {
    let Some(g) = gpu() else {
        eprintln!("parity: no GPU adapter, skipped");
        return;
    };
    for scale in [1.0f32, 1.5, 2.0] {
        let mut ui = libgui::Ui::new(libgui::Theme::dark(), SCENE_FONT).expect("font");
        let size = libgui::Vec2::new(120.0, 120.0);
        ui.begin_frame(libgui::FrameInfo { screen_size: size, scale, dt: 1.0 });
        ui.container(
            libgui::Layout::column().width(libgui::Size::Grow(1.0)).height(libgui::Size::Grow(1.0)),
            libgui::Frame { fill: libgui::Color::BLACK, ..libgui::Frame::none() },
            |ui| {
                ui.add_leaf(libgui::Id::new("m"), libgui::Layout::leaf(libgui::Size::Grow(1.0), libgui::Size::Grow(1.0)), libgui::Vec2::ZERO, false, |p, _| {
                    let mut pts = Vec::new();
                    for j in 0..5 {
                        for i in 0..5 {
                            pts.push(libgui::Vec2::new(10.5 + i as f32 * 20.0, 10.5 + j as f32 * 20.0));
                        }
                    }
                    // A fan into the middle too: many edges meeting at a point.
                    let mut idx = Vec::new();
                    for j in 0..4u32 {
                        for i in 0..4u32 {
                            let (a, b, c, d) = (j * 5 + i, j * 5 + i + 1, (j + 1) * 5 + i + 1, (j + 1) * 5 + i);
                            if (i + j) % 2 == 0 { idx.extend([a, b, c, a, c, d]) } else { idx.extend([a, b, d, b, c, d]) }
                        }
                    }
                    p.fill_mesh(&pts, &idx, libgui::Color::rgba(1.0, 1.0, 1.0, 0.5));
                });
            },
        );
        let out = ui.end_frame();
        let px = ((size.x * scale) as u32, (size.y * scale) as u32);
        let img = render_gpu(&g, &out, px);
        let lo = (13.0 * scale).ceil() as u32;
        let hi = (88.0 * scale).floor() as u32;
        let mut bad = Vec::new();
        for y in lo..hi {
            for x in lo..hi {
                let v = img[((y * px.0 + x) * 4) as usize];
                if v != 128 {
                    bad.push((x, y, v));
                }
            }
        }
        assert!(bad.is_empty(), "@{scale}x: {} pixels not exactly one layer, e.g. {:?}", bad.len(), &bad[..bad.len().min(6)]);
    }
}
