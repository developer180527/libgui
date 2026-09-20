# libgui: hybrid immediate/retained UI

A UI library for professional tools — node graphs, editors, timelines, CAD, engines — that you own
end to end.

```
cargo run --release -p libgui_demo          # editor demo with a wgpu 3D viewport
cargo test -p libgui                         # layout + backend-contract tests
cargo run -p libgui_shaders -- shaders_out   # export HLSL/MSL/GLSL/SPIR-V/WGSL
```

## Status

| | |
|---|---|
| CI | build, test, clippy and docs on Linux (x86_64 + ARM), Windows, macOS (ARM) |
| Also builds for | wasm32, Android (aarch64), iOS + simulator, 32-bit x86 |
| Portability | `Id` values verified under emulation on 32-bit **and big-endian** targets |
| Not tested | Android and iOS run on device, physical iPad, mixed-DPI multi-monitor docking |
| Rust | **1.90** for the GPU crates (`wgpu-hal` on Linux/Windows/Android), **1.87** for `libgui`, `libgui_nodes`, `libgui_soft` and `libgui_keymap` |
| Licence | **not yet chosen** — see `LICENSING.md` |
| Version | 0.1.0, pre-1.0: the API still changes between releases |

Not yet: multi-line text, font fallback, IME composition display, accessibility, colour picker,
layout persistence. See the roadmap.

## Crates

| crate | role |
|---|---|
| `crates/libgui` | Core. **No GPU code.** IDs, retained state, layout, input, theme, text, draw list, widgets, and the `Backend` trait. |
| `crates/libgui_shaders` | The one UI shader. Authored in WGSL, validated + cross-compiled by naga at build time to HLSL (SM 5.1), MSL 2.0, GLSL 4.50, SPIR-V. |
| `crates/libgui_wgpu` | `Backend` implementation for wgpu. Also the reference for writing your own. |
| `crates/libgui_keymap` | Cross-platform keymaps: each platform's bindings for libgui's widget actions, rebindable app actions, menu spelling (`⇧⌘S` / `Ctrl+Shift+S`). Optional. |
| `crates/libgui_soft` | CPU reference `Backend`: renders a frame to an RGBA8 image, the same bytes on every machine. Golden-image tests live here. |
| `crates/libgui_nodes` | Node-graph editing: nodes, ports, links, selection, routing. Built *on* libgui, not in it. |
| `crates/libgui_demo` | winit host + editor layout + an "engine" scene rendered offscreen and shown via `ui.viewport`. |
| `crates/libgui_solaris` | A second demo: one dense, Houdini-shaped editor — menu bar, shelf, viewport, parameter panel, node network, scene-graph tree, details table and timeline, all at once. Its own theme, and a ~170-line host. |
| `crates/libgui_cut` | A third demo: a non-linear video editor. A **timeline** — zoom, two-axis scroll, clips dragged between tracks, snapping — written as one custom surface on top of the library. |

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

- Caret + selection (mouse drag, or `Move { select: true }`), word and line movement and deletion,
  select-all, horizontal auto-scroll, placeholder, focus ring, blinking caret.
- The field acts on `UiAction`s, never on keys: which chords move by word or delete to the line start is
  the keymap's (see below). Without installed bindings it takes typed text but ignores Backspace.
- Keyboard focus: click to focus, click elsewhere / `Cancel` / `Submit` to release, `FocusNext` /
  `FocusPrevious` cycle fields. `ui.wants_keyboard()` tells the host not to route keys to the game.
- The host sends `Text` and `Paste` events and writes `PlatformOutput::copied_text` to the clipboard,
  so the core has no platform or clipboard dependency. `libgui_demo` is a winit + arboard reference.
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

## Keyboard shortcuts and keymaps

libgui has **no keymap** and no idea which OS it is on. It works in three layers:

- **Physical input.** `Key`s are layout-independent (USB HID usages), `Modifiers` are the four physical
  modifier keys, and a `Shortcut` is a concrete chord like Ctrl+Shift+S, matched exactly.
- **Widget actions.** libgui's own widgets respond to `UiAction`s (`Move { motion, select }`,
  `Delete(motion)`, `SelectAll`, `Copy`/`Cut`/`Paste`, `Submit`, `Cancel`, `FocusNext`/`FocusPrevious`),
  never to keys. The chord → action table is a `KeyBindings` the app installs with
  `ui.set_key_bindings(..)`; it is **empty by default**. A host with its own input layer (raw HID, a
  gamepad, an OS Edit menu) can push `InputEvent::Action(..)` instead.
- **Policy lives in `libgui_keymap`**, an optional crate: each platform's native widget bindings
  (Option-word, Cmd-line and the Emacs keys on macOS; Ctrl-word and Ctrl+Insert / Shift+Insert on
  Windows and Linux), platform-neutral chords for your own actions, rebinding, and menu spelling.

```rust
#[derive(Clone, Copy, PartialEq)]
enum Action { Save, Redo, Frame }

let mut keys = Keymap::for_current_platform();          // or Keymap::new(Platform::Mac)
keys.bind(Chord::primary(Key::S), Action::Save)         // Cmd+S on a Mac, Ctrl+S elsewhere
    .bind(Chord::primary(Key::Z).shift(), Action::Redo)
    .bind(Key::F, Action::Frame);
keys.install(&mut ui);                                   // widget bindings, once per Ui/window

if keys.triggered(&mut ui, Action::Save) { save(); }    // routed through ui.consume_shortcut
ui.menu_item_shortcut("Save", &keys.label(Action::Save)); // "⌘S" / "Ctrl+S"
keys.rebind(Action::Frame, Key::Period);                 // a keymap editor's "set"
```

Routing, which is libgui's:

- **Exact matching.** Ctrl+S never fires on Ctrl+Shift+S, and Cmd+S is not Ctrl+S.
- **Typing wins.** While a text field has focus, `consume_shortcut` refuses any key the bindings turned
  into an action this frame (Backspace, arrows, Cmd+A…) and any chord that types a character (`Space`,
  `F`). A chord with Ctrl or Cmd the field does not use (`Cmd+S`) still gets through.
- **Panels are scoped automatically.** A shortcut declared inside a dock panel only fires while that
  pane has focus, so the same key can mean different things in the outliner and the viewport. Wrap
  anything else in `ui.shortcut_scope(active, |ui| …)`; scopes nest, and an inactive one disables
  everything within it.
