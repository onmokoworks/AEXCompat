cmake_minimum_required(VERSION 3.20)
project(pf_ae_channel_native_probe LANGUAGES CXX)
if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()
add_library(pf_ae_channel_native_probe MODULE
  pf_ae_channel_native_probe.cpp
  pf_ae_channel_native_probe.rc
)
target_compile_features(pf_ae_channel_native_probe PRIVATE cxx_std_17)
target_include_directories(pf_ae_channel_native_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_ae_channel_native_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_ae_channel_native_probe PRIVATE /W4 /wd4103)
endif()
