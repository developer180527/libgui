# Design decisions

Why libgui is shaped the way it is.

Each entry is a decision that was made deliberately, with the forces that led
to it and **the consequences, including the ones that hurt**. A rationale
without its costs is marketing; the costs are why these are worth writing down.

The short version, if you read nothing else: **libgui does UI work and asks the
host for everything else.** Almost every decision below follows from that one,
and the rest follow from being a library other people's applications embed
rather than a framework that embeds them.

---

## 1. Hybrid immediate/retained, not one or the other

**Context.** An immediate-mode UI is a joy to write — the code reads like what
it draws, and there is no state to synchronise. It is also traditionally
wasteful: re-measuring text every frame, re-laying out everything, no memory of
what the user was doing. A retained UI is the opposite: efficient, and a
bookkeeping problem where every widget must be created, updated and destroyed
in step with the data behind it.

**Decision.** The API is immediate; the internals are retained. You call
widgets every frame. Behind them, libgui keeps per-widget state keyed by a
stable [`Id`], caches shaped text and layout, and solves the tree after the
frame is built.

**Consequences.**

- Widget state — a scroll position, a caret, a drag in progress — survives
  frames without the app holding it.
- Identity becomes load-bearing. See §8.
- A widget that is not called this frame loses its state, which is usually what
  you want and occasionally a surprise.

---

## 2. The core does UI work and nothing else

**Context.** A UI library that owns the window, the event loop and the GPU is
easy to start with and hard to embed. A game engine already has all three. So
does a CAD application, a video tool, or anything with a viewport.

**Decision.** `libgui` has **no window, no GPU, no clock, no filesystem, no
threads and no network**. It takes input events and three numbers, and hands
back a list of rectangles and a set of requests.

This is not a style preference; it is enforced. `tests/boundaries.rs` reads the
crate's own source and fails the build on `std::fs`, `std::time`,
`std::thread`, `std::net`, `std::process`, `std::env`, `std::io`,
`SystemTime` or `Instant::now`. Two files are exempt and say so:
`theme_watch.rs`, whose whole job is polling a file, and `profile.rs`, whose
whole job is reading a clock — both off by default.

**Consequences.**

- It embeds in anything. The same core drives wgpu, a CPU rasteriser, bgfx
  through a C++ engine, and a headless test.
- **The host has more to do.** Input translation, clipboard, cursor, IME
  placement and redraw scheduling are all yours. `libgui_winit` is ~350 lines
  showing what that costs; a C++ host writes the same.
- Anything the core cannot do itself becomes a *request* rather than an action.
  The clipboard is the clearest case: libgui never touches it, it reports
  `copied_text` and `paste_requested` and waits.

---

## 3. No key bindings and no focus policy in the core

**Context.** Which chord copies, which controls the keyboard visits, whether
clicking a button focuses it — all of this is platform convention, and
platforms disagree flatly. macOS visits text fields only until Full Keyboard
Access is on; Windows and Linux visit everything. Cmd on one, Ctrl on the
others.

**Decision.** Widgets respond to [`UiAction`]s — `Move`, `Delete`, `Submit`,
`Navigate` — and **never to keys**. What chord produces one lives in
`libgui_keymap`, which takes the platform as an *argument* rather than reading
the one it was compiled for.

**Consequences.**

- The same binary can behave like any platform, which is also what makes the
  keymap testable.
- A host with no keyboard at all can drive every widget by sending actions
  directly: a gamepad, a foot pedal, an accessibility switch.
- **A text field ignores Backspace until you install a table.** That is the
  cost, it surprises people, and it is written at the top of the manual.

---

## 4. The application owns its data; libgui owns its widgets' state

**Context.** The tempting thing is for a list widget to own the selection, a
tree to own which nodes are expanded, a table to own its sort. It reads well in
a demo and fights every real application, because those things belong to the
document.

**Decision.** libgui keeps what is *its* — scroll offsets, carets, hover
animations, drag state, keyboard cursors. The app keeps what is the
application's: the selection set, the expanded set, the document, the sort
order. Where the split is subtle, libgui keeps the **mechanism** and the app
keeps the **policy**.