- **Consumption is first-come, first-served,** so check panel shortcuts before global ones — build
  the panels, then the app's global actions. `libgui_demo` does exactly that: Delete in the outliner
  deletes the selected object (and does nothing while you type in its search box), while Space
  toggles playback globally (and types a space while you are in a field).

`ui.key_pressed` / `key_down` stay raw and unrouted, for held-key state like a viewport's fly
controls — gate those on `ui.wants_keyboard()`.

## Wrapping text

```rust
ui.paragraph("Text that wraps to the width it is given, and grows downwards.");
ui.paragraph_with(text, 13.0, colour, Align::Center);
```

A paragraph's height follows from its width, which layout only knows *after* it
has run. So a frame containing one solves twice: measure, place, re-measure the
paragraphs against the widths they got, and — only if that changed an answer —
solve again. A steady frame costs a walk of the paragraphs and nothing more,
and the frame a panel is resized on is right **on that frame**, not one later.
Dragging a splitter would otherwise leave a gap under every paragraph in the
window.

Its **minimum** width is its longest unbreakable word, not its full length, so
a paragraph never forces the panel around it wider. The consequence is that one
inside a `Size::Fit` container collapses to that word, because a `Fit` container
asks its children how wide they would like to be and a paragraph has no answer.
Give the container a width.

Line breaking is not UAX #14 — that needs a break-class table for every code
point — and `wrap.rs` says exactly what it is instead: after whitespace, after a
hyphen or slash, and between ideographs, which is what makes Japanese and
Chinese wrap at all rather than running off the edge as one line. It keeps the
part of *kinsoku shori* a reader notices (no line starting with closing
punctuation, none ending with an opening bracket), and it cuts a word that
cannot fit rather than letting it overflow. Thai and Khmer need word
segmentation and will break only at spaces; hyphenation dictionaries are not
there. Those belong behind `FontRasterizer`, the same seam complex shaping goes
through.

## Composing text (IME)

Typing Japanese, Korean or Chinese goes through a composition the user sees and
edits before accepting it. `InputEvent::ImePreedit { text, cursor }` carries it;
the field draws it inline at the caret, underlined, with the IME's own cursor
inside it — and **the `&mut String` does not change** until the host sends
`InputEvent::Text`. An empty preedit means the user backed out.

`PlatformOutput::text_input` reports the rect of what is being composed rather
than of the bare caret, so the candidate window sits under the text it belongs
to. `libgui_winit` maps winit's `Ime::Preedit`, `Enabled` and `Disabled`; any
other host does the same three lines.

## Keyboard focus

Every control is reachable from the keyboard, and **libgui decides nothing about
how**. Which chord moves focus, which activates, and even *which kinds of widget
the keyboard visits at all* are platform conventions that platforms disagree on
— so they are the app's, and the core is a mechanism with no opinion.

```rust
// A widget says what it is, and asks whether it has focus.
let r = ui.interact_focusable(id, FocusKind::Control);
if r.clicked { /* pressed, or activated from the keyboard */ }
if r.focused { /* draw a focused state, if the widget has one */ }
```

Widgets never look at keys. A focused widget is activated by `UiAction::Submit`
and left by `UiAction::Cancel`; focus moves on `FocusNext` / `FocusPrevious`.
What chord produces any of those is the keymap's, and a host with no keyboard at
all sends them directly with `InputEvent::Action` — a gamepad, a foot pedal, an
accessibility switch. Every test in `tests/focus.rs` drives the UI that way: not
one of them presses a key, and one fills in and submits a whole form with no
pointer in existence.

**The policy is the app's**, because the conventions differ:

| | macOS | Windows / Linux |
|---|---|---|
| Tab visits text fields | yes | yes |
| Tab visits buttons, toggles, sliders | only with Full Keyboard Access | yes |
| Clicking a button focuses it | no | yes |

```rust
ui.focus_policy = libgui_keymap::focus_policy(Platform::current());  // or
ui.focus_policy = libgui_keymap::full_keyboard_access(Platform::current());
keymap.install(&mut ui);                                            // does both
```

`FocusPolicy::default()` is neutral — everything reachable — because a UI that
cannot be driven from the keyboard is broken, and a library that quietly decides
otherwise on your behalf is worse than one that asks. `libgui_keymap` carries the
per-platform narrowing, takes the platform as an *argument*, and is opt-in:
`tests/boundaries.rs` fails the build if `target_os` ever appears in the core, so
libgui can always be asked to behave like a platform it was not built for.

The focus ring is drawn once, centrally, and clipped by whatever clips the
widget — so it works for custom widgets too — and only when focus arrived **by
keyboard**. A ring that appears under the mouse is noise.

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

## Node graphs (`libgui_nodes`)

A separate crate, because node editing is a domain with its own opinions and libgui stays general.
It never stores your graph: you declare it each frame, it lays out, draws and runs the interactions,
and hands back edits to apply.

```rust
let (events, ()) = libgui_nodes::graph(ui, "shader", &mut state, &style, |g| {
    for n in &mut doc.nodes {
        let cfg = NodeConfig::new(&n.title).inputs(&["A", "B"]).outputs(&["Out"]);
        g.node(n.id, n.pos, &cfg, |ui| {            // ordinary libgui widgets
            ui.slider("Amount", &mut n.amount, 0.0, 1.0);
        });
    }
    for l in &doc.links { g.link(l.from, l.to); }
});
for e in events { doc.apply(e); }                   // one place owns every edit
```

- **Edits are events, not mutations** (`NodeMoved`, `LinkCreated`, `LinkRemoved`, `SelectionChanged`,
  …), so undo, validation and collaboration have exactly one place to live.
- **Drag a port to link.** The loose end eases into any compatible port within `snap_px` — measured
  on screen, so it feels the same at any zoom. Dragging an input that already has a link picks that
  link up and re-routes it, which arrives as a `LinkRemoved` then a `LinkCreated`.
- **Links are pickable along their curve**, not by a bounding box, so crossing wires behave.
- **Selection**: click, shift/cmd-click to add, drag the selection as a group, click empty canvas to
  clear. Selected nodes rise above their neighbours.
- **`GraphStyle` is the whole look** — node fill, header, stripe, port size and grab radius, wire
  width and colours, snap distance and speed, routing (`Bezier` / `Orthogonal` / `Straight`).
  `GraphStyle::from_theme` derives it so a graph matches the app by default.
- Interaction state (`GraphState`) is yours: save it, inspect it, or drive it. `frame_bounds` fits
  the view to a rect.

