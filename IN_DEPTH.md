# libgui in depth

Longer notes on parts of libgui that the [manual](MANUAL.md) only summarises:
how text undo is split between libgui and the app, how wheel scrolling is
smoothed, how performance is guarded and measured, and how golden-image tests
work. [`DESIGN.md`](DESIGN.md) has the reasons behind the larger decisions.

## Undo is two different things, and only one of them is libgui's

**Your document's undo is yours.** libgui does not know what an extrude, a keyframe or a transaction
is, and it never touches your model.

**A focused field's undo is the field's.** With a caret in a text box, Cmd/Ctrl+Z has to take back the
*typing* — that is what every OS text control does, and an app that sends the chord to its document
while someone is mid-word destroys their work.

They never meet, by the same rule the clipboard already follows: **while a field has focus the chord
is the field's and `consume_shortcut` refuses it; the rest of the time your app gets it.** Your CAD's
undo stack does not have to know that a text box exists, and a text box does not have to push every
keystroke into your CAD's undo stack.

The field's history coalesces a burst of typing into one step, keeps deletes as their own run, breaks
on a caret move or a click, restores the caret along with the text, and is bounded (64 steps, 256 KB)
because it is retained state on a widget id. If your app writes the string itself — its own undo, a
reload, a value shared with another widget — the field notices on the next frame and drops a history
that described a buffer which no longer exists.

## Smoothing is policy, not a guess about your hardware

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
| A frame with a pane edge being dragged, vs a still one | **0.85x** — 57 µs, 0.34% of a 60 fps frame |
| A fully open 100,100-node tree vs a 100-node one | **0.80x** — 13 µs a frame; only the rows in view are built |
| A dragged edge, a dragged dock split, a live resize: border pixels vs at rest | **identical** every frame, 1x–2x, instanced and triangles |

They assert properties that hold on any machine: deterministic counts, and
*ratios* for complexity (4x the widgets must not cost more than 7x the time,
where linear is 4x) rather than wall-clock times that flake on a loaded CI box.
The one absolute budget is asserted in release only, since a debug build is an
order of magnitude slower. `crates/libgui_bench` is the measuring tool —
`--bin app` times a CAD-shaped window while a user drags, resizes, scrolls and
types (about 0.07 ms a frame in every case) — and these are the regression
guards.

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
glyphs either, and `crates/demo_gui/libgui_solaris/tests/resize.rs` drags the dense
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
