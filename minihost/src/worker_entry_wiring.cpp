#include <windows.h>
#include <bcrypt.h>
#include <d3d12.h>
#include <fcntl.h>
#include <io.h>
#include <excpt.h>

#include "trace_writer.hpp"

#include <array>
#include <algorithm>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <cstdarg>
#include <cstdio>
#include <cwchar>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <list>
#include <map>
#include <memory>
#include <mutex>
#include <new>
#include <sstream>
#include <set>
#include <string>
#include <thread>
#include <tuple>
#include <type_traits>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <variant>
#include <vector>

#include "native_stdout_guard.hpp"
#include "gpu_cuda_backend.hpp"
#include "gpu_device_info_registry.hpp"
#include "gpu_directx_backend.hpp"
#include "gpu_opencl_backend.hpp"
#include "gpu_memory_world_transport.hpp"
#include "host_audio_runtime.hpp"
#include "l2_cli_dispatch.h"
#include "l2_mode_execution.hpp"
#include "parameter_animation_transport.hpp"
#include "worker_parameter_runtime.hpp"
#include "worker_parameter_selftests.hpp"
#include "worker_parameter_selftest_routing.hpp"
#include "worker_pf_color_selftests.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_aefx_ace_suite.hpp"
#include "worker_aegp_persistent_data_suite.hpp"
#include "worker_flt_blur_suite.hpp"
#include "worker_ui_event_execution.hpp"
#include "pf_cache_on_load_suite.hpp"
#include "render_lifecycle.hpp"
#include "render_pixel_buffer.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "runtime_module_audit.hpp"
#include "strict_json.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_runtime_admission.hpp"
#include "worker_entry_admission.hpp"
#include "worker_session.hpp"
#include "worker_selftest_dispatch.hpp"
#include "worker_fixed_selftest_routing.hpp"
#include "worker_custom_selftest_routing.hpp"
#include "worker_host_guard_selftests.hpp"
#include "worker_aegp_utility_suite.hpp"
#include "worker_pf_pixel_data_suite.hpp"
#include "worker_pf_world_suite.hpp"
#include "worker_pf_pixel_format_registry.hpp"
#include "worker_pf_param_suites.hpp"
#include "worker_aegp_pf_interface_suite.hpp"
#include "worker_aegp_command_suites.hpp"
#include "worker_mask_suite_tables.hpp"
#include "worker_l2_render_abi.hpp"
#include "worker_classic_report.hpp"
#include "worker_smart_report.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_smart_execution.hpp"
#include "worker_smart_setup.hpp"
#include "worker_smart_dispatch.hpp"
#include "worker_smart_finalize.hpp"
#include "worker_smart_render_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_mask_selftests.hpp"
#include "worker_pf_path_runtime.hpp"
#include "worker_pf_path_selftests.hpp"
#include "worker_minidump_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_render_selftests.hpp"
#include "worker_aegp_async_layer_runtime.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_item_render_runtime.hpp"
#include "worker_aegp_world_selftests.hpp"
#include "worker_aegp_init_runtime.hpp"
#include "worker_aegp_init_execution.hpp"
#include "worker_aegp_init_orchestration.hpp"
#include "worker_aegp_init_report.hpp"
#include "worker_ui_event_report.hpp"
#include "worker_audio_execution.hpp"
#include "worker_drawbot_runtime.hpp"
#include "worker_early_mode_bridge.hpp"
#include "worker_param_checkout_runtime.hpp"
#include "worker_entry_bootstrap.hpp"
#include "worker_effect_bootstrap.hpp"
#include "worker_aegp_timeline_probe.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_host_selftests.hpp"
#include "worker_aegp_compat_selftests.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_classic_runtime.hpp"
#include "worker_classic_execution.hpp"
#include "worker_color_settings_runtime.hpp"
#include "worker_color_settings_selftests.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_host_suite_router.hpp"
#include "worker_host_suite_catalog.hpp"
#include "worker_suite_abi.hpp"
#include "worker_suite_registry.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_pf_world_transform_runtime.hpp"
#include "worker_pf_adv_time_suite.hpp"
#include "worker_pf_ansi_runtime.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_pf_effect_sequence_selftests.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_report.hpp"
#include "worker_render_session.hpp"
#include "worker_request_parser.hpp"
#include "strict_json.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_render_report.hpp"
#include "worker_render_receipts.hpp"
#include "worker_target.hpp"