Not there yet: marquee select, undo helpers, reroute nodes, comment/group boxes, a minimap, and
keyboard navigation.

## Menus, popups and tooltips

Floating content stacks by `Layer` (`Window` < `Popup` < `Tooltip` < `Drag`) rather than by build
order, so a menu is above a torn-off panel however early the panel was built.

```rust
ui.menu_button("File", |ui| {
    if ui.menu_item_shortcut("Save", &keys.label(Action::Save)).clicked { save(); }
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

## Widgets

`button` · `button_primary` · `checkbox` · `radio` · `toggle` · `slider` · `slider_vertical` ·
`drag_value` · `progress` · `combo` · `segmented` · `text_input` · `selectable` · `tree_row` ·
`label`/`heading`/`section` · `separator` · `plot` · `viewport` · menus and `context_menu` ·
`virtual_list`/`virtual_rows` · `canvas`.

Two worth calling out:

```rust
ui.drag_value_range("Scale", &mut scale, 0.005, 0.3..=2.0);   // drag left/right to change
ui.slider_vertical("Gain", &mut gain, 0.0, 1.0, 110.0);       // a fader: high at the top
```

`drag_value` is the control an inspector is mostly made of. It locks the pointer while dragging, so
a drag keeps going past the screen edge; the `word` modifier gives fine control and `shift` coarse.

## Scroll areas

```rust
ui.scroll_area("inspector", |ui| { /* fills remaining height */ });
let opts = ScrollOptions { stick_to_end: true, ..ScrollOptions::new(Size::Grow(1.0)) };
ui.scroll_area_with("console", opts, |ui| { /* log lines */ });
```

```rust
ui.scroll_area_with("timeline", ScrollOptions::both(Size::Grow(1.0), Size::Grow(1.0)), |ui| { … });
ui.scroll_area_with("ruler", ScrollOptions::horizontal(Size::Grow(1.0), Size::Fixed(28.0)), |ui| { … });
```

- **Scrolls in either or both directions.** The axes are independent — a sideways wheel does not
  nudge the vertical offset. A vertical-only area ignores wide content, so one long label cannot make
  a column scroll sideways. `Grow` children fill the *content* box, so rows in a horizontally
  scrolling area span its whole width.
- On an area that only scrolls sideways, a plain vertical scroll scrolls it: what you expect over a
  timeline.
- Scrolling goes to the innermost scroll area under the pointer (nesting works).
- **No device is named anywhere in the scroll logic.** See below.
- **A moving scroll runs sub-pixel; a still one sits on the pixel grid.** At rest the offset is
  rounded to whole physical pixels, so text is crisp and boxes have hard edges. The moment it moves,
  both the offset *and* the text baselines inside it stop rounding. That pairing matters: the first
  frames of a trackpad flick are fractions of a pixel each, and rounding them away turned a smooth
  ramp into "nothing, nothing, nothing, a whole pixel" — a stutter at the start of every scroll.
  Un-rounding the boxes alone would instead shear each label against the row it sits in, so the two
  switch together. Moving text is resampled by the shader's bilinear fetch, which is invisible in
  motion and exact again the moment the scroll stops.
- Overlay scrollbar fades in on hover, widens under the pointer; drag the thumb or click the track to jump.

### Smoothing is policy, not a guess about your hardware

A trackpad, a high-resolution wheel, a trackball, a joystick axis the host samples once a frame, a
jog dial, a MIDI encoder, an accessibility switch — they all arrive as `InputEvent::Wheel`, and
libgui neither knows nor asks which one it is. The only question it puts to the host is what one
unit of the delta *means*:

| `WheelUnit` | what it says | default handling |
|---|---|---|
| `Pixel` | **continuous** — already logical px, from something that moves smoothly | applied in full, the frame it arrives |
| `Line` | **stepped** — one event is a whole detent | eased over a few frames |
| `Page` | **stepped** — one event is a page | eased over a few frames |

That is a property of the *signal*, and the host is the only thing in the stack that knows it. A
free-spinning wheel reporting eighths of a detent should send `Pixel`; scroll driven from a D-pad
should send `Line`. Nothing further down has to recognise a device.

What the UI then does with each is entirely the app's, in `ScrollConfig`:

```rust
pub struct ScrollConfig {
    pub line: f32,              // logical px per notch          (24)
    pub page: f32,              // logical px per page          (480)
    pub continuous: Smoothing,  // Instant
    pub stepped: Smoothing,     // Eased { rate: 20.0 }
    pub friction: f32,          // fling deceleration, 1/s      (3.2)
    pub fling_cutoff: f32,      // a fling slower than this has stopped (5 px/s)
}
pub enum Smoothing { Instant, Eased { rate: f32 } }
```

```rust
ui.scroll.continuous = Smoothing::Eased { rate: 30.0 };   // ease everything, globally
ui.scroll.line = 3.0 * row_height;                        // three rows a notch

// or per area — a timeline need not feel like an inspector
let opts = ScrollOptions {
    config: Some(ScrollConfig { stepped: Smoothing::Instant, ..ui.scroll }),
    ..ScrollOptions::horizontal(Size::Grow(1.0), Size::Fixed(28.0))
};
```

`Eased { rate }` closes that fraction of the remaining distance per second, so it is frame-rate
independent, and it ends when it has less than half a *physical* pixel left to travel. Two things
override the config, because they are direct manipulation rather than a signal to interpret:
dragging a scrollbar thumb, and a finger on the glass (and the fling it throws). Those always track
exactly.

A scroll area remembers which smoothing is driving its current approach, so an ease a notch started
carries on through the input-free frames that follow it rather than snapping the moment the events
stop.
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
Indent width is `theme.metrics.indent`. The arrow is drawn as two strokes (`Painter::chevron`), not
a glyph, so it does not depend on the font having `▸`/`▾`.

The demo's outliner is a worked example of all of it: a variable-height tree (group headers are
taller than leaves), virtualised, that flattens to a plain list of hits while the search box is in
use — see `libgui_demo/src/panels.rs::outliner_rows`.

## Drag and drop

Any widget can be a drag source, any container a drop zone. A drag carries a `Payload`: a `kind`
that zones filter on, a `label` for the ghost, and a value only your app understands.

```rust
// source: a row that is both selectable and draggable resolves its input once
let r = ui.tree_row(("o", i), depth, Branch::Leaf, &names[i], i == selected);
let drag = ui.drag_source_from(&r.response, || Payload::new("object", i).with_label(&names[i]));
if r.response.clicked && !drag.dragging { selected = i; }

