//! The committed header must be what the table would emit.
//!
//! A header that has drifted from the library is not a compile error on the C
//! side — it is a call through a signature that moved, which corrupts memory
//! somewhere else entirely and is found days later. So it is a build failure
//! here instead.
//!
//! Set `LIBGUI_WRITE_HEADER=1` to update the committed file after changing the
//! table.

use std::path::Path;

#[test]
fn the_committed_header_matches_the_table() {
    let want = libgui_c::header::render();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(libgui_c::header::HEADER_PATH);

    if std::env::var("LIBGUI_WRITE_HEADER").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, &want).expect("write header");
        return;
    }

    let have = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("{}: {e}\nRun `LIBGUI_WRITE_HEADER=1 cargo test -p libgui_c` to create it.", path.display())
    });

    if have != want {
        // Show the first line that differs rather than the whole file.
        let (h, w) = (have.lines().collect::<Vec<_>>(), want.lines().collect::<Vec<_>>());
        let at = h.iter().zip(w.iter()).position(|(a, b)| a != b).unwrap_or(h.len().min(w.len()));
        panic!(
            "{} is stale at line {}:\n  committed: {:?}\n  table says: {:?}\n\
             Run `LIBGUI_WRITE_HEADER=1 cargo test -p libgui_c` to update it.",
            path.display(),
            at + 1,
            h.get(at).unwrap_or(&"<end of file>"),
            w.get(at).unwrap_or(&"<end of file>"),
        );
    }
}

/// Every entry in the table is a real exported symbol, spelled the same way.
/// The table is the only place a widget is declared, so this is what stops a
/// typo in it from becoming a header that promises a function nobody exported.
#[test]
fn every_declared_symbol_is_exported() {
    let want = libgui_c::header::render();
    for (name, _, _) in libgui_c::table::TABLE {
        assert!(name.starts_with("libgui_"), "{name} does not carry the prefix");
        assert!(want.contains(&format!(" {name}(")), "{name} is in the table but not in the header");
    }
}

/// The C side asserts its own `sizeof` against these. If a mirror gains a
/// field on one side only, that assert is what catches it — so the numbers
/// must be the real ones, not a constant someone typed.
#[test]
fn the_reported_sizes_are_the_real_ones() {
    use libgui_c::*;
    assert_eq!(libgui_sizeof_response() as usize, std::mem::size_of::<LibguiResponse>());
    assert_eq!(libgui_sizeof_tree_response() as usize, std::mem::size_of::<LibguiTreeResponse>());
    assert_eq!(libgui_sizeof_text_response() as usize, std::mem::size_of::<LibguiTextResponse>());
    assert_eq!(libgui_sizeof_vec2() as usize, 8);
    assert_eq!(libgui_sizeof_rect() as usize, 16);
    assert_eq!(libgui_sizeof_color() as usize, 16);
    assert_eq!(libgui_sizeof_modifiers() as usize, 4);
}
