# libgui

A UI library for professional tools (CAD, editors, node graphs, timelines,
engines) that you own end to end. It is written in Rust and has a C ABI and a
C++ header.

libgui is a **UI core and nothing else**. You give it input, a font and a
renderer. It gives back laid-out, drawable UI, plus requests for the cursor,
the clipboard and the IME. It owns no window, binds no keys, and never touches
the filesystem, the network or threads.

```rust
use libgui::*;

let mut ui = Ui::new(Theme::dark(), font_bytes)?;
libgui_keymap::Keymap::<MyAction>::for_current_platform().install(&mut ui);

// every frame
ui.begin_frame(FrameInfo { screen_size, scale, dt });
ui.heading("Model");
ui.checkbox("Visible", &mut visible);
if ui.button("Export").clicked { export(); }
let out = ui.end_frame();          // instances to draw + platform requests
```

```
cargo run --release -p libgui_demo     # editor with a wgpu 3-D viewport
cargo test -p libgui                    # the core's tests
```

C and C++ hosts: `add_subdirectory(crates/libgui_c)`, then link
`libgui::libgui`.

## Documentation

| Read | For |
|---|---|
| [`MANUAL.md`](MANUAL.md) | **Start here.** How libgui works, how to use it, and the API by area, with Rust and C names side by side |
| [`DESIGN.md`](DESIGN.md) | Why it is shaped this way, decision by decision |
| [`WIDGETS.md`](WIDGETS.md) | Fourteen applications surveyed against the widget set: what is here, what is missing, in what order |
| [`IN_DEPTH.md`](IN_DEPTH.md) | Longer notes: text undo, wheel smoothing, performance guards, arenas, damage tracking, caching, golden images |
| `cargo doc -p libgui --open` | Every signature |

## What it has

- **Layout and widgets.**
  - Rows, columns, scroll areas.
  - Buttons, checkboxes, toggles, radios, sliders, drag values, combo boxes,
    segmented controls.
  - Text fields, a multi-line editor with undo, validated fields, a colour
    picker, notifications.
  - Menus, popups, tooltips, splitters, progress bars, plots.
- **Large data.** Virtual lists, a virtual tree for 100,000-node assemblies,
  and tables with frozen and resizable columns.
- **Docking.** Unity-style docking with tabs that tear off into OS windows,
  and layouts saved as TOML.
- **Drawing.**
  - Shapes, gradients, lines and curves (solid or dashed), vector icons,
    images, and images and text turned to any angle.
  - Pan/zoom canvases whose widgets work in model coordinates.
  - Edges kept on the physical pixel grid.
- **Input.**
  - Every platform's keyboard conventions, via `libgui_keymap`.
  - Focus and Tab order, arrow keys in lists, trees and menus, type-ahead,
    multi-select.
  - IME composition, touch.
- **Motion and idling.** Easing and springs; the window sleeps when nothing
  moves.
- **Text.** Font fallback chains, and real shaping with rustybuzz.
- **Rendering.** One shader in five languages and a wgpu backend. A mesh form
  serves renderers that cannot instance (bgfx, GLES2), and a CPU reference
  renderer gives pixel-exact tests.

**Not yet:**
- accessibility (screen readers);
- bidirectional text;
- word wrap inside the text editor.

The full list is in [MANUAL §17](MANUAL.md#17-limitations).

## Status

| | |
|---|---|
| CI | build, test, clippy and docs on Linux (x86_64 + ARM), Windows, macOS (ARM) |
| Also builds for | wasm32, Android (aarch64), iOS + simulator, 32-bit x86 |
| Not tested | Android and iOS on device, physical iPad, mixed-DPI multi-monitor docking |
| Rust | **1.90** for the GPU crates, **1.87** for `libgui`, `libgui_nodes`, `libgui_soft`, `libgui_keymap`, `libgui_units` |
| Version | 0.1.0, pre-1.0: the API changes between releases |

## Crates

| Crate | Role |
|---|---|
| `libgui` | The core: IDs, retained state, layout, input, theme, text, draw list, widgets, docking. No GPU code |
| `libgui_keymap` | Each platform's key bindings and focus rules, app actions, menu shortcut spelling |
| `libgui_shaders` | The one UI shader, written in WGSL and cross-compiled to HLSL, MSL, GLSL and SPIR-V |
| `libgui_wgpu` | A wgpu `Backend`, and the model for writing your own |
| `libgui_winit` | winit event translation; a template for other hosts |
| `libgui_soft` | CPU reference renderer, for golden-image tests |
| `libgui_units` | Expression evaluator with units, plus a `number_input` |
| `libgui_nodes` | Node-graph editing, built on libgui |
| `libgui_c` | C ABI (`libgui.h`) and C++ header (`libgui.hpp`), with CMake and vcpkg packaging |

**Demos** (`crates/demo_gui/`):

| Demo | What it shows |
|---|---|
| `libgui_demo` | An editor with a 3-D viewport |
| `libgui_solaris` | A dense, Houdini-shaped editor |
| `libgui_cut` | A video editor timeline |
| `libgui_pad` | A text editor |
| `libgui_springs` | Spring animation |
| `libgui_colors` | Colour pickers and themes |

Run any of them with `cargo run --release -p <name>`.

## Roadmap

1. **Accessibility** via AccessKit. This blocks shipping to consumers.
2. **Text:**
   - bidirectional text;
   - word wrap in `text_area`;
   - double-click to select a word;
   - a multi-page glyph atlas.
3. **Drawing:**
   - a triangle primitive for shapes that change every frame;
   - a real line/area plot.
4. **Tables:** a 2-D cell cursor, cell editing, reordering columns by drag.
5. **Compositions:**
   - a searchable dropdown;
   - a command palette;
   - a split button;
   - a date picker.

[`WIDGETS.md`](WIDGETS.md) orders the widget gaps.

## Licence

Either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
Unless you state otherwise, any contribution you submit is dual-licensed the
same way.

`assets/Inter.ttf` is Inter, under the **SIL Open Font License 1.1** (see
`assets/Inter-OFL.txt`), not under either licence above. It ships for the tests
and demos. libgui takes any font you give it.
