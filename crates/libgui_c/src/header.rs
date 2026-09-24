//! The C header, emitted from the same table the exports come from.
//!
//! Three ways to get a header, and this is the third:
//!
//! - **cbindgen** parses the Rust and emits one. It cannot drift, but it is a
//!   third-party parse of your source, its output is shaped however the tool
//!   decides, and it cannot say what Rust does not — which pointer may be
//!   null, who owns what, what is safe to call when.
//! - **By hand** makes the header the real interface document, ordered and
//!   commented for the person including it. But it drifts, and a drifted
//!   header is not a compile error in C: it is a call through a signature that
//!   moved, which corrupts memory somewhere else entirely.
//! - **From the table**, which is this. Not a parse of Rust — an emission from
//!   the declaration the exports are already generated from. The committed
//!   header stays reviewable in git, and a test regenerates it and fails if
//!   the two differ, so editing one side alone breaks the build.
//!
//! The hand-written part is the preamble: the types, the callback vtables and
//! the prose. Those are small, stable, and exactly the parts a generator would
//! render worse.

use crate::table::TABLE;

/// The committed header, for the drift test to compare against.
pub const HEADER_PATH: &str = "include/libgui.h";

/// Everything above the generated block, written by hand because it is where a
/// C programmer learns how this works.
const PREAMBLE: &str = r##"/* libgui — C ABI.
 *
 * GENERATED IN PART. Everything between the BEGIN/END markers below comes from
 * the table in `src/table.rs`; edit that, not this. `cargo test -p libgui_c`
 * regenerates and fails if this file is stale.
 *
 * Rules at this boundary:
 *
 *  - Nothing panics across it. A Ui that panicked is poisoned: further calls
 *    do nothing and libgui_ui_poisoned() returns 1. Tear it down and rebuild.
 *  - No allocation crosses it. Strings are `const char*` you own, NUL
 *    terminated, UTF-8. Nothing returned needs freeing.
 *  - Check libgui_abi_version() == LIBGUI_ABI_VERSION once at start-up. A
 *    mismatch means this header and the library disagree about layout.
 *  - A NULL Ui, a NULL string or a NULL out-parameter is tolerated: the call
 *    does nothing useful and libgui_last_error() says what happened. It will
 *    not crash.
 *
 * What your renderer must agree with — none of it is guessable, and getting
 * one wrong looks like a nearly-right port:
 *
 *  - BLENDING: the shader outputs PREMULTIPLIED colour. Blend with
 *    src=ONE, dst=ONE_MINUS_SRC_ALPHA.
 *  - COLOUR SPACE: colours crossing this boundary are sRGB-ENCODED with
 *    STRAIGHT alpha, and the shader premultiplies. Render into a UNORM
 *    target, not an sRGB view -- the values are already encoded, so an sRGB
 *    target encodes them twice. libgui_frame_clear_color() is in the same
 *    space: 0..1 sRGB-encoded, straight alpha.
 *  - YOUR TEXTURES: RGBA8, sRGB-encoded values, composited as OPAQUE RGB --
 *    a viewport's own alpha is ignored. The instance colour still tints it.
 *  - SAMPLING: bilinear, clamped to edge. Hardware filtering is fine; the CPU
 *    reference does the same thing by hand, which is what makes the two
 *    comparable.
 *  - ORIENTATION: top-left origin, for both the glyph atlas and your
 *    textures; uv (0,0) is the top-left texel. An API whose render targets
 *    are bottom-left (GL) needs the image turned over: swap v0 and v1 in
 *    libgui_painter_image_uv (pass 1 then 0).
 *  - NO depth test, NO culling, NO scissor: clipping is per-instance, in the
 *    fragment shader.
 *
 * And the order within a frame, which matters to anyone rendering a scene
 * into a texture the UI shows:
 *
 *     build -> libgui_end_frame -> size your targets -> render your scene
 *           -> draw the UI
 *
 * A widget's rect exists only after layout, which happens inside
 * libgui_end_frame. Render your scene BEFORE that and you are sizing it from
 * last frame's rect, which shows as the viewport lagging a frame behind while
 * a window is resized.
 */
#ifndef LIBGUI_H
#define LIBGUI_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define LIBGUI_ABI_VERSION 2u

typedef struct LibguiUi LibguiUi;

typedef struct { float x, y; } LibguiVec2;
typedef struct { float x, y, w, h; } LibguiRect;
typedef struct { float r, g, b, a; } LibguiColor;
typedef struct { uint8_t shift, ctrl, alt, logo; } LibguiModifiers;

typedef struct {
    uint64_t        id;
    LibguiRect      rect;
    uint8_t         hovered;
    uint8_t         focused;
    uint8_t         active;
    uint8_t         pressed;
    uint8_t         clicked;
    uint8_t         double_clicked;
    uint8_t         secondary_pressed;
    uint8_t         middle_pressed;
    uint8_t         has_raw_delta;
    uint8_t         _pad[7];
    LibguiVec2      drag_delta;
    LibguiVec2      raw_delta;   /* only when has_raw_delta */
    LibguiVec2      scroll;
    LibguiVec2      mouse_pos;
    float           pinch;
    float           _pad2;
    LibguiModifiers modifiers;
} LibguiResponse;

typedef struct {
    LibguiResponse response;
    uint8_t        toggled;
    uint8_t        _pad[7];
} LibguiTreeResponse;

typedef struct {
    LibguiResponse response;
    uint8_t        changed;
    uint8_t        submitted;
    uint8_t        can_undo;
    uint8_t        can_redo;
    uint8_t        cancelled;        /* Escape: put back what was there */
    uint8_t        _pad[3];
    uint64_t       caret_line;
    uint64_t       caret_column;
    uint64_t       selection_start;  /* byte offsets into the string you passed */
    uint64_t       selection_end;
} LibguiTextResponse;

/* Lifecycle. */
uint32_t    libgui_abi_version(void);
const char* libgui_last_error(void);
LibguiUi*   libgui_ui_new(const uint8_t* font_bytes, uint64_t font_len);
/* A second Ui drawing from the same font system: the same faces, the same
 * shaping caches, and ONE glyph atlas. What a docked application wants for a
 * torn-off window -- otherwise each window rasterises every glyph again and
 * you upload another copy of the same image. libgui_frame_atlas then reports
 * the same pointer and version for every window sharing it. The new Ui has its
 * own theme, input, focus and widget state; free the handles in any order. */
