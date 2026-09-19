//! The architecture boundary, enforced rather than reviewed.
//!
//! `libgui` does UI work and nothing else: no windowing, no GPU, no clock, no
//! filesystem, no threads, no network. That is easy to state in a README and
//! easy to erode one convenience at a time — a `SystemTime::now()` for an
//! animation here, a `std::fs::read` for an icon there — and each one quietly
//! makes the crate harder to port, to embed in someone else's loop, or to run
//! deterministically in a test.
//!
//! So it is a test. It reads the crate's own source and manifest, which means
//! it fails on the commit that breaks the boundary rather than at the port.

use std::path::Path;

/// Paths that pull in the outside world. The host owns every one of these:
/// it supplies `dt`, it reads files, it talks to the window system.
const FORBIDDEN: &[&str] = &[
    "std::fs",
    "std::time",
    "std::thread",
    "std::net",
    "std::process",
    "std::env",
    "std::io",
    "SystemTime",
    "Instant::now",
];

/// Process-global state. An app embedding a UI owns its allocator, its
/// threading and its lifetimes; a library that keeps a hidden global takes one
/// of those away, and takes it away from every other user of the process too.
/// Without any, `#[global_allocator]` is simply the binary's choice and libgui
/// inherits it — which is what makes `perf_alloc.rs` able to count it at all.
const NO_GLOBALS: &[&str] = &["static mut", "thread_local!", "OnceLock", "OnceCell", "lazy_static", "AtomicUsize", "AtomicU64"];

/// `theme_watch` is the documented exception: an opt-in, off-by-default feature
/// whose whole job is to poll a file, kept in one module so it stays visible.
const EXEMPT_FILES: &[&str] = &[
    // Opt-in, off by default, and its whole job is to poll a file.
    "theme_watch.rs",
    // Opt-in, off by default, and its whole job is to read the clock. The
    // timers compile to nothing without the `profile` feature.
    "profile.rs",
];

/// Every direct dependency, and why it is allowed to be one.
const ALLOWED_DEPS: &[(&str, &str)] = &[
    ("bytemuck", "casting the instance buffer to bytes; no_std, no I/O"),
    ("fontdue", "optional, default: the built-in glyph rasteriser"),
    ("serde", "optional: theme (de)serialisation, pure data"),
    ("toml", "optional: theme file format, pure data"),
];

fn src_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Source with `#[cfg(test)]` modules removed: tests may use the clock and the
/// filesystem, the library may not. Modules are found by the attribute and cut
/// at the matching brace.
fn without_test_modules(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let Some(open) = after.find('{') else { break };
        let mut depth = 0usize;
        let mut end = None;
        for (i, c) in after[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(e) => rest = &after[e..],
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Every library file, test modules already cut out.
fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(src_dir()).expect("src/") {
        let path = entry.expect("entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if path.extension().is_none_or(|e| e != "rs") || EXEMPT_FILES.contains(&name.as_str()) {
            continue;
        }
        let src = without_test_modules(&std::fs::read_to_string(&path).expect("read"));
        out.push((name, src));
    }
    out
}

#[test]
fn the_core_reaches_for_nothing_outside_itself() {
    let mut found = Vec::new();
    for (name, src) in sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for bad in FORBIDDEN {
                if code.contains(bad) {
                    found.push(format!("{name}:{}: {bad} — {}", n + 1, code.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "libgui reached outside itself. The host owns these; pass the result in through \
         `FrameInfo` or an `InputEvent` instead:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn the_core_keeps_no_process_global_state() {
    let mut found = Vec::new();
    for (name, src) in sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for bad in NO_GLOBALS {
                if code.contains(bad) {
                    found.push(format!("{name}:{}: {bad} — {}", n + 1, code.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "libgui kept process-global state. Every `Ui` must be independent, so an app owns its \
         allocator, its threads and its lifetimes:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn the_dependency_list_is_the_one_we_agreed_on() {
    let manifest = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).expect("Cargo.toml");
    let deps = manifest.split("[dependencies]").nth(1).expect("[dependencies]");
    let found: Vec<&str> = deps
        .lines()
        .take_while(|l| !l.trim_start().starts_with('['))
        .filter_map(|l| l.split(['=', '.']).next().map(str::trim))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    for dep in &found {
        assert!(
            ALLOWED_DEPS.iter().any(|(name, _)| name == dep),
            "new direct dependency `{dep}`. A UI core earns its portability by having almost none: \
             add it to ALLOWED_DEPS with the reason, or put it in the host crate."
        );
    }
    for (name, _) in ALLOWED_DEPS {
        if !found.contains(name) {
            panic!("`{name}` is allow-listed but no longer a dependency: drop it from ALLOWED_DEPS.");
        }
    }
}

#[test]
fn test_modules_are_cut_before_scanning() {
    let src = "use std::fmt;\n#[cfg(test)]\nmod t {\n    use std::time::Instant;\n    fn f() { { } }\n}\nuse std::hash::Hash;\n";
    let out = without_test_modules(src);
    assert!(!out.contains("std::time"), "a #[cfg(test)] module was not cut: {out}");
    assert!(out.contains("std::fmt") && out.contains("std::hash"), "cut too much: {out}");
}