// zone: the innermost open container
let zone = ui.drop_zone(&["object"]);
if zone.hovered { ui.insertion_line(row_id, Axis::Y, zone.pointer.y >= mid); }
if let Some(p) = zone.dropped {
    if let Ok(from) = p.take::<usize>() { reorder(from, at); }
}

// once per frame, last: the ghost that follows the pointer in `Layer::Drag`
ui.drag_ghost();
```

The rules:

- A press becomes a drag only past `ui.drag_threshold` (4 logical px), so a click is still a click.
  The payload closure runs at that moment, not on every frame of hovering.
- Zones nest. The **innermost** zone under the pointer that accepts the drag wins, and a zone that
  does not accept the kind is not in the running at all — so a list of `"object"` rows inside a
  panel that accepts `"file"` does not swallow the file. One container is one zone.
- Releasing offers the payload to that zone for exactly one frame. `dropped` hands you the value by
  move; nobody takes it, it is gone. `Escape` cancels.
- `zone.pointer` is in the space the zone was built in (canvas coordinates inside a canvas), so
  working out an insertion index is the same arithmetic at any zoom.
- `zone.rect` and `ui.drop_highlight(rect)` draw the whole-zone highlight; `ui.insertion_line` draws
  the between-rows one. Both are ordinary nodes, so a scroll area clips them.

Drags from **outside** the UI are the host's to detect, and the core only routes them:

```rust
// host: winit reports a file drag one path at a time, with no position
let mut files = libgui_winit::FileDrop::default();
files.push_window_event(&mut ui, &event);          // -> ui.begin_external_drag(..)

// UI: identical to any other drag
if let Some(p) = ui.drop_zone(&[libgui_winit::FILES]).dropped {
    if let Ok(paths) = p.take::<Vec<PathBuf>>() { open(paths); }
}
```

`Ui::begin_external_drag` / `end_external_drag` is the whole seam. `libgui` itself never sees a
`PathBuf`, a clipboard format or an OS drag session: `libgui_winit::FileDrop` is one small,
replaceable host adapter, and any other host writes its own.

The demo's outliner does both: rows reorder by dragging, and files dropped from Finder or Explorer
become objects.

## A second demo

```bash
cargo run --release -p libgui_solaris
```

The first demo shows the features one at a time, which is how you learn them and
not how anyone uses them. `libgui_solaris` puts a whole tool's worth of UI on one
screen — a menu bar, a shelf, a viewport with rails and a HUD, a parameter editor
with fifty controls, a node network, a scene-graph tree, a details table and a
timeline — because density is the thing that actually breaks: the type size, the
splitters, the id scheme and the frame budget all hold up fine one panel at a
time.

It has its own [`Theme`] and nothing else of its own: no new widget mechanism, no
reaching inside the core. Its host is about 170 lines, because that is what a
host is once docking tear-off and a 3D scene are somebody else's problem.

## A third demo: a video editor

```bash
cargo run --release -p libgui_cut
```

`libgui_cut` is shaped like a non-linear editor — two monitors, a project bin, an
effect-controls tree and a sequence — because it needs the one widget no UI
library ships: a **timeline**.

The timeline is not a tree of widgets. Everything in it is decided by the same
two numbers, pixels-per-second and the scroll offset, and a widget tree would
spend its life keeping a ruler, a header column and a thousand clips agreeing
about them. So it takes a single rect from libgui and does the rest itself:

- **Two-axis scroll.** Horizontally the ruler and the clips move together while
  the track headers stay put; vertically the headers and the clips move together
  while the ruler stays.
- **Zoom about the pointer** (Cmd/Ctrl+wheel), so the frame under the cursor is
  still under the cursor afterwards.
- **Drags**: clips along time and between tracks — refusing a video track for
  audio — the playhead on the ruler, and both scrollbars.
- **Snapping** to clip edges and the playhead, which is what makes cutting feel
  solid rather than approximate.

That is the point of the demo: libgui supplies ids, layout, input routing, the
dock and the theme, and a custom surface of a few hundred lines supplies the
domain. `tests/timeline.rs` drives it with real presses, drags and wheels — the
timeline has to *behave*, not just render — and `tests/look.rs` renders the whole
editor to a PNG with the CPU backend, so the layout can be reviewed without a
window.

`cargo test -p libgui_solaris` renders it to `editor.png` with the CPU backend,
so the layout can be looked at without a window, and asserts what the whole
screen costs: **719 nodes, 5,459 instances, one draw call, no glyph
rasterisation and no allocations** in a steady frame.

Its panels are a dock tree, not a hand-written nesting: every splitter drags,
every panel tears off into its own OS window, and the tab strips down each
panel's top edge are real dock tabs rather than decoration — so they switch,
reorder, move between panels and tear off like anything else. The host is one
OS window per dock surface, sharing a single wgpu device.


## Tables and data grids

Virtualised rows, resizable and sortable columns, a header that stays put, and columns that can be
frozen against horizontal scrolling. Like the tree, libgui does not own your rows: it asks for the
cells it can actually see, by row and column index, and reports what the user did.

```rust
let mut cols = TableState::new([
    Column::new("Name").width(180.0).grow(1.0),
    Column::new("Kind").width(90.0),
    Column::new("Size").width(90.0).align(Align::End),
]).frozen(1);                                    // Name stays put when scrolled

