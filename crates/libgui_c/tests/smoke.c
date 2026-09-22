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

    libgui_ui_free(ui);
    libgui_ui_free(NULL);
    free(font);

    if (failures == 0) {
        printf("ok: C smoke test passed (%d paint callbacks)\n", painted);
    }
    return failures == 0 ? 0 : 1;
}
