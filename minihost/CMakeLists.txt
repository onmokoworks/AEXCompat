cmake_minimum_required(VERSION 3.20)

# Pin the MSVC diagnostic language before the compiler is probed (issue #657).
#
# With the Ninja generator, header dependencies are recovered by matching cl's
# `/showIncludes` lines against the `msvc_deps_prefix` CMake captured while
# probing the compiler. That prefix is localized: a configure run whose cl
# speaks Japanese records those bytes, and a later build whose cl speaks
# English (or the same words under a different console codepage) matches
# nothing. Ninja then records ZERO header dependencies per object and reports
# "no work to do" forever, so a header edit never rebuilds its dependents.
# The build directory keeps accumulating objects compiled against older
# headers while looking up to date, and the linked worker mixes them.
#
# That is not hypothetical: it produced a worker whose GLOBAL_SETUP dispatch
# access-violated for every AEX, and AviUtl2 registered zero filters (#651).
# `ninja -t deps` on that directory reported `#deps 0` for every object,
# including the ones rebuilt the same day.
#
# Setting VSLANG here fixes the language CMake probes; the launcher below fixes
# the language the build itself emits. Both are needed - they are separate
# processes with separate environments, and the failure is exactly the two
# disagreeing. An operator who has already chosen a language is left alone.
set(AEXCOMPAT_VSLANG "$ENV{VSLANG}")
string(STRIP "${AEXCOMPAT_VSLANG}" AEXCOMPAT_VSLANG)
# An operator's own value is honored, but only when it is a bare LCID: it is
# forwarded into every compile below, so anything else is replaced rather than
# passed through.
if(NOT AEXCOMPAT_VSLANG MATCHES "^[0-9]+$")
  set(AEXCOMPAT_VSLANG 1033)
endif()
set(ENV{VSLANG} "${AEXCOMPAT_VSLANG}")

project(AEXCompatMinihost LANGUAGES CXX)

# The build-time half of the pin: every compile runs through `cmake -E env`, so
# cl emits the same language whatever shell drives ninja. Without this, the
# prefix agrees only as long as configure and build happen to share a console.
#
# A launcher the operator already chose (ccache and friends) wins: replacing it
# would silently disable their tooling, and only this generator+compiler pair
# has the localization hazard to begin with.
if(MSVC AND CMAKE_GENERATOR MATCHES "Ninja" AND NOT CMAKE_CXX_COMPILER_LAUNCHER)
  set(CMAKE_CXX_COMPILER_LAUNCHER
      "${CMAKE_COMMAND}" -E env "VSLANG=${AEXCOMPAT_VSLANG}")
endif()

# Read every source as UTF-8 instead of as the operator's code page (issue
# #713).
#
# Without this, cl reads a source with no BOM as the current code page. On a
# Japanese-locale Windows that is cp932, where 0x81-0x9F and 0xE0-0xFC are lead
# bytes: a UTF-8 character whose last byte is one of those, sitting immediately
# before a bare LF, pairs with the newline. The line comment then runs into the
# next line and the declaration there disappears. That removed a `constexpr`
# from worker_smart_dispatch.cpp and broke this build on a clean checkout
# (#708) - invisibly, because a CRLF working copy absorbs the lead byte and
# still ends the line, and because no CI job builds minihost.
#
# `/utf-8` also sets the execution charset. Nothing in `minihost/src` has a
# non-ASCII string or character literal - every non-ASCII byte is inside a
# comment - so no emitted bytes change; only how the compiler reads the file
# does. It also retires the C4819 warnings those comments produce.
if(MSVC)
  add_compile_options(/utf-8)
endif()