LibguiUi*   libgui_ui_new_sharing_fonts(LibguiUi* other);
void        libgui_ui_free(LibguiUi* ui);
uint8_t     libgui_ui_poisoned(LibguiUi* ui);
void        libgui_begin_frame(LibguiUi* ui, float width, float height, float scale, float dt);
void        libgui_end_frame(LibguiUi* ui);
int32_t     libgui_set_theme(LibguiUi* ui, const char* name);

/* --- Containers, scopes and custom painting ------------------------------
 *
 * Hand-written, because these take a closure in Rust and a closure has no C
 * spelling. libgui splits every closure-taking builder into an open_/close_
 * pair for exactly this reason, so a container is a bracket here; only custom
 * painting genuinely needs a callback.
 *
 * This part is not generated, so it *can* drift from the library. What catches
 * that is tests/smoke.c, which compiles against this header and links the real
 * static library: a declaration that no longer matches fails to build there.
 */

typedef enum {
    LIBGUI_SIZE_FIXED = 0,  /* exactly `value` logical pixels */
    LIBGUI_SIZE_FIT   = 1,  /* as large as the contents need */
    LIBGUI_SIZE_GROW  = 2   /* share what is left, weighted by `value` */
} LibguiSizeKind;

typedef struct {
    LibguiSizeKind kind;
    float          value;
} LibguiSize;

typedef struct {
    uint8_t    axis;          /* 0 = row, 1 = column */
    uint8_t    _pad[3];
    LibguiSize width;
    LibguiSize height;
    float      pad_left, pad_right, pad_top, pad_bottom;
    float      gap;
    uint8_t    align_main;    /* 0 = start, 1 = center, 2 = end */
    uint8_t    align_cross;
    uint8_t    _pad2[2];
} LibguiLayout;

typedef struct {
    LibguiColor fill;
    LibguiColor border;
    float       border_width;
    float       radius;
    uint8_t     clip;
    uint8_t     shadow;
    uint8_t     _pad[6];
} LibguiFrame;

typedef struct LibguiPainter LibguiPainter;

/* `paint` draws; `drop_user` releases `user` when the frame is over. Leave
 * `drop_user` NULL if `user` outlives the frame on its own. Either may be
 * NULL; a NULL `paint` simply draws nothing. */
typedef struct {
    void (*paint)(LibguiPainter* p, LibguiRect r, void* user);
    void (*drop_user)(void* user);
    void* user;
} LibguiPaintFn;

uint64_t       libgui_id_from_name(const char* name);
void           libgui_open_container(LibguiUi* ui, uint64_t id, LibguiLayout layout, LibguiFrame frame);
void           libgui_close_container(LibguiUi* ui);
uint64_t       libgui_open_depth(LibguiUi* ui);
void           libgui_open_scroll_area(LibguiUi* ui, const char* key);
void           libgui_close_scroll_area(LibguiUi* ui);
uint8_t        libgui_open_enabled(LibguiUi* ui, uint8_t enabled);
void           libgui_close_enabled(LibguiUi* ui, uint8_t was);
void           libgui_add_leaf(LibguiUi* ui, uint64_t id, LibguiLayout layout, uint8_t interactive, LibguiPaintFn paint);
LibguiResponse libgui_interact(LibguiUi* ui, uint64_t id);

/* --- Drawing your own widget ---------------------------------------------
 *
 * The same palette a Rust widget has. `measure` is the one you cannot do
 * without: everything else draws, that is how you decide where. */

void libgui_painter_measure(LibguiPainter* p, float size, const char* text, float* out_w, float* out_h);
/* Exactly `px` PHYSICAL pixels wide, however the display is scaled: a 1.0-wide
 * rect on a 1.5x display lands on a pixel and a half and renders as a smear.
 * This is what keeps a rule or a witness line crisp. */
void libgui_painter_hairline(LibguiPainter* p, float x, float y, float px, float height, LibguiRect* out);
void libgui_painter_snap_rect(LibguiPainter* p, LibguiRect r, LibguiRect* out);

void libgui_painter_rect(LibguiPainter* p, LibguiRect r, LibguiColor fill, float radius);
void libgui_painter_rect_bordered(LibguiPainter* p, LibguiRect r, LibguiColor fill, float radius,
                                  float border_width, LibguiColor border);
void libgui_painter_line(LibguiPainter* p, float x0, float y0, float x1, float y1, float width, LibguiColor c);
void libgui_painter_text_left(LibguiPainter* p, LibguiRect r, float size, LibguiColor c, const char* text);
void libgui_painter_text(LibguiPainter* p, float x, float y, float size, LibguiColor c, const char* text);
void libgui_painter_text_right(LibguiPainter* p, LibguiRect r, float size, LibguiColor c, const char* text);
void libgui_painter_text_centered(LibguiPainter* p, LibguiRect r, float size, LibguiColor c, const char* text);
/* align: 0 left, 1 centre, 2 right */
void libgui_painter_text_wrapped(LibguiPainter* p, LibguiRect r, float size, LibguiColor c, uint32_t align,
                                 const char* text);

void libgui_painter_shadow(LibguiPainter* p, LibguiRect r, float radius, float blur, LibguiColor c);
void libgui_painter_image_tinted(LibguiPainter* p, LibguiRect r, uint64_t texture_index,
                                 float u0, float v0, float u1, float v1, float radius, LibguiColor tint);
/* `points` is `count` pairs of floats. */
void libgui_painter_polyline(LibguiPainter* p, const float* points, uint64_t count, float width, LibguiColor c);
void libgui_painter_bezier(LibguiPainter* p, float x0, float y0, float cx0, float cy0,
                           float cx1, float cy1, float x1, float y1, float width, LibguiColor c);
/* Leaves sideways and arrives sideways, the way a node graph draws a link. */
void libgui_painter_wire(LibguiPainter* p, float from_x, float from_y, float to_x, float to_y,
                         float width, LibguiColor c);
/* dir: 0 up, 1 down, 2 left, 3 right. Two strokes, not a glyph, so it stays
 * crisp and needs no font. */
void libgui_painter_chevron(LibguiPainter* p, LibguiRect r, float size, uint32_t dir, LibguiColor c);

/* --- Caching a subtree ----------------------------------------------------
 *
 * Replay a subtree's pixels instead of building it again. Returns 1 when it
 * HAS to be built: build it, then close. Returns 0 when it was replayed —
 * build nothing, close nothing.
 *
 * `deps` is a number you choose: a revision, a hash, anything that changes
 * when the pixels would. A section that updates at its own rate is this and
 * nothing else — put a coarse tick in `deps`:
 *
 *     uint64_t tick = (uint64_t)(now * 10.0);          // ten times a second
 *     if (libgui_open_cached(ui, "telemetry", tick)) {
 *         build_telemetry_panel(ui);
 *         libgui_close_cached(ui);
 *     }
 *
 * It refuses to replay when that would be wrong and you manage none of it: the
 * pointer over it, focus inside it, still animating, the DPI or canvas
 * transform changed, the atlas repacked, or it moved while a pointer was
 * inside. A replay survives the subtree MOVING but not RESIZING. */
