# libgui: hybrid immediate/retained UI

A starting point for an engine/CAD/tools UI library that you own end to end.

```
cargo run --release -p libgui_demo          # editor demo with a wgpu 3D viewport
cargo test -p libgui                         # layout + backend-contract tests
cargo run -p libgui_shaders -- shaders_out   # export HLSL/MSL/GLSL/SPIR-V/WGSL
```

## Crates

| crate | role |
|---|---|
| `crates/libgui` | Core. **No GPU code.** IDs, retained state, layout, input, theme, text, draw list, widgets, and the `Backend` trait. |
| `crates/libgui_shaders` | The one UI shader. Authored in WGSL, validated + cross-compiled by naga at build time to HLSL (SM 5.1), MSL 2.0, GLSL 4.50, SPIR-V. |
| `crates/libgui_wgpu` | `Backend` implementation for wgpu. Also the reference for writing your own. |
| `crates/libgui_demo` | winit host + editor layout + an "engine" scene rendered offscreen and shown via `ui.viewport`. |

## Frame lifecycle

```
ui.begin_frame(input)      hit-test mouse against LAST frame's rects → hovered id
  build_ui(&mut ui)        immediate calls: if ui.button("Save").clicked { … }
                           each call: make_id → interact → animate → add_leaf(layout, paint closure)
ui.end_frame()             layout::solve (measure bottom-up, place top-down)
                           run paint closures in tree order → DrawList + hit list
backend.prepare(&out)      upload globals, instances, atlas (if changed)
backend.render(pass, &out) one instanced draw per batch (texture switch)
```

Key ideas (see source comments):

- **Stable `Id`s** = parent id + label/key (`ui.rs::make_id`). This is what lets
  an immediate API own retained state: animations (`ui.animate`), drag, focus.
- **Layout after build** (`layout.rs`): `Fixed | Fit | Grow(weight)` per axis,
  padding, gap, alignment. Lets "fit content, then fill the rest" work in one frame.
  Space is distributed in a single pass: a `Grow` child never shrinks below its
  content, so if one clamps up to its minimum the row **overflows** its parent
  rather than re-dividing the remainder among its siblings (unlike flexbox).
- **One-frame-late input**: `Response.rect` is last frame's rect. Invisible in practice,
  and it keeps the model simple.
- **Paint closures** run after layout with final rects. Custom widgets and
  viewport overlays use the same `Painter`, which is the immediate layer.
- **SDF quads** (`libgui_wgpu/src/ui.wgsl`): rounded rects, borders, soft shadows,
  glyphs, rounded images, all in one shader, premultiplied alpha, DPI-correct AA.
- **Themes** (`theme.rs`, `theme_file.rs`): palette → metrics (density) → per-widget styles, loadable and hot-reloadable from TOML.

## Writing a widget

```rust
pub fn my_knob(ui: &mut Ui, label: &str, v: &mut f32) -> Response {
    let id = ui.make_id(("knob", label));
    let resp = ui.interact(id);
    if resp.active { *v = (*v - resp.drag_delta.y * 0.01).clamp(0.0, 1.0); }
    let hover = ui.animate_bool(id, 0, resp.hovered);
    let value = *v;
    ui.add_leaf(id, Layout::leaf(Size::Fixed(48.0), Size::Fixed(48.0)), Vec2::ZERO, true, move |p, r| {
        p.rect(r, p.theme.surface.lerp(p.theme.surface_hover, hover), 24.0);
        // … draw the arc for `value`
    });
    resp
}
```

## Custom RHI backends

`libgui::Backend` is the whole contract (see `crates/libgui/src/backend.rs`):

```rust
impl libgui::Backend for MyRhiUi {
    type Pass<'p> = MyCommandList<'p>;
    fn prepare(&mut self, frame: &FrameOutput) { /* globals, instances, atlas if version changed */ }
    fn begin(&mut self, pass: &mut Self::Pass<'_>) { /* bind pipeline, globals, instance buffer */ }
    fn draw(&mut self, pass: &mut Self::Pass<'_>, tex: TextureId, instances: Range<u32>) {
        /* bind atlas or your texture for tex, draw 6 verts x instances */
    }
}
```

