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
    println!("cargo:rerun-if-changed=../libgui/src/render_contract.rs");
    let body = std::fs::read_to_string(src_path).expect("read ui.wgsl");
    // Constants come from the contract, so the shader can't disagree with the core.
    let source = format!("{}\n{body}", libgui::render_contract::wgsl_prelude());
    let out = std::env::var("OUT_DIR").unwrap();
    let out = Path::new(&out);

    let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| {
        panic!("\n{}", e.emit_to_string_with_path(&source, src_path));
    });
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("\n{}", e.emit_to_string_with_path(&source, src_path)));
    check_contract(&module);

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
    std::fs::write(out.join("contract_version.rs"), libgui::render_contract::CONTRACT_VERSION.to_string()).unwrap();
    std::fs::write(out.join("ui.hlsl"), hlsl_src).unwrap();
    std::fs::write(out.join("ui.metal"), msl_src).unwrap();
    std::fs::write(out.join("ui.vert.glsl"), glsl_vs).unwrap();
    std::fs::write(out.join("ui.frag.glsl"), glsl_fs).unwrap();
    std::fs::write(out.join("ui.spv"), spv_bytes).unwrap();
}

/// Fail the build if the shader's interface disagrees with libgui::render_contract.
fn check_contract(m: &naga::Module) {
    use libgui::render_contract as rc;
    use naga::{AddressSpace, Binding, ScalarKind, ShaderStage, TypeInner, VectorSize};

    let entry = |name: &str, stage| {
        m.entry_points
            .iter()
            .find(|e| e.name == name && e.stage == stage)
            .unwrap_or_else(|| panic!("render contract: missing {stage:?} entry point `{name}`"))
    };
    let vs = entry(rc::VERTEX_ENTRY, ShaderStage::Vertex);
    entry(rc::FRAGMENT_ENTRY, ShaderStage::Fragment);

    // Instance inputs: exactly the contract's locations, each a vec4<f32>.
    let mut locations = Vec::new();
    let mut collect = |ty: naga::Handle<naga::Type>, binding: &Option<Binding>| match (&m.types[ty].inner, binding) {
        (TypeInner::Struct { members, .. }, None) => {
            for mem in members {
                if let Some(Binding::Location { location, .. }) = mem.binding {
                    locations.push((location, &m.types[mem.ty].inner));
                }
            }
        }
        (inner, Some(Binding::Location { location, .. })) => locations.push((*location, inner)),
        _ => {}
    };
    for arg in &vs.function.arguments {
        collect(arg.ty, &arg.binding);
    }
    locations.sort_by_key(|l| l.0);
    let expected: Vec<u32> = rc::INSTANCE_ATTRIBUTES.iter().map(|a| a.0).collect();
    let got: Vec<u32> = locations.iter().map(|l| l.0).collect();
    assert_eq!(got, expected, "render contract: vertex input locations");
    for (loc, inner) in locations {
        let ok = matches!(inner, TypeInner::Vector { size: VectorSize::Quad, scalar } if scalar.kind == ScalarKind::Float && scalar.width == 4);
        assert!(ok, "render contract: location {loc} must be vec4<f32>");
    }

    // Resource bindings.
    let mut globals = false;
    let mut texture = false;
    for (_, var) in m.global_variables.iter() {
        let Some(b) = &var.binding else { continue };
        match (&m.types[var.ty].inner, var.space) {
            (TypeInner::Struct { span, .. }, AddressSpace::Uniform) => {
                assert_eq!((b.group, b.binding), (rc::GLOBALS_GROUP, rc::GLOBALS_BINDING), "render contract: globals binding");
                assert_eq!(*span as usize, rc::GLOBALS_SIZE, "render contract: globals size");
                globals = true;
            }
            (TypeInner::Image { .. }, _) => {
                assert_eq!((b.group, b.binding), (rc::TEXTURE_GROUP, rc::TEXTURE_BINDING), "render contract: texture binding");
                texture = true;
            }
            (inner, space) => panic!("render contract: unexpected resource {inner:?} in {space:?} (e.g. a sampler)"),
        }
    }
    assert!(globals && texture, "render contract: shader must bind the globals block and one texture");
}
