# libgui manual

How an application drives libgui: the loop, the host contract, multiple
windows, and what every public API expects of you.

Every code block here is compiled and run by `crates/libgui/tests/manual.rs`.
Four of them were wrong when first written, and only compiling them found it —
if you change one, change the test with it.

**This tracks a pre-1.0 library and will change.** Where something is not
implemented, or is the app's job rather than the library's, it says so — the
gaps are as much the point of this document as the features. `README.md` covers
the design rationale; this covers the wiring.

---

## Contents

1. [The shape of the thing](#1-the-shape-of-the-thing)
2. [The frame loop](#2-the-frame-loop)
3. [The host contract](#3-the-host-contract) — what you must provide
4. [Multiple windows](#4-multiple-windows)
5. [Building a UI](#5-building-a-ui)
6. [Identity, state and keys](#6-identity-state-and-keys)
7. [Text and fonts](#7-text-and-fonts)
8. [Keyboard, focus and navigation](#8-keyboard-focus-and-navigation)
9. [Rendering](#9-rendering)
10. [Theming](#10-theming)
11. [Persistence](#11-persistence)
12. [Testing your UI](#12-testing-your-ui)
13. [Features](#13-features)
14. [Limitations](#14-limitations)

---

## 1. The shape of the thing

libgui is a **UI core and nothing else**. It has no window, no GPU, no clock, no
filesystem, no threads and no network — a test (`tests/boundaries.rs`) fails the
build if any of those appear. Everything it cannot do itself, it asks you for.

You call widgets every frame; libgui keeps the state behind them keyed by stable
`Id`s, solves layout after the frame is built, and hands back a list of
instances to draw.

```
  your app                    libgui                    your host
 ──────────                 ──────────                 ───────────
  native events   ──push──▶  InputEvent
                             begin_frame(FrameInfo)  ◀── size, DPI, dt
  build widgets   ──────────▶ layout + paint
                             end_frame() ─▶ FrameOutput
                                              ├─ draw (instances, batches)
                                              ├─ atlas
                                              └─ platform ──▶ cursor, clipboard,
                                                              IME, repaint hint
```

Crates:

| Crate | What it is | Need it? |
|---|---|---|
| `libgui` | the core: widgets, layout, input, docking, text | yes |
| `libgui_shaders` | the one shader, in WGSL/HLSL/MSL/GLSL/SPIR-V | if you render |
| `libgui_wgpu` | a `Backend` for wgpu, and the reference for writing one | optional |
| `libgui_winit` | event translation for winit; a template for any host | optional |
| `libgui_keymap` | per-platform key bindings and focus policy | recommended |
| `libgui_soft` | CPU renderer, for golden-image tests | tests only |
| `libgui_nodes` | node-graph editing | optional |

`libgui` itself depends on `bytemuck`, plus `fontdue` by default. Nothing else.

---

## 2. The frame loop

```rust
use libgui::*;

let mut ui = Ui::new(Theme::dark(), font_bytes)?;   // once

// every frame:
ui.begin_frame(FrameInfo { screen_size, scale, dt });
my_app_ui(&mut ui);
let out = ui.end_frame();
renderer.render(&out, &mut pass);
apply_platform_output(&window, &out.platform);
```

`FrameInfo` is three numbers you must supply:

| Field | Meaning |
|---|---|
| `screen_size` | drawable size in **logical** px |
| `scale` | physical px per logical px (DPI) |
| `dt` | seconds since the previous frame; clamped to 0.25 internally, so a stall does not make animations jump |

Push input **before** `begin_frame`, or at any point during the frame — events
queue up and are consumed by the widgets that want them.

```rust
ui.push(InputEvent::PointerMoved { pos });
```

### Drawing only when something changed

```rust
if ui.needs_frame() { request_redraw(); }
```

`FrameOutput::platform.repaint_after` is the same answer as a duration: `None`
means "nothing is moving, sleep until an event", `Some(0.0)` means "keep going",
`Some(t)` means "wake me in `t` seconds" (a blinking caret). An app that
redraws continuously anyway — a game engine, a 3D tool — can ignore both.

---

## 3. The host contract

Everything in this section is **your** job. libgui will not do any of it, and
each one is a deliberate omission rather than a missing feature.

### 3.1 Input you must translate

Send `InputEvent`s. The full set:

| Event | Notes |
|---|---|
| `PointerMoved { pos }` | logical px, relative to this window's content |
| `PointerDelta { delta }` | raw, unaccelerated motion; only needed for pointer lock |
| `PointerLeft` | pointer left the window |
| `PointerButton { button, pressed }` | `Primary`, `Secondary`, `Middle`, `Back`, `Forward` |
| `Wheel { delta, unit }` | `unit` is `Pixel`, `Line` or `Page` — **do not convert**, libgui applies its own per-unit policy |
| `Touch { id, phase, pos }` | `Started` / `Moved` / `Ended` / `Cancelled` |
| `Key { key, pressed, repeat }` | physical key |
| `ModifiersChanged(Modifiers)` | **required**: modifier state does *not* come from `Key` events |
| `Text(String)` | committed text, already composed |
| `ImePreedit { text, cursor }` | in-progress composition |
| `Paste(String)` | your answer to `paste_requested` |
| `Action(UiAction)` | drive a widget with no keyboard at all — a gamepad, a foot pedal, an accessibility switch |
| `FocusLost` | the window lost focus |

`ModifiersChanged` catches people out. Pressing `Key::SuperLeft` does **not**
set `Modifiers::logo`; the host reports modifier state separately, because
that is how every windowing library reports it.

`libgui_winit::push_window_event` does all of this for winit and is ~200 lines
you can read and port.

### 3.2 Output you must apply

`FrameOutput::platform` is a set of requests:

| Field | What to do |
|---|---|
| `cursor` | set the window's cursor icon |
| `copied_text: Option<String>` | put it on the clipboard |
| `paste_requested: bool` | read the clipboard and send `InputEvent::Paste` |
| `text_input: Option<Rect>` | where the caret is: position the IME candidate window, or show the soft keyboard |
| `wants_pointer` / `wants_keyboard` | libgui used this input; do not also act on it |
| `pointer_lock: bool` | grab/release the pointer |
| `repaint_after: Option<f32>` | see above |

**The clipboard is a request, not an action.** libgui never touches it.

### 3.3 What you supply once

- **A font.** `Ui::new(theme, bytes)`. libgui bundles Inter for its own tests
  and demos; it will not go looking for system fonts, because that is
  filesystem access and platform policy.
- **Key bindings.** libgui binds *nothing*. Without them a text field does not
  respond to Backspace. Use `libgui_keymap`:
  ```rust
  libgui_keymap::Keymap::<MyAction>::for_current_platform().install(&mut ui);
  ```
- **A renderer.** See [§9](#9-rendering).

### 3.4 What libgui will never do

No windows, no file dialogs, no native menu bars, no notifications, no system
tray, no clipboard access, no filesystem, no networking, no threads. If your app
needs a macOS menu bar, that is AppKit and yours.

---

## 4. Multiple windows

This is the part with the most moving pieces, so it gets the most space.

### 4.1 The model

`DockState<T>` owns one **`Surface`** per window. `SurfaceId::MAIN` is your main
window; every other surface is a floating one that appeared because a user
dragged a tab out. Each surface holds a tree of `DockNode`s — splits with a
fraction, and leaves holding a stack of tabs. `T` is your own tab type.

**libgui never creates windows.** It describes the windows it *wants*; you make
reality match.

### 4.2 One `Ui` per window

Each window needs its own `Ui`, because a `Ui` owns a font atlas, input state
and per-widget retained state, all of which are per-window. `DockState` is
shared across all of them.

```rust
struct Win {
    window:   Arc<Window>,
    renderer: MyRenderer,
    ui:       Ui,            // one per window
    dock_id:  SurfaceId,     // which surface this window shows
}

struct App {
    dock: DockState<MyTab>,  // one, shared
    wins: HashMap<WindowId, Win>,
}
```

### 4.3 The five steps, every iteration

```rust
// 1. Where the pointer is on the *desktop*, in physical screen px, and whether
//    the primary button is down. A tab being dragged between windows is not
//    inside any one window's coordinate space, so this is global.
dock.set_pointer(screen_pos, primary_down);

// 2. Where each window is, so libgui can convert between the two spaces.
for w in wins.values() {
    dock.set_surface_frame(w.dock_id, content_origin(&w.window), w.window.scale_factor() as f32);
}

// 3. Run the drag state machine.
dock.update();

// 4. Make the OS match `dock.surfaces()`.  (See below.)

// 5. Draw each window.
for w in wins.values_mut() {
    w.ui.begin_frame(FrameInfo { /* this window's size/scale/dt */ });
    dock.show(&mut w.ui, w.dock_id, &mut my_viewer);
    let out = w.ui.end_frame();
    w.renderer.render(&out, &mut pass);
}
```

`content_origin` is the **inner** (client-area) top-left in physical screen px,
not the outer frame. Getting this wrong makes tabs drop in the wrong place by
exactly the title-bar height.

### 4.4 Step 4 in full

```rust
// Destroy windows whose surface is gone.
let alive: Vec<SurfaceId> = dock.surfaces().iter().map(|s| s.id).collect();
wins.retain(|_, w| alive.contains(&w.dock_id));

// Create windows for new floating surfaces.
for s in dock.surfaces() {
    if s.floating && s.visible && !have_window_for(s.id) {
        // `s.window_size` is the initial inner size, logical px.
        // `s.window_pos` is where to put it, physical screen px.
        let w = create_window(s.id, s.first_tab(), s.window_size, s.window_pos);
        // Render it immediately, so it appears with content rather than blank.
        render(w);
    }
}

// Apply position and visibility to the windows that exist.
for w in wins.values_mut() {
    let Some(s) = dock.surface(w.dock_id) else { continue };
    if let Some(p) = s.window_pos { w.window.set_outer_position(p - decoration_offset); }
    if s.visible != w.visible { w.window.set_visible(s.visible); }
}
```

**`Surface::visible` matters more than it looks.** A tab dragged out of its bar
tears off into a *hidden* floating surface immediately, so that a drop back into
another panel never has to build and tear down a window, a `Ui` and a renderer
between the drop and the frame that shows the result. Only a tab dragged onto
the desktop becomes visible. Skip the `visible` check and you will create and
destroy a real OS window on every tab drag.

`Surface::window_pos` is `Some` only *while* libgui is moving the window; apply
it every update while it is.

### 4.5 Single-window hosts

Not every host can make windows — a tablet, a console, an engine that owns its
swapchain. Set:

```rust
dock.config.floating_mode = FloatingMode::InApp;
```

Floating panels are then drawn inside the main window using `Surface::rect`, in
the main window's logical coordinates, and steps 1–4 collapse to nothing. The
same layout and the same tab code work either way.

### 4.6 Panels

```rust
impl TabViewer for MyViewer {
    type Tab = MyTab;
    fn title(&self, tab: &MyTab) -> String { ... }
    fn id(&self, tab: &MyTab) -> u64 { ... }        // stable identity
    fn ui(&mut self, ui: &mut Ui, tab: &mut MyTab) { ... }
    fn scroll(&self, tab: &MyTab) -> bool { true }  // false for a viewport
    fn padding(&self, tab: &MyTab) -> Insets { ... }
}
```

`id` is **saved into layout files**, so derive it from a fixed name
(`Id::from_name("inspector")`), never from an enum's discriminant or a position
in a `Vec`. Reorder your enum and every saved layout breaks otherwise.

### 4.7 Building a layout in code

```rust
let mut dock = DockState::new();
let left  = dock.leaf(vec![MyTab::Hierarchy]);
let right = dock.leaf(vec![MyTab::Scene, MyTab::Game]);
let root  = dock.split(Axis::X, 0.25, left, right);
dock.set_root(SurfaceId::MAIN, root);
```

### 4.8 The rest of `DockState`

| Method | Use |
|---|---|
| `add_tab(surface, tab)` | open a panel into an existing window |
| `take_root(surface)` / `set_root` | replace a whole layout |
| `surfaces()` / `surface(id)` | what windows should exist |
| `is_dragging()` / `drop_target()` | drive your own drag feedback |
| `cancel_drag()` | Escape during a drag |
| `close_surface(id)` | the user closed a window |
| `config` | everything about how docking *feels* — see `DockConfig` |

---

## 5. Building a UI

### 5.1 Containers

Every container takes a `Layout` and a `Frame` (fill, border, radius, shadow,
clip) and a closure:

```rust
ui.container_id(Id::new("sidebar"),
    Layout::column().width(Size::Fixed(240.0)).height(Size::Grow(1.0))
        .padding(Insets::all(8.0)).gap(6.0),
    Frame { fill: theme.palette.bg_panel, ..Frame::none() },
    |ui| { /* children */ });
```

`Size` is `Fixed(px)`, `Grow(weight)` or `Fit`; `Layout` carries the min/max,
padding, gap and alignment. Shorthands: `ui.row`, `ui.column`, `ui.panel`, `ui.card`.

**Every builder that takes a closure has an `open_`/`close_` pair**, because C
and other FFI callers have no closures:

```rust
ui.open_container(Id::new("sidebar"), layout, frame);
/* children */
ui.close_container();
```

Pairs exist for `container`, `scroll_area`, `popup_body`, `menu` and
`collection`. `ui.open_depth()` tells you if you have left one open.

### 5.2 Widgets

Text: `label`, `label_muted`, `heading`, `section`, `paragraph`, `text_with`.

Input: `button`, `button_primary`, `button_styled`, `toggle`, `checkbox`,
`radio`, `slider`, `slider_vertical`, `drag_value`, `drag_value_range`, `combo`,
`segmented`, `text_input`, `text_area`.

Collections: `selectable`, `tree_row`, `table`, `virtual_list`, `virtual_rows`.

Chrome: `menu_button`, `menu_item`, `menu_item_shortcut`, `menu_separator`,
`submenu`, `context_menu`, `tooltip`, `splitter`, `separator`, `progress`,
`plot`, `viewport`.

Every interactive widget returns a `Response`: `hovered`, `pressed`, `clicked`,
`double_clicked`, `focused`, `active`, `drag_delta`, `scroll`, `rect`, and more.

```rust
if ui.button("Save").clicked { save(); }
```

A `*_keyed` variant exists wherever labels repeat (list rows, tree nodes) —
use it, or two rows with the same text will share state.

### 5.3 Layers

Popups, menus and tooltips draw above everything and are laid out in their own
pass. `ui.popup`, `ui.layer`, `ui.overlay`, `ui.tooltip`, `ui.context_menu`.
`ui.any_popup_open()` tells you whether to suppress your own shortcuts.

### 5.4 Canvases

`ui.canvas` gives a pan/zoom transform for a node graph, a timeline, a
schematic. Widgets inside work in canvas coordinates at any zoom —
`Response::drag_delta` and `mouse_pos` are already converted.

### 5.5 Drag and drop

`DragSource`, `DropZone` and `Payload` handle in-app drags. Files dragged from
the OS arrive through the host (`libgui_winit::FileDrop` shows the shape).

### 5.6 Custom drawing

```rust
ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(40.0)), Vec2::ZERO, true,
    move |p: &mut Painter, r: Rect| {
        p.rect(r, color, 4.0);
        p.text_left(r, 13.0, ink, text);
    });
```

The closure runs **after layout**, so `r` is the final rect. Anything it
captures must be `'static` — resolve strings with `ui.frame_text` first.

---

## 6. Identity, state and keys

Widget state lives behind an `Id` derived from the container path plus a key.
Duplicates in one container are disambiguated by build order, which is fine
until the build order changes:

> **Container ids are positional by default.** The moment you add conditional
> UI (`if show_advanced { ... }`), give containers explicit keys, or state will
> jump between widgets when the condition flips.

```rust
ui.container_id(Id::new("advanced"), ...);   // explicit
ui.with_key(item.id, |ui| { ... });          // scope a whole subtree
```

`Id::from_name("...")` is a stable hash — same value across Rust releases,
32/64-bit and endianness — so it is safe to persist and to pass over FFI.

---

## 7. Text and fonts

### 7.1 Fonts are yours

```rust
let ui = Ui::new(theme, my_font_bytes)?;                       // one face
let ui = Ui::with_fallbacks(theme, &[ui_font, cjk, emoji])?;   // a chain
```

**Without a fallback chain, scripts your font lacks render as nothing.** The
bundled Inter has no CJK, Arabic, Indic or emoji glyphs. `FontStack` tries faces
in order and keeps a combining mark with its base. Line metrics come from the
*first* face, so adding a CJK fallback does not change the height of a line of
Latin.

Which fonts go in the chain is yours to decide — libgui does not read the
filesystem.

### 7.2 Real shaping

Off by default (`features = ["shape"]`). `ShapeRasterizer` runs the font's own
layout tables through rustybuzz: ligatures, GPOS kerning, mark attachment,
contextual forms, Indic reordering.

```rust
let face = ShapeRasterizer::from_bytes(bytes)?.with_language("tr")?;
let ui = Ui::with_rasterizer(theme, Box::new(face));
```

`with_language` matters for languages whose letterforms differ over the same
codepoints (Turkish dotted/dotless i, Serbian vs Russian Cyrillic italics).
libgui does not read your process locale; you pass it.

Costs ~10.5 µs per shaped string against fontdue's ~0.26 µs. Shaped runs are
cached per (font, size, string), so a settled frame shapes nothing.

### 7.3 Your own text engine

Implement `FontRasterizer` (three methods: `line_metrics`, `shape`,
`rasterize`) and register it with `Ui::with_rasterizer`. Shaping is by glyph id,
so HarfBuzz, CoreText or DirectWrite drop straight in.

### 7.4 Knobs

`Fonts::set_tab_width`, `Fonts::set_atlas_limit`, `Ui::undo_run_pause`.

---

## 8. Keyboard, focus and navigation

### 8.1 libgui binds no keys

Widgets respond to `UiAction`s — `Move`, `Delete`, `Copy`, `Submit`, `Cancel`,
`FocusNext`, `Navigate(Nav)` — and never to keys. What chord produces one is
policy, and policy is platform-specific, so it lives in `libgui_keymap` or in
your app.

```rust
libgui_keymap::Keymap::<MyAction>::for_current_platform().install(&mut ui);
```

This also installs a `FocusPolicy`: macOS visits text fields only until Full
Keyboard Access is on; Windows and Linux visit everything.

### 8.2 App shortcuts

```rust
if ui.consume_shortcut(Shortcut::plain(Key::S).logo()) { save(); }
```

`consume_shortcut` respects focus: a focused text field takes Cmd+Z for its own
undo first, and **releases** it when it has nothing left to undo, so your
document's undo gets the chord. Build panels first and globals last.

### 8.3 Navigating within a collection

Tab moves *between* widgets. To move *within* a list or tree:

```rust
let nav = ui.open_collection("hierarchy", rows.len());
if nav.moved { selected = nav.cursor; }
for (i, row) in rows.iter().enumerate() {
    let r = ui.selectable_keyed(i, row, selected == i);
    if r.clicked { selected = i; ui.set_cursor(nav.id, i); }
    if nav.moved && nav.cursor == i { ui.scroll_to(r.id); }
}
ui.close_collection();
```

The collection becomes **one** Tab stop instead of one per row, and the arrows
move the cursor inside it. `nav.expand` / `nav.collapse` report a tree's
Right/Left for you to act on — libgui does not know your tree's shape.

### 8.4 Scrolling focus into view

Focus does this for itself. Call `ui.scroll_to(id)` by hand for a cursor libgui
does not own: a collection's current row, a search hit, a selection made in code.

---

## 9. Rendering

### 9.1 Using wgpu

```rust
use libgui::Backend;   // `prepare` / `render` come from the trait

let mut renderer = libgui_wgpu::Renderer::new(&device, &queue, format);

// each frame:
renderer.prepare(&out);                 // upload globals, instances, atlas
{
    let mut pass = encoder.begin_render_pass(&descriptor);
    renderer.render(&mut pass, &out);   // one instanced draw per batch
}
```

If `ui.needs_frame()` said nothing changed and you skipped the UI frame, the
instance buffer and atlas are still the ones `prepare` uploaded — keep the last
`out.batches().to_vec()` and call `renderer.render_batches(&mut pass, &batches)`
instead. That is how a viewport keeps animating while the UI around it costs
nothing.

### 9.2 Writing your own backend

`FrameOutput` gives you `instances()` (POD, 96 bytes each), `batches()` (ranges
grouped by texture), `globals()` (16 bytes) and `atlas()` (single-channel
coverage). Implement `Backend`, or just read the four fields yourself.

1. One pipeline from `libgui_shaders` — instance-rate vertex buffer of six
   `float4`s, triangle list, premultiplied alpha, no depth.
2. Upload globals; copy instances; re-upload the atlas when `Atlas::version`
   changes.
3. Per batch: bind the texture, draw `6` vertices × N instances.

Everything exact — strides, attribute order, blend mode, texture formats,
whether the target is sRGB — is in `libgui::render_contract`, with a
`CONTRACT_VERSION` to check against.

### 9.3 Renderers without instancing

bgfx caps instance data at five `vec4`s and an instance needs six; GLES2 and
WebGL1 have no per-instance attributes at all. `libgui::mesh` expands a
`DrawList` into one quad per primitive instead — four vertices, six indices,
every value the fragment shader needs already computed. Costs about 4.5× the
bytes, which is the price of not having instancing.

### 9.4 Your own textures

`TextureId` is opaque. Register a texture with your backend, draw it with
`ui.viewport(...)` — that is how a 3D scene goes in a panel. It is composited as
**opaque sRGB**: alpha ignored, no colour conversion, so convert a linear or HDR
target before it gets here.

---

## 10. Theming

`Theme` is `Palette` (colours) + `Metrics` (sizes, from a `Density` preset) +
one style struct per widget. `Theme::dark()`, `Theme::midnight()`,
`Theme::light()`.

```rust
ui.with_style(|t| { t.button.radius = 0.0; }, |ui| { ui.button("Square"); });
```

With `theme-toml` (default) themes load from and export to TOML. With
`theme-watch` (opt-in) `ThemeWatcher` reloads a file as you edit it — the only
filesystem access in the crate, and the reason it is a separate feature.

---

## 11. Persistence

```rust
let layout = dock.layout(&viewer);              // needs the viewer for tab ids
let toml = layout.to_toml()?;                   // Result — see below

let layout = DockLayout::from_toml(&text)?;
let restored = dock.restore(&layout, |id| my_tab_for(id))?;
// Panels this version of the app has that the saved layout never mentioned:
for id in restored.missing_from(all_my_tab_ids()) {
    dock.add_tab(SurfaceId::MAIN, my_tab_for(id).unwrap());
}
```

Versioned
(`DockLayout::VERSION`), and repairing: a newer version is refused, a panel your
app no longer has is dropped (`Restored::dropped`), a panel the layout never saw
is reported by `Restored::missing_from` so you can place it, and malformed input
is repaired rather than trusted.

`to_toml` returns a `Result` deliberately: the usual next step is writing over
the file holding the last good layout, and an error must stop that.

**Restoring a layout does not restore keyboard focus** or which pane was
focused.

---

## 12. Testing your UI

libgui is testable without a GPU or a window.

```rust
let mut ui = Ui::new(Theme::dark(), FONT)?;
ui.begin_frame(FrameInfo::default());
my_ui(&mut ui);
let out = ui.end_frame();
assert_eq!(out.draw.instances.len(), 3);
```

Drive it with `ui.push(InputEvent::…)` exactly as a user would. `FrameCost`
(`ui.frame_cost()`) reports nodes, glyphs rasterised, strings shaped, text
scanned and unkeyed duplicates; `Budget` turns those into assertions.

The first frames of any UI measure start-up, not steady state — glyphs are
rasterised once, layout settles, hover animations begin at zero — so
`testing::steady_frame` runs four and reports the last:

```rust
use libgui::testing::{steady_frame, Budget};

let cost = steady_frame(&mut ui, FrameInfo::default(), |ui| my_ui(ui));
Budget::steady(120).instances(400).assert(&cost);
```

`glyphs_rasterized == 0` and `text_shaped == 0` in a settled frame mean your UI
is not thrashing its caches. `libgui_soft` renders to an image for golden tests.

---

## 13. Features

| Feature | Default | What it adds |
|---|---|---|
| `fontdue` | on | the built-in rasteriser |
| `theme-toml` | on | TOML themes and layouts (pure data) |
| `serde` | on¹ | serialising themes and layouts |
| `shape` | **off** | rustybuzz shaping (see §7.2) |
| `theme-watch` | off | `ThemeWatcher`; the only filesystem access |
| `profile` | off | per-phase frame timings; the only clock |

¹ pulled in by `theme-toml`.

With no features at all, the core builds with `bytemuck` as its only dependency.

---

## 14. Limitations

The honest list, as of now.

### Blocking for some apps

- **No accessibility.** No AccessKit, no screen-reader tree, no reduced-motion
  or high-contrast awareness. For consumer software this is a legal requirement
  in several jurisdictions, not a nice-to-have. It is the largest single gap.
- **No bidi.** `ShapeRasterizer` gives an RTL letter its correct joined form,
  but the run lays out left to right. Arabic and Hebrew are not usable.

### Widgets

- No colour picker. `plot` is a debug bar chart, not a real line/area chart.
- No modal/dialog primitive (build one on `ui.popup` / `Layer`).
- No date picker, no toast/notification.
- **Disabled state exists only on menu items.** A button, checkbox, slider or
  field cannot be greyed out.
- Tables have no 2-D cell cursor; menus have no arrow-key navigation; no
  type-ahead in lists.
- Trees have no multi-select and no drag-to-reparent.

### Text

- No word wrap inside `text_area` — long lines scroll sideways. `paragraph`
  wraps read-only text fine.
- No double-click word select.
- Caret motion moves by `char`, so it can split a grapheme cluster (an emoji
  with a modifier, a base plus a combining mark).
- `ImePreedit` displays in `text_input` but not in `text_area`.
- One font per `Ui`: no separate UI/mono/icon faces in the theme. `FontStack`
  covers a missing *script*, not a different *role*.
- Tab width is per-`Ui`, not per-document.
- `FontStack` splits runs on a range check over the combining blocks and
  joiners, not a Unicode general-category lookup, so an exotic mark outside
  those ranges can be separated from its base.

### Drawing

- No rotated images or glyphs.
- No stroked/filled arbitrary paths, dashes or arrowheads.
- The glyph atlas is one page. It grows to `set_atlas_limit` (4096 default);
  past that it resets, with a one-frame flicker. Multi-page with LRU eviction
  is the real answer and is not done.

### Structure

- Layouts nesting deeper than 256 are dropped and counted
  (`FrameCost::too_deep`). Any real layout is under fifty.
- Container ids are positional by default (see §6).
- Restoring a layout does not restore focus.
- A text field's undo notices the app rewriting the document by the length and
  a hash of up to 64 bytes each side of the edit. A rewrite preserving both
  would go unnoticed; the alternative is comparing the whole document, which is
  what storing edits instead of snapshots exists to avoid.

### Pre-1.0

The API will change. There is no changelog yet and no semver guarantee. Pin an
exact version.