Pipeline, same for every API (details in `libgui_shaders` docs):

- Instance buffer, 96-byte stride, six `float4` attributes at locations 0 to 5; 6 vertices per instance.
- Group 0 / binding 0: `Globals` uniform (16 B). Group 1 / binding 0: one texture. **No samplers**
  (texel loads + in-shader bilinear), so HLSL is just `b0` + `t0, space1`.
- Premultiplied alpha blend, no depth, no culling, UNORM (non-sRGB) target.
- `TextureId::User(n)` is opaque; map `n` to any texture/SRV handle in your RHI, which is how
  your renderer's viewport targets appear in the UI. The image path ignores the texture's own
  alpha and composites it as opaque (rounded-corner mask and tint still apply), which suits
  viewport targets; RGBA icons need a shader change.

Pick the shader flavour your RHI consumes from `libgui_shaders::{HLSL, MSL, SPIRV, GLSL_VERTEX, GLSL_FRAGMENT, WGSL}`,
or export them to files for a C++ shader pipeline. Edit only `shaders/ui.wgsl`; a shader error fails the build.

## Text input, focus, clipboard

```rust
let r = ui.text_input("search", &mut filter, "Search objects…");
if r.changed { /* refilter */ }
if r.submitted { /* Enter pressed */ }
```

- Caret + selection (mouse drag, Shift+arrows), word jumps/deletes (`Modifiers::word`), line start/end and
  select-all (`Modifiers::command`), horizontal auto-scroll, placeholder, focus ring, blinking caret.
- Keyboard focus: click to focus, click elsewhere / Esc / Enter to release, **Tab / Shift+Tab** cycles fields.
  `ui.wants_keyboard()` tells the host not to route keys to the game.
- The host translates OS input into `Input::events` (`Event::Key`, `Text`, `Paste`, `Copy`, `Cut`) and writes
  `ui.take_copied()` to the clipboard, so the core has no platform or clipboard dependency.
  See `key_event` in `libgui_demo/src/main.rs` for a winit + arboard reference mapping.
- `ui.ime_rect()` gives the caret rect for positioning an IME candidate window.

## Widget identity

Widget ids come from the label, and duplicates in one container are separated by *build order*.
That is fine for static UI, but hiding the first of two same-labelled widgets hands its id — and
with it the animation, focus and drag state — to the second. Anywhere labels repeat or widgets are
conditional, give them a key that does not move:

```rust
ui.button_keyed("delete-selected", "Delete");     // one widget
ui.with_key(obj.id, |ui| {                        // a whole group, custom widgets included
    ui.selectable(&obj.name, obj.id == selected); // two objects named "Mesh" stay distinct
    if obj.removable { ui.button("Delete"); }
});
```

`with_key` salts every id built inside it (including those from your own `ui.make_id`), and nested
scopes combine. `button_keyed`, `button_styled_keyed`, `toggle_keyed`, `slider_keyed` and
`selectable_keyed` take a key for a single widget; `segmented`, `text_input`, `scroll_area` and
`viewport` already take one.

## Scroll areas

```rust
ui.scroll_area("inspector", |ui| { /* fills remaining height */ });
let opts = ScrollOptions { stick_to_end: true, ..ScrollOptions::new(Size::Grow(1.0)) };
ui.scroll_area_with("console", opts, |ui| { /* log lines */ });
```

- Wheel/trackpad goes to the innermost scroll area under the mouse (nesting works), with smoothing.
- Overlay scrollbar fades in on hover, widens under the pointer; drag the thumb or click the track to jump.
- Clipped hit-testing: widgets scrolled out of view can't be hovered or clicked.
- `stick_to_end` keeps logs/consoles pinned to the newest line while at the bottom.

