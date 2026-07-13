cmake_minimum_required(VERSION 3.20)
project(AEXCompatInstruments LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(aexcompat_trace_writer STATIC common/trace_writer.cpp)
target_include_directories(aexcompat_trace_writer PUBLIC common)

add_executable(trace_writer_selftest common/trace_writer_selftest/main.cpp)
target_link_libraries(trace_writer_selftest PRIVATE aexcompat_trace_writer)

if(DEFINED ENV{AE_SDK_ROOT} AND EXISTS "$ENV{AE_SDK_ROOT}")
  add_subdirectory(pf-null-echo)
else()
  message(STATUS "AE_SDK_ROOT is unavailable; pf-null-echo build is skipped")
endif()