uint8_t libgui_open_cached(LibguiUi* ui, const char* key, uint64_t deps);
void    libgui_close_cached(LibguiUi* ui);
/* One of your own textures: `texture` is the index that comes back in
 * LibguiBatch::texture_index with texture_kind 1. For the 3D view itself,
 * libgui_viewport is the widget. */
void libgui_painter_image(LibguiPainter* p, LibguiRect r, uint64_t texture, float radius);
void libgui_painter_image_uv(LibguiPainter* p, LibguiRect r, uint64_t texture,
                             float u0, float v0, float u1, float v1,
                             float radius, LibguiColor tint);

/* --- Input -------------------------------------------------------------- */

#define LIBGUI_BUTTON_PRIMARY   0u
#define LIBGUI_BUTTON_SECONDARY 1u
#define LIBGUI_BUTTON_MIDDLE    2u
#define LIBGUI_BUTTON_BACK      3u
#define LIBGUI_BUTTON_FORWARD   4u

/* Do NOT convert before sending: libgui applies its own policy per unit, and
 * a host that pre-multiplies lines into pixels loses the distinction a
 * trackpad depends on. */
#define LIBGUI_WHEEL_PIXEL 0u
#define LIBGUI_WHEEL_LINE  1u
#define LIBGUI_WHEEL_PAGE  2u

#define LIBGUI_TOUCH_STARTED   0u
#define LIBGUI_TOUCH_MOVED     1u
#define LIBGUI_TOUCH_ENDED     2u
#define LIBGUI_TOUCH_CANCELLED 3u

/* Keys. These numbers belong to this header, not to libgui's own enum, so
 * reordering that enum cannot silently change them. */
#define LIBGUI_KEY_ARROW_LEFT  1u
#define LIBGUI_KEY_ARROW_RIGHT 2u
#define LIBGUI_KEY_ARROW_UP    3u
#define LIBGUI_KEY_ARROW_DOWN  4u
#define LIBGUI_KEY_HOME        5u
#define LIBGUI_KEY_END         6u
#define LIBGUI_KEY_PAGE_UP     7u
#define LIBGUI_KEY_PAGE_DOWN   8u
#define LIBGUI_KEY_BACKSPACE   9u
#define LIBGUI_KEY_DELETE      10u
#define LIBGUI_KEY_ENTER       11u
#define LIBGUI_KEY_NUMPAD_ENTER 12u
#define LIBGUI_KEY_TAB         13u
#define LIBGUI_KEY_ESCAPE      14u
#define LIBGUI_KEY_SPACE       15u
#define LIBGUI_KEY_SHIFT_LEFT  16u
#define LIBGUI_KEY_SHIFT_RIGHT 17u
#define LIBGUI_KEY_CTRL_LEFT   18u
#define LIBGUI_KEY_CTRL_RIGHT  19u
#define LIBGUI_KEY_ALT_LEFT    20u
#define LIBGUI_KEY_ALT_RIGHT   21u
#define LIBGUI_KEY_SUPER_LEFT  22u
#define LIBGUI_KEY_SUPER_RIGHT 23u
#define LIBGUI_KEY_A           100u   /* A..Z are 100..125 */
#define LIBGUI_KEY_LETTER(c)   (100u + (uint32_t)((c) - 'A'))
#define LIBGUI_KEY_0           130u   /* 0..9 are 130..139 */
#define LIBGUI_KEY_DIGIT(n)    (130u + (uint32_t)(n))
#define LIBGUI_KEY_F1          140u   /* F1..F12 are 140..151 */
#define LIBGUI_KEY_F(n)        (140u + (uint32_t)((n) - 1))

void libgui_push_pointer_moved(LibguiUi* ui, float x, float y);
void libgui_push_pointer_delta(LibguiUi* ui, float dx, float dy);
void libgui_push_pointer_left(LibguiUi* ui);
void libgui_push_pointer_button(LibguiUi* ui, uint32_t btn, uint8_t pressed);
void libgui_push_wheel(LibguiUi* ui, float dx, float dy, uint32_t unit);
void libgui_push_touch(LibguiUi* ui, uint64_t id, uint32_t phase, float x, float y);
void libgui_push_key(LibguiUi* ui, uint32_t key, uint8_t pressed, uint8_t repeat);
void libgui_push_modifiers(LibguiUi* ui, LibguiModifiers m);
void libgui_push_text(LibguiUi* ui, const char* text);
void libgui_push_ime_preedit(LibguiUi* ui, const char* text, uint64_t cursor);
void libgui_push_paste(LibguiUi* ui, const char* text);
void libgui_push_focus_lost(LibguiUi* ui);

/* --- Keymap ------------------------------------------------------------- */

#define LIBGUI_PLATFORM_CURRENT (-1)
#define LIBGUI_PLATFORM_MAC     0
#define LIBGUI_PLATFORM_WINDOWS 1
#define LIBGUI_PLATFORM_LINUX   2

/* libgui binds no keys itself: widgets respond to actions, and which chord
 * produces one is a platform convention. Without this a text field ignores
 * Backspace. Call once, after libgui_ui_new. */
int32_t libgui_install_default_keymap(LibguiUi* ui);
/* 0, or 1 when `platform` is not a LIBGUI_PLATFORM_* value: nothing is
 * installed and libgui_last_error says so. An unknown value is NOT quietly
 * treated as the current platform — that would put Cmd where you asked for
 * Ctrl and say nothing. */
int32_t libgui_install_keymap(LibguiUi* ui, int32_t platform);
/* Which gesture a click with these modifiers means: 0 replace, 1 toggle,
 * 2 range, or -1 when `platform` is not a LIBGUI_PLATFORM_* value. */
int32_t libgui_select_kind(int32_t platform, LibguiModifiers m);
void    libgui_select(LibguiUi* ui, uint64_t collection, uint64_t index, int32_t kind,
                      int32_t* out_kind, uint64_t* out_lo, uint64_t* out_hi);
uint8_t libgui_consume_shortcut(LibguiUi* ui, uint32_t key, LibguiModifiers m);

/* --- Frame output -------------------------------------------------------- */

