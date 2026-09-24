// The C++ wrapper, compiled and run. Proves the header-only layer works and
// that the RAII guards actually close what they opened.
#include "libgui.hpp"

#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

static int failures = 0;
#define CHECK(cond, msg)                                 \
    do {                                                 \
        if (!(cond)) { std::printf("FAIL: %s\n", msg); failures++; } \
    } while (0)

int main(int argc, char** argv) {
    if (argc < 2) { std::printf("FAIL: no font path\n"); return 1; }
    std::FILE* f = std::fopen(argv[1], "rb");
    if (!f) { std::printf("FAIL: cannot open font\n"); return 1; }
    std::fseek(f, 0, SEEK_END);
    long len = std::ftell(f);
    std::fseek(f, 0, SEEK_SET);
    std::vector<uint8_t> font(static_cast<size_t>(len));
    if (std::fread(font.data(), 1, font.size(), f) != font.size()) { std::printf("FAIL: short read\n"); return 1; }
    std::fclose(f);

    libgui::Ui ui(font.data(), font.size());
    CHECK(ui.valid(), "Ui did not construct");
    if (!ui.valid()) { std::printf("  %s\n", libgui::Ui::last_error()); return 1; }
    ui.install_default_keymap();

    auto panel = libgui::Layout::column().width(libgui::grow()).height(libgui::fit()).padding(8.0f).gap(4.0f);
    auto bg = libgui::Frame::none().fill({0.1f, 0.1f, 0.1f, 1.0f}).radius(4.0f).clip();

    bool visible = true;
    float fillet = 0.25f;
    std::string name = "Part";
    bool clicked = false;
    int painted = 0;

    for (int i = 0; i < 8; i++) {
        ui.begin_frame(400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
        {
            auto _c = ui.container(libgui::id("panel"), panel, bg);
            ui.heading("Model");
            ui.checkbox("Visible", visible);
            ui.slider("Fillet", fillet, 0.0f, 1.0f);
            ui.text_input("name", name, "Name");

            // A command that does not apply: greyed out and inert.
            {
                auto _e = ui.enabled(false);
                CHECK(!ui.is_enabled(), "the disabled scope did not take");
                if (ui.button("Join").clicked) CHECK(false, "a disabled button reported a click");
            }
            CHECK(ui.is_enabled(), "the disabled scope did not restore");

            if (ui.button("Apply").clicked) clicked = true;

            // A widget drawn by a capturing lambda.
            auto leaf = libgui::Layout::row().width(libgui::fixed(60.0f)).height(libgui::fixed(24.0f));
            ui.add_leaf(libgui::id("gizmo"), leaf, true, [&painted](LibguiPainter* p, LibguiRect r) {
                painted++;
                libgui_painter_rect(p, r, LibguiColor{1.0f, 0.2f, 0.2f, 1.0f}, 2.0f);
            });

            CHECK(ui.open_depth() == 1, "the container stack is wrong inside the guard");
        }
        CHECK(ui.open_depth() == 0, "the RAII guard did not close the container");
        ui.end_frame();

        if (i == 2) ui.pointer_moved(40.0f, 120.0f);
        if (i == 3) ui.pointer_button(LIBGUI_BUTTON_PRIMARY, true);
        if (i == 4) ui.pointer_button(LIBGUI_BUTTON_PRIMARY, false);
    }

    CHECK(painted > 0, "the lambda paint callback never ran");
    CHECK(ui.instance_count() > 0, "nothing to draw");
    CHECK(!ui.batches().empty(), "no batches");
    CHECK(!ui.poisoned(), "the Ui was poisoned");
    (void)clicked;

    // An early return inside a scope must still close it — the reason the
    // guards exist at all.
    ui.begin_frame(400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
    [&] {
        auto _c = ui.container(libgui::id("early"), panel, bg);
        ui.label("leaving early");
        return;  // the guard closes on the way out
    }();
    CHECK(ui.open_depth() == 0, "an early return leaked an open container");
    ui.end_frame();

    // A collection, which is how a model browser is driven.
    ui.begin_frame(400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
    {
        auto [nav, _g] = ui.collection("model", 3);
        for (uint64_t i = 0; i < 3; i++) {
            ui.selectable(i, "Body", nav.cursor == i);
        }
    }
    ui.end_frame();
    CHECK(ui.open_depth() == 0, "the collection guard did not close");
    CHECK(!ui.poisoned(), "the collection poisoned the Ui");

    // The engine case: a 3D view to put a scene in, and the triangle form for
    // a renderer that cannot carry six vec4s of per-instance data.
    ui.enable_mesh();
    ui.begin_frame(800.0f, 600.0f, 1.0f, 1.0f / 60.0f);
    {
        // A viewport grows to fill what it is given, so its parent has to
        // have a height to give: inside the `fit` panel above it would be
        // zero pixels tall and draw nothing.
        auto stage = libgui::Layout::column().width(libgui::grow()).height(libgui::grow());
        auto _c = ui.container(libgui::id("body"), stage, bg);
        ui.viewport("scene", 7);
    }
    ui.end_frame();

    bool found = false;
    for (auto& b : ui.batches()) {
        if (b.texture_kind == 1 && b.texture_index == 7) found = true;
    }
    CHECK(found, "the viewport did not reach the host as a texture batch");
    CHECK(ui.mesh_vertex_count() == ui.instance_count() * 4, "a primitive did not become one quad");
    CHECK(ui.mesh_indices().size() == ui.instance_count() * 6, "the index count is wrong");
    CHECK(ui.mesh_vertices().size() == ui.mesh_vertex_count() * libgui_vertex_stride(), "the vertex span is the wrong length");
    CHECK(!ui.mesh_batches().empty(), "the mesh has no batches");
    CHECK(ui.mesh_fits_u16(), "a frame this small should fit 16-bit indices");
    CHECK(!ui.poisoned(), "the mesh poisoned the Ui");

    ui.enable_mesh(false);

    // The canvas and transform close themselves, and a dimension box reads
    // an expression into base units.
    {
        LibguiCanvasState view;
        libgui_canvas_state_default(&view);
        libgui::Units mm = libgui::Units::length_mm();
        CHECK(mm.format(25.4) == "25.4 mm", "Units::format");
        double depth = 12.0;
        std::string why;
        for (int pass = 0; pass < 2; pass++) {
            ui.begin_frame(400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
            {
                auto cv = ui.canvas("sketch", view);
                ui.label("an entity");
                {
                    auto _t = ui.transform(libgui::id("cam"), LibguiVec2{0.0f, 0.0f}, 2.0f);
                    ui.label("under my camera");
                }
                (void)cv;
            }
            float hot = ui.animate(libgui::id("w"), 0, true);
            (void)hot;
            ui.number_input("depth", depth, mm, &why);
            ui.end_frame();
            CHECK(ui.open_depth() == 0, "the canvas or transform guard did not close");
        }
        CHECK(view.visible.w > 0.0f, "the canvas state was not written back");
        CHECK(depth == 12.0 && why.empty(), "an untouched dimension box changed");
        CHECK(!ui.poisoned(), "the canvas or number box poisoned the Ui");
    }

    // A paste longer than the wrapper's spare room must arrive whole, in one
    // field. The wrapper offers the text plus 64 bytes and retries when the
    // field outgrows that.
    {
        ui.install_default_keymap();
        std::string name;
        LibguiRect r{};
        auto field = [&] {
            ui.begin_frame(400.0f, 300.0f, 1.0f, 1.0f / 60.0f);
            auto t = ui.text_input("name", name);
            ui.end_frame();
            r = t.response.rect;
            return t;
        };
        for (int i = 0; i < 3; i++) field();
        ui.pointer_moved(r.x + 4.0f, r.y + 4.0f);
        ui.pointer_button(0, true);
        field();
        ui.pointer_button(0, false);
        field();
        std::string big(200, 'x');
        ui.paste(big.c_str());
        field();
        field();
        CHECK(name == big, "a long paste into a C++ text field was lost or cut");
    }

    if (failures == 0) std::printf("ok: C++ wrapper test passed (%d paints)\n", painted);
    return failures == 0 ? 0 : 1;
}