// Worker-entry wiring moved from worker_main (issue #165): the component
// hook bootstrap, the selftest command dispatch, the AEGP world/receipt
// selftest hook table and its verify bridges, and the PF parameter state
// capture. Host identity (g_effect,
// g_layer, the comp scene objects) stays in l2_main; this TU reaches it
// through cross-TU declarations and the Phase D state owners.
namespace aexcompat::l2_detail {

using aexcompat::parameter_selftests::verify_parameter_animation_transport;
using aexcompat::parameter_selftests::verify_parameter_registry_capacity;
using aexcompat::parameter_selftests::verify_pf_param_utils_suite3;
using namespace aexcompat::pf_ae_channel;
using namespace aexcompat::pf_state_runtime;
using namespace aexcompat::render_options;
using namespace aexcompat::scene_runtime;
using namespace aexcompat::worker_runtime::handles;
using aexcompat::suite_abi::AegpTime;
using AegpLayerEffectBoundary = aexcompat::render_options::LayerEffectBoundary;
using AegpLayerRenderOptionsValue = aexcompat::render_options::LayerValue;
using aexcompat::worker_runtime::capture_module_audit_phase;
using aexcompat::worker_runtime::module_audit_passed;
using aexcompat::world_safety::DispatchWorldFormat;
using aexcompat::world_safety::bounded_argb8_world;
using aexcompat::world_safety::bounded_typed_world;
using aexcompat::world_registry::resolve_dispatch_world_format;
using ParamRecord = aexcompat::worker_runtime::parameters::ParamRecord;
using SmartResult = aexcompat::worker_runtime::smart_execution::Result;
using AegpStreamValue = aexcompat::scene_runtime::AegpStreamValue;

bool world_lifetimes_balanced();

// Host identity and entry-owned helpers that stay in l2_main.
extern OpaqueHostObject g_effect;
bool is_render_worker();
// Asks the production `make_bootstrap_abi_hooks` whether it left a utility
// callback null. Defined beside it in l2_main so the answer comes from the
// assignment list that ships, not from one a test wrote (issue #981).
bool verify_production_utility_callback_table();
void* aegp_comp_item_handle();
bool suite_leases_balanced();
uint32_t suite_acquire_count();
uint32_t suite_release_count();
uint32_t live_suite_reference_count();
int32_t __cdecl acquire_suite(const char* name, int32_t version, const void** suite);
int32_t __cdecl release_suite(const char* name, int32_t version);
void record_selector_dispatch(const char* selector);
uint32_t selftest_trigger_guarded_crash();
std::string escape(const std::string& input);
bool sha256(const std::filesystem::path& path, std::string& result);
std::string sha256_bytes(const unsigned char* data, std::size_t size);
bool __cdecl validate_render_options_item(int32_t plugin_id, void* item);
bool __cdecl initialize_layer_render_options(
    int32_t plugin_id, void* source, AegpLayerEffectBoundary boundary,
    AegpLayerRenderOptionsValue* value);
bool __cdecl scene_initialize_layer_render_options(
    int32_t plugin_id, void* source, int32_t boundary, void* value) noexcept;
struct AegpColorVal { double alpha, red, green, blue; };
int32_t __cdecl aegp_get_comp_bg_color(void* comp, AegpColorVal* color);
int32_t insert_layer_render_options(
    const AegpLayerRenderOptionsValue& value, void** output);
int32_t __cdecl aegp_get_new_effect_stream_by_index_v2(
    int32_t plugin_id, void* effect, int32_t index, void** stream);
int32_t __cdecl aegp_dispose_stream_v2(void* stream);
int32_t __cdecl aegp_get_stream_name_v2(void* stream, uint8_t force_english, char* name);
int32_t __cdecl aegp_get_stream_type_v2(void* stream, int32_t* type);
int32_t __cdecl aegp_get_new_stream_value_v2(
    int32_t plugin_id, void* stream, int32_t time_mode, const AegpTime* time,
    uint8_t pre_expression, AegpStreamValue* output);
int32_t __cdecl aegp_dispose_stream_value_v2(AegpStreamValue* output);
int32_t __cdecl aegp_set_stream_value_v2(
    int32_t plugin_id, void* stream, AegpStreamValue* input);
int32_t __cdecl aegp_get_effect_param_union_by_index_v3(
    int32_t plugin_id, void* effect, int32_t index, int32_t* type, void* param_union);
void raise_mask_access_violation();
aexcompat::mask_runtime::Snapshot mask_runtime_snapshot();
bool snapshot_mask_curve(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve);
bool install_synthetic_mask_scene(
    const std::vector<aexcompat::mask_runtime::CurveSnapshot>& curves);
std::vector<aexcompat::pf_path_runtime::PathInfo> enumerate_pf_paths();
bool snapshot_pf_path(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve);
bool bounded_pf_path_world(void* world, aexcompat::pf_path_runtime::WorldView& view);
bool mask_lifetimes_balanced();

namespace {
constexpr std::size_t kParamSize = 176;
constexpr int32_t kPfBadCallbackParam = 516;
auto& g_parameter_runtime = aexcompat::worker_runtime::parameters::state();
auto& g_params = g_parameter_runtime.records;
auto& g_parameter_timelines = g_parameter_runtime.timelines;
auto& g_async_manager =
    aexcompat::render_receipts::receipt_test_state().async_manager;
auto& g_synthetic_receipt_test_mode =
    aexcompat::render_receipts::receipt_test_state().synthetic_test_mode;
AegpSceneObject& g_aegp_comp_item = scene_runtime_state().composition_item;
AegpSceneObject& g_aegp_comp = scene_runtime_state().composition;
auto& g_host_callback_telemetry =
    aexcompat::worker_runtime::classic::host_callback_telemetry();
auto& g_transform_world_calls = g_host_callback_telemetry.transform_world_calls;
auto& g_last_transform_x = g_host_callback_telemetry.last_transform_x;
auto& g_last_transform_y = g_host_callback_telemetry.last_transform_y;
auto& g_last_transform_opacity = g_host_callback_telemetry.last_transform_opacity;
auto& g_render_context_state = aexcompat::render::render_context_state();
auto& g_full_resolution_width = g_render_context_state.full_resolution_width;
auto& g_full_resolution_height = g_render_context_state.full_resolution_height;

auto& smart_state() { return aexcompat::worker_runtime::smart::state(); }
}  // namespace

const aexcompat::aegp_world_selftests::Hooks& aegp_world_selftest_hooks() {
  static const aexcompat::aegp_world_selftests::Hooks hooks{
      &world_lifetimes_balanced,
      &aegp_comp_item_handle,
      +[](void* item, void** output) { return render_options_new_from_item(1, item, output); },
      &render_timestamp_reject,
      &render_checkin_rendered,
      &render_worthwhile_reject,
      +[](void* options, void** receipt) {
        return render_checkout_frame_reject(options, nullptr, nullptr, receipt);
      },
      &get_receipt_world,
      &checkin_frame,
      &bump_render_project_timestamp,
      &render_options_dispose,
      &aexcompat::aegp_external_render_runtime::cache_empty,
      +[](bool enabled) { g_synthetic_receipt_test_mode = enabled; },
      +[] { return aexcompat::render_receipts::lifetimes_balanced(); },
      +[](int32_t pixel_format, void** output) {
        return aexcompat::aegp_item_render_runtime::publish_synthetic(
            pixel_format, output);
      },
      +[](void** output) {
        return insert_layer_render_options(AegpLayerRenderOptionsValue{}, output);
      },
      +[](void* options, void** receipt) {
        return checkout_layer_frame_async(&g_async_manager, 1, options, receipt);
      },
      &dispose_layer_render_options,
      nullptr};
  return hooks;
}

bool verify_aegp_world_suite3() {
  return aexcompat::aegp_world_selftests::verify_world_suite3(
      aegp_world_selftest_hooks());
}

bool verify_aegp_world_mfr_safety() {
  return aexcompat::aegp_world_selftests::verify_world_mfr_safety(
      aegp_world_selftest_hooks());
}

bool async_receipt_lifetimes_balanced() {
  return aexcompat::render_receipts::lifetimes_balanced();
}

bool verify_aegp_async_receipts() {
  return aexcompat::aegp_world_selftests::verify_async_receipts(
      aegp_world_selftest_hooks());
}
bool render_options_lifetimes_balanced() {
  return item_live_count() == 0 && item_created_count() == item_disposed_count();
}

void clear_staged_item_worlds_for_test() {
  aexcompat::aegp_staged_item_runtime::clear();
}

bool verify_item_render_cycle_contract(void* options) {
  const AegpTime time{5, 24};
  if (!aexcompat::aegp_staged_item_runtime::verify_recursion_guard(
          aegp_comp_item_handle(), time, options, &render_checkout_frame_reject)) return false;
  const uint32_t old_generation =
      aexcompat::aegp_external_render_runtime::project_generation();
  bump_render_project_timestamp();
  void* rejected = reinterpret_cast<void*>(1);
  return aexcompat::aegp_external_render_runtime::project_generation() != old_generation &&
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) != 0 && !rejected &&
      render_options_dispose(options) == 0;
}



