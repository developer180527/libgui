/* Compiled against the committed header and linked to the real static library.
 *
 * This is the half the Rust tests cannot do: it proves the header a C++ host
 * includes actually describes the library it links, that every struct is the
 * size both sides think it is, and that the declarations compile as C.
 */
#include "libgui.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures = 0;

#define CHECK(cond, msg)                                            \
    do {                                                            \
        if (!(cond)) {                                              \
            printf("FAIL: %s\n", msg);                              \
            failures++;                                             \
        }                                                           \
    } while (0)

#define CHECK_SIZE(type, fn)                                                       \
    do {                                                                           \
        uint64_t lib = fn();                                                       \
        if (sizeof(type) != (size_t)lib) {                                         \
            printf("FAIL: %s is %zu bytes here and %llu in the library\n",          \
                   #type, sizeof(type), (unsigned long long)lib);                  \
            failures++;                                                            \
        }                                                                          \
    } while (0)

static int painted = 0;

/* --- docking ------------------------------------------------------------- */
#define TAB_MODEL      1001u
#define TAB_PROPERTIES 1002u
#define TAB_VIEWPORT   1003u

static int panels_drawn = 0;
static int reentrancy_refused = 0;

static void dock_title(uint64_t tab, char* buf, uint64_t cap, void* user) {
    const char* t = tab == TAB_MODEL ? "Model" : tab == TAB_PROPERTIES ? "Properties" : "Viewport";
    (void)user;
    strncpy(buf, t, (size_t)cap - 1);
    buf[cap - 1] = 0;
}

static void dock_panel(LibguiUi* ui, uint64_t tab, void* user) {
    CHECK(user == (void*)0x77, "the viewer's user pointer did not survive");
    panels_drawn++;
    /* Draw with the handle we were given: this is the re-entrancy that had to
     * be made sound. */
    libgui_label(ui, tab == TAB_MODEL ? "Two bodies" : "Fillet 2mm");
    libgui_button(ui, "Apply");

    /* And the three calls that must be refused rather than corrupt the walk. */
    uint64_t before = libgui_open_depth(ui);
    libgui_end_frame(ui);
    if (libgui_open_depth(ui) == before && !libgui_ui_poisoned(ui)) reentrancy_refused++;
}

static int cells_filled = 0;
static void table_cell(LibguiUi* ui, uint64_t row, uint64_t col, void* user) {
    char buf[64];
    CHECK(user == (void*)0x99, "the table's user pointer did not survive");
    cells_filled++;
    snprintf(buf, sizeof(buf), "r%llu c%llu", (unsigned long long)row, (unsigned long long)col);
    libgui_label(ui, buf);
}

static uint8_t dock_scroll(uint64_t tab, void* user) {
    (void)user;
    return tab == TAB_VIEWPORT ? 0 : 1;   /* a viewport scrolls itself */
}


static void paint_cb(LibguiPainter* p, LibguiRect r, void* user) {
    LibguiColor red = { 1.0f, 0.0f, 0.0f, 1.0f };
    painted++;
    CHECK(user == (void*)0x1234, "the user pointer did not survive the trip");
    libgui_painter_rect(p, r, red, 2.0f);
}

int main(int argc, char** argv) {
    if (argc < 2) {
        printf("FAIL: no font path given\n");
        return 1;
    }
    /* The ABI handshake a host does once at start-up. */
    CHECK(libgui_abi_version() == LIBGUI_ABI_VERSION, "header and library disagree on the ABI version");

    /* Every struct that crosses. A field added on one side only lands here. */
    CHECK_SIZE(LibguiVec2, libgui_sizeof_vec2);
    CHECK_SIZE(LibguiRect, libgui_sizeof_rect);
    CHECK_SIZE(LibguiColor, libgui_sizeof_color);
    CHECK_SIZE(LibguiModifiers, libgui_sizeof_modifiers);
    CHECK_SIZE(LibguiResponse, libgui_sizeof_response);
    CHECK_SIZE(LibguiTreeResponse, libgui_sizeof_tree_response);
    CHECK_SIZE(LibguiTextResponse, libgui_sizeof_text_response);
    CHECK_SIZE(LibguiBatch, libgui_sizeof_batch);
    CHECK_SIZE(LibguiGlobals, libgui_sizeof_globals);
    CHECK_SIZE(LibguiPlatformOutput, libgui_sizeof_platform_output);
    CHECK_SIZE(LibguiSurface, libgui_sizeof_surface);
    CHECK_SIZE(LibguiInsets, libgui_sizeof_insets);
    CHECK_SIZE(LibguiTableResponse, libgui_sizeof_table_response);
    CHECK_SIZE(LibguiDropZone, libgui_sizeof_drop_zone);

    /* Load the font the tests use. */
    FILE* f = fopen(argv[1], "rb");
    if (!f) {
        printf("FAIL: cannot open %s\n", argv[1]);
        return 1;
    }
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t* font = (uint8_t*)malloc((size_t)len);
    if (fread(font, 1, (size_t)len, f) != (size_t)len) {
        printf("FAIL: short read\n");
        return 1;
    }
    fclose(f);

    LibguiUi* ui = libgui_ui_new(font, (uint64_t)len);
    CHECK(ui != NULL, "libgui_ui_new returned null");
    if (!ui) return 1;
    CHECK(libgui_ui_poisoned(ui) == 0, "a fresh Ui is poisoned");

    /* A frame, the way a host builds one. */
    uint8_t visible = 1;
    float radius = 0.25f;
    LibguiLayout panel;
    memset(&panel, 0, sizeof(panel));
    panel.axis = 1;                      /* column */
    panel.width.kind = 2;                /* Grow */
    panel.width.value = 1.0f;
    panel.height.kind = 1;               /* Fit */
    panel.gap = 4.0f;
    LibguiFrame bg;
    memset(&bg, 0, sizeof(bg));
    bg.fill.a = 1.0f;
    bg.radius = 4.0f;
    bg.clip = 1;

    libgui_begin_frame(ui, 400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
    libgui_open_container(ui, libgui_id_from_name("panel"), panel, bg);
    libgui_heading(ui, "Model");
    libgui_label(ui, "Two bodies selected");
    libgui_checkbox(ui, "Visible", &visible);
    libgui_slider(ui, "Fillet", &radius, 0.0f, 1.0f);

    /* A command that does not apply: greyed out, and inert. */
    uint8_t was = libgui_open_enabled(ui, 0);
    CHECK(libgui_is_enabled(ui) == 0, "the disabled scope did not take");
    LibguiResponse join = libgui_button(ui, "Join");
    CHECK(join.clicked == 0, "a disabled button reported a click");
    libgui_close_enabled(ui, was);
    CHECK(libgui_is_enabled(ui) == 1, "the disabled scope did not restore");

    /* A widget the host draws itself. */
    LibguiLayout leaf;
    memset(&leaf, 0, sizeof(leaf));
    leaf.width.kind = 0;  /* Fixed */
    leaf.width.value = 60.0f;
    leaf.height.kind = 0;
    leaf.height.value = 24.0f;
    LibguiPaintFn cb;
    cb.paint = paint_cb;
    cb.drop_user = NULL;
    cb.user = (void*)0x1234;
    libgui_add_leaf(ui, libgui_id_from_name("gizmo"), leaf, 1, cb);

    CHECK(libgui_open_depth(ui) == 1, "the container stack is not where it should be");
    libgui_close_container(ui);
    libgui_end_frame(ui);

    CHECK(libgui_ui_poisoned(ui) == 0, "the frame poisoned the Ui");

    /* Nulls: a host will pass one eventually. */
    libgui_begin_frame(ui, 400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
    libgui_label(ui, NULL);
    libgui_checkbox(ui, NULL, NULL);
    libgui_end_frame(ui);
    CHECK(libgui_ui_poisoned(ui) == 0, "a null argument poisoned the Ui");

    /* --- the whole loop: input in, pixels out ---------------------------- */
    libgui_install_default_keymap(ui);

    /* Hover the button, then click it, and see the click come back. */
    uint8_t seen_click = 0;
    for (int i = 0; i < 6; i++) {
        libgui_begin_frame(ui, 400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
        libgui_open_container(ui, libgui_id_from_name("panel"), panel, bg);
        LibguiResponse b = libgui_button(ui, "Apply");
        if (b.clicked) seen_click = 1;
        libgui_close_container(ui);
        libgui_end_frame(ui);

        if (i == 1) libgui_push_pointer_moved(ui, 30.0f, 20.0f);
        if (i == 2) libgui_push_pointer_button(ui, LIBGUI_BUTTON_PRIMARY, 1);
        if (i == 3) libgui_push_pointer_button(ui, LIBGUI_BUTTON_PRIMARY, 0);
    }
    CHECK(seen_click, "a click pushed from C never reached a button");

    /* There is something to draw, and the host can reach it. */
    uint64_t n_inst = 0, n_batch = 0;
    const void* inst = libgui_frame_instances(ui, &n_inst);
    const LibguiBatch* batches = libgui_frame_batches(ui, &n_batch);
    CHECK(inst != NULL && n_inst > 0, "no instances to draw");
    CHECK(batches != NULL && n_batch > 0, "no batches to draw");
    CHECK(libgui_instance_stride() == 96, "the instance stride is not 96 bytes");
    CHECK(libgui_vertices_per_instance() == 6, "not 6 vertices per instance");

    uint32_t atlas_size = 0;
    uint64_t atlas_version = 0;
    const uint8_t* atlas = libgui_frame_atlas(ui, &atlas_size, &atlas_version);
    CHECK(atlas != NULL && atlas_size > 0, "no glyph atlas");

    LibguiGlobals g;
    libgui_frame_globals(ui, &g);
    CHECK(g.screen_width == 400.0f && g.scale == 1.0f, "the globals are wrong");

    LibguiPlatformOutput po;
    libgui_frame_platform(ui, &po);
    (void)po;

    /* Typing into a field, with the buffer the caller owns. */
    char text[64];
    strcpy(text, "Part");
    uint64_t need = 0;
    for (int i = 0; i < 8; i++) {
        libgui_begin_frame(ui, 400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
        libgui_open_container(ui, libgui_id_from_name("panel"), panel, bg);
        libgui_text_input(ui, "name", text, sizeof(text), "Name", &need);
        libgui_close_container(ui);
        libgui_end_frame(ui);

        if (i == 1) libgui_push_pointer_moved(ui, 60.0f, 20.0f);
        if (i == 2) libgui_push_pointer_button(ui, LIBGUI_BUTTON_PRIMARY, 1);
        if (i == 3) libgui_push_pointer_button(ui, LIBGUI_BUTTON_PRIMARY, 0);
        if (i == 5) libgui_push_text(ui, "X");
    }
    CHECK(strcmp(text, "PartX") == 0 || strcmp(text, "XPart") == 0,
          "typing did not reach the caller's buffer");
    CHECK(need == 5, "the reported length is wrong");

    /* A buffer too small truncates cleanly and says how much was needed. */
    char tiny[3];
    strcpy(tiny, "ab");
    libgui_begin_frame(ui, 400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
    libgui_text_input(ui, "tiny", tiny, sizeof(tiny), "", &need);
    libgui_end_frame(ui);
    CHECK(tiny[2] == 0, "the small buffer was not NUL-terminated");

    /* --- docking --------------------------------------------------------- */
    LibguiDock* dock = libgui_dock_new();
    CHECK(dock != NULL, "libgui_dock_new returned null");
    libgui_dock_set_in_app_floating(dock, 1);   /* no OS windows in a test */

    uint64_t left  = libgui_dock_leaf(dock, TAB_MODEL);
    uint64_t right = libgui_dock_leaf(dock, TAB_VIEWPORT);
    uint64_t root  = libgui_dock_split(dock, 0, 0.25f, left, right);
    CHECK(libgui_dock_set_root(dock, 0, root) == 0, "set_root failed");
    libgui_dock_add_tab(dock, 0, TAB_PROPERTIES);

    /* A node handle is consumed: using it twice is refused, not duplicated. */
    CHECK(libgui_dock_set_root(dock, 0, root) != 0, "a spent node handle was accepted");

    LibguiTabViewer viewer;
    viewer.title = dock_title;
    viewer.ui = dock_panel;
    viewer.scroll = dock_scroll;
    viewer.padding = NULL;
    viewer.user = (void*)0x77;

    for (int i = 0; i < 4; i++) {
        libgui_dock_set_pointer(dock, 0.0f, 0.0f, 0);
        libgui_dock_set_surface_frame(dock, 0, 0.0f, 0.0f, 1.0f);
        libgui_dock_update(dock);

        libgui_begin_frame(ui, 800.0f, 600.0f, 1.0f, 1.0f / 60.0f);
        libgui_dock_show(dock, ui, 0, &viewer);
        libgui_end_frame(ui);
    }
    CHECK(panels_drawn > 0, "no dock panel was ever drawn");
    CHECK(reentrancy_refused > 0, "ending the frame inside a panel was not refused");
    CHECK(!libgui_ui_poisoned(ui), "docking poisoned the Ui");

    /* The windows the host should have. */
    uint64_t surfaces = libgui_dock_surface_count(dock);
    CHECK(surfaces >= 1, "no surfaces");
    LibguiSurface s0;
    CHECK(libgui_dock_surface_at(dock, 0, &s0) == 0, "surface_at failed");
    CHECK(s0.floating == 0, "the main surface says it is floating");
    CHECK(s0.tab_count == 3, "the main surface lost a tab");
    CHECK(libgui_dock_surface_at(dock, 999, &s0) != 0, "an out-of-range surface was accepted");

    /* Layout round trip, snprintf-style. */
    int64_t want = libgui_dock_layout_to_toml(dock, NULL, 0);
    CHECK(want > 0, "the layout did not render");
    char* toml = (char*)malloc((size_t)want + 1);
    int64_t again = libgui_dock_layout_to_toml(dock, toml, (uint64_t)want + 1);
    CHECK(again == want, "the second call reported a different size");
    CHECK(libgui_dock_restore_from_toml(dock, toml) == 0, "the layout did not restore");
    CHECK(libgui_dock_restore_from_toml(dock, "this is not toml") != 0, "garbage was accepted as a layout");

    uint64_t after = libgui_dock_surface_count(dock);
    CHECK(after == surfaces, "restoring changed the window count");
    free(toml);
    libgui_dock_free(dock);
    libgui_dock_free(NULL);

    /* --- tables ---------------------------------------------------------- */
    LibguiTable* table = libgui_table_new();
    CHECK(table != NULL, "libgui_table_new returned null");
    libgui_table_add_column(table, "Name", 120.0f, 1.0f, 1, 1, 0);
    libgui_table_add_column(table, "Value", 80.0f, 0.0f, 1, 1, 2);
    libgui_table_set_frozen(table, 1);

    LibguiTableResponse tr;
    memset(&tr, 0, sizeof(tr));
    for (int i = 0; i < 3; i++) {
        libgui_begin_frame(ui, 800.0f, 600.0f, 1.0f, 1.0f / 60.0f);
        libgui_table_show(ui, table, "props", 1000, table_cell, (void*)0x99, &tr);
        libgui_end_frame(ui);
    }
    CHECK(cells_filled > 0, "no table cell was filled");
    /* A table virtualises: a thousand rows must not build a thousand rows. */
    CHECK(tr.row_count > 0 && tr.row_count < 1000, "the table did not virtualise");
    CHECK(!libgui_ui_poisoned(ui), "the table poisoned the Ui");
    libgui_table_free(table);
    libgui_table_free(NULL);

    /* --- themes from TOML ------------------------------------------------- */
    int64_t tneed = libgui_theme_to_toml(ui, NULL, 0);
    CHECK(tneed > 0, "the theme did not render as TOML");
    char* ttoml = (char*)malloc((size_t)tneed + 1);
    libgui_theme_to_toml(ui, ttoml, (uint64_t)tneed + 1);
    CHECK(libgui_set_theme_toml(ui, ttoml) == 0, "a theme this library wrote would not load back");
    /* A palette override, which is how an existing theme ports. */
    CHECK(libgui_set_theme_toml(ui, "extends = \"dark\"\n[palette]\naccent = \"#31639f\"\n") == 0,
          "a palette override was rejected");
    CHECK(libgui_set_theme_toml(ui, "[palette]\nbg_app = \"not a colour\"\n") != 0,
          "a bad colour was accepted");
    CHECK(libgui_last_error() != NULL, "the theme error was not reported");
    free(ttoml);

    /* --- drag and drop ---------------------------------------------------- */
    const char* kinds[1];
    kinds[0] = "body";
    LibguiDropZone zone;
    memset(&zone, 0, sizeof(zone));
    for (int i = 0; i < 3; i++) {
        libgui_begin_frame(ui, 800.0f, 600.0f, 1.0f, 1.0f / 60.0f);
        libgui_open_container(ui, libgui_id_from_name("tree"), panel, bg);
        LibguiResponse row = libgui_selectable(ui, "Body 1", 0);
        (void)row;
        libgui_drag_source(ui, libgui_id_from_name("row1"), "body", 4242, "Body 1");
        libgui_drop_zone(ui, kinds, 1, &zone);
        libgui_close_container(ui);
        libgui_end_frame(ui);
    }
    CHECK(libgui_dragging(ui) == NULL, "something was dragging when nothing should be");
    libgui_cancel_drag(ui);
    CHECK(!libgui_ui_poisoned(ui), "drag and drop poisoned the Ui");

    libgui_ui_free(ui);
    libgui_ui_free(NULL);
    free(font);

    if (failures == 0) {
        printf("ok: C smoke test passed (%d paint callbacks)\n", painted);
    }
    return failures == 0 ? 0 : 1;
}
