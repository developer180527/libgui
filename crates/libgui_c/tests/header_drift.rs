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
    for (name, _, _, _) in libgui_c::table::TABLE {
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

/// Every widget in the table carries its documentation into the header.
///
/// The generated half used to arrive as bare prototypes while the hand-written
/// half was documented, so a C++ caller reading libgui.h met a hundred-odd
/// undeclared intentions. Now the doc comment is captured from the same line
/// that declares the widget — which only helps if there is one, so adding a
/// widget without a doc comment fails here.
#[test]
fn every_generated_declaration_is_documented() {
    let header = libgui_c::header::render();
    let mut undocumented = Vec::new();
    for (name, _, _, docs) in libgui_c::table::TABLE {
        if docs.iter().all(|d| d.trim().is_empty()) {
            undocumented.push(*name);
            continue;
        }
        // And the words actually reach the file, above the declaration.
        let at = header.find(&format!(" {name}(")).unwrap_or_else(|| panic!("{name} is not in the header"));
        let before = &header[..at];
        let comment_end = before.rfind("*/").unwrap_or(0);
        let decl_start = before.rfind('\n').unwrap_or(0);
        assert!(comment_end + 3 >= decl_start, "{name} has no comment above it");
    }
    assert!(
        undocumented.is_empty(),
        "these widgets have no doc comment, so the header would describe them to nobody: {undocumented:?}"
    );
}

/// No block comment in the header may contain another `/*`.
///
/// A C block comment does not nest: an example that writes one inside a
/// comment ends it early, and the rest of the comment is compiled as code.
/// The generated half escapes this when it renders doc text; the hand-written
/// preamble has no such protection, and this caught exactly that mistake.
///
/// The C smoke test would also catch it, but only in the job that has a C
/// compiler, and the error it gives names a line number a hundred lines later.
#[test]
fn no_comment_in_the_header_ends_early() {
    let header = libgui_c::header::render();
    let mut inside = false;
    let bytes = header.as_bytes();
    let mut line = 1;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
        }
        match (&bytes[i..i + 2], inside) {
            (b"/*", false) => {
                inside = true;
                i += 2;
                continue;
            }
            (b"/*", true) => panic!(
                "line {line}: a `/*` inside a block comment ends it early — \
                 write examples with `//` instead"
            ),
            (b"*/", true) => {
                inside = false;
                i += 2;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    assert!(!inside, "the header ends inside a block comment");
}