bool world_lifetimes_balanced() {
  return aexcompat::world_registry::lifetimes_balanced();
}

bool valid_param_utils_index(int32_t index, bool allow_groups = false) {
  if (allow_groups && index >= -4 && index <= -1) return true;
  return std::any_of(g_params.begin(), g_params.end(),
      [index](const ParamRecord& param) { return param.index == index; });
}

template <typename T>
void append_pf_state_bytes(std::vector<unsigned char>& snapshot, const T& value) {
  const auto* bytes = reinterpret_cast<const unsigned char*>(&value);
  snapshot.insert(snapshot.end(), bytes, bytes + sizeof(value));
}

bool capture_pf_parameter_state(int32_t index,
                                std::vector<unsigned char>& snapshot) {
  try {
    for (const auto& param : g_params) {
      if (index >= 0 && param.index != index) continue;
      if (index == -3 && param.type == 0) continue;
      append_pf_state_bytes(snapshot, param.disk_id);
      append_pf_state_bytes(snapshot, param.type);
      const auto* raw = reinterpret_cast<const unsigned char*>(param.raw.data());
      snapshot.insert(snapshot.end(), raw, raw + param.raw.size());
    }
    for (const auto& timeline : g_parameter_timelines) {
      if (index >= 0 && timeline.slot != index) continue;
      append_pf_state_bytes(snapshot, timeline.slot);
      const std::size_t key_count = timeline.keys.size();
      append_pf_state_bytes(snapshot, key_count);
      for (const auto& key : timeline.keys) {
        append_pf_state_bytes(snapshot, key.time);
        append_pf_state_bytes(snapshot, key.scale);
        append_pf_state_bytes(snapshot, key.hold);
        append_pf_state_bytes(snapshot, key.kind);
        append_pf_state_bytes(snapshot, key.scalar);
        snapshot.insert(snapshot.end(), key.color.begin(), key.color.end());
        const auto* components =
            reinterpret_cast<const unsigned char*>(key.components.data());
        snapshot.insert(snapshot.end(), components,
                        components + sizeof(double) * key.components.size());
        append_pf_state_bytes(snapshot, key.component_count);
      }
    }
  } catch (const std::bad_alloc&) {
    return false;
  }
  return true;
}

