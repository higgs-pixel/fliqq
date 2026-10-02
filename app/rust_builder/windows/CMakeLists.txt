# The Flutter tooling requires that developers have a version of Visual Studio
# installed that includes CMake 3.14 or later. You should not increase this
# version, as doing so will cause the plugin to fail to compile for some
# customers of the plugin.
cmake_minimum_required(VERSION 3.14)

# Project-level configuration.
set(PROJECT_NAME "rust_lib_fliq")
project(${PROJECT_NAME} LANGUAGES CXX)

get_filename_component(RUST_DIR "${CMAKE_CURRENT_LIST_DIR}/../../rust" ABSOLUTE)
file(RELATIVE_PATH RUST_REL_DIR "${CMAKE_CURRENT_SOURCE_DIR}" "${RUST_DIR}")

include("../cargokit/cmake/cargokit.cmake")
apply_cargokit(${PROJECT_NAME} "${RUST_REL_DIR}" rust_lib_fliq "")

# List of absolute paths to libraries that should be bundled with the plugin.
# This list could contain prebuilt libraries, or libraries created by an
# external build triggered from this build file.
set(rust_lib_fliq_bundled_libraries
  "${${PROJECT_NAME}_cargokit_lib}"
  PARENT_SCOPE
)