let t = ui.table("assets", &mut cols, assets.len(), |ui, row, col| match col {
    0 => ui.label(&assets[row].name),
    1 => ui.label_muted(assets[row].kind),
    _ => ui.label_muted(&assets[row].size),
});
if let Some((col, order)) = t.sort_changed { sort(&mut assets, col, order); }
if let Some(i) = t.clicked_row { selected = Some(i); }
```

- **Rows are virtualised**, so 250,000 of them cost what fifty do. **Columns are not**: every column
  of every visible row is built and clipped if it is off to the side, which is the right trade for
  the tens of columns a table has and the wrong one for hundreds.
- **Sideways scrolling belongs to the table**, not to a scroll area, because the header, the frozen
  pane and every row have to agree on one offset to the exact pixel. A sideways gesture scrolls it,
  and so does a plain one with Shift held. The offset lives in `TableState`, so it saves and
  restores with the rest of your layout — as do the column widths and the sort order.
- **Cells hold widgets**, not just text: a toggle, a drag value, a colour swatch, anything.
- The same sub-pixel rule as scroll areas: a moving table draws its text sub-pixel, a still one
  snaps back to the grid. Row bands and column rules snap always — a band is a boundary, not a
  shape, and a half-pixel one is a smear at odd DPIs (`Painter::snap_rect` and `Painter::hairline`
  are public, for custom widgets that draw either).
- **A 250,000-row table allocates nothing per frame**, which `perf_alloc.rs` asserts.

The demo's Assets tab is a worked example: 200,000 rows, a frozen first column, sortable headers.

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

### Only build a window for a surface you can see

A tab dragged from one panel to another tears off into a floating surface the
moment it leaves its tab bar, and the dock hides that surface while the pointer
is over a drop target (`hide_window_over_target`). So the usual gesture — move
this tab next to that one — creates a surface that is never shown.

A host that maps surfaces to OS windows eagerly pays for that: an OS window, a
`Ui` with its own font atlas (~9 ms on its own), and a renderer with its own
pipelines, built, hidden before it is ever drawn, and destroyed again on the
drop. The teardown lands between the drop and the frame that shows its result,
which is felt as the tab taking its time to appear. Filter on `Surface::visible`
and the whole round trip disappears; a panel dragged out to the desktop becomes
visible and gets its window then:

```rust
.filter(|s| s.visible && !windows.values().any(|w| w.dock_id == s.id))
```

For that to hold, `visible` must not flicker, and there is one place it could.
`update()` hit-tests against the leaf geometry `show` recorded, and a tear-off
changes the tree — so for two frames (one for `show` to rebuild it, one more
because it reads rects from the previous layout) that geometry describes a
layout that no longer exists. Stale geometry is not allowed to answer "nothing
here": a miss keeps the previous target until the geometry has caught up.
Without that, the preview blinks and the window flashes into existence for a
frame. `crates/libgui/tests/dock_drop.rs` pins both halves.


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
  Flings coast with `ui.scroll.friction`. Drag widgets (sliders, splitters, viewport, tabs, text
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

- **Events:** pointer position and raw `PointerDelta` (unaccelerated), five buttons, wheel (continuous px, or stepped lines/pages),
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
| Allocations per widget per frame | **0** |
| A 500-widget inspector, whole frame | **1** allocation, 8 bytes |
| Allocations for 1,800 nested containers | **0** |
| Allocations in an empty frame | **0** |
| Draw instances for offscreen widgets | **0** (15x the rows, same instance count) |
| A 1,000,000-row virtual list vs a 100-row one | **identical** — 39 rows built, 203 instances, ~13 µs |
| 100,000 variable-height rows | ~210 µs (locating is O(rows); building is not) |
| Glyph rasterisation in a steady frame | **none** (atlas version unchanged) |
| 10,000 live controls (six per row, virtualised) | 307 nodes, 1,533 instances, 0 allocations |
| An idle UI | `repaint_after: None` — the host sleeps |
| A static UI under a host redrawing at 120 Hz | **0** UI frames per second |
| A cached 800-row panel beside a live widget | **0.044 ms** vs 0.487 (11x) |

They assert properties that hold on any machine: deterministic counts, and
*ratios* for complexity (4x the widgets must not cost more than 7x the time,
where linear is 4x) rather than wall-clock times that flake on a loaded CI box.
The one absolute budget is asserted in release only, since a debug build is an
order of magnitude slower. `crates/libgui_bench` is the measuring tool;
these are the regression guards.

### Torture tests

`tests/torture.rs` is the other half: deliberately unreasonable UIs, because the
failure mode there is never a wrong pixel. Writing them found two real defects
the ordinary guards could not have:

- **The atlas was thrashing.** 400 font sizes — what a zooming canvas produces,
  and what CJK will produce — filled it, and it reset to the *same size* and
  re-rasterised all 1,704 glyphs **every frame, forever**. It now grows once
  instead: 1,704 on the first frame, **0** after.
- **Deep nesting aborted the process.** `measure`, `place` and `paint` each
  recurse per level, and a debug build died between 400 and 500. A library may
  render something badly; it may not take the process down. Nesting past 256 is
  now dropped from the tree and reported as `FrameCost::too_deep`.

### The same guards, for your panels

A UI library can be as fast as it likes and still end up slow in your app, because
the expensive mistakes are made in the calling code: every row built instead of
virtualised, a `format!` per row per frame, a missing `with_key`. None of those
look wrong — the UI is perfectly correct, it is just doing a thousand times the
work — and nothing says so until someone opens a real project.

So `libgui::testing` is public. It is the same count-based machinery, pointed at
whatever you write:

```rust
#[test]
fn the_outliner_stays_cheap() {
    let mut ui = Ui::new(Theme::dark(), FONT).unwrap();
    let cost = testing::steady_frame(&mut ui, FrameInfo::default(), |ui| {
        my_app::outliner(ui, &mut state);
    });
    Budget::steady(80).instances(200).assert(&cost);
}
```

`steady_frame` runs a few frames first, because frame one is start-up — glyphs
rasterise once, layout settles, animations are at their starting value — and
returns what the settled frame cost:

| `FrameCost` | what a bad number means |
|---|---|
| `nodes` | how much UI you described. A missing virtualisation shows up here first |
| `instances` / `batches` | work handed to the GPU |
| `glyphs_rasterized` | **0** in a steady frame; anything else is atlas thrash |
| `text_shaped` | **0** in a steady frame; anything else is a string rebuilt with new content every frame (a counter, a timestamp, an unrounded float) |
| `offscreen_nodes` | laid out, then clipped away. A few is a virtual list's overscan; thousands is a `for` loop that should be `virtual_list` |
| `unkeyed_duplicates` | interactive widgets whose identity came from *build order* — a missing `with_key`, so focus, drag and animation state move to the neighbour when the list reorders |

The difference it catches, on the same 2,000 objects:

```
virtualised + keyed   nodes:   47   offscreen:   11   unkeyed: 0      text_shaped: 0
plain for loop        nodes: 2003   offscreen: 1989   unkeyed: 1960   text_shaped: 1
```

`Budget::check` returns every overrun worst-first, so the message leads with the
thing worth fixing. `unkeyed_duplicates` needs `ui.audit = true` (an O(nodes)
scan, which `steady_frame` turns on and a shipping app leaves off); everything
else is counted for free.

### The boundary is a test, not a convention

`libgui` does UI work and nothing else. That is easy to state and easy to erode
one convenience at a time — a `SystemTime::now()` for an animation, a
`std::fs::read` for an icon — so `tests/boundaries.rs` reads the crate's own
source and manifest and fails on the commit that breaks it:

- no `std::fs`, `std::time`, `std::thread`, `std::net`, `std::process`, `std::env`
  or `std::io` anywhere in `src/`, outside `#[cfg(test)]` and the one documented
  exception (`theme_watch`, opt-in, off by default, whose entire job is to poll a
  file);
