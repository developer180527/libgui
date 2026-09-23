# vcpkg port for libgui's C ABI.
#
# For a project that wants a *released* libgui rather than one it co-develops.
# If you are changing libgui alongside your app — which vCAD is — prefer
# `add_subdirectory(crates/libgui_c)`: it rebuilds when the Rust changes, and a
# port does not.
#
# vcpkg does not manage Rust, so cargo must be on PATH. That is the one
# prerequisite, and it is checked here rather than failing later with a
# confusing message.

vcpkg_find_acquire_program(CARGO)
if(NOT CARGO)
    message(FATAL_ERROR
        "libgui is written in Rust and needs `cargo` on PATH. Install the "
        "toolchain from https://rustup.rs and re-run vcpkg.")
endif()

vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO developer180527/libgui
    REF "v${VERSION}"
    SHA512 0  # filled in when a release is tagged
    HEAD_REF main
)

vcpkg_cmake_configure(
    SOURCE_PATH "${SOURCE_PATH}/crates/libgui_c"
)
vcpkg_cmake_install()

file(INSTALL "${SOURCE_PATH}/crates/libgui_c/include/libgui.h"
             "${SOURCE_PATH}/crates/libgui_c/include/libgui.hpp"
     DESTINATION "${CURRENT_PACKAGES_DIR}/include")

# Both licences, because the port is `MIT OR Apache-2.0` and a consumer picks.
vcpkg_install_copyright(FILE_LIST
    "${SOURCE_PATH}/LICENSE-MIT"
    "${SOURCE_PATH}/LICENSE-APACHE")
