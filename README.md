# libgui: hybrid immediate/retained UI

A UI library for professional tools — node graphs, editors, timelines, CAD, engines — that you own
end to end.

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

## Keyboard shortcuts

libgui supplies the **mechanism** — matching, platform-correct modifiers, routing by focus, and
consumption so one press cannot drive two commands. It supplies **no bindings**: what `Cmd+S` means
is your app's keymap, and your users will want to rebind it.

```rust
if ui.consume_shortcut(Shortcut::command(Key::S)) { save(); }             // Cmd+S / Ctrl+S
if ui.consume_shortcut(Shortcut::command(Key::Z).shift()) { redo(); }
ui.shortcut_label(Shortcut::command(Key::S));                             // "⌘S" or "Ctrl+S"
```

- **`command` is Cmd on Apple platforms and Ctrl elsewhere**, so one declaration is right on both,
  and the wrong one is rejected: `Ctrl+S` on a Mac does not fire a `command` shortcut. Matching is
  exact, so `Cmd+S` never fires on `Cmd+Shift+S`.
- **Typing wins.** While a text field has focus, keys it handles itself (`Delete`, arrows, `Enter`,
  `Cmd+A/C/X/V`) never reach an app shortcut — but `Cmd+S` still saves.
- **Panels are scoped automatically.** A shortcut declared inside a dock panel only fires while that
  pane has focus, so the same key can mean different things in the outliner and the viewport. Wrap
  anything else in `ui.shortcut_scope(active, |ui| …)`; scopes nest, and an inactive one disables
  everything within it.
- **Consumption is first-come, first-served,** so check panel shortcuts before global ones — build
  the panels, then the app's keymap. `libgui_demo` does exactly that: `Delete` in the outliner
  deletes the selected object (and does nothing while you type in its search box), while `Space`
  toggles playback globally.

`ui.key_pressed` / `key_down` stay raw and unrouted, for held-key state like a viewport's fly
controls — gate those on `ui.wants_keyboard()`.

## Canvases: pan and zoom

An unbounded coordinate space for node graphs, timelines, piano rolls, curve editors — anything
where the content has its own coordinates and the user moves a viewport over it.

```rust
ui.canvas("graph", &mut view, |ui, view| {
    for node in graph.nodes_in(view.visible) {          // cull to what is on screen
        let bar = ui.interact_drag(node.id);
        if bar.active { node.pos += bar.drag_delta; }   // canvas units: right at any zoom
        ui.layer_in(node.id, Layer::Window, node.rect, frame, |ui| {
            ui.slider("Amount", &mut node.amount, 0.0, 1.0);   // ordinary widgets
        });
    }
});
```

- **Ordinary widgets work inside it.** They lay out, hit-test and report `rect`, `mouse_pos` and
  `drag_delta` in **canvas coordinates**, so app logic is identical at any zoom — no dividing drag
  deltas by the zoom, no transforming the pointer yourself.
- **Position content with `ui.container_at(id, rect, frame, …)`** — a node, a clip on a timeline, a
  key on a curve. It attaches to the current container, so it follows the pan and zoom and is
  clipped to the canvas. `ui.layer` and `ui.layer_in` deliberately hang off the **root** so menus and
  floating windows sit above everything; that also means they ignore the canvas, so do not use them
  to place canvas content.
- **Text is rasterised at the zoomed resolution**, not scaled up from a 1x bitmap, so a node's
  labels stay crisp. The size is quantised so a continuous zoom does not re-rasterise every frame,
  and `measure` is unaffected, so layout is identical at any zoom.
- **The transform lives in `DrawList`,** so every primitive is mapped and nothing can draw
  untransformed by accident. Corner radii, border widths and blurs scale with the zoom; divide by
  `view.zoom` for hairlines that stay one pixel wide.
- **Off-screen content is culled** by the existing clip test, and `view.visible` lets you skip
  *building* what cannot be seen — the 2D equivalent of a virtualised list.
- Wheel zooms toward the pointer, middle-drag or dragging empty canvas pans; set
  `CanvasState::wheel_zooms = false` for a timeline. The state is the app's, so it can be saved or
  animated. `ui.with_transform(id, t, body)` is the raw primitive.

There is **no rotation**: the shader draws axis-aligned quads, so `Transform` is pan plus uniform
zoom.