- direct dependencies must be on an allow-list that carries the reason each one
  is there, so adding a fifth is a decision rather than an import.

No test can see *transitive* dependencies, so CI counts those too:

| `libgui` | crates pulled in |
|---|---|
| `--no-default-features` | **6** — `bytemuck` and the proc-macro crates behind its derive |
| default features | **23** — adds `fontdue` (rasteriser) and `serde`/`toml` (theme files) |

A jump in either fails the build, so growth is something somebody chose rather
than something a version bump did quietly.

### Three arenas, so a frame does not touch the allocator at all

A frame used to cost **2 allocations and 170 bytes per widget**. It now costs
**none**: a 500-widget inspector allocates *once*, for the whole frame, and that
one is a constant rather than a cost per widget. Allocation is global shared
state — a UI that churns it taxes every other thread in the process, not just
itself — so this is the number that matters more than microseconds on any one
machine.

All three came from the same shape of problem: per-node data with a per-frame
lifetime, reached for one `Vec`, one `Box` and one `String` at a time.

- **Children.** A `Node` held a `Vec<usize>`, so every container allocated for a
  list that never changes after it is built, and `place` allocated two more
  temporaries. Children now live in one arena that is reused frame to frame, and
  a node carries an 8-byte range into it. It works because `open_kids` is a
  *stack*: a container's children sit on top of it until the container closes,
  and any container opened inside it has already taken its own children away
  again, so what is left is contiguous. Layers are the exception — they hang off
  the root while other containers are open — so they are collected aside and
  joined to the root's children when it closes, which is where paint order wants
  them anyway. `place` and `paint` mark their slice of a shared scratch buffer,
  use it, and truncate back.
- **Paint closures.** Layout runs after the frame is built, so a widget hands
  over a closure to run once its rect is known, and the obvious home for that is
  a `Box` per widget per frame. `paint_arena.rs` writes the captures straight
  into one reused buffer with a pair of function pointers per entry to call and
  to drop them. **The widget API is unchanged** — a custom widget still passes an
  ordinary closure, and a closure needing an alignment the buffer cannot give is
  boxed first and the box stored inline, so nothing is refused. It is the only
  `unsafe` in the crate: the invariants are written out at the top of the file,
  an entry is marked consumed *before* it runs so a panic cannot double-drop it,
  and its tests cover a closure that never runs, a mid-frame drop, reallocation
  under load and the over-aligned path. They pass under Miri.

- **Text.** A widget is declared before its rect is known, so its paint closure
  has to own the text it will draw, and owning it meant a `String` per text
  widget per frame. A `FrameText` is eight bytes naming a range in a buffer that
  is reused, so the copy lands in space that already exists. **A custom widget
  does not have to use it**: `Painter::text` and friends take anything
  implementing `PaintText`, which `&str` and `String` both do, so an ordinary
  owned `String` in a closure keeps working exactly as before. Handles are valid
  for the frame that made them — the same life as the closure carrying one — and
  a stale handle resolves to `""` rather than to somebody else's text.

### Damage tracking: the frames you don't run

`repaint_after` lets a host that draws *only* for the UI go to sleep. The
interesting case is the host that doesn't: an engine running its viewport at
120 Hz, a DAW drawing meters, a video editor playing back. Those redraw every
frame for their own reasons, and without asking they rebuild, re-lay-out and
re-paint a UI that has not moved.

`Ui::needs_frame(elapsed)` answers **before** the frame is built:

```rust
let elapsed = now - last_ui_frame;
if ui.needs_frame(elapsed) {
    ui.begin_frame(FrameInfo { dt: elapsed, ..info });
    build(&mut ui);
    let out = ui.end_frame();
    renderer.prepare(&out);                        // only now
    batches.clear();
    batches.extend_from_slice(&out.draw.batches);
    last_ui_frame = now;
}
renderer.render_batches(&mut pass, &batches);      // every frame
```

It is true when input is queued, when something is animating, or when the caret
is due to blink — and false otherwise, which is the promise: **the frame you
skipped would have been byte-identical to the one you already have.** That is a
test, not a claim (`tests/damage.rs` compares the instance bytes of two settled
frames).

The instance buffer and the atlas are still the ones `prepare` uploaded, so a
skipped frame needs nothing but the list of draws. A user texture the app is
still rendering into keeps updating, which is how the demo's viewport animates
at full rate while the panels around it cost nothing.

**What this asks of you:** if your UI shows something that changes on its own —
a meter, a clock, a progress bar driven by a worker — call
`ui.request_repaint()` while it does. libgui cannot see your data. Widgets that
animate on their own behalf already do it (an indeterminate `progress` bar, a
pending tooltip), which is also why one of those on screen keeps the whole UI
awake: the demo used to show an idle spinner in the inspector, and nothing could
ever be skipped.

Measured, on a panel of 500 visible widgets: paint is **60%** of a frame, build
**38%**, and layout **3.5%** — so skipping whole frames is worth far more than
skipping layout for unchanged subtrees, which was the obvious-sounding thing to
build and would have bought almost nothing.

#### Resizing is not input

There is one change `needs_frame` cannot see, and it is the one most likely to
be felt: a resize. No `InputEvent` describes it — the new size reaches the UI
only through `FrameInfo` — so a host that gates on `needs_frame` alone happily
re-presents the batches it built at the *old* size. The window edge moves and
the UI inside it does not follow, until some unrelated event wakes it up. It
looks exactly like a slow layout, and it is not: the layout never ran.

So a host with a resizable window gates on the size-aware form, passing the
info the frame *would* be built with:

```rust
let info = FrameInfo { screen_size, scale, dt: elapsed };
if ui.needs_frame_for(&info, elapsed) {
    ui.begin_frame(info);
    build(&mut ui);
    renderer.upload(&ui.end_frame());
}
```

