# libgui manual

How an application drives libgui: the loop, the host contract, multiple
windows, and what every public API expects of you.

Every code block here is compiled and run by `crates/libgui/tests/manual.rs`.
Four of them were wrong when first written, and only compiling them found it —
if you change one, change the test with it.

**This tracks a pre-1.0 library and will change.** Where something is not
implemented, or is the app's job rather than the library's, it says so — the
gaps are as much the point of this document as the features. [`DESIGN.md`](DESIGN.md) covers
*why* the library is shaped this way, decision by decision; this covers the
wiring.

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
14. [Calling from C or C++](#14-calling-from-c-or-c)
15. [Limitations](#15-limitations)

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
`segmented`, `text_input`, `text_area`, `validated_input`.

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

**Fields the app validates.** `validated_input` is a text field whose value
changes only when *your* validator accepts it — a dimension, an email address,
a colour in hex. libgui owns the behaviour; you own the question.

```rust
let r = ui.validated_input_with("height", &mut param.source, &ValidatedOptions {
    display: Some(&shown),        // "40 mm" while nobody edits; the source is "width * 2"
    ..Default::default()
}, |text| doc.check_expression(text).map(|_| ()).map_err(|e| FieldError::new(e.to_string())));
if r.committed { doc.reevaluate(); }
```

- Your string is the **source** and is written only on an accepted commit —
  Enter, Tab, a click elsewhere. A half-typed entry never reaches your model.
- Editing starts from the source, never the display, so a parametric link
  (`width * 2`) survives being edited.
- The validator is asked once, on commit, with the whole text. Its grammar,
  its names — resolved however and whenever you like — and its units are
  yours.
- Refused text stays in the field with the reason beneath it. A refused Enter
  keeps focus, and puts the caret at `FieldError::at` if you gave one; a
  refused click-away does not pull focus back. Escape throws the edit away.

libgui ships **no expression language**, deliberately: which functions exist,
what a bare number means, and whether the text or the number is the truth are
an application's decisions, and a second grammar that disagrees with the app's
would give users different answers in different boxes. An app with no grammar
of its own can use the **`libgui_units`** companion crate — an evaluator with
units (`25.4mm`, `3/8"`, `w/2 + 1cm`) and a `number_input` helper built on this
field — the way key bindings come from `libgui_keymap`.

### 5.3 Disabled widgets

A command-enablement model — a ribbon that greys out what does not apply to the
selection, a panel dead until something is picked — wraps a group rather than
passing a flag to every widget:

```rust
ui.enabled(has_selection, |ui| {
    if ui.button("Join").clicked { join(); }
    if ui.button("Subtract").clicked { subtract(); }
});
```

Inside, widgets are inert — no hover, no click, no keyboard focus — and
everything painted is multiplied by `theme.metrics.disabled_alpha`. That last
part happens in the draw list rather than in each widget, so **your own
`add_leaf` drawing greys out too**, without knowing it can.

Disabling nests one way: `ui.enabled(true, …)` inside a disabled scope does not
re-enable. `ui.is_enabled()` reads the current state; `open_enabled` /
`close_enabled` are the closure-free pair.

### 5.4 Multi-select

libgui does not hold your selection — a CAD browser selects bodies, a file list
selects paths. What it keeps is the **anchor** a Shift-click extends from,
which every app would otherwise reimplement:

```rust
let nav = ui.open_collection("model", bodies.len());
for (i, body) in bodies.iter().enumerate() {
    let r = ui.selectable_keyed(i, body, picked.contains(&i));
    if r.clicked {
        match ui.select(nav.id, i, keymap.select_kind(&r.modifiers)) {
            Selection::Only(i)   => { picked.clear(); picked.insert(i); }
            Selection::Toggle(i) => { if !picked.remove(&i) { picked.insert(i); } }
            Selection::Range(r)  => { picked.clear(); picked.extend(r); }
        }
    }
}
ui.close_collection();
```

`Keymap::select_kind` maps modifiers per platform — Command toggles on macOS,
Control elsewhere, Shift takes a range and wins when both are held. A plain
click and a toggle move the anchor; a range does **not**, so dragging a
Shift-click up and down grows and shrinks one range instead of ratcheting.

Keyboard range-extend (Shift+Arrow) is not implemented.

### 5.5 Layers

Popups, menus and tooltips draw above everything and are laid out in their own
pass. `ui.popup`, `ui.layer`, `ui.overlay`, `ui.tooltip`, `ui.context_menu`.
`ui.any_popup_open()` tells you whether to suppress your own shortcuts.

### 5.6 Canvases

`ui.canvas` gives a pan/zoom transform for a node graph, a timeline, a
schematic, a CAD sketch. Widgets inside work in canvas coordinates at any zoom
— `Response::drag_delta` and `mouse_pos` are already converted — so snapping,
hit tolerance and the rest of your logic is written once in model units and is
correct at every zoom. Text is rasterised at the zoomed size rather than scaled
up. `CanvasView::visible` is the cull rect.

One exception, worth knowing before you build a sketcher on it: the `Response`
the canvas *returns* is the background's, and it is produced before the
transform is pushed, so **its `mouse_pos` is in window coordinates** while
everything built inside reports canvas coordinates. Convert it with the view's
transform if you need where on the canvas the user clicked empty space.

`ui.with_transform` is the same thing without the input handling, for a view
you drive yourself. Both have `open_`/`close_` forms.

**Animation** is one call, not a system. `ui.animate(id, slot, target)` keeps a
value per `(id, slot)` and eases it towards `target` each frame — the rate is
frame-rate independent, so the motion is the same at 60 and 144 Hz and right
across a dropped frame. `animate_bool` is the hover/press form. libgui asks the
host for another frame until the value arrives, which is what
`PlatformOutput::repaint_after` reports; `ui.request_repaint()` asks for one
for a reason libgui cannot see. There are no keyframes, easing curves or
springs: everything in the library, including the dock's sliding tabs, is built
from this.

### 5.7 Drag and drop

`DragSource`, `DropZone` and `Payload` handle in-app drags. Files dragged from
the OS arrive through the host (`libgui_winit::FileDrop` shows the shape).

### 5.8 Custom drawing

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

## 14. Calling from C or C++

`libgui_c` is a separate crate producing a `staticlib` and a `cdylib`, with a
header at `crates/libgui_c/include/libgui.h`.

It is separate for two reasons. `crate-type` is fixed in the manifest and
cannot be switched on by a feature, so declaring `staticlib` on `libgui` would
build one for every Rust-only user forever. And the two make **different
promises**: libgui's Rust API is pre-1.0 and changes freely, while a C ABI is a
promise about bytes that a C++ host links against. Different promises need
different version numbers, and a crate has one.

```c
#include "libgui.h"

if (libgui_abi_version() != LIBGUI_ABI_VERSION) { /* rebuild one of them */ }

LibguiUi* ui = libgui_ui_new(font_bytes, font_len);

libgui_begin_frame(ui, w, h, scale, dt);
libgui_open_container(ui, libgui_id_from_name("panel"), layout, frame);
libgui_heading(ui, "Model");
libgui_checkbox(ui, "Visible", &visible);

uint8_t was = libgui_open_enabled(ui, has_selection);
if (libgui_button(ui, "Join").clicked) { join(); }
libgui_close_enabled(ui, was);

libgui_close_container(ui);
libgui_end_frame(ui);
```

**The rules at the boundary:**

- **Nothing panics across it.** Every entry point catches. A `Ui` that panicked
  is *poisoned*: further calls do nothing and `libgui_ui_poisoned` says so.
  Tear it down and build a fresh one.
- **No allocation crosses it.** Strings are `const char*` you own, NUL
  terminated, UTF-8. Nothing returned needs freeing.
- **Null is tolerated everywhere.** A null handle, label or out-parameter makes
  the call do nothing useful and records why in `libgui_last_error`. It does
  not crash.
- **Check the ABI version once at start-up.**

### 14.1 Putting your scene in it, and getting triangles out

Two things an engine needs that a form-filling toolkit does not.

**The 3D view.** `libgui_viewport(ui, key, texture)` takes an index you
register with your own renderer and fills what its container gives it. The
index comes back in `LibguiBatch::texture_index` with `texture_kind` 1, and
drawing it is yours — libgui never learns what a texture is. The response is
what a camera is driven from. It *grows*, so its container needs a height to
give: inside one whose height is `Fit` it is zero pixels tall and draws
nothing. `libgui_painter_image` is the same thing inside a custom leaf, for an
icon or a thumbnail.

**Triangles.** libgui's own output is one instance per primitive — 96 bytes,
six vertex attributes — and not every renderer can draw that: bgfx carries at
most five vec4s of instance data, GLES2 and WebGL1 have no per-instance
attributes at all, and plenty of engine RHIs expose a vertex and index draw
and nothing else. Call `libgui_enable_mesh(ui, 1)` and every frame is also
expanded into one quad per primitive:

```c
libgui_enable_mesh(ui, 1);
/* ... build and end the frame ... */
uint64_t nv = 0, ni = 0, nb = 0;
const void*        vertices = libgui_mesh_vertices(ui, &nv);
const uint32_t*    indices  = libgui_mesh_indices(ui, &ni);
const LibguiBatch* batches  = libgui_mesh_batches(ui, &nb);
/* batches[i].first/count are into the index buffer. */
```

The vertex is what the shader's *varyings* are, already computed, so porting
the shader is a vertex stage that moves `pos` into clip space and passes the
rest through. Describe your vertex layout with `libgui_vertex_stride` and
`libgui_vertex_attribute` rather than hard-coding offsets that could move, and
check `libgui_mesh_fits_u16` if your index buffers are 16-bit.

It costs about four and a half times the bytes, so it is off by default and a
renderer that can instance should keep instancing.

**If your renderer streams into a fixed per-frame buffer**, set a ceiling and
the frame is cut to fit it. bgfx's transient buffer is 6 MB by default — about
thirteen thousand quads — and a frame that goes over does not run slowly, it
loses the draw call, which a dense table or a node graph reaches sooner than
people expect.

```c
libgui_set_mesh_limits(ui, 65536, 98304);   /* 0 for either means no limit */
uint64_t n = 0;
const LibguiMeshChunk* chunks = libgui_mesh_chunks(ui, &n);
for (uint64_t i = 0; i < n; i++) {
    upload_vertices(vertices + chunks[i].vertex_first, chunks[i].vertex_count);
    upload_indices(indices + chunks[i].index_first, chunks[i].index_count);
    for (uint32_t b = 0; b < chunks[i].batch_count; b++) {
        const LibguiBatch* d = &batches[chunks[i].batch_first + b];
        draw(d->first, d->count);
    }
}
```

Indices count from their chunk's first vertex and a batch's range from its
chunk's first index, so the two slices upload and draw untouched. A vertex
limit of 65,536 or less also makes `libgui_mesh_fits_u16` true whatever the
frame contains. Cutting the frame up changes nothing anyone can see:
`mesh_parity.rs` renders every scene whole and in sixteen-quad chunks and
requires the images to be identical, not merely close.

### 14.2 Checking your renderer against the reference

A backend for an unusual RHI is ported by hand, and a hand port is *nearly*
right: corners a shade off, a shadow that clips, glyphs half a pixel up at 1.5x
DPI. Each is invisible until something puts it beside the reference, so the
reference ships.

`libgui_soft` renders a frame on the CPU by evaluating the same shader per
pixel with IEEE-exact operations, so it produces the same bytes on every
machine. `libgui_enable_reference_render(ui, 1)` renders every frame that way
as well, and `libgui_reference_pixels` hands back RGBA8, premultiplied,
top-left origin. `libgui_conformance_*` is a gallery covering every primitive —
borders, shadows, lines, glyphs, rounded images, clipping — so a failure names
the scene, which names the primitive. `crates/libgui_c/tests/smoke.c` runs the
loop.

What your renderer has to agree with is in the header, in one block, because
none of it is guessable: premultiplied output, sRGB-encoded straight-alpha
colours into a UNORM target, bilinear clamped sampling, a top-left origin, and
no depth, culling or scissor. So is the order within a frame — **build →
`libgui_end_frame` → size your targets → render your scene → draw the UI** —
because a widget's rect exists only after layout, and rendering a scene before
that sizes it from last frame's rect.

### 14.3 How it stays in step with the library

The widget surface is declared **once**, in `src/table.rs`, and the macro emits
the `extern "C"` functions, the header declarations and a symbol manifest from
that one input. **Adding a widget is one line**; it cannot be added to one
output and not the others.

`cargo test -p libgui_c` regenerates the header and fails if the committed one
is stale (`LIBGUI_WRITE_HEADER=1` updates it).

Each widget's doc comment is carried into the header above its declaration, so
`libgui.h` reads the way the Rust documentation does and neither can be updated
without the other. A widget with no doc comment fails the drift test, since a
declaration nobody explained is what the generated half used to be.

The hand-written part of the header — the types, the callback vtables, the
containers — is guarded differently, by `tests/smoke.c`: a C program compiled
against the committed header and linked to the real static library, which
`static_assert`s every struct against `libgui_sizeof_*`. Mirrors are where ABI
bugs hide, and that test prints things like:

```
FAIL: LibguiResponse is 88 bytes here and 96 in the library
```

### 14.4 C++

`libgui.hpp` sits on the same ABI and costs nothing at runtime. It exists
because the three tedious things in C are the three you do constantly:

```cpp
libgui::Ui ui(font.data(), font.size());
ui.install_default_keymap();

ui.begin_frame(w, h, scale, dt);
{
    auto _c = ui.container(libgui::id("panel"),
                           libgui::Layout::column().padding(8).gap(4),
                           libgui::Frame::none().fill(bg).radius(4).clip());
    ui.heading("Model");
    ui.checkbox("Visible", visible);        // bool&, not uint8_t*
    ui.text_input("name", name);            // std::string&, grown as needed

    { auto _e = ui.enabled(has_selection);
      if (ui.button("Join").clicked) join(); }

    ui.add_leaf(libgui::id("gizmo"), leaf, true,
                [&](LibguiPainter* p, LibguiRect r) { draw_gizmo(p, r); });
}
ui.end_frame();
```

- **RAII guards** close every `open_`/`close_` pair, including on an early
  return or a throw — which is exactly where a hand-written close is forgotten.
- **Lambdas as paint callbacks.** The lambda is copied to the heap and deleted
  through `drop_user` when libgui drops the closure, so it may capture freely.
- **Views** over the frame output: `ui.instances()`, `ui.batches()`,
  `ui.atlas()`.

C++17, header-only, and it adds nothing to the ABI — so `tests/smoke.cpp`
compiles it against the same static library the C test uses.

### 14.5 One atlas for every window

Each `Ui` carries a font system: the faces, the shaping caches and the glyph
atlas. A docked application gives every window its own `Ui`, so a panel torn
into a new window used to rasterise every glyph again and the host uploaded a
second copy of the same image — for a CJK interface, thousands of glyphs and
megabytes of texture per window.

`libgui_ui_new_sharing_fonts(other)` (Rust: `Ui::sharing_fonts`) makes a second
`Ui` that draws from the first one's font system. `libgui_frame_atlas` then
reports the same pointer and the same version for every window sharing it, so
the "has it changed?" check a host already does is all it needs to upload once.
Only the font system is shared: theme, input, focus and widget state stay the
window's own, and the handles can be freed in any order.

Windows on displays of different scales share it too — a glyph is keyed by the
size it is rasterised at, so 1x and 2x live in the same image without evicting
each other.

### 14.6 What crosses, and what does not yet

Widgets (label, heading, button, checkbox, toggle, slider, drag value,
progress, selectable, tree row, menu items, tooltip, context menu), text input
and text area over a caller-owned buffer, combo and segmented pickers,
containers, scroll areas, the disabled scope, the collection cursor,
multi-select, stable ids, custom painting, the full input set, the keymap, the
viewport and painter image calls, the pan/zoom canvas and raw transforms,
animation, validated fields (and the `libgui_units` evaluator), docking, tables, drag and drop, themes from
TOML, and the frame output — instances, batches, atlas, globals, platform
requests, the expanded mesh and its chunks, the reference renderer and the
conformance gallery, and a shared font system.

Docking and tables both take callbacks that build through the host's own
handle while libgui holds the borrow, which is a question about provenance that
no ordinary run can answer: both the sound and the unsound version draw the
same pixels. Miri is what tells them apart, so the dock panel, the table cell
and the paint callback are each driven from a Rust test that CI runs under it.

Popups and layers cross too. **Modals are built from layers, deliberately** —
libgui has none, because what a modal *blocks* (whether the menu bar still
works, whether Escape cancels, whether the 3D view keeps orbiting) is the app's
question, not a layout one. A layer covering the window with a translucent fill
is the scrim; a second at the dialog's rect is the dialog.

`libgui_ui_new_with_fallbacks` takes a chain. Without one, scripts the first
font lacks render as **nothing** — the bundled Inter has no CJK, Arabic, Indic
or emoji glyphs, so a name typed in Japanese comes out blank rather than as
boxes.

**Every exported function is called by a test**, checked by extracting the
symbols and grepping the test directory rather than by reading the list.

### 14.7 Drawing your own widgets, and drawing them less often

The whole painter crosses — `measure`, `hairline`, `snap_rect`, `shadow`,
`polyline`, `bezier`, `wire`, `chevron`, the four text placements, tinted
images — so a custom widget written in C++ has the same palette as one written
in Rust. `measure` is the one you cannot do without: everything else draws,
that is how you decide where.

`hairline` is worth knowing about for CAD specifically. A 1.0-wide rect on a
1.5× display lands on a pixel and a half and renders as a grey smear; `hairline`
gives you exactly N *physical* pixels, so rules, grid lines and witness lines
stay crisp at any scale.

A custom widget also needs to **move**, and to sit in a **coordinate space** of
its own. Both cross:

```c
LibguiCanvasState view;
libgui_canvas_state_default(&view);          /* once, kept across frames */

/* ... each frame ... */
LibguiCanvasView v;
LibguiResponse bg = libgui_open_canvas(ui, "sketch", &view, &v);
for (size_t i = 0; i < n; i++) {
    if (!overlaps(ent[i].bounds, v.visible)) continue;   /* cull */
    draw_entity(ui, &ent[i]);                            /* in model units */
}
libgui_close_canvas(ui);
if (bg.clicked) deselect_all();
```

Inside that, a widget's rect, `mouse_pos` and `drag_delta` are in **canvas**
coordinates, which is the whole reason to use it rather than a viewport and
your own arithmetic: the snapping is written once and is right at every zoom.
`bg` itself is the documented exception — window coordinates, as in §5.6 —
and `libgui_transform_inv_point` converts it.

`libgui_open_transform` is the raw form, for a camera you drive yourself.

A text field's buffer is yours, and a paste can outgrow it. `out_len` says
when; grow the buffer and fetch the rest with `libgui_text_overflow` and the
field's `response.id`. Do not call the field again instead — in the same frame
that is a second, unfocused widget, and the paste is lost. The C++ wrapper does
this for you.

`libgui_validated_input` takes the validator as a function pointer and a
`void*`, called synchronously on commit, so nothing is retained and nothing
needs freeing. It must not call libgui with the same handle: libgui is
mid-widget, and the call is refused — without that refusal it is a segfault.
In C++ it is a lambda, and an exception it throws is a refusal rather than an
unwind through libgui.

Motion is `libgui_animate_bool(ui, id, slot, on)` and friends, returning a 0..1
you blend with. Unlike the Rust calls these mark `id` alive for the frame
themselves: C has no other way to say an id exists, and the alternative failure
is an animation that silently resets every frame and never moves.
`libgui_request_repaint` asks for a frame for a reason libgui cannot see, and
`libgui_needs_frame` answers whether one is needed at all — the call a CAD host
spinning its viewport at 120 Hz makes before deciding to rebuild the UI.

Your widget also inherits things it does not ask for: the focus ring, keyboard
reachability (if you use `libgui_interact`), clipping, and — because the fade
happens in the draw list rather than per widget — **it greys out inside
`libgui_open_enabled(ui, 0)` without knowing that scope exists.**

#### Updating a section less often

```c
uint64_t tick = (uint64_t)(now * 10.0);          /* ten times a second */
if (libgui_open_cached(ui, "telemetry", tick)) {
    build_telemetry_panel(ui);
    libgui_close_cached(ui);
}
```

`libgui_open_cached` replays a subtree's recorded pixels instead of building
it: no layout, no text measurement, no draw-list work. Profiling puts paint at
about ninety per cent of a frame, so this is where a busy panel's cost actually
is. `deps` is any number that changes when the pixels would — a revision, a
hash, or a coarse tick as above, which is all "run this section at 10 Hz"
amounts to.

It refuses to replay when that would be wrong, and you manage none of it: the
pointer over it, focus inside it, still animating, the DPI or canvas transform
changed, the atlas repacked, or it moved while a pointer was inside it. A
replay survives the subtree **moving** but not **resizing**.

That is the second of three ways to do less work. The first is
`libgui_needs_frame`, which lets the whole window sleep. The third is redrawing
last frame's batches with no UI frame at all, which is how a viewport animates
while the UI around it costs nothing.

### 14.8 Building it from CMake

```cmake
add_subdirectory(third_party/libgui/crates/libgui_c)
target_link_libraries(vcad PRIVATE libgui::libgui)
```

That is the whole integration: the target carries the header directory, the
platform system libraries, and a rule that rebuilds the Rust when any source in
the workspace changes. The only prerequisite is `cargo` on PATH, which CMake
checks at configure time rather than failing later with a linker error about a
file that was never built.

`Debug` maps to cargo's `debug` profile and everything else to `release`, each
configuration with its own `IMPORTED_LOCATION_<CONFIG>` so Visual Studio and
Ninja Multi-Config work as well as a single-config generator. **A debug build
of libgui is an order of magnitude slower**; building it as `Release` inside a
Debug application is a reasonable thing to do.

No Corrosion dependency, because that would make every consumer vendor
Corrosion too — set `LIBGUI_USE_CORROSION=ON` if you already have it. A vcpkg
port is in `crates/libgui_c/packaging/vcpkg/` for consuming a *released*
libgui, which is the wrong choice while you are changing libgui alongside your
app. See `crates/libgui_c/packaging/README.md`.

## 15. Limitations

The honest list, as of now.

### Blocking for some apps

- **No accessibility.** No AccessKit, no screen-reader tree, no reduced-motion
  or high-contrast awareness. For consumer software this is a legal requirement
  in several jurisdictions, not a nice-to-have. It is the largest single gap.
- **No bidi.** `ShapeRasterizer` gives an RTL letter its correct joined form,
  but the run lays out left to right. Arabic and Hebrew are not usable.

### Widgets

[`WIDGETS.md`](WIDGETS.md) surveys fourteen applications against this list and
puts it in the order the gaps are worth closing.

- No colour picker. `plot` is a debug bar chart, not a real line/area chart.
- No modal/dialog primitive (build one on `ui.popup` / `Layer`).
- No date picker, no toast/notification.
- Tables have no 2-D cell cursor; menus have no arrow-key navigation; no
  type-ahead in lists.
- Trees have no drag-to-reparent. Multi-select works (§5.4) but only by
  pointer: Shift+Arrow does not extend a selection.
- The C API (§14) covers everything the Rust API does that a host needs,
  docking and tables included — both through function-pointer vtables. What it
  does not cross is what does not exist yet, which is the rest of this list.

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
