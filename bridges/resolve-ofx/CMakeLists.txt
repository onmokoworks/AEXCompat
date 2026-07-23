cmake_minimum_required(VERSION 3.20)
project(AEXCompatResolveOfx LANGUAGES CXX)

add_library(aexcompat_resolve_ofx SHARED src/resolve_ofx_plugin.cpp)
target_compile_features(aexcompat_resolve_ofx PRIVATE cxx_std_17)
target_compile_options(aexcompat_resolve_ofx PRIVATE /EHsc)
target_include_directories(aexcompat_resolve_ofx PRIVATE include)
set_target_properties(aexcompat_resolve_ofx PROPERTIES
  PREFIX ""
  SUFFIX ".ofx"
  OUTPUT_NAME "AEXCompatResolve")

add_executable(resolve_ofx_smoke src/resolve_ofx_smoke.cpp)
target_compile_features(resolve_ofx_smoke PRIVATE cxx_std_17)
target_compile_options(resolve_ofx_smoke PRIVATE /EHsc)
target_include_directories(resolve_ofx_smoke PRIVATE include)
if (WIN32)
  target_link_libraries(resolve_ofx_smoke PRIVATE bcrypt)
endif()

include(GNUInstallDirs)
install(TARGETS aexcompat_resolve_ofx
  RUNTIME DESTINATION "AEXCompatResolve.ofx.bundle/Contents/Win64")
