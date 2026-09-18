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
- **One-frame-late input**: `Response.rect` is last frame's rect. Invisible in practice,
  and it keeps the model simple.
- **Paint closures** run after layout with final rects. Custom widgets and
  viewport overlays use the same `Painter`, which is the immediate layer.
- **SDF quads** (`libgui_wgpu/src/ui.wgsl`): rounded rects, borders, soft shadows,
  glyphs, rounded images, all in one shader, premultiplied alpha, DPI-correct AA.
- **Theme tokens** (`theme.rs`): every colour/metric lives in one struct.

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
  your renderer's viewport targets appear in the UI.

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

## Integrating with your engine

- Render your scene to a texture, `renderer.register_texture(&view)` (wgpu backend), then show it with
  `ui.viewport(...)`. Use the returned `Response` for camera/gizmo input.
- Use `ui.wants_mouse()` to decide whether input goes to the game or the UI.
- Call `backend.render(&mut pass, &out)` inside any pass you already own (e.g. after
  your post-processing), so the UI composites over the frame.
- Run it alongside Dear ImGui during migration: both just record into the same pass.

## Roadmap (roughly in order)

1. ~~Text input~~ ✅ single-line; next: multi-line editor, IME preedit, double-click word select, undo.
2. ~~Scroll areas~~ ✅; next: virtualised lists/trees (only build visible rows), horizontal scroll, keyboard PageUp/Down.
3. **Keyboard/shortcut routing**, menus, popups/context menus, tooltips (needs a layer/z-order stack).
4. **Docking + tabs + splitters**: a retained dock tree stored in `Ui`, serialised to disk.
5. **Real text shaping**: replace `text.rs` internals with `cosmic-text`/`swash` or HarfBuzz
   (ligatures, bidi, font fallback, CJK), multi-page atlas with LRU eviction.
6. **Theme hot-reload** from a RON/TOML file.
7. **Accessibility** via AccessKit: emit a node per interactive widget from the same tree.
8. **Perf**: skip layout/paint for unchanged subtrees, persistent GPU buffers,
   and a "sleep when idle" mode for tools (CAD) instead of redrawing continuously.

## Known scaffold limitations

- Text fields are single-line; no wrapping, IME preedit, or undo yet. No complex shaping.
- Glyph atlas resets when full (possible one-frame flicker).
- No z-layers yet: overlays are drawn inside their node's paint closure.
- Container ids are positional; give containers explicit keys once you add conditional UI.

Font: Inter (SIL Open Font License, see `assets/Inter-OFL.txt`).
