cmake_minimum_required(VERSION 3.20)
project(pf_frame_origin_probe LANGUAGES CXX)

# Two variants of one translation unit. Both expand and state PF_OutData::origin;
# they differ in where the answer comes from, which is the property issue #984
# turns on: `offered` derives it from what the host left in out_data->width at
# FRAME_SETUP (the Basic_3D shape), `layer` from the layer parameter.
foreach(target pf_frame_origin_probe pf_frame_origin_offered_probe)
  add_library(${target} MODULE pf_frame_origin_probe.cpp ${target}.rc)
  target_include_directories(${target} PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  set_target_properties(${target} PROPERTIES PREFIX "" SUFFIX ".aex")
endforeach()

target_compile_definitions(pf_frame_origin_offered_probe PRIVATE
  AEXCOMPAT_ORIGIN_FROM_OUT_DATA=1)
