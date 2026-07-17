cmake_minimum_required(VERSION 3.20)
project(pf_color_oracle LANGUAGES CXX)
if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()
add_library(pf_color_oracle MODULE pf_color_oracle.cpp pf_color_oracle.rc)
target_compile_features(pf_color_oracle PRIVATE cxx_std_17)
target_include_directories(pf_color_oracle PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers" "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util")
set_target_properties(pf_color_oracle PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_color_oracle PRIVATE /W4 /wd4103)
endif()
