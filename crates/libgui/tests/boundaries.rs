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

/// Deciding anything from the platform it was built for. Which controls the
/// keyboard visits, what Cmd means, how a shortcut is spelled: all convention,
/// all different per platform, and none of it libgui's to assume. A core that
/// compiles differently on macOS is a core you cannot test on Linux, cannot
/// embed in a host with its own conventions, and cannot ask to behave like
/// another platform on request. The knowledge lives in `libgui_keymap`, which
/// is opt-in and takes the platform as an argument.
const NO_PLATFORM: &[&str] = &["target_os", "target_vendor", "target_family", "cfg!(unix", "cfg!(windows"];

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
    ("rustybuzz", "optional, off: real shaping (feature `shape`); pure Rust, no_std, no I/O"),
    ("self_cell", "optional, off: holds font bytes beside the rustybuzz face that borrows them"),
    ("serde", "optional: theme (de)serialisation, pure data"),
    ("toml", "optional: theme file format, pure data"),
];

fn src_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The source with every comment and the inside of every string and char
/// literal replaced by spaces, newlines kept so line numbers still point at
/// the right place. What is left is code, and only code is checked: a doc
/// comment may *mention* `std::fs`, and a `"http://…"` literal must not hide
/// the rest of its line the way splitting on `//` did.
///
/// A lexer, not a parser: comments (nested block comments too), strings,
/// byte and C strings, raw strings with any number of `#`, char literals —
/// and lifetimes, which begin with the same quote and must be left alone.
fn code_only(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut i = 0;
    while i < c.len() {
        let at = |k: usize| c.get(k).copied();
        // Line comment, including `///` and `//!`.
        if c[i] == '/' && at(i + 1) == Some('/') {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
            continue;
        }
        // Block comment; they nest.
        if c[i] == '/' && at(i + 1) == Some('*') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '/' && at(i + 1) == Some('*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == Some('/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
            continue;
        }
        // Raw string: `r"`, `r#"`, `br##"` … — only where `r` starts a token.
        let prev_ident = i > 0 && ident(c[i - 1]);
        let r_at = if !prev_ident && c[i] == 'r' {
            Some(i)
        } else if !prev_ident && (c[i] == 'b' || c[i] == 'c') && at(i + 1) == Some('r') {
            Some(i + 1)
        } else {
            None
        };
        if let Some(r) = r_at {
            let mut k = r + 1;
            while at(k) == Some('#') {
                k += 1;
            }
            if at(k) == Some('"') {
                let hashes = k - r - 1;
                for &ch in &c[i..=k] {
                    out.push(ch);
                }
                i = k + 1;
                while i < c.len() {
                    if c[i] == '"' && (0..hashes).all(|h| at(i + 1 + h) == Some('#')) {
                        for &ch in &c[i..=i + hashes] {
                            out.push(ch);
                        }
                        i += 1 + hashes;
                        break;
                    }
                    blank(&mut out, c[i]);
                    i += 1;
                }
                continue;
            }
        }
        // Ordinary string (a `b` or `c` prefix is just code before it).
        if c[i] == '"' {
            out.push('"');
            i += 1;
            while i < c.len() {
                if c[i] == '\\' {
                    blank(&mut out, c[i]);
                    if i + 1 < c.len() {
                        blank(&mut out, c[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                if c[i] == '"' {
                    out.push('"');
                    i += 1;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
            continue;
        }
        // Char literal — `'x'` or `'\n'` / `'\u{..}'` — and not a lifetime.
        if c[i] == '\'' {
            let close = if at(i + 1) == Some('\\') {
                (i + 2..c.len()).find(|&k| c[k] == '\'')
            } else if at(i + 2) == Some('\'') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(end) = close {
                out.push('\'');
                for &ch in &c[i + 1..end] {
                    blank(&mut out, ch);
                }
                out.push('\'');
                i = end + 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Code with `#[cfg(test)]` items removed: tests may use the clock and the
/// filesystem, the library may not. Runs on `code_only` output, so a brace in
/// a string or a comment cannot end a module early. An item ends at its
/// matching brace, or at a `;` if it has none (`#[cfg(test)] use …;`).
fn without_test_modules(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let semi = after.find(';');
        let open = after.find('{');
        let end = match (open, semi) {
            (Some(o), Some(s)) if s < o => Some(s + 1),
            (Some(o), _) => {
                let mut depth = 0usize;
                let mut end = None;
                for (i, c) in after[o..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(o + i + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end
            }
            (None, Some(s)) => Some(s + 1),
            (None, None) => None,
        };
        match end {
            Some(e) => {
                // Keep the line count: a cut item still leaves its newlines.
                out.extend(after[..e].chars().filter(|&c| c == '\n'));
                rest = &after[e..];
            }
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Every library file as code only: comments and literal contents blanked,
/// test modules cut out.
fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(src_dir()).expect("src/") {
        let path = entry.expect("entry").path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if path.extension().is_none_or(|e| e != "rs") || EXEMPT_FILES.contains(&name.as_str()) {
            continue;
        }
        let src = without_test_modules(&code_only(&std::fs::read_to_string(&path).expect("read")));
        out.push((name, src));
    }
    out
}

#[test]
fn the_core_reaches_for_nothing_outside_itself() {
    let mut found = Vec::new();
    for (name, src) in sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line;
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
fn the_core_decides_nothing_from_the_platform_it_was_built_for() {
    let mut found = Vec::new();
    for (name, src) in sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line;
            for bad in NO_PLATFORM {
                if code.contains(bad) {
                    found.push(format!("{name}:{}: {bad} — {}", n + 1, code.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "libgui decided something from the target platform. Keyboard focus, key bindings and \
         shortcut spelling are convention, not fact: put the knowledge in `libgui_keymap`, which \
         takes the platform as an argument, and leave the core able to behave like any of \
         them:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn the_core_keeps_no_process_global_state() {
    let mut found = Vec::new();
    for (name, src) in sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line;
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
    let out = without_test_modules(&code_only(src));
    assert!(!out.contains("std::time"), "a #[cfg(test)] module was not cut: {out}");
    assert!(out.contains("std::fmt") && out.contains("std::hash"), "cut too much: {out}");
}

/// The scanner's own edge cases. Each of these was a way for a violation to
/// hide, or for innocent text to be flagged, when the scan was a split on `//`
/// and a brace count over raw source.
#[test]
fn the_scanner_sees_code_and_only_code() {
    let code = |src: &str| code_only(src);

    // A `//` inside a string no longer hides the rest of the line.
    assert!(code(r#"let u = "http://x"; std::fs::read(p);"#).contains("std::fs"));
    // Comments and literals are not code.
    assert!(!code("// std::fs::read\n/* std::net */ let x = 1;").contains("std::"));
    assert!(!code(r#"let s = "std::fs"; let b = b"static mut";"#).contains("std::fs"));
    assert!(!code(r#"let b = b"static mut";"#).contains("static mut"));
    // Block comments nest.
    assert!(code("/* a /* b */ std::fs */ let x = 1;").trim_start().starts_with("let x"));
    // Raw strings end at the right number of hashes, and code after them counts.
    let raw = code(r####"let s = r##"has "# inside, std::fs"##; std::time::Instant::now();"####);
    assert!(!raw.contains("std::fs") && raw.contains("std::time"), "{raw}");
    // A lifetime is not a char literal: treating it as one would swallow code.
    let life = code("fn f<'a>(x: &'a str) { std::env::var(x); }");
    assert!(life.contains("std::env"), "{life}");
    // Char literals, escaped ones included, are blanked without eating code.
    let chars = code(r"let q = '\''; let b = '{'; let n = '\n'; std::process::exit(0);");
    assert!(chars.contains("std::process") && !chars.contains("'{'"), "{chars}");
    // Line numbers survive, so a report points at the right line.
    assert_eq!(code("a /* x\ny */ b\n\"p\nq\"").lines().count(), 4);
}

#[test]
fn a_brace_in_a_string_cannot_end_a_test_module_early() {
    let src = "#[cfg(test)]\nmod t {\n    const S: &str = \"}\";\n    use std::time::Instant;\n}\nlet x = 1;\n";
    let out = without_test_modules(&code_only(src));
    assert!(!out.contains("std::time"), "the module ended at the brace in the string: {out}");
    assert!(out.contains("let x"), "cut too much: {out}");
    assert_eq!(out.lines().count(), src.lines().count(), "line numbers moved");
}

#[test]
fn a_cfg_test_item_without_a_body_ends_at_its_semicolon() {
    let src = "#[cfg(test)]\nuse std::time::Instant;\nfn keep() { std::fs::read(p); }\n";
    let out = without_test_modules(&code_only(src));
    assert!(!out.contains("std::time"), "{out}");
    assert!(out.contains("std::fs"), "the cut ran past the `use` into real code: {out}");
}
