cmake_minimum_required(VERSION 3.20)
project(pf_performance_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR
   NOT EXISTS "$ENV{AE_SDK_ROOT}/Examples/Headers/AE_Effect.h")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_library(pf_performance_probe MODULE
  pf_performance_probe.cpp
  pf_performance_probe.rc
)
target_compile_features(pf_performance_probe PRIVATE cxx_std_17)
target_include_directories(pf_performance_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_performance_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_performance_probe PRIVATE /W4 /wd4103)
endif()
