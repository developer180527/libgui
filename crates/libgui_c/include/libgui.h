/* libgui — C ABI.
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

/* Struct sizes, for the static_asserts below. */
uint64_t libgui_sizeof_response(void);
uint64_t libgui_sizeof_tree_response(void);
uint64_t libgui_sizeof_text_response(void);
uint64_t libgui_sizeof_vec2(void);
uint64_t libgui_sizeof_rect(void);
uint64_t libgui_sizeof_color(void);
uint64_t libgui_sizeof_modifiers(void);

/* === BEGIN GENERATED — from src/table.rs === */

void           libgui_label(LibguiUi* ui, const char* text);
void           libgui_label_muted(LibguiUi* ui, const char* text);
void           libgui_heading(LibguiUi* ui, const char* text);
void           libgui_section(LibguiUi* ui, const char* text);
void           libgui_paragraph(LibguiUi* ui, const char* text);
void           libgui_space(LibguiUi* ui, float px);
void           libgui_flex(LibguiUi* ui);
void           libgui_separator(LibguiUi* ui);
LibguiResponse libgui_button(LibguiUi* ui, const char* label);
LibguiResponse libgui_button_primary(LibguiUi* ui, const char* label);
LibguiResponse libgui_button_keyed(LibguiUi* ui, uint64_t key, const char* label);
LibguiResponse libgui_checkbox(LibguiUi* ui, const char* label, uint8_t* value);
LibguiResponse libgui_toggle(LibguiUi* ui, const char* label, uint8_t* value);
LibguiResponse libgui_slider(LibguiUi* ui, const char* label, float* value, float min, float max);
LibguiResponse libgui_slider_vertical(LibguiUi* ui, const char* label, float* value, float min, float max, float height);
LibguiResponse libgui_drag_value(LibguiUi* ui, const char* label, float* value, float speed);
void           libgui_progress(LibguiUi* ui, const char* label, float value);
LibguiResponse libgui_selectable(LibguiUi* ui, const char* label, uint8_t selected);
LibguiResponse libgui_selectable_keyed(LibguiUi* ui, uint64_t key, const char* label, uint8_t selected);
LibguiResponse libgui_menu_item(LibguiUi* ui, const char* label);
LibguiResponse libgui_menu_item_shortcut(LibguiUi* ui, const char* label, const char* hint);
void           libgui_menu_separator(LibguiUi* ui);
uint8_t        libgui_open_menu(LibguiUi* ui, const char* label);
void           libgui_close_menu(LibguiUi* ui);
void           libgui_scroll_to(LibguiUi* ui, uint64_t id);
uint8_t        libgui_is_enabled(LibguiUi* ui);

/* === END GENERATED === */

#ifdef __cplusplus
}  /* extern "C" */
#endif

#endif /* LIBGUI_H */
