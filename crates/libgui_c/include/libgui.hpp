// libgui — header-only C++ wrapper over the C ABI.
//
// This adds nothing to the ABI and costs nothing at runtime. It exists because
// the three things C makes tedious are the three things you do constantly:
//
//   1. `open_`/`close_` pairs, which an early `return` or a throw will skip.
//      Here they are RAII guards that close themselves.
//   2. Paint callbacks, which in C are a function pointer plus a `void*` you
//      must remember to free. Here they are lambdas, deleted through
//      `drop_user` when the frame ends.
//   3. Pointer-and-length pairs out of the frame, which are views here.
//
// C++17. Include `libgui.h` first if you want the raw C API alongside.
#ifndef LIBGUI_HPP
#define LIBGUI_HPP

#include "libgui.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <utility>

namespace libgui {

// A borrowed contiguous range. Not std::span, so this stays C++17.
template <class T>
class span {
public:
    span() = default;
    span(const T* data, std::size_t size) : data_(data), size_(size) {}
    const T* begin() const { return data_; }
    const T* end() const { return data_ + size_; }
    const T* data() const { return data_; }
    std::size_t size() const { return size_; }
    bool empty() const { return size_ == 0; }
    const T& operator[](std::size_t i) const { return data_[i]; }

private:
    const T* data_ = nullptr;
    std::size_t size_ = 0;
};

// ---------------------------------------------------------------------------
// Builders, so a layout reads as one expression rather than twelve
// assignments into a zeroed struct.
// ---------------------------------------------------------------------------

inline LibguiSize fixed(float px) { return LibguiSize{LIBGUI_SIZE_FIXED, px}; }
inline LibguiSize fit() { return LibguiSize{LIBGUI_SIZE_FIT, 0.0f}; }
inline LibguiSize grow(float weight = 1.0f) { return LibguiSize{LIBGUI_SIZE_GROW, weight}; }

class Layout {
public:
    static Layout row() { return Layout(0); }
    static Layout column() { return Layout(1); }

    Layout& width(LibguiSize s) { l_.width = s; return *this; }
    Layout& height(LibguiSize s) { l_.height = s; return *this; }
    Layout& padding(float all) { return padding(all, all, all, all); }
    Layout& padding(float left, float right, float top, float bottom) {
        l_.pad_left = left; l_.pad_right = right; l_.pad_top = top; l_.pad_bottom = bottom;
        return *this;
    }
    Layout& gap(float g) { l_.gap = g; return *this; }
    Layout& align(uint8_t main, uint8_t cross) { l_.align_main = main; l_.align_cross = cross; return *this; }

