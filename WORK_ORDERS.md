# Work orders: reviewer gaps (October 2026)

Six points came from a reviewer integrating libgui. Two need no work (video
scopes, vendoring). Four are real; each has a work order below, in the order
recommended.

| # | Gap | Status today | Priority |
|---|---|---|---|
| WO-1 | One glyph-atlas page; flicker when full | **Done** (contract 5): pages, least-recently-used reuse, nothing on screen lost unless one frame outgrows the whole budget | — |
| WO-2 | No word wrap in `text_area` | Long lines scroll sideways | Medium |
| WO-3 | No triangle primitive | **Done** (contract 5): `fill_polygon`, `_with_holes`, `fill_mesh`; seam-free on CPU and GPU | — |
| WO-4 | No right-to-left text | Shaped correctly, laid out left to right | Low unless shipping to RTL markets |
| — | Video scopes | Correct as planned: GPU textures through `viewport`/`image` | None |
| — | Pre-1.0 API | Vendoring a commit is right; add a changelog | Small (WO-5) |

---

## WO-1 Multi-page glyph atlas with eviction

**Today.** One single-channel page grows to `set_atlas_limit` (4096 default).
When it fills, it is cleared and repacked; glyphs needed that frame are drawn
the frame after, so text blinks out once.

**How a professional library handles it.** Several fixed-size pages, sometimes
grouped by size class. Each entry records the frame it was last used, so the
library always knows which glyphs are in use. When a page fills, the
least-recently-used glyphs not drawn this frame are evicted, or the
least-used page is recycled whole. Nothing on screen ever disappears, the
memory ceiling is fixed, and the cost is spread over time instead of paid in
one frame. Dear ImGui, Skia, WebRender and Chromium all work this way.

**Scope.**
- `TextureId::Atlas` becomes atlas page *n*. Batches split at page changes.
- Each page carries its own version, so hosts re-upload one page, not all.
- Eviction: least recently used by frame stamp; never evict a glyph drawn this
  frame. The atlas only resets when one frame alone needs more than every page
  together, and that is reported in `FrameCost`.
- Shared fonts (`Ui::sharing_fonts`): the stamp is the newest frame across
  every sharing `Ui`.
- Contract version 5. Updates to the shader (no change expected), wgpu, the
  CPU renderer, mesh, C (`libgui_frame_atlas` becomes per page) and the
  `libgui.h` rendering rules.

**Done when.**
- A torture test cycles through ~20 sizes × 3 scripts (Latin, CJK, symbols)
  for 600 frames with no frame missing a glyph. It compares every frame
  against an uncapped reference.
- No allocation in steady state, and a settled frame rasterises no glyphs.
- GPU and mesh parity hold; golden images are unchanged.
- The C smoke test uploads per page.

**Risk.** It is a renderer contract change, so every host updates once. Bundle
it with WO-3 to do that only once.

---

## WO-2 Word wrap in `text_area`

**Today.** `paragraph` wraps read-only text, with cached line breaks.
`text_area` does not wrap; long lines scroll sideways.

**How a professional library handles it.** Lines are laid out per paragraph
and cached, and only paragraphs that are edited or resized are redone; long
documents stay fast because only visible paragraphs are drawn. The caret
knows two positions: where it is in the text, and which visual line it sits
on, because a wrap point is both the end of one line and the start of the
next. Up and Down keep the column the caret started from. Home and End go to
the edges of the visual line, not the whole paragraph. Selection is drawn as
one rect per visual line.

**Scope.**
- `TextAreaOptions::wrap` (on by default? decide).
- Reuse the wrap cache.
- Caret affinity at wrap points; Up/Down keep their column across wrapped
  lines; Home/End go to the visual line.