typedef struct {
    /* Which of your textures, when texture_kind is 1. 64 bits, so a native
     * handle or a pointer survives the trip; it was 32 and cut them in half. */
    uint64_t texture_index;
    uint32_t texture_kind;   /* 0 = glyph atlas, 1 = one of your textures */
    uint32_t first;
    uint32_t count;
    uint32_t _pad;           /* zero */
} LibguiBatch;

typedef struct {
    float screen_width, screen_height;
    float scale;
    float _pad;
} LibguiGlobals;

typedef struct {
    uint32_t cursor;
    uint8_t  has_copied_text;
    uint8_t  paste_requested;
    uint8_t  has_text_input;
    uint8_t  wants_pointer;
    uint8_t  wants_keyboard;
    uint8_t  pointer_lock;
    uint8_t  _pad[2];
    float    repaint_after;  /* < 0: sleep until an event */
    float    text_input_x, text_input_y, text_input_w, text_input_h;
} LibguiPlatformOutput;

/* All valid until the next libgui_begin_frame. Copy what you need. */
const void*        libgui_frame_instances(LibguiUi* ui, uint64_t* out_count);
const LibguiBatch* libgui_frame_batches(LibguiUi* ui, uint64_t* out_count);
const uint8_t*     libgui_frame_atlas(LibguiUi* ui, uint32_t* out_size, uint64_t* out_version);
void               libgui_frame_globals(LibguiUi* ui, LibguiGlobals* out);
void               libgui_frame_clear_color(LibguiUi* ui, LibguiColor* out);
void               libgui_frame_platform(LibguiUi* ui, LibguiPlatformOutput* out);
const char*        libgui_frame_copied_text(LibguiUi* ui);
uint8_t            libgui_needs_frame(LibguiUi* ui, float elapsed);
uint64_t           libgui_instance_stride(void);
uint32_t           libgui_vertices_per_instance(void);

/* --- The triangle form ---------------------------------------------------
 *
 * libgui's own output is one instance per primitive, and not every renderer
 * can draw that: bgfx carries at most five vec4s of instance data and an
 * instance needs six, GLES2 and WebGL1 have no per-instance attributes at all.
 * Turn this on and every frame is also expanded into one quad per primitive —
 * four vertices, six indices — which any vertex+index draw can take.
 *
 * It costs about four and a half times the bytes, so a renderer that can
 * instance should keep instancing. Off by default.
 *
 * The vertex is what the shader's *varyings* are, already computed: porting
 * the shader is a vertex stage that moves pos into clip space and passes the
 * rest through, and a fragment stage over values that are all already there.
 * Describe the layout with libgui_vertex_attribute rather than hard-coding it.
 */
void               libgui_enable_mesh(LibguiUi* ui, uint8_t on);
const void*        libgui_mesh_vertices(LibguiUi* ui, uint64_t* out_count);
const uint32_t*    libgui_mesh_indices(LibguiUi* ui, uint64_t* out_count);
const LibguiBatch* libgui_mesh_batches(LibguiUi* ui, uint64_t* out_count);
uint8_t            libgui_mesh_fits_u16(LibguiUi* ui);

/* One upload's worth of the mesh. A renderer streaming into a fixed per-frame
 * buffer has a ceiling -- bgfx's transient buffer is 6 MB by default, about
 * thirteen thousand quads -- and a frame that goes over it does not run
 * slowly, it loses the draw call. Set a limit and the frame is cut to fit.
 *
 * Indices count from the chunk's vertex_first, and each batch's `first` counts
 * from its chunk's index_first, so the two slices upload and draw untouched:
 *
 *     uint64_t n = 0;
 *     const LibguiMeshChunk* chunks = libgui_mesh_chunks(ui, &n);
 *     for (uint64_t i = 0; i < n; i++) {
 *         upload_vertices(vertices + chunks[i].vertex_first, chunks[i].vertex_count);
 *         upload_indices(indices + chunks[i].index_first, chunks[i].index_count);
 *         for (uint32_t b = 0; b < chunks[i].batch_count; b++) {
 *             const LibguiBatch* d = &batches[chunks[i].batch_first + b];
 *             draw(d->first, d->count);
 *         }
 *     }
 */
typedef struct {
    uint32_t vertex_first, vertex_count;
    uint32_t index_first, index_count;
    uint32_t batch_first, batch_count;
} LibguiMeshChunk;

/* Zero for either means no limit, which is the default. Rounded down to whole
 * quads; below one quad is treated as one. */
void                   libgui_set_mesh_limits(LibguiUi* ui, uint32_t max_vertices, uint32_t max_indices);
const LibguiMeshChunk* libgui_mesh_chunks(LibguiUi* ui, uint64_t* out_count);
uint64_t           libgui_vertex_stride(void);
uint32_t           libgui_vertex_attribute_count(void);
uint8_t            libgui_vertex_attribute(uint32_t index, uint32_t* out_floats, uint32_t* out_offset);
/* What that attribute is: "pos", "local", "uv", "color", "border_color",
 * "clip", "params", "half_size", "seg" -- the shader's varyings, so a host can
 * match by name instead of hard-coding the order. NULL when out of range. */
const char*        libgui_vertex_attribute_name(uint32_t index);

/* --- Checking your renderer ----------------------------------------------
 *
 * A backend written for an unusual RHI is ported by hand, and a hand port is
 * *nearly* right: corners a shade off, a shadow that clips, glyphs half a
 * pixel up at 1.5x DPI. Each is invisible until something puts it beside the
 * reference. So the reference ships.
 *
 * libgui_soft renders a frame on the CPU by evaluating the same shader per
 * pixel, with IEEE-exact operations only, so it produces the same bytes on
 * every machine. Turn it on, build a scene, compare:
 *
 *     libgui_enable_reference_render(ui, 1);
 *     for (uint32_t i = 0; i < libgui_conformance_scene_count(); i++) {
 *         if (libgui_conformance_scene_needs_input(i)) continue;
 *         float w, h;
 *         libgui_conformance_scene_size(i, &w, &h);
 *         for (int pass = 0; pass < 2; pass++) {    // layout settles on 2
 *             libgui_begin_frame(ui, w, h, scale, 1.0f);
 *             libgui_conformance_build(ui, i);
 *             libgui_end_frame(ui);
 *         }
 *         my_renderer_draw(ui);
 *         uint32_t rw, rh;
 *         const uint8_t* want = libgui_reference_pixels(ui, &rw, &rh);
 *         compare(my_readback(), want, rw, rh);
 *     }
 *
 * A failure names the scene, which names the primitive.
 */
void           libgui_enable_reference_render(LibguiUi* ui, uint8_t on);
/* RGBA8, premultiplied, top-left origin, width*height*4 bytes. NULL unless
 * the reference render was on when the frame ended. Valid until the next
 * libgui_begin_frame. Your own textures draw as nothing here. */
