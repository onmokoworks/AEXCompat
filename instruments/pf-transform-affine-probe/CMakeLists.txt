cmake_minimum_required(VERSION 3.20)
project(pf_transform_affine_probe LANGUAGES CXX)
if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()
add_library(pf_transform_affine_probe MODULE pf_transform_affine_probe.cpp pf_transform_affine_probe.rc)
target_compile_features(pf_transform_affine_probe PRIVATE cxx_std_17)
target_include_directories(pf_transform_affine_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers" "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
set_target_properties(pf_transform_affine_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_transform_affine_probe PRIVATE /W4 /wd4103)
endif()