The demo's *Node Graph* tab is a worked example: a zooming grid, bezier wires between ports, nodes
with sliders and toggles inside them, drag-to-move, and culling.

## Lines and curves

```rust
p.line(a, b, 2.0, colour);                       // round caps
p.polyline(&points, 1.5, colour);                // joins are round for free
p.bezier(p0, c0, c1, p1, 2.0, colour);           // flattened by on-screen size
p.wire(from, to, 2.0, colour);                   // node-graph cable: horizontal tangents
```

Node wires, automation and easing curves, waveforms, motion paths, line charts — none of which the
axis-aligned rounded rect could express.

A segment is one instance evaluated as a capsule SDF, so it is anti-aliased, clipped and culled like
everything else, and it follows the canvas transform. Overlapping round caps make a round join, so a
polyline needs no join geometry. (Two segments double-blend where they meet, which shows only on
translucent strokes.)

This bumped `CONTRACT_VERSION` to 2, but **not** the instance layout: a segment's endpoints ride in
`uv`, which untextured primitives do not use, so the 96-byte stride and the six attributes a backend
binds are unchanged. Curves are flattened on the CPU by their size *on screen*, so they stay smooth
zoomed in without wasting instances zoomed out.

## Menus, popups and tooltips

Floating content stacks by `Layer` (`Window` < `Popup` < `Tooltip` < `Drag`) rather than by build
order, so a menu is above a torn-off panel however early the panel was built.

```rust
ui.menu_button("File", |ui| {
    if ui.menu_item_shortcut("Save", Shortcut::command(Key::S)).clicked { save(); }
    ui.menu_item_ex("Undo", None, can_undo);            // greyed out when it cannot run
    ui.menu_separator();
    ui.submenu("Export", |ui| { /* … */ });
});

let r = ui.selectable(&name, selected);
ui.context_menu(&r, |ui| { if ui.menu_item("Rename").clicked { rename(); } });
ui.tooltip(&r, "Double-click to rename");
```

- **A menu blocks what is under it.** An open popup puts an invisible full-window sheet in the
  `Popup` layer beneath its panels, so content behind cannot be hovered or clicked through, and a
  press on it dismisses. Choosing an item dismisses too; Escape backs out one level.
- **Menu bars behave like menu bars:** with one menu open, moving across the other buttons opens
  them.
- **Shortcuts shown in a menu are only labels.** `menu_item_shortcut` prints `⌘S` or `Ctrl+S` from
  the same `Shortcut` you handle with `consume_shortcut`; declaring it in the menu does not bind it.
- **A popup sizes itself to its content**, flips above its anchor when there is no room below, and
  is clamped on screen. It is measured a frame late, like `Response::rect`, so it settles on the
  frame after it opens.
- **Tooltips** wait `theme.tooltip.delay`, sit above popups, are never interactive, and never appear
  while a menu is open or a drag is in progress. An idle UI wakes itself to show one.

`ui.popup(id, min_width, body)` is the primitive underneath, with `open_popup`, `close_popups` and
`popup_open` if you want to drive one yourself. Look is themed via `theme.menu` and `theme.tooltip`.

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

## Virtualised lists

```rust
ui.virtual_list("objects", scene.len(), 24.0, |ui, i| {
    if ui.selectable_keyed(i, &scene[i].name, i == selected).clicked { selected = i; }
});
```

Only the visible rows are built, so a list of a million items costs the same as a list of fifty
(measured: 39 rows built, 203 draw instances, ~18 µs either way). Rows that were not built are
replaced by spacers of the right height, so layout, the scrollbar and the scroll maths still see the
whole list.

The contract is that every row is exactly `row_height` tall — that is what lets the library place
row *n* without having built rows `0..n`. Rows are clipped to it. `ListOptions` adds `gap`,
`padding`, `overscan` (rows built beyond the viewport, so a fast fling shows no gap) and
`stick_to_end`.

**Rows of different heights** use `ui.virtual_rows(key, rows, |i| height_of(i), |ui, i| …)`. Only the
visible rows are *built*, but locating the first one costs a `height` call per row, so the frame is
O(rows) rather than O(1): about 2 ns per row (100 000 rows ≈ 210 µs). Good into the tens of
thousands; use `virtual_list` when the rows really are uniform and the list is huge.

Rows are addressed by index. For a filtered or sorted view, resolve to a list of indices first and
virtualise over that, keying rows by the underlying item so selection follows it.