Multi-select is the sharpest example. libgui holds the *anchor* a Shift-click
extends from — bookkeeping every app would otherwise write and get subtly wrong
— and returns `Only` / `Toggle` / `Range`. What a row *is* stays yours: a CAD
browser selects bodies, a file list selects paths.

**Consequences.**

- Widgets take `&mut` to your data and write through it.
- Undo, persistence and multi-window all work, because the state that matters
  is somewhere you can save.
- More typing at the call site than a framework that owns everything.

---

## 5. Layout is solved after the frame is built

**Context.** A widget's size can depend on its siblings, its container, and the
window. Nothing knows its final rect while the frame is still being described.

**Decision.** The build pass records a tree; `end_frame` solves it (`fit`, then
`arrange`) and then paints. Paint closures run *after* layout and are handed
their final rect.

**Consequences.**

- `Response::rect` is **last frame's**. One frame of latency, invisible in
  practice, and the reason `scroll_to` and hit testing are written the way they
  are.
- A paint closure must own what it captures (`'static`), so text is resolved
  into a frame arena first.
- A two-pass "tell me the rects, then I will draw into them" API is not
  possible without a frame of lag. This was considered for the C docking
  binding and rejected for exactly that reason.

---

## 6. Every closure-taking builder has an `open_`/`close_` pair

**Context.** libgui's natural Rust shape is `ui.container(…, |ui| { … })`. C has
no closures, and a C++ host is a first-class target.

**Decision.** Anything that takes a body also exists as a bracket:
`open_container`/`close_container`, `open_scroll_area`, `open_popup_body`,
`open_menu`, `open_collection`, `open_layer`. The closure form calls the pair,
so the two cannot drift.

**Consequences.**

- The C ABI needs a callback only where one is genuinely unavoidable: custom
  painting, dock panels and table cells.
- One declaration carries the documentation too. The doc comment on a widget's
  line in the table becomes both its rustdoc and the comment above its C
  declaration, so the header explains itself and cannot drift from the Rust
  side; a widget without one fails the drift test.
- Two entry points per builder to document.
- `open_depth()` exists so a binding can blame the caller for a missing close
  rather than letting an assertion fire somewhere less useful. The C binding
  checks it at `libgui_end_frame` and poisons the handle, because libgui's own
  check is a `debug_assert` and an application ships the release build.
- **A pair whose `open_` answers a question is the sharp one.** `open_cached`
  returns false on a frame that replayed, and `open_popup_body` returns false
  while the popup is closed — which is most frames. Ignoring the answer
  compiles, and closing anyway used to pop the *enclosing* cache or close the
  caller's own container: content vanished from later frames and the handle
  still called itself healthy. Both now refuse, name themselves, and check
  they are closing the node they opened. A quiet `debug_assert` was the wrong
  choice for a mistake only a release build can make.

---

## 7. The render contract is instances, with a mesh form for renderers that cannot

**Context.** One instance per primitive is the efficient form: 96 bytes, six
`vec4` attributes, six vertices. Not every renderer can draw it. bgfx caps
instance data at five `vec4`s; GLES2 and WebGL1 have no per-instance attributes
at all; plenty of engine RHIs expose a vertex-and-index draw and nothing more.

**Decision.** The contract is instanced (`CONTRACT_VERSION = 2`,
`INSTANCE_STRIDE = 96`). `libgui::mesh` expands the same frame into one quad
per primitive — four vertices, six indices, `VERTEX_STRIDE = 112` — with every
value the fragment shader needs already computed. The vertex mirrors the
shader's *varyings*, not its inputs, so porting the shader is a four-line
vertex stage.

**Consequences.**

- Anyone can render libgui. vCAD takes the mesh form through bgfx.
- The mesh costs about **4.5× the bytes**, which a test pins and the docs say.
  A renderer that can instance still should.
- Two paths to keep correct. `mesh_parity.rs` renders every golden scene × 2 themes × 3
  scales both ways and allows one 8-bit step of difference in at most one pixel
  in ten thousand.

---

## 8. Identity is positional by default, and that is a footgun

**Context.** Retained state needs a key. Making every call site pass one is
noise; deriving it from the container and build order is invisible and correct
— until the build order changes.

