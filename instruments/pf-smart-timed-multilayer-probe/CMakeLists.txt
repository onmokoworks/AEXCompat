cmake_minimum_required(VERSION 3.20)
project(pf_smart_timed_multilayer_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_library(pf_smart_timed_primary_probe MODULE
  pf_smart_timed_multilayer_probe.cpp pf_smart_timed_multilayer_probe.rc)
target_compile_features(pf_smart_timed_primary_probe PRIVATE cxx_std_17)
target_compile_definitions(pf_smart_timed_primary_probe PRIVATE AEX_TIMED_PROBE_SLOT=0)
target_include_directories(pf_smart_timed_primary_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
set_target_properties(pf_smart_timed_primary_probe PROPERTIES PREFIX "" SUFFIX ".aex")

add_library(pf_smart_timed_multilayer_probe MODULE
  pf_smart_timed_multilayer_probe.cpp
  pf_smart_timed_multilayer_probe.rc
)
target_compile_features(pf_smart_timed_multilayer_probe PRIVATE cxx_std_17)
target_include_directories(pf_smart_timed_multilayer_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_smart_timed_multilayer_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_smart_timed_multilayer_probe PRIVATE /W4 /wd4103)
endif()