## Docking (Unity-style, multi-window)

```rust
let mut dock = DockState::<MyTab>::new();
let left = dock.leaf(vec![MyTab::Outliner]);
let right = dock.leaf(vec![MyTab::Scene, MyTab::Game]);
let root = dock.split(Axis::X, 0.2, left, right);
dock.set_root(SurfaceId::MAIN, root);

// every frame, per window:
dock.show(&mut ui, window_surface_id, &mut my_tab_viewer);
```

- **Tabs:** click to activate, drag sideways to reorder live (neighbours slide out of the way).
- **Tear-off:** drag a tab out of its bar and it *instantly* becomes a real OS window under the pointer,
  following it until release. Drag the only tab of a floating window to move that window.
- **Dock back:** hover any pane: centre or tab bar = add as tab; near an edge = split that pane;
  near a window edge = dock along the whole side. An animated preview shows the result; the dragged
  window hides while over a target (configurable). Release to dock; the floating window closes.
  Esc cancels a drag. Closing a floating window returns its tabs to the main window.
- **Splitters:** drag the gaps; thin visual gap with a wider invisible grab area that wins over content.

**Tuning the feel:** every parameter is in `DockConfig` (thresholds, drop-zone sizes, animation
speeds, tab metrics, splitter size, hide-over-target). The demo's *Dock Tuning* panel edits them live;
copy the values you like into `DockConfig::default()`.

**Host contract:** libgui never creates windows. Per loop iteration the host forwards the global
pointer (`set_pointer`, physical screen px), reports window placement (`set_surface_frame`), calls
`update()`, then makes OS windows match `dock.surfaces()` (create/destroy, apply `window_pos` and
`visible`) and renders each window with `dock.show`. `libgui_demo/src/main.rs` is a complete
winit reference (~450 lines) with one shared wgpu device and a renderer + `Ui` per window.

## Theming: palette, density, per-widget styles, hot reload

A `Theme` is three layers: a **palette** (named colours), **metrics** (sizes, fonts; generated by a
`Density`: compact / regular / touch), and **per-widget styles** (`button`, `button_primary`, `toggle`,
`slider`, `selectable`, `text_input`, `segmented`, `scrollbar`, `tab`, `splitter`, `panel`, `plot`,
`viewport`, `drop_preview`) derived from the first two. Change `palette.accent` and everything using the
accent follows; change `button.radius` and only buttons change.

**TOML themes** extend a preset and override only what they mention:

```toml
name = "Unity-ish"
extends = "dark"            # dark | midnight | light
density = "compact"         # compact | regular | touch
[palette]
accent = "#3a79d8"
[tab]
radius = 2
bar_fill = "#282828"
fill_hover = "mix(surface, accent, 0.15)@0.6"   # hex, palette names, name@alpha, mix(a, b, t)
```

Unknown keys, bad colours and type mismatches are reported with their path (`unknown key button.radus`).
`ThemeWatcher` polls a file (no platform deps) and hands you the new theme or the error; keep the last
good theme on error. `theme.to_toml()` exports every resolved value as a reference.
See `themes/unity.toml`, `themes/blender.toml`, `themes/custom.toml`.

**In code:**

```rust
ui.theme = Theme::from_toml(&src)?;                  // or Theme::dark() / midnight() / light()
ui.theme.set_density(Density::Touch);                // regenerate metrics + styles
ui.button_styled("Delete", &danger_style);           // explicit style for one widget
ui.with_style(|t| t.button.radius = 0.0, |ui| {      // scoped override for a subtree
    ui.button("Square");
});
```

Widgets copy their style when built, so `with_style` scopes are exact. Behaviour ("feel") stays in
`DockConfig`; the look of tabs, splitters and the drop preview is in the theme.