bool verify_suite_release_without_acquire_rejected() {
  const uint32_t acquires_before = suite_acquire_count();
  const uint32_t releases_before = suite_release_count();
  const uint32_t live_before = live_suite_reference_count();
  return release_suite("AEGP Layer Mask Suite", 999) != 0 &&
      suite_acquire_count() == acquires_before &&
      suite_release_count() == releases_before &&
      live_suite_reference_count() == live_before;
}

bool verify_pf_effect_sequence_data_suite1() {
  return aexcompat::pf_effect_sequence_selftests::verify_suite1(
      &g_effect, {&acquire_suite, &release_suite,
                  &g_effect_sequence_data_suite1, kPfBadCallbackParam});
}

std::string hex_bytes(const unsigned char* data, std::size_t size) {
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (std::size_t index = 0; index < size; ++index)
    text << std::setw(2) << static_cast<unsigned>(data[index]);
  return text.str();
}

}  // namespace aexcompat::l2_detail

using namespace aexcompat::l2_detail;

// The render-receipt scene gate stays defined in l2_main with the
// worker-kind selector it reads.
bool __cdecl scene_render_receipt_enabled();

bool run_pf_path_data_hardening_selftest() {
  return verify_pf_path_data_hardening(
      {&g_effect, &enumerate_pf_paths, &snapshot_pf_path, &bounded_pf_path_world},
      {&g_layer, &raise_mask_access_violation, &mask_runtime_snapshot,
       &snapshot_mask_curve, &mask_lifetimes_balanced, &install_synthetic_mask_scene});
}