## Trees

libgui does not own your tree. You keep the nodes and which are expanded, flatten the visible ones
each frame, and virtualise over that list — so a tree costs only what is on screen, however deep or
wide:

```rust
let rows = flatten(&tree, &expanded);                  // Vec<(node, depth)>
ui.virtual_list("tree", rows.len(), row_h, |ui, i| {
    let (node, depth) = rows[i];
    let r = ui.tree_row(node, depth, branch_of(node), &tree[node].name, node == selected);
    if r.toggled { toggle(&mut expanded, node); }      // the arrow
    if r.response.clicked { selected = node; }         // the row
});
```

`tree_row` draws the indentation, a disclosure arrow and a label styled like `selectable`.
`toggled` and `response.clicked` are never both true, so expanding a node never also selects it.
Indent width is `theme.metrics.indent`. The arrow is a glyph (`▸`/`▾`) because the shader has no
triangle primitive.

The demo's outliner is a worked example of all of it: a variable-height tree (group headers are
taller than leaves), virtualised, that flattens to a plain list of hits while the search box is in
use — see `libgui_demo/src/panels.rs::outliner_rows`.

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
- **Font backend (`FontRasterizer`):** libgui asks a font for line metrics, to *shape* a string into
  glyph ids (so HarfBuzz/CoreText/DirectWrite ligatures, contextual forms and marks work), and to
  rasterise one glyph. It keeps measurement, shaped-run caching, DPI/zoom fitting, carets (mapped
  through clusters), the atlas and pixel snapping. `FontdueRasterizer` (feature `fontdue`, default) is
  built in; register your own with `Fonts::add_rasterizer` / `Ui::with_rasterizer`.
- **Features:** `theme-toml` (default) parses/exports themes, pure data; `theme-watch` (opt-in) adds
  `ThemeWatcher`, the only filesystem access in the crate; `fontdue` (default) is the built-in font
  backend, and without it the core has only `bytemuck` as a dependency.

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
| A 1,000,000-row virtual list vs a 100-row one | **identical** — 39 rows built, 203 instances, ~13 µs |
| 100,000 variable-height rows | ~210 µs (locating is O(rows); building is not) |
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
2. ~~Scroll areas~~ ✅ ~~virtualised lists, variable row heights, trees~~ ✅ `ui.virtual_list`, `ui.virtual_rows`, `ui.tree_row`; next: horizontal scroll, keyboard PageUp/Down, multi-select and drag-to-reparent.
3. ~~Keyboard/shortcut routing~~ ✅ ~~menus, popups/context menus, tooltips, z-order~~ ✅ `Layer`, `popup`, `menu_button`, `context_menu`, `tooltip`; next: checkable/icon menu items, keyboard navigation within a menu, "safe triangle" submenu tracking.
4. ~~Docking + tabs + splitters~~ ✅ Unity-style with OS-window tear-off; next: layout save/load, tab close/context menu, maximize pane.
5. ~~Paths~~ ✅ `p.line` / `polyline` / `bezier` / `wire`, a `Line` primitive at `CONTRACT_VERSION` 2;
   next: stroked/filled arbitrary paths, dashes, arrowheads, and a real line/area plot (`plot` is
   still a debug bar chart).
6. **Horizontal and 2D scrolling**, then tables/data grids with resizable and frozen columns.
7. **Real text shaping**: replace `text.rs` internals with `cosmic-text`/`swash` or HarfBuzz
   (ligatures, bidi, font fallback, CJK), multi-page atlas with LRU eviction.
8. ~~Theme hot-reload~~ ✅ TOML themes, per-widget styles, density presets; next: multiple fonts (UI/mono/icons) in the theme, per-widget disabled states.
9. **Accessibility** via AccessKit: emit a node per interactive widget from the same tree.
10. **Perf**: skip layout/paint for unchanged subtrees, persistent GPU buffers,
   and a "sleep when idle" mode for tools (CAD) instead of redrawing continuously.

## Known scaffold limitations

- Text fields are single-line; no wrapping, IME preedit, or undo yet. No complex shaping.
- Glyph atlas resets when full (possible one-frame flicker).
- Container ids are positional; give containers explicit keys once you add conditional UI.

Font: Inter (SIL Open Font License, see `assets/Inter-OFL.txt`).
