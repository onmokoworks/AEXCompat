cmake_minimum_required(VERSION 3.20)
project(AEXCompatResolveOfx LANGUAGES CXX)

if (APPLE)
  add_library(aexcompat_resolve_ofx MODULE src/resolve_ofx_plugin.cpp)
else()
  add_library(aexcompat_resolve_ofx SHARED src/resolve_ofx_plugin.cpp)
endif()
target_compile_features(aexcompat_resolve_ofx PRIVATE cxx_std_17)
if (MSVC)
  target_compile_options(aexcompat_resolve_ofx PRIVATE /EHsc)
endif()
target_include_directories(aexcompat_resolve_ofx PRIVATE include)
set_target_properties(aexcompat_resolve_ofx PROPERTIES
  PREFIX ""
  SUFFIX ".ofx"
  OUTPUT_NAME "AEXCompatResolve")

add_executable(resolve_ofx_smoke src/resolve_ofx_smoke.cpp)
target_compile_features(resolve_ofx_smoke PRIVATE cxx_std_17)
if (MSVC)
  target_compile_options(resolve_ofx_smoke PRIVATE /EHsc)
endif()
target_include_directories(resolve_ofx_smoke PRIVATE include)
if (WIN32)
  target_link_libraries(resolve_ofx_smoke PRIVATE bcrypt)
endif()

include(GNUInstallDirs)
if (WIN32)
  install(TARGETS aexcompat_resolve_ofx
    RUNTIME DESTINATION "AEXCompatResolve.ofx.bundle/Contents/Win64")
elseif (APPLE)
  install(TARGETS aexcompat_resolve_ofx
    LIBRARY DESTINATION "AEXCompatResolve.ofx.bundle/Contents/MacOS")
  install(FILES Info.plist
    DESTINATION "AEXCompatResolve.ofx.bundle/Contents")
endif()