bool run_pf_mask_composition_selftest() {
  return verify_pf_mask_composition(
      {&g_effect, &enumerate_pf_paths, &snapshot_pf_path, &bounded_pf_path_world},
      {&g_layer, &raise_mask_access_violation, &mask_runtime_snapshot,
       &snapshot_mask_curve, &mask_lifetimes_balanced, &install_synthetic_mask_scene});
}

// Component hook wiring for the worker entry (issue #171). Every hooks
// struct the entry used to assemble inline is registered here, keeping
// worker_main_impl to admission, invocation resolution, mode execution,
// report, and exit code.
int configure_worker_entry_bootstrap() {
  SceneSuiteFactoryHooks scene_factory{};
  scene_factory.render_scene_enabled = &scene_render_receipt_enabled;
  scene_factory.comp_bg_color = reinterpret_cast<void*>(&aegp_get_comp_bg_color);
  scene_factory.effect_param_union =
      reinterpret_cast<void*>(&aegp_get_effect_param_union_by_index_v3);
  scene_factory.legacy_stream_callbacks = {{
      reinterpret_cast<void*>(&aegp_get_new_effect_stream_by_index_v2),
      reinterpret_cast<void*>(&aegp_dispose_stream_v2),
      reinterpret_cast<void*>(&aegp_get_stream_name_v2),
      reinterpret_cast<void*>(&aegp_get_stream_type_v2),
      reinterpret_cast<void*>(&aegp_get_new_stream_value_v2),
      reinterpret_cast<void*>(&aegp_dispose_stream_value_v2),
      reinterpret_cast<void*>(&aegp_set_stream_value_v2)}};
  scene_factory.keyframe_callbacks[2] = reinterpret_cast<void*>(&insert_keyframe);
  scene_factory.keyframe_callbacks[3] = reinterpret_cast<void*>(&delete_keyframe);
  scene_factory.keyframe_callbacks[5] = reinterpret_cast<void*>(&set_keyframe_value);
  scene_factory.keyframe_callbacks[6] =
      reinterpret_cast<void*>(&get_stream_value_dimensionality);
  scene_factory.keyframe_callbacks[7] =
      reinterpret_cast<void*>(&get_stream_temporal_dimensionality);
  scene_factory.keyframe_callbacks[8] =
      reinterpret_cast<void*>(&get_new_keyframe_spatial_tangents);
  scene_factory.keyframe_callbacks[9] =
      reinterpret_cast<void*>(&set_keyframe_spatial_tangents);
  scene_factory.keyframe_callbacks[10] =
      reinterpret_cast<void*>(&get_keyframe_temporal_ease);
  scene_factory.keyframe_callbacks[11] =
      reinterpret_cast<void*>(&set_keyframe_temporal_ease);
  scene_factory.keyframe_callbacks[12] = reinterpret_cast<void*>(&get_keyframe_flags);
  scene_factory.keyframe_callbacks[13] = reinterpret_cast<void*>(&set_keyframe_flag);
  scene_factory.keyframe_callbacks[15] =
      reinterpret_cast<void*>(&set_keyframe_interpolation);
  scene_factory.keyframe_callbacks[16] = reinterpret_cast<void*>(&start_add_keyframes);
  scene_factory.keyframe_callbacks[17] = reinterpret_cast<void*>(&add_keyframes);
  scene_factory.keyframe_callbacks[18] = reinterpret_cast<void*>(&set_add_keyframe);
  scene_factory.keyframe_callbacks[19] = reinterpret_cast<void*>(&end_add_keyframes);
  scene_factory.keyframe_callbacks[20] = reinterpret_cast<void*>(&get_keyframe_label);
  scene_factory.keyframe_callbacks[21] = reinterpret_cast<void*>(&set_keyframe_label);

  const SceneContext scene_host{
      {&bump_render_project_timestamp, &validate_render_options_item,
       &scene_initialize_layer_render_options, &suite_leases_balanced,
       &make_utf16_handle, &free_aegp_mem_handle, scene_factory},
      &g_aegp_comp_item, &g_aegp_comp, &g_layer, &g_effect,
      &g_full_resolution_width,
      &g_full_resolution_height, &aexcompat::worker_runtime::smart::width,
      &aexcompat::worker_runtime::smart::height};
  const SceneRuntimeContext scene_runtime_host{
      {&suite_leases_balanced}, &g_aegp_comp_item, &g_aegp_comp,
      &g_full_resolution_width, &g_full_resolution_height,
      &aexcompat::worker_runtime::smart::width,
      &aexcompat::worker_runtime::smart::height};
  const PfHostContext pf_host_context{
      {
          [](void* world, int32_t pixel_bytes, unsigned char*& pixels,
             int32_t& rowbytes, int32_t& width, int32_t& height) -> bool {
            return bounded_typed_world(world, pixel_bytes, pixels, rowbytes, width, height);
          },
          [](const void* world, DispatchWorldFormat& result) -> bool {
            return resolve_dispatch_world_format(world, result);
          },
          []() -> const char* { return smart_state().pixel_format.c_str(); },
          [](const char* value) -> bool {
            if (!value || (std::strcmp(value, "argb8") != 0 &&
                           std::strcmp(value, "argb16") != 0 &&
                           std::strcmp(value, "argb32f") != 0)) return false;
            smart_state().pixel_format = value;
            return true;
          },
          &acquire_suite,
          &release_suite,
      },
      &g_effect,
      &g_batch_sampling_suite1,
  };
  aexcompat::worker_runtime::entry_bootstrap::Hooks bootstrap_hooks{};
  bootstrap_hooks.pf_state = {
      []() -> void* { return &g_effect; },
      [](int32_t index, bool allow_groups) -> bool {
        return valid_param_utils_index(index, allow_groups);
      },
      &capture_pf_parameter_state};
  bootstrap_hooks.pf_ae_channel = {
      []() -> void* { return &g_effect; },
      []() -> std::size_t { return g_params.size(); },
      [](std::size_t index) -> bool {
        return index < g_params.size() && g_params[index].type == 0;
      },
      &sha256};
  bootstrap_hooks.scene = scene_host;
  bootstrap_hooks.scene_runtime = scene_runtime_host;
  bootstrap_hooks.validate_item = &validate_render_options_item;
  bootstrap_hooks.initialize_layer = &initialize_layer_render_options;
  bootstrap_hooks.effect_ref = &g_effect;
  bootstrap_hooks.pf = pf_host_context;
  bootstrap_hooks.world_transform = {
      {pf_host_context.hooks.resolve_world,
       pf_host_context.hooks.resolve_dispatch_world_format,
       pf_host_context.hooks.pixel_format,
       pf_host_context.hooks.set_pixel_format,
       &bounded_argb8_world},
      {&g_transform_world_calls, &g_last_transform_x, &g_last_transform_y,
       &g_last_transform_opacity}};
  bootstrap_hooks.adv_time = {&acquire_suite, &release_suite, &suite_acquire_count,
                              &suite_release_count, &suite_leases_balanced};
  bootstrap_hooks.hash = &sha256;
  bootstrap_hooks.audit_capture = &capture_module_audit_phase;
  bootstrap_hooks.audit_passed = &module_audit_passed;
  bootstrap_hooks.trace = &record_selector_dispatch;
  if (!aexcompat::flt_blur::configure({
          &g_effect,
          [](const void* world, DispatchWorldFormat& result) -> bool {
            return resolve_dispatch_world_format(world, result);
          },
          &acquire_suite,
          &release_suite}))
    return 1;
  return aexcompat::worker_runtime::entry_bootstrap::configure(bootstrap_hooks);
}

