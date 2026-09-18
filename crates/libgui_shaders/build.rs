//! Validates `shaders/ui.wgsl` with naga and cross-compiles it for every RHI
//! family. A shader error fails the build with naga's diagnostic.

use naga::back::{glsl, hlsl, msl, spv};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::ShaderStage;
use std::path::Path;

const VS: &str = "vs_main";
const FS: &str = "fs_main";

fn main() {
    let src_path = "shaders/ui.wgsl";
    println!("cargo:rerun-if-changed={src_path}");
    let source = std::fs::read_to_string(src_path).expect("read ui.wgsl");
    let out = std::env::var("OUT_DIR").unwrap();
    let out = Path::new(&out);

    let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| {
        panic!("\n{}", e.emit_to_string_with_path(&source, src_path));
    });
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("\n{}", e.emit_to_string_with_path(&source, src_path)));

    // HLSL (D3D11/D3D12): both entry points in one file.
    // WGSL @group(g) @binding(b) maps to register(bN/sN/tN, space g).
    let mut hlsl_src = String::new();
    let hlsl_opts = hlsl::Options { shader_model: hlsl::ShaderModel::V5_1, ..Default::default() };
    hlsl::Writer::new(&mut hlsl_src, &hlsl_opts, &Default::default())
        .write(&module, &info, None)
        .expect("HLSL generation");

    // MSL (Metal): both entry points.
    let msl_opts = msl::Options { lang_version: (2, 0), ..Default::default() };
    let (msl_src, _) = msl::write_string(&module, &info, &msl_opts, &Default::default()).expect("MSL generation");

    // GLSL 4.50 (OpenGL / Vulkan-GLSL toolchains): one file per stage.
    let glsl_stage = |stage, entry: &str| {
        let mut s = String::new();
        // Fixed slots: UBO binding 0 = Globals, texture unit 0 = UI texture.
        let mut binding_map = glsl::BindingMap::default();
        binding_map.insert(naga::ResourceBinding { group: 0, binding: 0 }, 0);
        binding_map.insert(naga::ResourceBinding { group: 1, binding: 0 }, 0);
        let opts = glsl::Options { version: glsl::Version::Desktop(450), binding_map, ..Default::default() };
        let pipe = glsl::PipelineOptions { shader_stage: stage, entry_point: entry.into(), multiview: None };
        glsl::Writer::new(&mut s, &module, &info, &opts, &pipe, Default::default())
            .and_then(|mut w| w.write())
            .expect("GLSL generation");
        s
    };
    let glsl_vs = glsl_stage(ShaderStage::Vertex, VS);
    let glsl_fs = glsl_stage(ShaderStage::Fragment, FS);

    // SPIR-V (Vulkan): one module containing both entry points.
    let words = spv::write_vec(&module, &info, &spv::Options::default(), None).expect("SPIR-V generation");
    let spv_bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();

    std::fs::write(out.join("ui.wgsl"), &source).unwrap();
    std::fs::write(out.join("ui.hlsl"), hlsl_src).unwrap();
    std::fs::write(out.join("ui.metal"), msl_src).unwrap();
    std::fs::write(out.join("ui.vert.glsl"), glsl_vs).unwrap();
    std::fs::write(out.join("ui.frag.glsl"), glsl_fs).unwrap();
    std::fs::write(out.join("ui.spv"), spv_bytes).unwrap();
}