**Decision.** An `Id` comes from the container path plus a key, and duplicates
in one container are disambiguated by build order. `Id::from_name` is a stable
hash: same value across Rust releases, 32/64-bit and endianness, so it is safe
to persist and to pass over FFI.

**Consequences.**

- Most code never thinks about ids.
- **The moment you add conditional UI, state jumps between widgets.** `with_key`
  and the `*_keyed` variants exist for this, `FrameCost::unkeyed_duplicates`
  counts the risk, and the manual says so in its own section.

---

## 9. The C ABI is a separate crate, not a feature

**Context.** vCAD is C++. The obvious move is `crate-type = ["lib",
"staticlib"]` on `libgui` behind a feature.

**Decision.** `libgui_c` is its own crate.

Two reasons, one technical and one about promises. **Technical:** `crate-type`
is fixed in the manifest and *cannot* be switched on by a feature, so declaring
`staticlib` on `libgui` would build one for every Rust-only user, forever.
**About promises:** libgui's Rust API is pre-1.0 and changes freely; a C ABI is
a promise about *bytes* — struct layouts, symbol names, calling convention —
that a C++ host links against. Different promises need different version
numbers, and a crate has one.

**Consequences.**

- `LIBGUI_ABI_VERSION` moves on its own schedule.
- The core's dependency allowlist stays meaningful.
- Two surfaces to keep in step, which §11 is about.

---

## 10. At the C boundary: nothing panics, nothing allocates, nothing is trusted

**Context.** A library that aborts someone else's application over its own bug
is not one they can ship. A library that hands back memory the caller must free
is one they will leak.

**Decision.**

- **Nothing panics across it.** `extern "C"` would abort the process, which is
  not a library's decision to make about someone else's application. Every
  entry point catches; a `Ui` that panicked is *poisoned*, further calls do
  nothing, and `libgui_ui_poisoned` says so. Half a built frame is not worth
  continuing into — the container stack is already unbalanced.
- **No allocation crosses.** Strings go in as `const char*` the caller owns.
  Anything that would return one takes a buffer and a capacity and reports the
  length needed, which is `snprintf`'s contract.
- **Null is tolerated everywhere**, reported through `libgui_last_error`, and
  never fatal.

**Consequences.**

- A text field copies its string in and out each frame. For a name field that
  is nothing; for a multi-megabyte document it would matter.
- The exports are `unsafe fn`, because null is handled but a non-null invalid
  pointer is the one thing no signature can guard.

---

## 11. Invariants are enforced by tests, not by review

**Context.** Every rule above is the kind that erodes one convenience at a
time, and a rule that only lives in a document is a rule that is already being
broken somewhere.

**Decision.** Each becomes a test that fails on the commit that breaks it.

| Rule | Guard |
|---|---|
| no I/O, clock, threads, globals, `target_os` | `boundaries.rs` reads the crate's own source |
| no new dependency without a reason | an allowlist with one line of justification each |
| behavioural defaults are the app's | `policy.rs` |
| the C header matches the library | regenerated from the table; a stale one fails |
| the header matches at the *byte* level | `smoke.c` compiles it and asserts every `sizeof` |
| the C callbacks are sound | Miri, which found real UB the C tests could not |
| a steady frame costs nothing new | `FrameCost` + `Budget` |

**Consequences.**

- 439 tests, and CI on four platforms in debug and release.
- Some tests are slow. The Miri run is minutes; it stays because it is the only
  thing that can answer the question it answers.

---

## 12. Performance is measured, not asserted

**Context.** "Fast" is a claim, and claims rot.

**Decision.** The numbers in the documentation come from a measurement, and the
ones that matter are assertions. `FrameCost` reports nodes, glyphs rasterised,
strings shaped, text scanned and unkeyed duplicates; `Budget::steady(…)` turns
those into a test. `glyphs_rasterized == 0` in a settled frame means the caches
are not thrashing.

Where a decision rested on a number, the number is in the commit that made it:
a text area at rest went from 2.23 ms to 0.01 ms when the scroll became an
anchor; shaping costs 10.5 µs against fontdue's 0.26 µs, which is why it is off
by default; the C boundary is inside the noise at 600 widgets per frame, which
is why nobody should worry about it.

