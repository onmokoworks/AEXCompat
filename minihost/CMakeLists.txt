cmake_minimum_required(VERSION 3.20)
project(AEXCompatMinihost LANGUAGES CXX)

add_library(aexcompat_trace_writer STATIC
  ../instruments/common/trace_writer.cpp
)
target_compile_features(aexcompat_trace_writer PUBLIC cxx_std_17)
target_include_directories(aexcompat_trace_writer PUBLIC
  ../instruments/common
)

add_executable(aex_l1_worker src/main.cpp)
target_compile_features(aex_l1_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_l1_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_l1_worker PRIVATE bcrypt)

set(AEXCOMPAT_L2_WORKER_SOURCES
    src/l2_main.cpp
    src/native_stdout_guard.cpp
    src/parameter_animation_transport.cpp
    src/pf_cache_on_load_suite.cpp
    src/render_lifecycle.cpp
    src/render_pixel_buffer.cpp
    src/render_pixel_transport.cpp
    src/runtime_module_audit.cpp
    src/strict_json.cpp
    src/suite_lease_tracker.cpp
    src/worker_selector_dispatch.cpp
    src/worker_suite_abi.cpp
    src/worker_world_safety.cpp)

add_executable(aex_l2_worker ${AEXCOMPAT_L2_WORKER_SOURCES})
target_compile_features(aex_l2_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_l2_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_l2_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(aex_render_worker ${AEXCOMPAT_L2_WORKER_SOURCES})
target_compile_features(aex_render_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_render_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX AEXCOMPAT_RENDER_WORKER=1)
target_link_libraries(aex_render_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(aex_smart_worker ${AEXCOMPAT_L2_WORKER_SOURCES})
target_compile_features(aex_smart_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_smart_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX AEXCOMPAT_SMART_WORKER=1)
target_link_libraries(aex_smart_worker PRIVATE bcrypt aexcompat_trace_writer)
