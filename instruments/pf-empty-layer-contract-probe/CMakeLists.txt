cmake_minimum_required(VERSION 3.20)
project(pf_empty_layer_contract_probe LANGUAGES CXX)

set(MAP_TIME_OFFSET "0" CACHE STRING "Optional-map checkout offset in time-step units")
if(NOT MAP_TIME_OFFSET MATCHES "^-?[0-5]$")
  message(FATAL_ERROR "MAP_TIME_OFFSET must be an integer from -5 to 5")
endif()

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_library(pf_empty_layer_contract_probe MODULE
  pf_empty_layer_contract_probe.cpp
  pf_empty_layer_contract_probe.rc
)
target_compile_features(pf_empty_layer_contract_probe PRIVATE cxx_std_17)
target_compile_definitions(pf_empty_layer_contract_probe PRIVATE
  AEXCOMPAT_PROBE_MAP_TIME_OFFSET=${MAP_TIME_OFFSET})
target_include_directories(pf_empty_layer_contract_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
  "$ENV{AE_SDK_ROOT}/Examples/Util"
)
set_target_properties(pf_empty_layer_contract_probe PROPERTIES PREFIX "" SUFFIX ".aex")
if(MSVC)
  target_compile_options(pf_empty_layer_contract_probe PRIVATE /W4 /wd4103)
endif()