// Self-test command dispatch for the worker entry (issue #171): the four
// selftest catalogs and their hook wiring register here; worker_main_impl
// only consumes the optional exit code.
std::optional<int> dispatch_worker_selftests(int argc, wchar_t** argv) {
  const aexcompat::worker_runtime::selftest::AegpHooks aegp_selftests{
      &verify_aegp_projector_levels, &verify_aegp_effect_stack,
      &verify_aegp_apply_effect, &verify_aegp_resizer_3d_chain,
      &verify_aegp_get_effect_camera, &verify_legacy_effect_compat_suites,
      kAegpEffectInstanceCapacity, kAegpEffectLeaseCapacity};
  if (const auto selftest_exit =
          aexcompat::worker_runtime::selftest::dispatch_aegp(
              argc, argv, aegp_selftests))
    return *selftest_exit;
  wchar_t cancel_gate[2]{};
  aexcompat::aegp_async_layer::set_cancel_test_gate(is_render_worker() &&
      GetEnvironmentVariableW(L"AEXCOMPAT_TEST_ASYNC_CANCEL_GATE", cancel_gate,
                              2) == 1 && cancel_gate[0] == L'1');
  SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
  const auto fixed_selftest = aexcompat::worker_runtime::fixed_selftests::dispatch(
      {argc, argv, is_render_worker()},
      {{&escape, &selftest_trigger_guarded_crash, &suite_leases_balanced},
       {&verify_aegp_installed_effect_catalog_suite4,
        &aexcompat::l2_detail::verify_aegp_layer_suite1_slots,
        &aexcompat::l2_detail::verify_aegp_loaded_plugin_effect_streams,
        &verify_parameter_animation_transport, &verify_parameter_registry_capacity,
        &verify_pf_param_utils_suite3,
        &verify_pre_checkout_result_contract,
        &aexcompat::worker_runtime::smart::checkout_intersection_self_test,
        &aexcompat::render::smart_geometry_rect_self_test,
        &aexcompat::worker_runtime::smart::concurrency_self_test,
        +[] {
          const SmartResult skipped{};
          return skipped.runtime && skipped.runtime->pixel_format.empty() &&
              skipped.runtime->input_checkout_request[0] == -1 &&
              skipped.runtime->map_checkout_request[0] == -1;
        },
        &verify_pixel_data_suites, &verify_legacy_fill_matte_callbacks,
        &verify_pf_ae_channel_suite,
        &aexcompat::pf_color_selftests::verify_pf_color_suite,
        &aexcompat::pf_color_selftests::verify_pf_color_param_suite,
        &verify_iterate_suites,
        &verify_world_transform_composite_rect, &verify_world_transform_affine,
        &verify_world_transform_blend, &verify_world_transform_transfer_mask,
        +[] { return verify_aegp_world_suite3() && verify_aegp_world_mfr_safety(); },
        &verify_pf_batch_sampling_suite, &verify_pf_ae_channel_native_provider,
        &verify_aegp_layer_render_options_suite2,
        &verify_utils_handle_callbacks_wired,
        &verify_production_utility_callback_table,
        &aexcompat::flt_blur::selftest, &aexcompat::aefx_ace::selftest,
        &aexcompat::worker_runtime::persistent_data::selftest,
        &aexcompat::worker_runtime::selftest_native_stdout_routing,
        &aexcompat::worker_runtime::persistent_data::selftest4}});
  // Compatibility anchors for selftests whose command catalog now lives in
  // worker_fixed_selftest_routing.cpp.
  // --self-test-world-transform-affine
  // --self-test-world-transform-blend
  // --self-test-world-transform-transfer-mask
  // L"--self-test-pf-checkout-intersection"
  // L"--self-test-pf-smart-geometry-rects"
  if (fixed_selftest.handled) return fixed_selftest.exit_code;
  const auto parameter_selftest =
      aexcompat::worker_runtime::parameter_selftests::dispatch(
          {argc, argv},
          {&verify_aegp_keyframe_suite5_mutations,
           &keyframe_suite5_abi_wiring_valid, &g_keyframe_mutations,
           &g_invalid_keyframe_operations, &mask_lifetimes_balanced});
  if (parameter_selftest.handled) {
    std::cout << parameter_selftest.output;
    return parameter_selftest.exit_code;
  }
  const auto custom_selftest =
      aexcompat::worker_runtime::custom_selftests::dispatch(
          {argc, argv},
          {&run_pf_path_data_hardening_selftest,
           &run_pf_mask_composition_selftest,
           &verify_world_double_dispose_rejected,
           &verify_world_value_semantics,
           &verify_world_allocation_limit_rejected,
           &verify_owned_world_snapshot_is_atomic,
           &verify_owned_world_snapshot_concurrent_dispose,
           &verify_pf_effect_sequence_data_suite1,
           &verify_aegp_async_receipts, &sha256_bytes, &hex_bytes,
           &g_render_options_baseline8, &g_render_options_time8,
           &g_render_options_downsample8, &g_render_options_roi_inside8,
           &g_render_options_matte8, &g_render_options_argb16,
           &g_render_options_argb32f});
  if (custom_selftest.handled) {
    std::cout << custom_selftest.output;
    return custom_selftest.exit_code;
  }
  return std::nullopt;
}