**Consequences.**

- Optimisations are justified rather than guessed. The anchor rewrite happened
  because the byte-scan floor was measured at 0.84 ms and that was still too
  much.
- A benchmark that flatters is worse than none. The first C-boundary run said
  the ABI was *22% faster* than native Rust, which is impossible; it was
  ordering, and running both sides twice in reverse collapsed it to zero.

---

## 13. What libgui refuses to decide

Some things look like gaps and are deliberate.

- **Modals.** There is no modal widget. What a modal *blocks* — whether the
  menu bar still works, whether Escape cancels, whether the 3D view keeps
  orbiting — is the application's question. libgui supplies layers and
  stacking; the policy is yours.
- **Trees.** `tree_row` draws a row at a depth with a disclosure arrow. Which
  nodes are expanded, what a child is, how deep it goes — the app's.
- **Fonts.** libgui reads no files and enumerates no system fonts, because both
  are filesystem access and platform policy. It takes bytes. The bundled Inter
  exists so the tests and demos have a font whose glyphs never move.
- **Locale.** `ShapeRasterizer` takes a language tag rather than reading the
  process locale, because an application that renders one document in Turkish
  and another in English cannot be served by a process-wide answer.
- **Sorting.** A table reports that a column header was clicked. It does not
  sort your data.

The test for all of these: *could two reasonable applications want different
answers?* If yes, libgui reports and the app decides.

---

## 14. Accessibility: the shape of the decision, before the work

Accessibility is the largest gap in the library and the one that blocks
shipping to the public. The architecture is settled even though the work is
not, and it follows §2.

- **The core emits the tree.** Only libgui knows what each widget is, what it
  is called, what state it is in, where it ended up and in what order focus
  visits it.
- **The core does not talk to the platform.** UIA, NSAccessibility and AT-SPI
  are the host's, through AccessKit's adapters.
- **The tree is a neutral type, not `accesskit::TreeUpdate`.** That type cannot
  cross the C ABI, and vCAD is the reason the C ABI exists. A neutral form
  serves both, and a thin `libgui_accesskit` crate absorbs AccessKit's own
  pre-1.0 churn.

Known hard parts, recorded now so they are not discovered later: immediate mode
must be diffed into incremental updates; the tree must be built only when an
assistive technology is actually attached, or it is a permanent tax; a
virtualised list must report its *true* size or a screen reader will confidently
announce the wrong one; and icon-only buttons have no name unless something
supplies one.

---

## 15. Doing less work is the app's decision, three ways

**Context.** An immediate-mode library rebuilds everything every frame, which
is its whole appeal and its obvious cost. On a laptop that cost is battery.

**Decision.** Three independent mechanisms, because they save different things
and an application knows which it needs.

1. **The window sleeps.** `needs_frame` / `repaint_after`: nothing is moving,
   so draw nothing at all.
2. **A subtree replays.** `cached` records a subtree's pixels and replays them
   while its `deps` are unchanged. `deps` is the app's, which is what makes
   "run this panel at 10 Hz" a coarse tick rather than an API.
3. **The renderer redraws without a UI frame.** `render_batches` re-issues the
   last frame's batches, so a 3D viewport animates while the UI costs nothing.

**Consequences.**

- The library never decides to skip work behind your back. Every one of these
  is something the app turns on.
- `cached` has to refuse when replaying would be wrong — pointer over it, focus
  inside, still animating, DPI or transform changed, atlas repacked, or moved
  while a pointer was inside it. Those are guards the app does not have to
  know about, and they are why it is not simply "did deps change".
- A replay survives moving but **not resizing**, which is a real limit and on
  the roadmap rather than hidden.

---

## 16. Pre-1.0, and what that means for you

The Rust API changes. `Response` gained a field recently; `ShapedGlyph` gained
one; a function's arity changed. That is what pre-1.0 means and it is fine:
Rust callers rebuild.

The C ABI cannot move like that, which is why it versions separately (§9).
`LIBGUI_ABI_VERSION` is checked once at start-up, and a mismatch is a loud
failure rather than a silent misread.

There is no changelog yet and no semver guarantee. Pin an exact version.