# Build provenance embedded in every worker (issue #649).
#
# The broker admits a locally built worker by comparing `minihost/src` mtimes
# against the worker's own. That comparison needs the sources to be present, so
# a worker shipped beside the plugin - with no repository around it - can only
# ever come back "indeterminate" and is refused. Recording what the worker was
# built from, in the worker, gives the broker something to judge that survives
# leaving the build tree behind.
#
# Resolved at configure time, so `CMAKE_CONFIGURE_DEPENDS` below re-runs it when
# HEAD moves - the same contract `guest/crates/aex-guest-worker/build.rs` gets
# from `cargo:rerun-if-changed`.
set(AEXCOMPAT_REPOSITORY_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/..")
# `-DAEXCOMPAT_BUILD_REVISION=` as well as the environment variable, so pinning
# a release build does not depend on which one the operator reaches for.
if(NOT DEFINED AEXCOMPAT_BUILD_REVISION)
  set(AEXCOMPAT_BUILD_REVISION "$ENV{AEXCOMPAT_BUILD_REVISION}")
endif()
string(STRIP "${AEXCOMPAT_BUILD_REVISION}" AEXCOMPAT_BUILD_REVISION)
set(AEXCOMPAT_BUILD_DIRTY "0")

find_package(Git QUIET)
if(GIT_FOUND)
  if(AEXCOMPAT_BUILD_REVISION STREQUAL "")
    execute_process(
      COMMAND "${GIT_EXECUTABLE}" rev-parse --short=12 HEAD
      WORKING_DIRECTORY "${AEXCOMPAT_REPOSITORY_ROOT}"
      OUTPUT_VARIABLE AEXCOMPAT_BUILD_REVISION
      OUTPUT_STRIP_TRAILING_WHITESPACE
      ERROR_QUIET)
  endif()
  # Asked unconditionally, including when the revision was pinned: a pinned
  # revision does not make the tree clean, and the broker's own build script
  # asks unconditionally too. Recording "clean" on either side while the other
  # records "dirty" is the two halves disagreeing about the same tree.
  # Fails closed: a `git status` that could not run records dirty. Reading a
  # failed query as "clean" would let a modified tree assert an identity it has
  # not earned, and the broker's build script fails closed the same way.
  execute_process(
    COMMAND "${GIT_EXECUTABLE}" status --porcelain --untracked-files=no
    WORKING_DIRECTORY "${AEXCOMPAT_REPOSITORY_ROOT}"
    OUTPUT_VARIABLE AEXCOMPAT_GIT_STATUS
    RESULT_VARIABLE AEXCOMPAT_GIT_STATUS_RESULT
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET)
  if(NOT AEXCOMPAT_GIT_STATUS_RESULT EQUAL 0 OR NOT AEXCOMPAT_GIT_STATUS STREQUAL "")
    set(AEXCOMPAT_BUILD_DIRTY "1")
  endif()
  # Re-configure when HEAD moves, so the worker's revision does not silently
  # fall behind the broker's. The branch ref matters most: committing on the
  # current branch rewrites `refs/heads/<branch>` and leaves `.git/HEAD`
  # untouched, so watching HEAD alone would miss every ordinary commit.
  execute_process(
    COMMAND "${GIT_EXECUTABLE}" symbolic-ref -q HEAD
    WORKING_DIRECTORY "${AEXCOMPAT_REPOSITORY_ROOT}"
    OUTPUT_VARIABLE AEXCOMPAT_GIT_BRANCH_REF
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET)
  foreach(AEXCOMPAT_GIT_REF HEAD packed-refs "${AEXCOMPAT_GIT_BRANCH_REF}")
    if(AEXCOMPAT_GIT_REF STREQUAL "")
      continue()
    endif()
    execute_process(
      COMMAND "${GIT_EXECUTABLE}" rev-parse --git-path "${AEXCOMPAT_GIT_REF}"
      WORKING_DIRECTORY "${AEXCOMPAT_REPOSITORY_ROOT}"
      OUTPUT_VARIABLE AEXCOMPAT_GIT_REF_PATH
      OUTPUT_STRIP_TRAILING_WHITESPACE
      ERROR_QUIET)
    if(AEXCOMPAT_GIT_REF_PATH STREQUAL "")
      continue()
    endif()
    # `--git-path` answers relative to the repository root, while `if(EXISTS)`
    # resolves against the cmake process's own directory. Anchor it, or a
    # configure run started from anywhere else registers no dependency at all
    # and the revision then never refreshes.
    if(NOT IS_ABSOLUTE "${AEXCOMPAT_GIT_REF_PATH}")
      set(AEXCOMPAT_GIT_REF_PATH
          "${AEXCOMPAT_REPOSITORY_ROOT}/${AEXCOMPAT_GIT_REF_PATH}")
    endif()
    if(EXISTS "${AEXCOMPAT_GIT_REF_PATH}")
      set_property(DIRECTORY APPEND PROPERTY
        CMAKE_CONFIGURE_DEPENDS "${AEXCOMPAT_GIT_REF_PATH}")
    endif()
  endforeach()
endif()
# An unknown revision is not a failure: a source archive with no git around it
# still builds, and the broker treats "unknown" as "cannot identify" rather than
# as a match.
if(AEXCOMPAT_BUILD_REVISION STREQUAL "")
  set(AEXCOMPAT_BUILD_REVISION "unknown")
endif()
configure_file(
  "${CMAKE_CURRENT_SOURCE_DIR}/src/worker_build_provenance.hpp.in"
  "${CMAKE_CURRENT_BINARY_DIR}/worker_build_provenance.hpp"
  @ONLY)

add_library(aexcompat_trace_writer STATIC
  ../instruments/common/trace_writer.cpp
)
target_compile_features(aexcompat_trace_writer PUBLIC cxx_std_17)
target_include_directories(aexcompat_trace_writer PUBLIC
  ../instruments/common
)

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
    src/worker_flt_blur_suite.cpp
    src/worker_pf_private_callbacks.cpp
    src/worker_aefx_ace_suite.cpp
    src/worker_pf_private_effect_suite.cpp
    src/worker_runtime_admission.cpp
    src/worker_entry_admission.cpp
    src/worker_session.cpp
    src/worker_selftest_dispatch.cpp
    src/worker_fixed_selftest_routing.cpp
    src/worker_custom_selftest_routing.cpp
    src/worker_host_guard_selftests.cpp
    src/worker_openmp_policy.cpp
    src/worker_aegp_compute_cache.cpp
    src/worker_aegp_utility_suite.cpp
    src/worker_aegp_persistent_data_suite.cpp
    src/worker_pf_pixel_data_suite.cpp
    src/worker_pf_world_suite.cpp
    src/worker_pf_pixel_format_registry.cpp
    src/worker_pf_param_suites.cpp
    src/worker_aegp_pf_interface_suite.cpp
    src/worker_bee_scene_facade.cpp
    src/worker_pf_progress_info.cpp
    src/worker_pf_world_facade.cpp
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
    src/worker_pf_gaussian_kernel.cpp
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
    src/worker_aegp_entry_guard.cpp
    src/worker_aegp_init_orchestration.cpp
    src/worker_aegp_init_report.cpp
    src/worker_ui_event_report.cpp
    src/worker_system_sound_suppression.cpp
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
    src/worker_aegp_scene_model.cpp
    src/worker_aegp_scene_transaction.cpp
    src/worker_aegp_scene.cpp
    src/worker_aegp_scene_runtime.cpp
    src/worker_aegp_scene_selftests.cpp
    src/worker_active_plugin_context.cpp
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
    src/worker_dynamic_suite_registry.cpp
    src/worker_companion_manifest.cpp
    src/worker_companion_runtime.cpp
    src/worker_cor_ace_profile.cpp
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
set_source_files_properties(src/l2_main.cpp PROPERTIES OBJECT_DEPENDS
  "${CMAKE_CURRENT_SOURCE_DIR}/src/l2_main_support.inc;${CMAKE_CURRENT_SOURCE_DIR}/src/l2_main_entry.inc")
set_source_files_properties(
  ../tests/native/worker_aegp_compute_cache_selftest.cpp
  PROPERTIES COMPILE_OPTIONS "/EHsc")

# Keep a recorded "clean" honest. A configure run only re-runs when something in
# `CMAKE_CONFIGURE_DEPENDS` changes, so without this an edit after a clean
# configure leaves the worker asserting the commit it no longer matches - and
# that assertion is what admits it where no sources are present to check.
#
# Registered only when the tree was clean: a dirty build already records `1`, and
# a stale `1` can only over-restrict. So the reconfigure cost falls on clean
# builds - the release case, where being right is the point - and not on the
# edit-build loop, which is dirty by definition.
if(AEXCOMPAT_BUILD_DIRTY STREQUAL "0")
  foreach(AEXCOMPAT_PROVENANCE_SOURCE ${AEXCOMPAT_WORKER_RUNTIME_CORE_SOURCES})
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS
      "${CMAKE_CURRENT_SOURCE_DIR}/${AEXCOMPAT_PROVENANCE_SOURCE}")
  endforeach()
