//! Golden images: every scene in `scenes/mod.rs`, at every scale and theme,
//! rendered by the CPU backend and compared with the PNGs in `tests/golden/`.
//!
//! The CPU backend renders the same bytes on every machine, so the tolerance is
//! one 8-bit step per channel (for the odd last-bit difference in the maths
//! libgui's layout uses) and not one pixel beyond it.
//!
//! When a change to the look is intended:
//!
//! ```text
//! LIBGUI_BLESS=1 cargo test -p libgui_soft --test golden
//! ```
//!
//! rewrites the PNGs; review them in the diff like any other change. On a
//! mismatch the test writes `<name>.actual.png` and `<name>.diff.png`
//! (changed pixels in magenta over a faded copy of the expected image) next to
//! the build, and prints where.

use libgui_soft::scenes;

/// The font the scenes are drawn with; the library does not embed one.
const SCENE_FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

use libgui_soft::{SoftRenderer, Target};
use scenes::{Scene, SCALES, THEMES};
use std::path::{Path, PathBuf};

/// Largest allowed per-channel difference.
const TOLERANCE: u8 = 1;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden")
}

fn bless() -> bool {
    std::env::var("LIBGUI_BLESS").is_ok_and(|v| v == "1")
}

fn file_name(scene: &Scene, scale: f32, theme: &str) -> String {
    format!("{}@{scale}x-{theme}.png", scene.name)
}

fn render(scene: &Scene, theme: fn() -> libgui::Theme, scale: f32) -> Target {
    scene.run(theme(), scale, SCENE_FONT, |out, (w, h)| SoftRenderer::new().render_to_image(out, w, h))
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::High);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn read_png(path: &Path) -> Option<Target> {
    let file = std::fs::File::open(path).ok()?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    assert_eq!(info.color_type, png::ColorType::Rgba, "{}: goldens are RGBA8", path.display());
    buf.truncate(info.buffer_size());
    Some(Target { width: info.width, height: info.height, data: buf })
}

/// Pixels differing by more than the tolerance, and the largest difference.
fn compare(expected: &Target, actual: &Target) -> (usize, u8) {
    let mut bad = 0;
    let mut worst = 0;
    for (e, a) in expected.data.chunks(4).zip(actual.data.chunks(4)) {
        let d = e.iter().zip(a).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        worst = worst.max(d);
        if d > TOLERANCE {
            bad += 1;
        }
    }
    (bad, worst)
}

fn diff_image(expected: &Target, actual: &Target) -> Vec<u8> {
    let mut out = Vec::with_capacity(expected.data.len());
    for (e, a) in expected.data.chunks(4).zip(actual.data.chunks(4)) {
        let d = e.iter().zip(a).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if d > TOLERANCE {
            out.extend_from_slice(&[255, 0, 255, 255]);
        } else {
            let grey = ((e[0] as u32 + e[1] as u32 + e[2] as u32) / 3 / 4 + 32) as u8;
            out.extend_from_slice(&[grey, grey, grey, 255]);
        }
    }
    out
}

/// Check one scene at every scale and theme; report every variant that
/// differs, not just the first.
fn check(name: &str) {
    let scene = scenes::scene(name);
    let mut failures = Vec::new();
    for (theme_name, theme) in THEMES {
        for scale in SCALES {
            let file = file_name(scene, scale, theme_name);
            let path = golden_dir().join(&file);
            let actual = render(scene, theme, scale);
            if bless() {
                std::fs::create_dir_all(golden_dir()).unwrap();
                write_png(&path, actual.width, actual.height, &actual.data);
                continue;
            }
            let Some(expected) = read_png(&path) else {
                failures.push(format!("{file}: no golden image (run with LIBGUI_BLESS=1 to create it)"));
                continue;
            };
            if (expected.width, expected.height) != (actual.width, actual.height) {
                failures.push(format!(
                    "{file}: size {}x{}, expected {}x{}",
                    actual.width, actual.height, expected.width, expected.height
                ));
                continue;
            }
            let (bad, worst) = compare(&expected, &actual);
            if bad > 0 {
                let dir = out_dir();
                std::fs::create_dir_all(&dir).unwrap();
                let stem = file.trim_end_matches(".png");
                let (a, d) = (dir.join(format!("{stem}.actual.png")), dir.join(format!("{stem}.diff.png")));
                write_png(&a, actual.width, actual.height, &actual.data);
                write_png(&d, actual.width, actual.height, &diff_image(&expected, &actual));
                failures.push(format!("{file}: {bad} pixels differ (worst by {worst}); see {}", d.display()));
            }
        }
    }
    assert!(failures.is_empty(), "golden images differ:\n  {}", failures.join("\n  "));
}

macro_rules! goldens {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                check(stringify!($name));
            }
        )*

        /// Every scene has a test, so none can be added and forgotten.
        #[test]
        fn every_scene_has_a_golden_test() {
            let tested = [$(stringify!($name)),*];
            for s in scenes::SCENES {
                assert!(tested.contains(&s.name), "scene `{}` has no golden test: add it to goldens!", s.name);
            }
        }
    };
}

goldens!(widgets, tree, shapes, text, text_area, button_hover, button_pressed, tooltip, text_focused, combo_open, menu_open, scroll_mid, drag_reorder, table, focus_ring, paragraph);

/// The same scene renders to the same bytes twice in one process: nothing in
/// the pipeline depends on the clock, hashing seeds or allocation order.
#[test]
fn rendering_is_deterministic() {
    let s = scenes::scene("widgets");
    let a = render(s, libgui::Theme::dark, 1.5);
    let b = render(s, libgui::Theme::dark, 1.5);
    assert!(a == b, "two renders of the same scene differ");
}