const uint8_t* libgui_reference_pixels(LibguiUi* ui, uint32_t* out_w, uint32_t* out_h);

uint32_t       libgui_conformance_scene_count(void);
/* Owned by the library, valid until the next call. NULL when out of range. */
const char*    libgui_conformance_scene_name(uint32_t index);
uint8_t        libgui_conformance_scene_size(uint32_t index, float* out_w, float* out_h);
/* 1 when the scene needs the pointer or keyboard driven first — a hovered
 * button, a drag in flight. Skip those for a first port; the rest cover every
 * primitive. */
uint8_t        libgui_conformance_scene_needs_input(uint32_t index);
/* Build it between begin_frame and end_frame, at the size above. */
uint8_t        libgui_conformance_build(LibguiUi* ui, uint32_t index);

/* --- Text fields and pickers --------------------------------------------- */

/* The caller owns the buffer. `cap` includes the NUL. `out_len` receives the
 * length the text *is*. More than cap-1 means it did not fit -- a paste, say
 * -- and the rest is kept aside for this frame only:
 *
 *     uint64_t need = 0;
 *     LibguiTextResponse r = libgui_text_input(ui, "path", buf, cap, "", &need);
 *     if (need >= cap) {
 *         buf = realloc(buf, cap = need + 1);
 *         libgui_text_overflow(ui, r.response.id, buf, cap);
 *     }
 *
 * Do NOT call the field again instead. A second call with the same key in one
 * frame is a different, unfocused widget: it applies nothing, and the text is
 * lost. Waiting for the next frame loses it too, because that frame is fed
 * from the truncated buffer. */
LibguiTextResponse libgui_text_input(LibguiUi* ui, const char* key, char* buf, uint64_t cap,
                                     const char* placeholder, uint64_t* out_len);
LibguiTextResponse libgui_text_area(LibguiUi* ui, const char* key, char* buf, uint64_t cap,
                                    uint64_t rows, uint64_t* out_len);
/* The whole text of a field that did not fit, this frame. snprintf contract;
 * 0 when nothing overflowed. */
uint64_t           libgui_text_overflow(LibguiUi* ui, uint64_t id, char* buf, uint64_t cap);
LibguiResponse     libgui_combo(LibguiUi* ui, const char* label, uint64_t* selected,
                                const char* const* options, uint64_t count);
LibguiResponse     libgui_segmented(LibguiUi* ui, const char* key, uint64_t* selected,
                                    const char* const* options, uint64_t count);

/* --- Collection cursor ---------------------------------------------------- */

/* Read straight after libgui_open_collection, which fills them. */
uint64_t libgui_nav_cursor(void);
uint8_t  libgui_nav_moved(void);
uint8_t  libgui_nav_focused(void);
uint8_t  libgui_nav_activated(void);
uint8_t  libgui_nav_expand(void);
uint8_t  libgui_nav_collapse(void);

/* --- Docking ------------------------------------------------------------- */

typedef struct LibguiDock LibguiDock;

typedef struct { float left, right, top, bottom; } LibguiInsets;

/* How you draw panels. Every pointer may be NULL except `ui`.
 *
 * The LibguiUi* handed to `ui` is the handle you already own, pointed at the
 * borrow libgui is holding. Draw with it freely; do NOT store it past the
 * call, end the frame, free it, or call libgui_dock_show again — those three
 * are refused with an error rather than corrupting the walk in progress. */
typedef struct {
    void    (*title)(uint64_t tab, char* buf, uint64_t cap, void* user);
    void    (*ui)(LibguiUi* ui, uint64_t tab, void* user);
    uint8_t (*scroll)(uint64_t tab, void* user);            /* NULL = yes */
    void    (*padding)(uint64_t tab, LibguiInsets* out, void* user);
    void*   user;
} LibguiTabViewer;

/* One window the host should have. */
typedef struct {
    uint64_t   id;
    uint8_t    floating;
    /* Read this before creating a window. A tab dragged out of its bar becomes
     * a HIDDEN floating surface at once, so that dropping it into another
     * panel never builds and destroys a window, a Ui and a renderer between
     * the drop and the frame that shows the result. Only a tab dragged onto
     * the desktop becomes visible. */
    uint8_t    visible;
    uint8_t    has_window_pos;
    uint8_t    _pad;
    LibguiVec2 window_pos;    /* inner top-left, physical screen px */
    LibguiVec2 window_size;   /* initial inner size, logical px */
    float      rect_x, rect_y, rect_w, rect_h;  /* in-app floating placement */
    uint64_t   tab_count;
} LibguiSurface;

LibguiDock* libgui_dock_new(void);
void        libgui_dock_free(LibguiDock* dock);
void        libgui_dock_set_in_app_floating(LibguiDock* dock, uint8_t in_app);

/* Building a layout. Node handles are consumed when used. */
uint64_t libgui_dock_leaf(LibguiDock* dock, uint64_t tab);
uint64_t libgui_dock_split(LibguiDock* dock, uint32_t axis, float fraction, uint64_t first, uint64_t second);
int32_t  libgui_dock_set_root(LibguiDock* dock, uint64_t surface, uint64_t node);
void     libgui_dock_add_tab(LibguiDock* dock, uint64_t surface, uint64_t tab);

/* Once per loop iteration, in this order. */
void     libgui_dock_set_pointer(LibguiDock* dock, float x, float y, uint8_t down);
void     libgui_dock_set_surface_frame(LibguiDock* dock, uint64_t surface, float origin_x, float origin_y, float scale);
void     libgui_dock_update(LibguiDock* dock);
uint64_t libgui_dock_surface_count(LibguiDock* dock);
int32_t  libgui_dock_surface_at(LibguiDock* dock, uint64_t i, LibguiSurface* out);
void     libgui_dock_show(LibguiDock* dock, LibguiUi* ui, uint64_t surface, const LibguiTabViewer* viewer);

void     libgui_dock_close_surface(LibguiDock* dock, uint64_t surface);
uint8_t  libgui_dock_is_dragging(LibguiDock* dock);
void     libgui_dock_cancel_drag(LibguiDock* dock);

/* Layout persistence. snprintf's contract: call with cap 0 to learn the size,
 * allocate, call again. Returns the length needed, or -1 on failure. */
int64_t  libgui_dock_layout_to_toml(LibguiDock* dock, char* buf, uint64_t cap);
int32_t  libgui_dock_restore_from_toml(LibguiDock* dock, const char* toml);

