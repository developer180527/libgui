# Proves the CMake integration works the way a consumer uses it:
# add_subdirectory, link libgui::libgui, build and run. CI runs this on Linux,
# macOS and Windows — an integration nobody exercises is one that has drifted.
cmake_minimum_required(VERSION 3.19)
project(libgui_cmake_check LANGUAGES C CXX)
add_subdirectory(${LIBGUI_ROOT}/crates/libgui_c libgui_build)
add_executable(smoke ${LIBGUI_ROOT}/crates/libgui_c/tests/smoke.c)
target_link_libraries(smoke PRIVATE libgui::libgui)
add_executable(smokepp ${LIBGUI_ROOT}/crates/libgui_c/tests/smoke.cpp)
target_compile_features(smokepp PRIVATE cxx_std_17)
target_link_libraries(smokepp PRIVATE libgui::libgui)
