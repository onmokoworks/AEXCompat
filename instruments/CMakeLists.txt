cmake_minimum_required(VERSION 3.20)
project(AEXCompatInstruments LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(aexcompat_trace_writer STATIC common/trace_writer.cpp)
target_include_directories(aexcompat_trace_writer PUBLIC common)

add_executable(trace_writer_selftest common/trace_writer_selftest/main.cpp)
target_link_libraries(trace_writer_selftest PRIVATE aexcompat_trace_writer)

if(DEFINED ENV{AE_SDK_ROOT} AND EXISTS "$ENV{AE_SDK_ROOT}")
  add_executable(abi_layout_probe abi-layout-probe/main.cpp)
  target_include_directories(abi_layout_probe PRIVATE
    "$ENV{AE_SDK_ROOT}/Examples/Headers"
    "$ENV{AE_SDK_ROOT}/Examples/Headers/SP"
    "$ENV{AE_SDK_ROOT}/Examples/Util"
  )
  add_subdirectory(pf-null-echo)
  add_subdirectory(pf-callback-tracer)
  add_subdirectory(pf-crashkit)
else()
  message(STATUS "AE_SDK_ROOT is unavailable; pf-null-echo build is skipped")
endif()