/* --- Tables --------------------------------------------------------------- */

typedef struct LibguiTable LibguiTable;

typedef struct {
    uint8_t  sort_changed;
    uint8_t  sort_descending;
    uint8_t  row_clicked;
    uint8_t  column_resized;
    uint8_t  _pad[4];
    uint64_t sort_column;
    uint64_t clicked_row;
    uint64_t resized_column;
    uint64_t first_row;   /* a table virtualises: these are the rows actually */
    uint64_t row_count;   /* built, not the total */
} LibguiTableResponse;

LibguiTable* libgui_table_new(void);
void         libgui_table_free(LibguiTable* table);
void         libgui_table_add_column(LibguiTable* table, const char* title, float width, float grow,
                                     uint8_t resizable, uint8_t sortable, uint32_t align);
void         libgui_table_clear_columns(LibguiTable* table);
void         libgui_table_set_frozen(LibguiTable* table, uint64_t frozen);
/* `cell` is called per visible cell. The LibguiUi* is the handle you own —
 * same rules as a dock panel. */
void         libgui_table_show(LibguiUi* ui, LibguiTable* table, const char* key, uint64_t rows,
                               void (*cell)(LibguiUi* ui, uint64_t row, uint64_t col, void* user),
                               void* user, LibguiTableResponse* out);

/* --- Themes --------------------------------------------------------------- */

/* Load a palette from TOML. This is how an existing theme ports without
 * retyping a colour. Errors name the section and the field. */
int32_t libgui_set_theme_toml(LibguiUi* ui, const char* toml);
/* Every value resolved, as a reference to copy from. snprintf's contract. */
int64_t libgui_theme_to_toml(LibguiUi* ui, char* buf, uint64_t cap);

/* --- Drag and drop -------------------------------------------------------- */

typedef struct {
    uint8_t    hovered;
    uint8_t    dropped;
    uint8_t    _pad[6];
    uint64_t   value;      /* what libgui_drag_source was given */
    LibguiRect rect;
    float      pointer_x, pointer_y;
} LibguiDropZone;

/* `value` is yours to interpret — a body id, a row index, a pointer you cast.
 * libgui carries it and hands it back at the drop. Returns 1 while this widget
 * is the one being dragged. */
uint8_t     libgui_drag_source(LibguiUi* ui, uint64_t id, const char* kind, uint64_t value, const char* label);
void        libgui_drop_zone(LibguiUi* ui, const char* const* kinds, uint64_t count, LibguiDropZone* out);
const char* libgui_dragging(LibguiUi* ui);   /* NULL when nothing is */
void        libgui_cancel_drag(LibguiUi* ui);

/* --- Fields the app validates -------------------------------------------------- */

/* Asked, on commit, whether `text` is acceptable. Return 1 to accept. To
 * refuse, return 0, write the reason into `error` (truncation is fine) and, if
 * you know it, the byte offset of the problem into *error_at -- it arrives as
 * UINT64_MAX, meaning none. `text` is NUL-terminated, `len` bytes, and valid
 * only for the call. DO NOT call libgui from here with the same handle:
 * libgui is mid-widget, and such calls are refused. */
typedef uint8_t (*LibguiValidateFn)(void* user, const char* text, uint64_t len,
                                    char* error, uint64_t error_cap, uint64_t* error_at);

typedef struct {
    const char* display;          /* shown while nobody edits: "40 mm" for "width * 2". NULL: the source */
    const char* placeholder;
    uint8_t     select_on_focus;  /* 1 by default: typing replaces the entry */
    uint8_t     _pad[7];
    char*       error;            /* the current refusal's reason, "" when none. May be NULL. */
    uint64_t    error_cap;
} LibguiValidatedOptions;

void libgui_validated_options_default(LibguiValidatedOptions* out);

typedef struct {
    LibguiResponse response;
    uint8_t        committed;     /* accepted new text was written to buf: act on this one */
    uint8_t        changed;       /* the field was edited; buf was not */
    uint8_t        cancelled;     /* Escape threw the edit away */
    uint8_t        focused;
    uint8_t        has_error;     /* the field holds refused text; buf is untouched */
    uint8_t        _pad[3];
    uint64_t       error_at;      /* the validator's byte offset, or UINT64_MAX */
} LibguiValidatedResponse;

/* A text field whose buffer changes only when YOUR validator accepts the edit.
 * libgui owns the behaviour; you own the question -- your grammar, your names
 * resolved however you like, your units.
 *
 * `buf` is your SOURCE text, written only on an accepted commit: Enter, Tab or
 * a click elsewhere. Escape throws the edit away. Refused text stays in the
 * field with the reason beneath it; a refused Enter keeps focus, with the
 * caret at error_at. Editing always starts from the source, never the
 * display, so a parametric link survives an edit. A NULL validator accepts
 * everything. A committed text longer than `cap` is fetched with
 * libgui_text_overflow, as for libgui_text_input.
 *
 *     static uint8_t check(void* doc, const char* t, uint64_t n,
 *                          char* err, uint64_t cap, uint64_t* at) {
 *         return my_expr_parse((Doc*)doc, t, err, cap, at);
 *     }
 *     LibguiValidatedOptions o;
 *     libgui_validated_options_default(&o);
 *     o.display = value_text;                  // "40 mm"
 *     if (libgui_validated_input(ui, "height", src, sizeof src, &o, check, doc, NULL).committed)
 *         set_parameter(doc, "height", src);   // the expression, not the number
 */
LibguiValidatedResponse libgui_validated_input(LibguiUi* ui, const char* key, char* buf, uint64_t cap,
                                               const LibguiValidatedOptions* opts,
                                               LibguiValidateFn validate, void* user,
                                               uint64_t* out_len);

/* --- An evaluator with units, for an app with no grammar of its own ----------- */

/* NOT libgui: the libgui_units companion, bundled so a C host need not link a
 * second library. What an expression means is an application's decision; this
 * is one answer, usable as a validator above. An app with its own evaluator
 * ignores all of it. Every unit is a factor to one BASE unit, and values leave
 * in base units. */
typedef struct LibguiUnits LibguiUnits;

LibguiUnits* libgui_units_none(void);           /* arithmetic only */
LibguiUnits* libgui_units_length_mm(void);      /* mm cm m um µm in " ft '; shown in mm */
LibguiUnits* libgui_units_angle_deg(void);      /* deg ° rad; shown in ° */
LibguiUnits* libgui_units_new(const char* base);
/* One `name` is `factor` base units. The same factor again is an alias. */
void         libgui_units_add(LibguiUnits* units, const char* name, double factor);
/* Show values in `name`, and read a bare number as `name`. */
void         libgui_units_set_display(LibguiUnits* units, const char* name);
void         libgui_units_free(LibguiUnits* units);