- Selection rects and hit-testing per visual line.
- Rewrap when the width changes, kept to the paragraphs on screen. IME
  composition appears inline (this also closes the "no IME preedit in
  `text_area`" gap).

**Done when.**
- Caret movement tests cover Up, Down, Home and End across wrapped lines, and
  the caret is right on both sides of a wrap point.
- Selection across a wrap renders correctly (golden image).
- A 10,000-line document scrolls without rewrapping off-screen text
  (`FrameCost` budget).
- Undo and paste still work.

---

## WO-3 Triangle primitive

**Today.** Lines, rects, glyph-atlas paths. Dense waveforms are already handled:
`scope` and `p.trace` draw one stroke per pixel column, whatever the sample
count, and `trace_fill` fills under them. Textures are **not** needed for
those. What is missing is filled geometry that changes every frame (sketch
regions, smooth area charts, polygons): today `fill_path` rasterises it on the
CPU into the atlas, every time it changes.

**How a professional library handles it.** The library splits filled shapes
into triangles on the CPU and anti-aliases their edges, either with a thin
feathered fringe or with coverage computed in the shader, and sends them to
the GPU as vertices. Instanced renderers handle this as a second kind of draw
alongside quads, with no per-shape texture. Skia, NanoVG, Dear ImGui and
Vello all work this way.

**Scope.**
- A `Triangles` draw: vertices with position, colour and an anti-aliasing
  coordinate, plus indices. Emitted by `p.fill_polygon(points, colour)` and
  `p.fill_path_live(path, colour)` (the latter splits the path into triangles
  every frame instead of caching it in the atlas).
- Edges anti-aliased with a 1 px feathered fringe.
- Instanced backends get a second pipeline (vertex + index); the mesh path
  appends to the triangles it already produces.
- `trace_fill` can move onto it, making fills smoother than columns.
- Contract version 5, shared with WO-1.

**Done when.**
- CPU, GPU and mesh parity hold on a polygon gallery scene: convex, concave,
  holes, and slivers thinner than a pixel.
- A 2,000-vertex shape that changes every frame costs no rasterisation and no
  atlas use.
- C exports and the C++ wrapper exist; `libgui.h` documents the second draw
  kind.

---

## WO-4 Right-to-left and bidirectional text

**Today.** `ShapeRasterizer` shapes Arabic and Hebrew correctly (joined
forms), but runs are laid out left to right, and caret and selection assume
left to right.

**How a professional library handles it.**
- Run the Unicode bidirectional algorithm (UAX #9) on each paragraph to split
  it into runs of one direction, and reorder them visually per line, after
  wrapping.
- Shape each run in its own direction.
- The caret works in text order but is drawn visually. Where two directions
  meet it has two possible screen positions, and the platform decides which
  to draw (macOS shows a split caret).
- Arrow keys move visually.
- A selection can be several separate rects on one line.
- Alignment and paragraph direction come from the text or are set
  explicitly; the layout of the whole app is mirrored for right-to-left
  languages.

Pango, ICU/HarfBuzz, DirectWrite, CoreText and Chromium's LayoutNG all work
this way.

**Scope.**
- Bidi runs (the `unicode-bidi` crate behind the `shape` feature) in shaping
  and wrapping.
- Visual reordering per line.
- Caret: logical position, visual drawing, and an option for how to show the
  caret where directions meet.
- Selection with split rects; hit-testing per run.
- Paragraph direction and `Align::Start` resolving to right for
  right-to-left text.
- Mirroring the layout of whole panels for right-to-left languages is a
  follow-up, not part of this order.

**Done when.**
- Golden images of mixed Arabic, Hebrew and Latin lines, with numbers inside
  right-to-left text.
- Caret walk tests across direction boundaries.
- Selection rects across a boundary are checked.
- The text input and `paragraph` both pass.

**Note.** Pair with the accessibility work: both block shipping to
consumers in those markets.

---

## WO-5 Changelog for vendored users

Add `CHANGELOG.md`: one entry per commit that changes the API or
`CONTRACT_VERSION`, listing what a vendored user must change (e.g. contract 4:
port the dash test into a hand-written fragment shader). Most of this is
already in commit messages; this collects it where a vendored user will look.

---

## Not work orders

- **Video scopes.** The waveform monitor and vectorscope belong on the GPU as
  the app's own textures, shown through `ui.viewport` or `p.image`. That is the
  intended path for pixel-heavy views; `scope` is for UI-scale data.
- **Pinning by vendoring.** Correct for pre-1.0. WO-5 makes upgrading the pin
  cheaper.