    operator LibguiLayout() const { return l_; }

private:
    explicit Layout(uint8_t axis) : l_{} { l_.axis = axis; l_.width = grow(); l_.height = fit(); }
    LibguiLayout l_;
};

class Frame {
public:
    static Frame none() { return Frame(); }
    Frame& fill(LibguiColor c) { f_.fill = c; return *this; }
    Frame& border(LibguiColor c, float w) { f_.border = c; f_.border_width = w; return *this; }
    Frame& radius(float r) { f_.radius = r; return *this; }
    Frame& clip(bool on = true) { f_.clip = on ? 1 : 0; return *this; }
    Frame& shadow(bool on = true) { f_.shadow = on ? 1 : 0; return *this; }
    operator LibguiFrame() const { return f_; }

private:
    Frame() : f_{} {}
    LibguiFrame f_;
};

inline uint64_t id(const char* name) { return libgui_id_from_name(name); }

class Ui;

// A guard that closes what it opened, however the scope ends — including an
// early return or a throw, which is exactly where a hand-written close is
// forgotten.
template <void (*Close)(LibguiUi*)>
class Guard {
public:
    explicit Guard(LibguiUi* ui) : ui_(ui) {}
    ~Guard() { if (ui_) Close(ui_); }
    Guard(Guard&& o) noexcept : ui_(o.ui_) { o.ui_ = nullptr; }
    Guard& operator=(Guard&&) = delete;
    Guard(const Guard&) = delete;
    Guard& operator=(const Guard&) = delete;

private:
    LibguiUi* ui_;
};

namespace detail {
inline void close_container(LibguiUi* ui) { libgui_close_container(ui); }
inline void close_scroll(LibguiUi* ui) { libgui_close_scroll_area(ui); }
inline void close_collection(LibguiUi* ui) { libgui_close_collection(ui); }
inline void close_menu(LibguiUi* ui) { libgui_close_menu(ui); }
inline void close_canvas(LibguiUi* ui) { libgui_close_canvas(ui); }
inline void close_transform(LibguiUi* ui) { libgui_close_transform(ui); }

// A lambda reaches the C side as a function pointer plus a void*, and is
// deleted through `drop_user` when libgui drops the paint closure — at the end
// of the frame, whether or not the widget was visible.
template <class F>
void paint_trampoline(LibguiPainter* p, LibguiRect r, void* user) {
    (*static_cast<F*>(user))(p, r);
}
template <class F>
void paint_delete(void* user) {
    delete static_cast<F*>(user);
}
}  // namespace detail

using ContainerGuard = Guard<detail::close_container>;
using ScrollGuard = Guard<detail::close_scroll>;
using CollectionGuard = Guard<detail::close_collection>;
using MenuGuard = Guard<detail::close_menu>;
using CanvasGuard = Guard<detail::close_canvas>;
using TransformGuard = Guard<detail::close_transform>;

// A units table, freed when it goes out of scope. Move-only, because two
// owners of one table would free it twice.
class Units {
public:
    static Units length_mm() { return Units(libgui_units_length_mm()); }
    static Units angle_deg() { return Units(libgui_units_angle_deg()); }
    static Units none() { return Units(libgui_units_none()); }
    explicit Units(const char* base) : h_(libgui_units_new(base)) {}
    ~Units() { libgui_units_free(h_); }
    Units(Units&& o) noexcept : h_(o.h_) { o.h_ = nullptr; }
    Units& operator=(Units&& o) noexcept {
        if (this != &o) { libgui_units_free(h_); h_ = o.h_; o.h_ = nullptr; }
        return *this;
    }
    Units(const Units&) = delete;
    Units& operator=(const Units&) = delete;

    Units& add(const char* name, double factor) { libgui_units_add(h_, name, factor); return *this; }
    Units& display(const char* name) { libgui_units_set_display(h_, name); return *this; }
    std::string format(double v, uint32_t decimals = 3) const {
        std::string out(libgui_units_format(h_, v, decimals, nullptr, 0), '\0');
        libgui_units_format(h_, v, decimals, out.data(), out.size() + 1);
        return out;
    }
    const LibguiUnits* raw() const { return h_; }

private:
    explicit Units(LibguiUnits* h) : h_(h) {}
    LibguiUnits* h_;
};

// What `canvas` reported: the background's response, the view, and the guard
// that closes it. The background's `mouse_pos` is in WINDOW coordinates;
// everything built inside the canvas is in canvas coordinates.
struct Canvas {
    LibguiResponse background;
    LibguiCanvasView view;
    CanvasGuard guard;
};

// The enabled scope restores what it found rather than forcing "enabled", so
// it needs the saved value.
class EnabledGuard {
public:
    EnabledGuard(LibguiUi* ui, uint8_t was) : ui_(ui), was_(was) {}
    ~EnabledGuard() { if (ui_) libgui_close_enabled(ui_, was_); }
    EnabledGuard(EnabledGuard&& o) noexcept : ui_(o.ui_), was_(o.was_) { o.ui_ = nullptr; }
    EnabledGuard& operator=(EnabledGuard&&) = delete;
    EnabledGuard(const EnabledGuard&) = delete;
    EnabledGuard& operator=(const EnabledGuard&) = delete;

private:
    LibguiUi* ui_;
    uint8_t was_;
};

// What `open_collection` reported. Read once; the C accessors are thread-local
// and only valid straight after the call.
struct Nav {
    uint64_t id = 0;
    uint64_t cursor = 0;
    bool moved = false;
    bool focused = false;
    bool activated = false;
    bool expand = false;
    bool collapse = false;
};

class Ui {
public:
    Ui(const uint8_t* font, uint64_t font_len) : h_(libgui_ui_new(font, font_len)) {}
    ~Ui() { libgui_ui_free(h_); }
    Ui(Ui&& o) noexcept : h_(o.h_) { o.h_ = nullptr; }
    Ui& operator=(Ui&& o) noexcept {
        if (this != &o) { libgui_ui_free(h_); h_ = o.h_; o.h_ = nullptr; }
        return *this;
    }
    Ui(const Ui&) = delete;
    Ui& operator=(const Ui&) = delete;

