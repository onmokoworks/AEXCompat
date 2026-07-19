cmake_minimum_required(VERSION 3.20)
project(pf_selector_timeline_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

function(add_seltimeline_flavor target rc_file)
  add_library(${target} MODULE
    pf_selector_timeline_probe.cpp
    ${rc_file}
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
endfunction()

add_seltimeline_flavor(pf_selector_timeline_classic pf_selector_timeline_classic.rc)
add_seltimeline_flavor(pf_selector_timeline_smart pf_selector_timeline_smart.rc)
target_compile_definitions(pf_selector_timeline_smart PRIVATE SELTIMELINE_SMART=1)
