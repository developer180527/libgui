# libgui — CMake integration.
#
# Drop this into a C or C++ project and it builds the Rust static library for
# you, with no Rust knowledge needed beyond having `cargo` on PATH:
#
#     add_subdirectory(third_party/libgui/crates/libgui_c)
#     target_link_libraries(vcad PRIVATE libgui::libgui)
#
# That is all. The header directory, the platform link libraries and the
# rebuild-when-Rust-changes rule come with the target.
#
# There is deliberately no dependency on Corrosion. Corrosion is good, and
# adding it would mean every consumer vendors it too — for one static library
# from one crate, shelling out to cargo is fewer moving parts. If you already
# use Corrosion, `corrosion_import_crate(MANIFEST_PATH …/libgui_c/Cargo.toml)`
# works instead and this file gets out of the way (see LIBGUI_USE_CORROSION).

cmake_minimum_required(VERSION 3.19)
project(libgui_c LANGUAGES C)

option(LIBGUI_USE_CORROSION "Import the crate through Corrosion instead of calling cargo" OFF)

set(LIBGUI_C_DIR "${CMAKE_CURRENT_SOURCE_DIR}")
get_filename_component(LIBGUI_WORKSPACE "${LIBGUI_C_DIR}/../.." ABSOLUTE)

# --- what to link against besides the library itself -------------------------
# A Rust static library needs its platform's system libraries. `cargo rustc --
# --print native-static-libs` prints these; they are spelled out rather than
# queried so a build does not depend on parsing cargo's output.
if(WIN32)
  set(LIBGUI_SYS_LIBS ws2_32 userenv advapi32 ntdll bcrypt)
elseif(APPLE)
  set(LIBGUI_SYS_LIBS m)
else()
  set(LIBGUI_SYS_LIBS m pthread dl)
endif()

if(LIBGUI_USE_CORROSION)
  find_package(Corrosion REQUIRED)
  corrosion_import_crate(MANIFEST_PATH "${LIBGUI_C_DIR}/Cargo.toml" CRATES libgui_c)
  add_library(libgui INTERFACE)
  target_link_libraries(libgui INTERFACE libgui_c ${LIBGUI_SYS_LIBS})
  target_include_directories(libgui INTERFACE "${LIBGUI_C_DIR}/include")
  add_library(libgui::libgui ALIAS libgui)
  return()
endif()

find_program(LIBGUI_CARGO cargo REQUIRED
  DOC "The Rust toolchain's cargo. Install from https://rustup.rs if missing.")

# --- map CMake configurations onto cargo profiles ----------------------------
#
# A generator expression cannot be used here: IMPORTED_LOCATION is read when
# the build system is written, and a `$<CONFIG>` left in a file path ends up in
# a Makefile rule as literal text. So each configuration gets its own custom
# command and its own IMPORTED_LOCATION_<CONFIG>, which is what makes this work
# under Visual Studio and Ninja Multi-Config as well as plain Make.

if(CMAKE_CONFIGURATION_TYPES)
  set(LIBGUI_CONFIGS ${CMAKE_CONFIGURATION_TYPES})
elseif(CMAKE_BUILD_TYPE)
  set(LIBGUI_CONFIGS ${CMAKE_BUILD_TYPE})
else()
  set(LIBGUI_CONFIGS Release)
endif()

if(DEFINED ENV{CARGO_TARGET_DIR})
  set(LIBGUI_TARGET_DIR "$ENV{CARGO_TARGET_DIR}")
else()
  set(LIBGUI_TARGET_DIR "${LIBGUI_WORKSPACE}/target")
endif()

if(MSVC)
  set(LIBGUI_LIB_NAME "libgui_c.lib")
else()
  set(LIBGUI_LIB_NAME "liblibgui_c.a")
endif()

# Rebuild when any Rust source or manifest changes. CONFIGURE_DEPENDS so adding
# a file re-runs CMake rather than silently building stale code.
file(GLOB_RECURSE LIBGUI_RUST_SOURCES CONFIGURE_DEPENDS
  "${LIBGUI_WORKSPACE}/crates/*/src/*.rs"
  "${LIBGUI_WORKSPACE}/crates/*/Cargo.toml"
  "${LIBGUI_WORKSPACE}/Cargo.toml")

add_library(libgui_static STATIC IMPORTED GLOBAL)
add_custom_target(libgui_cargo)

# Several CMake configurations share one cargo profile — RelWithDebInfo and
# Release are both `release` — so a profile gets one command however many
# configurations point at it.
set(LIBGUI_BUILT_PROFILES "")
foreach(cfg IN LISTS LIBGUI_CONFIGS)
  string(TOUPPER "${cfg}" CFG)
  if(cfg STREQUAL "Debug")
    set(prof "debug")
    set(flags "")
  else()
    set(prof "release")
    set(flags "--release")
  endif()
  set(lib "${LIBGUI_TARGET_DIR}/${prof}/${LIBGUI_LIB_NAME}")

  if(NOT prof IN_LIST LIBGUI_BUILT_PROFILES)
    list(APPEND LIBGUI_BUILT_PROFILES "${prof}")
    add_custom_command(
      OUTPUT "${lib}"
      COMMAND "${LIBGUI_CARGO}" build --package libgui_c --locked ${flags}
      WORKING_DIRECTORY "${LIBGUI_WORKSPACE}"
      DEPENDS ${LIBGUI_RUST_SOURCES}
      COMMENT "Building libgui_c (${prof})"
      VERBATIM
      USES_TERMINAL)
    add_custom_target(libgui_cargo_${prof} DEPENDS "${lib}")
    add_dependencies(libgui_cargo libgui_cargo_${prof})
  endif()

  set_property(TARGET libgui_static PROPERTY IMPORTED_LOCATION_${CFG} "${lib}")
  # Read for any configuration the project defines but this file did not map.
  if(NOT DEFINED LIBGUI_FALLBACK_LIB)
    set(LIBGUI_FALLBACK_LIB "${lib}")
  endif()
endforeach()
set_property(TARGET libgui_static PROPERTY IMPORTED_LOCATION "${LIBGUI_FALLBACK_LIB}")
add_dependencies(libgui_static libgui_cargo)

# The target a consumer links. INTERFACE, so the header path and the system
# libraries travel with it.
add_library(libgui INTERFACE)
add_dependencies(libgui libgui_cargo)
target_link_libraries(libgui INTERFACE libgui_static ${LIBGUI_SYS_LIBS})
target_include_directories(libgui INTERFACE "${LIBGUI_C_DIR}/include")
add_library(libgui::libgui ALIAS libgui)

# The bundled font, for a consumer that wants it. libgui reads no files itself;
# this is only so a sample or a test can find one.
set(LIBGUI_FONT "${LIBGUI_WORKSPACE}/assets/Inter.ttf" CACHE FILEPATH "Bundled Inter")