    bool valid() const { return h_ != nullptr; }
    bool poisoned() const { return libgui_ui_poisoned(h_) != 0; }
    LibguiUi* raw() const { return h_; }
    static const char* last_error() { return libgui_last_error(); }

    // --- frame ---
    void begin_frame(float w, float h, float scale, float dt) { libgui_begin_frame(h_, w, h, scale, dt); }
    void end_frame() { libgui_end_frame(h_); }
    bool needs_frame(float elapsed) const { return libgui_needs_frame(h_, elapsed) != 0; }
    int install_default_keymap() { return libgui_install_default_keymap(h_); }
    int set_theme(const char* name) { return libgui_set_theme(h_, name); }

    span<const std::byte> instances() const {
        uint64_t n = 0;
        auto* p = libgui_frame_instances(h_, &n);
        return {static_cast<const std::byte*>(p), static_cast<std::size_t>(n) * libgui_instance_stride()};
    }
    uint64_t instance_count() const {
        uint64_t n = 0;
        libgui_frame_instances(h_, &n);
        return n;
    }
    span<const LibguiBatch> batches() const {
        uint64_t n = 0;
        auto* p = libgui_frame_batches(h_, &n);
        return {p, static_cast<std::size_t>(n)};
    }
    span<const uint8_t> atlas(uint32_t* size, uint64_t* version) const {
        auto* p = libgui_frame_atlas(h_, size, version);
        std::size_t n = size ? static_cast<std::size_t>(*size) * (*size) : 0;
        return {p, n};
    }
    // --- the triangle form ---
    //
    // For a renderer with no per-instance attributes: bgfx, GLES2, WebGL1.
    // Off unless asked for, and about four and a half times the bytes, so a
    // renderer that can instance should keep instancing.
    void enable_mesh(bool on = true) { libgui_enable_mesh(h_, on ? 1 : 0); }
    span<const std::byte> mesh_vertices() const {
        uint64_t n = 0;
        auto* p = libgui_mesh_vertices(h_, &n);
        return {static_cast<const std::byte*>(p), static_cast<std::size_t>(n) * libgui_vertex_stride()};
    }
    uint64_t mesh_vertex_count() const {
        uint64_t n = 0;
        libgui_mesh_vertices(h_, &n);
        return n;
    }
    span<const uint32_t> mesh_indices() const {
        uint64_t n = 0;
        auto* p = libgui_mesh_indices(h_, &n);
        return {p, static_cast<std::size_t>(n)};
    }
    span<const LibguiBatch> mesh_batches() const {
        uint64_t n = 0;
        auto* p = libgui_mesh_batches(h_, &n);
        return {p, static_cast<std::size_t>(n)};
    }
    /// True when the indices fit a 16-bit buffer, which is all GLES2 and
    /// WebGL1 have.
    bool mesh_fits_u16() const { return libgui_mesh_fits_u16(h_) != 0; }

    LibguiGlobals globals() const { LibguiGlobals g{}; libgui_frame_globals(h_, &g); return g; }
    LibguiColor clear_color() const { LibguiColor c{}; libgui_frame_clear_color(h_, &c); return c; }
    LibguiPlatformOutput platform() const { LibguiPlatformOutput p{}; libgui_frame_platform(h_, &p); return p; }
    const char* copied_text() const { return libgui_frame_copied_text(h_); }

    // --- input ---
    void pointer_moved(float x, float y) { libgui_push_pointer_moved(h_, x, y); }
    void pointer_button(uint32_t b, bool down) { libgui_push_pointer_button(h_, b, down ? 1 : 0); }
    void pointer_left() { libgui_push_pointer_left(h_); }
    void wheel(float dx, float dy, uint32_t unit) { libgui_push_wheel(h_, dx, dy, unit); }
    void key(uint32_t k, bool down, bool repeat = false) { libgui_push_key(h_, k, down ? 1 : 0, repeat ? 1 : 0); }
    void modifiers(LibguiModifiers m) { libgui_push_modifiers(h_, m); }
    void text(const char* s) { libgui_push_text(h_, s); }
    void paste(const char* s) { libgui_push_paste(h_, s); }
    void focus_lost() { libgui_push_focus_lost(h_); }