/* A name an expression may use. value is in base units; dim is 1 for a
 * quantity of the table's kind (a length) and 0 for a count or a ratio. */
typedef struct {
    const char* name;
    double      value;
    int32_t     dim;
    uint32_t    _pad;
} LibguiVar;

/* Returns 1 and writes *out_value on success; 0 with the reason in err and its
 * byte offset in *out_err_at otherwise. Any output may be NULL. The signature
 * after `text` is shaped so it can sit inside a LibguiValidateFn.
 *
 * Numbers, + - * / and parentheses, units after a number, the names in
 * `vars`, and pi. A bare number is in the display unit. 3/8" is three eighths
 * of an inch. w*h in a length table is refused as an area. No functions and
 * no implicit multiplication: 2w is an error. */
uint8_t  libgui_units_eval(const LibguiUnits* units, const char* text,
                           const LibguiVar* vars, uint64_t var_count,
                           double* out_value, char* err, uint64_t err_cap, uint64_t* out_err_at);
/* snprintf contract: returns the length needed, writes what fits. */
uint64_t libgui_units_format(const LibguiUnits* units, double value, uint32_t decimals,
                             char* buf, uint64_t cap);

/* --- Canvas, transforms and animation -------------------------------------- */

/* Where the view sits over an unbounded canvas. The app owns this across
 * frames; libgui writes `visible` each frame and the app may write the rest to
 * drive the view itself (zoom to fit, a zoom box). */
typedef struct {
    LibguiVec2 pan;          /* canvas origin from the widget's top-left, window px */
    float      zoom;
    float      min_zoom, max_zoom;
    uint8_t    wheel_zooms;  /* 1: wheel zooms (a sketch). 0: it scrolls (a timeline). */
    LibguiRect visible;      /* written each frame, in canvas coordinates */
} LibguiCanvasState;

typedef struct {
    LibguiRect visible;      /* in canvas coordinates: cull against it */
    float      zoom;
    LibguiVec2 xform_pan;    /* window = canvas * xform_zoom + xform_pan */
    float      xform_zoom;
} LibguiCanvasView;

void libgui_canvas_state_default(LibguiCanvasState* state);

/* THIS IS WHAT A SKETCHER IS BUILT ON. Widgets built between open and close
 * lay out, hit-test and report their rect and drag deltas in CANVAS
 * coordinates, so snapping, hit tolerance and the rest of your logic is
 * written once in model units and is correct at any zoom. Text is rasterised
 * at the zoomed size rather than scaled up.
 *
 * The wheel zooms toward the pointer, the middle button pans, two fingers pan
 * and pinch about their midpoint. The response returned is the BACKGROUND's,
 * so `clicked` on it means the user clicked empty canvas.
 *
 *     LibguiCanvasState view;
 *     libgui_canvas_state_default(&view);          // once, kept across frames
 *
 *     LibguiCanvasView v;
 *     LibguiResponse bg = libgui_open_canvas(ui, "sketch", &view, &v);
 *     for (size_t i = 0; i < n; i++) {
 *         if (!overlaps(ent[i].bounds, v.visible)) continue;    // cull
 *         draw_entity(ui, &ent[i]);
 *     }
 *     libgui_close_canvas(ui);
 *     if (bg.clicked) deselect_all();
 */
LibguiResponse libgui_open_canvas(LibguiUi* ui, const char* key,
                                  LibguiCanvasState* state, LibguiCanvasView* out);
void           libgui_close_canvas(LibguiUi* ui);

/* The same, without the input handling: a transform you drive yourself, for a
 * view libgui should not manage. zoom is clamped away from zero. */
void libgui_open_transform(LibguiUi* ui, uint64_t id, LibguiVec2 pan, float zoom);
void libgui_close_transform(LibguiUi* ui);

LibguiVec2 libgui_transform_point(LibguiVec2 pan, float zoom, LibguiVec2 p);
LibguiVec2 libgui_transform_inv_point(LibguiVec2 pan, float zoom, LibguiVec2 p);
/* Zoom about a window position, keeping the canvas point under it still: what
 * a zoom button calls. `origin` is the canvas widget's top-left in window
 * coordinates, which is the rect a previous frame reported for it. */
void libgui_canvas_zoom_at(LibguiCanvasState* state, LibguiVec2 window_pos,
                           LibguiVec2 origin, float factor);

/* HOW A CUSTOM WIDGET MOVES. A value is retained per (id, slot) -- 256 slots
 * per widget -- and eases towards the target you pass each frame. The rate is
 * frame-rate independent: the same motion at 60 and 144 Hz, and right across a
 * dropped frame. libgui asks the host for another frame until it arrives,
 * which is what `repaint_after` in the frame reports.
 *
 *     float hot = libgui_animate_bool(ui, id, 0, resp.hovered);
 */
float libgui_animate(LibguiUi* ui, uint64_t id, uint8_t slot, float target);
float libgui_animate_bool(LibguiUi* ui, uint64_t id, uint8_t slot, uint8_t on);
/* speed is 1/s; higher is snappier. */
float libgui_animate_with_speed(LibguiUi* ui, uint64_t id, uint8_t slot, float target, float speed);
/* Jump to a value; it eases from there towards its next target. */
void  libgui_set_anim(LibguiUi* ui, uint64_t id, uint8_t slot, float value);
/* Ask for another frame for a reason libgui cannot see: your own simulation is
 * running, a file finished loading, a tool is mid-gesture. */
void  libgui_request_repaint(LibguiUi* ui);
/* Keep an id's retained state alive for a frame in which no widget with that
 * id was built -- a row scrolled out of a list, a panel behind a tab. libgui
 * forgets an id it did not see. The libgui_animate* calls do this for you. */
void  libgui_keep_id(LibguiUi* ui, uint64_t id);

/* --- Popups, layers and modals -------------------------------------------- */

#define LIBGUI_LAYER_WINDOW  0u
#define LIBGUI_LAYER_POPUP   1u
#define LIBGUI_LAYER_TOOLTIP 2u

/* Open anchored to a widget's rect: the popup appears beneath it and flips up
 * when there is no room. Then build the body on the frames where
 * libgui_open_popup_body returns 1. */
void    libgui_open_popup(LibguiUi* ui, uint64_t id, LibguiRect anchor);
/* A submenu: opens id while keeping parent open, where libgui_open_popup
 * would replace it. Does nothing if parent is not open. */
