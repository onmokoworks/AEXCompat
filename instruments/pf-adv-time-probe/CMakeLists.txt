cmake_minimum_required(VERSION 3.20)
project(pf_adv_time_probe LANGUAGES CXX)
if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()
add_library(pf_adv_time_probe MODULE pf_adv_time_probe.cpp pf_adv_time_probe.rc)
target_compile_features(pf_adv_time_probe PRIVATE cxx_std_17)
target_include_directories(pf_adv_time_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers" "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
set_target_properties(pf_adv_time_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_adv_time_probe PRIVATE /W4 /wd4103)
endif()