The other half of a resize belongs to the host: reconfigure the swapchain
**once per frame you actually present**, not once per resize event. A live
resize delivers an event per pointer move, and tearing down a swapchain that
often stalls on the frame still in flight. Both demo hosts compare the window
against their config at the top of `draw` and reconfigure only when they
disagree, which also covers the platforms that resize a window without sending
an event at all (rotation, Stage Manager, split view).

Resizing is otherwise ordinary work. Every panel's rect changes, so nothing can
be replayed from the subtree cache — but nothing re-shapes text or re-rasterises
glyphs either, and `crates/libgui_solaris/tests/resize.rs` drags the dense
editor's corner 300 times, one frame per pixel, to keep it that way.

### Profiling: knowing rather than guessing

Every guard in this repo asserts a *count*, because counts are identical on
every machine and cannot flake. Counts are the wrong thing to optimise against,
though — they cannot tell you which phase is expensive. So `--features profile`
adds phase timings to `FrameOutput::profile`, and nothing in the library reads a
clock unless you turn it on:

```
cargo run --release -p libgui_bench --features libgui/profile
```

```text
workload        end_frame   measure   place    paint    paint%
boxes   (500)     0.027ms    0.004    0.005    0.017     63%
labels  (500)     0.074ms    0.003    0.004    0.066     90%
inspector (500)   0.188ms    0.005    0.007    0.171     91%
inspector (2000)  0.590ms    0.016    0.028    0.528     90%
```

**Layout is four per cent of a frame. Paint is ninety.** That number is why
damage tracking here is about instances and not about layout: the obvious
feature to build — "skip layout for unchanged subtrees" — would have bought
almost nothing.

### Subtree caching: repainting only what moved

Frame skipping cannot help when *part* of the window is live: a meter, a clock,
a playhead forces a frame, and the other nine tenths repaint for nothing.
`ui.cached` replays a subtree's recorded instances instead:

```rust
ui.cached("outliner", (objects.len(), revision, selected), |ui| {
    for (i, o) in objects.iter().enumerate() {
        ui.with_key(o.id, |ui| { let _ = ui.selectable(&o.name, i == selected); });
    }
});
```

Measured, with a live label forcing a frame every time:

| rows | uncached | cached | |
|---|---|---|---|
| 200 | 0.197 ms | **0.029 ms** | 6.8× |
| 800 | 0.506 ms | **0.064 ms** | 7.9× |
| 800, pointer resting inside | 0.427 ms | **0.062 ms** | 6.9× |

Same instance count either way — the pixels are identical, and `tests/cache.rs`
proves it the only way worth trusting: it runs a second `Ui` that never caches,
drives both with the same events, and compares the instance **bytes** every
frame.

`deps` is the one thing the library cannot check for you, so get it right — a
length alone will miss a reorder. Everything else *is* checked: a replay happens
only when the pointer is doing the same thing over the subtree as when it was
recorded, keyboard focus is outside it, nothing inside is still animating, and
the theme has not changed. A miss simply builds, so a subtree that never
qualifies is correct and costs one hash.

Two things make the difference between a cache that helps and one that only
helps in demos:

- **A pointer resting inside is not a reason to rebuild.** The widget under it
  is the same widget, so the pixels are the same pixels — and a pointer resting
  in a panel is what someone *reading* one looks like. On an 800-row panel that
  turns 0.427 ms into 0.062 ms, a case that previously never cached at all.
  Moving the pointer, or pressing a button, does rebuild.
- **A subtree that merely moved keeps its recording.** Its instances are
  translated and clipped afresh against whatever encloses them now, because the
  ancestors did not move just because it did. That needed the clip a subtree
  imposes on *itself* to be tracked separately from the clip imposed *on* it,
  and it needed recordings to stop losing instances to culling — a culled
  recording is only true where it was made, and a subtree sliding under a clip
  would show holes at the edge it left. Instances culled inside a recording are
  now kept aside and culled again, against the clip that is there, on every
  replay: the draw list stays exactly the size a build would have produced.

Widgets inside a replayed subtree do not run, so they cannot report anything.
Interaction still *works* — hit rects are replayed too, and the pointer arriving
invalidates the cache — but a `Response` from inside only comes back on a frame
that built. Cache the parts of your UI that are display, not the parts you read
answers from.

### Bring your own allocator

`#[global_allocator]` is the binary's choice, and libgui inherits it: the crate
keeps **no process-global state at all** — no statics, no `thread_local!`, no
`OnceLock`, no lazy initialisation — so every `Ui` is independent and allocates
through whatever you installed. That is a test (`boundaries.rs`), not a claim,
and `perf_alloc.rs` is the demonstration: it installs its own allocator and
counts every call libgui makes through it.

