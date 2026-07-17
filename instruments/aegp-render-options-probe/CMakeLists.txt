cmake_minimum_required(VERSION 3.20)
project(aegp_render_options_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_executable(aegp_render_options_probe main.cpp)
target_compile_features(aegp_render_options_probe PRIVATE cxx_std_17)
target_include_directories(aegp_render_options_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP")
if(MSVC)
  target_compile_options(aegp_render_options_probe PRIVATE /W4 /wd4103)
endif()
