# Changelog

libgui is pre-1.0 and is usually vendored by pinning a commit. This file
lists, for each change to the API or the render contract, **what you have to
change** when you move your pin past it. Additions that ask nothing of you
are named briefly; the commit messages have the detail.

Three version numbers matter:

- **`CONTRACT_VERSION`** (`libgui_contract_version()` in C): the instance
  format and the shader's behaviour. Only a host with **its own port of the
  shader** (bgfx, a game engine's RHI) has to act when it changes. wgpu,
  `libgui_soft` and the mesh path are updated with the library.
- **`LIBGUI_ABI_VERSION`**: the layout of the C structs. A mismatch is
  refused at start-up; rebuild against the new `libgui.h`.
- **The Rust API**, which changes freely before 1.0. The compiler finds most
  of it; this file lists the changes it cannot see, such as new behaviour or
  new dependencies.

Newest first.

---

## Unreleased: right-to-left text (WO-4)

**Rust and C, no API change.** Hebrew, Arabic and mixed-direction text are now
laid out right to left (Unicode bidi, UAX #9): reordered per line, carets on
the right side of a character, Left and Right moving on screen, selections
in several pieces, brackets mirrored, and right-to-left paragraphs aligned
right in `text_area`, `paragraph` and `text_wrapped`.

**What you change:**
- **Vendoring with `cargo vendor` or an offline registry:** add the new
  dependency `unicode-bidi` 0.3 (no dependencies of its own). It sits behind
  the new **default** feature `bidi`. Building with `default-features = false`
  and without `bidi` drops it and keeps the old left-to-right layout.
- **A custom `FontRasterizer` that mirrors brackets itself** (a HarfBuzz,
  CoreText or DirectWrite backend told the run's direction) should override
  the new `shape_rtl` to shape the run as right to left. The default mirrors
  brackets and calls `shape`; with a shaper that also mirrors, `(` turns
  twice and comes out as `(`. `ShapeRasterizer` and `FontStack` already do
  this.
- `Fonts::caret_x` and `byte_at_x` now answer visually for right-to-left text:
  the start of a Hebrew word is at its right edge. Code that assumed x grows
  with the byte offset is wrong for such text.
- Text without right-to-left characters is laid out exactly as before; no
  golden image changed.

## 2026-10-08: wrap in the text area (WO-2) · `bf88a69`

**What you change:**
- **`text_area` wraps by default.** A code or script editor that wants hard
  lines scrolling sideways sets `TextAreaOptions { wrap: false, .. }` (C:
  `libgui_text_area_with` with `wrap = false`).
- Home and End now go to the ends of the **row on screen**, not the logical
  line. `TextResponse::caret` still reports the logical line and column.

Added: `libgui_text_area_with`, `LibguiTextAreaOptions`,
`libgui_text_area_options_default`; C++ `ui.text_area(key, s, opts)`.

## 2026-10-08: contract 5, atlas pages and triangles (WO-1, WO-3) · `14b48e0`

**`CONTRACT_VERSION` 5.** Same 96-byte instance.

**What you change:**
- **Every host:** the glyph atlas is several **pages**, one texture each
  (2048², single channel). `TextureId::Atlas(page)` replaces the one atlas.
  In C, read `libgui_frame_atlas_pages` (`LibguiAtlasPage`) and upload each
  page whose version changed; a batch's `texture_index` names its page. A
  host that uploaded "the atlas" draws later pages with the wrong texture.
- **Hand-ported shaders:** add the fifth kind, **Triangle**. Two corners are
  in `uv`, and the third corner plus per-edge outline bits are in
  `border_color`. Take coverage at the pixel centre
  (`gl_FragCoord.xy / scale`), not at the interpolated position; otherwise
  meshes show seams on real GPUs. Outline edges are anti-aliased; shared
  edges use the exact tie rule in `ui.wgsl`. The rendering rules at the top
  of `libgui.h` ("Version 5") spell it out. The mesh path needs the same
  fragment branch.
- `set_atlas_limit` is still the memory budget (default 16 MB, four pages).
  A frame that needs more than the whole budget is counted in
  `FrameCost::atlas_overflows`.

Added: `fill_polygon`, `fill_polygon_with_holes`, `fill_mesh` (C:
`libgui_painter_fill_polygon` / `_with_holes` / `_fill_mesh`).

## 2026-10-08: modal dialogs · `1e7ebb2`

Added: `ui.modal`, `ModalOptions`, `any_modal_open`, `ModalStyle`, and the
C calls `libgui_open_modal` / `libgui_close_modal`.

**What you change:**
- A match on `Layer` gains the new `Layer::Modal`, which sits above windows
  and below popups.
- While a modal is open, the app's own input (a 3-D view orbiting behind it)
  should check `any_modal_open()`. App shortcuts built outside the dialog are
  held back unless `ModalOptions::shortcuts_behind` is set.
- A theme file written before this still loads; `ModalStyle` takes defaults.

## 2026-10-07: scopes and meters · `acc83bd`

Added: `ui.scope`, `ui.meter`, `meter_with_average`, `p.trace`,
`p.trace_fill`, `ScopeStyle` and `MeterStyle` in the theme, and C
equivalents. Nothing to change; older theme files still load.

## 2026-10-07: contract 4, dashes and rotation · `bf6815d`

**`CONTRACT_VERSION` 4.** Same 96-byte instance.

**What you change:**
- **Hand-ported shaders:** add the dash test to the line branch of the
  fragment stage. A line carries `on` and `off` in `params[1..3]` and its
  phase in `border_color[0]`; zeros mean a solid line, as before.
  Images and glyphs carry a turn in `border_color` as `(cos − 1, sin)`. The
  instanced vertex stage turns the quad. The mesh path turns quads on the
  CPU, so a mesh-path port needs only the dash test.
- Check `libgui_contract_version()` at start-up against the version your
  port implements.

Added: `dashed_line`, `dashed_polyline`, `Dash`, `image_rotated`,
`text_rotated`, and C equivalents.

## 2026-10-06: toasts, and real time across a sleep · `ee5a8fd`

**What you change:**
- **Pass libgui the real elapsed time as `dt`**, even after a long sleep. libgui
  clamps its animation steps to 0.25 s itself; timing (double-clicks, toasts,
  held meter peaks) now uses the real value. A host that clamped `dt` for its
  own scene should keep its clamp there and stop applying it to libgui.
- Call `ui.show_toasts()` last each frame if you use toasts.

Added: `ui.toast`, `Toast`, `request_repaint_in`, and C equivalents.

## 2026-10-06: docked tabs in an idle window · `16b6615`

**What you change:** a host that rebuilds a window only when its `Ui` needs a
frame should also rebuild it when `DockState::needs_frame(surface)` is true
(C: `libgui_dock_needs_frame`). Otherwise a tab dropped into an idle window
does not appear until the pointer reaches it.

## 2026-10-06: keyboard gaps · `8425d46`

**What you change:** a host with its **own** key bindings, rather than
`libgui_keymap`, should bind Space to the new `UiAction::Activate` (and Enter
on Windows and Linux). Without it no focused control can be pressed from the
keyboard.

## 2026-10-06: edges on the pixel grid · `a7265c0`

**What you change:**
- Layout rects are now whole physical pixels. Code that compared rects with
  exact fractional values will see them move by up to a pixel.
- **A C host that sizes a render target from a widget's rect** should use the
  new `libgui_rect_of(ui, id, &rect)` after `end_frame`. `response.rect` is
  last frame's rect, which stretches the view during a drag.

## 2026-10-05: springs · `0e76676`

Added: `animate_spring`, `set_spring`, `pointer_velocity`,
`Response::released`. `Response` gained a field; in C it took a padding
byte, so no struct moved.

## 2026-10-03: contract 3, image alpha · `7d36858`

**`CONTRACT_VERSION` 3.**

**What you change:**
- **Hand-ported shaders:** an image's `params[1]` says how to read its alpha:
  opaque, premultiplied or straight. Straight alpha is premultiplied **per
  texel, before filtering**. A renderer that filters in hardware should
  premultiply on upload. An older port draws every image opaque.

Added: `fill_path` (no backend change: it draws through the atlas).

## 2026-09-24: validation moves to the app · `54b6d7b`

**What you change:** `number_input` and `libgui_number_input` are gone from
the core. Use `validated_input` with your own validator, or
`libgui_units::number_input` for the bundled unit grammar. In C use
`libgui_validated_input` with a callback.

## 2026-09-24: long pastes in C · `29aeb17`

**What you change:** when a C text field's buffer is too small for a paste,
fetch the whole text with `libgui_text_overflow`. Do not call the field again
in the same frame: that is a different widget and the paste is lost.
`TextResponse` gained `cancelled`.

## 2026-09-23: one atlas for several windows · `85ff1a3`

**What you change (Rust):** `FrameOutput::atlas` changed type to a snapshot
behind an `Rc`; `frame.atlas()` is unchanged. Added `Ui::sharing_fonts` and
`Mesh::build_limited`.

## 2026-09-23: `LIBGUI_ABI_VERSION` 2 · `c71670f`

**What you change:** rebuild against the new `libgui.h`. Texture ids are 64
bits end to end, so a native handle or pointer survives the trip. Before,
they were cut to 32 bits and came back as a different texture.
`LibguiBatch` gained a field.

Added: the conformance kit (`libgui_enable_reference_render`,
`libgui_conformance_*`) and `libgui_viewport_uv`.

## 2026-09-23: the C ABI · `95cd0a0`

`LIBGUI_ABI_VERSION` 1. Check `libgui_abi_version() == LIBGUI_ABI_VERSION`
once at start-up.

## 2026-09-18: contract 2 · `34447d7`

**`CONTRACT_VERSION` 2.** Same 96-byte instance.

**What you change:**
- **Hand-ported shaders:** add the fourth kind, **Line**: a segment with
  round caps, drawn as a capsule SDF. Its endpoints are in `uv` and its half
  width in `params[0]`. Polylines and curves are many of these.

Added: `container_at` places a container inside a canvas, so it pans and
zooms with it; `layer` ignores an enclosing canvas by design.

## 2026-09-18: contract 1 · `b9b4ef4`

The render contract begins: a 96-byte instance and one shader.
