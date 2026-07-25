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

set(AEXCOMPAT_WORKER_RUNTIME_CORE_SOURCES
    src/gpu_cuda_backend.cpp
    src/gpu_directx_backend.cpp
    src/gpu_device_info_registry.cpp
    src/gpu_memory_world_transport.cpp
    src/gpu_opencl_backend.cpp
    src/host_audio_runtime.cpp
    src/aex_string_table_impl.cpp
    src/extended_inter_memory.cpp
    src/l2_cli_dispatch.cpp
    src/l2_mode_execution.cpp
    src/l2_main.cpp
    src/native_stdout_guard.cpp
    src/parameter_animation_transport.cpp
    src/worker_parameter_runtime.cpp
    src/worker_parameter_selftests.cpp
    src/worker_parameter_selftest_routing.cpp
    src/worker_pf_color_selftests.cpp
    src/worker_parameter_execution.cpp
    src/worker_ui_event_execution.cpp
    src/pf_cache_on_load_suite.cpp
    src/render_lifecycle.cpp
    src/render_pixel_buffer.cpp
    src/render_pixel_transport.cpp
    src/render_subsystem.cpp
    src/runtime_module_audit.cpp
    src/strict_json.cpp
    src/suite_lease_tracker.cpp
    src/worker_selector_dispatch.cpp
    src/worker_suite_call_slot_probe.cpp
    src/worker_suite_registry.cpp
    src/worker_runtime_admission.cpp
    src/worker_entry_admission.cpp
    src/worker_session.cpp
    src/worker_selftest_dispatch.cpp
    src/worker_fixed_selftest_routing.cpp
    src/worker_custom_selftest_routing.cpp
    src/worker_host_guard_selftests.cpp
    src/worker_aegp_compute_cache.cpp
    src/worker_aegp_utility_suite.cpp
    src/worker_pf_pixel_data_suite.cpp
    src/worker_pf_world_suite.cpp
    src/worker_pf_pixel_format_registry.cpp
    src/worker_pf_param_suites.cpp
    src/worker_aegp_pf_interface_suite.cpp
    src/worker_aegp_command_suites.cpp
    src/worker_mask_suite_tables.cpp
    src/worker_l2_render_abi.cpp
    src/worker_classic_report.cpp
    src/worker_smart_report.cpp
    src/worker_smart_runtime.cpp
    src/worker_smart_execution.cpp
    src/worker_smart_setup.cpp
    src/worker_smart_dispatch.cpp
    src/worker_smart_finalize.cpp
    src/worker_smart_render_runtime.cpp
    src/worker_mask_runtime.cpp
    src/worker_mask_selftests.cpp
    src/worker_minidump_runtime.cpp
    src/worker_pf_helper_runtime.cpp
    src/worker_mask_runtime_callbacks.cpp
    src/worker_pf_path_runtime.cpp
    src/worker_pf_path_selftests.cpp
    src/worker_aegp_render_options.cpp
    src/worker_aegp_render_selftests.cpp
      src/worker_aegp_async_layer_runtime.cpp
      src/worker_aegp_staged_item_runtime.cpp
      src/worker_aegp_external_render_runtime.cpp
      src/worker_aegp_layer_render_runtime.cpp
      src/worker_aegp_item_render_runtime.cpp
    src/worker_aegp_world_selftests.cpp
    src/worker_aegp_init_runtime.cpp
    src/worker_aegp_init_execution.cpp
    src/worker_aegp_init_orchestration.cpp
    src/worker_aegp_init_report.cpp
    src/worker_ui_event_report.cpp
    src/worker_audio_execution.cpp
    src/worker_drawbot_runtime.cpp
    src/worker_param_checkout_runtime.cpp
    src/worker_classic_render_runtime.cpp
    src/worker_host_suite_wiring.cpp
    src/worker_l2_shared_helpers.cpp
    src/worker_l2_payload_parsers.cpp
    src/worker_early_mode_bridge.cpp
    src/worker_entry_wiring.cpp
    src/worker_entry_bootstrap.cpp
    src/worker_effect_bootstrap.cpp
    src/worker_aegp_timeline_probe.cpp
    src/worker_aegp_scene.cpp
    src/worker_aegp_scene_runtime.cpp
    src/worker_aegp_scene_selftests.cpp
    src/worker_aegp_host_selftests.cpp
    src/worker_aegp_compat_selftests.cpp
    src/worker_classic_runtime.cpp
    src/worker_classic_execution.cpp
    src/worker_color_settings_runtime.cpp
    src/worker_color_settings_selftests.cpp
    src/worker_compute_cache_suite.cpp
    src/worker_handle_runtime.cpp
    src/worker_host_suite_router.cpp
    src/worker_host_suite_catalog.cpp
    src/worker_pf_suites.cpp
    src/worker_pf_sampling_runtime.cpp
    src/worker_pf_effect_sequence_selftests.cpp
    src/worker_pf_world_transform_runtime.cpp
    src/worker_pf_adv_time_suite.cpp
    src/worker_pf_ansi_runtime.cpp
    src/worker_pf_ae_channel_runtime.cpp
    src/worker_pf_state_runtime.cpp
    src/worker_report.cpp
    src/worker_cluster_manifest.cpp
    src/worker_request_parser.cpp
    src/worker_render_session.cpp
    src/worker_audio_session.cpp
    src/worker_invocation_orchestration.cpp
    src/worker_render_report.cpp
    src/worker_render_receipts.cpp
    src/worker_suite_abi.cpp
    src/worker_world_registry.cpp
    src/worker_world_safety.cpp)