    // --- scopes, all RAII ---
    [[nodiscard]] ContainerGuard container(uint64_t cid, LibguiLayout l, LibguiFrame f) {
        libgui_open_container(h_, cid, l, f);
        return ContainerGuard(h_);
    }
    [[nodiscard]] ScrollGuard scroll_area(const char* key) {
        libgui_open_scroll_area(h_, key);
        return ScrollGuard(h_);
    }
    [[nodiscard]] EnabledGuard enabled(bool on) {
        return EnabledGuard(h_, libgui_open_enabled(h_, on ? 1 : 0));
    }
    bool is_enabled() const { return libgui_is_enabled(h_) != 0; }
    uint64_t open_depth() const { return libgui_open_depth(h_); }

    // A pan/zoom canvas. `state` is yours, kept across frames; start it with
    // libgui_canvas_state_default.
    [[nodiscard]] Canvas canvas(const char* key, LibguiCanvasState& state) {
        LibguiCanvasView v{};
        LibguiResponse bg = libgui_open_canvas(h_, key, &state, &v);
        return Canvas{bg, v, CanvasGuard(h_)};
    }
    [[nodiscard]] TransformGuard transform(uint64_t tid, LibguiVec2 pan, float zoom) {
        libgui_open_transform(h_, tid, pan, zoom);
        return TransformGuard(h_);
    }

    // --- motion ---
    float animate(uint64_t wid, uint8_t slot, float target) { return libgui_animate(h_, wid, slot, target); }
    float animate(uint64_t wid, uint8_t slot, bool on) { return libgui_animate_bool(h_, wid, slot, on ? 1 : 0); }
    float animate(uint64_t wid, uint8_t slot, float target, float speed) {
        return libgui_animate_with_speed(h_, wid, slot, target, speed);
    }
    void set_anim(uint64_t wid, uint8_t slot, float v) { libgui_set_anim(h_, wid, slot, v); }
    void request_repaint() { libgui_request_repaint(h_); }
    void keep_id(uint64_t wid) { libgui_keep_id(h_, wid); }

    // A menu is a scope only when it opened, so this returns whether to build
    // the items and the guard that closes it.
    std::pair<bool, MenuGuard> menu(const char* label) {
        bool open = libgui_open_menu(h_, label) != 0;
        return {open, MenuGuard(open ? h_ : nullptr)};
    }
    std::pair<bool, MenuGuard> context_menu(uint64_t widget) {
        bool open = libgui_open_context_menu(h_, widget) != 0;
        return {open, MenuGuard(open ? h_ : nullptr)};
    }

    std::pair<Nav, CollectionGuard> collection(const char* key, uint64_t len) {
        Nav n;
        n.id = libgui_open_collection(h_, key, len);
        n.cursor = libgui_nav_cursor();
        n.moved = libgui_nav_moved() != 0;
        n.focused = libgui_nav_focused() != 0;
        n.activated = libgui_nav_activated() != 0;
        n.expand = libgui_nav_expand() != 0;
        n.collapse = libgui_nav_collapse() != 0;
        return {n, CollectionGuard(h_)};
    }

    // --- widgets ---
    void label(const char* t) { libgui_label(h_, t); }
    void label_muted(const char* t) { libgui_label_muted(h_, t); }
    void heading(const char* t) { libgui_heading(h_, t); }
    void section(const char* t) { libgui_section(h_, t); }
    void paragraph(const char* t) { libgui_paragraph(h_, t); }
    void separator() { libgui_separator(h_); }
    void space(float px) { libgui_space(h_, px); }
    void flex() { libgui_flex(h_); }