A per-instance `Allocator` (Rust's `allocator_api`) is *not* supported, and
cannot be while it is unstable. In practice what a real-time loop wants is not a
special allocator but no allocator at all during a frame, which is the property
above — and `Ui::reserve(widgets)` sizes the buffers up front so the *first*
frame gets it too:

```rust
let mut ui = Ui::new(theme, font)?;
ui.reserve(4_000);   // a container counts as a widget; overestimate freely
```

Measured on 4,000 widgets with no text: **109 allocations on the first frame
without it, 1 with it, 0 on every frame after either way.**

## Saving the workspace

A tool that forgets where its panels were is one people re-arrange every
morning. `dock.layout(&viewer)` snapshots the split tree, the fractions, which
tabs are in which pane and which is active, and the floating windows;
`dock.restore(&saved, ..)` puts it back.

```rust
// Saving, when the window closes:
std::fs::write(path, dock.layout(&viewer).to_toml())?;   // the app's I/O, not libgui's

// Loading, at startup:
let saved = DockLayout::from_toml(&std::fs::read_to_string(path)?)?;
let report = dock.restore(&saved, Tab::from_key)?;       // None for a panel this build lost
for key in report.missing_from(Tab::ALL.iter().map(|t| t.key())) {
    dock.add_tab(SurfaceId::MAIN, Tab::from_key(key).unwrap());   // a panel this build gained
}
```

- **Tabs are saved by `TabViewer::id`**, so that id must mean the same thing in
  the next version of your app: hash a fixed name (`"outliner"`), never use
  `self as u64`, which renumbers every panel after one inserted in the middle.
- **A layout is advice, not a command.** The app that loads it is rarely the one
  that saved it: a tab this build no longer has is dropped, along with the pane
  and the window it would have left empty; a tab the layout never mentioned is
  reported in `Restored::missing_from` rather than silently lost; and a layout
  written by a newer libgui is refused whole, so you can fall back to your
  default rather than half-apply it.
- **Files get hand-edited and copied between machines**, so nonsense in one is
  repaired rather than trusted: a NaN fraction, a zero-size window, an active
  index past the end of its stack.
- **The file names its panels.** Tab ids are opaque numbers, so each pane also
  records its tabs' titles — for whoever opens a customer's layout to work out
  what is where. They are written on save and ignored on restore.
- **libgui does no I/O.** It hands over a `DockLayout` of plain data; where that
  lives is the app's business. `to_toml`/`from_toml` come with the `theme-toml`
  feature (on by default); with `serde` alone, use any format you like.

`libgui_demo` does exactly this: the workspace is written to
`~/.libgui-demo-layout.toml` (or `$LIBGUI_LAYOUT`) on exit and restored at
startup, and `View ▸ Save layout now` writes it on demand.

## Golden images

Every scene in `crates/libgui_soft/tests/scenes/mod.rs` (widgets at rest, each
interactive state, raw primitives, text, a scroll area mid-scroll) is rendered
at 1x, 1.5x and 2x in the dark and light themes by `libgui_soft` and compared
with the PNGs in `crates/libgui_soft/tests/golden/`.

- **Deterministic.** The CPU renderer uses only IEEE-exact float operations,
  and libgui's clock is the sum of frame `dt`s, so a scene renders to the same
  bytes on any OS or CPU. The tolerance is one 8-bit step per channel.
- **Faithful.** `libgui_wgpu/tests/parity.rs` renders every scene through the
  real shader too, and fails if the two disagree. On Apple GPUs they are never
  more than one step apart. It skips itself where there is no GPU adapter.
- **Changing the look on purpose:**
  `LIBGUI_BLESS=1 cargo test -p libgui_soft --test golden`, then review the
  PNGs in the diff. On a mismatch, `<name>.actual.png` and `<name>.diff.png`
  (changed pixels in magenta) are written under `target/tmp/golden/`.
- **Adding a scene:** add it to `SCENES` and to the `goldens!` list; a test
  fails if a scene has no golden test.

Goldens check how things look, not how they move: motion and timing (scroll
lag, fling, easing) stay as frame-trace tests like
`trackpad_scroll_is_exact_and_offsets_land_on_pixels`.

## Roadmap (roughly in order)

1. ~~Text input~~ ✅ ~~IME preedit~~ ✅ ~~word wrap~~ ✅ single-line editing, `ui.paragraph` for
   wrapped text, `InputEvent::ImePreedit` for compositions; next: the multi-line *editor* (caret
   across lines, selection across lines), double-click word select, undo.
2. ~~Scroll areas~~ ✅ ~~virtualised lists, variable row heights, trees~~ ✅ `ui.virtual_list`, `ui.virtual_rows`, `ui.tree_row`; next: horizontal scroll, keyboard PageUp/Down, multi-select and drag-to-reparent.
3. ~~Keyboard/shortcut routing~~ ✅ ~~menus, popups/context menus, tooltips, z-order~~ ✅ `Layer`, `popup`, `menu_button`, `context_menu`, `tooltip`; next: checkable/icon menu items, keyboard navigation within a menu, "safe triangle" submenu tracking.
4. ~~Docking + tabs + splitters~~ ✅ Unity-style with OS-window tear-off; ~~layout save/load~~ ✅ `dock.layout()` / `dock.restore()`, versioned and repairing; next: tab close/context menu, maximize pane.
5. ~~Paths~~ ✅ `p.line` / `polyline` / `bezier` / `wire`, a `Line` primitive at `CONTRACT_VERSION` 2;
   next: stroked/filled arbitrary paths, dashes, arrowheads, and a real line/area plot (`plot` is
   still a debug bar chart).
6. ~~Horizontal and 2D scrolling~~ ✅ ~~general drag and drop~~ ✅ ~~tables/data grids with
   resizable and frozen columns~~ ✅ `ScrollOptions::both`, `drag_source` / `drop_zone` / `Payload`
   with a host seam for OS drags, `ui.table` with `TableState`; next: column reordering by drag,
   cell selection and keyboard navigation, and auto-scroll while dragging near an edge.
7. **Real text shaping**: replace `text.rs` internals with `cosmic-text`/`swash` or HarfBuzz
   (ligatures, bidi, font fallback, CJK), multi-page atlas with LRU eviction.
8. ~~Theme hot-reload~~ ✅ TOML themes, per-widget styles, density presets; next: multiple fonts (UI/mono/icons) in the theme, per-widget disabled states.
9. ~~Keyboard focus and navigation~~ ✅ `FocusKind` / `FocusPolicy`, activation through
   `UiAction`, a centrally drawn focus ring, per-platform policy in `libgui_keymap`; next:
   arrow-key navigation *within* lists, trees, tables and menus.
10. **Accessibility** via AccessKit: emit a node per interactive widget from the same tree — now
   unblocked, since it needs a focus order to describe.
11. ~~**Perf**: a "sleep when idle" mode instead of redrawing continuously~~ ✅
   `repaint_after` for hosts that draw only the UI, `Ui::needs_frame` +
   `Backend::render_batches` for hosts that redraw anyway, `ui.cached` for a
   *partly* changed UI (including one that merely moved, or has a pointer
   resting in it), and `--features profile` for knowing which of those to reach
   for; next: persistent GPU buffers, and a replay that survives its subtree
   being resized rather than only moved.

## Known scaffold limitations

- Restoring a layout does not restore keyboard focus or which pane was focused.
- Text fields are single-line and have no undo. Multi-line *editing* is not there yet: `paragraph`
  wraps read-only text, and `ImePreedit` shows a composition.
- No complex shaping and no font fallback. The bundled Inter has no CJK, Arabic or Indic glyphs, so
  those scripts render as blanks until a `FontRasterizer` that does both is plugged in — the
  wrapping already breaks them correctly, there is simply nothing to draw.
- Glyph atlas grows to 4096² when full (one re-rasterisation, once); past that it resets, with a
  one-frame flicker. Multi-page + LRU is still the real answer.
- Layouts nesting deeper than 256 are dropped and counted (`FrameCost::too_deep`), because the three
  recursive layout passes would otherwise overflow the stack. Any real layout is under fifty.
- Container ids are positional; give containers explicit keys once you add conditional UI.

Font: Inter (SIL Open Font License, see `assets/Inter-OFL.txt`).
