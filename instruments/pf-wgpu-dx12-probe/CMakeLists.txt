cmake_minimum_required(VERSION 3.20)
project(pf_wgpu_dx12_probe LANGUAGES CXX RC)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_library(pf_wgpu_dx12_probe MODULE
  pf_wgpu_dx12_probe.cpp
  pf_wgpu_dx12_probe.rc
)
target_compile_features(pf_wgpu_dx12_probe PRIVATE cxx_std_17)
target_include_directories(pf_wgpu_dx12_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_compile_definitions(pf_wgpu_dx12_probe PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)
set_target_properties(pf_wgpu_dx12_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_wgpu_dx12_probe PRIVATE /W4 /wd4103 /permissive- /Brepro)
  target_link_options(pf_wgpu_dx12_probe PRIVATE /Brepro)
endif()
