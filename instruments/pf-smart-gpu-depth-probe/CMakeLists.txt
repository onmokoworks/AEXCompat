cmake_minimum_required(VERSION 3.20)
project(pf_smart_gpu_depth_probe LANGUAGES CXX)
if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()
add_library(pf_smart_gpu_depth_probe MODULE pf_smart_gpu_depth_probe.cpp pf_smart_gpu_depth_probe.rc)
target_compile_features(pf_smart_gpu_depth_probe PRIVATE cxx_std_17)
target_include_directories(pf_smart_gpu_depth_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
set_target_properties(pf_smart_gpu_depth_probe PROPERTIES PREFIX "" SUFFIX ".aex")
