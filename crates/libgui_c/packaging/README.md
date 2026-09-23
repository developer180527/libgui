# Packaging

Three ways to get libgui into a C or C++ build, in the order most projects
should try them.

## 1. `add_subdirectory` — what vCAD should use

```cmake
add_subdirectory(third_party/libgui/crates/libgui_c)
target_link_libraries(vcad PRIVATE libgui::libgui)
```

That is the whole integration. The target carries the header directory, the
platform system libraries, and a rule that rebuilds the Rust when any `.rs` or
manifest in the workspace changes.

The only prerequisite is `cargo` on PATH. CMake looks for it at configure time
and says so plainly if it is missing, rather than failing later with a linker
error about a file that was never built.

**Use this while you are co-developing libgui and your app**, which is the case
whenever a change to one is a change to the other. A package manager's copy is
a snapshot; this is the source.

### Configurations

`Debug` builds cargo's `debug` profile; everything else builds `release`. Each
configuration gets its own `IMPORTED_LOCATION_<CONFIG>`, so Visual Studio and
Ninja Multi-Config work as well as a single-config generator. Configurations
that share a profile — `Release` and `RelWithDebInfo` — share one build.

A debug build of libgui is **an order of magnitude slower** than a release one:
the performance tests in this repository only assert their budgets in release
for that reason. If your Debug build feels sluggish in the UI, that is why, and
building libgui as `Release` inside a Debug app is a reasonable thing to do.

## 2. Corrosion

If your project already uses [Corrosion](https://github.com/corrosion-rs/corrosion):

```cmake
set(LIBGUI_USE_CORROSION ON)
add_subdirectory(third_party/libgui/crates/libgui_c)
```

or import the crate yourself and link `libgui_c` directly. The CMakeLists gets
out of the way.

Corrosion is not the default here for one reason: it would make every consumer
vendor Corrosion too. For a single static library from a single crate, calling
cargo is fewer moving parts. Nothing is wrong with Corrosion — it does more
than this needs.

## 3. vcpkg

`packaging/vcpkg/` holds a port, for a project that wants a released libgui
rather than one it co-develops. It needs `cargo` on PATH: vcpkg does not manage
Rust toolchains, and the portfile checks rather than letting the build fail
somewhere less obvious.

The `SHA512` is a placeholder until a release is tagged — a port cannot be
published without one, and inventing a hash would be worse than leaving it
visible.

## What you link against

`libgui::libgui` is an INTERFACE target carrying:

- the static library (`liblibgui_c.a`, or `libgui_c.lib` under MSVC);
- `include/`, which has `libgui.h` and the optional C++ wrapper `libgui.hpp`;
- the system libraries a Rust static library needs — `ws2_32 userenv advapi32
  ntdll bcrypt` on Windows, `m` on macOS, `m pthread dl` elsewhere.

## Fonts

libgui reads no files, so nothing ships a font to a consumer automatically.
`LIBGUI_FONT` points at the bundled Inter for a sample or a test; an
application passes its own bytes to `libgui_ui_new`. See the licence note in
`LICENSING.md` — Inter is under the OFL, which is not the code's licence.
