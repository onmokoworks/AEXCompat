cmake_minimum_required(VERSION 3.20)
project(issue26_scene_probe LANGUAGES CXX RC)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR
   NOT EXISTS "$ENV{AE_SDK_ROOT}/Examples/Headers/AE_GeneralPlug.h")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_library(issue26_scene_probe MODULE
  issue26_scene_probe.cpp
  issue26_scene_probe.rc
)
target_compile_features(issue26_scene_probe PRIVATE cxx_std_17)
target_include_directories(issue26_scene_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/Win"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
target_compile_definitions(issue26_scene_probe PRIVATE
  MSWindows
  WIN32
  _WINDOWS
)
set_target_properties(issue26_scene_probe PROPERTIES
  PREFIX ""
  SUFFIX ".aex"
)
if(MSVC)
  target_compile_options(issue26_scene_probe PRIVATE /W4 /wd4103)
endif()