set_source_files_properties(src/worker_aegp_compute_cache.cpp
  PROPERTIES COMPILE_OPTIONS "/EHsc")
set_source_files_properties(
  ../tests/native/worker_aegp_compute_cache_selftest.cpp
  PROPERTIES COMPILE_OPTIONS "/EHsc")

add_library(aex_worker_runtime_core OBJECT ${AEXCOMPAT_WORKER_RUNTIME_CORE_SOURCES})
target_compile_features(aex_worker_runtime_core PRIVATE cxx_std_17)
target_compile_definitions(aex_worker_runtime_core PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX AEXCOMPAT_COMPILE_AEGP_SCENE_CALLBACKS=1)
target_link_libraries(aex_worker_runtime_core PRIVATE aexcompat_trace_writer)

add_executable(aex_l2_worker src/worker_l2_entry.cpp $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(aex_l2_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_l2_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_l2_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(aex_render_worker src/worker_render_entry.cpp $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(aex_render_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_render_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_render_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(aex_smart_worker src/worker_smart_entry.cpp $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(aex_smart_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_smart_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_smart_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(worker_suite_registry_selftest
  ../tests/native/worker_suite_registry_selftest.cpp
  src/suite_lease_tracker.cpp
  src/worker_aegp_compute_cache.cpp
  src/worker_minidump_runtime.cpp
  src/worker_selector_dispatch.cpp
  src/worker_suite_call_slot_probe.cpp
  src/worker_suite_registry.cpp
)
target_compile_features(worker_suite_registry_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_suite_registry_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_suite_registry_selftest PRIVATE src)
target_link_libraries(worker_suite_registry_selftest PRIVATE
  bcrypt aexcompat_trace_writer)

add_executable(worker_aegp_compute_cache_selftest
  ../tests/native/worker_aegp_compute_cache_selftest.cpp
  src/suite_lease_tracker.cpp
  src/worker_aegp_compute_cache.cpp
  src/worker_minidump_runtime.cpp
  src/worker_selector_dispatch.cpp
  src/worker_suite_call_slot_probe.cpp
  src/worker_suite_registry.cpp
)
target_compile_features(worker_aegp_compute_cache_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_aegp_compute_cache_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_aegp_compute_cache_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_aegp_compute_cache_selftest PRIVATE src)
target_link_libraries(worker_aegp_compute_cache_selftest PRIVATE
  bcrypt aexcompat_trace_writer)

add_executable(aex_string_table_selftest
  ../tests/native/aex_string_table_selftest.cpp
  src/aex_string_table_impl.cpp
)
target_compile_features(aex_string_table_selftest PRIVATE cxx_std_17)
target_include_directories(aex_string_table_selftest PRIVATE src)
target_compile_definitions(aex_string_table_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)

add_executable(extended_inter_memory_selftest
  ../tests/native/extended_inter_memory_selftest.cpp
  src/extended_inter_memory.cpp
)
target_compile_features(extended_inter_memory_selftest PRIVATE cxx_std_17)
target_include_directories(extended_inter_memory_selftest PRIVATE src)

add_executable(worker_classic_runtime_selftest
  ../tests/native/worker_classic_runtime_selftest.cpp
  $<TARGET_OBJECTS:aex_worker_runtime_core>
)
target_compile_features(worker_classic_runtime_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_classic_runtime_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_classic_runtime_selftest PRIVATE src)
target_link_libraries(worker_classic_runtime_selftest PRIVATE
  bcrypt aexcompat_trace_writer)

add_executable(worker_aegp_init_runtime_selftest
  ../tests/native/worker_aegp_init_runtime_selftest.cpp
  src/worker_aegp_init_runtime.cpp
)
target_compile_features(worker_aegp_init_runtime_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_aegp_init_runtime_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_aegp_init_runtime_selftest PRIVATE src)
