cmake_minimum_required(VERSION 3.20)
project(pf_custom_ui_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_library(pf_custom_ui_probe MODULE
  pf_custom_ui_probe.cpp
  pf_custom_ui_probe.rc
)
target_compile_features(pf_custom_ui_probe PRIVATE cxx_std_17)
target_include_directories(pf_custom_ui_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_compile_definitions(pf_custom_ui_probe PRIVATE PF_DEEP_COLOR_AWARE=1)
set_target_properties(pf_custom_ui_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_custom_ui_probe PRIVATE /W4 /wd4103)
endif()