void    libgui_open_child_popup(LibguiUi* ui, uint64_t parent, uint64_t id, LibguiRect anchor);
uint8_t libgui_open_popup_body(LibguiUi* ui, uint64_t id, float min_width);
void    libgui_close_popup_body(LibguiUi* ui);
uint8_t libgui_popup_open(LibguiUi* ui, uint64_t id);
/* Check before acting on your own shortcuts, so a chord typed into an open
 * menu does not also fire a command. */
uint8_t libgui_any_popup_open(LibguiUi* ui);
void    libgui_close_popup(LibguiUi* ui, uint64_t id);
void    libgui_close_popups(LibguiUi* ui);

/* A container at an explicit rect, above the window's flow content.
 *
 * THIS IS HOW YOU BUILD A MODAL, because libgui has none: a layer covering the
 * window with a translucent fill as the scrim, then a second at the dialog's
 * rect. What a modal *blocks* — whether the menu bar still works, whether
 * Escape cancels — is your question, so libgui supplies the stacking and
 * leaves the policy alone. */
void libgui_open_layer(LibguiUi* ui, uint64_t id, uint32_t z, LibguiRect rect, LibguiFrame frame);
void libgui_close_layer(LibguiUi* ui);

/* --- Font fallback --------------------------------------------------------- */

/* Without a chain, scripts the first font lacks render as NOTHING — the
 * bundled Inter has no CJK, Arabic, Indic or emoji glyphs, so a name typed in
 * Japanese comes out blank rather than as boxes. Line metrics come from the
 * first face, so adding a CJK fallback does not change the height of a line of
 * Latin. Which fonts go in the chain is yours: libgui reads no files. */
LibguiUi* libgui_ui_new_with_fallbacks(const uint8_t* const* fonts, const uint64_t* lens, uint64_t count);

/* Struct sizes, for the static_asserts below. */
uint64_t libgui_sizeof_response(void);
uint64_t libgui_sizeof_tree_response(void);
uint64_t libgui_sizeof_text_response(void);
uint64_t libgui_sizeof_vec2(void);
uint64_t libgui_sizeof_rect(void);
uint64_t libgui_sizeof_color(void);
uint64_t libgui_sizeof_modifiers(void);
uint64_t libgui_sizeof_batch(void);
uint64_t libgui_sizeof_globals(void);
uint64_t libgui_sizeof_platform_output(void);
uint64_t libgui_sizeof_surface(void);
uint64_t libgui_sizeof_insets(void);
uint64_t libgui_sizeof_table_response(void);
uint64_t libgui_sizeof_drop_zone(void);
uint64_t libgui_sizeof_var(void);
uint64_t libgui_sizeof_validated_options(void);
uint64_t libgui_sizeof_validated_response(void);
"##;

const FOOTER: &str = r##"
#ifdef __cplusplus
}  /* extern "C" */
#endif

#endif /* LIBGUI_H */
"##;

/// A widget's Rust doc comment as a C block comment.
///
/// The same words reach both outputs, so a C programmer is not left with a
/// wall of bare prototypes while the Rust side is documented. Only the
/// spelling changes: rustdoc link and code syntax means nothing in C, and a
/// `*/` inside the text would end the comment early.
fn doc_comment(docs: &[&str]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in docs {
        // `/// text` arrives as " text"; fences carry no meaning here.
        let line = line.strip_prefix(' ').unwrap_or(line);
        if line.trim_start().starts_with("```") {
            continue;
        }
        lines.push(
            line.replace("[`", "")
                .replace("`]", "")
                .replace('`', "")
                // Never let the text close the comment it is inside.
                .replace("*/", "* /"),
        );
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    // A one-liner on one line; most of the table is one-liners.
    if lines.len() == 1 {
        return format!("/* {} */\n", lines[0].trim());
    }
    let mut out = String::from("/*");
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            out.push(' ');
        } else {
            out.push_str(" *");
            if !line.is_empty() {
                out.push(' ');
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    // Close on its own line, so adding a line to the doc is a one-line diff.
    out.push_str(" */\n");
    out
}

/// Render the header this build would produce.
pub fn render() -> String {
    let mut out = String::from(PREAMBLE);
    out.push_str("\n/* === BEGIN GENERATED — from src/table.rs === */\n\n");
    // Widest return type and name, so the declarations line up in a column the
    // way a hand-written header would.
    let ret_w = TABLE.iter().map(|(_, r, _, _)| r.len()).max().unwrap_or(0);
    for (name, ret, params, docs) in TABLE {
        let mut args = String::from("LibguiUi* ui");
        for (p, ty) in *params {
            args.push_str(&format!(", {ty} {p}"));
        }
        out.push_str(&doc_comment(docs));
        out.push_str(&format!("{ret:<ret_w$} {name}({args});\n\n"));
    }
    out.push_str("\n/* === END GENERATED === */\n");
    out.push_str(FOOTER);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Most of the table is one-liners, and a lone `*/` under each would
    /// double the length of the header for nothing.
    #[test]
    fn a_one_line_doc_is_a_one_line_comment() {
        assert_eq!(doc_comment(&[" A heading."]), "/* A heading. */\n");
    }

    #[test]
    fn a_longer_doc_keeps_its_shape() {
        let out = doc_comment(&[" First line.", "", " Second paragraph."]);
        assert_eq!(out, "/* First line.\n *\n * Second paragraph.\n */\n");
    }

    /// The one way documentation could break the file it is written into.
    #[test]
    fn a_comment_terminator_in_the_text_cannot_end_the_comment() {
        let out = doc_comment(&[" Divide with /* and */ carefully.", " More."]);
        assert!(!out.contains("*/\n *"), "the comment ended early:\n{out}");
        assert_eq!(out.matches("*/").count(), 1, "exactly one terminator, the real one:\n{out}");
    }

    /// rustdoc syntax means nothing in C: links and backticks are noise to a
    /// reader who cannot click them.
    #[test]
    fn rustdoc_syntax_is_spelled_the_way_c_reads_it() {
        let out = doc_comment(&[" See [`libgui_close_menu`] and `key`."]);
        assert_eq!(out, "/* See libgui_close_menu and key. */\n");
    }

    /// A fenced block is Rust's way of marking an example; the example itself
    /// is worth keeping, the fence is not.
    #[test]
    fn code_fences_are_dropped_and_their_contents_kept() {
        let out = doc_comment(&[" Like this:", "", " ```c", " libgui_label(ui, \"hi\");", " ```"]);
        assert!(out.contains("libgui_label(ui,"), "the example was lost:\n{out}");
        assert!(!out.contains("```"), "a fence survived:\n{out}");
    }
}

