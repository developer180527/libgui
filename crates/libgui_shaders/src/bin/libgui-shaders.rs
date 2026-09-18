//! Export the generated shaders to a directory:
//!   cargo run -p libgui_shaders -- path/to/out
fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "shaders_out".into());
    std::fs::create_dir_all(&dir).expect("create output dir");
    for (name, bytes) in libgui_shaders::files() {
        let path = std::path::Path::new(&dir).join(name);
        std::fs::write(&path, bytes).expect("write shader");
        println!("{} ({} bytes)", path.display(), bytes.len());
    }
}