Demo: the *Appearance* panel switches themes/density and shows reload status; or launch with
`LIBGUI_THEME=unity LIBGUI_DENSITY=touch cargo run --release -p libgui_demo`, then edit
`themes/custom.toml` (select *Custom*) and save to see it update live.

## iPad / touch

libgui runs on iPadOS with the same code: winit + wgpu (Metal) + the core.

```bash
./scripts/ios-sim.sh                                   # build target/ios-sim/libgui.app
xcrun simctl install booted target/ios-sim/libgui.app
xcrun simctl launch booted com.libgui.demo
```

- **Touch input:** set `Input::pointer_kind = Touch` and fill `Input::touches`. libgui derives the primary
  pointer, has no hover on touch, and tells taps from scrolls: moving past `ui.touch_slop` (8 px) on
  anything that isn't a drag widget scrolls the innermost scroll area instead, cancelling the tap.
  Flings coast with `ui.scroll_friction`. Drag widgets (sliders, splitters, viewport, tabs, text
  selection) use `interact_drag` and keep the finger.
- **Gestures:** two fingers give `ui.gesture()` (pan + zoom); widgets under them get
  `Response::pinch` / `pan2`. A second finger cancels a pending tap.
- **Docking on tablets:** `DockConfig::floating_mode = FloatingMode::InApp` turns torn-off tabs into
  floating panels inside the app (grip to move, corner to resize, x to close). Tab bars and edges
  dock; a pane's middle leaves the panel floating. Try it on desktop with `LIBGUI_INAPP=1`.
- **Host notes (see the demo):** size the swapchain from `outer_size` on iOS (`inner_size` is the safe
  area), pad the UI by the safe-area insets, don't request a window size on iOS, keep taps that start and
  end between two frames down for one frame, and call `set_ime_allowed(ui.wants_keyboard())` to show
  the on-screen keyboard (winit maps its Return to a newline insert: treat it as Enter).

## Architecture boundaries

- **`libgui` does UI work only:** layout, widgets, input timing, text layout/rasterisation into a CPU
  atlas, theming, docking. No windowing, GPU, clipboard, threads or clocks; the host supplies `dt`.
  With default features it does no I/O at all.
- **Renderer contract (`libgui::render_contract`):** primitive kinds, instance layout, bindings, entry
  points, blending, texel formats and colour space, with a `CONTRACT_VERSION`. `libgui_shaders`
  generates the shader's constants from it and checks the shader's inputs and bindings against it at
  build time (naga), so a mismatch fails the build rather than rendering wrongly. Hand-written
  backends (e.g. a C++ RHI with its own HLSL) should use the same values and check the version.
- **Stable ids:** widget ids use libgui's own SipHash-1-3 (`StableHasher`), pinned by tests, so they are
  identical across Rust releases, 32/64-bit and endianness: safe to persist and to pass over FFI.
- **Features:** `theme-toml` (default) parses/exports themes, pure data; `theme-watch` (opt-in) adds
  `ThemeWatcher`, the only filesystem access in the crate.

## Hosts and input providers

libgui talks to a host through two small types, so any windowing layer or input device can drive it:

```rust
ui.push(InputEvent::PointerMoved { pos });                      // as events arrive, any source
ui.push(InputEvent::Key { key: Key::from_hid_usage(u).unwrap(), pressed, repeat: false });
ui.begin_frame(FrameInfo { screen_size, scale, dt });
/* build UI */
let out = ui.end_frame();          // draw data for your Backend + out.platform:
// cursor, copied_text, paste_requested, text_input (show keyboard/IME at caret),
// wants_pointer / wants_keyboard, pointer_lock, repaint_after (None = sleep until input)
```

- **Events:** pointer position and raw `PointerDelta` (unaccelerated), five buttons, wheel (pixel/line/page),
  touch, physical `Key`s (US-layout names, like HID usages) plus separate `Text`, modifiers, clipboard, focus loss.