    LibguiResponse button(const char* l) { return libgui_button(h_, l); }
    LibguiResponse button_primary(const char* l) { return libgui_button_primary(h_, l); }
    LibguiResponse checkbox(const char* l, bool& v) {
        uint8_t b = v ? 1 : 0;
        auto r = libgui_checkbox(h_, l, &b);
        v = b != 0;
        return r;
    }
    LibguiResponse toggle(const char* l, bool& v) {
        uint8_t b = v ? 1 : 0;
        auto r = libgui_toggle(h_, l, &b);
        v = b != 0;
        return r;
    }
    LibguiResponse slider(const char* l, float& v, float lo, float hi) { return libgui_slider(h_, l, &v, lo, hi); }
    LibguiResponse drag_value(const char* l, float& v, float speed) { return libgui_drag_value(h_, l, &v, speed); }
    /// Your own texture, filling what the container has left: the 3D view, a
    /// render target, a video frame. `texture` comes back in
    /// `LibguiBatch::texture_index` with `texture_kind` 1; drawing it is
    /// yours. Drive a camera from the response.
    LibguiResponse viewport(const char* key, uint64_t texture) { return libgui_viewport(h_, key, texture); }
    LibguiResponse selectable(const char* l, bool sel) { return libgui_selectable(h_, l, sel ? 1 : 0); }
    LibguiResponse selectable(uint64_t key, const char* l, bool sel) {
        return libgui_selectable_keyed(h_, key, l, sel ? 1 : 0);
    }
    LibguiTreeResponse tree_row(uint64_t key, uint64_t depth, uint64_t branch, const char* l, bool sel) {
        return libgui_tree_row(h_, key, depth, branch, l, sel ? 1 : 0);
    }
    LibguiResponse menu_item(const char* l) { return libgui_menu_item(h_, l); }
    void menu_separator() { libgui_menu_separator(h_); }
    LibguiResponse interact(uint64_t wid) { return libgui_interact(h_, wid); }
    void tooltip(uint64_t wid, const char* t) { libgui_tooltip(h_, wid, t); }
    void scroll_to(uint64_t wid) { libgui_scroll_to(h_, wid); }
    void set_cursor(uint64_t coll, uint64_t index) { libgui_set_cursor(h_, coll, index); }

    // A text field over a std::string, grown as needed. The C form wants a
    // buffer and a capacity; this hides that. When the text outgrew what was
    // offered -- a long paste -- the rest is fetched with
    // libgui_text_overflow. Not by calling the field again: in the same frame
    // that is a second, unfocused widget, and the paste would be lost.
    LibguiTextResponse text_input(const char* key, std::string& s, const char* placeholder = "") {
        return edit(s, [&](char* buf, uint64_t cap, uint64_t* need) {
            return libgui_text_input(h_, key, buf, cap, placeholder, need);
        });
    }
    // A CAD dimension box. `value` is in the table's base unit and changes
    // only on commit. `error`, if given, receives the reason text that did
    // not evaluate was refused, and is cleared when it is fixed.
    LibguiNumberResponse number_input(const char* key, double& value, const Units& units,
                                      LibguiNumberOptions opts, std::string* error = nullptr) {
        char why[160];
        opts.error = why;
        opts.error_cap = sizeof why;
        auto r = libgui_number_input(h_, key, &value, units.raw(), &opts);
        if (error) error->assign(why);
        return r;
    }
    LibguiNumberResponse number_input(const char* key, double& value, const Units& units,
                                      std::string* error = nullptr) {
        LibguiNumberOptions o;
        libgui_number_options_default(&o);
        return number_input(key, value, units, o, error);
    }

    LibguiTextResponse text_area(const char* key, std::string& s, uint64_t rows = 6) {
        return edit(s, [&](char* buf, uint64_t cap, uint64_t* need) {
            return libgui_text_area(h_, key, buf, cap, rows, need);
        });
    }

    // A widget you draw yourself, from a lambda. The lambda is copied to the
    // heap and deleted through `drop_user`, so it may capture freely.
    template <class F>
    void add_leaf(uint64_t wid, LibguiLayout l, bool interactive, F&& paint) {
        using Fn = std::decay_t<F>;
        LibguiPaintFn cb;
        cb.paint = &detail::paint_trampoline<Fn>;
        cb.drop_user = &detail::paint_delete<Fn>;
        cb.user = new Fn(std::forward<F>(paint));
        libgui_add_leaf(h_, wid, l, interactive ? 1 : 0, cb);
    }

private:
    template <class Call>
    LibguiTextResponse edit(std::string& s, Call&& call) {
        // One spare byte for the NUL, and room to type into.
        std::size_t cap = s.size() + 64;
        std::string buf(cap, '\0');
        std::memcpy(buf.data(), s.data(), s.size());
        uint64_t need = 0;
        auto r = call(buf.data(), static_cast<uint64_t>(cap), &need);
        if (need >= cap) {
            buf.assign(need + 1, '\0');
            libgui_text_overflow(h_, r.response.id, buf.data(), static_cast<uint64_t>(need + 1));
        }
        s.assign(buf.c_str());
        return r;
    }

    LibguiUi* h_ = nullptr;
};

}  // namespace libgui

#endif  // LIBGUI_HPP
