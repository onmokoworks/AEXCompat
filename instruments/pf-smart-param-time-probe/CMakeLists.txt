cmake_minimum_required(VERSION 3.20)
project(pf_smart_param_time_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

foreach(target pf_smart_param_time_probe pf_smart_global_setup_error_probe)
  add_library(${target} MODULE
    pf_smart_param_time_probe.cpp
    pf_smart_param_time_probe.rc
  )
  target_compile_features(${target} PRIVATE cxx_std_17)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
  if(MSVC)
    target_compile_options(${target} PRIVATE /W4 /wd4103)
  endif()
endforeach()
target_compile_definitions(pf_smart_global_setup_error_probe PRIVATE
  AEXCOMPAT_FAIL_GLOBAL_SETUP=1)
