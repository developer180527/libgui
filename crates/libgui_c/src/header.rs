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
 */
#ifndef LIBGUI_H
#define LIBGUI_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

#define LIBGUI_ABI_VERSION 1u

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
    uint8_t        _pad[4];
    uint64_t       caret_line;
    uint64_t       caret_column;
    uint64_t       selection_start;  /* byte offsets into the string you passed */
    uint64_t       selection_end;
} LibguiTextResponse;

/* Lifecycle. */
uint32_t    libgui_abi_version(void);
const char* libgui_last_error(void);
LibguiUi*   libgui_ui_new(const uint8_t* font_bytes, uint64_t font_len);
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

void libgui_painter_rect(LibguiPainter* p, LibguiRect r, LibguiColor fill, float radius);
void libgui_painter_rect_bordered(LibguiPainter* p, LibguiRect r, LibguiColor fill, float radius,
                                  float border_width, LibguiColor border);
void libgui_painter_line(LibguiPainter* p, float x0, float y0, float x1, float y1, float width, LibguiColor c);
void libgui_painter_text_left(LibguiPainter* p, LibguiRect r, float size, LibguiColor c, const char* text);

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
int32_t libgui_install_keymap(LibguiUi* ui, int32_t platform);
int32_t libgui_select_kind(int32_t platform, LibguiModifiers m);
void    libgui_select(LibguiUi* ui, uint64_t collection, uint64_t index, int32_t kind,
                      int32_t* out_kind, uint64_t* out_lo, uint64_t* out_hi);
uint8_t libgui_consume_shortcut(LibguiUi* ui, uint32_t key, LibguiModifiers m);

/* --- Frame output -------------------------------------------------------- */

typedef struct {
    uint32_t texture_kind;   /* 0 = glyph atlas, 1 = one of your textures */
    uint32_t texture_index;
    uint32_t first;
    uint32_t count;
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

/* --- Text fields and pickers --------------------------------------------- */

/* The caller owns the buffer. `cap` includes the NUL. `out_len` receives the
 * length the text *is*: more than cap-1 means it was truncated, so grow the
 * buffer and call again next frame. */
LibguiTextResponse libgui_text_input(LibguiUi* ui, const char* key, char* buf, uint64_t cap,
                                     const char* placeholder, uint64_t* out_len);
LibguiTextResponse libgui_text_area(LibguiUi* ui, const char* key, char* buf, uint64_t cap,
                                    uint64_t rows, uint64_t* out_len);
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
void     libgui_scroll_to_id(LibguiUi* ui, uint64_t id);

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
"##;

const FOOTER: &str = r##"
#ifdef __cplusplus
}  /* extern "C" */
#endif

#endif /* LIBGUI_H */
"##;

/// Render the header this build would produce.
pub fn render() -> String {
    let mut out = String::from(PREAMBLE);
    out.push_str("\n/* === BEGIN GENERATED — from src/table.rs === */\n\n");
    // Widest return type and name, so the declarations line up in a column the
    // way a hand-written header would.
    let ret_w = TABLE.iter().map(|(_, r, _)| r.len()).max().unwrap_or(0);
    for (name, ret, params) in TABLE {
        let mut args = String::from("LibguiUi* ui");
        for (p, ty) in *params {
            args.push_str(&format!(", {ty} {p}"));
        }
        out.push_str(&format!("{ret:<ret_w$} {name}({args});\n"));
    }
    out.push_str("\n/* === END GENERATED === */\n");
    out.push_str(FOOTER);
    out
}