- **Timing lives in the core:** a press and release inside one frame still clicks, fingers that tap between
  frames still tap, modifiers are derived from modifier keys when a host only sends keys (raw HID),
  Cmd/Ctrl+C/X/V become copy/cut/paste requests, and chords are never typed as text.
- **Raw HID / relative input:** `Key::from_hid_usage` maps USB HID keyboard usages; `VirtualCursor` turns raw
  deltas into a cursor with your own sensitivity; a widget calling `ui.request_pointer_lock()` (the demo's
  viewport while orbiting) makes drags use raw deltas and asks the host to hide and lock the cursor, so
  an orbit never stops at the screen edge. Keep text input on the OS: HID gives keys, not characters.
- **`libgui_winit`:** the winit adapter (`push_window_event`, `push_device_event`, `PlatformState::apply`)
  is ~250 lines and the template for other hosts (SDL, a C++ engine loop).

## Integrating with your engine

- Render your scene to a texture, `renderer.register_texture(&view)` (wgpu backend), then show it with
  `ui.viewport(...)`. Use the returned `Response` for camera/gizmo input.
- Use `ui.wants_mouse()` to decide whether input goes to the game or the UI.
- Call `backend.render(&mut pass, &out)` inside any pass you already own (e.g. after
  your post-processing), so the UI composites over the frame.
- Run it alongside Dear ImGui during migration: both just record into the same pass.

## Performance guards

The UI is meant to be a rounding error next to a pro app's real work, so that is
enforced by tests rather than left to a benchmark nobody runs
(`crates/libgui/tests/perf.rs`, `perf_alloc.rs`):

| guard | today |
|---|---|
| A visible 211-widget inspector, as a share of a 60 fps frame | **0.26%** (44 µs) |
| Allocations per widget per frame | **2.00** (170 bytes) |
| Allocations in an empty frame | **0** |
| Draw instances for offscreen widgets | **0** (15x the rows, same instance count) |
| Glyph rasterisation in a steady frame | **none** (atlas version unchanged) |
| An idle UI | `repaint_after: None` — the host sleeps |

They assert properties that hold on any machine: deterministic counts, and
*ratios* for complexity (4x the widgets must not cost more than 7x the time,
where linear is 4x) rather than wall-clock times that flake on a loaded CI box.
The one absolute budget is asserted in release only, since a debug build is an
order of magnitude slower. `crates/libgui_bench` is the measuring tool;
these are the regression guards.

## Roadmap (roughly in order)

1. ~~Text input~~ ✅ single-line; next: multi-line editor, IME preedit, double-click word select, undo.
2. ~~Scroll areas~~ ✅; next: virtualised lists/trees (only build visible rows), horizontal scroll, keyboard PageUp/Down.
3. **Keyboard/shortcut routing**, menus, popups/context menus, tooltips (needs a layer/z-order stack).
4. ~~Docking + tabs + splitters~~ ✅ Unity-style with OS-window tear-off; next: layout save/load, tab close/context menu, maximize pane.
5. **Real text shaping**: replace `text.rs` internals with `cosmic-text`/`swash` or HarfBuzz
   (ligatures, bidi, font fallback, CJK), multi-page atlas with LRU eviction.
6. ~~Theme hot-reload~~ ✅ TOML themes, per-widget styles, density presets; next: multiple fonts (UI/mono/icons) in the theme, per-widget disabled states.
7. **Accessibility** via AccessKit: emit a node per interactive widget from the same tree.
8. **Perf**: skip layout/paint for unchanged subtrees, persistent GPU buffers,
   and a "sleep when idle" mode for tools (CAD) instead of redrawing continuously.

## Known scaffold limitations

- Text fields are single-line; no wrapping, IME preedit, or undo yet. No complex shaping.
- Glyph atlas resets when full (possible one-frame flicker).
- No z-layers yet: overlays are drawn inside their node's paint closure.
- Container ids are positional; give containers explicit keys once you add conditional UI.

Font: Inter (SIL Open Font License, see `assets/Inter-OFL.txt`).
