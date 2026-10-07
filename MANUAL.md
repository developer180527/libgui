# libgui manual

How libgui works, how to use it, and what each API is for.

- **Part I** explains the model in a few pages. Read it first.
- **Part II** shows how to use it, one task at a time.
- **Part III** lists the API by area, with Rust and C names side by side.

[`DESIGN.md`](DESIGN.md) explains *why* things are the way they are. This
manual sticks to *what* and *how*.

Every code block here is compiled by `crates/libgui/tests/manual.rs`. If you
change a block, change the test with it. libgui is pre-1.0, so the API still
changes.

---

## Contents

**Part I: How libgui works**
1. [The idea](#1-the-idea)
2. [The pieces](#2-the-pieces)

**Part II: Using it**

3. [A first window](#3-a-first-window)
4. [The host's job](#4-the-hosts-job)
5. [Building a UI](#5-building-a-ui)
6. [Lists, trees and selection](#6-lists-trees-and-selection)
7. [Popups, menus, notifications](#7-popups-menus-notifications)
8. [Your own widgets](#8-your-own-widgets)
9. [Motion](#9-motion)
10. [Keyboard and focus](#10-keyboard-and-focus)
11. [Text and fonts](#11-text-and-fonts)
12. [Docking and multiple windows](#12-docking-and-multiple-windows)
13. [Rendering](#13-rendering)
14. [Themes, layouts, tests, features](#14-themes-layouts-tests-features)
15. [From C and C++](#15-from-c-and-c)

**Part III: Reference**

16. [API by area](#16-api-by-area)
17. [Limitations](#17-limitations)

---

# Part I: How libgui works

## 1. The idea

libgui is a **UI core**. It has no window, GPU, clock, filesystem, threads or
network; a test fails the build if any of these creep in. It does three things:

1. **Takes input.** Your host turns OS events into `InputEvent`s and pushes them.
2. **Builds the UI.** Your code calls widgets every frame (*immediate mode*).
   libgui lays them out and remembers their state between frames.
3. **Hands back the result.** You get a list of rectangles to draw (one GPU
   instance each), a glyph atlas, and requests for the host: which cursor to
   show, what to copy to the clipboard, when to wake up next.

```
  OS events ─push─▶ Ui ◀─ begin_frame(size, DPI, dt)
                    │
  your code ───────▶│  widgets → layout → paint
                    │
                    └─ end_frame() ─▶ FrameOutput
                                       ├─ draw: instances + batches ─▶ your renderer
                                       ├─ atlas: glyph texture
                                       └─ platform: cursor, clipboard, IME, repaint_after
```

Everything that depends on the platform comes from you or from a companion
crate:

| Crate | What it is |
|---|---|
| `libgui` | the core: layout, input, widgets, docking, text |
| `libgui_keymap` | key bindings and focus rules for each platform (libgui itself binds no keys) |
| `libgui_shaders` | the one shader, in WGSL / HLSL / MSL / GLSL / SPIR-V |
| `libgui_wgpu` | a wgpu renderer, and the model for writing your own |
| `libgui_winit` | turns winit events into `InputEvent`s; a template for other hosts |
| `libgui_soft` | CPU renderer, for pixel-exact tests |
| `libgui_units` | expression evaluator with units, for number fields |
| `libgui_nodes` | node-graph editing |
| `libgui_c` | the C ABI and the C++ header |

## 2. The pieces

Every widget, built-in or yours, is made of these eight pieces.

**`Ui` and the frame.** One `Ui` per window. Each frame runs
`begin_frame` → your calls → `end_frame`. Nothing is drawn while you build the
frame; the real drawing happens after layout.

**`Id`: identity.** Each widget has an `Id`, made from its parent container's
`Id` plus a key: the label, or one you give. libgui stores what it remembers
between frames under that `Id`: hover, focus, drag, scroll offset, animation.
It discards the state of any `Id` that is not built in a frame. Your *data*
(the checkbox's `bool`, the slider's `f32`) stays in your variables; libgui
only borrows it.

**Layout: containers and leaves.** The UI is a tree. A *container* (row or
column) arranges its children. A *leaf* takes up space and paints. Sizes are
`Fixed(px)`, `Grow(weight)` or `Fit`. Layout is solved once the whole frame has
been built, and every edge lands on a whole physical pixel.

**`Painter`: the drawing primitives.** A leaf paints through a closure that
runs after layout, with its final rectangle. The available calls:
- shapes: `rect`, `rect_bordered`, `shadow`, `gradient`;
- lines: `line`, `polyline`, `dashed_line`, `dashed_polyline`, `bezier`,
  `wire`, `chevron`, `hairline`;
- text: `text`, `text_left`, `text_centered`, `text_right`, `text_wrapped`,
  `text_rotated`, plus `measure` to size text before drawing it;
- vector paths: `fill_path`;
- images: `image`, `image_uv`, `image_tinted`, `image_with_alpha`.

Each call becomes one or two 96-byte instances.

**`Response`: interaction.** `ui.interact(id)` hit-tests the leaf with that
`Id`, using its rectangle from the previous frame. It reports `hovered`,
`pressed`, `clicked`, `double_clicked`, `active` (held), `released`,
`drag_delta`, `scroll`, `mouse_pos`, `modifiers` and `focused`. Every built-in
widget returns one of these.

**Actions, not keys.** Widgets react to `UiAction`s such as `Move`, `Delete`,
`Copy`, `Submit`, `Cancel`, `FocusNext`, `Navigate` and `Activate`, never to
raw keys. A *keymap* (from `libgui_keymap`, or your own) turns key chords into
actions, so the platform's conventions stay outside the core.

**Layers.** Popups, menus, tooltips and notifications are built into separate
layers that are laid out and drawn above the window. While a popup is open, it
gets the keyboard.

**Output.** `end_frame` returns:
- the draw list: instances grouped into batches by texture;
- the glyph atlas;
- `PlatformOutput`, with `repaint_after`. This is `None` when nothing is moving,
  so the host can sleep until the next event.

**Widgets are compositions of these pieces.** For example, a button is an
`Id`, an `interact`, an `animate_bool` for its hover fade, and a leaf that
paints a rectangle and some text. The built-in widgets use the same public
calls you do, so a widget you write behaves like a built-in one: it greys out
when disabled, can take keyboard focus, and works inside scroll areas and
zoomed canvases.

---

# Part II: Using it

## 3. A first window

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

| `FrameInfo` | |
|---|---|
| `screen_size` | drawable size in **logical** px |
| `scale` | physical px per logical px (DPI) |
| `dt` | the **real** seconds since the last frame, however long the host slept. Animations step by at most 0.25 s, so a stall does not make them jump. Timing (double-click, type-ahead, how long a notification stays) uses the full value. If you clamp, clamp only your own scene's step |

Push input before `begin_frame`, or at any time; events queue up:

```rust
ui.push(InputEvent::PointerMoved { pos });
```

**Draw only when needed.** `out.platform.repaint_after` is `None` when you can
sleep until the next event, `Some(0.0)` to keep drawing, and `Some(t)` to wake
in `t` seconds (a blinking caret, a notification about to leave). A host that
redraws all the time anyway (an engine, a 3-D viewport) asks
`ui.needs_frame(elapsed)` before each frame and skips the UI work when it
returns false. Something of yours that falls due later asks for a frame with
`ui.request_repaint_in(seconds)`.

## 4. The host's job

libgui deliberately leaves all of this to you.

**Input to translate.** Translate each OS event into one of these:

| Event | Notes |
|---|---|
| `PointerMoved { pos }` | logical px, relative to this window's content |
| `PointerDelta { delta }` | raw motion; only needed for pointer lock |
| `PointerLeft` | the pointer left the window |
| `PointerButton { button, pressed }` | `Primary`, `Secondary`, `Middle`, `Back`, `Forward` |
| `Wheel { delta, unit }` | `Pixel`, `Line` or `Page`. **Do not convert**: libgui applies its own rule per unit |
| `Touch { id, phase, pos }` | `Started` / `Moved` / `Ended` / `Cancelled` |
| `Key { key, pressed, repeat }` | physical key |
| `ModifiersChanged(Modifiers)` | **required**: modifier state does not come from `Key` events |
| `Text(String)` / `ImePreedit { text, cursor }` | committed text / text the input method is still composing |
| `Paste(String)` | your answer to `paste_requested` |
| `Action(UiAction)` | drive the UI without a keyboard: a gamepad, a switch |
| `FocusLost` | the window lost focus |

`libgui_winit::push_window_event` does all of this for winit in about 200
lines you can read and port.

**Output to apply** (`out.platform`):
- `cursor`: set the window's cursor;
- `copied_text`: put it on the clipboard;
- `paste_requested`: read the clipboard and send `Paste`;
- `text_input`: where the caret is, for the IME candidate window or the soft
  keyboard;
- `wants_pointer` / `wants_keyboard`: libgui used this input, so your app should
  not also act on it;
- `pointer_lock`: grab or release the pointer;
- `repaint_after`: see above.

**Supply once.**
- **A font**, as bytes: `Ui::new(theme, bytes)`.
- **Key bindings**:
  `libgui_keymap::Keymap::<MyAction>::for_current_platform().install(&mut ui)`.
  Without them, Backspace does nothing in a text field.
- **A renderer** (§13).

**Never in libgui.** Windows, file dialogs, native menu bars, the system tray,
the clipboard itself, the filesystem, networking, threads.

## 5. Building a UI

### Containers

```rust
ui.container_id(Id::new("sidebar"),
    Layout::column().width(Size::Fixed(240.0)).height(Size::Grow(1.0))
        .padding(Insets::all(8.0)).gap(6.0),
    Frame { fill: theme.palette.bg_panel, ..Frame::none() },
    |ui| { /* children */ });
```

A `Layout` holds the axis, sizes, min/max, padding, gap and alignment. A
`Frame` holds the fill, border, radius, shadow and clip. Shorthands: `ui.row`,
`ui.column`, `ui.panel`, `ui.card`, `ui.scroll_area`.

**Everything that takes a closure also has an `open_` / `close_` pair**, for C
and for code that cannot pass closures:

```rust
ui.open_container(Id::new("sidebar"), layout, frame);
/* children */
ui.close_container();
```

`ui.open_depth()` tells you whether something was left open.

### Widgets

Every interactive widget returns a `Response`:

```rust
if ui.button("Save").clicked { save(); }
```

- **Text:** `label`, `label_muted`, `heading`, `section`, `paragraph`,
  `text_with`.
- **Input:** `button` (`_primary`, `_styled`), `toggle`, `checkbox`, `radio`,
  `slider` (`_vertical`), `drag_value` (`_range`), `combo`, `segmented`,
  `text_input`, `text_area`, `validated_input`, `color_picker`,
  `color_button`.
- **Collections:** `selectable`, `tree_row`, `tree_view`, `table`,
  `virtual_list`, `virtual_rows`.
- **Chrome:** `menu_button`, `menu_item`, `submenu`, `context_menu`, `tooltip`,
  `splitter`, `separator`, `progress`, `plot`, `viewport`, `show_toasts`.

**Repeated labels.** Rows that can share a label need a `*_keyed` variant,
otherwise the rows share their state.

### Identity

The `Id` of a container is **positional** by default. Once part of the UI is
conditional (`if show_advanced { … }`), give containers explicit `Id`s, or
their state moves to the wrong widget when the condition changes.

```rust
ui.container_id(Id::new("advanced"), ...);   // explicit
ui.with_key(item.id, |ui| { ... });          // scope a whole subtree
```

`Id::from_name("…")` gives the same value on every platform and across
releases, so it is safe to save to disk and to pass over FFI.

### Disabled groups

```rust
ui.enabled(has_selection, |ui| {
    if ui.button("Join").clicked { join(); }
    if ui.button("Subtract").clicked { subtract(); }
});
```

Widgets inside are inert and drawn faded, and so is your own painting. An
`enabled(true, …)` nested inside a disabled group does **not** re-enable.

### Fields your app validates

`validated_input` changes your value only when *your* validator accepts it.
Use it for a dimension, an expression or a hex colour.

```rust
let r = ui.validated_input_with("height", &mut param.source, &ValidatedOptions {
    display: Some(&shown),        // "40 mm" while nobody edits; the source is "width * 2"
    ..Default::default()
}, |text| doc.check_expression(text).map(|_| ()).map_err(|e| FieldError::new(e.to_string())));
if r.committed { doc.reevaluate(); }
```

- **Commit:** your string is written only when you accept the text, on Enter,
  Tab or a click elsewhere.
- **Refused text:** it stays in the field with the reason shown under it.
  Escape throws the edit away.
- **Grammar:** libgui has no expression language; the grammar is yours. If you
  don't have one, `libgui_units` adds `25.4mm`, `w/2 + 1cm` and a
  `number_input`.

### Colour

`ui.color_picker(key, &mut color)` shows a saturation/value square, a hue
strip, an alpha strip and a hex field. `ui.color_button` is a swatch that opens
a picker in a popup. On the response, `changed` is set on every frame of a
drag; `finished` is set once per gesture, which is when to push an undo step.

## 6. Lists, trees and selection

### Keyboard cursor

A **collection** makes a list one Tab stop and lets the arrow keys move a
cursor inside it:

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

- **Tree keys:** `nav.expand` and `nav.collapse` report Right and Left.
- **Shift+arrows:** `nav.extend` is set when the move should extend the
  selection.
- **Type-ahead:** `ui.type_ahead(&mut nav, len, |i| label)` jumps to the row
  whose label starts with what was typed.

### Selection

The selection itself stays yours. libgui keeps the **anchor** that a
Shift-click extends from:

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

### Long lists

**`virtual_list`** builds only the rows on screen, so a million rows cost what
a screenful does. Every row must be the same height; `virtual_rows` handles
rows of different heights. Without a closure:

```rust
for i in ui.open_virtual_list("objects", names.len(), ListOptions::new(24.0)) {
    ui.open_virtual_row(i);
    ui.label(&names[i]);
    ui.close_virtual_row();
}
ui.close_virtual_list();
```

`ListOptions { reveal: Some(i), .. }` scrolls row `i` into view whether or
not it was built.

### Trees

**`tree_view`** is a tree that builds only the visible rows. You describe the
tree; libgui never stores its shape:

```rust
impl TreeSource for Assembly {
    type Key = PartId;
    fn roots(&self, out: &mut Vec<PartId>) { out.extend(&self.top) }
    fn children(&self, n: PartId, out: &mut Vec<PartId>) { out.extend(&self[n].kids) }
    fn has_children(&self, n: PartId) -> bool { !self[n].kids.is_empty() }
    fn label(&self, n: PartId) -> Cow<'_, str> { self[n].name.as_str().into() }
    fn selected(&self, n: PartId) -> bool { self.picked.contains(&n) }
}

let r = ui.tree_view("assembly", &mut tree_state, &model);
```

- **Loading on demand:** `children` is asked only for expanded nodes, so they
  can load lazily; `r.expanded` says which node just opened.
- **Revealing a node:** `tree_state.reveal(key, parent)` opens its ancestors
  and scrolls to it, for example to show a part picked in the 3-D view.
- **Selection:** `tree_state.select(key, kind)` applies selection by key.
- **Your own rows:** use `tree_state.rows()` with `virtual_list` and
  `tree_row`.

**`table`** has frozen columns, column resizing and virtualised rows; see
`TableState`.

## 7. Popups, menus, notifications

**Popups and layers.**
- Building: `ui.popup`, `ui.layer`, `ui.tooltip`, `ui.context_menu`,
  `ui.menu_button` / `menu_item` / `submenu`.
- Keyboard: menus work from the keyboard without extra code.
- Shortcuts: `ui.any_popup_open()` tells your global shortcuts to wait.

**Modals.** libgui has no modal type. What a modal blocks is your app's
decision, so build one from two layers: a translucent layer over the whole
window, then the dialog's own layer.

**Notifications.**

```rust
// Anywhere — mid-frame, an event handler, between frames:
let undo = ui.toast(Toast::info("Deleted 3 parts").action("Undo"));
ui.toast(Toast::success("Saved bracket_v3.step"));
ui.toast(Toast::error("Export failed: disk full")); // stays until closed

// Once a frame, last, so the stack sits above everything:
let r = ui.show_toasts();            // or show_toasts_with(ToastOptions { corner, .. })
if r.action == Some(undo) { restore(); }
```

- **Timing:** they leave after four seconds; errors stay until closed.
- **Too many:** past `max_visible` (4) the rest wait, and their clocks start
  only once they show.
- **Hover:** the pointer resting on one holds it.
- **No cost while waiting:** the window sleeps until the next one is due,
  which is why `dt` must be real.

**Drag and drop.**
- Inside the app: `drag_source` and `drop_zone` carry a `Payload`, and
  `insertion_line` marks where a drop will land.
- Files from the OS: the host calls `begin_external_drag` /
  `end_external_drag`.

## 8. Your own widgets

### The five pieces

1. **Identity:** `ui.make_id(("my_widget", key))`.
2. **Interaction:** `ui.interact(id)` for clicks. Use `interact_drag` to drag,
   or `interact_focusable(_drag)(id, FocusKind::Control)` to also be a Tab stop
   (draw a focus ring when `focused`).
3. **State:** the value lives in your variable; motion comes from
   `animate_bool` or a spring.
4. **Layout:** `ui.add_leaf(id, Layout::leaf(w, h), intrinsic, interactive,
   paint)`.
5. **Paint:** the closure runs after layout with the final rect, using the
   Painter calls from §2.

```rust
let id = ui.make_id(("swatch", key));
let r = ui.interact_focusable_drag(id, FocusKind::Control);
if r.active { *value = pick(r.mouse_pos, r.rect); }
let hot = ui.animate_bool(id, 0, r.hovered);
let c = *value;
ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(24.0)), Vec2::ZERO, true,
    move |p, rect| p.rect_bordered(rect, c, 4.0, 1.0 + hot, p.theme.palette.border));
```

### Paint closures

The closure must be `'static`. Resolve text you want to draw with
`ui.frame_text` first.

```rust
ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Fixed(40.0)), Vec2::ZERO, true,
    move |p: &mut Painter, r: Rect| {
        p.rect(r, color, 4.0);
        p.text_left(r, 13.0, ink, text);
    });
```

### Icons

**Drawn icons** are a `Path`, rasterised once per size into the atlas and drawn
like text: sharp at every DPI and tinted by the colour you pass.

```rust
let play = Rc::new(Path::new(24.0, 24.0)
    .move_to(Vec2::new(6.0, 4.0)).line_to(Vec2::new(20.0, 12.0))
    .line_to(Vec2::new(6.0, 20.0)).close());
let icon = play.clone();
ui.add_leaf(id, Layout::leaf(Size::Fixed(16.0), Size::Fixed(16.0)), Vec2::ZERO, true,
    move |p, r| p.fill_path(&icon, r, ink));
```

**Loaded icons** are your own texture, drawn with
`p.image_with_alpha(…, ImageAlpha::Straight)`. If your renderer filters in
hardware, premultiply the texture when you upload it, or edges get dark halos.

### Gradients and crisp lines

- `p.gradient(r, from, to, Axis::X)` draws a gradient as two instances, with
  nothing new needed in the renderer.
- `p.hairline` and `p.snap_rect` give lines exactly N **physical** pixels wide,
  so rules and grid lines stay sharp at 1.5× scale.

### Dashed lines

```rust
p.dashed_line(a, b, 1.0, ink, Dash::even(4.0));                  // construction line
p.dashed_line(a, b, 2.0, ink, Dash::dotted(2.0));                // dotted
p.dashed_polyline(&outline, 1.0, ink, Dash::new(6.0, 3.0));      // hidden edge
p.dashed_line(a, b, 1.0, ink, Dash::even(4.0).phase(t * 20.0));  // marching ants
```

- **Units:** `on` and `off` are in the same units as the width, so they scale
  with a canvas's zoom.
- **Polylines:** the pattern runs on across the joins rather than restarting
  at each point; `dashed_polyline` returns the phase it ended at.
- **Cost:** one instance per segment, like a solid line; the shader cuts the
  gaps. Dash ends are square; the line's own two ends keep their round caps.

### Turned images and text

```rust
p.image_rotated(r, knob_tex, [0.0, 0.0, 1.0, 1.0], 0.0, Color::WHITE, ImageAlpha::Straight, angle);
p.text_rotated(axis_mid, 12.0, ink, "Height (mm)", -FRAC_PI_2);   // reads bottom to top
p.text_rotated((a + b) * 0.5, 11.0, ink, "42.0 mm", angle_of(a, b)); // along a dimension line
```

- **Direction:** angles are radians, clockwise on screen. Images turn about
  the centre of `r`; text is centred on the point you give.
- **Drawing only:** these are drawing calls. Layout and hit-testing stay
  upright, so a turned knob is still interacted with through its upright
  rect.
- **Sharpness:** upright text is snapped to the pixel grid. Turned text cannot
  be, so it is resampled and is a shade softer, which reads well for labels.
- **Edges:** a turned image's edges are anti-aliased.

### Canvases

`ui.canvas(key, &mut state, |ui, view| …)` pans and zooms, for a node graph, a
timeline or a CAD sketch.
- **Coordinates:** widgets inside report rects, `mouse_pos` and `drag_delta` in
  **canvas** coordinates, so your snapping logic is written once.
- **Culling:** `view.visible` is the rectangle to cull against.
- **The background response:** the response the canvas itself returns is the
  one exception; it is in window coordinates.
- **Without input handling:** `ui.with_transform` is the same transform with no
  pan or zoom input.

### Caching

`ui.cached(key, deps, |ui| …)` replays a section's recorded pixels instead of
building it again, until `deps` changes. libgui refuses to replay whenever a
replay would be wrong, for example while the pointer is over the section.

### Example

`crates/libgui/src/color_picker.rs` is a complete widget built only from these
pieces.

## 9. Motion

**Easing.** `ui.animate(id, slot, target)` eases a value towards `target`; the
motion is the same at any frame rate. `animate_bool` is the 0-to-1 form, for
hover and press.

**Springs.** A spring carries velocity, so when the target changes halfway, the
motion curves towards the new target instead of reversing on the spot:

```rust
// The theme's spring (critically damped: quick, and then still).
let open = ui.animate_spring(id, 0, if drawer_open { 1.0 } else { 0.0 });

// Or your own: `response` is roughly how long it takes, in seconds;
// `damping` 1 is no overshoot, below 1 bounces.
let x = ui.animate_spring_with(id, 1, target_x, Spring::new(0.4, 0.6));
```

**Throwing.** When a drag ends, hand the spring the pointer's velocity:

```rust
if r.released {
    ui.set_spring(id, 0, x, ui.pointer_velocity().x);   // momentum handed over
} else if r.active {
    ui.set_spring(id, 0, x, 0.0);                       // held where the pointer is
}
```

**Reduced motion.** `Metrics::reduced_motion` makes every spring arrive at
once; pass on the OS setting.

**Frames.** libgui asks for frames while anything is moving and lets the window
sleep once everything rests. `ui.request_repaint()` asks for a frame for a
reason libgui cannot see.

## 10. Keyboard and focus

**libgui binds no keys.** A keymap turns chords into `UiAction`s and sets the
platform's focus rules. On macOS, Tab visits only text fields unless Full
Keyboard Access is on.

```rust
libgui_keymap::Keymap::<MyAction>::for_current_platform().install(&mut ui);
```

**App shortcuts.** A focused text field gets the first chance at a chord such
as Cmd+Z. It passes the chord on when it has nothing left to undo, so build
panels first and global shortcuts last.

```rust
if ui.consume_shortcut(Shortcut::plain(Key::S).logo()) { save(); }
```

**Activate.** `Activate` presses whatever has focus:
- Space presses it everywhere.
- Enter presses it too on Windows and Linux.
- On a Mac, Enter belongs to the default button.

**Menus.**
- The arrow keys move the highlight; Right opens a submenu and Left closes it.
- Escape backs out one level at a time.
- While a popup is open, it gets the keys.

**Adjustable controls.** Sliders, drag values and the colour picker take the
arrow keys when focused:
- arrows step by one, Page Up / Page Down by ten;
- Home and End jump to the ends;
- Shift makes each step ten times larger.

**Focus in code.** `ui.focused()` and `ui.set_focus(id)` read and set focus;
`ui.scroll_to(id)` brings a widget into view.

**Raw input.** For a custom widget, `ui.key_pressed`, `key_down`,
`button_down`, `button_pressed` and `hover_time` read the input directly.

## 11. Text and fonts

**Fonts are yours.** libgui never reads system fonts.

```rust
let ui = Ui::new(theme, my_font_bytes)?;                       // one face
let ui = Ui::with_fallbacks(theme, &[ui_font, cjk, emoji])?;   // a chain
```

Text in a script the font lacks renders as **nothing**. The bundled Inter has
no CJK, Arabic, Indic or emoji, so add a fallback chain if you need them.

**Real shaping** (ligatures, kerning, Indic reordering) is the `shape` feature:

```rust
let face = ShapeRasterizer::from_bytes(bytes)?.with_language("tr")?;
let ui = Ui::with_rasterizer(theme, Box::new(face));
```

**Your own engine.** To use another text engine (HarfBuzz, CoreText,
DirectWrite), implement `FontRasterizer` (three methods).

**Sharing fonts between windows.** `Ui::sharing_fonts(&other)` makes windows
share one font system and atlas, so glyphs are rasterised and uploaded once.

## 12. Docking and multiple windows

### The model

`DockState<T>` holds one **surface** per window:
- `SurfaceId::MAIN` is your main window;
- every other surface is a window that appeared because a tab was dragged out.

Each surface holds a tree of splits and tab stacks; `T` is your tab type.
**libgui never creates windows.** It describes the windows it wants, and your
host makes them exist.

Each window needs its own `Ui`; all of them share the one `DockState`.

### Every iteration

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

`content_origin` is the **inner** (client-area) top-left in physical screen
pixels. If you use the outer frame instead, tabs drop off by the height of the
title bar.

**Step 4** creates and destroys OS windows:

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

- **Respect `visible`.** A torn-off tab starts as a *hidden* surface; it becomes
  visible only when dropped on the desktop. Ignore `visible` and you create and
  destroy an OS window on every tab drag.
- **Hosts that skip idle windows** must also ask the dock, or a tab docked from
  another window shows up late:

```rust
if w.ui.needs_frame_for(&info, idle) || dock.needs_frame(w.dock_id) {
    // build the frame: … dock.show(&mut w.ui, w.dock_id, &mut my_viewer) …
}
```

- **Hosts that cannot make windows** (tablets, consoles, engines) set
  `dock.config.floating_mode = FloatingMode::InApp`. Floating panels are then
  drawn inside the main window and steps 1–4 disappear.

### Panels and layouts

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

`id` is **saved into layout files**. Derive it from a fixed name with
`Id::from_name("inspector")`, not from an enum's position.

```rust
let mut dock = DockState::new();
let left  = dock.leaf(vec![MyTab::Hierarchy]);
let right = dock.leaf(vec![MyTab::Scene, MyTab::Game]);
let root  = dock.split(Axis::X, 0.25, left, right);
dock.set_root(SurfaceId::MAIN, root);
```

Other `DockState` calls:
- `add_tab`;
- `take_root` / `set_root`;
- `is_dragging` / `drop_target` / `cancel_drag`;
- `close_surface`;
- `config`, which controls how docking feels.

## 13. Rendering

### wgpu

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

If you skipped a UI frame because `needs_frame` returned false, redraw the
previous batches with `renderer.render_batches`. That is how a viewport keeps
animating while the UI around it costs nothing.

### Your own renderer

A renderer needs four things from the frame:
- `instances()`: plain data, 96 bytes each;
- `batches()`: instance ranges grouped by texture;
- `globals()`: 16 bytes;
- `atlas()`: a single-channel coverage texture.

The work per frame:
1. **Pipeline:** create one pipeline from `libgui_shaders`: six `float4`s per
   instance, premultiplied alpha, no depth.
2. **Upload:** upload the globals and instances, and the atlas whenever
   `Atlas::version` changes.
3. **Draw:** for each batch, bind its texture and draw 6 vertices × N
   instances.

`libgui::render_contract` holds every exact detail (strides, blend mode,
formats), with a version number.

**Without instancing** (bgfx, GLES2, WebGL1), `libgui::mesh` expands each frame
into plain quads instead.

### Your own textures

Register the texture with your renderer and show it with `ui.viewport(…)`;
this is how a 3-D scene sits in a panel. It is drawn opaque, in sRGB. Size the
render target from `ui.rect_of(id)` **after** `end_frame`, not from
`response.rect`, which is a frame old.

## 14. Themes, layouts, tests, features

### Themes

A `Theme` is a `Palette` (colours) plus `Metrics` (sizes) plus one style per
widget. The built-in themes are `Theme::dark()`, `midnight()` and `light()`.
Themes load from and save to TOML.

```rust
ui.with_style(|t| { t.button.radius = 0.0; }, |ui| { ui.button("Square"); });
```

### Saving layouts

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

A saved layout is versioned. When it is restored:
- a newer version is refused;
- panels your app no longer has are dropped;
- malformed input is repaired.

`to_toml` returns a `Result`, so a failure can stop you overwriting the last
good file.

### Testing

libgui needs no GPU and no window to test:

```rust
let mut ui = Ui::new(Theme::dark(), FONT)?;
ui.begin_frame(FrameInfo::default());
my_ui(&mut ui);
let out = ui.end_frame();
assert_eq!(out.draw.instances.len(), 3);
```

- **Input:** drive the UI with `ui.push(InputEvent::…)`, exactly as a user
  would.
- **Cost:** `ui.frame_cost()` and `Budget` turn a frame's cost into assertions.
- **Pixels:** `libgui_soft` renders to an image, for golden tests.

```rust
use libgui::testing::{steady_frame, Budget};

let cost = steady_frame(&mut ui, FrameInfo::default(), |ui| my_ui(ui));
Budget::steady(120).instances(400).assert(&cost);
```

### Cargo features

| Feature | Default | Adds |
|---|---|---|
| `fontdue` | on | the built-in rasteriser |
| `theme-toml` | on | TOML themes and layouts (with `serde`) |
| `shape` | off | rustybuzz shaping |
| `theme-watch` | off | `ThemeWatcher`, the crate's only filesystem access |
| `profile` | off | per-phase timings, the crate's only clock |

With no features, the core's only dependency is `bytemuck`.

## 15. From C and C++

`libgui_c` builds a static and a dynamic library, with
`crates/libgui_c/include/libgui.h` and the header-only `libgui.hpp`. It is a
separate crate because a C ABI promises stable bytes, while the Rust API is
pre-1.0.

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

**Rules at the boundary:**
- **Nothing panics across it.** A `Ui` that panicked is *poisoned*: later calls
  do nothing, and `libgui_ui_poisoned` reports it.
- **No allocation crosses it.** Strings are UTF-8 `const char*` that you own;
  nothing returned needs freeing.
- **Null is tolerated everywhere.** A null argument makes the call do nothing,
  and `libgui_last_error` says why.
- **Callbacks must not re-enter.** A callback (a validator, a label, a tree
  source) must not call back into libgui with the same handle; the call is
  refused.

**Closures become pairs.** Every Rust closure becomes an `open_` / `close_`
pair in C. In C++ the pairs are RAII guards, and lambdas work as paint
callbacks and virtual-list rows:

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

**Renderers that cannot instance** (bgfx and similar) call
`libgui_enable_mesh(ui, 1)` and read `libgui_mesh_vertices` / `_indices` /
`_batches`. For fixed per-frame buffers, `libgui_set_mesh_limits` cuts the
frame into chunks that fit:

```c
libgui_enable_mesh(ui, 1);
/* ... build and end the frame ... */
uint64_t nv = 0, ni = 0, nb = 0;
const void*        vertices = libgui_mesh_vertices(ui, &nv);
const uint32_t*    indices  = libgui_mesh_indices(ui, &ni);
const LibguiBatch* batches  = libgui_mesh_batches(ui, &nb);
/* batches[i].first/count are into the index buffer. */
```

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

**Checking a renderer port.** `libgui_enable_reference_render` renders each
frame on the CPU as well, with exactly the same bytes on every machine, so you
can compare a port against it pixel by pixel. `libgui_conformance_*` is a
gallery with one scene per primitive.

**The order within a frame:** build, then `libgui_end_frame`, then size your
render targets (`libgui_rect_of`), then render your scene, then draw the UI.

**Canvases and caching from C:**

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

```c
uint64_t tick = (uint64_t)(now * 10.0);          /* ten times a second */
if (libgui_open_cached(ui, "telemetry", tick)) {
    build_telemetry_panel(ui);
    libgui_close_cached(ui);
}
```

**Text buffers.** A C text field writes into your buffer. When a paste does not
fit, `out_len` says so; grow the buffer and fetch the rest with
`libgui_text_overflow`. The C++ wrapper does this for you.

**CMake:**

```cmake
add_subdirectory(third_party/libgui/crates/libgui_c)
target_link_libraries(vcad PRIVATE libgui::libgui)
```

`cargo` must be on `PATH`. A debug build of libgui is about ten times slower,
so building libgui as Release inside a Debug app is reasonable.

**How the C API keeps up with Rust:**
- **Widget calls:** declared once in `src/table.rs`, which generates the
  function, its header line and its documentation together.
- **Header drift:** `cargo test -p libgui_c` fails when the committed header is
  stale; regenerate it with `LIBGUI_WRITE_HEADER=1`.
- **Struct layouts:** `tests/smoke.c` checks every struct's size against the
  library.
- **Test coverage:** every exported function is called by a test.

### What C can reach

Everything an application or a custom widget needs (§16 lists it all):
- **Building:** widgets, containers, scroll areas, virtual lists, trees,
  tables, popups, menus, layers, canvases, caching.
- **Custom widgets:** focusable custom widgets and the full Painter.
- **Input and state:** raw key and button state, animation and springs,
  validated fields, the colour picker, notifications, drag and drop (including
  from the OS).
- **Platform:** docking, themes from TOML, the keymap.
- **Output:** every form of frame output.

What stays Rust-only, and why:

| Rust-only | Why | From C instead |
|---|---|---|
| `virtual_rows` (rows of different heights) | takes a height closure | fixed-height `libgui_open_virtual_list`, or a scroll area |
| `with_style` / `button_styled` | the `Theme` struct is not mirrored | `libgui_set_theme_toml` for a whole theme |
| `shortcut_scope` | closure-scoped | test `libgui_any_popup_open` / focus yourself |
| `overlay`, `layer_fit_in`, `container_at` | closure-scoped placement | `libgui_open_layer` with a rect |
| `Ui::with_rasterizer` | takes a Rust trait object | `libgui_ui_new_with_fallbacks` |
| `frame_cost`, `profile`, `gesture`, `release_action` | diagnostics, touch gestures, rare | — |

---

# Part III: Reference

## 16. API by area

Rust names are methods on `Ui` unless stated otherwise. C names drop the
`libgui_` prefix; ✗ means Rust-only (see §15). In C++, most calls are
`ui.name(…)`.

### Frame and host

| What | Rust | C |
|---|---|---|
| create | `Ui::new`, `with_fallbacks`, `sharing_fonts` | `ui_new`, `ui_new_with_fallbacks`, `ui_new_sharing_fonts` |
| frame | `begin_frame`, `end_frame` | `begin_frame`, `end_frame`, `frame_*` |
| input | `push(InputEvent)` | `push_pointer_*`, `push_key`, `push_text`, `push_wheel`, `push_touch`, … |
| idle | `needs_frame`, `needs_frame_for`, `request_repaint`, `request_repaint_in` | `needs_frame`, `request_repaint`, `request_repaint_in` |
| keymap | `libgui_keymap::Keymap::install` | `install_keymap`, `install_default_keymap` |
| rect after layout | `rect_of` | `rect_of` |

### Layout

| What | Rust | C |
|---|---|---|
| container | `container(_id)`, `row`, `column`, `panel`, `card`, `open_/close_container` | `open_/close_container` |
| scroll | `scroll_area(_with)`, `open_/close_scroll_area` | `open_/close_scroll_area` |
| spacing | `space`, `flex`, `separator` | `space`, `flex`, `separator` |
| disabled | `enabled`, `open_/close_enabled`, `is_enabled` | `open_/close_enabled`, `is_enabled` |
| identity | `make_id`, `with_key`, `Id::from_name`, `keep_id` | `id_from_name`, `keep_id` |
| layers | `layer`, `layer_in`, `popup`, `open_popup` | `open_/close_layer`, `open_popup`, `open_child_popup` |
| transform | `canvas`, `open_canvas`, `with_transform` | `open_/close_canvas`, `open_/close_transform` |
| caching | `cached` | `open_/close_cached` |

### Widgets

| What | Rust | C |
|---|---|---|
| text | `label`, `label_muted`, `heading`, `section`, `paragraph` | same names |
| buttons | `button`, `button_primary`, `button_keyed`, `button_styled` | `button`, `button_primary`, `button_keyed`, ✗ |
| choices | `checkbox`, `toggle`, `radio`, `combo`, `segmented` | same names |
| numbers | `slider`, `slider_vertical`, `drag_value`, `drag_value_range` | same names |
| text entry | `text_input`, `text_area`, `validated_input` | same names, `text_overflow` |
| colour | `color_picker`, `color_button` | same names |
| display | `progress`, `plot`, `viewport`, `tooltip` | same names |
| panes | `splitter` | `splitter` |

### Collections

| What | Rust | C |
|---|---|---|
| cursor | `open_/close_collection`, `set_cursor`, `type_ahead` | `open_/close_collection`, `nav_*`, `set_cursor`, `type_ahead` |
| selection | `select`, `selectable(_keyed)` | `select`, `select_kind`, `selectable(_keyed)` |
| long lists | `virtual_list`, `open_virtual_list` / `_row`, `virtual_rows` | `open_/close_virtual_list`, `open_/close_virtual_row` |
| trees | `tree_view` + `TreeSource`, `tree_row` | `tree_new`, `tree_view` + `LibguiTreeSource`, `tree_row` |
| tables | `table` + `TableState` | `table_new`, `table_show`, … |

### Menus, notifications, drag and drop

| What | Rust | C |
|---|---|---|
| menus | `menu_button`, `submenu`, `menu_item(_shortcut)`, `context_menu` | `open_/close_menu`, `menu_item*`, `open_context_menu` |
| popups | `any_popup_open`, `close_popups` | same names |
| notifications | `toast`, `dismiss_toast`, `toast_count`, `show_toasts(_with)` | same names |
| drag and drop | `drag_source`, `drop_zone`, `dragging`, `drag_ghost`, `insertion_line` | same names |
| from the OS | `begin_/end_external_drag` | same names |

### Custom widgets

| What | Rust | C |
|---|---|---|
| hit test | `interact`, `interact_drag`, `interact_focusable(_drag)` | `interact`, `interact_drag`, `interact_focusable` |
| leaf | `add_leaf` + `Painter` | `add_leaf` + `painter_*` |
| dashes, turning | `dashed_line`, `dashed_polyline`, `image_rotated`, `text_rotated` | `painter_dashed_line`, `painter_dashed_polyline`, `painter_image_rotated`, `painter_text_rotated` |
| focus | `focused`, `set_focus`, `scroll_to` | `focused`, `set_focus`, `scroll_to` |
| raw input | `key_pressed`, `key_down`, `button_down`, `button_pressed`, `hover_time`, `pointer_velocity` | `key_pressed`, `key_down`, `pointer_button_down`, `pointer_button_pressed`, `hover_time`, `pointer_velocity` |
| pointer lock | `request_pointer_lock` | `request_pointer_lock` |
| shortcuts | `consume_shortcut` | `consume_shortcut` |
| motion | `animate`, `animate_bool`, `animate_spring(_with)`, `set_spring`, `set_anim` | same names, `spring_velocity` |

### Docking (`DockState`)

| What | Rust | C |
|---|---|---|
| build | `leaf`, `split`, `set_root`, `add_tab` | `dock_leaf`, `dock_split`, `dock_set_root`, `dock_add_tab` |
| per iteration | `set_pointer`, `set_surface_frame`, `update`, `show`, `needs_frame` | `dock_set_pointer`, `dock_set_surface_frame`, `dock_update`, `dock_show`, `dock_needs_frame` |
| windows | `surfaces`, `surface`, `close_surface` | `dock_surface_count`, `dock_surface_at`, `dock_close_surface` |
| save | `layout` / `restore` | `dock_layout_to_toml` / `dock_restore_from_toml` |

### Theme and output

| What | Rust | C |
|---|---|---|
| theme | `Theme::dark/light/midnight`, `ui.theme`, TOML | `set_theme`, `set_theme_toml`, `theme_to_toml` |
| draw data | `out.draw`, `atlas`, `platform` | `frame_instances`, `frame_batches`, `frame_atlas`, `frame_platform` |
| mesh | `libgui::mesh` | `enable_mesh`, `mesh_*`, `set_mesh_limits` |
| reference | `libgui_soft` | `enable_reference_render`, `reference_pixels`, `conformance_*` |

The full signatures are in rustdoc (`cargo doc -p libgui --open`) and in
`libgui.h`, whose comments are the same text.

## 17. Limitations

**Can block some apps**
- **No accessibility tree** (AccessKit, screen readers). This is the largest
  gap.
- **No bidirectional text.** Arabic and Hebrew lay out left to right.

**Widgets**
- `plot` is a simple bar chart.
- There is no modal type (build one from layers, §7) and no date picker.
- Tables have no 2-D cell cursor; trees cannot be reordered by dragging.

**Text**
- `text_area` does not wrap; long lines scroll sideways.
- There is no double-click to select a word.
- The caret moves by `char` and can split an emoji sequence.
- There is one font role per `Ui`.

**Drawing**
- Turning is for drawing only: widgets, layout and hit-testing stay upright,
  and a vector `Path` icon is not turned (turn its points instead).
- Dash ends are square; there are no round or custom dash caps, and no
  arrowheads.
- The glyph atlas is one page; when it fills, it resets with a one-frame
  flicker.

**Structure**
- Container `Id`s are positional by default (§5).
- Restoring a layout does not restore focus.

**Pre-1.0.** The API changes between versions, so pin an exact one.
