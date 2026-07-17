cmake_minimum_required(VERSION 3.20)
project(pf_suite_abi_probe LANGUAGES CXX)

if(NOT DEFINED ENV{AE_SDK_ROOT} OR NOT EXISTS "$ENV{AE_SDK_ROOT}")
  message(FATAL_ERROR "AE_SDK_ROOT must identify an After Effects SDK root")
endif()

add_executable(pf_suite_abi_probe main.cpp)
target_compile_features(pf_suite_abi_probe PRIVATE cxx_std_17)
target_include_directories(pf_suite_abi_probe PRIVATE
  "$ENV{AE_SDK_ROOT}/Examples/Headers"
  "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
)
if(MSVC)
  target_compile_options(pf_suite_abi_probe PRIVATE /W4 /wd4103)
endif()