endif()

add_library(aex_worker_runtime_core OBJECT
  ${AEXCOMPAT_WORKER_RUNTIME_CORE_SOURCES}
  src/worker_build_provenance.cpp)
target_compile_features(aex_worker_runtime_core PRIVATE cxx_std_17)
# Where the configured provenance header lands. Object-library sources are all
# linked into every worker, so each one carries the marker.
target_include_directories(aex_worker_runtime_core PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
target_compile_definitions(aex_worker_runtime_core PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX AEXCOMPAT_COMPILE_AEGP_SCENE_CALLBACKS=1)
target_link_libraries(aex_worker_runtime_core PRIVATE aexcompat_trace_writer)

add_executable(aex_l2_worker src/worker_l2_entry.cpp $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(aex_l2_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_l2_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_l2_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(worker_report_json_selftest
    ../tests/native/worker_report_json_selftest.cpp
    src/worker_report.cpp)
target_compile_features(worker_report_json_selftest PRIVATE cxx_std_17)
target_include_directories(worker_report_json_selftest PRIVATE src)

add_executable(l2_mode_execution_selftest
    ../tests/native/l2_mode_execution_selftest.cpp
    src/l2_mode_execution.cpp)
target_compile_features(l2_mode_execution_selftest PRIVATE cxx_std_17)
target_include_directories(l2_mode_execution_selftest PRIVATE src)
target_compile_options(l2_mode_execution_selftest PRIVATE /UNDEBUG)

add_executable(worker_temporal_checkout_report_selftest
    ../tests/native/worker_temporal_checkout_report_selftest.cpp)
target_compile_features(worker_temporal_checkout_report_selftest PRIVATE cxx_std_17)
target_include_directories(worker_temporal_checkout_report_selftest PRIVATE src)

add_executable(aex_render_worker src/worker_render_entry.cpp $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(aex_render_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_render_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_render_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(aex_smart_worker src/worker_smart_entry.cpp $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(aex_smart_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_smart_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_smart_worker PRIVATE bcrypt aexcompat_trace_writer)

add_executable(worker_classic_suite_report_selftest
    ../tests/native/worker_classic_suite_report_selftest.cpp
    $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(worker_classic_suite_report_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_classic_suite_report_selftest PRIVATE
    UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_classic_suite_report_selftest PRIVATE src)
target_link_libraries(worker_classic_suite_report_selftest PRIVATE
    bcrypt aexcompat_trace_writer)

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

add_library(worker_session_detach_fixture SHARED
  ../tests/native/worker_session_detach_fixture.cpp)
target_compile_features(worker_session_detach_fixture PRIVATE cxx_std_17)
target_compile_definitions(worker_session_detach_fixture PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)

add_executable(worker_session_contained_exit_selftest
  ../tests/native/worker_session_contained_exit_selftest.cpp
  $<TARGET_OBJECTS:aex_worker_runtime_core>)
target_compile_features(worker_session_contained_exit_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_session_contained_exit_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_session_contained_exit_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_session_contained_exit_selftest PRIVATE src)
target_link_libraries(worker_session_contained_exit_selftest PRIVATE
  bcrypt aexcompat_trace_writer)

add_executable(worker_aegp_scene_model_selftest
  ../tests/native/worker_aegp_scene_model_selftest.cpp
  src/worker_aegp_scene_model.cpp
  src/worker_aegp_scene_transaction.cpp
)
target_compile_features(worker_aegp_scene_model_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_aegp_scene_model_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_aegp_scene_model_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_aegp_scene_model_selftest PRIVATE src)

add_executable(worker_aegp_entry_guard_selftest
  ../tests/native/worker_aegp_entry_guard_selftest.cpp
  src/worker_aegp_entry_guard.cpp
)
target_compile_features(worker_aegp_entry_guard_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_aegp_entry_guard_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_aegp_entry_guard_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_aegp_entry_guard_selftest PRIVATE src)

add_executable(aex_string_table_selftest
  ../tests/native/aex_string_table_selftest.cpp
  src/aex_string_table_impl.cpp
)
target_compile_features(aex_string_table_selftest PRIVATE cxx_std_17)
target_include_directories(aex_string_table_selftest PRIVATE src)
target_compile_definitions(aex_string_table_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)

add_executable(worker_active_plugin_context_selftest
  ../tests/native/worker_active_plugin_context_selftest.cpp
  src/worker_active_plugin_context.cpp
)
target_compile_features(worker_active_plugin_context_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_active_plugin_context_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_active_plugin_context_selftest PRIVATE src)

add_executable(rust_host_core_abi_selftest
  ../tests/native/rust_host_core_abi_selftest.cpp
)
target_compile_features(rust_host_core_abi_selftest PRIVATE cxx_std_17)
target_include_directories(rust_host_core_abi_selftest PRIVATE
  ../broker/crates/broker/include)

add_executable(rust_host_core_ffi_dual_run_selftest
  ../tests/native/rust_host_core_ffi_dual_run_selftest.cpp
)
target_compile_features(rust_host_core_ffi_dual_run_selftest PRIVATE cxx_std_17)
target_compile_definitions(rust_host_core_ffi_dual_run_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(rust_host_core_ffi_dual_run_selftest PRIVATE
  ../broker/crates/broker/include)

add_executable(extended_inter_memory_selftest
  ../tests/native/extended_inter_memory_selftest.cpp
  src/extended_inter_memory.cpp
)
target_compile_features(extended_inter_memory_selftest PRIVATE cxx_std_17)
target_include_directories(extended_inter_memory_selftest PRIVATE src)

add_executable(worker_param_checkout_time_selftest
  ../tests/native/worker_param_checkout_time_selftest.cpp
  src/worker_param_checkout_runtime.cpp
  src/worker_classic_runtime.cpp
  src/worker_parameter_runtime.cpp
)
target_compile_features(worker_param_checkout_time_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_param_checkout_time_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_param_checkout_time_selftest PRIVATE src)

add_executable(worker_pf_sampling_wiring_selftest
  ../tests/native/worker_pf_sampling_wiring_selftest.cpp
  src/worker_pf_sampling_runtime.cpp
  # worker_pf_sampling_runtime calls pf_world_transform::copy_world8, defined in
  # worker_pf_world_transform_runtime, which in turn needs world_safety's
  # DispatchWorldFormatScope. Without both TUs the target fails to link (#892).
  src/worker_pf_world_transform_runtime.cpp
  src/worker_world_safety.cpp
  # world_safety's register_world attaches the PF_World facade (#1276).
  src/worker_pf_world_facade.cpp
  src/worker_effect_bootstrap.cpp
)
target_compile_features(worker_pf_sampling_wiring_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_pf_sampling_wiring_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_pf_sampling_wiring_selftest PRIVATE src)

add_executable(worker_cross_thread_dispatch_world_selftest
  ../tests/native/worker_cross_thread_dispatch_world_selftest.cpp
  src/worker_world_safety.cpp
  # world_safety's register_world gives each handed-out world AE's PF_World
  # shape behind reserved_long4 (#1276).
  src/worker_pf_world_facade.cpp
)
target_compile_features(worker_cross_thread_dispatch_world_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_cross_thread_dispatch_world_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_cross_thread_dispatch_world_selftest PRIVATE src)

add_executable(worker_pf_bad_callback_param_selftest
  ../tests/native/worker_pf_bad_callback_param_selftest.cpp
  src/worker_pf_world_transform_runtime.cpp
  src/worker_world_safety.cpp
  src/worker_pf_world_facade.cpp
)
target_compile_features(worker_pf_bad_callback_param_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_pf_bad_callback_param_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_pf_bad_callback_param_selftest PRIVATE src)

add_executable(worker_classic_execution_selftest
  ../tests/native/worker_classic_execution_selftest.cpp
  src/worker_classic_execution.cpp
)
target_compile_features(worker_classic_execution_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_classic_execution_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_classic_execution_selftest PRIVATE src)

add_executable(host_audio_runtime_selftest
  ../tests/native/host_audio_runtime_selftest.cpp
  src/host_audio_runtime.cpp
  src/render_subsystem.cpp
  # render_subsystem's prepare_world_layout embeds the PF_World facade (#1276).
  src/worker_pf_world_facade.cpp
  src/worker_effect_bootstrap.cpp
)
target_compile_features(host_audio_runtime_selftest PRIVATE cxx_std_17)
target_compile_definitions(host_audio_runtime_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(host_audio_runtime_selftest PRIVATE src)

add_executable(worker_effect_bootstrap_timing_selftest
  ../tests/native/worker_effect_bootstrap_timing_selftest.cpp
  src/worker_effect_bootstrap.cpp
)
target_compile_features(worker_effect_bootstrap_timing_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_effect_bootstrap_timing_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_effect_bootstrap_timing_selftest PRIVATE src)

add_executable(worker_render_lifecycle_selftest
  ../tests/native/worker_render_lifecycle_selftest.cpp
  src/render_lifecycle.cpp
  src/render_subsystem.cpp
  src/worker_pf_world_facade.cpp
)
target_compile_features(worker_render_lifecycle_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_render_lifecycle_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_render_lifecycle_selftest PRIVATE src)

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

add_executable(worker_handle_runtime_selftest
  ../tests/native/worker_handle_runtime_selftest.cpp
  src/worker_handle_runtime.cpp
)
target_compile_features(worker_handle_runtime_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_handle_runtime_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_handle_runtime_selftest PRIVATE src)
target_link_libraries(worker_handle_runtime_selftest PRIVATE
  aexcompat_trace_writer)

add_executable(worker_openmp_policy_selftest
  ../tests/native/worker_openmp_policy_selftest.cpp
  src/worker_openmp_policy.cpp
)
target_compile_features(worker_openmp_policy_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_openmp_policy_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_openmp_policy_selftest PRIVATE src)

add_executable(worker_legacy_support_init_selftest
  ../tests/native/worker_legacy_support_init_selftest.cpp
)
target_compile_features(worker_legacy_support_init_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_legacy_support_init_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_legacy_support_init_selftest PRIVATE src)

add_executable(worker_sweetpea_bootstrap_selftest
  ../tests/native/worker_sweetpea_bootstrap_selftest.cpp
)
target_compile_features(worker_sweetpea_bootstrap_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_sweetpea_bootstrap_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_sweetpea_bootstrap_selftest PRIVATE src)

add_executable(worker_aegp_init_runtime_selftest
  ../tests/native/worker_aegp_init_runtime_selftest.cpp
  src/worker_aegp_init_runtime.cpp
)
target_compile_features(worker_aegp_init_runtime_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_aegp_init_runtime_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_aegp_init_runtime_selftest PRIVATE src)

add_executable(worker_aegp_register_suite_selftest
  ../tests/native/worker_aegp_register_suite_selftest.cpp
  src/worker_aegp_command_suites.cpp
  src/worker_aegp_init_runtime.cpp
)
target_compile_features(worker_aegp_register_suite_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_aegp_register_suite_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_aegp_register_suite_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_aegp_register_suite_selftest PRIVATE src)

add_executable(worker_dynamic_suite_registry_selftest
  ../tests/native/worker_dynamic_suite_registry_selftest.cpp
  src/worker_aegp_entry_guard.cpp
  src/worker_aegp_init_runtime.cpp
  src/worker_companion_runtime.cpp
  src/worker_dynamic_suite_registry.cpp
  src/worker_host_suite_catalog.cpp
  src/worker_host_suite_router.cpp
  src/worker_pf_ansi_runtime.cpp
  src/suite_lease_tracker.cpp
  src/worker_aegp_compute_cache.cpp
  src/worker_minidump_runtime.cpp
  src/worker_selector_dispatch.cpp
  src/worker_suite_call_slot_probe.cpp
  src/worker_suite_registry.cpp
)
add_library(worker_dynamic_suite_companion_fixture SHARED
  ../tests/native/worker_dynamic_suite_companion_fixture.cpp)
add_library(worker_dynamic_suite_consumer_fixture SHARED
  ../tests/native/worker_dynamic_suite_consumer_fixture.cpp)
target_compile_features(worker_dynamic_suite_companion_fixture PRIVATE cxx_std_17)
target_compile_features(worker_dynamic_suite_consumer_fixture PRIVATE cxx_std_17)
target_compile_definitions(worker_dynamic_suite_companion_fixture PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_definitions(worker_dynamic_suite_consumer_fixture PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_include_directories(worker_dynamic_suite_companion_fixture PRIVATE
  ../tests/native)
target_include_directories(worker_dynamic_suite_consumer_fixture PRIVATE
  ../tests/native)
add_dependencies(worker_dynamic_suite_registry_selftest
  worker_dynamic_suite_companion_fixture
  worker_dynamic_suite_consumer_fixture)
target_compile_features(worker_dynamic_suite_registry_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_dynamic_suite_registry_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_dynamic_suite_registry_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_dynamic_suite_registry_selftest PRIVATE
  src ../tests/native)
target_link_libraries(worker_dynamic_suite_registry_selftest PRIVATE
  bcrypt aexcompat_trace_writer)

add_executable(worker_companion_manifest_selftest
  ../tests/native/worker_companion_manifest_selftest.cpp
  src/worker_companion_manifest.cpp
  src/strict_json.cpp)
target_compile_features(worker_companion_manifest_selftest PRIVATE cxx_std_17)
target_compile_definitions(worker_companion_manifest_selftest PRIVATE
  UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_compile_options(worker_companion_manifest_selftest PRIVATE /UNDEBUG)
target_include_directories(worker_companion_manifest_selftest PRIVATE src)
