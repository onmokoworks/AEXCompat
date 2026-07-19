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
#include <cstring>
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
#include "pf_cache_on_load_suite.hpp"
#include "render_lifecycle.hpp"
#include "render_pixel_buffer.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "runtime_module_audit.hpp"
#include "strict_json.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_runtime_admission.hpp"
#include "worker_session.hpp"
#include "worker_selftest_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_minidump_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_init_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_classic_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_host_suite_router.hpp"
#include "worker_suite_abi.hpp"
#include "worker_suite_registry.hpp"
#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_pf_adv_time_suite.hpp"
#include "worker_report.hpp"
#include "worker_request_parser.hpp"
#include "worker_render_report.hpp"
#include "worker_render_receipts.hpp"
#include "worker_target.hpp"

// Internal worker implementation uses a named namespace so subsystem
// translation units can own callback state without including implementation
// fragments into this file.
namespace aexcompat::l2_detail {

aexcompat::worker_target::Kind g_worker_target =
    aexcompat::worker_target::Kind::L2;

bool is_render_worker() {
  return g_worker_target == aexcompat::worker_target::Kind::Render;
}

bool is_smart_worker() {
  return g_worker_target == aexcompat::worker_target::Kind::Smart;
}

bool is_rendering_worker() {
  return is_render_worker() || is_smart_worker();
}

namespace opencl = aexcompat::gpu_runtime::opencl;

using aexcompat::strict_json::JsonValue;
using aexcompat::strict_json::StrictJsonParser;
using aexcompat::strict_json::json_exact_keys;
using aexcompat::strict_json::json_i32;
using aexcompat::strict_json::json_member;
using aexcompat::strict_json::json_string;
using aexcompat::strict_json::json_u64;
using aexcompat::parameter_animation::AnimationKey;
using aexcompat::parameter_animation::AnimationValueKind;
using aexcompat::parameter_animation::ParameterAnimationKey;
using aexcompat::parameter_animation::ParameterTimeline;
using aexcompat::parameter_animation::load_parameter_animation;
using aexcompat::parameter_animation::rational_less;
using aexcompat::suites::cache_on_load_suite;
using aexcompat::suites::configure_cache_on_load_suite;
using aexcompat::gpu_runtime::device_info_registry;
namespace directx_backend = aexcompat::gpu_runtime::directx_backend;
using aexcompat::suite_abi::AegpLayerRenderOptionsSuite1;
using aexcompat::suite_abi::AegpLayerRenderOptionsSuite2;
using aexcompat::suite_abi::AegpRect;
using aexcompat::suite_abi::AegpRenderOptionsSuite1;
using aexcompat::suite_abi::AegpRenderOptionsSuite4;
using aexcompat::suite_abi::AegpTime;
using AegpLayerEffectBoundary = aexcompat::render_options::LayerEffectBoundary;
using AegpLayerRenderOptionsValue = aexcompat::render_options::LayerValue;
using AegpRenderOptionsValue = aexcompat::render_options::ItemValue;
using namespace aexcompat::render_options;
using namespace aexcompat::scene_runtime;
constexpr std::size_t kMaxRenderOptions = 32;
void* aegp_comp_item_handle();
bool snapshot_render_options(void* handle, AegpRenderOptionsValue& value) {
  return snapshot_item(handle, value);
}
bool snapshot_layer_render_options(void* handle, AegpLayerRenderOptionsValue& value) {
  return snapshot_layer(handle, value);
}
int32_t insert_layer_render_options(
    const AegpLayerRenderOptionsValue& value, void** output) {
  return insert_layer_value(value, output);
}
using aexcompat::gpu_runtime::cuda_backend;
using aexcompat::gpu_runtime::CudaDevicePointer;
using aexcompat::gpu_runtime::gpu_get_device_count;
using aexcompat::gpu_runtime::gpu_get_device_info;
using aexcompat::gpu_runtime::kMaxGpuDevices;
using aexcompat::host_audio::checkout_layer_audio;
using aexcompat::host_audio::checkin_layer_audio;
using aexcompat::host_audio::get_audio_data;
using namespace aexcompat::worker_runtime::handles;
using aexcompat::render_safety::InputPixelBuffer;
using aexcompat::render_safety::OutputPixelBuffer;
using aexcompat::render_lifecycle::RenderLifecycle;
using aexcompat::worker_runtime::redirect_native_stdout;
using aexcompat::worker_runtime::restore_native_stdout;
using aexcompat::worker_runtime::configure_selector_dispatch_audit;
using aexcompat::worker_runtime::configure_selector_dispatch_trace;
using aexcompat::worker_runtime::configure_runtime_module_hash;
using aexcompat::worker_runtime::capture_module_audit;
using aexcompat::worker_runtime::capture_module_audit_phase;
using aexcompat::worker_runtime::effect_selector_name;
using aexcompat::worker_runtime::guarded_effect_call;
using aexcompat::worker_runtime::invoke_entry_seh;
using aexcompat::worker_runtime::invoke_smart_pre_render_cleanup_seh;
using aexcompat::worker_runtime::module_audit_json;
using aexcompat::worker_runtime::module_audit_passed;
using aexcompat::worker_runtime::module_audit_report;
using aexcompat::worker_runtime::selector_dispatch_telemetry;
using aexcompat::worker_runtime::RuntimeAdmissionRequest;
using aexcompat::worker_runtime::RuntimeContext;
using aexcompat::worker_runtime::RuntimeHostHooks;
using aexcompat::worker_runtime::SuiteResolveResult;
using aexcompat::worker_runtime::suite_registry;
using aexcompat::worker_runtime::WorkerSession;
using SmartRuntimeSession = aexcompat::worker_runtime::smart::Session;
using aexcompat::worker_runtime::smart::checkout_output;
using aexcompat::worker_runtime::smart::checkout_pixels;
using aexcompat::worker_runtime::smart::checkin_pixels;
using aexcompat::worker_runtime::smart::pre_checkout_layer;
using aexcompat::worker_runtime::admit_runtime;
using aexcompat::render_receipts::ReceiptDraft;
using aexcompat::render_receipts::ReceiptSnapshot;
using aexcompat::render_receipts::kMaxReceiptBytes;
using aexcompat::world_safety::DispatchWorldFormat;
using aexcompat::world_safety::DispatchWorldFormatScope;
using aexcompat::world_safety::LocalEffectWorld;
using aexcompat::world_safety::LocalRationalScale;
using aexcompat::world_safety::bounded_argb8_world;
using aexcompat::world_safety::bounded_typed_world;
using aexcompat::world_safety::kEffectWorldSize;
using aexcompat::world_safety::resolve_registered_dispatch_world;
using aexcompat::world_registry::dispose_world;
using aexcompat::world_registry::get_pixel_format;
using aexcompat::world_registry::kPixelFormatArgb32;
using aexcompat::world_registry::kPixelFormatArgb64;
using aexcompat::world_registry::kPixelFormatArgb128;
using aexcompat::world_registry::kPixelFormatGpuBgra128;
using aexcompat::world_registry::legacy_new_world;
using aexcompat::world_registry::new_world;
using aexcompat::world_registry::resolve_dispatch_world_format;
using aexcompat::world_registry::PlatformWorldBacking;
using aexcompat::world_registry::aegp_world_dispose;
using aexcompat::world_registry::aegp_world_dispose_platform;
using aexcompat::world_registry::aegp_world_fast_blur;
using aexcompat::world_registry::aegp_world_fill_pf_world;
using aexcompat::world_registry::aegp_world_get_base_addr8;
using aexcompat::world_registry::aegp_world_get_base_addr16;
using aexcompat::world_registry::aegp_world_get_base_addr32;
using aexcompat::world_registry::aegp_world_get_rowbytes;
using aexcompat::world_registry::aegp_world_get_size;
using aexcompat::world_registry::aegp_world_get_type;
using aexcompat::world_registry::aegp_world_new_owned;
using aexcompat::world_registry::aegp_world_new_platform;
using aexcompat::world_registry::aegp_world_reference_platform;
using aexcompat::world_registry::aegp_world_type_from_format;
using aexcompat::render_pixel_transport::argb_to_rgba8;
using aexcompat::render_pixel_transport::argb_to_rgba_native;
using aexcompat::render_pixel_transport::rgba8_to_argb;

auto& g_module_audit = module_audit_report();

auto& smart_state() { return aexcompat::worker_runtime::smart::state(); }

void bump_render_project_timestamp();
constexpr std::size_t kInSize = 408;
constexpr std::size_t kOutSize = 408;
constexpr std::size_t kParamSize = 176;
// Modern AE has no fixed parameter-count limit; retain a high bounded host cap.
constexpr std::size_t kMaxParams = 1024;
constexpr std::size_t kInAddParam = 16;
constexpr std::size_t kInUtils = 176;
constexpr std::size_t kInEffectRef = 184;
constexpr std::size_t kInQuality = 192;
constexpr std::size_t kInVersion = 196;
constexpr std::size_t kInCurrentTime = 224;
constexpr std::size_t kInTimeStep = 228;
constexpr std::size_t kInLocalTimeStep = 236;
constexpr std::size_t kInTimeScale = 240;
constexpr std::size_t kInApplicationId = 204;
constexpr int16_t kHostSpecMajor = 13;
constexpr int16_t kHostSpecMinor = 28;
constexpr std::size_t kInNumParams = 208;
constexpr std::size_t kInGlobalData = 312;
constexpr std::size_t kInSequenceData = 320;
constexpr std::size_t kInFrameData = 328;
constexpr std::size_t kInPicaBasic = 384;
constexpr std::size_t kOutGlobalData = 40;
constexpr std::size_t kOutNumParams = 48;
constexpr std::size_t kOutSequenceData = 56;
constexpr std::size_t kOutFrameData = 72;
constexpr std::size_t kOutWidth = 80;
constexpr std::size_t kOutHeight = 84;
constexpr std::size_t kOutOrigin = 88;
constexpr std::size_t kOutFlags = 96;
constexpr std::size_t kOutMessage = 100;
constexpr std::size_t kOutFlags2 = 400;
constexpr uint32_t kOutFlagDeepColorAware = 1u << 25;
constexpr uint32_t kOutFlagWideTimeInput = 1u << 1;
constexpr uint32_t kOutFlagIDoDialog = 1u << 5;
constexpr uint32_t kOutFlagSendDoDialog = 1u << 7;
constexpr uint32_t kOutFlagDisplayErrorMessage = 1u << 8;
constexpr uint32_t kOutFlagNopRender = 1u << 18;
constexpr uint32_t kOutFlagIWriteInputBuffer = 1u << 11;
constexpr uint32_t kOutFlagIExpandBuffer = 1u << 9;
constexpr uint32_t kOutFlagIShrinkBuffer = 1u << 12;
constexpr uint32_t kOutFlagIUseShutterAngle = 1u << 19;
constexpr uint32_t kOutFlagIUseAudio = 1u << 20;
constexpr uint32_t kOutFlagAudioEffectOnly = 1u << 31;
constexpr uint32_t kOutFlag2SupportsSmartRender = 1u << 10;
constexpr uint32_t kOutFlag2AutomaticWideTimeInput = 1u << 17;
constexpr uint32_t kOutFlag2FloatColorAware = 1u << 12;
constexpr std::size_t kParamType = 12;
constexpr std::size_t kParamUiFlags = 4;
constexpr std::size_t kParamName = 16;
constexpr std::size_t kParamNameSize = 32;
constexpr std::size_t kParamFlags = 48;
constexpr int32_t kGlobalSetup = 1;
constexpr int32_t kGlobalSetdown = 3;
constexpr int32_t kParamsSetup = 4;
constexpr int32_t kAbout = 0;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceResetup = 6;
constexpr int32_t kSequenceFlatten = 7;
constexpr int32_t kSequenceSetdown = 8;
constexpr int32_t kDoDialog = 9;
constexpr int32_t kFrameSetup = 10;
constexpr int32_t kFrameSetdown = 12;
constexpr int32_t kRender = 11;
constexpr int32_t kUserChangedParam = 13;
constexpr int32_t kUpdateParamsUi = 14;
constexpr int32_t kQueryDynamicFlags = 18;
constexpr int32_t kEvent = 15;
constexpr int32_t kGetExternalDependencies = 16;
constexpr int32_t kArbitraryCallback = 22;
constexpr int32_t kGetFlattenedSequenceData = 28;
constexpr int32_t kAudioRender = 19;
constexpr int32_t kAudioSetup = 20;
constexpr int32_t kAudioSetdown = 21;
constexpr int32_t kSmartPreRender = 23;
constexpr int32_t kSmartRender = 24;
constexpr int32_t kSmartRenderGpu = 31;
constexpr int32_t kGpuDeviceSetup = 32;
constexpr int32_t kGpuDeviceSetdown = 33;
constexpr std::size_t kUtilsSize = 552;
constexpr std::size_t kUtilsSubpixelSample = 8;
constexpr std::size_t kUtilsBeginSampling = 0;
constexpr std::size_t kUtilsAreaSample = 16;
constexpr std::size_t kUtilsEndSampling = 32;
constexpr std::size_t kUtilsBlend = 48;
constexpr std::size_t kUtilsConvolve = 56;
constexpr std::size_t kUtilsCopy = 64;
constexpr std::size_t kUtilsFill = 72;
constexpr std::size_t kUtilsIterate = 88;
constexpr std::size_t kUtilsPremultiply = 96;
constexpr std::size_t kUtilsPremultiplyColor = 104;
constexpr std::size_t kUtilsNewWorld = 112;
constexpr std::size_t kUtilsDisposeWorld = 120;
constexpr std::size_t kUtilsTransformWorld = 152;
constexpr std::size_t kUtilsAnsiCeil = 224;
constexpr std::size_t kUtilsAnsiFabs = 248;
constexpr std::size_t kUtilsAnsiPow = 296;
constexpr std::size_t kUtilsAnsiSin = 304;
constexpr std::size_t kUtilsAnsiSprintf = 328;
constexpr std::size_t kUtilsAnsiStrcpy = 336;
constexpr std::size_t kUtilsColorCallbacks = 368;
constexpr std::size_t kUtilsNewHandle = 160;
constexpr std::size_t kUtilsLockHandle = 168;
constexpr std::size_t kUtilsUnlockHandle = 176;
constexpr std::size_t kUtilsDisposeHandle = 184;
constexpr std::size_t kUtilsGetPlatformData = 432;
static_assert(kUtilsColorCallbacks + sizeof(PfColorCallbacks8) == kUtilsGetPlatformData);
constexpr std::size_t kUtilsFill16 = 488;
constexpr std::size_t kUtilsPremultiplyColor16 = 496;
constexpr std::size_t kUtilsGetPixelData8 = 528;
constexpr std::size_t kUtilsGetPixelData16 = 536;

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using AegpEntry = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t, void**);
using AddParamCallback = int32_t(__cdecl*)(void*, int32_t, void*);

auto& g_last_seh_exception_code = selector_dispatch_telemetry().seh_code;
auto& g_last_seh_exception_address = selector_dispatch_telemetry().seh_address;
auto& g_last_seh_exception_module = selector_dispatch_telemetry().seh_module;
auto& g_last_seh_selector = selector_dispatch_telemetry().selector;
auto& g_last_seh_error = selector_dispatch_telemetry().error;
aexcompat::TraceWriter* g_trace_writer{};

void record_selector_dispatch(const char* selector) {
  if (g_trace_writer && selector) g_trace_writer->selector_dispatch(selector);
}

const char* trace_worker_label() {
  switch (g_worker_target) {
    case aexcompat::worker_target::Kind::Render: return "aex_render_worker";
    case aexcompat::worker_target::Kind::Smart: return "aex_smart_worker";
    case aexcompat::worker_target::Kind::L2: return "aex_l2_worker";
  }
  return "aex_l2_worker";
}

int capture_seh_exception(EXCEPTION_POINTERS* information) {
  return aexcompat::worker_runtime::minidump::capture_seh_exception(
      information, {g_last_seh_exception_code, g_last_seh_exception_address,
                    g_last_seh_exception_module});
}

// Raises a real access violation under the production __except filter so the
// crash-minidump path can be exercised end to end. Kept in its own function
// because a frame mixing __try with unwindable C++ objects will not compile.
uint32_t selftest_trigger_guarded_crash() {
  __try {
    volatile int* target = nullptr;
    *target = 1;
    return 0;
  } __except (capture_seh_exception(GetExceptionInformation())) {
    return GetExceptionCode();
  }
}

// Keep every direct EffectMain selector call on the same audited boundary.
#define entry(...) guarded_effect_call(entry, __VA_ARGS__)

using ParamRecord = aexcompat::worker_runtime::parameters::ParamRecord;
using RequestedKind = aexcompat::worker_runtime::parameters::RequestedKind;
using RequestedAssignment = aexcompat::worker_runtime::parameters::RequestedAssignment;
using RequestedAssignments = aexcompat::worker_runtime::parameters::RequestedAssignments;
static_assert(aexcompat::worker_runtime::parameters::kDefinitionSize == kParamSize);
auto& g_parameter_runtime = aexcompat::worker_runtime::parameters::state();
auto& g_params = g_parameter_runtime.records;
auto& g_parameter_timelines = g_parameter_runtime.timelines;
auto& g_keyframe_checkout_ledger = g_parameter_runtime.keyframe_checkout_ledger;
auto& g_keyframe_checkout_mutex = g_parameter_runtime.keyframe_checkout_mutex;
const ParameterTimeline *parameter_timeline(int32_t slot) {
  return aexcompat::worker_runtime::parameters::timeline(slot);
}
auto& g_arbitrary_copy_calls = g_parameter_runtime.arbitrary.copy_calls;
auto& g_arbitrary_dispose_calls = g_parameter_runtime.arbitrary.dispose_calls;
auto& g_invalid_arbitrary_operations = g_parameter_runtime.arbitrary.invalid_operations;
auto& g_arbitrary_print_calls = g_parameter_runtime.arbitrary.print_calls;
auto& g_arbitrary_print_failures = g_parameter_runtime.arbitrary.print_failures;
auto& g_arbitrary_roundtrip_calls = g_parameter_runtime.arbitrary.roundtrip_calls;
auto& g_arbitrary_roundtrip_failures = g_parameter_runtime.arbitrary.roundtrip_failures;
auto& g_arbitrary_scan_calls = g_parameter_runtime.arbitrary.scan_calls;
auto& g_arbitrary_scan_failures = g_parameter_runtime.arbitrary.scan_failures;
auto& g_arbitrary_compare_disagreements = g_parameter_runtime.arbitrary.compare_disagreements;
auto& g_arbitrary_new_calls = g_parameter_runtime.arbitrary.new_calls;
auto& g_arbitrary_interpolation_calls = g_parameter_runtime.arbitrary.interpolation_calls;
auto& g_arbitrary_interpolation_failures = g_parameter_runtime.arbitrary.interpolation_failures;
auto& g_last_arbitrary_interpolation_amount = g_parameter_runtime.arbitrary.last_interpolation_amount;
std::wstring g_plugin_file_path;
const aexcompat::host_audio::Telemetry& audio_telemetry() {
  return aexcompat::host_audio::runtime().telemetry();
}
bool audio_handle_lifetimes_balanced() {
  return aexcompat::host_audio::runtime().lifetimes_balanced();
}
auto& g_update_params_ui_advertised = g_parameter_runtime.ui.update_advertised;
auto& g_query_dynamic_flags_advertised = g_parameter_runtime.ui.dynamic_flags_advertised;
auto& g_conditional_ui_selectors_dispatched = g_parameter_runtime.ui.conditional_selectors_dispatched;
auto& g_update_params_ui_error = g_parameter_runtime.ui.update_error;
auto& g_query_dynamic_flags_error = g_parameter_runtime.ui.dynamic_flags_error;
auto& g_active_ui_params = g_parameter_runtime.ui.active_params;
auto& g_active_ui_param_count = g_parameter_runtime.ui.active_param_count;
auto& g_update_params_ui_active = g_parameter_runtime.ui.update_active;
auto& g_user_changed_param_active = g_parameter_runtime.ui.user_changed_active;
auto& g_update_param_ui_calls = g_parameter_runtime.ui.update_calls;
auto& g_user_changed_param_requested = g_parameter_runtime.ui.user_changed_requested;
auto& g_user_changed_param_slot = g_parameter_runtime.ui.user_changed_slot;
auto& g_user_changed_param_error = g_parameter_runtime.ui.user_changed_error;
auto& g_aegp_init_runtime = aexcompat::worker_runtime::aegp_init::state();
auto& g_aegp_init_mode = g_aegp_init_runtime.init_mode;
bool& g_aegp_update_menu_mode = scene_runtime_state().update_menu_mode;
auto& g_aegp_idle_mode = g_aegp_init_runtime.idle_mode;
bool& g_aegp_command_roundtrip_mode = scene_runtime_state().command_roundtrip_mode;
bool& g_aegp_active_idle_roundtrip_mode = scene_runtime_state().active_idle_roundtrip_mode;
bool& g_aegp_comp_idle_roundtrip_mode = scene_runtime_state().comp_idle_roundtrip_mode;
bool g_aegp_keyframe_roundtrip_mode = false;
bool g_aegp_seek_roundtrip_mode = false;
bool g_aegp_trim_roundtrip_mode = false;
bool g_aegp_switch_roundtrip_mode = false;
bool g_skip_about = false;
uint32_t g_aegp_commands_created = 0;
uint32_t g_aegp_menu_commands_inserted = 0;
auto& g_aegp_command_hooks = g_aegp_init_runtime.command_hooks;
auto& g_aegp_update_menu_hooks = g_aegp_init_runtime.update_menu_hooks;
auto& g_aegp_idle_hooks = g_aegp_init_runtime.idle_hooks;
auto& g_aegp_death_hooks = g_aegp_init_runtime.death_hooks;
int32_t g_next_aegp_command = 10000;
uint32_t g_aegp_command_enable_calls = 0;
uint32_t g_aegp_command_check_calls = 0;
uint32_t g_aegp_command_checked_true_calls = 0;
uint32_t g_aegp_command_checked_false_calls = 0;
uint32_t& g_aegp_item_current_time_calls = scene_runtime_state().item_current_time_calls;
uint32_t& g_aegp_item_set_current_time_calls = scene_runtime_state().item_set_current_time_calls;
int32_t& g_aegp_item_last_set_time_value = scene_runtime_state().item_last_set_time_value;
uint32_t& g_aegp_item_last_set_time_scale = scene_runtime_state().item_last_set_time_scale;
uint32_t& g_aegp_item_name_calls = scene_runtime_state().item_name_calls;
uint32_t& g_aegp_item_duration_calls = scene_runtime_state().item_duration_calls;
uint32_t& g_aegp_item_type_calls = scene_runtime_state().item_type_calls;
uint32_t& g_aegp_comp_from_item_calls = scene_runtime_state().comp_from_item_calls;
uint32_t& g_aegp_comp_framerate_calls = scene_runtime_state().comp_framerate_calls;
uint32_t& g_aegp_layer_count_calls = scene_runtime_state().layer_count_calls;
uint32_t& g_aegp_layer_by_index_calls = scene_runtime_state().layer_by_index_calls;
uint32_t& g_aegp_layer_source_item_calls = scene_runtime_state().layer_source_item_calls;
uint32_t& g_aegp_layer_id_calls = scene_runtime_state().layer_id_calls;
uint32_t& g_aegp_layer_attribute_calls = scene_runtime_state().layer_attribute_calls;
uint32_t& g_aegp_layer_trim_set_calls = scene_runtime_state().layer_trim_set_calls;
uint32_t& g_aegp_layer_flag_set_calls = scene_runtime_state().layer_flag_set_calls;
auto& g_aegp_layer_flags = scene_runtime_state().layer_flags;
uint32_t& g_aegp_layer_name_calls = scene_runtime_state().layer_name_calls;
uint32_t& g_aegp_effect_count_calls = scene_runtime_state().effect_count_calls;
uint32_t& g_aegp_effect_acquires = scene_runtime_state().effect_acquires;
uint32_t& g_aegp_effect_disposes = scene_runtime_state().effect_disposes;
uint32_t& g_aegp_effect_metadata_calls = scene_runtime_state().effect_metadata_calls;
uint32_t& g_aegp_stream_acquires = scene_runtime_state().stream_acquires;
uint32_t& g_aegp_stream_disposes = scene_runtime_state().stream_disposes;
uint32_t& g_aegp_stream_value_acquires = scene_runtime_state().stream_value_acquires;
uint32_t& g_aegp_stream_value_disposes = scene_runtime_state().stream_value_disposes;
uint32_t& g_aegp_stream_sampled_selector_mask = scene_runtime_state().stream_sampled_selector_mask;
uint32_t& g_aegp_effect_param_name_calls = scene_runtime_state().effect_param_name_calls;
uint32_t& g_aegp_effect_param_value_calls = scene_runtime_state().effect_param_value_calls;
uint32_t g_aegp_effect_param_union_calls = 0;
uint32_t& g_aegp_keyframe_count_calls = scene_runtime_state().keyframe_count_calls;
uint32_t& g_aegp_keyframed_stream_reports = scene_runtime_state().keyframed_stream_reports;
uint32_t& g_aegp_keyframe_time_calls = scene_runtime_state().keyframe_time_calls;
uint32_t& g_aegp_keyframe_value_calls = scene_runtime_state().keyframe_value_calls;
uint32_t& g_aegp_keyframe_interpolation_calls = scene_runtime_state().keyframe_interpolation_calls;
uint32_t& g_aegp_collection_creates = scene_runtime_state().collection_creates;
uint32_t& g_aegp_collection_disposes = scene_runtime_state().collection_disposes;
uint32_t& g_aegp_collection_item_reads = scene_runtime_state().collection_item_reads;
int32_t& g_aegp_scene_frame = scene_runtime_state().scene_frame;
int32_t& g_aegp_first_observed_frame = scene_runtime_state().first_observed_frame;
int32_t& g_aegp_last_observed_frame = scene_runtime_state().last_observed_frame;
auto& g_aegp_update_menu_registrations = g_aegp_init_runtime.update_menu_registrations;
using AegpCommandRegistration =
    aexcompat::worker_runtime::aegp_init::CommandRegistration;
using AegpUpdateMenuRegistration =
    aexcompat::worker_runtime::aegp_init::UpdateMenuRegistration;
auto& g_aegp_idle_registrations = g_aegp_init_runtime.idle_registrations;
auto& g_aegp_death_registrations = g_aegp_init_runtime.death_registrations;
auto& g_aegp_command_registrations = g_aegp_init_runtime.command_registrations;
std::vector<int32_t> g_aegp_inserted_commands;
auto& g_checkout_layer_definitions = g_parameter_runtime.checkout.definitions;
auto& g_param_checkout_mutex = g_parameter_runtime.checkout.mutex;
auto& g_live_param_checkouts = g_parameter_runtime.checkout.live;
auto& g_param_checkout_calls = g_parameter_runtime.checkout.checkout_calls;
auto& g_param_checkin_calls = g_parameter_runtime.checkout.checkin_calls;
auto& g_automatic_param_checkins = g_parameter_runtime.checkout.automatic_checkins;
auto& g_invalid_param_checkins = g_parameter_runtime.checkout.invalid_checkins;
auto& g_rejected_temporal_param_checkouts = g_parameter_runtime.checkout.rejected_temporal;
auto& g_wide_time_checkout_allowed = g_parameter_runtime.checkout.wide_time_allowed;
bool g_classic_shutter_dependency_advertised = false;
auto& g_checkout_current_time = g_parameter_runtime.checkout.current_time;
auto& g_checkout_current_time_scale = g_parameter_runtime.checkout.current_time_scale;
auto& g_last_param_checkout_index = g_parameter_runtime.checkout.last_index;
auto& g_last_param_checkout_time = g_parameter_runtime.checkout.last_time;
auto& g_last_param_checkout_time_step = g_parameter_runtime.checkout.last_time_step;
auto& g_last_param_checkout_time_scale = g_parameter_runtime.checkout.last_time_scale;
auto& g_options_button_name = g_parameter_runtime.ui.options_button_name;
auto& g_options_button_name_calls = g_parameter_runtime.ui.options_button_name_calls;
std::atomic<uint32_t> g_channel_count_queries{0};
uint32_t g_duck_quacks = 0;
uint32_t g_transform_world_calls = 0;
int32_t g_last_transform_x = 0;
int32_t g_last_transform_y = 0;
uint8_t g_last_transform_opacity = 0;
uint32_t g_abort_calls = 0;
uint32_t g_progress_calls = 0;
uint32_t g_register_ui_calls = 0;
struct CustomUiRegistration {
  uint32_t events{};
  int32_t comp_width{};
  int32_t comp_height{};
  int32_t comp_alignment{};
  int32_t layer_width{};
  int32_t layer_height{};
  int32_t layer_alignment{};
  int32_t preview_width{};
  int32_t preview_height{};
  int32_t preview_alignment{};
};
CustomUiRegistration g_custom_ui_registration;
uint32_t g_invalid_custom_ui_registrations{};
uint32_t g_adv_app_info_text_calls{};
std::string g_last_adv_app_info_text;
int32_t g_last_progress_current = 0;
int32_t g_last_progress_total = 0;
int32_t g_secondary_layer_slot = 6;
using ExternalLayerInput =
    aexcompat::worker_runtime::request_parser::LayerInput;
bool parse_layer_transport_key(const wchar_t* text, ExternalLayerInput& layer) {
  if (!text) return false;
  if (std::wstring(text).compare(0, 3, L"v1|") != 0) {
    try { layer.slot = std::stoi(text); } catch (...) { return false; }
    return layer.slot > 0;
  }
  int consumed = 0;
  if (swscanf_s(text, L"v1|%d|%d|%u%n", &layer.slot, &layer.time,
          &layer.time_scale, &consumed) != 3 || text[consumed] != L'\0' ||
      layer.slot <= 0 || layer.time_scale == 0) return false;
  layer.timed = true;
  return true;
}

bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale) {
  return static_cast<int64_t>(left) * right_scale ==
      static_cast<int64_t>(right) * left_scale;
}
// Opt-in world snapshot dumps and output checksum detail (issue #19). Both
// default off; the broker enables them per run with the --dump-worlds-v1 and
// --output-checksum-detail-v1 argv trailers, and the dump directory is
// broker-managed. Raw pixel bytes never enter the JSON report; only counts,
// row CRCs, and channel digests do.
std::filesystem::path g_dump_worlds_dir;
uint32_t g_world_dumps_written = 0;
uint32_t g_world_dumps_skipped = 0;
uint64_t g_world_dump_bytes = 0;
bool g_output_checksum_detail = false;
std::vector<uint32_t> g_output_row_crc32;
std::array<std::string, 4> g_output_channel_sha256;
bool g_mask_model_enabled = false;
std::string g_mask_scene_id = "none";
struct SpatialRatio { int32_t numerator{1}; uint32_t denominator{1}; };
SpatialRatio g_downsample_x;
SpatialRatio g_downsample_y;
SpatialRatio g_pixel_aspect_ratio;
int32_t g_full_resolution_width{};
int32_t g_full_resolution_height{};
int32_t g_pre_effect_source_origin_x{};
int32_t g_pre_effect_source_origin_y{};
int32_t g_render_quality{1};
int32_t g_render_field{};
int32_t g_shutter_angle{};
int32_t g_shutter_phase{};

static_assert(sizeof(MaskFeather) == 40);
static_assert(offsetof(MaskFeather, segment_s) == 8);
static_assert(offsetof(MaskFeather, radius) == 16);
static_assert(offsetof(MaskFeather, interp) == 32);
static_assert(offsetof(MaskFeather, type) == 33);
static_assert(sizeof(HostTime) == 8);
static_assert(sizeof(StreamValue) == 40);
OutlineData* sampled_outline(HostStreamRef* stream, const HostTime* time,
                             std::unique_ptr<OutlineData>& owned);
OpaqueHostObject g_effect{0x45464658};
OpaqueHostObject g_layer{0x4c415952};

void raise_mask_access_violation() {
  RaiseException(EXCEPTION_ACCESS_VIOLATION, 0, 0, nullptr);
}
std::unordered_map<HostMask*, uint32_t> g_pf_path_checkouts;
uint32_t g_pf_path_checkout_calls{};
uint32_t g_pf_path_checkin_calls{};
uint32_t g_pf_path_mask_calls{};
uint32_t g_invalid_pf_path_operations{};
int32_t g_pf_path_reject_reason{};
double g_pf_path_last_feather_x{};
double g_pf_path_last_feather_y{};
double g_pf_path_last_opacity{};
int32_t g_pf_path_last_quality{};
std::array<int32_t, 4> g_pf_path_last_bounds{};
struct PfPathSegPrep {
  OpaqueHostObject opaque{0x50534547};
  HostMask* path{};
  int32_t segment{};
  std::array<std::array<double, 2>, 4> controls{};
  std::vector<double> parameters;
  std::vector<std::array<double, 2>> points;
  std::vector<double> cumulative_lengths;
};
std::list<PfPathSegPrep> g_pf_path_segment_preps;
std::mutex g_pf_path_segment_preps_mutex;
uint32_t g_pf_path_preps_created{};
uint32_t g_pf_path_preps_disposed{};
constexpr std::size_t kMaxPfPathSegmentPreps = 256;
constexpr int32_t kPfOutOfMemory = 4;
constexpr int32_t kPfBadCallbackParam = 516;
constexpr int32_t kPfSuiteToolNone = 0;
bool g_render_ui_context_active{};
using aexcompat::pf_helper::reset;

aexcompat::mask_runtime::Snapshot mask_runtime_snapshot() {
  aexcompat::mask_runtime::Snapshot snapshot;
  snapshot.active_masks = static_cast<uint32_t>(std::count_if(
      g_mask_scene.begin(), g_mask_scene.end(), [](const auto& mask) { return !mask.deleted; }));
  snapshot.masks_acquired = g_mask_lifetime.masks_acquired;
  snapshot.masks_disposed = g_mask_lifetime.masks_disposed;
  snapshot.streams_acquired = g_mask_lifetime.streams_acquired;
  snapshot.streams_disposed = g_mask_lifetime.streams_disposed;
  snapshot.values_acquired = g_mask_lifetime.values_acquired;
  snapshot.values_disposed = g_mask_lifetime.values_disposed;
  snapshot.mask_mutations = g_mask_mutations;
  snapshot.invalid_mask_operations = g_invalid_mask_operations;
  snapshot.outline_mutations = g_outline_mutations;
  snapshot.invalid_outline_operations = g_invalid_outline_operations;
  snapshot.keyframe_mutations = g_keyframe_mutations;
  snapshot.invalid_keyframe_operations = g_invalid_keyframe_operations;
  return snapshot;
}

bool snapshot_mask_curve(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve) {
  HostMask* mask = find_mask(handle);
  if (!mask || mask->deleted) return false;
  aexcompat::mask_runtime::CurveSnapshot candidate;
  candidate.id = mask->id;
  candidate.open = mask->open;
  candidate.vertices.reserve(mask->vertices.size());
  for (const auto& vertex : mask->vertices) {
    candidate.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
                                  vertex.tangent_in_y, vertex.tangent_out_x,
                                  vertex.tangent_out_y});
  }
  curve = std::move(candidate);
  return true;
}

std::size_t distinct_vertex_count(const OutlineData& mask) {
  return mask.vertices.size() - static_cast<std::size_t>(!mask.open && !mask.vertices.empty());
}

void sync_closed_vertex(OutlineData& mask) {
  if (!mask.open && !mask.vertices.empty()) mask.vertices.back() = mask.vertices.front();
}

bool mask_lifetimes_balanced() {
  return g_mask_lifetime.masks_acquired == g_mask_lifetime.masks_disposed &&
      g_mask_lifetime.streams_acquired == g_mask_lifetime.streams_disposed &&
      g_mask_lifetime.values_acquired == g_mask_lifetime.values_disposed &&
      g_stream_refs.empty() && g_stream_values.empty() && g_add_keyframe_transactions.empty() &&
      std::none_of(g_mask_scene.begin(), g_mask_scene.end(), [](const auto& mask) {
        return mask.mask_live || mask.stream_live || mask.value_live;
      });
}

bool pf_path_lifetimes_balanced() {
  return g_pf_path_checkout_calls == g_pf_path_checkin_calls &&
      g_pf_path_checkouts.empty() &&
      g_pf_path_preps_created == g_pf_path_preps_disposed &&
      g_pf_path_segment_preps.empty();
}

bool configure_mask_scene(const std::string& scene_id) {
  aexcompat::mask_runtime::configure_host_context(
      {&g_layer, &raise_mask_access_violation, &mask_runtime_snapshot,
       &snapshot_mask_curve});
  if (!g_stream_refs.empty() || !g_stream_values.empty() ||
      !g_add_keyframe_transactions.empty()) return false;
  aexcompat::mask_runtime::SceneSeed seed;
  if (!aexcompat::mask_runtime::build_scene_seed(scene_id, seed)) return false;
  g_mask_scene.clear();
  g_mask_scene.reserve(kMaxHostMasks);
  g_mask_lifetime = {};
  g_mask_scene_id = seed.id;
  for (auto& source : seed.masks) {
    HostMask mask;
    mask.id = g_next_mask_id++;
    mask.outline_stream_id = g_next_stream_id++;
    mask.feather_stream_id = g_next_stream_id++;
    mask.opacity_stream_id = g_next_stream_id++;
    mask.expansion_stream_id = g_next_stream_id++;
    mask.open = source.open;
    mask.dynamic_order = source.dynamic_order;
    mask.vertices.reserve(source.vertices.size());
    for (const auto& vertex : source.vertices) {
      mask.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
          vertex.tangent_in_y, vertex.tangent_out_x, vertex.tangent_out_y});
    }
    g_mask_scene.push_back(std::move(mask));
  }
  return true;
}

std::vector<HostMask*> ordered_active_masks() {
  std::vector<HostMask*> masks;
  for (auto& mask : g_mask_scene) if (!mask.deleted) masks.push_back(&mask);
  std::sort(masks.begin(), masks.end(), [](const HostMask* left, const HostMask* right) {
    return left->dynamic_order < right->dynamic_order;
  });
  return masks;
}

HostMask* find_mask(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.mask; });
  return found == g_mask_scene.end() ? nullptr : &*found;
}
HostStreamRef* find_stream(void* handle) {
  const auto found = std::find_if(g_stream_refs.begin(), g_stream_refs.end(),
      [handle](auto& stream) { return handle == &stream.opaque; });
  return found == g_stream_refs.end() ? nullptr : &*found;
}
OutlineData* find_outline(void* handle) {
  const auto found = std::find_if(g_mask_scene.begin(), g_mask_scene.end(),
      [handle](auto& mask) { return handle == &mask.outline; });
  if (found != g_mask_scene.end()) return &*found;
  for (auto& mask : g_mask_scene) {
    const auto key = std::find_if(mask.keyframes.begin(), mask.keyframes.end(),
        [handle](auto& item) { return handle == &item.outline; });
    if (key != mask.keyframes.end()) return &*key;
  }
  for (auto& item : g_stream_values) {
    if (item.second.outline && handle == &item.second.outline->outline)
      return item.second.outline;
  }
  return nullptr;
}

std::size_t mask_open_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted && mask.open; }));
}

std::size_t active_mask_count() {
  return static_cast<std::size_t>(std::count_if(g_mask_scene.begin(), g_mask_scene.end(),
      [](const auto& mask) { return !mask.deleted; }));
}

std::size_t mask_tangent_vertex_count() {
  std::size_t count = 0;
  for (const auto& mask : g_mask_scene) {
    if (mask.deleted) continue;
    const auto end = !mask.open && !mask.vertices.empty()
        ? mask.vertices.end() - 1 : mask.vertices.end();
    count += static_cast<std::size_t>(std::count_if(mask.vertices.begin(), end,
        [](const auto& vertex) {
          return vertex.tangent_in_x != 0 || vertex.tangent_in_y != 0 ||
                 vertex.tangent_out_x != 0 || vertex.tangent_out_y != 0;
        }));
  }
  return count;
}

void write_rect(void* destination, int32_t width, int32_t height) {
  auto* bytes = static_cast<std::byte*>(destination);
  const int32_t values[4] = {0, 0, width, height};
  std::memcpy(bytes, values, sizeof(values));
}

// PF_CheckoutResult is 76 bytes: result_rect @0, max_result_rect @16,
// par (rational num/den) @32, solid + 3 reserved bytes @40, ref_width @44,
// ref_height @48, 6 reserved longs @52. Callers pass the struct uninitialized,
// so every field must be written, not only the rects. ref_width/ref_height are
// the pre-downsample layer dimensions, which differ from the checkout world
// size when a spatial context supplies a full resolution.
constexpr size_t kCheckoutResultBytes = 76;

constexpr uint32_t kMaxGuidMixInBytes = 1024 * 1024;
std::atomic<uint32_t> g_comp_bg_color_successes{};
std::atomic<uint32_t> g_comp_bg_color_rejections{};
std::atomic<uint32_t> g_guid_mix_in_calls{};
std::atomic<uint32_t> g_guid_mix_in_successes{};
std::atomic<uint32_t> g_guid_mix_in_rejections{};
std::atomic<uint32_t> g_guid_mix_in_last_size{};
std::atomic<uint32_t> g_guid_mix_in_max_size{};
std::atomic<int32_t> g_guid_mix_in_last_result{};

void reset_smart_host_telemetry() {
  g_comp_bg_color_successes.store(0, std::memory_order_relaxed);
  g_comp_bg_color_rejections.store(0, std::memory_order_relaxed);
  g_guid_mix_in_calls.store(0, std::memory_order_relaxed);
  g_guid_mix_in_successes.store(0, std::memory_order_relaxed);
  g_guid_mix_in_rejections.store(0, std::memory_order_relaxed);
  g_guid_mix_in_last_size.store(0, std::memory_order_relaxed);
  g_guid_mix_in_max_size.store(0, std::memory_order_relaxed);
  g_guid_mix_in_last_result.store(0, std::memory_order_relaxed);
}

int32_t __cdecl guid_mix_in_ptr(void* effect_ref, uint32_t size, const void* bytes) {
  g_guid_mix_in_calls.fetch_add(1, std::memory_order_relaxed);
  g_guid_mix_in_last_size.store(size, std::memory_order_relaxed);
  uint32_t observed = g_guid_mix_in_max_size.load(std::memory_order_relaxed);
  while (observed < size && !g_guid_mix_in_max_size.compare_exchange_weak(
      observed, size, std::memory_order_relaxed)) {}
  const int32_t result = effect_ref == &g_effect && bytes && size > 0 &&
      size <= kMaxGuidMixInBytes ? 0 : 4;
  (result == 0 ? g_guid_mix_in_successes : g_guid_mix_in_rejections)
      .fetch_add(1, std::memory_order_relaxed);
  g_guid_mix_in_last_result.store(result, std::memory_order_relaxed);
  return result;
}

bool verify_pre_checkout_result_case(int32_t expected_par_numerator,
                                     int32_t expected_par_denominator,
                                     int32_t expected_reference_width,
                                     int32_t expected_reference_height) {
  std::array<std::byte, kCheckoutResultBytes> result{};
  result.fill(std::byte{0xCD});
  const int32_t status = pre_checkout_layer(
      nullptr, 0, 0, nullptr, g_checkout_current_time, 1,
      g_checkout_current_time_scale, result.data());
  if (status != 0) return false;
  int32_t rect[4];
  std::memcpy(rect, result.data(), sizeof(rect));
  if (rect[0] != 0 || rect[1] != 0 || rect[2] != 640 || rect[3] != 360) return false;
  std::memcpy(rect, result.data() + 16, sizeof(rect));
  if (rect[0] != 0 || rect[1] != 0 || rect[2] != 640 || rect[3] != 360) return false;
  int32_t par[2];
  std::memcpy(par, result.data() + 32, sizeof(par));
  if (par[0] != expected_par_numerator || par[1] != expected_par_denominator) return false;
  int32_t reference_size[2];
  std::memcpy(reference_size, result.data() + 44, sizeof(reference_size));
  if (reference_size[0] != expected_reference_width ||
      reference_size[1] != expected_reference_height) return false;
  for (std::size_t offset = 40; offset < 44; ++offset) {
    if (result[offset] != std::byte{0}) return false;
  }
  for (std::size_t offset = 52; offset < kCheckoutResultBytes; ++offset) {
    if (result[offset] != std::byte{0}) return false;
  }
  return true;
}

bool verify_pre_checkout_result_contract() {
  SmartRuntimeSession smart_session;
  const int32_t saved_width = smart_state().width;
  const int32_t saved_height = smart_state().height;
  const SpatialRatio saved_par = g_pixel_aspect_ratio;
  const int32_t saved_full_width = g_full_resolution_width;
  const int32_t saved_full_height = g_full_resolution_height;
  smart_state().width = 640;
  smart_state().height = 360;
  smart_state().current_time = g_checkout_current_time;
  smart_state().current_time_scale = g_checkout_current_time_scale;
  smart_state().pixel_aspect_numerator = 1;
  smart_state().pixel_aspect_denominator = 1;
  smart_state().full_resolution_width = 0;
  smart_state().full_resolution_height = 0;
  bool passed = verify_pre_checkout_result_case(1, 1, 640, 360);
  smart_state().pixel_aspect_numerator = 10;
  smart_state().pixel_aspect_denominator = 11;
  smart_state().full_resolution_width = 1280;
  smart_state().full_resolution_height = 720;
  passed = verify_pre_checkout_result_case(10, 11, 1280, 720) && passed;
  smart_state().width = saved_width;
  smart_state().height = saved_height;
  g_pixel_aspect_ratio = saved_par;
  g_full_resolution_width = saved_full_width;
  g_full_resolution_height = saved_full_height;
  return passed;
}

bool verify_handle_resize_while_locked_rejected() {
  const uint32_t invalid_before = statistics().invalid_operations;
  void** handle = new_handle(16);
  if (!handle || !lock_handle(handle)) return false;
  const int32_t resize_error = resize_handle(32, &handle);
  unlock_handle(handle);
  dispose_handle(handle);
  return resize_error != 0 && statistics().invalid_operations == invalid_before + 1 &&
      handle_lifetimes_balanced();
}

int32_t __cdecl register_with_aegp(void*, const char*, int32_t* plugin_id) {
  if (!plugin_id) return 4;
  *plugin_id = 1;
  return 0;
}

uint32_t g_main_hwnd_queries{};

// AE always fills the caller-owned HWND storage. The headless worker has no
// application window, so the desktop window is published as the deterministic
// dialog parent instead of leaving the caller's buffer uninitialized.
int32_t __cdecl get_main_hwnd(void* main_hwnd) {
  if (!main_hwnd) return 4;
  ++g_main_hwnd_queries;
  const HWND desktop = GetDesktopWindow();
  std::memcpy(main_hwnd, &desktop, sizeof(desktop));
  return 0;
}

int32_t __cdecl get_effect_layer(void* effect, void** layer) {
  if (effect != &g_effect || !layer) return 4;
  *layer = &g_layer;
  return 0;
}
int32_t __cdecl convert_effect_to_comp_time(
    void* effect, int32_t what_time, uint32_t time_scale, AegpTime* comp_time);
int32_t __cdecl get_effect_camera(
    void* effect, const AegpTime* comp_time, void** camera_layer);
int32_t __cdecl get_effect_camera_matrix(void* effect, const AegpTime* comp_time,
    AegpMatrix4* camera_matrix, double* distance_to_image_plane,
    int16_t* image_plane_width, int16_t* image_plane_height);

bool verify_keyframe_ownership_rejection() {
  const uint32_t invalid_before = g_invalid_keyframe_operations;
  const uint32_t mutations_before = g_keyframe_mutations;
  void* mask = nullptr; void* stream = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0) return false;
  const HostTime later{20, 1}, earlier{10, 1}, batched{30, 1}, cancelled{40, 1};
  int32_t later_index = -1, earlier_index = -1, duplicate_index = -1, count = -1;
  HostTime observed{};
  bool passed = insert_keyframe(stream, 0, &later, &later_index) == 0 && later_index == 0 &&
      insert_keyframe(stream, 0, &earlier, &earlier_index) == 0 && earlier_index == 0 &&
      insert_keyframe(stream, 0, &earlier, &duplicate_index) == 0 && duplicate_index == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 2 &&
      get_keyframe_time(stream, 0, 0, &observed) == 0 && time_equal(observed, earlier) &&
      set_keyframe_flag(stream, 0, 1, 1) == 0 &&
      set_keyframe_interpolation(stream, 0, 2, 3) == 0 &&
      set_keyframe_label(stream, 0, 7) == 0;
  int32_t flags{}, in_interp{}, out_interp{}, label{};
  int16_t value_dimensions{-1}, temporal_dimensions{-1};
  passed = passed && get_keyframe_flags(stream, 0, &flags) == 0 && flags == 1 &&
      get_keyframe_interpolation(stream, 0, &in_interp, &out_interp) == 0 &&
      in_interp == 2 && out_interp == 3 &&
      get_keyframe_label(stream, 0, &label) == 0 && label == 7 &&
      get_stream_value_dimensionality(stream, &value_dimensions) == 0 &&
      value_dimensions == 0 &&
      get_stream_temporal_dimensionality(stream, &temporal_dimensions) == 0 &&
      temporal_dimensions == kHostTemporalDimensions;
  StreamValue spatial_in{}, spatial_out{}, spatial_check_in{}, spatial_check_out{};
  MaskVertex spatial_vertex{};
  passed = passed && get_new_keyframe_spatial_tangents(
      1, stream, 0, &spatial_in, &spatial_out) == 0 &&
      get_mask_outline_vertex_info(spatial_in.value, 0, &spatial_vertex) == 0;
  spatial_vertex.tangent_in_x = -12.5;
  spatial_vertex.tangent_out_y = 19.25;
  passed = passed && set_mask_outline_vertex_info(
      spatial_in.value, 0, &spatial_vertex) == 0 &&
      set_keyframe_spatial_tangents(stream, 0, &spatial_in, &spatial_out) == 0;
  const StreamValue stale_spatial = spatial_in;
  passed = passed && dispose_stream_value(&spatial_in) == 0 &&
      set_keyframe_spatial_tangents(stream, 0, &stale_spatial, &spatial_out) != 0 &&
      dispose_stream_value(&spatial_out) == 0 &&
      get_new_keyframe_spatial_tangents(
          1, stream, 0, &spatial_check_in, &spatial_check_out) == 0 &&
      get_mask_outline_vertex_info(spatial_check_in.value, 0, &spatial_vertex) == 0 &&
      spatial_vertex.tangent_in_x == -12.5 && spatial_vertex.tangent_out_y == 19.25 &&
      dispose_stream_value(&spatial_check_out) == 0 &&
      dispose_stream_value(&spatial_check_in) == 0;
  const KeyframeEase ease_in{23.5, 67.0}, ease_out{31.25, 72.5};
  KeyframeEase observed_in{-1.0, -1.0}, observed_out{-1.0, -1.0};
  passed = passed && set_keyframe_temporal_ease(
      stream, 0, 0, &ease_in, &ease_out) == 0 &&
      get_keyframe_temporal_ease(stream, 0, 0, &observed_in, &observed_out) == 0 &&
      observed_in.speed == ease_in.speed && observed_in.influence == ease_in.influence &&
      observed_out.speed == ease_out.speed && observed_out.influence == ease_out.influence;
  StreamValue unchanged_in{};
  unchanged_in.stream = reinterpret_cast<void*>(0x1111);
  unchanged_in.value = reinterpret_cast<void*>(0x2222);
  const StreamValue unchanged_before = unchanged_in;
  KeyframeEase unchanged_ease{41.0, 42.0};
  passed = passed && get_new_keyframe_spatial_tangents(
      2, stream, 0, &unchanged_in, nullptr) != 0 &&
      unchanged_in.stream == unchanged_before.stream && unchanged_in.value == unchanged_before.value &&
      set_keyframe_spatial_tangents(stream, 0, &unchanged_in, nullptr) != 0 &&
      get_keyframe_temporal_ease(stream, 0, 1, &unchanged_ease, nullptr) != 0 &&
      unchanged_ease.speed == 41.0 && unchanged_ease.influence == 42.0 &&
      set_keyframe_temporal_ease(stream, -1, 0, &ease_in, &ease_out) != 0 &&
      get_new_keyframe_spatial_tangents(1, stream, 0, nullptr, nullptr) != 0 &&
      set_keyframe_temporal_ease(stream, 0, 0, nullptr, nullptr) != 0;
  StreamValue later_value{}, hold_value{}, midpoint_value{};
  MaskVertex later_vertex{}, hold_vertex{}, midpoint_vertex{};
  const HostTime midpoint{15, 1};
  passed = passed && get_new_keyframe_value(1, stream, 1, &later_value) == 0 &&
      get_mask_outline_vertex_info(later_value.value, 0, &later_vertex) == 0;
  later_vertex.x += 10;
  passed = passed && set_mask_outline_vertex_info(later_value.value, 0, &later_vertex) == 0 &&
      set_keyframe_value(stream, 1, &later_value) == 0 &&
      dispose_stream_value(&later_value) == 0 &&
      get_new_stream_value(1, stream, 0, &midpoint, 0, &hold_value) == 0 &&
      get_mask_outline_vertex_info(hold_value.value, 0, &hold_vertex) == 0 &&
      hold_vertex.x == later_vertex.x - 10 && dispose_stream_value(&hold_value) == 0 &&
      set_keyframe_interpolation(stream, 0, 2, 1) == 0 &&
      get_new_stream_value(1, stream, 0, &midpoint, 0, &midpoint_value) == 0 &&
      get_mask_outline_vertex_info(midpoint_value.value, 0, &midpoint_vertex) == 0 &&
      midpoint_vertex.x == later_vertex.x - 5 && dispose_stream_value(&midpoint_value) == 0;
  StreamValue source{}, checked{};
  passed = passed && get_new_stream_value(1, stream, 0, nullptr, 0, &source) == 0 &&
      set_keyframe_value(stream, 0, &source) == 0 &&
      get_new_keyframe_value(1, stream, 0, &checked) == 0 &&
      checked.value != source.value && delete_keyframe(stream, 0) != 0 &&
      dispose_stream_value(&checked) == 0 && dispose_stream_value(&source) == 0;
  StreamValue batch_value{};
  void* transaction = nullptr; int32_t staged_index = -1;
  passed = passed && start_add_keyframes(stream, &transaction) == 0 &&
      add_keyframes(transaction, 0, &cancelled, &staged_index) == 0 && staged_index == 0 &&
      end_add_keyframes(0, transaction) == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 2 &&
      get_new_stream_value(1, stream, 0, nullptr, 0, &batch_value) == 0 &&
      start_add_keyframes(stream, &transaction) == 0 &&
      add_keyframes(transaction, 0, &batched, &staged_index) == 0 &&
      set_add_keyframe(transaction, staged_index, &batch_value) == 0 &&
      end_add_keyframes(1, transaction) == 0 &&
      dispose_stream_value(&batch_value) == 0 &&
      get_stream_num_keyframes(stream, &count) == 0 && count == 3 &&
      delete_keyframe(stream, 2) == 0 && delete_keyframe(stream, 1) == 0 &&
      delete_keyframe(stream, 0) == 0 && dispose_stream(stream) == 0 && dispose_mask(mask) == 0;
  return passed && g_invalid_keyframe_operations == invalid_before + 8 &&
      g_keyframe_mutations == mutations_before + 14 && mask_lifetimes_balanced();
}


bool verify_dynamic_stream_tree_rejection() {
  const auto original_scene = g_mask_scene;
  const uint32_t mutations_before = g_dynamic_stream_mutations;
  const uint32_t invalid_before = g_invalid_dynamic_stream_operations;
  void* mask = nullptr; void* mask_root = nullptr; void* layer_root = nullptr;
  void* parade = nullptr; void* atom = nullptr; void* outline = nullptr;
  void* opacity = nullptr; void* parent = nullptr; void* added = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_dynamic_stream_for_mask(1, mask, &mask_root) != 0 ||
      get_new_dynamic_stream_for_layer(1, &g_layer, &layer_root) != 0 ||
      get_new_dynamic_stream_by_match_name(1, layer_root, "ADBE Mask Parade", &parade) != 0 ||
      get_new_dynamic_stream_by_index(1, parade, 0, &atom) != 0 ||
      get_new_dynamic_stream_by_match_name(1, atom, "ADBE Mask Shape", &outline) != 0 ||
      get_new_dynamic_stream_by_match_name(1, atom, "ADBE Mask Opacity", &opacity) != 0)
    return false;
  int32_t depth{}, grouping{}, count{}, index{}; char match_name[40]{};
  uint32_t flags{}; uint8_t boolean{};
  bool passed = get_dynamic_stream_depth(layer_root, &depth) == 0 && depth == 0 &&
      get_dynamic_stream_grouping_type(parade, &grouping) == 0 && grouping == 2 &&
      get_num_streams_in_group(atom, &count) == 0 && count == 4 &&
      get_dynamic_match_name(outline, match_name) == 0 &&
      std::strcmp(match_name, "ADBE Mask Shape") == 0 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 0 &&
      get_new_parent_dynamic_stream(1, outline, &parent) == 0 &&
      get_dynamic_stream_grouping_type(parent, &grouping) == 0 && grouping == 1 &&
      set_dynamic_stream_flag(outline, 2, 0, 1) == 0 &&
      get_dynamic_stream_flags(outline, &flags) == 0 && flags == 2 &&
      set_dynamic_stream_flag(outline, 1, 0, 1) != 0 &&
      is_separation_leader(opacity, &boolean) == 0 && boolean == 0;
  StreamValue opacity_value{};
  passed = passed && get_new_stream_value(1, opacity, 0, nullptr, 0, &opacity_value) == 0 &&
      opacity_value.one_d == 100.0;
  opacity_value.one_d = 75.0;
  passed = passed && set_stream_value(1, opacity, &opacity_value) == 0 &&
      dispose_stream_value(&opacity_value) == 0 &&
      can_add_dynamic_stream(parade, "ADBE Mask Atom", &boolean) == 0 && boolean == 1 &&
      add_dynamic_stream(1, parade, "ADBE Mask Atom", &added) == 0;
  int32_t duplicate_index = -1;
  passed = passed && duplicate_dynamic_stream(1, atom, &duplicate_index) == 0 &&
      duplicate_index == 2 && reorder_dynamic_stream(atom, 2) == 0 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 2;
  const uint16_t renamed[]{'R','e','n','a','m','e','d',0};
  passed = passed && set_dynamic_stream_name(atom, renamed) == 0 &&
      get_dynamic_stream_modified(atom, &boolean) == 0 && boolean == 1;
  void* duplicate = nullptr;
  passed = passed && get_new_dynamic_stream_by_index(1, parade, 1, &duplicate) == 0 &&
      delete_dynamic_stream(duplicate) == 0 && dispose_stream(duplicate) == 0 &&
      delete_dynamic_stream(added) == 0 && dispose_stream(added) == 0 &&
      get_num_streams_in_group(parade, &count) == 0 && count == 1 &&
      get_dynamic_stream_index(atom, &index) == 0 && index == 0;
  passed = passed && dispose_stream(parent) == 0 && dispose_stream(opacity) == 0 &&
      dispose_stream(outline) == 0 && dispose_stream(atom) == 0 &&
      dispose_stream(parade) == 0 && dispose_stream(layer_root) == 0 &&
      dispose_stream(mask_root) == 0 && dispose_mask(mask) == 0;
  const bool balanced = mask_lifetimes_balanced();
  g_mask_scene = original_scene; g_mask_scene.reserve(kMaxHostMasks);
  return passed && balanced && g_dynamic_stream_mutations == mutations_before + 8 &&
      g_invalid_dynamic_stream_operations == invalid_before + 1;
}

bool verify_aegp_memory_and_strings_rejection() {
  const auto original_scene = g_mask_scene;
  const auto memory_before = aegp_memory_statistics();
  void* memory = nullptr; void* data = nullptr; uint32_t size{}; int32_t count{}, total{};
  bool passed = new_aegp_mem_handle(1, "fault probe", 32, 1, &memory) == 0 &&
      lock_aegp_mem_handle(memory, &data) == 0 && data &&
      std::all_of(static_cast<std::byte*>(data), static_cast<std::byte*>(data) + 32,
          [](std::byte value) { return value == std::byte{}; }) &&
      lock_aegp_mem_handle(memory, &data) == 0 &&
      resize_aegp_mem_handle("locked", 64, memory) != 0 &&
      unlock_aegp_mem_handle(memory) == 0 && unlock_aegp_mem_handle(memory) == 0 &&
      resize_aegp_mem_handle("resized", 64, memory) == 0 &&
      get_aegp_mem_handle_size(memory, &size) == 0 && size == 64 &&
      get_aegp_mem_stats(1, &count, &total) == 0 && count == 1 && total == 64 &&
      free_aegp_mem_handle(memory) == 0;
  void* mask = nullptr; void* stream = nullptr; void* name = nullptr; void* expression = nullptr;
  passed = passed && get_layer_mask_by_index(&g_layer, 0, &mask) == 0 &&
      get_new_mask_stream(1, mask, 400, &stream) == 0 &&
      unsupported_stream_name(1, stream, 1, &name) == 0 &&
      lock_aegp_mem_handle(name, &data) == 0 && data &&
      std::u16string(static_cast<const char16_t*>(data)) == u"Mask Path" &&
      unlock_aegp_mem_handle(name) == 0 && free_aegp_mem_handle(name) == 0;
  const uint16_t source[]{'t','i','m','e','*','2',0}; uint8_t enabled{};
  passed = passed && unsupported_set_expression(1, stream, source) == 0 &&
      get_expression_state(1, stream, &enabled) == 0 && enabled == 1 &&
      unsupported_get_expression(1, stream, &expression) == 0 &&
      lock_aegp_mem_handle(expression, &data) == 0 && data &&
      std::u16string(static_cast<const char16_t*>(data)) == u"time*2" &&
      unlock_aegp_mem_handle(expression) == 0 && free_aegp_mem_handle(expression) == 0 &&
      reject_expression_state(1, stream, 0) == 0 &&
      get_expression_state(1, stream, &enabled) == 0 && enabled == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0;
  const bool balanced = mask_lifetimes_balanced() && aegp_memory_balanced();
  g_mask_scene = original_scene; g_mask_scene.reserve(kMaxHostMasks);
  const auto memory_after = aegp_memory_statistics();
  return passed && balanced &&
      memory_after.invalid_operations == memory_before.invalid_operations + 1 &&
      memory_after.created == memory_before.created + 3 &&
      memory_after.freed == memory_before.freed + 3;
}


bool verify_mask_double_dispose_rejected() {
  void* mask = nullptr;
  return get_layer_mask_by_index(&g_layer, 0, &mask) == 0 &&
      dispose_mask(mask) == 0 && dispose_mask(mask) == 4 &&
      mask_lifetimes_balanced();
}

bool verify_stream_dispose_with_live_value_rejected() {
  void* mask = nullptr;
  void* stream = nullptr;
  StreamValue value{};
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0 ||
      get_new_stream_value(1, stream, 0, nullptr, 0, &value) != 0)
    return false;
  const int32_t premature_error = dispose_stream(stream);
  return premature_error == 4 && dispose_stream_value(&value) == 0 &&
      dispose_stream(stream) == 0 && dispose_mask(mask) == 0 &&
      mask_lifetimes_balanced();
}

bool verify_stream_metadata_and_ownership_rejection() {
  const uint32_t invalid_before = g_invalid_stream_operations;
  const uint32_t metadata_before = g_stream_metadata_queries;
  const uint32_t duplicates_before = g_stream_duplicates;
  void* mask = nullptr;
  void* stream = nullptr;
  void* duplicate = nullptr;
  void* rejected = reinterpret_cast<void*>(1);
  if (get_layer_mask_by_index(&g_layer, 0, &mask) != 0 ||
      get_new_mask_stream(1, mask, 400, &stream) != 0 ||
      get_new_mask_stream(1, mask, 999, &rejected) == 0 || rejected != nullptr ||
      duplicate_stream_ref(1, stream, &duplicate) != 0)
    return false;
  uint8_t boolean{};
  int32_t interpolations{}, flags{}, type{}, id{}, duplicate_id{};
  double minimum = -1, maximum = -1;
  char units[32]{'x'};
  StreamValue first{}, second{};
  bool passed = can_vary_over_time(stream, &boolean) == 0 && boolean == 1 &&
      get_valid_interpolations(stream, &interpolations) == 0 && interpolations == 0xffff &&
      get_stream_units_text(stream, 0, units) == 0 && units[0] == '\0' &&
      get_stream_properties(stream, &flags, &minimum, &maximum) == 0 && flags == 0 &&
      minimum == 0 && maximum == 0 && is_stream_timevarying(stream, &boolean) == 0 &&
      boolean == 0 && get_stream_type(stream, &type) == 0 && type == 11 &&
      get_unique_stream_id(stream, &id) == 0 &&
      get_unique_stream_id(duplicate, &duplicate_id) == 0 && duplicate_id == id &&
      get_expression_state(1, stream, &boolean) == 0 && boolean == 0 &&
      get_new_stream_value(1, stream, 0, nullptr, 0, &first) == 0 &&
      get_new_stream_value(1, duplicate, 0, nullptr, 0, &second) == 0 &&
      first.value != second.value;
  HostMask* host_mask = find_mask(mask);
  OutlineData* first_outline = find_outline(first.value);
  OutlineData* second_outline = find_outline(second.value);
  const double original_x = host_mask && !host_mask->vertices.empty() ? host_mask->vertices[0].x : 0;
  MaskVertex edited_vertex{};
  if (first_outline && !first_outline->vertices.empty()) {
    edited_vertex = first_outline->vertices[0];
    edited_vertex.x += 3;
  }
  passed = passed && host_mask && first_outline && second_outline &&
      set_mask_outline_vertex_info(first.value, 0, &edited_vertex) == 0 &&
      host_mask->vertices[0].x == original_x &&
      set_stream_value(1, stream, &first) == 0 &&
      host_mask->vertices[0].x == original_x + 3 &&
      second_outline->vertices[0].x == original_x;
  const OutlineData committed = static_cast<const OutlineData&>(*host_mask);
  if (second_outline && !second_outline->vertices.empty())
    second_outline->vertices[0].x = std::numeric_limits<double>::quiet_NaN();
  StreamValue forged = second;
  passed = passed && set_stream_value(1, duplicate, &second) != 0 &&
      static_cast<const OutlineData&>(*host_mask).vertices[0].x == committed.vertices[0].x &&
      set_stream_value(1, stream, &second) != 0 &&
      set_stream_value(1, stream, &forged) != 0 &&
      set_stream_value(1, nullptr, &first) != 0 &&
      set_stream_value(2, stream, &first) != 0 &&
      dispose_stream_value(&first) == 0 &&
      set_stream_value(1, stream, &first) != 0 &&
      dispose_stream_value(&first) != 0;
  host_mask->keyframes.push_back(snapshot_keyframe(*host_mask, HostTime{0, 1}));
  passed = passed && set_stream_value(1, duplicate, &second) != 0;
  host_mask->keyframes.clear();
  passed = passed && dispose_stream_value(&second) == 0 &&
      dispose_stream_value(&second) != 0 &&
      dispose_stream(duplicate) == 0 && dispose_stream(stream) == 0 &&
      dispose_mask(mask) == 0;
  return passed && g_invalid_stream_operations >= invalid_before + 9 &&
      g_stream_metadata_queries == metadata_before + 9 &&
      g_stream_duplicates == duplicates_before + 1 && mask_lifetimes_balanced();
}

bool verify_outline_mutation_rejection() {
  if (g_mask_scene.empty()) return false;
  HostMask& mask = g_mask_scene.front();
  const HostMask original = mask;
  const uint32_t invalid_before = g_invalid_outline_operations;
  const uint32_t mutations_before = g_outline_mutations;
  void* outline = &mask.outline;
  int32_t segments = -1;
  MaskVertex replacement{11, 2, -1, 0, 1, 0};
  MaskVertex observed{};
  MaskFeather feather{1, 0.25, 3.0, 0.5f, 0.75f, 0, 0};
  int32_t feather_index = -1;
  int32_t feather_count = -1;
  bool passed = set_mask_outline_open(outline, 1) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 3 &&
      set_mask_outline_open(outline, 0) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 4 &&
      set_mask_outline_vertex_info(outline, 1, &replacement) == 0 &&
      get_mask_outline_vertex_info(outline, 1, &observed) == 0 && observed.x == 11 &&
      create_mask_outline_vertex(outline, 2) == 0 &&
      get_mask_outline_num_segments(outline, &segments) == 0 && segments == 5 &&
      delete_mask_outline_vertex(outline, 2) == 0 &&
      create_mask_outline_feather(outline, &feather, &feather_index) == 0 &&
      feather_index == 0 && get_mask_outline_num_feathers(outline, &feather_count) == 0 &&
      feather_count == 1;
  feather.radius = 2.0;
  passed = passed && set_mask_outline_feather_info(outline, 0, &feather) == 0 &&
      get_mask_outline_feather_info(outline, 0, &feather) == 0 && feather.radius == 2.0;
  MaskFeather invalid = feather;
  invalid.radius = -1.0;
  passed = passed && set_mask_outline_feather_info(outline, 0, &invalid) != 0 &&
      delete_mask_outline_feather(outline, 0) == 0;
  mask = original;
  return passed && g_invalid_outline_operations == invalid_before + 1 &&
      g_outline_mutations == mutations_before + 8;
}

bool verify_mask_attribute_and_ownership_rejection() {
  const auto original_scene = g_mask_scene;
  const uint32_t invalid_before = g_invalid_mask_operations;
  const uint32_t mutations_before = g_mask_mutations;
  void* original = nullptr;
  if (get_layer_mask_by_index(&g_layer, 0, &original) != 0) return false;
  const double color[4]{1.0, 0.2, 0.4, 0.6};
  double observed_color[4]{};
  uint8_t byte_value{};
  int32_t long_value{};
  bool passed = set_mask_invert(original, 1) == 0 &&
      get_mask_invert(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_mode(original, 3) == 0 && get_mask_mode(original, &long_value) == 0 &&
      long_value == 3 && set_mask_motion_blur(original, 2) == 0 &&
      get_mask_motion_blur(original, &byte_value) == 0 && byte_value == 2 &&
      set_mask_feather_falloff(original, 1) == 0 &&
      get_mask_feather_falloff(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_color(original, color) == 0 && get_mask_color(original, observed_color) == 0 &&
      std::equal(std::begin(color), std::end(color), std::begin(observed_color)) &&
      set_mask_lock(original, 1) == 0 && get_mask_lock(original, &byte_value) == 0 &&
      byte_value == 1 && set_mask_roto_bezier(original, 1) == 0 &&
      get_mask_roto_bezier(original, &byte_value) == 0 && byte_value == 1 &&
      set_mask_mode(original, 99) != 0;
  int32_t original_id{}, duplicate_id{}, count{};
  void* duplicate = nullptr;
  passed = passed && get_mask_id(original, &original_id) == 0 &&
      duplicate_mask(original, &duplicate) == 0 &&
      get_mask_id(duplicate, &duplicate_id) == 0 && duplicate_id != original_id &&
      get_layer_num_masks(&g_layer, &count) == 0 && count == 2 &&
      delete_mask_from_layer(duplicate) == 0 && dispose_mask(duplicate) == 0 &&
      get_layer_num_masks(&g_layer, &count) == 0 && count == 1;
  void* created = nullptr;
  int32_t created_index = -1;
  passed = passed && create_new_mask(&g_layer, &created, &created_index) == 0 &&
      created_index == 1 && delete_mask_from_layer(created) == 0 &&
      dispose_mask(created) == 0 && dispose_mask(original) == 0;
  const bool balanced = mask_lifetimes_balanced();
  g_mask_scene = original_scene;
  g_mask_scene.reserve(kMaxHostMasks);
  return passed && g_invalid_mask_operations == invalid_before + 1 &&
      g_mask_mutations == mutations_before + 11 && balanced;
}

struct UtilitySuite {
  // Function pointer positions mirror the reviewed Adobe suite versions.
  // AEGP_UtilitySuite6 (acquisition version 13) publishes 33 slots; only
  // RegisterWithAEGP (slot 9) and GetMainHWND (slot 10) are supported, the
  // rest stay null so out-of-range reads fail closed instead of leaving the
  // table shorter than the ABI the effect compiled against.
  void* unsupported[9]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[22]{};
};
static_assert(sizeof(UtilitySuite) == 33 * sizeof(void*));
static_assert(offsetof(UtilitySuite, register_with_aegp) == 9 * sizeof(void*));
static_assert(offsetof(UtilitySuite, get_main_hwnd) == 10 * sizeof(void*));
struct UtilitySuite3 {
  // AEGP_UtilitySuite3 (acquisition version 7) publishes 25 slots with
  // RegisterWithAEGP at slot 7 and GetMainHWND at slot 8.
  void* unsupported[7]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[16]{};
};
static_assert(sizeof(UtilitySuite3) == 25 * sizeof(void*));
static_assert(offsetof(UtilitySuite3, register_with_aegp) == 7 * sizeof(void*));
static_assert(offsetof(UtilitySuite3, get_main_hwnd) == 8 * sizeof(void*));
struct PfInterfaceSuite {
  decltype(&get_effect_layer) get_effect_layer;
  decltype(&get_new_effect_for_effect) get_new_effect_for_effect;
  decltype(&convert_effect_to_comp_time) convert_effect_to_comp_time;
  decltype(&get_effect_camera) get_effect_camera;
  decltype(&get_effect_camera_matrix) get_effect_camera_matrix;
};
static_assert(sizeof(PfInterfaceSuite) == 5 * sizeof(void*));
static_assert(offsetof(PfInterfaceSuite, convert_effect_to_comp_time) == 2 * sizeof(void*));
static_assert(offsetof(PfInterfaceSuite, get_effect_camera) == 3 * sizeof(void*));
static_assert(offsetof(PfInterfaceSuite, get_effect_camera) == 24);
static_assert(offsetof(PfInterfaceSuite, get_effect_camera_matrix) == 32);
int32_t __cdecl unsupported_path_mask() { return 4; }
struct LegacyRect { int32_t left, top, right, bottom; };
struct PfPathVertex {
  double x, y, tan_in_x, tan_in_y, tan_out_x, tan_out_y;
};
static_assert(sizeof(PfPathVertex) == 48);

bool valid_checked_pf_path(HostMask* path) {
  return path && !path->deleted &&
      g_pf_path_checkouts.find(path) != g_pf_path_checkouts.end();
}

bool snapshot_checked_pf_path(void* path,
    aexcompat::mask_runtime::CurveSnapshot& snapshot) {
  auto* checked = static_cast<HostMask*>(path);
  return valid_checked_pf_path(checked) &&
      aexcompat::mask_runtime::snapshot_curve(&checked->mask, snapshot);
}

std::size_t distinct_vertex_count(
    const aexcompat::mask_runtime::CurveSnapshot& path) {
  return path.vertices.size() -
      static_cast<std::size_t>(!path.open && !path.vertices.empty());
}

int32_t __cdecl pf_num_paths(void* effect_ref, int32_t* count) {
  if (!effect_ref || !count) return 4;
  *count = static_cast<int32_t>(ordered_active_masks().size());
  return 0;
}

int32_t __cdecl pf_path_info(void* effect_ref, int32_t index, int32_t* unique_id) {
  const auto masks = ordered_active_masks();
  if (!effect_ref || !unique_id || index < 0 ||
      static_cast<std::size_t>(index) >= masks.size()) return 4;
  *unique_id = masks[static_cast<std::size_t>(index)]->id;
  return 0;
}

int32_t __cdecl pf_checkout_path(void* effect_ref, int32_t unique_id, int32_t,
                                 int32_t time_step, uint32_t time_scale, void** path) {
  HostMask* mask = nullptr;
  for (auto& candidate : g_mask_scene)
    if (!candidate.deleted && candidate.id == unique_id) { mask = &candidate; break; }
  if (!effect_ref || !path || time_step <= 0 || time_scale == 0 || !mask || mask->deleted) {
    ++g_invalid_pf_path_operations;
    return 4;
  }
  ++g_pf_path_checkouts[mask];
  ++g_pf_path_checkout_calls;
  *path = mask;
  return 0;
}

int32_t __cdecl pf_checkin_path(void* effect_ref, int32_t unique_id, int32_t changed,
                                void* path) {
  auto* mask = static_cast<HostMask*>(path);
  const auto found = g_pf_path_checkouts.find(mask);
  if (!effect_ref || changed != 0 || !mask || mask->id != unique_id ||
      found == g_pf_path_checkouts.end() || found->second == 0) {
    ++g_invalid_pf_path_operations;
    return 4;
  }
  if (--found->second == 0) g_pf_path_checkouts.erase(found);
  ++g_pf_path_checkin_calls;
  return 0;
}

int32_t __cdecl pf_path_is_open(void* effect_ref, void* path, int8_t* open) {
  aexcompat::mask_runtime::CurveSnapshot curve;
  if (!effect_ref || !open || !snapshot_checked_pf_path(path, curve)) return 4;
  *open = curve.open ? 1 : 0;
  return 0;
}

int32_t __cdecl pf_path_num_segments(void* effect_ref, void* path, int32_t* count) {
  aexcompat::mask_runtime::CurveSnapshot curve;
  if (!effect_ref || !count || !snapshot_checked_pf_path(path, curve)) return 4;
  const std::size_t vertices = distinct_vertex_count(curve);
  *count = static_cast<int32_t>(curve.open && vertices > 0 ? vertices - 1 : vertices);
  return 0;
}

int32_t __cdecl pf_path_vertex_info(void* effect_ref, void* path, int32_t index,
                                    PfPathVertex* vertex) {
  aexcompat::mask_runtime::CurveSnapshot curve;
  if (!effect_ref || !vertex || !snapshot_checked_pf_path(path, curve) || index < 0 ||
      static_cast<std::size_t>(index) >= curve.vertices.size()) return 4;
  const auto& source = curve.vertices[static_cast<std::size_t>(index)];
  *vertex = {source.x, source.y, source.tangent_in_x, source.tangent_in_y,
             source.tangent_out_x, source.tangent_out_y};
  return 0;
}

int32_t pf_path_segment_count(const HostMask& path) {
  const auto count = distinct_vertex_count(path);
  return static_cast<int32_t>(path.open && count > 0 ? count - 1 : count);
}

HostMask* pf_path_by_id(int32_t unique_id) {
  for (auto& candidate : g_mask_scene)
    if (!candidate.deleted && candidate.id == unique_id) return &candidate;
  return nullptr;
}

using PfPathPoint = std::array<double, 2>;
using PfPathCubic = std::array<PfPathPoint, 4>;

std::array<double, 2> eval_pf_cubic(const HostMask& path, int32_t segment, double t) {
  const auto count = distinct_vertex_count(path);
  const auto& a = path.vertices[static_cast<std::size_t>(segment)];
  const auto& b = path.vertices[(static_cast<std::size_t>(segment) + 1) % count];
  const double u = 1.0 - t;
  const double c1x = a.x + a.tangent_out_x, c1y = a.y + a.tangent_out_y;
  const double c2x = b.x + b.tangent_in_x, c2y = b.y + b.tangent_in_y;
  return {u * u * u * a.x + 3.0 * u * u * t * c1x +
              3.0 * u * t * t * c2x + t * t * t * b.x,
          u * u * u * a.y + 3.0 * u * u * t * c1y +
              3.0 * u * t * t * c2y + t * t * t * b.y};
}

bool populate_pf_segment_prep(PfPathSegPrep& prep, HostMask* path, int32_t segment,
                              int32_t frequency) {
  aexcompat::mask_runtime::CurveSnapshot curve;
  if (!snapshot_checked_pf_path(path, curve) || segment < 0 || frequency < 1 ||
      frequency > 1024) return false;
  const auto vertex_count = distinct_vertex_count(curve);
  const auto segment_count = static_cast<int32_t>(
      curve.open && vertex_count > 0 ? vertex_count - 1 : vertex_count);
  if (segment >= segment_count) return false;
  prep.path = path;
  prep.segment = segment;
  const auto& a = curve.vertices[static_cast<std::size_t>(segment)];
  const auto& b = curve.vertices[(static_cast<std::size_t>(segment) + 1) % vertex_count];
  prep.controls = {{{a.x, a.y}, {a.x + a.tangent_out_x, a.y + a.tangent_out_y},
                    {b.x + b.tangent_in_x, b.y + b.tangent_in_y}, {b.x, b.y}}};
  for (const auto& point : prep.controls)
    if (!std::isfinite(point[0]) || !std::isfinite(point[1])) return false;
  const double control_polygon = ::pf_path_point_distance(prep.controls[0], prep.controls[1]) +
      ::pf_path_point_distance(prep.controls[1], prep.controls[2]) +
      ::pf_path_point_distance(prep.controls[2], prep.controls[3]);
  const double tolerance = std::max(1e-9, std::max(1.0, control_polygon) *
      1e-7 / std::sqrt(static_cast<double>(frequency)));
  prep.parameters.push_back(0.0);
  prep.points.push_back(prep.controls[0]);
  ::append_adaptive_pf_cubic(prep.controls, 0.0, 1.0, tolerance, 0,
                             prep.parameters, prep.points);
  if (prep.points.size() < 2 || prep.parameters.size() != prep.points.size()) return false;
  prep.cumulative_lengths.reserve(prep.points.size());
  prep.cumulative_lengths.push_back(0.0);
  for (std::size_t sample = 1; sample < prep.points.size(); ++sample) {
    prep.cumulative_lengths.push_back(prep.cumulative_lengths.back() +
        ::pf_path_point_distance(prep.points[sample - 1], prep.points[sample]));
  }
  return prep.cumulative_lengths.size() == prep.points.size() &&
      std::isfinite(prep.cumulative_lengths.back());
}

int32_t __cdecl pf_path_prepare_seg_length(void* effect_ref, void* path, int32_t segment,
                                           int32_t frequency, void** prep) {
  auto* mask = static_cast<HostMask*>(path);
  if (!effect_ref || !prep || *prep) {
    ++g_invalid_pf_path_operations;
    return kPfBadCallbackParam;
  }
  try {
    PfPathSegPrep candidate;
    if (!populate_pf_segment_prep(candidate, mask, segment, frequency)) {
      ++g_invalid_pf_path_operations;
      return kPfBadCallbackParam;
    }
    std::lock_guard<std::mutex> lock(g_pf_path_segment_preps_mutex);
    if (g_pf_path_segment_preps.size() >= kMaxPfPathSegmentPreps)
      return kPfOutOfMemory;
    g_pf_path_segment_preps.push_back(std::move(candidate));
    *prep = &g_pf_path_segment_preps.back();
    ++g_pf_path_preps_created;
    return 0;
  } catch (const std::bad_alloc&) {
    return kPfOutOfMemory;
  } catch (...) {
    ++g_invalid_pf_path_operations;
    return kPfBadCallbackParam;
  }
}

PfPathSegPrep* checked_pf_segment_prep(void** prep, HostMask* path, int32_t segment) {
  if (!prep || !*prep) return nullptr;
  auto* candidate = static_cast<PfPathSegPrep*>(*prep);
  const auto found = std::find_if(g_pf_path_segment_preps.begin(), g_pf_path_segment_preps.end(),
      [candidate](auto& item) { return &item == candidate; });
  return found != g_pf_path_segment_preps.end() && found->path == path &&
      found->segment == segment && valid_checked_pf_path(path) ? &*found : nullptr;
}

PfPathSegPrep* registered_pf_segment_prep(void** prep, HostMask* path, int32_t segment) {
  if (!prep || !*prep || !path) return nullptr;
  auto* candidate = static_cast<PfPathSegPrep*>(*prep);
  const auto found = std::find_if(g_pf_path_segment_preps.begin(), g_pf_path_segment_preps.end(),
      [candidate](auto& item) { return &item == candidate; });
  return found != g_pf_path_segment_preps.end() && found->path == path &&
      found->segment == segment ? &*found : nullptr;
}

int32_t __cdecl pf_path_get_seg_length(void* effect_ref, void* path, int32_t segment,
                                       void** prep, double* length) {
  std::lock_guard<std::mutex> lock(g_pf_path_segment_preps_mutex);
  auto* checked = checked_pf_segment_prep(prep, static_cast<HostMask*>(path), segment);
  if (!effect_ref || !length || !checked || checked->cumulative_lengths.empty())
    return kPfBadCallbackParam;
  *length = checked->cumulative_lengths.back();
  return 0;
}

bool evaluate_pf_segment_length(PfPathSegPrep& prep, double length,
                                double* x, double* y, double* dx, double* dy) {
  if (!x || !y || !std::isfinite(length) || prep.cumulative_lengths.empty() ||
      length < 0.0 || length > prep.cumulative_lengths.back()) return false;
  const auto upper = std::lower_bound(prep.cumulative_lengths.begin(),
                                      prep.cumulative_lengths.end(), length);
  std::size_t right = static_cast<std::size_t>(upper - prep.cumulative_lengths.begin());
  if (right == 0) right = 1;
  if (right >= prep.points.size()) right = prep.points.size() - 1;
  const std::size_t left = right - 1;
  const double start = prep.cumulative_lengths[left];
  const double span = prep.cumulative_lengths[right] - start;
  const double fraction = span == 0.0 ? 0.0 : (length - start) / span;
  const double t = prep.parameters[left] +
      (prep.parameters[right] - prep.parameters[left]) * fraction;
  const auto point = ::eval_pf_cubic(prep.controls, t);
  *x = point[0];
  *y = point[1];
  if (dx && dy) {
    const auto derivative = ::deriv_pf_cubic(prep.controls, t);
    const double magnitude = std::hypot(derivative[0], derivative[1]);
    // Cusps and degenerate segments have a defined zero arc-length derivative.
    *dx = magnitude > 0.0 ? derivative[0] / magnitude : 0.0;
    *dy = magnitude > 0.0 ? derivative[1] / magnitude : 0.0;
  }
  return std::isfinite(*x) && std::isfinite(*y) &&
      (!dx || (std::isfinite(*dx) && std::isfinite(*dy)));
}

int32_t __cdecl pf_path_eval_seg_length(void* effect_ref, void* path, void** prep,
                                        int32_t segment, double length, double* x, double* y) {
  std::lock_guard<std::mutex> lock(g_pf_path_segment_preps_mutex);
  auto* checked = checked_pf_segment_prep(prep, static_cast<HostMask*>(path), segment);
  return effect_ref && checked && evaluate_pf_segment_length(*checked, length, x, y, nullptr, nullptr)
      ? 0 : kPfBadCallbackParam;
}

int32_t __cdecl pf_path_eval_seg_length_deriv1(void* effect_ref, void* path, void** prep,
                                               int32_t segment, double length,
                                               double* x, double* y, double* dx, double* dy) {
  std::lock_guard<std::mutex> lock(g_pf_path_segment_preps_mutex);
  auto* checked = checked_pf_segment_prep(prep, static_cast<HostMask*>(path), segment);
  return effect_ref && checked && evaluate_pf_segment_length(*checked, length, x, y, dx, dy)
      ? 0 : kPfBadCallbackParam;
}

int32_t __cdecl pf_path_cleanup_seg_length(void* effect_ref, void* path, int32_t segment,
                                           void** prep) {
  std::lock_guard<std::mutex> lock(g_pf_path_segment_preps_mutex);
  auto* checked = registered_pf_segment_prep(prep, static_cast<HostMask*>(path), segment);
  if (!effect_ref || !checked) {
    ++g_invalid_pf_path_operations;
    return kPfBadCallbackParam;
  }
  const auto found = std::find_if(g_pf_path_segment_preps.begin(), g_pf_path_segment_preps.end(),
      [checked](auto& item) { return &item == checked; });
  g_pf_path_segment_preps.erase(found);
  *prep = nullptr;
  ++g_pf_path_preps_disposed;
  return 0;
}

bool verify_pf_path_data_hardening() {
  g_mask_scene.clear();
  g_mask_scene.reserve(kMaxHostMasks);
  g_pf_path_checkouts.clear();
  g_pf_path_segment_preps.clear();
  g_pf_path_checkout_calls = 0;
  g_pf_path_checkin_calls = 0;
  g_pf_path_preps_created = 0;
  g_pf_path_preps_disposed = 0;

  const auto add_mask = [](int32_t id, bool open, std::vector<MaskVertex> vertices) {
    HostMask mask;
    mask.id = id;
    mask.open = open;
    mask.vertices = std::move(vertices);
    g_mask_scene.push_back(std::move(mask));
  };
  const MaskVertex vertex{1, 2, 0, 0, 0, 0};
  add_mask(1, true, {});
  add_mask(2, false, {});
  add_mask(3, true, {vertex});
  add_mask(4, false, {vertex, vertex});

  for (auto& mask : g_mask_scene) {
    void* path = nullptr;
    int32_t segments = -1;
    void* prep = nullptr;
    if (pf_checkout_path(&g_effect, mask.id, 0, 1, 1, &path) != 0 ||
        pf_path_num_segments(&g_effect, path, &segments) != 0 || segments < 0 ||
        (mask.id != 4 && segments != 0) || (mask.id == 4 && segments != 1) ||
        (segments == 0 && pf_path_prepare_seg_length(
            &g_effect, path, 0, 1, &prep) == 0) || prep != nullptr ||
        pf_checkin_path(&g_effect, mask.id, 0, path) != 0)
      return false;
  }

  g_mask_scene.clear();
  HostMask rectangle;
  rectangle.id = 5;
  rectangle.open = false;
  rectangle.vertices = {{0, 0, 0, 0, 0, 0}, {4, 0, 0, 0, 0, 0},
                        {4, 4, 0, 0, 0, 0}, {0, 0, 0, 0, 0, 0}};
  g_mask_scene.push_back(std::move(rectangle));
  HostMask foreign;
  foreign.id = 6;
  foreign.open = true;
  foreign.vertices = {{0, 0, 0, 0, 1, 0}, {1, 0, -1, 0, 0, 0}};
  g_mask_scene.push_back(std::move(foreign));
  HostMask* mask = &g_mask_scene.front();
  HostMask* foreign_mask = &g_mask_scene.back();
  void* path = nullptr;
  void* foreign_path = nullptr;
  void* prep = nullptr;
  if (pf_checkout_path(&g_effect, mask->id, 0, 1, 1, &path) != 0 ||
      pf_checkout_path(&g_effect, foreign_mask->id, 0, 1, 1, &foreign_path) != 0 ||
      pf_path_prepare_seg_length(&g_effect, path, 0, 4, &prep) != 0)
    return false;
  void* live_prep = prep;
  void* stale_prep = prep;
  double length = 0;
  if (pf_path_cleanup_seg_length(&g_effect, path, 1, &prep) == 0 || prep != live_prep ||
      pf_path_cleanup_seg_length(&g_effect, foreign_path, 0, &prep) == 0 || prep != live_prep ||
      pf_checkin_path(&g_effect, foreign_mask->id, 0, foreign_path) != 0 ||
      pf_checkin_path(&g_effect, mask->id, 0, path) != 0 ||
      pf_path_get_seg_length(&g_effect, path, 0, &prep, &length) == 0 ||
      pf_path_cleanup_seg_length(&g_effect, path, 0, &prep) != 0 || prep != nullptr ||
      pf_path_cleanup_seg_length(&g_effect, path, 0, &stale_prep) == 0 ||
      stale_prep != live_prep)
    return false;

  const auto verify_curve = [](int32_t id, bool open, std::vector<MaskVertex> vertices,
                               double expected_length, double length_tolerance,
                               double expected_mid_x, double expected_mid_y,
                               double position_tolerance, bool expect_zero_derivative) {
    g_mask_scene.clear();
    HostMask curve;
    curve.id = id;
    curve.open = open;
    curve.vertices = std::move(vertices);
    if (!open) curve.vertices.push_back(curve.vertices.front());
    g_mask_scene.push_back(std::move(curve));
    void* curve_path = nullptr;
    void* curve_prep = nullptr;
    if (pf_checkout_path(&g_effect, id, 0, 1, 1, &curve_path) != 0 ||
        pf_path_prepare_seg_length(&g_effect, curve_path, 0, 1, &curve_prep) != 0)
      return false;
    double curve_length = 0.0, x = 0.0, y = 0.0, dx = 0.0, dy = 0.0;
    const bool evaluated =
        pf_path_get_seg_length(&g_effect, curve_path, 0, &curve_prep, &curve_length) == 0 &&
        std::abs(curve_length - expected_length) <= length_tolerance &&
        pf_path_eval_seg_length_deriv1(&g_effect, curve_path, &curve_prep, 0,
            curve_length * 0.5, &x, &y, &dx, &dy) == 0 &&
        std::abs(x - expected_mid_x) <= position_tolerance &&
        std::abs(y - expected_mid_y) <= position_tolerance &&
        (expect_zero_derivative ? std::hypot(dx, dy) == 0.0
                                : std::abs(std::hypot(dx, dy) - 1.0) <= 1e-10) &&
        pf_path_eval_seg_length(&g_effect, curve_path, &curve_prep, 0,
            -1.0, &x, &y) != 0 &&
        pf_path_eval_seg_length(&g_effect, curve_path, &curve_prep, 0,
            curve_length + 1.0, &x, &y) != 0;
    const bool cleaned =
        pf_path_cleanup_seg_length(&g_effect, curve_path, 0, &curve_prep) == 0 &&
        curve_prep == nullptr && pf_checkin_path(&g_effect, id, 0, curve_path) == 0;
    return evaluated && cleaned;
  };

  constexpr double kappa = 0.5522847498307936;
  if (!verify_curve(10, true,
          {{0, 0, 0, 0, 0, 0}, {4, 0, 0, 0, 0, 0}},
          4.0, 1e-10, 2.0, 0.0, 1e-8, false) ||
      !verify_curve(11, true,
          {{1, 0, 0, 0, 0, kappa}, {0, 1, kappa, 0, 0, 0}},
          1.5707963267948966, 3e-4, 0.7071067811865476,
          0.7071067811865476, 3e-5, false) ||
      !verify_curve(12, true,
          {{0, 0, 0, 0, 3, 6}, {6, 0, -3, -6, 0, 0}},
          9.537946844777451, 2e-3, 3.0, 0.0, 2e-4, false) ||
      !verify_curve(13, true,
          {{2, 3, 0, 0, 0, 0}, {2, 3, 0, 0, 0, 0}},
          0.0, 0.0, 2.0, 3.0, 0.0, true) ||
      !verify_curve(14, false,
          {{0, 0, 0, 0, 0, 0}, {1, 0, 0, 0, 0, 0}},
          1.0, 1e-10, 0.5, 0.0, 1e-8, false))
    return false;
  return pf_path_lifetimes_balanced();
}

int32_t __cdecl pf_path_is_inverted(void* effect_ref, int32_t unique_id, int8_t* inverted) {
  HostMask* mask = pf_path_by_id(unique_id);
  if (!effect_ref || !mask || !inverted) return 4;
  *inverted = mask->invert ? 1 : 0;
  return 0;
}

int32_t __cdecl pf_path_get_mask_mode(void* effect_ref, int32_t unique_id, int32_t* mode) {
  HostMask* mask = pf_path_by_id(unique_id);
  if (!effect_ref || !mask || !mode) return 4;
  *mode = mask->mode;
  return 0;
}

int32_t __cdecl pf_path_get_name(void* effect_ref, int32_t unique_id, char* name) {
  HostMask* mask = pf_path_by_id(unique_id);
  if (!effect_ref || !mask || !name) return 4;
  const std::string value = "Mask " + std::to_string(mask->dynamic_order + 1);
  if (value.size() > 31) return 4;
  std::memcpy(name, value.c_str(), value.size() + 1);
  return 0;
}

int32_t __cdecl pf_mask_world_with_path(void* effect_ref, void** path, double feather_x,
                                        double feather_y, int32_t invert, double opacity,
                                        int32_t quality, void* world, LegacyRect* bounds);
#include "worker_l2_suite_abi.hpp"

UtilitySuite g_utility_suite{{}, &register_with_aegp, &get_main_hwnd, {}};
UtilitySuite3 g_utility_suite3{{}, &register_with_aegp, &get_main_hwnd, {}};
PfInterfaceSuite g_pf_interface_suite{&get_effect_layer, &get_new_effect_for_effect,
    &convert_effect_to_comp_time, &get_effect_camera,
    &get_effect_camera_matrix};
std::array<void*, 14> g_aegp_dynamic_stream_suite2{};
using AegpStreamValue = aexcompat::scene_runtime::AegpStreamValue;
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
int32_t __cdecl aegp_set_dynamic_stream_flag_v2(
    void* stream, uint32_t one_flag, uint8_t undoable, uint8_t set);
int32_t __cdecl aegp_get_effect_param_union_by_index_v3(
    int32_t plugin_id, void* effect, int32_t index, int32_t* type, void* param_union);
PfMaskSuite1 g_pf_mask_suite1{&pf_mask_world_with_path};
std::array<void*, 4> g_pf_path_query_suite1{};
std::array<void*, 11> g_pf_path_data_suite1{};
WorldTransformSuite1 g_world_transform_suite1{};
std::array<void*, 19> g_ansi_suite1{};
std::array<void*, 1> g_effect_ui_suite1{};
// PF_AdvAppSuite1 is frozen at ten callbacks; keep its storage independent
// from the eleven-slot v2 table so versioned suite identity cannot alias.
std::array<void*, 10> g_adv_app_suite1{};
std::array<void*, 11> g_adv_app_suite2{};
static_assert(sizeof(g_adv_app_suite1) == 10 * sizeof(void*));
static_assert(sizeof(g_adv_app_suite2) == 11 * sizeof(void*));
struct PfAdvItemSuite1;
extern PfAdvItemSuite1 g_adv_item_suite1;
std::array<void*, 2> g_drawbot_draw_suite1{};
std::array<void*, 13> g_drawbot_supplier_suite1{};
std::array<void*, 17> g_drawbot_surface_suite2{};
std::array<void*, 6> g_drawbot_path_suite1{};
std::array<void*, 1> g_effect_custom_ui_suite1{};
std::array<void*, 2> g_effect_custom_ui_suite2{};
int32_t __cdecl convert_effect_to_comp_time(
    void* effect, int32_t what_time, uint32_t time_scale, AegpTime* comp_time) {
  if (effect != &g_effect || time_scale == 0 || !comp_time) return 4;
  const int64_t checked_value = static_cast<int64_t>(what_time);
  const uint64_t checked_scale = static_cast<uint64_t>(time_scale);
  if (checked_value < (std::numeric_limits<int32_t>::min)() ||
      checked_value > (std::numeric_limits<int32_t>::max)() ||
      checked_scale > (std::numeric_limits<uint32_t>::max)())
    return 4;
  const AegpTime converted{static_cast<int32_t>(checked_value),
                           static_cast<uint32_t>(checked_scale)};
  *comp_time = converted;
  return 0;
}
struct AegpTimeStamp { std::array<uint8_t, 4> bytes{}; };
static_assert(sizeof(AegpTimeStamp) == 4);
static_assert(std::is_same_v<decltype(&new_layer_render_options),
                             aexcompat::suite_abi::AegpLayerOptionsNew>);
static_assert(std::is_same_v<decltype(&new_from_upstream_of_effect),
                             aexcompat::suite_abi::AegpLayerOptionsNew>);
static_assert(std::is_same_v<decltype(&new_from_downstream_of_effect),
                             aexcompat::suite_abi::AegpLayerOptionsNew>);
static_assert(std::is_same_v<decltype(&duplicate_layer_render_options),
                             aexcompat::suite_abi::AegpLayerOptionsDuplicate>);
static_assert(std::is_same_v<decltype(&dispose_layer_render_options),
                             aexcompat::suite_abi::AegpLayerOptionsDispose>);
static_assert(std::is_same_v<decltype(&set_layer_render_time),
                             aexcompat::suite_abi::AegpLayerOptionsSetTime>);
static_assert(std::is_same_v<decltype(&get_layer_render_time),
                             aexcompat::suite_abi::AegpLayerOptionsGetTime>);
static_assert(std::is_same_v<decltype(&set_layer_render_time_step),
                             aexcompat::suite_abi::AegpLayerOptionsSetTime>);
static_assert(std::is_same_v<decltype(&get_layer_render_time_step),
                             aexcompat::suite_abi::AegpLayerOptionsGetTime>);
static_assert(std::is_same_v<decltype(&set_layer_render_world_type),
                             aexcompat::suite_abi::AegpLayerOptionsSetWorldType>);
static_assert(std::is_same_v<decltype(&get_layer_render_world_type),
                             aexcompat::suite_abi::AegpLayerOptionsGetWorldType>);
static_assert(std::is_same_v<decltype(&set_layer_render_downsample),
                             aexcompat::suite_abi::AegpLayerOptionsSetDownsample>);
static_assert(std::is_same_v<decltype(&get_layer_render_downsample),
                             aexcompat::suite_abi::AegpLayerOptionsGetDownsample>);
static_assert(std::is_same_v<decltype(&set_layer_render_matte),
                             aexcompat::suite_abi::AegpLayerOptionsSetMatte>);
static_assert(std::is_same_v<decltype(&get_layer_render_matte),
                             aexcompat::suite_abi::AegpLayerOptionsGetMatte>);
AegpLayerRenderOptionsSuite1& g_layer_render_options_suite1 =
    aexcompat::suite_abi::aegp_layer_render_options_suite1_table();
AegpLayerRenderOptionsSuite2& g_layer_render_options_suite2 =
    aexcompat::suite_abi::aegp_layer_render_options_suite2_table();
static_assert(std::is_same_v<decltype(&render_options_new_from_item),
                             aexcompat::suite_abi::AegpRenderOptionsNew>);
static_assert(std::is_same_v<decltype(&render_options_duplicate),
                             aexcompat::suite_abi::AegpRenderOptionsDuplicate>);
static_assert(std::is_same_v<decltype(&render_options_dispose),
                             aexcompat::suite_abi::AegpRenderOptionsDispose>);
static_assert(std::is_same_v<decltype(&render_options_set_time),
                             aexcompat::suite_abi::AegpRenderOptionsSetTime>);
static_assert(std::is_same_v<decltype(&render_options_get_time),
                             aexcompat::suite_abi::AegpRenderOptionsGetTime>);
static_assert(std::is_same_v<decltype(&render_options_set_time_step),
                             aexcompat::suite_abi::AegpRenderOptionsSetTime>);
static_assert(std::is_same_v<decltype(&render_options_get_time_step),
                             aexcompat::suite_abi::AegpRenderOptionsGetTime>);
static_assert(std::is_same_v<decltype(&render_options_set_field),
                             aexcompat::suite_abi::AegpRenderOptionsSetI32>);
static_assert(std::is_same_v<decltype(&render_options_get_field),
                             aexcompat::suite_abi::AegpRenderOptionsGetI32>);
static_assert(std::is_same_v<decltype(&render_options_set_world_type),
                             aexcompat::suite_abi::AegpRenderOptionsSetI32>);
static_assert(std::is_same_v<decltype(&render_options_get_world_type),
                             aexcompat::suite_abi::AegpRenderOptionsGetI32>);
static_assert(std::is_same_v<decltype(&render_options_set_downsample),
                             aexcompat::suite_abi::AegpRenderOptionsSetDownsample>);
static_assert(std::is_same_v<decltype(&render_options_get_downsample),
                             aexcompat::suite_abi::AegpRenderOptionsGetDownsample>);
static_assert(std::is_same_v<decltype(&render_options_set_roi),
                             aexcompat::suite_abi::AegpRenderOptionsSetRoi>);
static_assert(std::is_same_v<decltype(&render_options_get_roi),
                             aexcompat::suite_abi::AegpRenderOptionsGetRoi>);
static_assert(std::is_same_v<decltype(&render_options_set_matte),
                             aexcompat::suite_abi::AegpRenderOptionsSetI32>);
static_assert(std::is_same_v<decltype(&render_options_get_matte),
                             aexcompat::suite_abi::AegpRenderOptionsGetI32>);
static_assert(std::is_same_v<decltype(&render_options_set_channel_order),
                             aexcompat::suite_abi::AegpRenderOptionsSetI8>);
static_assert(std::is_same_v<decltype(&render_options_get_channel_order),
                             aexcompat::suite_abi::AegpRenderOptionsGetI8>);
static_assert(std::is_same_v<decltype(&render_options_get_guide_layers),
                             aexcompat::suite_abi::AegpRenderOptionsGetU8>);
static_assert(std::is_same_v<decltype(&render_options_set_guide_layers),
                             aexcompat::suite_abi::AegpRenderOptionsSetU8>);
static_assert(std::is_same_v<decltype(&render_options_get_quality),
                             aexcompat::suite_abi::AegpRenderOptionsGetI8>);
static_assert(std::is_same_v<decltype(&render_options_set_quality),
                             aexcompat::suite_abi::AegpRenderOptionsSetI8>);
AegpRenderOptionsSuite1& g_render_options_suite1 =
    aexcompat::suite_abi::aegp_render_options_suite1_table();
AegpRenderOptionsSuite4& g_render_options_suite4 =
    aexcompat::suite_abi::aegp_render_options_suite4_table();
int32_t __cdecl checkout_item_frame_async(void*, uint32_t, void*, void**);
int32_t __cdecl checkout_layer_frame_async(void*, uint32_t, void*, void**);
struct AegpRenderAsyncManagerSuite1 {
  decltype(&checkout_item_frame_async) checkout_item_frame;
  decltype(&checkout_layer_frame_async) checkout_layer_frame;
};
static_assert(sizeof(AegpRenderAsyncManagerSuite1) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderAsyncManagerSuite1, checkout_item_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderAsyncManagerSuite1, checkout_layer_frame) == 1 * sizeof(void*));
AegpRenderAsyncManagerSuite1 g_render_async_manager_suite1{};

using AegpRenderCancelV1 = int32_t(__cdecl*)(void*, uint8_t*);
using AegpAsyncFrameReadyCallback =
    int32_t(__cdecl*)(uint64_t, uint8_t, int32_t, void*, void*);
int32_t __cdecl render_checkout_frame_reject(void*, AegpRenderCancelV1, void*, void**);
int32_t __cdecl render_checkout_layer_reject(void*, uint8_t, void*, void*, void**);
int32_t __cdecl render_checkout_layer_v5(void*, AegpRenderCancelV1, void*, void**);
int32_t __cdecl render_checkout_layer_async_reject(
    void*, AegpAsyncFrameReadyCallback, void*, uint64_t*);
int32_t __cdecl render_cancel_async_reject(uint64_t);
int32_t __cdecl checkin_frame(void*);
int32_t __cdecl get_receipt_world(void*, void***);
int32_t __cdecl render_get_region_reject(void*, void*);
int32_t __cdecl render_sufficient_reject(void*, void*, uint8_t*);
int32_t __cdecl render_sound_reject(void*, const void*, const void*, const void*, void*, void*, void**);
int32_t __cdecl render_timestamp_reject(void*);
int32_t __cdecl render_changed_reject(void*, const void*, const void*, const void*, uint8_t*);
int32_t __cdecl render_worthwhile_reject(void*, const void*, uint8_t*);
int32_t __cdecl render_checkin_rendered(void*, const void*, uint32_t, void*);
int32_t __cdecl render_guid_reject(void*, void**);
struct AegpRenderSuite4 {
  decltype(&render_checkout_frame_reject) render_frame;
  decltype(&render_checkout_layer_reject) render_layer;
  decltype(&checkin_frame) checkin;
  decltype(&get_receipt_world) get_world;
  decltype(&render_get_region_reject) get_region;
  decltype(&render_sufficient_reject) sufficient;
  decltype(&render_sound_reject) render_sound;
  decltype(&render_timestamp_reject) timestamp;
  decltype(&render_changed_reject) changed;
  decltype(&render_worthwhile_reject) worthwhile;
  decltype(&render_checkin_rendered) checkin_rendered;
  decltype(&render_guid_reject) guid;
};
static_assert(sizeof(AegpRenderSuite4) == 12 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, render_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, render_layer) == 1 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, checkin) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, get_world) == 3 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, get_region) == 4 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, sufficient) == 5 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, render_sound) == 6 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, timestamp) == 7 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, changed) == 8 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, worthwhile) == 9 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, checkin_rendered) == 10 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, guid) == 11 * sizeof(void*));
AegpRenderSuite4 g_aegp_render_suite4{};
struct AegpRenderSuite5 {
  decltype(&render_checkout_frame_reject) render_frame;
  decltype(&render_checkout_layer_v5) render_layer;
  decltype(&render_checkout_layer_async_reject) render_layer_async;
  decltype(&render_cancel_async_reject) cancel_async;
  decltype(&checkin_frame) checkin;
  decltype(&get_receipt_world) get_world;
  decltype(&render_get_region_reject) get_region;
  decltype(&render_sufficient_reject) sufficient;
  decltype(&render_sound_reject) render_sound;
  decltype(&render_timestamp_reject) timestamp;
  decltype(&render_changed_reject) changed;
  decltype(&render_worthwhile_reject) worthwhile;
  decltype(&render_checkin_rendered) checkin_rendered;
  decltype(&render_guid_reject) guid;
};
static_assert(sizeof(AegpRenderSuite5) == 14 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, render_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, render_layer) == 1 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, render_layer_async) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, cancel_async) == 3 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, checkin) == 4 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, get_world) == 5 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, guid) == 13 * sizeof(void*));
AegpRenderSuite5 g_aegp_render_suite5{};
struct AegpRenderSuite2 {
  decltype(&render_checkout_frame_reject) render_frame;
  decltype(&checkin_frame) checkin;
  decltype(&get_receipt_world) get_world;
  decltype(&render_get_region_reject) get_region;
  decltype(&render_sufficient_reject) sufficient;
  decltype(&render_sound_reject) render_sound;
  decltype(&render_timestamp_reject) timestamp;
  decltype(&render_changed_reject) changed;
  decltype(&render_worthwhile_reject) worthwhile;
  decltype(&render_checkin_rendered) checkin_rendered;
};
static_assert(sizeof(AegpRenderSuite2) == 10 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, render_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, checkin) == 1 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, get_world) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, get_region) == 3 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, sufficient) == 4 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, render_sound) == 5 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, timestamp) == 6 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, changed) == 7 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, worthwhile) == 8 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, checkin_rendered) == 9 * sizeof(void*));
AegpRenderSuite2 g_aegp_render_suite2{};
static_assert(std::is_same_v<decltype(&aegp_world_new_owned),
                             aexcompat::suite_abi::AegpWorldNew>);
static_assert(std::is_same_v<decltype(&aegp_world_dispose),
                             aexcompat::suite_abi::AegpWorldDispose>);
static_assert(std::is_same_v<decltype(&aegp_world_get_type),
                             aexcompat::suite_abi::AegpWorldGetType>);
static_assert(std::is_same_v<decltype(&aegp_world_get_size),
                             aexcompat::suite_abi::AegpWorldGetSize>);
static_assert(std::is_same_v<decltype(&aegp_world_get_rowbytes),
                             aexcompat::suite_abi::AegpWorldGetRowbytes>);
static_assert(std::is_same_v<decltype(&aegp_world_get_base_addr8),
                             aexcompat::suite_abi::AegpWorldGetBaseAddress8>);
static_assert(std::is_same_v<decltype(&aegp_world_get_base_addr16),
                             aexcompat::suite_abi::AegpWorldGetBaseAddress16>);
static_assert(std::is_same_v<decltype(&aegp_world_get_base_addr32),
                             aexcompat::suite_abi::AegpWorldGetBaseAddress32>);
static_assert(std::is_same_v<decltype(&aegp_world_fill_pf_world),
                             aexcompat::suite_abi::AegpWorldFillPfWorld>);
static_assert(std::is_same_v<decltype(&aegp_world_fast_blur),
                             aexcompat::suite_abi::AegpWorldFastBlur>);
static_assert(std::is_same_v<decltype(&aegp_world_new_platform),
                             aexcompat::suite_abi::AegpWorldNewPlatform>);
static_assert(std::is_same_v<decltype(&aegp_world_dispose_platform),
                             aexcompat::suite_abi::AegpWorldDisposePlatform>);
static_assert(std::is_same_v<decltype(&aegp_world_reference_platform),
                             aexcompat::suite_abi::AegpWorldReferencePlatform>);
enum class ColorProfileKind : uint8_t { Srgb, LinearSrgb, ImportedRgb };
struct AegpGuidValue { std::array<uint8_t, 16> bytes{}; };
struct ColorProfileRecord {
  uint64_t generation{};
  int32_t owner_plugin_id{1};
  ColorProfileKind kind{ColorProfileKind::Srgb};
  std::vector<uint8_t> icc_bytes;
  bool live{true};
};
constexpr std::size_t kMaxColorProfiles = 32;
constexpr std::size_t kMaxIccProfileBytes = 16384;
constexpr std::array<uint8_t, 16> kWorkingSrgbGuid = {
    0x7a, 0x5c, 0xe3, 0x0c, 0x9d, 0x4d, 0x4f, 0x9a,
    0x8b, 0x1f, 0x6c, 0x2d, 0x4e, 0x8a, 0x11, 0x01};
constexpr std::array<uint8_t, 16> kWorkingLinearSrgbGuid = {
    0x7a, 0x5c, 0xe3, 0x0c, 0x9d, 0x4d, 0x4f, 0x9a,
    0x8b, 0x1f, 0x6c, 0x2d, 0x4e, 0x8a, 0x11, 0x02};
struct AegpItemViewToken { uint32_t tag{0x56494557}; };
AegpItemViewToken g_aegp_item_view{};
std::mutex g_color_settings_mutex;
std::unordered_map<void*, ColorProfileRecord> g_color_profiles;
std::atomic<uint64_t> g_color_profile_generation{1};
ColorProfileKind g_working_color_space_kind{ColorProfileKind::Srgb};
std::vector<uint8_t> g_working_color_space_icc;
uint32_t g_color_profiles_created{};
uint32_t g_color_profiles_disposed{};
uint32_t g_invalid_color_profile_operations{};
uint32_t g_color_xform_calls{};
int32_t __cdecl color_get_blending_tables(void*, void**);
int32_t __cdecl color_does_view_have_xform(void*, uint8_t*);
int32_t __cdecl color_xform_working_to_view(void*, void**, void**);
int32_t __cdecl color_get_new_working_space_profile(int32_t, void*, void**);
int32_t __cdecl color_get_new_profile_from_icc(int32_t, int32_t, const void*, void**);
int32_t __cdecl color_get_new_icc_from_profile(int32_t, void*, void**);
int32_t __cdecl color_get_new_profile_description(int32_t, void*, void**);
int32_t __cdecl color_dispose_profile(void*);
int32_t __cdecl color_get_profile_approximate_gamma(void*, float*);
int32_t __cdecl color_is_rgb_profile(void*, uint8_t*);
int32_t __cdecl color_set_working_color_space(int32_t, void*, void*);
int32_t __cdecl color_is_ocio_used(int32_t, uint8_t*);
int32_t __cdecl color_get_ocio_configuration_file(int32_t, void**);
int32_t __cdecl color_get_ocio_configuration_file_path(int32_t, void**);
int32_t __cdecl color_get_ocio_working_colorspace(int32_t, void**);
int32_t __cdecl color_get_ocio_display_colorspace(int32_t, void**, void**);
int32_t __cdecl color_is_colorspace_aware_effects_enabled(int32_t, uint8_t*);
int32_t __cdecl color_get_lut_interpolation_method(int32_t, uint16_t*);
int32_t __cdecl color_get_graphics_white_luminance(int32_t, uint16_t*);
int32_t __cdecl color_get_working_colorspace_id(int32_t, AegpGuidValue*);
struct AegpColorSettingsSuite6 {
  decltype(&color_get_blending_tables) get_blending_tables;
  decltype(&color_does_view_have_xform) does_view_have_xform;
  decltype(&color_xform_working_to_view) xform_working_to_view;
  decltype(&color_get_new_working_space_profile) get_new_working_space_profile;
  decltype(&color_get_new_profile_from_icc) get_new_profile_from_icc;
  decltype(&color_get_new_icc_from_profile) get_new_icc_from_profile;
  decltype(&color_get_new_profile_description) get_new_profile_description;
  decltype(&color_dispose_profile) dispose_profile;
  decltype(&color_get_profile_approximate_gamma) get_profile_approximate_gamma;
  decltype(&color_is_rgb_profile) is_rgb_profile;
  decltype(&color_set_working_color_space) set_working_color_space;
  decltype(&color_is_ocio_used) is_ocio_used;
  decltype(&color_get_ocio_configuration_file) get_ocio_configuration_file;
  decltype(&color_get_ocio_configuration_file_path) get_ocio_configuration_file_path;
  decltype(&color_get_ocio_working_colorspace) get_ocio_working_colorspace;
  decltype(&color_get_ocio_display_colorspace) get_ocio_display_colorspace;
  decltype(&color_is_colorspace_aware_effects_enabled) is_colorspace_aware_effects_enabled;
  decltype(&color_get_lut_interpolation_method) get_lut_interpolation_method;
  decltype(&color_get_graphics_white_luminance) get_graphics_white_luminance;
  decltype(&color_get_working_colorspace_id) get_working_colorspace_id;
};
static_assert(sizeof(AegpColorSettingsSuite6) == 20 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_blending_tables) == 0 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, does_view_have_xform) == 1 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, xform_working_to_view) == 2 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_working_space_profile) == 3 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_profile_from_icc) == 4 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_icc_from_profile) == 5 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_new_profile_description) == 6 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, dispose_profile) == 7 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_profile_approximate_gamma) == 8 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, is_rgb_profile) == 9 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, set_working_color_space) == 10 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, is_ocio_used) == 11 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_configuration_file) == 12 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_configuration_file_path) == 13 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_working_colorspace) == 14 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_ocio_display_colorspace) == 15 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, is_colorspace_aware_effects_enabled) == 16 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_lut_interpolation_method) == 17 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_graphics_white_luminance) == 18 * sizeof(void*));
static_assert(offsetof(AegpColorSettingsSuite6, get_working_colorspace_id) == 19 * sizeof(void*));
AegpColorSettingsSuite6 g_color_settings_suite6{};
std::array<void*, 8> g_effect_overlay_theme_suite1{};
std::array<void*, 11> g_app_suite4{};
std::array<void*, 12> g_app_suite5{};
std::array<void*, 15> g_app_suite6{};
struct DrawbotOpaque { uint32_t tag; };
DrawbotOpaque g_drawbot_draw{0x44524157};
DrawbotOpaque g_drawbot_supplier{0x53555050};
DrawbotOpaque g_drawbot_surface{0x53555246};
enum class DrawbotObjectKind { Pen, Brush, Path };
struct DrawbotObject {
  DrawbotObjectKind kind{};
  std::array<float, 4> color{};
  std::array<float, 4> rect{};
  float pen_size{};
  uint32_t path_points{};
};
std::unordered_map<void*, std::unique_ptr<DrawbotObject>> g_drawbot_objects;
uint32_t g_drawbot_objects_created{};
uint32_t g_drawbot_objects_released{};
uint32_t g_drawbot_paint_rect_calls{};
uint32_t g_drawbot_fill_path_calls{};
uint32_t g_drawbot_stroke_path_calls{};
uint32_t g_drawbot_invalid_operations{};
uint32_t g_drawbot_get_supplier_calls{};
uint32_t g_drawbot_get_surface_calls{};
uint32_t g_drawbot_get_drawing_ref_calls{};
uint32_t g_overlay_stroke_path_calls{};
uint32_t g_app_get_background_color_calls{};
uint32_t g_app_color_picker_calls{};
uint32_t g_app_invalidate_rect_calls{};
uint32_t g_app_progress_dialogs_created{};
uint32_t g_app_progress_dialogs_disposed{};
std::array<float, 4> g_app_picker_color{1.0f, 0.25f, 0.75f, 0.5f};
std::array<int32_t, 4> g_app_invalidated_rect{};
struct HostUiContext {
  uint32_t magic{0x05ea771e};
  int32_t window_type{2};
  void* reserved_filter{};
  std::array<intptr_t, 4> plugin_state{};
  void* draw_ref{};
  void* pane{};
  void* job_manager{};
};
HostUiContext g_ui_context;
HostUiContext* g_ui_context_pointer = &g_ui_context;
struct PfHelperUiContextScope {
  explicit PfHelperUiContextScope(int32_t context)
      : runtime_scope(context), previous_active(g_render_ui_context_active) {
    g_render_ui_context_active = context >= 0 && context < 3;
  }
  ~PfHelperUiContextScope() { g_render_ui_context_active = previous_active; }
  aexcompat::pf_helper::UiContextScope runtime_scope;
  bool previous_active;
};
uint32_t g_ui_drag_calls{};
bool g_ui_drag_requested{};
bool g_ui_drag_terminated{};
uint32_t g_ui_coordinate_transform_calls{};
bool g_render_click_enabled{};
bool g_render_draw_enabled{};
int32_t g_render_click_x{};
int32_t g_render_click_y{};
int32_t g_render_click_error{-1};
int32_t g_render_click_out_flags{};
bool g_render_click_changed_value{};
int32_t g_render_draw_error{-1};
int32_t g_render_draw_out_flags{};
std::array<int32_t, 4> g_render_ui_lifecycle_errors{-1, -1, -1, -1};
bool g_render_ui_context_closed{};
std::vector<std::array<float, 4>> g_drawbot_fill_colors;

int32_t __cdecl drawbot_get_supplier(void* draw, void** supplier) {
  if (draw != &g_drawbot_draw || !supplier) return 4;
  *supplier = &g_drawbot_supplier;
  ++g_drawbot_get_supplier_calls;
  return 0;
}
int32_t __cdecl drawbot_get_surface(void* draw, void** surface) {
  if (draw != &g_drawbot_draw || !surface) return 4;
  *surface = &g_drawbot_surface;
  ++g_drawbot_get_surface_calls;
  return 0;
}
int32_t new_drawbot_object(DrawbotObjectKind kind, void** output) {
  if (!output || g_drawbot_objects.size() >= 256) return 4;
  auto object = std::make_unique<DrawbotObject>();
  object->kind = kind;
  void* key = object.get();
  g_drawbot_objects.emplace(key, std::move(object));
  ++g_drawbot_objects_created;
  *output = key;
  return 0;
}
int32_t __cdecl drawbot_new_pen(void* supplier, const float* color, float size, void** pen) {
  if (supplier != &g_drawbot_supplier || !color || !std::isfinite(size) || size <= 0) return 4;
  const int32_t error = new_drawbot_object(DrawbotObjectKind::Pen, pen);
  if (!error) {
    std::copy_n(color, 4, g_drawbot_objects[*pen]->color.begin());
    g_drawbot_objects[*pen]->pen_size = size;
  }
  return error;
}
int32_t __cdecl drawbot_new_brush(void* supplier, const float* color, void** brush) {
  if (supplier != &g_drawbot_supplier || !color) return 4;
  const int32_t error = new_drawbot_object(DrawbotObjectKind::Brush, brush);
  if (!error) std::copy_n(color, 4, g_drawbot_objects[*brush]->color.begin());
  return error;
}
int32_t __cdecl drawbot_new_path(void* supplier, void** path) {
  return supplier == &g_drawbot_supplier ? new_drawbot_object(DrawbotObjectKind::Path, path) : 4;
}
int32_t __cdecl drawbot_release_object(void* object) {
  const auto found = g_drawbot_objects.find(object);
  if (found == g_drawbot_objects.end()) { ++g_drawbot_invalid_operations; return 4; }
  g_drawbot_objects.erase(found);
  ++g_drawbot_objects_released;
  return 0;
}
int32_t __cdecl drawbot_add_rect(void* path, const float* rect) {
  const auto found = g_drawbot_objects.find(path);
  if (found == g_drawbot_objects.end() || found->second->kind != DrawbotObjectKind::Path || !rect)
    return 4;
  if (!std::all_of(rect, rect + 4, [](float value) { return std::isfinite(value); }) ||
      rect[2] < 0 || rect[3] < 0) return 4;
  std::copy_n(rect, 4, found->second->rect.begin());
  return 0;
}
int32_t __cdecl drawbot_path_point(void* path, float x, float y) {
  const auto found = g_drawbot_objects.find(path);
  if (found == g_drawbot_objects.end() || found->second->kind != DrawbotObjectKind::Path ||
      !std::isfinite(x) || !std::isfinite(y) || found->second->path_points >= 4096) return 4;
  ++found->second->path_points;
  return 0;
}
int32_t __cdecl drawbot_paint_rect(void* surface, const float* color, const float* rect) {
  if (surface != &g_drawbot_surface || !color || !rect) return 4;
  ++g_drawbot_paint_rect_calls;
  return 0;
}
int32_t __cdecl drawbot_fill_path(void* surface, void* brush, void* path, int32_t fill_type) {
  const auto brush_it = g_drawbot_objects.find(brush), path_it = g_drawbot_objects.find(path);
  if (surface != &g_drawbot_surface || fill_type != 1 ||
      brush_it == g_drawbot_objects.end() || path_it == g_drawbot_objects.end() ||
      brush_it->second->kind != DrawbotObjectKind::Brush ||
      path_it->second->kind != DrawbotObjectKind::Path) return 4;
  g_drawbot_fill_colors.push_back(brush_it->second->color);
  ++g_drawbot_fill_path_calls;
  return 0;
}
int32_t __cdecl drawbot_stroke_path(void* surface, void* pen, void* path) {
  const auto pen_it = g_drawbot_objects.find(pen), path_it = g_drawbot_objects.find(path);
  if (surface != &g_drawbot_surface || pen_it == g_drawbot_objects.end() ||
      path_it == g_drawbot_objects.end() || pen_it->second->kind != DrawbotObjectKind::Pen ||
      path_it->second->kind != DrawbotObjectKind::Path) return 4;
  ++g_drawbot_stroke_path_calls;
  return 0;
}
int32_t __cdecl get_drawing_reference(void* context, void** drawing) {
  if (context != &g_ui_context_pointer || !drawing) return 4;
  *drawing = &g_drawbot_draw;
  ++g_drawbot_get_drawing_ref_calls;
  return 0;
}
int g_async_manager{};
int32_t __cdecl get_context_async_manager(void* input, void* extra, void** manager) {
  if (!input || !extra || !manager) return 4;
  *manager = &g_async_manager;
  return 0;
}
int32_t __cdecl app_get_background_color(uint16_t* color) {
  if (!color) return 4;
  color[0] = color[1] = color[2] = 0x3030;
  ++g_app_get_background_color_calls;
  return 0;
}
int32_t __cdecl app_get_color(int16_t color_type, uint16_t* color) {
  if (!color || color_type < 0 || (color_type > 127 && (color_type < 1000 || color_type > 1004)))
    return 4;
  const uint16_t value = static_cast<uint16_t>(0x2020 + (color_type & 7) * 0x0808);
  color[0] = color[1] = color[2] = value;
  return 0;
}
int32_t __cdecl app_get_language(char* language) {
  if (!language) return 4;
  std::memcpy(language, "en_US", sizeof("en_US"));
  return 0;
}
int32_t __cdecl app_get_font_style(int16_t, char*, int16_t*, int16_t*, int16_t*) { return 4; }
int32_t __cdecl app_set_cursor(int16_t) { return 4; }
int32_t __cdecl app_is_render_engine(uint8_t* render_engine) {
  if (!render_engine) return 4;
  *render_engine = 1;  // The SDK includes no-UI hosts in render-engine semantics.
  return 0;
}
int32_t __cdecl app_color_picker(const char* title, const float* sample_color,
                                 int32_t, float* new_color) {
  if (!g_render_ui_context_active || !title || !sample_color || !new_color) return 4;
  // PF_PixelFloat is alpha, red, green, blue; the CLI color is RGBA.
  new_color[0] = g_app_picker_color[3];
  new_color[1] = g_app_picker_color[0];
  new_color[2] = g_app_picker_color[1];
  new_color[3] = g_app_picker_color[2];
  ++g_app_color_picker_calls;
  return 0;
}
int32_t __cdecl app_invalidate_rect(void* context, const int32_t* rect) {
  if (context != &g_ui_context_pointer || !g_render_ui_context_active) return 4;
  if (rect) std::copy_n(rect, 4, g_app_invalidated_rect.begin());
  else g_app_invalidated_rect.fill(0);
  ++g_app_invalidate_rect_calls;
  return 0;
}
int32_t __cdecl app_get_mouse(int32_t*) { return 4; }
int32_t __cdecl app_convert_local_to_global(const int32_t*, int32_t*) { return 4; }
int32_t __cdecl app_get_color_at_global_point(const int32_t*, int16_t, int16_t, float*) {
  return 4;
}
struct AppProgressDialog { uint32_t magic{0x50524744}; };
std::unordered_map<void*, std::unique_ptr<AppProgressDialog>> g_app_progress_dialogs;
int32_t __cdecl app_create_progress_dialog(const uint16_t* title, const uint16_t*, int32_t,
                                            void** dialog) {
  if (!title || !dialog || g_app_progress_dialogs.size() >= 32) return 4;
  auto progress = std::make_unique<AppProgressDialog>();
  void* key = progress.get();
  g_app_progress_dialogs.emplace(key, std::move(progress));
  *dialog = key;
  ++g_app_progress_dialogs_created;
  return 0;
}
int32_t __cdecl app_update_progress_dialog(void* dialog, int32_t count, int32_t total) {
  if (g_app_progress_dialogs.find(dialog) == g_app_progress_dialogs.end() || count < 0 ||
      total < 0 || (total != 0 && count > total)) return 4;
  return 0;
}
int32_t __cdecl app_dispose_progress_dialog(void* dialog) {
  if (g_app_progress_dialogs.erase(dialog) != 1) return 4;
  ++g_app_progress_dialogs_disposed;
  return 0;
}
int32_t __cdecl overlay_foreground(float* color) {
  if (!color) return 4;
  color[0] = color[1] = color[2] = 0.9f;
  color[3] = 1.0f;
  return 0;
}
int32_t __cdecl overlay_stroke_path(void* draw, void* path, int32_t) {
  const auto found = g_drawbot_objects.find(path);
  if (draw != &g_drawbot_draw || found == g_drawbot_objects.end() ||
      found->second->kind != DrawbotObjectKind::Path || found->second->path_points == 0) return 4;
  ++g_overlay_stroke_path_calls;
  return 0;
}
int32_t __cdecl ui_transform_point(void*, void* context, int32_t, uint32_t, int32_t* point) {
  if (context != &g_ui_context_pointer || !point) return 4;
  ++g_ui_coordinate_transform_calls;
  return 0;
}
int32_t __cdecl ui_transform_point_simple(void*, void* context, int32_t* point) {
  if (context != &g_ui_context_pointer || !point) return 4;
  ++g_ui_coordinate_transform_calls;
  return 0;
}

struct PfAeChannelSuite1 {
  decltype(&get_layer_channel_count) count;
  decltype(&get_layer_channel_indexed) indexed;
  decltype(&get_layer_channel_typed) typed;
  decltype(&checkout_layer_channel) checkout;
  decltype(&checkin_layer_channel) checkin;
};
static_assert(sizeof(PfAeChannelSuite1) == 5 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, count) == 0 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, indexed) == 1 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, typed) == 2 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, checkout) == 3 * sizeof(void*));
static_assert(offsetof(PfAeChannelSuite1, checkin) == 4 * sizeof(void*));
PfAeChannelSuite1 g_channel_suite1{&get_layer_channel_count, &get_layer_channel_indexed,
    &get_layer_channel_typed, &checkout_layer_channel, &checkin_layer_channel};
std::array<void*, 1> g_duck_suite1{};
MaskSuite g_mask_suite{&get_layer_num_masks, &get_layer_mask_by_index, &dispose_mask,
    &get_mask_invert, &set_mask_invert, &get_mask_mode, &set_mask_mode,
    &get_mask_motion_blur, &set_mask_motion_blur,
    &get_mask_feather_falloff, &set_mask_feather_falloff, &get_mask_id,
    &create_new_mask, &delete_mask_from_layer, &get_mask_color, &set_mask_color,
    &get_mask_lock, &set_mask_lock, &get_mask_roto_bezier, &set_mask_roto_bezier,
    &duplicate_mask};
MaskSuite5 g_mask_suite5{&get_layer_num_masks, &get_layer_mask_by_index, &dispose_mask,
    &get_mask_invert, &set_mask_invert, &get_mask_mode, &set_mask_mode,
    &get_mask_motion_blur, &set_mask_motion_blur, &get_mask_id,
    &create_new_mask, &delete_mask_from_layer, &get_mask_color, &set_mask_color,
    &get_mask_lock, &set_mask_lock, &get_mask_roto_bezier, &set_mask_roto_bezier,
    &duplicate_mask};
StreamSuite g_stream_suite{&is_stream_legal, &can_vary_over_time,
    &get_valid_interpolations, &unsupported_new_layer_stream,
    &unsupported_effect_stream_count, &unsupported_new_effect_stream,
    &get_new_mask_stream, &dispose_stream, &unsupported_stream_name,
    &get_stream_units_text, &get_stream_properties, &is_stream_timevarying,
    &get_stream_type, &get_new_stream_value, &dispose_stream_value,
    &set_stream_value, &unsupported_layer_stream_value,
    &get_expression_state, &reject_expression_state, &unsupported_get_expression,
    &unsupported_set_expression, &duplicate_stream_ref, &get_unique_stream_id};
KeyframeSuite g_keyframe_suite{&get_stream_num_keyframes, &get_keyframe_time,
    &insert_keyframe, &delete_keyframe, &get_new_keyframe_value,
    &set_keyframe_value, &get_stream_value_dimensionality,
    &get_stream_temporal_dimensionality, &get_new_keyframe_spatial_tangents,
    &set_keyframe_spatial_tangents, &get_keyframe_temporal_ease,
    &set_keyframe_temporal_ease, &get_keyframe_flags, &set_keyframe_flag,
    &get_keyframe_interpolation, &set_keyframe_interpolation,
    &start_add_keyframes, &add_keyframes, &set_add_keyframe,
    &end_add_keyframes, &get_keyframe_label, &set_keyframe_label};

bool verify_aegp_keyframe_suite5_mutations() {
  const bool abi_wiring =
      g_keyframe_suite.get_stream_num_keyframes == &get_stream_num_keyframes &&
      g_keyframe_suite.get_keyframe_time == &get_keyframe_time &&
      g_keyframe_suite.insert_keyframe == &insert_keyframe &&
      g_keyframe_suite.delete_keyframe == &delete_keyframe &&
      g_keyframe_suite.get_new_keyframe_value == &get_new_keyframe_value &&
      g_keyframe_suite.set_keyframe_value == &set_keyframe_value &&
      g_keyframe_suite.get_stream_value_dimensionality == &get_stream_value_dimensionality &&
      g_keyframe_suite.get_stream_temporal_dimensionality ==
          &get_stream_temporal_dimensionality &&
      g_keyframe_suite.get_new_keyframe_spatial_tangents ==
          &get_new_keyframe_spatial_tangents &&
      g_keyframe_suite.set_keyframe_spatial_tangents == &set_keyframe_spatial_tangents &&
      g_keyframe_suite.get_keyframe_temporal_ease == &get_keyframe_temporal_ease &&
      g_keyframe_suite.set_keyframe_temporal_ease == &set_keyframe_temporal_ease &&
      g_keyframe_suite.get_keyframe_flags == &get_keyframe_flags &&
      g_keyframe_suite.set_keyframe_flag == &set_keyframe_flag &&
      g_keyframe_suite.get_keyframe_interpolation == &get_keyframe_interpolation &&
      g_keyframe_suite.set_keyframe_interpolation == &set_keyframe_interpolation &&
      g_keyframe_suite.start_add_keyframes == &start_add_keyframes &&
      g_keyframe_suite.add_keyframes == &add_keyframes &&
      g_keyframe_suite.set_add_keyframe == &set_add_keyframe &&
      g_keyframe_suite.end_add_keyframes == &end_add_keyframes &&
      g_keyframe_suite.get_keyframe_label_color_index == &get_keyframe_label &&
      g_keyframe_suite.set_keyframe_label_color_index == &set_keyframe_label;
  if (!abi_wiring || !configure_mask_scene("rectangle")) return false;
  const bool passed = verify_keyframe_ownership_rejection() &&
      g_stream_refs.empty() && g_stream_values.empty() &&
      g_add_keyframe_transactions.empty() && mask_lifetimes_balanced();
  g_mask_scene.clear();
  g_mask_scene_id = "none";
  return passed;
}
DynamicStreamSuite g_dynamic_stream_suite{&get_new_dynamic_stream_for_layer,
    &get_new_dynamic_stream_for_mask, &get_dynamic_stream_depth,
    &get_dynamic_stream_grouping_type, &get_num_streams_in_group,
    &get_dynamic_stream_flags, &set_dynamic_stream_flag,
    &get_new_dynamic_stream_by_index, &get_new_dynamic_stream_by_match_name,
    &delete_dynamic_stream, &reorder_dynamic_stream, &duplicate_dynamic_stream,
    &set_dynamic_stream_name, &can_add_dynamic_stream, &add_dynamic_stream,
    &get_dynamic_match_name, &get_new_parent_dynamic_stream,
    &get_dynamic_stream_modified, &get_dynamic_stream_index,
    &is_separation_leader, &are_dimensions_separated,
    &reject_set_dimensions_separated, &reject_get_separation_follower,
    &is_separation_follower, &reject_get_separation_leader,
    &reject_get_separation_dimension};
MaskOutlineSuite g_mask_outline_suite{&is_mask_outline_open, &set_mask_outline_open,
                                      &get_mask_outline_num_segments,
                                      &get_mask_outline_vertex_info,
                                      &set_mask_outline_vertex_info,
                                      &create_mask_outline_vertex,
                                      &delete_mask_outline_vertex,
                                      &get_mask_outline_num_feathers,
                                      &get_mask_outline_feather_info,
                                      &set_mask_outline_feather_info,
                                      &create_mask_outline_feather,
                                      &delete_mask_outline_feather};

std::mutex g_world_mutex;

constexpr int32_t kSyntheticCompWidth = 17;
constexpr int32_t kSyntheticCompHeight = 9;
std::array<uint8_t, 4> g_render_options_baseline8{};
std::array<uint8_t, 4> g_render_options_time8{};
std::array<uint8_t, 4> g_render_options_downsample8{};
std::array<uint8_t, 4> g_render_options_roi_outside8{};
std::array<uint8_t, 4> g_render_options_roi_inside8{};
std::array<uint8_t, 4> g_render_options_field_excluded8{};
std::array<uint8_t, 4> g_render_options_matte8{};
std::array<uint16_t, 4> g_render_options_argb16{};
std::array<float, 4> g_render_options_argb32f{};
bool g_synthetic_receipt_test_mode{};
std::atomic<uint32_t> g_render_project_timestamp{1};
std::atomic<bool> g_render_timestamp_exhausted{};
struct StagedItemWorld {
  void* item{};
  AegpTime time{};
  AegpTime time_step{};
  int8_t quality{1};
  uint8_t guide_layers{};
  int32_t pixel_format{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  uint32_t project_generation{};
  uint64_t stage_generation{};
  std::shared_ptr<const std::vector<std::byte>> backing;
};
constexpr std::size_t kMaxStagedItemWorlds = 32;
std::mutex g_staged_item_world_mutex;
std::vector<StagedItemWorld> g_staged_item_worlds;
std::atomic<uint64_t> g_staged_item_generation{1};
struct ItemRenderStackKey {
  void* item{};
  AegpTime time{};
  uint32_t project_generation{};
};
thread_local std::vector<ItemRenderStackKey> g_item_render_stack;
uint32_t g_staged_item_worlds_published{};
uint32_t g_staged_item_world_cache_hits{};
uint32_t g_staged_item_world_cache_misses{};
uint32_t g_staged_item_world_cycles_rejected{};
struct ExternalRenderedFrame {
  AegpRenderOptionsValue options{};
  AegpRect rendered_region{};
  uint32_t timestamp{};
  uint32_t ticks_to_render{};
  std::shared_ptr<PlatformWorldBacking> backing;
};
std::vector<ExternalRenderedFrame> g_external_render_cache;
constexpr std::size_t kMaxExternalRenderCache = 16;
uint32_t g_external_frames_checked_in{};

bool same_render_options(const AegpRenderOptionsValue& left,
                         const AegpRenderOptionsValue& right) {
  return left.item == right.item && left.time.value == right.time.value &&
      left.time.scale == right.time.scale && left.time_step.value == right.time_step.value &&
      left.time_step.scale == right.time_step.scale && left.field == right.field &&
      left.world_type == right.world_type && left.downsample_x == right.downsample_x &&
      left.downsample_y == right.downsample_y &&
      std::memcmp(&left.roi, &right.roi, sizeof(left.roi)) == 0 && left.matte == right.matte &&
      left.channel_order == right.channel_order &&
      left.render_guide_layers == right.render_guide_layers &&
      left.render_quality == right.render_quality;
}
int32_t __cdecl app_get_personal_info(char* info) {
  if (!info) return 4;
  std::memset(info, 0, 64 * 3);
  std::memcpy(info, "AEXCompat", sizeof("AEXCompat"));
  std::memcpy(info + 64, "onmokoworks", sizeof("onmokoworks"));
  std::memcpy(info + 128, "SDK fixture", sizeof("SDK fixture"));
  return 0;
}
void bump_render_project_timestamp() {
  uint32_t current = g_render_project_timestamp.load();
  for (;;) {
    const uint32_t next = current == UINT32_MAX ? UINT32_MAX : current + 1;
    if (next == current) {
      g_render_timestamp_exhausted.store(true);
      std::lock_guard<std::mutex> lock(g_world_mutex);
      g_external_render_cache.clear();
      {
        std::lock_guard<std::mutex> stage_lock(g_staged_item_world_mutex);
        g_staged_item_worlds.clear();
      }
      return;
    }
    if (g_render_project_timestamp.compare_exchange_weak(current, next)) {
      std::lock_guard<std::mutex> lock(g_world_mutex);
      g_external_render_cache.clear();
      {
        std::lock_guard<std::mutex> stage_lock(g_staged_item_world_mutex);
        g_staged_item_worlds.clear();
      }
      return;
    }
  }
}

bool claim_opaque_generation(std::atomic<uint64_t>& counter, uint64_t& generation) {
  const uint64_t limit = (std::numeric_limits<uintptr_t>::max)() / 8;
  uint64_t current = counter.load();
  for (;;) {
    if (current == 0 || current > limit) return false;
    const uint64_t next = current == limit ? limit + 1 : current + 1;
    if (counter.compare_exchange_weak(current, next)) {
      generation = current;
      return true;
    }
  }
}

struct LoadedEffectReceiptContext {
  EffectEntry entry{};
  std::array<std::byte, kInSize>* input{};
  std::array<std::byte, kOutSize>* output{};
  int32_t current_time{};
  int32_t time_scale{1};
  std::string case_id{"default"};
  const void* requested{};
  const std::vector<unsigned char>* external_rgba{};
  const std::vector<ExternalLayerInput>* external_layers{};
  int32_t external_width{};
  int32_t external_height{};
  int32_t time_step{1};
  int32_t total_time{1};
  int32_t pixel_bytes{4};
  const std::vector<unsigned char>* source_argb{};
  int32_t source_width{};
  int32_t source_height{};
  const std::vector<unsigned char>* downstream_argb{};
  int32_t downstream_width{};
  int32_t downstream_height{};
  int32_t downstream_pixel_bytes{};
  bool downstream_finalized{};
  const std::vector<unsigned char>* all_effects_argb{};
  int32_t all_effects_width{};
  int32_t all_effects_height{};
  int32_t all_effects_pixel_bytes{};
  bool all_effects_finalized{};
  int32_t active_item_time{};
  bool active_item_time_valid{};
};
thread_local LoadedEffectReceiptContext g_loaded_effect_receipt_context{};
std::atomic<uint64_t> g_pf_adv_item_touches{};
std::atomic<uint64_t> g_pf_adv_item_rerenders{};

int32_t checked_adv_item_move(int32_t direction, int32_t steps, int32_t step,
                              int32_t& time) {
  if ((direction != 0 && direction != 1) || steps < 0 || step <= 0) return 4;
  const int64_t distance = static_cast<int64_t>(steps) * step;
  const int64_t moved = static_cast<int64_t>(time) + (direction == 0 ? distance : -distance);
  if (moved < INT32_MIN || moved > INT32_MAX) return 4;
  time = static_cast<int32_t>(moved);
  return 0;
}

bool active_adv_item_context(void* in_data) {
  return g_loaded_effect_receipt_context.entry && g_loaded_effect_receipt_context.input &&
      in_data == g_loaded_effect_receipt_context.input->data();
}

bool active_adv_item_world(const void* world, DispatchWorldFormat& result) {
  return resolve_registered_dispatch_world(world, result);
}

int32_t __cdecl adv_item_move_time_step(void* in_data, void* world,
                                        int32_t direction, int32_t steps) {
  auto& context = g_loaded_effect_receipt_context;
  DispatchWorldFormat effect_world{};
  if (!active_adv_item_context(in_data) || !active_adv_item_world(world, effect_world) ||
      !effect_world.data || effect_world.width <= 0 || effect_world.height <= 0 ||
      effect_world.rowbytes <= 0 || context.pixel_bytes <= 0 ||
      effect_world.width > INT32_MAX / context.pixel_bytes ||
      effect_world.rowbytes < effect_world.width * context.pixel_bytes) return 4;
  int32_t moved = context.current_time;
  if (checked_adv_item_move(direction, steps, context.time_step, moved) != 0) return 4;
  context.active_item_time = moved;
  context.active_item_time_valid = true;
  return 0;
}

int32_t __cdecl adv_item_move_time_step_active(int32_t direction, int32_t steps) {
  auto& context = g_loaded_effect_receipt_context;
  if (!context.entry || context.time_step <= 0) return 4;
  int32_t moved = context.active_item_time_valid ? context.active_item_time : context.current_time;
  if (checked_adv_item_move(direction, steps, context.time_step, moved) != 0) return 4;
  context.active_item_time = moved;
  context.active_item_time_valid = true;
  return 0;
}

int32_t __cdecl adv_item_touch_active() {
  if (!g_loaded_effect_receipt_context.entry) return 4;
  ++g_pf_adv_item_touches;
  bump_render_project_timestamp();
  return 0;
}

int32_t __cdecl adv_item_force_rerender(void* in_data, void* world) {
  DispatchWorldFormat effect_world{};
  if (!active_adv_item_context(in_data) || !active_adv_item_world(world, effect_world) ||
      !effect_world.data || effect_world.width <= 0 || effect_world.height <= 0 ||
      effect_world.rowbytes <= 0) return 4;
  ++g_pf_adv_item_rerenders;
  bump_render_project_timestamp();
  return 0;
}

int32_t __cdecl adv_item_effect_is_active(void* context_handle, uint8_t* enabled) {
  if (enabled) *enabled = 0;
  if (!context_handle || !enabled || !g_loaded_effect_receipt_context.entry) return 4;
  // UI context handles are opaque. A live render owns no UI context, so headless mode
  // can only report disabled without dereferencing an untrusted or stale handle.
  return 0;
}

struct PfAdvItemSuite1 {
  decltype(&adv_item_move_time_step) move_time_step;
  decltype(&adv_item_move_time_step_active) move_time_step_active_item;
  decltype(&adv_item_touch_active) touch_active_item;
  decltype(&adv_item_force_rerender) force_rerender;
  decltype(&adv_item_effect_is_active) effect_is_active_or_enabled;
};
static_assert(sizeof(PfAdvItemSuite1) == 5 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, move_time_step) == 0 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, move_time_step_active_item) == 1 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, touch_active_item) == 2 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, force_rerender) == 3 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, effect_is_active_or_enabled) == 4 * sizeof(void*));
PfAdvItemSuite1 g_adv_item_suite1{&adv_item_move_time_step,
    &adv_item_move_time_step_active, &adv_item_touch_active,
    &adv_item_force_rerender, &adv_item_effect_is_active};
bool g_loaded_effect_receipt_fixture_passed{};
bool g_loaded_effect_receipt_unsupported_rejected{};
bool g_loaded_effect_receipt_stale_world_rejected{};
struct AsyncLayerRequest {
  uint64_t id{};
  EffectEntry entry{};
  AegpAsyncFrameReadyCallback callback{};
  void* refcon{};
  int32_t world_type{1};
  AegpLayerRenderOptionsValue options{};
  int32_t pixel_bytes{4};
  int32_t width{};
  int32_t height{};
  int32_t current_time{};
  int32_t time_scale{1};
  std::vector<unsigned char> pixels;
  std::atomic<int32_t> state{0};  // 0 queued, 1 completing, 2 canceled, 3 done
  std::mutex gate_mutex;
  std::condition_variable gate_changed;
};
std::mutex g_async_layer_request_mutex;
std::unordered_map<uint64_t, std::shared_ptr<AsyncLayerRequest>> g_async_layer_requests;
std::vector<std::thread> g_async_layer_threads;
uint64_t g_next_async_layer_request_id{1};
uint64_t g_async_layer_reserved_bytes{};
bool g_async_layer_accepting{true};
uint32_t g_async_layer_requests_created{};
uint32_t g_async_layer_requests_completed{};
uint32_t g_async_layer_requests_canceled{};
uint32_t g_async_layer_callback_failures{};
uint32_t g_async_layer_callback_exceptions{};
bool g_async_layer_cancel_test_gate{};
void drain_async_layer_requests();

// Bounded synthetic-item contract, not an Adobe rendering model. Output pixels sample
// source (x * downsample_x, y * downsample_y); ROI and fields are evaluated there.
// Field 0 renders all rows, 1 renders even source rows, and 2 renders odd source rows.
std::array<uint8_t, 4> synthetic_item_pixel(const AegpRenderOptionsValue& options,
                                            int32_t source_x, int32_t source_y) {
  const int64_t scaled_time = static_cast<int64_t>(options.time.value) * 256 / options.time.scale;
  const uint8_t time = static_cast<uint8_t>(scaled_time);
  return {{static_cast<uint8_t>(64 + source_x * 11 + source_y * 7 + time),
           static_cast<uint8_t>(source_x * 17 + time),
           static_cast<uint8_t>(source_y * 29 + time * 3),
           static_cast<uint8_t>(source_x * 5 + source_y * 13 + time * 7)}};
}

void populate_synthetic_item_pixels(ReceiptDraft& receipt, int32_t type,
                                    int32_t width, int32_t height) {
  const auto& options = receipt.render_options;
  const AegpRect source_roi = options.roi.left == 0 && options.roi.top == 0 &&
      options.roi.right == 0 && options.roi.bottom == 0
      ? AegpRect{0, 0, kSyntheticCompWidth, kSyntheticCompHeight} : options.roi;
  const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 : 16);
  for (int32_t y = 0; y < height; ++y) {
    const int32_t source_y = y * options.downsample_y;
    for (int32_t x = 0; x < width; ++x) {
      const int32_t source_x = x * options.downsample_x;
      const bool in_roi = source_x >= source_roi.left && source_x < source_roi.right &&
          source_y >= source_roi.top && source_y < source_roi.bottom;
      const bool in_field = options.field == 0 ||
          (options.field == 1 && (source_y & 1) == 0) ||
          (options.field == 2 && (source_y & 1) != 0);
      if (!in_roi || !in_field) continue;
      auto pixel = synthetic_item_pixel(options, source_x, source_y);
      if (options.matte == 1) pixel[0] = 255; // Supported "opaque alpha" synthetic matte.
      if (options.channel_order == 1)
        pixel = {{pixel[3], pixel[2], pixel[1], pixel[0]}};
      std::byte* destination = receipt.pixels.data() +
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      if (type == 1) {
        std::memcpy(destination, pixel.data(), 4);
      } else if (type == 2) {
        std::array<uint16_t, 4> converted{};
        for (std::size_t channel = 0; channel < 4; ++channel)
          converted[channel] = static_cast<uint16_t>(pixel[channel]) * 257u;
        std::memcpy(destination, converted.data(), sizeof(converted));
      } else {
        std::array<float, 4> converted{};
        for (std::size_t channel = 0; channel < 4; ++channel)
          converted[channel] = static_cast<float>(pixel[channel]) / 255.0f;
        std::memcpy(destination, converted.data(), sizeof(converted));
      }
    }
  }
}

int32_t publish_async_receipt(int32_t pixel_format, void** output,
                              const AegpRenderOptionsValue* options = nullptr) {
  if (!output) return 4;
  *output = nullptr;
  const int32_t type = aegp_world_type_from_format(pixel_format);
  const int32_t bytes_per_pixel = type == 1 ? 4 : (type == 2 ? 8 : (type == 3 ? 16 : 0));
  const int32_t width = options
      ? (kSyntheticCompWidth + options->downsample_x - 1) / options->downsample_x : 8;
  const int32_t height = options
      ? (kSyntheticCompHeight + options->downsample_y - 1) / options->downsample_y : 4;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * bytes_per_pixel;
  if (!bytes_per_pixel || bytes > kMaxReceiptBytes) return 4;
  std::unique_ptr<ReceiptDraft> receipt;
  try {
    receipt = std::make_unique<ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
  } catch (const std::bad_alloc&) {
    return 4;
  }
  receipt->pixel_format = pixel_format;
  if (options) {
    receipt->has_render_options = true;
    receipt->render_options = *options;
    const AegpRect full{0, 0, kSyntheticCompWidth, kSyntheticCompHeight};
    const bool full_roi = options->roi.left == 0 && options->roi.top == 0 &&
        options->roi.right == 0 && options->roi.bottom == 0;
    const AegpRect clipped = full_roi ? full : AegpRect{
        (std::max)(0, options->roi.left), (std::max)(0, options->roi.top),
        (std::min)(kSyntheticCompWidth, options->roi.right),
        (std::min)(kSyntheticCompHeight, options->roi.bottom)};
    const auto ceil_div = [](int32_t value, int16_t divisor) {
      return value <= 0 ? 0 : (value + divisor - 1) / divisor;
    };
    receipt->rendered_region = {
        ceil_div(clipped.left, options->downsample_x),
        ceil_div(clipped.top, options->downsample_y),
        ceil_div(clipped.right, options->downsample_x),
        ceil_div(clipped.bottom, options->downsample_y)};
  } else {
    receipt->rendered_region = {0, 0, width, height};
  }
  receipt->world.world_flags = type == 1 ? 0 : 1;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = width * bytes_per_pixel;
  receipt->world.width = width;
  receipt->world.height = height;
  receipt->world.extent_hint = {0, 0, width, height};
  receipt->world.pix_aspect_ratio = {1, 1};
  if (options) populate_synthetic_item_pixels(*receipt, type, width, height);
  return aexcompat::render_receipts::register_receipt(
      std::move(receipt), output);
}

int32_t publish_external_cached_receipt(const AegpRenderOptionsValue& options,
                                        void** output, bool* cache_hit) {
  if (output) *output = nullptr;
  if (cache_hit) *cache_hit = false;
  if (!output || !cache_hit || g_render_timestamp_exhausted.load()) return 4;
  const uint32_t current_timestamp = g_render_project_timestamp.load();
  AegpRenderOptionsValue cached_options{};
  AegpRect cached_region{};
  std::shared_ptr<PlatformWorldBacking> backing;
  {
    std::lock_guard<std::mutex> lock(g_world_mutex);
    const auto cached = std::find_if(g_external_render_cache.begin(),
        g_external_render_cache.end(), [&](const auto& frame) {
          return frame.timestamp == current_timestamp &&
              same_render_options(frame.options, options);
        });
    if (cached == g_external_render_cache.end()) return 0;
    cached_options = cached->options;
    cached_region = cached->rendered_region;
    backing = cached->backing;
  }
  if (!backing || !backing->world.data || backing->world.width <= 0 ||
      backing->world.height <= 0 || backing->world.rowbytes <= 0) return 4;
  const int32_t type = aegp_world_type_from_format(backing->pixel_format);
  const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 : (type == 3 ? 16 : 0));
  const uint64_t tight_rowbytes = static_cast<uint64_t>(backing->world.width) * pixel_bytes;
  const uint64_t bytes = tight_rowbytes * backing->world.height;
  if (!pixel_bytes || backing->world.rowbytes < tight_rowbytes ||
      bytes == 0 || bytes > kMaxReceiptBytes) return 4;
  std::unique_ptr<ReceiptDraft> receipt;
  try {
    receipt = std::make_unique<ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
    std::lock_guard<std::mutex> pixels_lock(backing->pixels_mutex);
    for (int32_t y = 0; y < backing->world.height; ++y) {
      std::memcpy(receipt->pixels.data() + static_cast<std::size_t>(y) * tight_rowbytes,
          static_cast<const std::byte*>(backing->world.data) +
              static_cast<std::size_t>(y) * backing->world.rowbytes,
          static_cast<std::size_t>(tight_rowbytes));
    }
  } catch (...) { return 4; }
  if (current_timestamp != g_render_project_timestamp.load() ||
      g_render_timestamp_exhausted.load()) return 4;
  receipt->pixel_format = backing->pixel_format;
  receipt->has_render_options = true;
  receipt->render_options = cached_options;
  receipt->rendered_region = cached_region;
  receipt->render_timestamp = current_timestamp;
  receipt->world.world_flags = type == 1 ? 0 : 1;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = static_cast<int32_t>(tight_rowbytes);
  receipt->world.width = backing->world.width;
  receipt->world.height = backing->world.height;
  receipt->world.extent_hint = {0, 0, backing->world.width, backing->world.height};
  receipt->world.pix_aspect_ratio = {1, 1};
  std::lock_guard<std::mutex> lock(g_world_mutex);
  if (current_timestamp != g_render_project_timestamp.load() ||
      g_render_timestamp_exhausted.load()) return 4;
  if (aexcompat::render_receipts::register_receipt(
          std::move(receipt), output) != 0) return 4;
  *cache_hit = true;
  return 0;
}

int32_t item_world_pixel_bytes(int32_t pixel_format) {
  if (pixel_format == kPixelFormatArgb32) return 4;
  if (pixel_format == kPixelFormatArgb64) return 8;
  if (pixel_format == kPixelFormatArgb128) return 16;
  return 0;
}

bool same_stage_rational(const AegpTime& left, const AegpTime& right) {
  return left.scale != 0 && right.scale != 0 &&
      static_cast<int64_t>(left.value) * right.scale ==
      static_cast<int64_t>(right.value) * left.scale;
}

bool publish_staged_item_world(void* item, AegpTime time, AegpTime time_step,
    int8_t quality, uint8_t guide_layers, int32_t pixel_format, int32_t width,
    int32_t height, int32_t rowbytes, const void* pixels) {
  const int32_t pixel_bytes = item_world_pixel_bytes(pixel_format);
  const uint64_t tight_rowbytes = static_cast<uint64_t>(width) * pixel_bytes;
  const uint64_t tight_bytes = tight_rowbytes * height;
  if (!item || time.scale == 0 || time_step.scale == 0 || time_step.value <= 0 ||
      quality < 0 || quality > 1 || guide_layers > 1 || !pixel_bytes || width <= 0 ||
      height <= 0 || width > 4096 || height > 4096 || rowbytes < tight_rowbytes ||
      !pixels || tight_bytes == 0 || tight_bytes > kMaxReceiptBytes) return false;
  std::shared_ptr<std::vector<std::byte>> backing;
  try {
    backing = std::make_shared<std::vector<std::byte>>(static_cast<std::size_t>(tight_bytes));
    for (int32_t y = 0; y < height; ++y)
      std::memcpy(backing->data() + static_cast<std::size_t>(y) * tight_rowbytes,
          static_cast<const std::byte*>(pixels) + static_cast<std::size_t>(y) * rowbytes,
          static_cast<std::size_t>(tight_rowbytes));
  } catch (...) { return false; }
  const uint32_t project_generation = g_render_project_timestamp.load();
  const uint64_t stage_generation = g_staged_item_generation.fetch_add(1);
  if (project_generation == 0 || stage_generation == 0) return false;
  StagedItemWorld stage{item, time, time_step, quality, guide_layers, pixel_format,
      width, height, static_cast<int32_t>(tight_rowbytes), project_generation,
      stage_generation, std::move(backing)};
  std::lock_guard<std::mutex> lock(g_staged_item_world_mutex);
  auto same_key = [&](const StagedItemWorld& value) {
    return value.item == item && same_stage_rational(value.time, time) &&
        same_stage_rational(value.time_step, time_step) && value.quality == quality &&
        value.guide_layers == guide_layers && value.pixel_format == pixel_format &&
        value.width == width && value.height == height && value.rowbytes == tight_rowbytes &&
        value.project_generation == project_generation;
  };
  const auto existing = std::find_if(g_staged_item_worlds.begin(),
      g_staged_item_worlds.end(), same_key);
  if (existing != g_staged_item_worlds.end()) *existing = std::move(stage);
  else if (g_staged_item_worlds.size() == kMaxStagedItemWorlds) {
    const auto oldest = std::min_element(g_staged_item_worlds.begin(),
        g_staged_item_worlds.end(), [](const auto& left, const auto& right) {
          return left.stage_generation < right.stage_generation;
        });
    *oldest = std::move(stage);
  } else {
    try { g_staged_item_worlds.push_back(std::move(stage)); }
    catch (...) { return false; }
  }
  ++g_staged_item_worlds_published;
  return true;
}

bool snapshot_staged_item_world(const AegpRenderOptionsValue& options,
                                StagedItemWorld& stage) {
  const int32_t pixel_format = options.world_type == 1 ? kPixelFormatArgb32 :
      (options.world_type == 2 ? kPixelFormatArgb64 :
       (options.world_type == 3 ? kPixelFormatArgb128 : 0));
  const uint32_t generation = g_render_project_timestamp.load();
  std::lock_guard<std::mutex> lock(g_staged_item_world_mutex);
  const auto found = std::find_if(g_staged_item_worlds.rbegin(),
      g_staged_item_worlds.rend(), [&](const StagedItemWorld& value) {
        return value.item == options.item && same_stage_rational(value.time, options.time) &&
            same_stage_rational(value.time_step, options.time_step) &&
            value.quality == options.render_quality &&
            value.guide_layers == options.render_guide_layers &&
            value.pixel_format == pixel_format && value.project_generation == generation;
      });
  if (found == g_staged_item_worlds.rend() || !found->backing) {
    ++g_staged_item_world_cache_misses;
    return false;
  }
  stage = *found;
  ++g_staged_item_world_cache_hits;
  return true;
}

bool item_stack_contains(const ItemRenderStackKey& key) {
  return std::any_of(g_item_render_stack.begin(), g_item_render_stack.end(),
      [&](const auto& active) {
        return active.item == key.item && active.project_generation == key.project_generation &&
            same_stage_rational(active.time, key.time);
      });
}

struct ItemRenderStackScope {
  bool entered{};
  explicit ItemRenderStackScope(ItemRenderStackKey key) {
    if (item_stack_contains(key)) return;
    g_item_render_stack.push_back(key);
    entered = true;
  }
  ~ItemRenderStackScope() { if (entered) g_item_render_stack.pop_back(); }
};

float read_staged_channel(const std::byte* pixel, int32_t pixel_bytes, int channel) {
  if (pixel_bytes == 4)
    return static_cast<float>(reinterpret_cast<const uint8_t*>(pixel)[channel]) / 255.0f;
  if (pixel_bytes == 8)
    return static_cast<float>(reinterpret_cast<const uint16_t*>(pixel)[channel]) / 32768.0f;
  float value = 0.0f;
  std::memcpy(&value, pixel + channel * sizeof(float), sizeof(value));
  return std::isfinite(value) ? value : 0.0f;
}

void write_staged_channel(std::byte* pixel, int32_t pixel_bytes, int channel, float value) {
  value = std::clamp(value, 0.0f, 1.0f);
  if (pixel_bytes == 4)
    reinterpret_cast<uint8_t*>(pixel)[channel] =
        static_cast<uint8_t>(std::lround(value * 255.0f));
  else if (pixel_bytes == 8)
    reinterpret_cast<uint16_t*>(pixel)[channel] =
        static_cast<uint16_t>(std::lround(value * 32768.0f));
  else
    std::memcpy(pixel + channel * sizeof(float), &value, sizeof(value));
}

int32_t transform_staged_item_world(const StagedItemWorld& stage,
    const AegpRenderOptionsValue& options, std::unique_ptr<ReceiptDraft>& receipt) {
  const int32_t pixel_bytes = item_world_pixel_bytes(stage.pixel_format);
  if (!stage.backing || !pixel_bytes || options.downsample_x <= 0 ||
      options.downsample_y <= 0 || options.field < 0 || options.field > 2 ||
      options.matte < 0 || options.matte > 2 || options.channel_order < 0 ||
      options.channel_order > 1) return 4;
  const uint64_t source_bytes = static_cast<uint64_t>(stage.rowbytes) * stage.height;
  if (stage.width <= 0 || stage.height <= 0 || stage.rowbytes < stage.width * pixel_bytes ||
      source_bytes != stage.backing->size()) return 4;
  const int32_t width = (stage.width + options.downsample_x - 1) / options.downsample_x;
  const int32_t height = (stage.height + options.downsample_y - 1) / options.downsample_y;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if (bytes == 0 || bytes > kMaxReceiptBytes) return 4;
  try {
    receipt = std::make_unique<ReceiptDraft>();
    receipt->pixels.resize(static_cast<std::size_t>(bytes));
  } catch (...) { return 4; }
  receipt->staged_source_pin = stage.backing;
  const bool zero_roi = options.roi.left == 0 && options.roi.top == 0 &&
      options.roi.right == 0 && options.roi.bottom == 0;
  const AegpRect roi = zero_roi ? AegpRect{0, 0, stage.width, stage.height} :
      AegpRect{(std::max)(0, options.roi.left), (std::max)(0, options.roi.top),
          (std::min)(stage.width, options.roi.right),
          (std::min)(stage.height, options.roi.bottom)};
  const float background = 0x3030 / 65535.0f;
  for (int32_t y = 0; y < height; ++y) {
    const int32_t source_y = y * options.downsample_y;
    for (int32_t x = 0; x < width; ++x) {
      const int32_t source_x = x * options.downsample_x;
      const bool in_roi = source_x >= roi.left && source_x < roi.right &&
          source_y >= roi.top && source_y < roi.bottom;
      const bool in_field = options.field == 0 ||
          (options.field == 1 && (source_y & 1) == 0) ||
          (options.field == 2 && (source_y & 1) != 0);
      if (!in_roi || !in_field) continue;
      const std::byte* source = stage.backing->data() +
          static_cast<std::size_t>(source_y) * stage.rowbytes +
          static_cast<std::size_t>(source_x) * pixel_bytes;
      std::array<float, 4> argb{};
      for (int c = 0; c < 4; ++c) argb[c] = read_staged_channel(source, pixel_bytes, c);
      if (options.matte == 1)
        for (int c = 1; c < 4; ++c) argb[c] *= argb[0];
      else if (options.matte == 2)
        for (int c = 1; c < 4; ++c)
          argb[c] = argb[c] * argb[0] + background * (1.0f - argb[0]);
      const std::array<float, 4> packed = options.channel_order == 0 ? argb :
          std::array<float, 4>{{argb[3], argb[2], argb[1], argb[0]}};
      std::byte* destination = receipt->pixels.data() +
          (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
      for (int c = 0; c < 4; ++c)
        write_staged_channel(destination, pixel_bytes, c, packed[c]);
    }
  }
  const auto ceil_div = [](int32_t value, int32_t divisor) {
    return value <= 0 ? 0 : (value + divisor - 1) / divisor;
  };
  receipt->pixel_format = stage.pixel_format;
  receipt->has_render_options = true;
  receipt->render_options = options;
  receipt->rendered_region = {ceil_div(roi.left, options.downsample_x),
      ceil_div(roi.top, options.downsample_y), ceil_div(roi.right, options.downsample_x),
      ceil_div(roi.bottom, options.downsample_y)};
  receipt->render_timestamp = stage.project_generation;
  receipt->world.data = receipt->pixels.data();
  receipt->world.rowbytes = width * pixel_bytes;
  receipt->world.world_flags = pixel_bytes == 4 ? 0 : 1;
  receipt->world.width = width;
  receipt->world.height = height;
  receipt->world.extent_hint = {0, 0, width, height};
  receipt->world.pix_aspect_ratio = {1, 1};
  return 0;
}

int32_t register_item_receipt(std::unique_ptr<ReceiptDraft> receipt, void** output) {
  return aexcompat::render_receipts::register_receipt(
      std::move(receipt), output);
}

int32_t publish_item_receipt(void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (!receipt) return 4;
  AegpRenderOptionsValue snapshot{};
  if (!snapshot_render_options(options, snapshot)) return 4;
  const ItemRenderStackKey stack_key{
      snapshot.item, snapshot.time, g_render_project_timestamp.load()};
  if (item_stack_contains(stack_key)) {
    ++g_staged_item_world_cycles_rejected;
    return 4;
  }
  ItemRenderStackScope stack_scope(stack_key);
  if (!stack_scope.entered) return 4;
  StagedItemWorld stage{};
  if (snapshot_staged_item_world(snapshot, stage)) {
    std::unique_ptr<ReceiptDraft> staged_receipt;
    if (transform_staged_item_world(stage, snapshot, staged_receipt) != 0) return 4;
    return register_item_receipt(std::move(staged_receipt), receipt);
  }
  if (!g_synthetic_receipt_test_mode || snapshot.matte == 2) return 4;
  const int32_t pixel_format = snapshot.world_type == 1 ? kPixelFormatArgb32 :
      (snapshot.world_type == 2 ? kPixelFormatArgb64 :
       (snapshot.world_type == 3 ? kPixelFormatArgb128 : 0));
  return pixel_format ? publish_async_receipt(pixel_format, receipt, &snapshot) : 4;
}

int32_t publish_loaded_layer_receipt_from_context(
    const LoadedEffectReceiptContext& context,
    const AegpLayerRenderOptionsValue& options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (!receipt) return 4;
  const int32_t pixel_format = options.world_type == 1 ? kPixelFormatArgb32 :
      (options.world_type == 2 ? kPixelFormatArgb64 :
       (options.world_type == 3 ? kPixelFormatArgb128 : 0));
  const int32_t pixel_bytes = options.world_type == 1 ? 4 :
      (options.world_type == 2 ? 8 : (options.world_type == 3 ? 16 : 0));
  const bool current_time = options.time.scale != 0 && context.time_scale != 0 &&
      static_cast<int64_t>(options.time.value) * context.time_scale ==
      static_cast<int64_t>(context.current_time) * options.time.scale;
  const bool wants_downstream =
      options.effect_boundary == AegpLayerEffectBoundary::downstream;
  const bool wants_all_staged = options.effect_boundary == AegpLayerEffectBoundary::all &&
      context.all_effects_finalized;
  const auto* selected_pixels = wants_downstream ? context.downstream_argb :
      (wants_all_staged ? context.all_effects_argb : context.source_argb);
  const int32_t selected_width = wants_downstream ? context.downstream_width :
      (wants_all_staged ? context.all_effects_width : context.source_width);
  const int32_t selected_height = wants_downstream ? context.downstream_height :
      (wants_all_staged ? context.all_effects_height : context.source_height);
  const int32_t source_pixel_bytes = wants_downstream ? context.downstream_pixel_bytes :
      (wants_all_staged ? context.all_effects_pixel_bytes : context.pixel_bytes);
  if (!context.entry || !selected_pixels || pixel_format == 0 ||
      options.time_step.scale == 0 || options.time_step.value <= 0 || !current_time ||
      options.downsample_x <= 0 || options.downsample_y <= 0 || options.matte == 2 ||
      !layer_effect_boundary_is_live(options) ||
      (wants_downstream && !context.downstream_finalized) ||
      selected_width <= 0 || selected_height <= 0 ||
      selected_width > 4096 || selected_height > 4096) return 4;
  const uint64_t source_bytes = static_cast<uint64_t>(selected_width) *
      selected_height * source_pixel_bytes;
  const int32_t width = (selected_width + options.downsample_x - 1) /
      options.downsample_x;
  const int32_t height = (selected_height + options.downsample_y - 1) /
      options.downsample_y;
  const uint64_t bytes = static_cast<uint64_t>(width) * height * pixel_bytes;
  if ((source_pixel_bytes != 4 && source_pixel_bytes != 8 && source_pixel_bytes != 16) ||
      selected_pixels->size() != source_bytes || bytes == 0 ||
      bytes > kMaxReceiptBytes) return 4;
  std::unique_ptr<ReceiptDraft> loaded_receipt;
  try {
    loaded_receipt = std::make_unique<ReceiptDraft>();
    loaded_receipt->pixels.resize(static_cast<std::size_t>(bytes));
    for (int32_t y = 0; y < height; ++y) {
      const int32_t source_y = y * options.downsample_y;
      for (int32_t x = 0; x < width; ++x) {
        const int32_t source_x = x * options.downsample_x;
        const unsigned char* source = selected_pixels->data() +
            (static_cast<std::size_t>(source_y) * selected_width + source_x) *
                source_pixel_bytes;
        std::array<float, 4> channels{};
        if (source_pixel_bytes == 4) {
          for (int c = 0; c < 4; ++c) channels[c] = source[c] / 255.0f;
        } else if (source_pixel_bytes == 8) {
          const auto* values = reinterpret_cast<const uint16_t*>(source);
          for (int c = 0; c < 4; ++c) channels[c] = values[c] / 65535.0f;
        } else {
          std::memcpy(channels.data(), source, sizeof(channels));
        }
        if (options.matte == 1)
          for (int c = 1; c < 4; ++c) channels[c] *= channels[0];
        unsigned char* destination = reinterpret_cast<unsigned char*>(
            loaded_receipt->pixels.data()) +
            (static_cast<std::size_t>(y) * width + x) * pixel_bytes;
        if (pixel_bytes == 4) {
          for (int c = 0; c < 4; ++c) destination[c] = static_cast<unsigned char>(
              std::clamp(channels[c], 0.0f, 1.0f) * 255.0f + 0.5f);
        } else if (pixel_bytes == 8) {
          std::array<uint16_t, 4> values{};
          for (int c = 0; c < 4; ++c) values[c] = static_cast<uint16_t>(
              std::clamp(channels[c], 0.0f, 1.0f) * 65535.0f + 0.5f);
          std::memcpy(destination, values.data(), sizeof(values));
        } else {
          std::memcpy(destination, channels.data(), sizeof(channels));
        }
      }
    }
  } catch (const std::bad_alloc&) {
    return 4;
  }
  loaded_receipt->pixel_format = pixel_format;
  loaded_receipt->rendered_region = {0, 0, width, height};
  loaded_receipt->world.data = loaded_receipt->pixels.data();
  loaded_receipt->world.rowbytes = width * pixel_bytes;
  loaded_receipt->world.world_flags = pixel_bytes == 4 ? 0 : 1;
  loaded_receipt->world.width = width;
  loaded_receipt->world.height = height;
  loaded_receipt->world.extent_hint = {0, 0, width, height};
  loaded_receipt->world.pix_aspect_ratio = {1, 1};
  return aexcompat::render_receipts::register_receipt(
      std::move(loaded_receipt), receipt);
}
int32_t publish_loaded_layer_receipt(
    const AegpLayerRenderOptionsValue& options, void** receipt) {
  if (is_render_worker())
    return publish_loaded_layer_receipt_from_context(
        g_loaded_effect_receipt_context, options, receipt);
  if (receipt) *receipt = nullptr;
  return 4;
}
int32_t __cdecl checkout_item_frame_async(
    void* manager, uint32_t purpose, void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (manager != &g_async_manager || purpose == 0) return 4;
  return publish_item_receipt(options, receipt);
}
int32_t __cdecl checkout_layer_frame_async(
    void* manager, uint32_t purpose, void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (manager != &g_async_manager || purpose == 0 || !receipt ||
      !snapshot_layer_render_options(options, snapshot)) return 4;
  if (is_render_worker() && g_loaded_effect_receipt_context.entry)
    return publish_loaded_layer_receipt(snapshot, receipt);
  const int32_t pixel_format = snapshot.world_type == 1 ? kPixelFormatArgb32 :
      (snapshot.world_type == 2 ? kPixelFormatArgb64 : kPixelFormatArgb128);
  return publish_async_receipt(pixel_format, receipt);
}
int32_t __cdecl get_receipt_world(void* receipt, void*** world) {
  return aexcompat::render_receipts::get_world(receipt, world);
}
int32_t __cdecl checkin_frame(void* receipt) {
  return aexcompat::render_receipts::checkin(receipt);
}

bool checkin_frame_if_live(void* receipt) {
  return aexcompat::render_receipts::checkin_if_live(receipt);
}

int32_t __cdecl render_checkout_frame_reject(
    void* options, AegpRenderCancelV1 check_cancel, void* cancel_refcon, void** out) {
  if (out) *out = nullptr;
  if (!out) return 4;
  if (check_cancel) {
    uint8_t cancelled = 0;
    const int32_t cancel_error = check_cancel(cancel_refcon, &cancelled);
    if (cancel_error != 0) return cancel_error;
    if (cancelled != 0) return 4;
  }
  AegpRenderOptionsValue snapshot{};
  if (!snapshot_render_options(options, snapshot)) return 4;
  bool cache_hit = false;
  const int32_t cache_error = publish_external_cached_receipt(snapshot, out, &cache_hit);
  if (cache_error != 0 || cache_hit) return cache_error;
  return publish_item_receipt(options, out);
}
int32_t __cdecl render_checkout_layer_reject(
    void* options, uint8_t, void* check_cancel_raw, void* cancel_refcon, void** out) {
  if (out) *out = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (!out || !snapshot_layer_render_options(options, snapshot)) return 4;
  const auto check_cancel = reinterpret_cast<AegpRenderCancelV1>(check_cancel_raw);
  if (check_cancel) {
    uint8_t cancelled = 0;
    const int32_t cancel_error = check_cancel(cancel_refcon, &cancelled);
    if (cancel_error != 0) return cancel_error;
    if (cancelled != 0) return 4;
  }
  return publish_loaded_layer_receipt(snapshot, out);
}
int32_t __cdecl render_checkout_layer_v5(
    void* options, AegpRenderCancelV1 check_cancel, void* cancel_refcon, void** out) {
  if (out) *out = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (!out || !snapshot_layer_render_options(options, snapshot)) return 4;
  if (check_cancel) {
    uint8_t cancelled = 0;
    const int32_t cancel_error = check_cancel(cancel_refcon, &cancelled);
    if (cancel_error != 0) return cancel_error;
    if (cancelled != 0) return 4;
  }
  return publish_loaded_layer_receipt(snapshot, out);
}
int32_t invoke_async_layer_callback_seh(AegpAsyncFrameReadyCallback callback,
    uint64_t request_id, uint8_t canceled, int32_t error, void* receipt,
    void* refcon, int32_t* callback_error, uint32_t* out_exception_code) {
  if (!callback || !callback_error || !out_exception_code) return 4;
  *callback_error = 4;
  *out_exception_code = 0;
  __try {
    *callback_error = callback(request_id, canceled, error, receipt, refcon);
    return 0;
  } __except(EXCEPTION_EXECUTE_HANDLER) {
    *out_exception_code = GetExceptionCode();
    return 4;
  }
}

int32_t __cdecl render_checkout_layer_async_reject(
    void* options, AegpAsyncFrameReadyCallback callback, void* refcon, uint64_t* request_id) {
  if (request_id) *request_id = 0;
  if (is_render_worker()) {
  const auto& context = g_loaded_effect_receipt_context;
  AegpLayerRenderOptionsValue snapshot{};
  if (!snapshot_layer_render_options(options, snapshot)) return 4;
  const bool wants_downstream =
      snapshot.effect_boundary == AegpLayerEffectBoundary::downstream;
  const bool wants_all_staged = snapshot.effect_boundary == AegpLayerEffectBoundary::all &&
      context.all_effects_finalized;
  const auto* selected_pixels = wants_downstream ? context.downstream_argb :
      (wants_all_staged ? context.all_effects_argb : context.source_argb);
  const int32_t selected_width = wants_downstream ? context.downstream_width :
      (wants_all_staged ? context.all_effects_width : context.source_width);
  const int32_t selected_height = wants_downstream ? context.downstream_height :
      (wants_all_staged ? context.all_effects_height : context.source_height);
  const int32_t selected_pixel_bytes = wants_downstream ? context.downstream_pixel_bytes :
      (wants_all_staged ? context.all_effects_pixel_bytes : context.pixel_bytes);
  const int32_t pixel_bytes = snapshot.world_type == 1 ? 4 :
      (snapshot.world_type == 2 ? 8 : (snapshot.world_type == 3 ? 16 : 0));
  if (!request_id || !callback || !context.entry || !selected_pixels ||
      (wants_downstream && !context.downstream_finalized) ||
      !pixel_bytes || selected_width <= 0 || selected_height <= 0) return 4;
  const uint64_t source_bytes = static_cast<uint64_t>(selected_width) *
      selected_height * selected_pixel_bytes;
  const uint64_t output_bytes = static_cast<uint64_t>(
      (selected_width + snapshot.downsample_x - 1) / snapshot.downsample_x) *
      ((selected_height + snapshot.downsample_y - 1) / snapshot.downsample_y) *
      pixel_bytes;
  const uint64_t bytes = (std::max)(source_bytes, output_bytes);
  if (source_bytes == 0 || bytes > kMaxReceiptBytes ||
      selected_pixels->size() != source_bytes) return 4;
  std::shared_ptr<AsyncLayerRequest> request;
  try {
    request = std::make_shared<AsyncLayerRequest>();
    request->entry = context.entry;
    request->callback = callback;
    request->refcon = refcon;
    request->world_type = snapshot.world_type;
    request->options = snapshot;
    request->pixel_bytes = selected_pixel_bytes;
    request->width = selected_width;
    request->height = selected_height;
    request->current_time = context.current_time;
    request->time_scale = context.time_scale;
    request->pixels = *selected_pixels;
  } catch (const std::bad_alloc&) {
    return 4;
  }
  std::lock_guard<std::mutex> lock(g_async_layer_request_mutex);
  if (!g_async_layer_accepting || g_async_layer_requests.size() >= 32 ||
      g_async_layer_reserved_bytes > kMaxReceiptBytes - bytes) return 4;
  request->id = g_next_async_layer_request_id++;
  if (request->id == 0) return 4;
  g_async_layer_reserved_bytes += bytes;
  try {
    g_async_layer_requests.emplace(request->id, request);
    *request_id = request->id;
    g_async_layer_threads.emplace_back([request, bytes] {
      if (g_async_layer_cancel_test_gate) {
        std::unique_lock<std::mutex> gate_lock(request->gate_mutex);
        request->gate_changed.wait_for(gate_lock, std::chrono::seconds(5),
            [&] { return request->state.load() != 0; });
      }
      int32_t expected = 0;
      const bool completion_won = request->state.compare_exchange_strong(expected, 1);
      void* receipt = nullptr;
      int32_t error = 0;
      uint8_t canceled = 0;
      if (completion_won) {
        LoadedEffectReceiptContext context{};
        context.entry = request->entry;
        context.pixel_bytes = request->pixel_bytes;
        context.source_argb = &request->pixels;
        context.source_width = request->width;
        context.source_height = request->height;
        context.current_time = request->current_time;
        context.time_scale = request->time_scale;
        if (request->options.effect_boundary == AegpLayerEffectBoundary::downstream) {
          context.downstream_argb = &request->pixels;
          context.downstream_width = request->width;
          context.downstream_height = request->height;
          context.downstream_pixel_bytes = request->pixel_bytes;
          context.downstream_finalized = true;
        } else if (request->options.effect_boundary == AegpLayerEffectBoundary::all) {
          context.all_effects_argb = &request->pixels;
          context.all_effects_width = request->width;
          context.all_effects_height = request->height;
          context.all_effects_pixel_bytes = request->pixel_bytes;
          context.all_effects_finalized = true;
        }
        error = publish_loaded_layer_receipt_from_context(
            context, request->options, &receipt);
      } else {
        canceled = 1;
      }
      int32_t callback_error = 0;
      uint32_t callback_exception = 0;
      const int32_t invoke_error = invoke_async_layer_callback_seh(
          request->callback, request->id, canceled, error, receipt, request->refcon,
          &callback_error, &callback_exception);
      if (invoke_error != 0 || callback_error != 0) checkin_frame_if_live(receipt);
      request->state.store(3);
      std::lock_guard<std::mutex> completed_lock(g_async_layer_request_mutex);
      g_async_layer_reserved_bytes -= bytes;
      if (canceled) ++g_async_layer_requests_canceled;
      else ++g_async_layer_requests_completed;
      if (invoke_error != 0 || callback_error != 0) ++g_async_layer_callback_failures;
      if (callback_exception != 0) ++g_async_layer_callback_exceptions;
      g_async_layer_requests.erase(request->id);
    });
  } catch (const std::system_error&) {
    g_async_layer_requests.erase(request->id);
    g_async_layer_reserved_bytes -= bytes;
    *request_id = 0;
    return 4;
  } catch (const std::bad_alloc&) {
    g_async_layer_requests.erase(request->id);
    g_async_layer_reserved_bytes -= bytes;
    *request_id = 0;
    return 4;
  }
  ++g_async_layer_requests_created;
  return 0;
  } else {
  return 4;
  }
}
int32_t __cdecl render_cancel_async_reject(uint64_t request_id) {
  if (is_render_worker()) {
  std::lock_guard<std::mutex> lock(g_async_layer_request_mutex);
  const auto found = g_async_layer_requests.find(request_id);
  if (found == g_async_layer_requests.end()) return 4;
  int32_t expected = 0;
  if (!found->second->state.compare_exchange_strong(expected, 2)) return 4;
  found->second->gate_changed.notify_one();
  return 0;
  } else {
  return 4;
  }
}

void drain_async_layer_requests() {
  std::vector<std::thread> threads;
  {
    std::lock_guard<std::mutex> lock(g_async_layer_request_mutex);
    g_async_layer_accepting = false;
    threads.swap(g_async_layer_threads);
  }
  for (auto& thread : threads)
    if (thread.joinable()) thread.join();
}
bool async_layer_requests_balanced() {
  std::lock_guard<std::mutex> lock(g_async_layer_request_mutex);
  return g_async_layer_requests.empty() && g_async_layer_reserved_bytes == 0 &&
      g_async_layer_requests_created ==
          g_async_layer_requests_completed + g_async_layer_requests_canceled;
}
int32_t __cdecl render_get_region_reject(void* receipt, void* region) {
  if (!receipt || !region) return 4;
  ReceiptSnapshot snapshot{};
  if (!aexcompat::render_receipts::snapshot(receipt, snapshot)) return 4;
  std::memcpy(region, &snapshot.rendered_region, sizeof(AegpRect));
  return 0;
}
int32_t __cdecl render_sufficient_reject(void* rendered, void* proposed, uint8_t* out) {
  if (out) *out = 0;
  AegpRenderOptionsValue first{}, second{};
  if (!out || !snapshot_render_options(rendered, first) ||
      !snapshot_render_options(proposed, second)) return 4;
  const auto roi = [](const AegpRenderOptionsValue& options) {
    return options.roi.left == 0 && options.roi.top == 0 &&
        options.roi.right == 0 && options.roi.bottom == 0
        ? AegpRect{0, 0, kSyntheticCompWidth, kSyntheticCompHeight} : options.roi;
  };
  const AegpRect rendered_roi = roi(first), proposed_roi = roi(second);
  const auto same_rational = [](const AegpTime& left, const AegpTime& right) {
    return static_cast<int64_t>(left.value) * right.scale ==
        static_cast<int64_t>(right.value) * left.scale;
  };
  const bool same_time = same_rational(first.time, second.time);
  const bool same_step = same_rational(first.time_step, second.time_step);
  const bool contains = rendered_roi.left <= proposed_roi.left &&
      rendered_roi.top <= proposed_roi.top && rendered_roi.right >= proposed_roi.right &&
      rendered_roi.bottom >= proposed_roi.bottom;
  *out = first.item == second.item && same_time && same_step &&
      first.field == second.field && first.world_type == second.world_type &&
      first.downsample_x == second.downsample_x &&
      first.downsample_y == second.downsample_y && first.matte == second.matte &&
      first.channel_order == second.channel_order &&
      first.render_guide_layers == second.render_guide_layers &&
      first.render_quality == second.render_quality && contains;
  return 0;
}
int32_t __cdecl render_sound_reject(void*, const void*, const void*, const void*, void*, void*, void** out) {
  if (out) *out = nullptr; return 4;
}
void store_render_timestamp(AegpTimeStamp& timestamp, uint32_t value) {
  std::memcpy(timestamp.bytes.data(), &value, sizeof(value));
}
bool read_render_timestamp(const void* timestamp, uint32_t& value) {
  if (!timestamp) return false;
  std::memcpy(&value, timestamp, sizeof(value));
  return value != 0;
}
int32_t __cdecl render_timestamp_reject(void* output) {
  if (!output || g_render_timestamp_exhausted.load()) return 4;
  AegpTimeStamp timestamp{};
  store_render_timestamp(timestamp, g_render_project_timestamp.load());
  std::memcpy(output, &timestamp, sizeof(timestamp));
  return 0;
}
int32_t __cdecl render_changed_reject(void* item, const void* start_raw,
    const void* duration_raw, const void* timestamp, uint8_t* out) {
  if (out) *out = 0;
  uint32_t observed = 0;
  const auto* start = static_cast<const AegpTime*>(start_raw);
  const auto* duration = static_cast<const AegpTime*>(duration_raw);
  if (!out || item != aegp_comp_item_handle() || !start || !duration ||
      start->scale == 0 || duration->scale == 0 || duration->value < 0 ||
      !read_render_timestamp(timestamp, observed)) return 4;
  if (g_render_timestamp_exhausted.load()) { *out = 1; return 0; }
  *out = observed != g_render_project_timestamp.load();
  return 0;
}
int32_t __cdecl render_worthwhile_reject(void* options, const void* timestamp, uint8_t* out) {
  if (out) *out = 0;
  uint32_t observed = 0;
  AegpRenderOptionsValue snapshot{};
  if (!out || !snapshot_render_options(options, snapshot) ||
      !read_render_timestamp(timestamp, observed)) return 4;
  if (g_render_timestamp_exhausted.load()) return 0;
  const bool timestamp_current = observed == g_render_project_timestamp.load();
  if (!timestamp_current) return 0;
  std::lock_guard<std::mutex> lock(g_world_mutex);
  *out = std::none_of(g_external_render_cache.begin(), g_external_render_cache.end(),
      [&](const auto& frame) {
        return frame.timestamp == observed && same_render_options(frame.options, snapshot);
      });
  return 0;
}
int32_t __cdecl render_checkin_rendered(void* options, const void* timestamp,
                                        uint32_t ticks_to_render, void* image) {
  AegpRenderOptionsValue snapshot{};
  uint32_t observed = 0;
  const bool metadata_valid = snapshot_render_options(options, snapshot) &&
      read_render_timestamp(timestamp, observed);
  if (!metadata_valid || g_render_timestamp_exhausted.load()) return 4;
  std::lock_guard<std::mutex> lock(g_world_mutex);
  if (observed != g_render_project_timestamp.load() || g_render_timestamp_exhausted.load())
    return 4;
  std::shared_ptr<PlatformWorldBacking> backing;
  if (!aexcompat::world_registry::snapshot_platform_world(image, backing) ||
      aegp_world_type_from_format(backing->pixel_format) != snapshot.world_type)
    return 4;
  const int32_t width = backing->world.width;
  const int32_t height = backing->world.height;
  AegpRect rendered_region{0, 0, width, height};
  if (snapshot.roi.left != 0 || snapshot.roi.top != 0 ||
      snapshot.roi.right != 0 || snapshot.roi.bottom != 0) {
    const auto ceil_div = [](int32_t value, int32_t divisor) {
      return value <= 0 ? 0 : (value + divisor - 1) / divisor;
    };
    rendered_region = {
        (std::max)(0, snapshot.roi.left / snapshot.downsample_x),
        (std::max)(0, snapshot.roi.top / snapshot.downsample_y),
        (std::min)(width, ceil_div(snapshot.roi.right, snapshot.downsample_x)),
        (std::min)(height, ceil_div(snapshot.roi.bottom, snapshot.downsample_y))};
    if (rendered_region.right < rendered_region.left ||
        rendered_region.bottom < rendered_region.top) return 4;
  }
  auto cached = std::find_if(g_external_render_cache.begin(), g_external_render_cache.end(),
      [&](const auto& frame) {
        return frame.timestamp == observed && same_render_options(frame.options, snapshot);
      });
  if (cached == g_external_render_cache.end() &&
      g_external_render_cache.size() < kMaxExternalRenderCache) {
    try {
      g_external_render_cache.reserve(g_external_render_cache.size() + 1);
    } catch (...) { return 4; }
    cached = g_external_render_cache.end();
  }
  std::shared_ptr<PlatformWorldBacking> adopted;
  if (!aexcompat::world_registry::adopt_platform_world(image, adopted) ||
      adopted != backing) return 4;
  const ExternalRenderedFrame frame{
      snapshot, rendered_region, observed, ticks_to_render, std::move(adopted)};
  if (cached != g_external_render_cache.end()) {
    *cached = frame;
  } else if (g_external_render_cache.size() >= kMaxExternalRenderCache) {
    g_external_render_cache.front() = frame;
  } else {
    g_external_render_cache.push_back(frame);
  }
  ++g_external_frames_checked_in;
  return 0;
}
int32_t __cdecl render_guid_reject(void* receipt, void** out) {
  if (out) *out = nullptr;
  if (!receipt || !out) return 4;
  ReceiptSnapshot snapshot{};
  if (!aexcompat::render_receipts::snapshot(receipt, snapshot)) return 4;
  const auto& guid = snapshot.guid;
  if (new_aegp_mem_handle(1, "render receipt guid", static_cast<uint32_t>(guid.size()),
                          1, out) != 0) return 4;
  void* bytes = nullptr;
  if (lock_aegp_mem_handle(*out, &bytes) != 0 || !bytes) {
    free_aegp_mem_handle(*out); *out = nullptr; return 4;
  }
  std::memcpy(bytes, guid.data(), guid.size());
  return unlock_aegp_mem_handle(*out);
}

uint32_t color_settings_read_be32(const uint8_t* bytes) {
  return (static_cast<uint32_t>(bytes[0]) << 24) |
      (static_cast<uint32_t>(bytes[1]) << 16) |
      (static_cast<uint32_t>(bytes[2]) << 8) |
      static_cast<uint32_t>(bytes[3]);
}
void color_settings_write_be32(std::vector<uint8_t>& out, std::size_t offset, uint32_t value) {
  out[offset] = static_cast<uint8_t>(value >> 24);
  out[offset + 1] = static_cast<uint8_t>(value >> 16);
  out[offset + 2] = static_cast<uint8_t>(value >> 8);
  out[offset + 3] = static_cast<uint8_t>(value);
}
void color_settings_write_be16(std::vector<uint8_t>& out, std::size_t offset, uint16_t value) {
  out[offset] = static_cast<uint8_t>(value >> 8);
  out[offset + 1] = static_cast<uint8_t>(value);
}
void color_settings_write_tag(std::vector<uint8_t>& out, std::size_t tag_offset, const char tag[4],
                              std::size_t data_offset, std::size_t data_size) {
  std::memcpy(out.data() + tag_offset, tag, 4);
  color_settings_write_be32(out, tag_offset + 4, static_cast<uint32_t>(data_offset));
  color_settings_write_be32(out, tag_offset + 8, static_cast<uint32_t>(data_size));
}
std::vector<uint8_t> color_settings_build_minimal_rgb_icc(float gamma, const char* ascii_desc) {
  constexpr std::size_t kHeader = 128;
  constexpr std::size_t kTagCount = 9;
  constexpr std::size_t kTagTable = 4 + kTagCount * 12;
  const std::size_t wtpt = kHeader + kTagTable;
  const std::size_t rxyz = wtpt + 20;
  const std::size_t gxyz = rxyz + 20;
  const std::size_t bxyz = gxyz + 20;
  const std::size_t rtrc = bxyz + 20;
  const std::size_t gtrc = rtrc + 20;
  const std::size_t btrc = gtrc + 20;
  const std::size_t desc = btrc + 20;
  const std::size_t cprt = desc + 84;
  const std::size_t profile_size = cprt + 44;
  std::vector<uint8_t> profile(profile_size, 0);
  color_settings_write_be32(profile, 0, static_cast<uint32_t>(profile_size));
  profile[12] = 'm'; profile[13] = 'n'; profile[14] = 't'; profile[15] = 'r';
  profile[16] = 'R'; profile[17] = 'G'; profile[18] = 'B'; profile[19] = ' ';
  profile[36] = 'a'; profile[37] = 'c'; profile[38] = 's'; profile[39] = 'p';
  color_settings_write_be32(profile, 128, static_cast<uint32_t>(kTagCount));
  color_settings_write_tag(profile, 132, "wtpt", wtpt, 20);
  color_settings_write_tag(profile, 144, "rXYZ", rxyz, 20);
  color_settings_write_tag(profile, 156, "gXYZ", gxyz, 20);
  color_settings_write_tag(profile, 168, "bXYZ", bxyz, 20);
  color_settings_write_tag(profile, 180, "rTRC", rtrc, 20);
  color_settings_write_tag(profile, 192, "gTRC", gtrc, 20);
  color_settings_write_tag(profile, 204, "bTRC", btrc, 20);
  color_settings_write_tag(profile, 216, "desc", desc, 84);
  color_settings_write_tag(profile, 228, "cprt", cprt, 44);
  const auto write_xyz = [&](std::size_t offset, float x, float y, float z) {
    profile[offset + 0] = 'X'; profile[offset + 1] = 'Y'; profile[offset + 2] = 'Z'; profile[offset + 3] = ' ';
    const auto fixed = [](float value) {
      const double scaled = std::round(static_cast<double>(value) * 65536.0);
      const double bounded = (std::max)(static_cast<double>((std::numeric_limits<int32_t>::min)()),
                                       (std::min)(static_cast<double>((std::numeric_limits<int32_t>::max)()), scaled));
      return static_cast<uint32_t>(static_cast<int32_t>(bounded));
    };
    color_settings_write_be32(profile, offset + 8, fixed(x));
    color_settings_write_be32(profile, offset + 12, fixed(y));
    color_settings_write_be32(profile, offset + 16, fixed(z));
  };
  write_xyz(wtpt, 0.95047f, 1.0f, 1.08883f);
  write_xyz(rxyz, 0.43607f, 0.22249f, 0.01392f);
  write_xyz(gxyz, 0.38515f, 0.71687f, 0.09708f);
  write_xyz(bxyz, 0.14307f, 0.06061f, 0.71410f);
  const auto write_curv = [&](std::size_t offset) {
    profile[offset + 0] = 'c'; profile[offset + 1] = 'u'; profile[offset + 2] = 'r'; profile[offset + 3] = 'v';
    color_settings_write_be32(profile, offset + 8, 1);
    const uint16_t encoded = static_cast<uint16_t>(std::round(
        (std::min)(255.99609375f, (std::max)(0.0f, gamma)) * 256.0f));
    color_settings_write_be16(profile, offset + 12, encoded);
  };
  write_curv(rtrc); write_curv(gtrc); write_curv(btrc);
  profile[desc + 0] = 'd'; profile[desc + 1] = 'e'; profile[desc + 2] = 's'; profile[desc + 3] = 'c';
  color_settings_write_be32(profile, desc + 8, 67);
  color_settings_write_be32(profile, desc + 12, 0x6c6172656e);  // 'enUS' style marker
  const std::size_t text_len = std::strlen(ascii_desc);
  profile[desc + 16] = 't'; profile[desc + 17] = 'e'; profile[desc + 18] = 'x'; profile[desc + 19] = 't';
  color_settings_write_be32(profile, desc + 24, static_cast<uint32_t>(text_len + 1));
  std::memcpy(profile.data() + desc + 28, ascii_desc, text_len + 1);
  profile[cprt + 0] = 't'; profile[cprt + 1] = 'e'; profile[cprt + 2] = 'x'; profile[cprt + 3] = 't';
  color_settings_write_be32(profile, cprt + 8, 1);
  profile[cprt + 12] = 'A'; profile[cprt + 13] = 'E'; profile[cprt + 14] = 'X';
  return profile;
}
constexpr std::array<uint8_t, 3144> kCanonicalSrgbIcc = {
    0x00, 0x00, 0x0c, 0x48, 0x4c, 0x69, 0x6e, 0x6f, 0x02, 0x10, 0x00, 0x00, 0x6d, 0x6e, 0x74, 0x72,
    0x52, 0x47, 0x42, 0x20, 0x58, 0x59, 0x5a, 0x20, 0x07, 0xce, 0x00, 0x02, 0x00, 0x09, 0x00, 0x06,
    0x00, 0x31, 0x00, 0x00, 0x61, 0x63, 0x73, 0x70, 0x4d, 0x53, 0x46, 0x54, 0x00, 0x00, 0x00, 0x00,
    0x49, 0x45, 0x43, 0x20, 0x73, 0x52, 0x47, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6, 0xd6, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xd3, 0x2d,
    0x48, 0x50, 0x20, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x11, 0x63, 0x70, 0x72, 0x74, 0x00, 0x00, 0x01, 0x50, 0x00, 0x00, 0x00, 0x33,
    0x64, 0x65, 0x73, 0x63, 0x00, 0x00, 0x01, 0x84, 0x00, 0x00, 0x00, 0x6c, 0x77, 0x74, 0x70, 0x74,
    0x00, 0x00, 0x01, 0xf0, 0x00, 0x00, 0x00, 0x14, 0x62, 0x6b, 0x70, 0x74, 0x00, 0x00, 0x02, 0x04,
    0x00, 0x00, 0x00, 0x14, 0x72, 0x58, 0x59, 0x5a, 0x00, 0x00, 0x02, 0x18, 0x00, 0x00, 0x00, 0x14,
    0x67, 0x58, 0x59, 0x5a, 0x00, 0x00, 0x02, 0x2c, 0x00, 0x00, 0x00, 0x14, 0x62, 0x58, 0x59, 0x5a,
    0x00, 0x00, 0x02, 0x40, 0x00, 0x00, 0x00, 0x14, 0x64, 0x6d, 0x6e, 0x64, 0x00, 0x00, 0x02, 0x54,
    0x00, 0x00, 0x00, 0x70, 0x64, 0x6d, 0x64, 0x64, 0x00, 0x00, 0x02, 0xc4, 0x00, 0x00, 0x00, 0x88,
    0x76, 0x75, 0x65, 0x64, 0x00, 0x00, 0x03, 0x4c, 0x00, 0x00, 0x00, 0x86, 0x76, 0x69, 0x65, 0x77,
    0x00, 0x00, 0x03, 0xd4, 0x00, 0x00, 0x00, 0x24, 0x6c, 0x75, 0x6d, 0x69, 0x00, 0x00, 0x03, 0xf8,
    0x00, 0x00, 0x00, 0x14, 0x6d, 0x65, 0x61, 0x73, 0x00, 0x00, 0x04, 0x0c, 0x00, 0x00, 0x00, 0x24,
    0x74, 0x65, 0x63, 0x68, 0x00, 0x00, 0x04, 0x30, 0x00, 0x00, 0x00, 0x0c, 0x72, 0x54, 0x52, 0x43,
    0x00, 0x00, 0x04, 0x3c, 0x00, 0x00, 0x08, 0x0c, 0x67, 0x54, 0x52, 0x43, 0x00, 0x00, 0x04, 0x3c,
    0x00, 0x00, 0x08, 0x0c, 0x62, 0x54, 0x52, 0x43, 0x00, 0x00, 0x04, 0x3c, 0x00, 0x00, 0x08, 0x0c,
    0x74, 0x65, 0x78, 0x74, 0x00, 0x00, 0x00, 0x00, 0x43, 0x6f, 0x70, 0x79, 0x72, 0x69, 0x67, 0x68,
    0x74, 0x20, 0x28, 0x63, 0x29, 0x20, 0x31, 0x39, 0x39, 0x38, 0x20, 0x48, 0x65, 0x77, 0x6c, 0x65,
    0x74, 0x74, 0x2d, 0x50, 0x61, 0x63, 0x6b, 0x61, 0x72, 0x64, 0x20, 0x43, 0x6f, 0x6d, 0x70, 0x61,
    0x6e, 0x79, 0x00, 0x00, 0x64, 0x65, 0x73, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12,
    0x73, 0x52, 0x47, 0x42, 0x20, 0x49, 0x45, 0x43, 0x36, 0x31, 0x39, 0x36, 0x36, 0x2d, 0x32, 0x2e,
    0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0x73, 0x52, 0x47,
    0x42, 0x20, 0x49, 0x45, 0x43, 0x36, 0x31, 0x39, 0x36, 0x36, 0x2d, 0x32, 0x2e, 0x31, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x58, 0x59, 0x5a, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf3, 0x51, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x01, 0x16, 0xcc, 0x58, 0x59, 0x5a, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x58, 0x59, 0x5a, 0x20, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x6f, 0xa2, 0x00, 0x00, 0x38, 0xf5, 0x00, 0x00, 0x03, 0x90, 0x58, 0x59, 0x5a, 0x20,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x62, 0x99, 0x00, 0x00, 0xb7, 0x85, 0x00, 0x00, 0x18, 0xda,
    0x58, 0x59, 0x5a, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x24, 0xa0, 0x00, 0x00, 0x0f, 0x84,
    0x00, 0x00, 0xb6, 0xcf, 0x64, 0x65, 0x73, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x16,
    0x49, 0x45, 0x43, 0x20, 0x68, 0x74, 0x74, 0x70, 0x3a, 0x2f, 0x2f, 0x77, 0x77, 0x77, 0x2e, 0x69,
    0x65, 0x63, 0x2e, 0x63, 0x68, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x16, 0x49, 0x45, 0x43, 0x20, 0x68, 0x74, 0x74, 0x70, 0x3a, 0x2f, 0x2f, 0x77, 0x77, 0x77, 0x2e,
    0x69, 0x65, 0x63, 0x2e, 0x63, 0x68, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x64, 0x65, 0x73, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2e,
    0x49, 0x45, 0x43, 0x20, 0x36, 0x31, 0x39, 0x36, 0x36, 0x2d, 0x32, 0x2e, 0x31, 0x20, 0x44, 0x65,
    0x66, 0x61, 0x75, 0x6c, 0x74, 0x20, 0x52, 0x47, 0x42, 0x20, 0x63, 0x6f, 0x6c, 0x6f, 0x75, 0x72,
    0x20, 0x73, 0x70, 0x61, 0x63, 0x65, 0x20, 0x2d, 0x20, 0x73, 0x52, 0x47, 0x42, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2e, 0x49, 0x45, 0x43, 0x20, 0x36, 0x31, 0x39,
    0x36, 0x36, 0x2d, 0x32, 0x2e, 0x31, 0x20, 0x44, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74, 0x20, 0x52,
    0x47, 0x42, 0x20, 0x63, 0x6f, 0x6c, 0x6f, 0x75, 0x72, 0x20, 0x73, 0x70, 0x61, 0x63, 0x65, 0x20,
    0x2d, 0x20, 0x73, 0x52, 0x47, 0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x64, 0x65, 0x73, 0x63,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x52, 0x65, 0x66, 0x65, 0x72, 0x65, 0x6e, 0x63,
    0x65, 0x20, 0x56, 0x69, 0x65, 0x77, 0x69, 0x6e, 0x67, 0x20, 0x43, 0x6f, 0x6e, 0x64, 0x69, 0x74,
    0x69, 0x6f, 0x6e, 0x20, 0x69, 0x6e, 0x20, 0x49, 0x45, 0x43, 0x36, 0x31, 0x39, 0x36, 0x36, 0x2d,
    0x32, 0x2e, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x52,
    0x65, 0x66, 0x65, 0x72, 0x65, 0x6e, 0x63, 0x65, 0x20, 0x56, 0x69, 0x65, 0x77, 0x69, 0x6e, 0x67,
    0x20, 0x43, 0x6f, 0x6e, 0x64, 0x69, 0x74, 0x69, 0x6f, 0x6e, 0x20, 0x69, 0x6e, 0x20, 0x49, 0x45,
    0x43, 0x36, 0x31, 0x39, 0x36, 0x36, 0x2d, 0x32, 0x2e, 0x31, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x76, 0x69, 0x65, 0x77, 0x00, 0x00, 0x00, 0x00, 0x00, 0x13, 0xa4, 0xfe,
    0x00, 0x14, 0x5f, 0x2e, 0x00, 0x10, 0xcf, 0x14, 0x00, 0x03, 0xed, 0xcc, 0x00, 0x04, 0x13, 0x0b,
    0x00, 0x03, 0x5c, 0x9e, 0x00, 0x00, 0x00, 0x01, 0x58, 0x59, 0x5a, 0x20, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x4c, 0x09, 0x56, 0x00, 0x50, 0x00, 0x00, 0x00, 0x57, 0x1f, 0xe7, 0x6d, 0x65, 0x61, 0x73,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x8f, 0x00, 0x00, 0x00, 0x02,
    0x73, 0x69, 0x67, 0x20, 0x00, 0x00, 0x00, 0x00, 0x43, 0x52, 0x54, 0x20, 0x63, 0x75, 0x72, 0x76,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x0a, 0x00, 0x0f,
    0x00, 0x14, 0x00, 0x19, 0x00, 0x1e, 0x00, 0x23, 0x00, 0x28, 0x00, 0x2d, 0x00, 0x32, 0x00, 0x37,
    0x00, 0x3b, 0x00, 0x40, 0x00, 0x45, 0x00, 0x4a, 0x00, 0x4f, 0x00, 0x54, 0x00, 0x59, 0x00, 0x5e,
    0x00, 0x63, 0x00, 0x68, 0x00, 0x6d, 0x00, 0x72, 0x00, 0x77, 0x00, 0x7c, 0x00, 0x81, 0x00, 0x86,
    0x00, 0x8b, 0x00, 0x90, 0x00, 0x95, 0x00, 0x9a, 0x00, 0x9f, 0x00, 0xa4, 0x00, 0xa9, 0x00, 0xae,
    0x00, 0xb2, 0x00, 0xb7, 0x00, 0xbc, 0x00, 0xc1, 0x00, 0xc6, 0x00, 0xcb, 0x00, 0xd0, 0x00, 0xd5,
    0x00, 0xdb, 0x00, 0xe0, 0x00, 0xe5, 0x00, 0xeb, 0x00, 0xf0, 0x00, 0xf6, 0x00, 0xfb, 0x01, 0x01,
    0x01, 0x07, 0x01, 0x0d, 0x01, 0x13, 0x01, 0x19, 0x01, 0x1f, 0x01, 0x25, 0x01, 0x2b, 0x01, 0x32,
    0x01, 0x38, 0x01, 0x3e, 0x01, 0x45, 0x01, 0x4c, 0x01, 0x52, 0x01, 0x59, 0x01, 0x60, 0x01, 0x67,
    0x01, 0x6e, 0x01, 0x75, 0x01, 0x7c, 0x01, 0x83, 0x01, 0x8b, 0x01, 0x92, 0x01, 0x9a, 0x01, 0xa1,
    0x01, 0xa9, 0x01, 0xb1, 0x01, 0xb9, 0x01, 0xc1, 0x01, 0xc9, 0x01, 0xd1, 0x01, 0xd9, 0x01, 0xe1,
    0x01, 0xe9, 0x01, 0xf2, 0x01, 0xfa, 0x02, 0x03, 0x02, 0x0c, 0x02, 0x14, 0x02, 0x1d, 0x02, 0x26,
    0x02, 0x2f, 0x02, 0x38, 0x02, 0x41, 0x02, 0x4b, 0x02, 0x54, 0x02, 0x5d, 0x02, 0x67, 0x02, 0x71,
    0x02, 0x7a, 0x02, 0x84, 0x02, 0x8e, 0x02, 0x98, 0x02, 0xa2, 0x02, 0xac, 0x02, 0xb6, 0x02, 0xc1,
    0x02, 0xcb, 0x02, 0xd5, 0x02, 0xe0, 0x02, 0xeb, 0x02, 0xf5, 0x03, 0x00, 0x03, 0x0b, 0x03, 0x16,
    0x03, 0x21, 0x03, 0x2d, 0x03, 0x38, 0x03, 0x43, 0x03, 0x4f, 0x03, 0x5a, 0x03, 0x66, 0x03, 0x72,
    0x03, 0x7e, 0x03, 0x8a, 0x03, 0x96, 0x03, 0xa2, 0x03, 0xae, 0x03, 0xba, 0x03, 0xc7, 0x03, 0xd3,
    0x03, 0xe0, 0x03, 0xec, 0x03, 0xf9, 0x04, 0x06, 0x04, 0x13, 0x04, 0x20, 0x04, 0x2d, 0x04, 0x3b,
    0x04, 0x48, 0x04, 0x55, 0x04, 0x63, 0x04, 0x71, 0x04, 0x7e, 0x04, 0x8c, 0x04, 0x9a, 0x04, 0xa8,
    0x04, 0xb6, 0x04, 0xc4, 0x04, 0xd3, 0x04, 0xe1, 0x04, 0xf0, 0x04, 0xfe, 0x05, 0x0d, 0x05, 0x1c,
    0x05, 0x2b, 0x05, 0x3a, 0x05, 0x49, 0x05, 0x58, 0x05, 0x67, 0x05, 0x77, 0x05, 0x86, 0x05, 0x96,
    0x05, 0xa6, 0x05, 0xb5, 0x05, 0xc5, 0x05, 0xd5, 0x05, 0xe5, 0x05, 0xf6, 0x06, 0x06, 0x06, 0x16,
    0x06, 0x27, 0x06, 0x37, 0x06, 0x48, 0x06, 0x59, 0x06, 0x6a, 0x06, 0x7b, 0x06, 0x8c, 0x06, 0x9d,
    0x06, 0xaf, 0x06, 0xc0, 0x06, 0xd1, 0x06, 0xe3, 0x06, 0xf5, 0x07, 0x07, 0x07, 0x19, 0x07, 0x2b,
    0x07, 0x3d, 0x07, 0x4f, 0x07, 0x61, 0x07, 0x74, 0x07, 0x86, 0x07, 0x99, 0x07, 0xac, 0x07, 0xbf,
    0x07, 0xd2, 0x07, 0xe5, 0x07, 0xf8, 0x08, 0x0b, 0x08, 0x1f, 0x08, 0x32, 0x08, 0x46, 0x08, 0x5a,
    0x08, 0x6e, 0x08, 0x82, 0x08, 0x96, 0x08, 0xaa, 0x08, 0xbe, 0x08, 0xd2, 0x08, 0xe7, 0x08, 0xfb,
    0x09, 0x10, 0x09, 0x25, 0x09, 0x3a, 0x09, 0x4f, 0x09, 0x64, 0x09, 0x79, 0x09, 0x8f, 0x09, 0xa4,
    0x09, 0xba, 0x09, 0xcf, 0x09, 0xe5, 0x09, 0xfb, 0x0a, 0x11, 0x0a, 0x27, 0x0a, 0x3d, 0x0a, 0x54,
    0x0a, 0x6a, 0x0a, 0x81, 0x0a, 0x98, 0x0a, 0xae, 0x0a, 0xc5, 0x0a, 0xdc, 0x0a, 0xf3, 0x0b, 0x0b,
    0x0b, 0x22, 0x0b, 0x39, 0x0b, 0x51, 0x0b, 0x69, 0x0b, 0x80, 0x0b, 0x98, 0x0b, 0xb0, 0x0b, 0xc8,
    0x0b, 0xe1, 0x0b, 0xf9, 0x0c, 0x12, 0x0c, 0x2a, 0x0c, 0x43, 0x0c, 0x5c, 0x0c, 0x75, 0x0c, 0x8e,
    0x0c, 0xa7, 0x0c, 0xc0, 0x0c, 0xd9, 0x0c, 0xf3, 0x0d, 0x0d, 0x0d, 0x26, 0x0d, 0x40, 0x0d, 0x5a,
    0x0d, 0x74, 0x0d, 0x8e, 0x0d, 0xa9, 0x0d, 0xc3, 0x0d, 0xde, 0x0d, 0xf8, 0x0e, 0x13, 0x0e, 0x2e,
    0x0e, 0x49, 0x0e, 0x64, 0x0e, 0x7f, 0x0e, 0x9b, 0x0e, 0xb6, 0x0e, 0xd2, 0x0e, 0xee, 0x0f, 0x09,
    0x0f, 0x25, 0x0f, 0x41, 0x0f, 0x5e, 0x0f, 0x7a, 0x0f, 0x96, 0x0f, 0xb3, 0x0f, 0xcf, 0x0f, 0xec,
    0x10, 0x09, 0x10, 0x26, 0x10, 0x43, 0x10, 0x61, 0x10, 0x7e, 0x10, 0x9b, 0x10, 0xb9, 0x10, 0xd7,
    0x10, 0xf5, 0x11, 0x13, 0x11, 0x31, 0x11, 0x4f, 0x11, 0x6d, 0x11, 0x8c, 0x11, 0xaa, 0x11, 0xc9,
    0x11, 0xe8, 0x12, 0x07, 0x12, 0x26, 0x12, 0x45, 0x12, 0x64, 0x12, 0x84, 0x12, 0xa3, 0x12, 0xc3,
    0x12, 0xe3, 0x13, 0x03, 0x13, 0x23, 0x13, 0x43, 0x13, 0x63, 0x13, 0x83, 0x13, 0xa4, 0x13, 0xc5,
    0x13, 0xe5, 0x14, 0x06, 0x14, 0x27, 0x14, 0x49, 0x14, 0x6a, 0x14, 0x8b, 0x14, 0xad, 0x14, 0xce,
    0x14, 0xf0, 0x15, 0x12, 0x15, 0x34, 0x15, 0x56, 0x15, 0x78, 0x15, 0x9b, 0x15, 0xbd, 0x15, 0xe0,
    0x16, 0x03, 0x16, 0x26, 0x16, 0x49, 0x16, 0x6c, 0x16, 0x8f, 0x16, 0xb2, 0x16, 0xd6, 0x16, 0xfa,
    0x17, 0x1d, 0x17, 0x41, 0x17, 0x65, 0x17, 0x89, 0x17, 0xae, 0x17, 0xd2, 0x17, 0xf7, 0x18, 0x1b,
    0x18, 0x40, 0x18, 0x65, 0x18, 0x8a, 0x18, 0xaf, 0x18, 0xd5, 0x18, 0xfa, 0x19, 0x20, 0x19, 0x45,
    0x19, 0x6b, 0x19, 0x91, 0x19, 0xb7, 0x19, 0xdd, 0x1a, 0x04, 0x1a, 0x2a, 0x1a, 0x51, 0x1a, 0x77,
    0x1a, 0x9e, 0x1a, 0xc5, 0x1a, 0xec, 0x1b, 0x14, 0x1b, 0x3b, 0x1b, 0x63, 0x1b, 0x8a, 0x1b, 0xb2,
    0x1b, 0xda, 0x1c, 0x02, 0x1c, 0x2a, 0x1c, 0x52, 0x1c, 0x7b, 0x1c, 0xa3, 0x1c, 0xcc, 0x1c, 0xf5,
    0x1d, 0x1e, 0x1d, 0x47, 0x1d, 0x70, 0x1d, 0x99, 0x1d, 0xc3, 0x1d, 0xec, 0x1e, 0x16, 0x1e, 0x40,
    0x1e, 0x6a, 0x1e, 0x94, 0x1e, 0xbe, 0x1e, 0xe9, 0x1f, 0x13, 0x1f, 0x3e, 0x1f, 0x69, 0x1f, 0x94,
    0x1f, 0xbf, 0x1f, 0xea, 0x20, 0x15, 0x20, 0x41, 0x20, 0x6c, 0x20, 0x98, 0x20, 0xc4, 0x20, 0xf0,
    0x21, 0x1c, 0x21, 0x48, 0x21, 0x75, 0x21, 0xa1, 0x21, 0xce, 0x21, 0xfb, 0x22, 0x27, 0x22, 0x55,
    0x22, 0x82, 0x22, 0xaf, 0x22, 0xdd, 0x23, 0x0a, 0x23, 0x38, 0x23, 0x66, 0x23, 0x94, 0x23, 0xc2,
    0x23, 0xf0, 0x24, 0x1f, 0x24, 0x4d, 0x24, 0x7c, 0x24, 0xab, 0x24, 0xda, 0x25, 0x09, 0x25, 0x38,
    0x25, 0x68, 0x25, 0x97, 0x25, 0xc7, 0x25, 0xf7, 0x26, 0x27, 0x26, 0x57, 0x26, 0x87, 0x26, 0xb7,
    0x26, 0xe8, 0x27, 0x18, 0x27, 0x49, 0x27, 0x7a, 0x27, 0xab, 0x27, 0xdc, 0x28, 0x0d, 0x28, 0x3f,
    0x28, 0x71, 0x28, 0xa2, 0x28, 0xd4, 0x29, 0x06, 0x29, 0x38, 0x29, 0x6b, 0x29, 0x9d, 0x29, 0xd0,
    0x2a, 0x02, 0x2a, 0x35, 0x2a, 0x68, 0x2a, 0x9b, 0x2a, 0xcf, 0x2b, 0x02, 0x2b, 0x36, 0x2b, 0x69,
    0x2b, 0x9d, 0x2b, 0xd1, 0x2c, 0x05, 0x2c, 0x39, 0x2c, 0x6e, 0x2c, 0xa2, 0x2c, 0xd7, 0x2d, 0x0c,
    0x2d, 0x41, 0x2d, 0x76, 0x2d, 0xab, 0x2d, 0xe1, 0x2e, 0x16, 0x2e, 0x4c, 0x2e, 0x82, 0x2e, 0xb7,
    0x2e, 0xee, 0x2f, 0x24, 0x2f, 0x5a, 0x2f, 0x91, 0x2f, 0xc7, 0x2f, 0xfe, 0x30, 0x35, 0x30, 0x6c,
    0x30, 0xa4, 0x30, 0xdb, 0x31, 0x12, 0x31, 0x4a, 0x31, 0x82, 0x31, 0xba, 0x31, 0xf2, 0x32, 0x2a,
    0x32, 0x63, 0x32, 0x9b, 0x32, 0xd4, 0x33, 0x0d, 0x33, 0x46, 0x33, 0x7f, 0x33, 0xb8, 0x33, 0xf1,
    0x34, 0x2b, 0x34, 0x65, 0x34, 0x9e, 0x34, 0xd8, 0x35, 0x13, 0x35, 0x4d, 0x35, 0x87, 0x35, 0xc2,
    0x35, 0xfd, 0x36, 0x37, 0x36, 0x72, 0x36, 0xae, 0x36, 0xe9, 0x37, 0x24, 0x37, 0x60, 0x37, 0x9c,
    0x37, 0xd7, 0x38, 0x14, 0x38, 0x50, 0x38, 0x8c, 0x38, 0xc8, 0x39, 0x05, 0x39, 0x42, 0x39, 0x7f,
    0x39, 0xbc, 0x39, 0xf9, 0x3a, 0x36, 0x3a, 0x74, 0x3a, 0xb2, 0x3a, 0xef, 0x3b, 0x2d, 0x3b, 0x6b,
    0x3b, 0xaa, 0x3b, 0xe8, 0x3c, 0x27, 0x3c, 0x65, 0x3c, 0xa4, 0x3c, 0xe3, 0x3d, 0x22, 0x3d, 0x61,
    0x3d, 0xa1, 0x3d, 0xe0, 0x3e, 0x20, 0x3e, 0x60, 0x3e, 0xa0, 0x3e, 0xe0, 0x3f, 0x21, 0x3f, 0x61,
    0x3f, 0xa2, 0x3f, 0xe2, 0x40, 0x23, 0x40, 0x64, 0x40, 0xa6, 0x40, 0xe7, 0x41, 0x29, 0x41, 0x6a,
    0x41, 0xac, 0x41, 0xee, 0x42, 0x30, 0x42, 0x72, 0x42, 0xb5, 0x42, 0xf7, 0x43, 0x3a, 0x43, 0x7d,
    0x43, 0xc0, 0x44, 0x03, 0x44, 0x47, 0x44, 0x8a, 0x44, 0xce, 0x45, 0x12, 0x45, 0x55, 0x45, 0x9a,
    0x45, 0xde, 0x46, 0x22, 0x46, 0x67, 0x46, 0xab, 0x46, 0xf0, 0x47, 0x35, 0x47, 0x7b, 0x47, 0xc0,
    0x48, 0x05, 0x48, 0x4b, 0x48, 0x91, 0x48, 0xd7, 0x49, 0x1d, 0x49, 0x63, 0x49, 0xa9, 0x49, 0xf0,
    0x4a, 0x37, 0x4a, 0x7d, 0x4a, 0xc4, 0x4b, 0x0c, 0x4b, 0x53, 0x4b, 0x9a, 0x4b, 0xe2, 0x4c, 0x2a,
    0x4c, 0x72, 0x4c, 0xba, 0x4d, 0x02, 0x4d, 0x4a, 0x4d, 0x93, 0x4d, 0xdc, 0x4e, 0x25, 0x4e, 0x6e,
    0x4e, 0xb7, 0x4f, 0x00, 0x4f, 0x49, 0x4f, 0x93, 0x4f, 0xdd, 0x50, 0x27, 0x50, 0x71, 0x50, 0xbb,
    0x51, 0x06, 0x51, 0x50, 0x51, 0x9b, 0x51, 0xe6, 0x52, 0x31, 0x52, 0x7c, 0x52, 0xc7, 0x53, 0x13,
    0x53, 0x5f, 0x53, 0xaa, 0x53, 0xf6, 0x54, 0x42, 0x54, 0x8f, 0x54, 0xdb, 0x55, 0x28, 0x55, 0x75,
    0x55, 0xc2, 0x56, 0x0f, 0x56, 0x5c, 0x56, 0xa9, 0x56, 0xf7, 0x57, 0x44, 0x57, 0x92, 0x57, 0xe0,
    0x58, 0x2f, 0x58, 0x7d, 0x58, 0xcb, 0x59, 0x1a, 0x59, 0x69, 0x59, 0xb8, 0x5a, 0x07, 0x5a, 0x56,
    0x5a, 0xa6, 0x5a, 0xf5, 0x5b, 0x45, 0x5b, 0x95, 0x5b, 0xe5, 0x5c, 0x35, 0x5c, 0x86, 0x5c, 0xd6,
    0x5d, 0x27, 0x5d, 0x78, 0x5d, 0xc9, 0x5e, 0x1a, 0x5e, 0x6c, 0x5e, 0xbd, 0x5f, 0x0f, 0x5f, 0x61,
    0x5f, 0xb3, 0x60, 0x05, 0x60, 0x57, 0x60, 0xaa, 0x60, 0xfc, 0x61, 0x4f, 0x61, 0xa2, 0x61, 0xf5,
    0x62, 0x49, 0x62, 0x9c, 0x62, 0xf0, 0x63, 0x43, 0x63, 0x97, 0x63, 0xeb, 0x64, 0x40, 0x64, 0x94,
    0x64, 0xe9, 0x65, 0x3d, 0x65, 0x92, 0x65, 0xe7, 0x66, 0x3d, 0x66, 0x92, 0x66, 0xe8, 0x67, 0x3d,
    0x67, 0x93, 0x67, 0xe9, 0x68, 0x3f, 0x68, 0x96, 0x68, 0xec, 0x69, 0x43, 0x69, 0x9a, 0x69, 0xf1,
    0x6a, 0x48, 0x6a, 0x9f, 0x6a, 0xf7, 0x6b, 0x4f, 0x6b, 0xa7, 0x6b, 0xff, 0x6c, 0x57, 0x6c, 0xaf,
    0x6d, 0x08, 0x6d, 0x60, 0x6d, 0xb9, 0x6e, 0x12, 0x6e, 0x6b, 0x6e, 0xc4, 0x6f, 0x1e, 0x6f, 0x78,
    0x6f, 0xd1, 0x70, 0x2b, 0x70, 0x86, 0x70, 0xe0, 0x71, 0x3a, 0x71, 0x95, 0x71, 0xf0, 0x72, 0x4b,
    0x72, 0xa6, 0x73, 0x01, 0x73, 0x5d, 0x73, 0xb8, 0x74, 0x14, 0x74, 0x70, 0x74, 0xcc, 0x75, 0x28,
    0x75, 0x85, 0x75, 0xe1, 0x76, 0x3e, 0x76, 0x9b, 0x76, 0xf8, 0x77, 0x56, 0x77, 0xb3, 0x78, 0x11,
    0x78, 0x6e, 0x78, 0xcc, 0x79, 0x2a, 0x79, 0x89, 0x79, 0xe7, 0x7a, 0x46, 0x7a, 0xa5, 0x7b, 0x04,
    0x7b, 0x63, 0x7b, 0xc2, 0x7c, 0x21, 0x7c, 0x81, 0x7c, 0xe1, 0x7d, 0x41, 0x7d, 0xa1, 0x7e, 0x01,
    0x7e, 0x62, 0x7e, 0xc2, 0x7f, 0x23, 0x7f, 0x84, 0x7f, 0xe5, 0x80, 0x47, 0x80, 0xa8, 0x81, 0x0a,
    0x81, 0x6b, 0x81, 0xcd, 0x82, 0x30, 0x82, 0x92, 0x82, 0xf4, 0x83, 0x57, 0x83, 0xba, 0x84, 0x1d,
    0x84, 0x80, 0x84, 0xe3, 0x85, 0x47, 0x85, 0xab, 0x86, 0x0e, 0x86, 0x72, 0x86, 0xd7, 0x87, 0x3b,
    0x87, 0x9f, 0x88, 0x04, 0x88, 0x69, 0x88, 0xce, 0x89, 0x33, 0x89, 0x99, 0x89, 0xfe, 0x8a, 0x64,
    0x8a, 0xca, 0x8b, 0x30, 0x8b, 0x96, 0x8b, 0xfc, 0x8c, 0x63, 0x8c, 0xca, 0x8d, 0x31, 0x8d, 0x98,
    0x8d, 0xff, 0x8e, 0x66, 0x8e, 0xce, 0x8f, 0x36, 0x8f, 0x9e, 0x90, 0x06, 0x90, 0x6e, 0x90, 0xd6,
    0x91, 0x3f, 0x91, 0xa8, 0x92, 0x11, 0x92, 0x7a, 0x92, 0xe3, 0x93, 0x4d, 0x93, 0xb6, 0x94, 0x20,
    0x94, 0x8a, 0x94, 0xf4, 0x95, 0x5f, 0x95, 0xc9, 0x96, 0x34, 0x96, 0x9f, 0x97, 0x0a, 0x97, 0x75,
    0x97, 0xe0, 0x98, 0x4c, 0x98, 0xb8, 0x99, 0x24, 0x99, 0x90, 0x99, 0xfc, 0x9a, 0x68, 0x9a, 0xd5,
    0x9b, 0x42, 0x9b, 0xaf, 0x9c, 0x1c, 0x9c, 0x89, 0x9c, 0xf7, 0x9d, 0x64, 0x9d, 0xd2, 0x9e, 0x40,
    0x9e, 0xae, 0x9f, 0x1d, 0x9f, 0x8b, 0x9f, 0xfa, 0xa0, 0x69, 0xa0, 0xd8, 0xa1, 0x47, 0xa1, 0xb6,
    0xa2, 0x26, 0xa2, 0x96, 0xa3, 0x06, 0xa3, 0x76, 0xa3, 0xe6, 0xa4, 0x56, 0xa4, 0xc7, 0xa5, 0x38,
    0xa5, 0xa9, 0xa6, 0x1a, 0xa6, 0x8b, 0xa6, 0xfd, 0xa7, 0x6e, 0xa7, 0xe0, 0xa8, 0x52, 0xa8, 0xc4,
    0xa9, 0x37, 0xa9, 0xa9, 0xaa, 0x1c, 0xaa, 0x8f, 0xab, 0x02, 0xab, 0x75, 0xab, 0xe9, 0xac, 0x5c,
    0xac, 0xd0, 0xad, 0x44, 0xad, 0xb8, 0xae, 0x2d, 0xae, 0xa1, 0xaf, 0x16, 0xaf, 0x8b, 0xb0, 0x00,
    0xb0, 0x75, 0xb0, 0xea, 0xb1, 0x60, 0xb1, 0xd6, 0xb2, 0x4b, 0xb2, 0xc2, 0xb3, 0x38, 0xb3, 0xae,
    0xb4, 0x25, 0xb4, 0x9c, 0xb5, 0x13, 0xb5, 0x8a, 0xb6, 0x01, 0xb6, 0x79, 0xb6, 0xf0, 0xb7, 0x68,
    0xb7, 0xe0, 0xb8, 0x59, 0xb8, 0xd1, 0xb9, 0x4a, 0xb9, 0xc2, 0xba, 0x3b, 0xba, 0xb5, 0xbb, 0x2e,
    0xbb, 0xa7, 0xbc, 0x21, 0xbc, 0x9b, 0xbd, 0x15, 0xbd, 0x8f, 0xbe, 0x0a, 0xbe, 0x84, 0xbe, 0xff,
    0xbf, 0x7a, 0xbf, 0xf5, 0xc0, 0x70, 0xc0, 0xec, 0xc1, 0x67, 0xc1, 0xe3, 0xc2, 0x5f, 0xc2, 0xdb,
    0xc3, 0x58, 0xc3, 0xd4, 0xc4, 0x51, 0xc4, 0xce, 0xc5, 0x4b, 0xc5, 0xc8, 0xc6, 0x46, 0xc6, 0xc3,
    0xc7, 0x41, 0xc7, 0xbf, 0xc8, 0x3d, 0xc8, 0xbc, 0xc9, 0x3a, 0xc9, 0xb9, 0xca, 0x38, 0xca, 0xb7,
    0xcb, 0x36, 0xcb, 0xb6, 0xcc, 0x35, 0xcc, 0xb5, 0xcd, 0x35, 0xcd, 0xb5, 0xce, 0x36, 0xce, 0xb6,
    0xcf, 0x37, 0xcf, 0xb8, 0xd0, 0x39, 0xd0, 0xba, 0xd1, 0x3c, 0xd1, 0xbe, 0xd2, 0x3f, 0xd2, 0xc1,
    0xd3, 0x44, 0xd3, 0xc6, 0xd4, 0x49, 0xd4, 0xcb, 0xd5, 0x4e, 0xd5, 0xd1, 0xd6, 0x55, 0xd6, 0xd8,
    0xd7, 0x5c, 0xd7, 0xe0, 0xd8, 0x64, 0xd8, 0xe8, 0xd9, 0x6c, 0xd9, 0xf1, 0xda, 0x76, 0xda, 0xfb,
    0xdb, 0x80, 0xdc, 0x05, 0xdc, 0x8a, 0xdd, 0x10, 0xdd, 0x96, 0xde, 0x1c, 0xde, 0xa2, 0xdf, 0x29,
    0xdf, 0xaf, 0xe0, 0x36, 0xe0, 0xbd, 0xe1, 0x44, 0xe1, 0xcc, 0xe2, 0x53, 0xe2, 0xdb, 0xe3, 0x63,
    0xe3, 0xeb, 0xe4, 0x73, 0xe4, 0xfc, 0xe5, 0x84, 0xe6, 0x0d, 0xe6, 0x96, 0xe7, 0x1f, 0xe7, 0xa9,
    0xe8, 0x32, 0xe8, 0xbc, 0xe9, 0x46, 0xe9, 0xd0, 0xea, 0x5b, 0xea, 0xe5, 0xeb, 0x70, 0xeb, 0xfb,
    0xec, 0x86, 0xed, 0x11, 0xed, 0x9c, 0xee, 0x28, 0xee, 0xb4, 0xef, 0x40, 0xef, 0xcc, 0xf0, 0x58,
    0xf0, 0xe5, 0xf1, 0x72, 0xf1, 0xff, 0xf2, 0x8c, 0xf3, 0x19, 0xf3, 0xa7, 0xf4, 0x34, 0xf4, 0xc2,
    0xf5, 0x50, 0xf5, 0xde, 0xf6, 0x6d, 0xf6, 0xfb, 0xf7, 0x8a, 0xf8, 0x19, 0xf8, 0xa8, 0xf9, 0x38,
    0xf9, 0xc7, 0xfa, 0x57, 0xfa, 0xe7, 0xfb, 0x77, 0xfc, 0x07, 0xfc, 0x98, 0xfd, 0x29, 0xfd, 0xba,
    0xfe, 0x4b, 0xfe, 0xdc, 0xff, 0x6d, 0xff, 0xff
};
static_assert(kCanonicalSrgbIcc.size() == 3144);
const std::vector<uint8_t>& color_settings_builtin_srgb_icc() {
  static const std::vector<uint8_t> profile(kCanonicalSrgbIcc.begin(), kCanonicalSrgbIcc.end());
  return profile;
}
const std::vector<uint8_t>& color_settings_builtin_linear_icc() {
  static const std::vector<uint8_t> profile =
      color_settings_build_minimal_rgb_icc(1.0f, "Linear sRGB");
  return profile;
}
bool color_settings_validate_icc(const void* data, int32_t size) {
  if (!data || size < 132) return false;
  const auto* bytes = static_cast<const uint8_t*>(data);
  const uint32_t declared = color_settings_read_be32(bytes);
  if (declared != static_cast<uint32_t>(size) || declared < 132 ||
      declared > kMaxIccProfileBytes) return false;
  if (std::memcmp(bytes + 36, "acsp", 4) != 0) return false;
  if (std::memcmp(bytes + 16, "RGB ", 4) != 0) return false;
  const uint32_t tag_count = color_settings_read_be32(bytes + 128);
  if (tag_count > 64) return false;
  const uint64_t tag_table_end = 132ULL + static_cast<uint64_t>(tag_count) * 12ULL;
  if (tag_table_end > declared) return false;
  std::array<std::pair<uint32_t, uint32_t>, 64> spans{};
  for (uint32_t index = 0; index < tag_count; ++index) {
    const std::size_t entry = 132 + static_cast<std::size_t>(index) * 12;
    const uint32_t offset = color_settings_read_be32(bytes + entry + 4);
    const uint32_t tag_size = color_settings_read_be32(bytes + entry + 8);
    if (offset < tag_table_end || tag_size < 4 ||
        static_cast<uint64_t>(offset) + static_cast<uint64_t>(tag_size) > declared) return false;
    const uint32_t end = offset + tag_size;
    for (uint32_t prior = 0; prior < index; ++prior) {
      const auto [prior_offset, prior_end] = spans[prior];
      const bool identical = offset == prior_offset && end == prior_end;
      const bool disjoint = end <= prior_offset || offset >= prior_end;
      if (!identical && !disjoint) return false;
    }
    spans[index] = {offset, end};
  }
  return true;
}
ColorProfileKind color_settings_classify_icc(const std::vector<uint8_t>& icc) {
  if (icc == color_settings_builtin_srgb_icc()) return ColorProfileKind::Srgb;
  if (icc == color_settings_builtin_linear_icc()) return ColorProfileKind::LinearSrgb;
  return ColorProfileKind::ImportedRgb;
}
std::u16string color_settings_profile_description(ColorProfileKind kind) {
  if (kind == ColorProfileKind::LinearSrgb) return u"Linear sRGB";
  if (kind == ColorProfileKind::Srgb) return u"sRGB IEC61966-2.1";
  return u"Imported RGB";
}
bool color_settings_snapshot_profile(void* handle, ColorProfileRecord& record) {
  if (!handle) return false;
  std::lock_guard<std::mutex> lock(g_color_settings_mutex);
  const auto found = g_color_profiles.find(handle);
  if (found == g_color_profiles.end() || !found->second.live) return false;
  try { record = found->second; }
  catch (...) { return false; }
  return true;
}
int32_t color_settings_create_profile(int32_t plugin_id, ColorProfileKind kind,
                                      const std::vector<uint8_t>& icc, void** handle) {
  if (plugin_id != 1 || !handle) return 4;
  *handle = nullptr;
  if (icc.empty() || icc.size() > kMaxIccProfileBytes) return 4;
  uint64_t generation = 0;
  if (!claim_opaque_generation(g_color_profile_generation, generation)) return 4;
  const auto token = reinterpret_cast<void*>(static_cast<uintptr_t>((generation << 3) | 7));
  std::lock_guard<std::mutex> lock(g_color_settings_mutex);
  if (g_color_profiles.size() >= kMaxColorProfiles) return 4;
  ColorProfileRecord record{};
  record.generation = generation;
  record.owner_plugin_id = plugin_id;
  record.kind = kind;
  try {
    record.icc_bytes = icc;
    record.live = true;
    if (!g_color_profiles.emplace(token, std::move(record)).second) return 4;
  } catch (...) { return 4; }
  ++g_color_profiles_created;
  *handle = token;
  return 0;
}
bool color_settings_profiles_balanced() {
  std::lock_guard<std::mutex> lock(g_color_settings_mutex);
  return g_color_profiles.empty() && g_color_profiles_created == g_color_profiles_disposed;
}
float color_settings_linear_to_srgb(float channel) {
  if (!(channel > 0.0f)) return 0.0f;
  if (channel <= 0.0031308f) return 12.92f * channel;
  return 1.055f * std::pow(channel, 1.0f / 2.4f) - 0.055f;
}
template <typename T>
T color_settings_clamp_channel(float value, float max_value) {
  const float clamped = (std::max)(0.0f, (std::min)(max_value, value));
  return static_cast<T>(clamped + (clamped >= 0.0f ? 0.5f : -0.5f));
}
int32_t color_settings_transform_pixel(ColorProfileKind working_kind, int32_t type,
                                       const void* src_pixel, void* dst_pixel) {
  float alpha = 0.0f, red = 0.0f, green = 0.0f, blue = 0.0f;
  if (type == 1) {
    std::array<uint8_t, 4> storage{};
    std::memcpy(storage.data(), src_pixel, sizeof(storage));
    const auto* pixel = storage.data();
    alpha = pixel[0] / 255.0f; red = pixel[1] / 255.0f; green = pixel[2] / 255.0f; blue = pixel[3] / 255.0f;
  } else if (type == 2) {
    std::array<uint16_t, 4> storage{};
    std::memcpy(storage.data(), src_pixel, sizeof(storage));
    const auto* pixel = storage.data();
    alpha = pixel[0] / 32768.0f; red = pixel[1] / 32768.0f; green = pixel[2] / 32768.0f; blue = pixel[3] / 32768.0f;
  } else if (type == 3) {
    std::array<float, 4> storage{};
    std::memcpy(storage.data(), src_pixel, sizeof(storage));
    const auto* pixel = storage.data();
    alpha = pixel[0]; red = pixel[1]; green = pixel[2]; blue = pixel[3];
    if (!std::isfinite(red) || !std::isfinite(green) || !std::isfinite(blue)) return 4;
  } else return 4;
  if (working_kind == ColorProfileKind::LinearSrgb) {
    red = color_settings_linear_to_srgb(red);
    green = color_settings_linear_to_srgb(green);
    blue = color_settings_linear_to_srgb(blue);
  } else if (working_kind != ColorProfileKind::Srgb) return 4;
  if (type == 1) {
    std::array<uint8_t, 4> pixel{};
    std::memcpy(pixel.data(), src_pixel, sizeof(pixel[0]));
    pixel[1] = color_settings_clamp_channel<uint8_t>(red * 255.0f, 255.0f);
    pixel[2] = color_settings_clamp_channel<uint8_t>(green * 255.0f, 255.0f);
    pixel[3] = color_settings_clamp_channel<uint8_t>(blue * 255.0f, 255.0f);
    std::memcpy(dst_pixel, pixel.data(), sizeof(pixel));
  } else if (type == 2) {
    std::array<uint16_t, 4> pixel{};
    std::memcpy(pixel.data(), src_pixel, sizeof(pixel[0]));
    pixel[1] = color_settings_clamp_channel<uint16_t>(red * 32768.0f, 32768.0f);
    pixel[2] = color_settings_clamp_channel<uint16_t>(green * 32768.0f, 32768.0f);
    pixel[3] = color_settings_clamp_channel<uint16_t>(blue * 32768.0f, 32768.0f);
    std::memcpy(dst_pixel, pixel.data(), sizeof(pixel));
  } else {
    std::array<float, 4> pixel{};
    std::memcpy(pixel.data(), src_pixel, sizeof(pixel[0]));
    pixel[1] = red; pixel[2] = green; pixel[3] = blue;
    std::memcpy(dst_pixel, pixel.data(), sizeof(pixel));
  }
  return 0;
}
int32_t color_settings_init_empty_utf16_handle(void** handle) {
  if (!handle) return 4;
  *handle = nullptr;
  return make_utf16_handle(std::u16string(), "color settings empty utf16", handle);
}
int32_t __cdecl color_get_blending_tables(void*, void** blending_tables) {
  if (blending_tables) *blending_tables = nullptr;
  ++g_invalid_color_profile_operations;
  return 4;
}
int32_t __cdecl color_does_view_have_xform(void* view, uint8_t* has_xform) {
  if (!has_xform) return 4;
  *has_xform = 0;
  if (view != &g_aegp_item_view) { ++g_invalid_color_profile_operations; return 4; }
  std::lock_guard<std::mutex> lock(g_color_settings_mutex);
  *has_xform = g_working_color_space_kind == ColorProfileKind::LinearSrgb ? 1 : 0;
  return 0;
}
int32_t __cdecl color_xform_working_to_view(void* view, void** src, void** dst) {
  if (!src || !dst) return 4;
  // AEGP_WorldH is an opaque handle token in this host, not a client-dereferenceable pointer.
  if (view != &g_aegp_item_view) { ++g_invalid_color_profile_operations; return 4; }
  aexcompat::world_registry::AegpWorldSnapshot src_view{}, dst_view{};
  if (!aexcompat::world_registry::snapshot_aegp_world(src, src_view) ||
      !aexcompat::world_registry::snapshot_aegp_world(dst, dst_view))
    return 4;
  const LocalEffectWorld& src_world = src_view.world;
  const LocalEffectWorld& dst_world = dst_view.world;
  const int32_t src_type = aegp_world_type_from_format(src_view.pixel_format);
  const int32_t dst_type = aegp_world_type_from_format(dst_view.pixel_format);
  if (!src_type || src_type != dst_type || src_world.width != dst_world.width ||
      src_world.height != dst_world.height || src_world.width <= 0 || src_world.height <= 0) return 4;
  const int32_t bytes_per_pixel = src_type == 1 ? 4 : (src_type == 2 ? 8 : 16);
  const int64_t minimum_rowbytes = static_cast<int64_t>(src_world.width) * bytes_per_pixel;
  if (minimum_rowbytes > (std::numeric_limits<int32_t>::max)() ||
      src_world.rowbytes < minimum_rowbytes || dst_world.rowbytes < minimum_rowbytes) return 4;
  const uint64_t pixel_bytes = static_cast<uint64_t>(src_world.height) * src_world.rowbytes;
  if (pixel_bytes > 128ULL * 1024 * 1024) return 4;
  std::unique_lock<std::mutex> src_pixels;
  std::unique_lock<std::mutex> dst_pixels;
  if (src_view.backing_pin && src_view.backing_pin == dst_view.backing_pin) {
    src_pixels = std::unique_lock<std::mutex>(src_view.backing_pin->pixels_mutex);
  } else if (src_view.backing_pin && dst_view.backing_pin) {
    src_pixels = std::unique_lock<std::mutex>(
        src_view.backing_pin->pixels_mutex, std::defer_lock);
    dst_pixels = std::unique_lock<std::mutex>(
        dst_view.backing_pin->pixels_mutex, std::defer_lock);
    std::lock(src_pixels, dst_pixels);
  } else if (src_view.backing_pin) {
    src_pixels = std::unique_lock<std::mutex>(src_view.backing_pin->pixels_mutex);
  } else if (dst_view.backing_pin) {
    dst_pixels = std::unique_lock<std::mutex>(dst_view.backing_pin->pixels_mutex);
  }
  ColorProfileKind working_kind{};
  {
    std::lock_guard<std::mutex> lock(g_color_settings_mutex);
    working_kind = g_working_color_space_kind;
  }
  if (working_kind == ColorProfileKind::ImportedRgb) return 4;
  std::vector<std::byte> scratch;
  const bool in_place = src == dst;
  if (in_place) {
    try { scratch.resize(static_cast<std::size_t>(pixel_bytes)); }
    catch (...) { return 4; }
    std::memcpy(scratch.data(), src_world.data, static_cast<std::size_t>(pixel_bytes));
  }
  const auto* read_base = in_place ? scratch.data() : static_cast<const std::byte*>(src_world.data);
  auto* write_base = static_cast<std::byte*>(dst_world.data);
  for (int32_t y = 0; y < src_world.height; ++y) {
    const auto* src_row = read_base + static_cast<std::size_t>(y) * src_world.rowbytes;
    auto* dst_row = write_base + static_cast<std::size_t>(y) * dst_world.rowbytes;
    for (int32_t x = 0; x < src_world.width; ++x) {
      const auto* src_pixel = src_row + static_cast<std::size_t>(x) * bytes_per_pixel;
      auto* dst_pixel = dst_row + static_cast<std::size_t>(x) * bytes_per_pixel;
      if (color_settings_transform_pixel(working_kind, src_type, src_pixel, dst_pixel) != 0) return 4;
    }
  }
  ++g_color_xform_calls;
  return 0;
}
int32_t __cdecl color_get_new_working_space_profile(int32_t plugin_id, void* comp, void** profile) {
  if (profile) *profile = nullptr;
  if (plugin_id != 1 || comp != composition_handle() || !profile) return 4;
  ColorProfileKind kind{};
  std::vector<uint8_t> icc;
  try {
    std::lock_guard<std::mutex> lock(g_color_settings_mutex);
    kind = g_working_color_space_kind;
    icc = g_working_color_space_icc;
    if (icc.empty())
      icc = kind == ColorProfileKind::LinearSrgb ? color_settings_builtin_linear_icc()
                                                 : color_settings_builtin_srgb_icc();
  } catch (...) { return 4; }
  return color_settings_create_profile(plugin_id, kind, icc, profile);
}
int32_t __cdecl color_get_new_profile_from_icc(int32_t plugin_id, int32_t icc_size,
                                               const void* icc_data, void** profile) {
  if (profile) *profile = nullptr;
  if (plugin_id != 1 || !profile || !icc_data || icc_size <= 0 ||
      static_cast<std::size_t>(icc_size) > kMaxIccProfileBytes) {
    ++g_invalid_color_profile_operations; return 4;
  }
  if (!color_settings_validate_icc(icc_data, icc_size)) { ++g_invalid_color_profile_operations; return 4; }
  try {
    std::vector<uint8_t> icc(static_cast<const uint8_t*>(icc_data),
                             static_cast<const uint8_t*>(icc_data) + icc_size);
    return color_settings_create_profile(plugin_id, color_settings_classify_icc(icc), icc, profile);
  } catch (...) { return 4; }
}
int32_t __cdecl color_get_new_icc_from_profile(int32_t plugin_id, void* profile, void** icc_handle) {
  if (icc_handle) *icc_handle = nullptr;
  ColorProfileRecord record{};
  if (plugin_id != 1 || !icc_handle || !color_settings_snapshot_profile(profile, record)) {
    ++g_invalid_color_profile_operations; return 4;
  }
  if (new_aegp_mem_handle(plugin_id, "color profile icc", static_cast<uint32_t>(record.icc_bytes.size()),
                          1, icc_handle) != 0) return 4;
  void* bytes = nullptr;
  if (lock_aegp_mem_handle(*icc_handle, &bytes) != 0 || !bytes) {
    free_aegp_mem_handle(*icc_handle); *icc_handle = nullptr; return 4;
  }
  std::memcpy(bytes, record.icc_bytes.data(), record.icc_bytes.size());
  return unlock_aegp_mem_handle(*icc_handle);
}
int32_t __cdecl color_get_new_profile_description(int32_t plugin_id, void* profile, void** desc_handle) {
  if (desc_handle) *desc_handle = nullptr;
  ColorProfileRecord record{};
  if (plugin_id != 1 || !desc_handle || !color_settings_snapshot_profile(profile, record)) {
    ++g_invalid_color_profile_operations; return 4;
  }
  return make_utf16_handle(color_settings_profile_description(record.kind),
                           "color profile description", desc_handle);
}
int32_t __cdecl color_dispose_profile(void* profile) {
  if (!profile) { ++g_invalid_color_profile_operations; return 4; }
  std::lock_guard<std::mutex> lock(g_color_settings_mutex);
  const auto found = g_color_profiles.find(profile);
  if (found == g_color_profiles.end() || !found->second.live) {
    ++g_invalid_color_profile_operations; return 4;
  }
  found->second.live = false;
  g_color_profiles.erase(found);
  ++g_color_profiles_disposed;
  return 0;
}
int32_t __cdecl color_get_profile_approximate_gamma(void* profile, float* gamma) {
  if (!gamma) return 4;
  *gamma = 0.0f;
  ColorProfileRecord record{};
  if (!color_settings_snapshot_profile(profile, record)) { ++g_invalid_color_profile_operations; return 4; }
  if (record.kind == ColorProfileKind::ImportedRgb) return 4;
  *gamma = record.kind == ColorProfileKind::LinearSrgb ? 1.0f : 2.2f;
  return 0;
}
int32_t __cdecl color_is_rgb_profile(void* profile, uint8_t* is_rgb) {
  if (!is_rgb) return 4;
  *is_rgb = 0;
  ColorProfileRecord record{};
  if (!color_settings_snapshot_profile(profile, record)) { ++g_invalid_color_profile_operations; return 4; }
  *is_rgb = 1;
  return 0;
}
int32_t __cdecl color_set_working_color_space(int32_t plugin_id, void* comp, void* profile) {
  ColorProfileRecord record{};
  if (plugin_id != 1 || comp != composition_handle() || !color_settings_snapshot_profile(profile, record))
    return 4;
  std::lock_guard<std::mutex> lock(g_color_settings_mutex);
  try {
    g_working_color_space_kind = record.kind;
    g_working_color_space_icc = record.icc_bytes;
  } catch (...) { return 4; }
  return 0;
}
int32_t __cdecl color_is_ocio_used(int32_t plugin_id, uint8_t* used) {
  if (!used) return 4;
  *used = 0;
  if (plugin_id != 1) return 4;
  return 0;
}
int32_t __cdecl color_get_ocio_configuration_file(int32_t plugin_id, void** config_handle) {
  if (config_handle) *config_handle = nullptr;
  if (plugin_id != 1 || !config_handle) return 4;
  return color_settings_init_empty_utf16_handle(config_handle);
}
int32_t __cdecl color_get_ocio_configuration_file_path(int32_t plugin_id, void** path_handle) {
  if (path_handle) *path_handle = nullptr;
  if (plugin_id != 1 || !path_handle) return 4;
  return color_settings_init_empty_utf16_handle(path_handle);
}
int32_t __cdecl color_get_ocio_working_colorspace(int32_t plugin_id, void** working_handle) {
  if (working_handle) *working_handle = nullptr;
  if (plugin_id != 1 || !working_handle) return 4;
  return color_settings_init_empty_utf16_handle(working_handle);
}
int32_t __cdecl color_get_ocio_display_colorspace(int32_t plugin_id, void** display_handle,
                                                  void** view_handle) {
  if (display_handle) *display_handle = nullptr;
  if (view_handle) *view_handle = nullptr;
  if (plugin_id != 1 || !display_handle || !view_handle) return 4;
  if (color_settings_init_empty_utf16_handle(display_handle) != 0) return 4;
  return color_settings_init_empty_utf16_handle(view_handle);
}
int32_t __cdecl color_is_colorspace_aware_effects_enabled(int32_t plugin_id, uint8_t* enabled) {
  if (!enabled) return 4;
  *enabled = 0;
  if (plugin_id != 1) return 4;
  return 0;
}
int32_t __cdecl color_get_lut_interpolation_method(int32_t plugin_id, uint16_t* method) {
  if (!method) return 4;
  *method = 0;
  if (plugin_id != 1) return 4;
  return 0;
}
int32_t __cdecl color_get_graphics_white_luminance(int32_t plugin_id, uint16_t* luminance) {
  if (!luminance) return 4;
  *luminance = 0;
  if (plugin_id != 1) return 4;
  return 0;
}
int32_t __cdecl color_get_working_colorspace_id(int32_t plugin_id, AegpGuidValue* guid) {
  if (!guid) return 4;
  guid->bytes = {};
  if (plugin_id != 1) return 4;
  ColorProfileKind kind{};
  { std::lock_guard<std::mutex> lock(g_color_settings_mutex); kind = g_working_color_space_kind; }
  guid->bytes = kind == ColorProfileKind::LinearSrgb ? kWorkingLinearSrgbGuid : kWorkingSrgbGuid;
  return 0;
}

bool world_lifetimes_balanced();

bool verify_aegp_world_suite3() {
  int32_t null_value = 0;
  if (aegp_world_get_type(nullptr, &null_value) == 0 ||
      aegp_world_get_type(reinterpret_cast<void**>(1), nullptr) == 0 ||
      aegp_world_get_size(nullptr, &null_value, &null_value) == 0 ||
      aegp_world_get_rowbytes(nullptr, nullptr) == 0 ||
      aegp_world_fill_pf_world(nullptr, nullptr) == 0) return false;
  const std::array<int32_t, 3> formats{
      kPixelFormatArgb32, kPixelFormatArgb64, kPixelFormatArgb128};
  for (std::size_t index = 0; index < formats.size(); ++index) {
    std::array<std::byte, kEffectWorldSize> storage{};
    if (new_world(nullptr, 7, 5, 1, formats[index], storage.data()) != 0) return false;
    void* token = storage.data();
    void** handle = &token;
    if (!aexcompat::world_registry::register_borrowed_view(
            handle, storage.data(), formats[index], false)) return false;
    int32_t type = 0, width = 0, height = 0;
    uint32_t rowbytes = 0;
    void* pixels = nullptr;
    std::array<std::byte, kEffectWorldSize> projection{};
    const bool metadata_ok = aegp_world_get_type(handle, &type) == 0 &&
        type == static_cast<int32_t>(index + 1) &&
        aegp_world_get_size(handle, &width, &height) == 0 && width == 7 && height == 5 &&
        aegp_world_get_rowbytes(handle, &rowbytes) == 0 &&
        rowbytes == 7U * static_cast<uint32_t>(4U << index) &&
        aegp_world_fill_pf_world(handle, projection.data()) == 0 &&
        std::memcmp(projection.data(), storage.data(), kEffectWorldSize) == 0;
    const int32_t correct_addr = index == 0 ? aegp_world_get_base_addr8(handle, &pixels) :
        (index == 1 ? aegp_world_get_base_addr16(handle, &pixels) :
                      aegp_world_get_base_addr32(handle, &pixels));
    void* wrong_pixels = reinterpret_cast<void*>(1);
    const int32_t wrong_addr = index == 0 ? aegp_world_get_base_addr16(handle, &wrong_pixels) :
        aegp_world_get_base_addr8(handle, &wrong_pixels);
    LocalEffectWorld world{};
    std::memcpy(&world, storage.data(), sizeof(world));
    const bool addresses_ok = correct_addr == 0 && pixels == world.data && wrong_addr != 0 &&
        wrong_pixels == reinterpret_cast<void*>(1);
    if (aexcompat::world_registry::unregister_borrowed_view(handle) !=
        aexcompat::world_registry::UnregisterBorrowedViewResult::removed) return false;
    const bool stale_rejected = aegp_world_get_type(handle, &type) != 0;
    if (dispose_world(nullptr, storage.data()) != 0 || !metadata_ok || !addresses_ok ||
        !stale_rejected) return false;
  }

  std::array<std::byte, kEffectWorldSize> borrowed{};
  std::array<std::byte, 16> borrowed_pixels{};
  auto* world = reinterpret_cast<LocalEffectWorld*>(borrowed.data());
  world->world_flags = 2;
  world->data = borrowed_pixels.data();
  world->rowbytes = 8;
  world->width = 2;
  world->height = 2;
  void* token = borrowed.data();
  void** handle = &token;
  if (!aexcompat::world_registry::register_borrowed_view(
          handle, borrowed.data(), kPixelFormatArgb32)) return false;
  int32_t width = 0, height = 0;
  const bool borrowed_live = aegp_world_get_size(handle, &width, &height) == 0 &&
      width == 2 && height == 2 && aegp_world_dispose(handle) != 0;
  if (aexcompat::world_registry::unregister_borrowed_view(handle) !=
      aexcompat::world_registry::UnregisterBorrowedViewResult::removed) return false;
  if (!borrowed_live || aegp_world_get_size(handle, &width, &height) == 0) return false;

  for (int32_t type = 1; type <= 3; ++type) {
    void** owned = nullptr;
    void* base = nullptr;
    LocalEffectWorld projection{};
    int32_t actual_type = 0;
    uint32_t rowbytes = 0;
    if (aegp_world_new_owned(1, type, 4, 3, &owned) != 0 || !owned ||
        aegp_world_get_type(owned, &actual_type) != 0 || actual_type != type ||
        aegp_world_get_size(owned, &width, &height) != 0 || width != 4 || height != 3 ||
        aegp_world_get_rowbytes(owned, &rowbytes) != 0 ||
        rowbytes != static_cast<uint32_t>(4 * (4 << (type - 1))) ||
        aegp_world_fill_pf_world(owned, &projection) != 0 ||
        projection.width != 4 || projection.height != 3 ||
        (type == 1 ? aegp_world_get_base_addr8(owned, &base) :
         (type == 2 ? aegp_world_get_base_addr16(owned, &base) :
                      aegp_world_get_base_addr32(owned, &base))) != 0 ||
        !base || aegp_world_dispose(owned) != 0 ||
        aegp_world_get_type(owned, &actual_type) == 0 || aegp_world_dispose(owned) == 0)
      return false;
  }
  void** blur_world = nullptr;
  void* blur_base = nullptr;
  if (aegp_world_new_owned(1, 1, 5, 5, &blur_world) != 0 ||
      aegp_world_get_base_addr8(blur_world, &blur_base) != 0 || !blur_base) return false;
  auto* blur_pixels = static_cast<uint8_t*>(blur_base);
  std::fill(blur_pixels, blur_pixels + 5 * 5 * 4, 0);
  for (int channel = 0; channel < 4; ++channel) blur_pixels[(2 * 5 + 2) * 4 + channel] = 255;
  std::array<uint8_t, 5 * 5 * 4> unchanged{};
  std::memcpy(unchanged.data(), blur_pixels, unchanged.size());
  if (aegp_world_fast_blur(0.0, 0, 1, blur_world) != 0 ||
      std::memcmp(unchanged.data(), blur_pixels, unchanged.size()) != 0 ||
      aegp_world_fast_blur(1.0, 0, 1, blur_world) != 0 ||
      blur_pixels[(2 * 5 + 2) * 4] >= 255 || blur_pixels[(2 * 5 + 1) * 4] == 0 ||
      aegp_world_fast_blur(-1.0, 0, 1, blur_world) == 0 ||
      aegp_world_fast_blur(1.0, 2, 1, blur_world) == 0 ||
      aegp_world_dispose(blur_world) != 0 ||
      aegp_world_fast_blur(1.0, 0, 1, blur_world) == 0) return false;

  void* platform = nullptr;
  void** reference = nullptr;
  void* pixels = nullptr;
  if (aegp_world_new_platform(1, 1, 3, 2, &platform) != 0 || !platform ||
      aegp_world_reference_platform(1, platform, &reference) != 0 || !reference ||
      aegp_world_get_base_addr8(reference, &pixels) != 0 || !pixels ||
      aegp_world_dispose_platform(platform) != 0 ||
      aegp_world_get_size(reference, &width, &height) != 0 || width != 3 || height != 2 ||
      aegp_world_dispose_platform(platform) == 0 ||
      aegp_world_dispose(reference) != 0 ||
      aegp_world_get_size(reference, &width, &height) == 0 ||
      aegp_world_dispose(reference) == 0) return false;

  void* options = nullptr;
  AegpTimeStamp timestamp{};
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &options) != 0 ||
      render_timestamp_reject(&timestamp) != 0 ||
      aegp_world_new_platform(1, 1, kSyntheticCompWidth, kSyntheticCompHeight,
                              &platform) != 0 ||
      aegp_world_reference_platform(1, platform, &reference) != 0 ||
      aegp_world_get_base_addr8(reference, &pixels) != 0 || !pixels)
    return false;
  const std::array<uint8_t, 4> external_sentinel{{231, 17, 91, 203}};
  std::memcpy(pixels, external_sentinel.data(), external_sentinel.size());
  if (
      render_checkin_rendered(options, &timestamp, 1, platform) != 0 ||
      aegp_world_dispose_platform(platform) == 0) return false;
  uint8_t worthwhile = 1;
  if (render_worthwhile_reject(options, &timestamp, &worthwhile) != 0 || worthwhile != 0)
    return false;
  void* cached_receipt = nullptr;
  void** cached_world = nullptr;
  void* cached_pixels = nullptr;
  if (render_checkout_frame_reject(options, nullptr, nullptr, &cached_receipt) != 0 ||
      !cached_receipt || get_receipt_world(cached_receipt, &cached_world) != 0 ||
      aegp_world_get_base_addr8(cached_world, &cached_pixels) != 0 || !cached_pixels ||
      std::memcmp(cached_pixels, external_sentinel.data(), external_sentinel.size()) != 0 ||
      checkin_frame(cached_receipt) != 0 || aegp_world_dispose(reference) != 0)
    return false;
  bump_render_project_timestamp();
  if (aegp_world_new_platform(1, 1, 1, 1, &platform) != 0 ||
      render_checkin_rendered(options, &timestamp, 1, platform) == 0 ||
      aegp_world_dispose_platform(platform) != 0 || render_options_dispose(options) != 0)
    return false;
  return world_lifetimes_balanced() && g_external_render_cache.empty() &&
      aexcompat::world_registry::aegp_lifetimes_balanced();
}

bool verify_aegp_world_mfr_safety() {
  void** world = nullptr;
  if (aegp_world_new_owned(1, 1, 16, 16, &world) != 0 || !world) return false;
  std::atomic_bool start{false};
  std::atomic_bool invalid{false};
  std::vector<std::thread> readers;
  try {
    for (int worker = 0; worker < 4; ++worker) {
      readers.emplace_back([&] {
        while (!start.load(std::memory_order_acquire)) std::this_thread::yield();
        for (int iteration = 0; iteration < 256; ++iteration) {
          int32_t type = 0, width = 0, height = 0;
          uint32_t rowbytes = 0;
          if (aegp_world_get_type(world, &type) != 0 || type != 1 ||
              aegp_world_get_size(world, &width, &height) != 0 ||
              width != 16 || height != 16 ||
              aegp_world_get_rowbytes(world, &rowbytes) != 0 || rowbytes != 64)
            invalid.store(true, std::memory_order_release);
        }
      });
    }
  } catch (...) {
    start.store(true, std::memory_order_release);
    for (auto& reader : readers) reader.join();
    aegp_world_dispose(world);
    return false;
  }
  start.store(true, std::memory_order_release);
  for (int iteration = 0; iteration < 16; ++iteration) {
    if (aegp_world_fast_blur(0.25, 0, iteration & 1, world) != 0)
      invalid.store(true, std::memory_order_release);
  }
  for (auto& reader : readers) reader.join();
  if (invalid.load(std::memory_order_acquire) ||
      aegp_world_dispose(world) != 0 ||
      !aexcompat::world_registry::aegp_lifetimes_balanced()) return false;

  std::array<aexcompat::world_registry::AegpWorldSnapshot, 64> pins{};
  for (auto& pin : pins) {
    void** pinned_world = nullptr;
    if (aegp_world_new_owned(1, 1, 1, 1, &pinned_world) != 0 ||
        !aexcompat::world_registry::snapshot_aegp_world(pinned_world, pin) ||
        !pin.backing_pin || aegp_world_dispose(pinned_world) != 0) return false;
  }
  void** rejected = reinterpret_cast<void**>(1);
  if (aegp_world_new_owned(1, 1, 1, 1, &rejected) == 0 || rejected != nullptr)
    return false;
  pins[0] = {};
  void** admitted = nullptr;
  if (aegp_world_new_owned(1, 1, 1, 1, &admitted) != 0 || !admitted ||
      aegp_world_dispose(admitted) != 0) return false;
  pins = {};
  return aexcompat::world_registry::aegp_lifetimes_balanced();
}

bool async_receipt_lifetimes_balanced() {
  return aexcompat::render_receipts::lifetimes_balanced();
}

bool verify_aegp_async_receipts() {
  struct SyntheticReceiptScope {
    SyntheticReceiptScope() { g_synthetic_receipt_test_mode = true; }
    ~SyntheticReceiptScope() { g_synthetic_receipt_test_mode = false; }
  } synthetic_receipt_scope;
  if (!async_receipt_lifetimes_balanced()) return false;
  const std::array<int32_t, 3> formats{
      kPixelFormatArgb32, kPixelFormatArgb64, kPixelFormatArgb128};
  std::unordered_set<void*> receipt_handles;
  std::unordered_set<void**> world_handles;
  for (std::size_t index = 0; index < formats.size(); ++index) {
    void* receipt = reinterpret_cast<void*>(1);
    if (publish_async_receipt(formats[index], &receipt) != 0 || !receipt) return false;
    void** world = nullptr;
    if (get_receipt_world(receipt, &world) != 0 || !world) return false;
    if (!receipt_handles.insert(receipt).second || !world_handles.insert(world).second)
      return false;
    int32_t type = 0, width = 0, height = 0;
    void* pixels = nullptr;
    void* wrong_pixels = reinterpret_cast<void*>(1);
    const int32_t correct = index == 0 ? aegp_world_get_base_addr8(world, &pixels) :
        (index == 1 ? aegp_world_get_base_addr16(world, &pixels) :
                      aegp_world_get_base_addr32(world, &pixels));
    const int32_t wrong = index == 0 ? aegp_world_get_base_addr16(world, &wrong_pixels) :
        aegp_world_get_base_addr8(world, &wrong_pixels);
    if (aegp_world_get_type(world, &type) != 0 || type != static_cast<int32_t>(index + 1) ||
        aegp_world_get_size(world, &width, &height) != 0 || width != 8 || height != 4 ||
        correct != 0 || !pixels || wrong == 0 || wrong_pixels != reinterpret_cast<void*>(1) ||
        aegp_world_dispose(world) == 0) return false;
    void** stale_world = world;
    if (checkin_frame(receipt) != 0 || get_receipt_world(receipt, &world) == 0 ||
        aegp_world_get_type(stale_world, &type) == 0 || checkin_frame(receipt) == 0) return false;
  }

  AegpLayerRenderOptionsValue layer_options{};
  void* options = nullptr;
  if (insert_layer_render_options(layer_options, &options) != 0 || !options) return false;
  void* receipt = nullptr;
  if (checkout_layer_frame_async(&g_async_manager, 1, options, &receipt) != 0 ||
      !receipt || dispose_layer_render_options(options) != 0) return false;
  void** world = nullptr;
  int32_t width = 0, height = 0;
  const bool survives_options = get_receipt_world(receipt, &world) == 0 && world &&
      aegp_world_get_size(world, &width, &height) == 0 && width == 8 && height == 4;
  return survives_options && checkin_frame(receipt) == 0 &&
      async_receipt_lifetimes_balanced();
}

bool render_options_lifetimes_balanced() {
  return item_live_count() == 0 && item_created_count() == item_disposed_count();
}

bool verify_aegp_render_options_suite1() {
  struct SyntheticReceiptScope {
    SyntheticReceiptScope() { g_synthetic_receipt_test_mode = true; }
    ~SyntheticReceiptScope() { g_synthetic_receipt_test_mode = false; }
  } synthetic_receipt_scope;
  if (!render_options_lifetimes_balanced() || !async_receipt_lifetimes_balanced()) return false;
  const auto render_probe = [](AegpRenderOptionsValue options, int32_t type,
                               int32_t sample_x, int32_t sample_y, void* output) {
    ReceiptDraft probe{};
    probe.render_options = options;
    probe.rendered_region = options.roi.right == 0
        ? AegpRect{0, 0, kSyntheticCompWidth, kSyntheticCompHeight} : options.roi;
    const int32_t width = (kSyntheticCompWidth + options.downsample_x - 1) /
        options.downsample_x;
    const int32_t height = (kSyntheticCompHeight + options.downsample_y - 1) /
        options.downsample_y;
    const int32_t pixel_bytes = type == 1 ? 4 : (type == 2 ? 8 : 16);
    probe.pixels.resize(static_cast<std::size_t>(width) * height * pixel_bytes);
    populate_synthetic_item_pixels(probe, type, width, height);
    std::memcpy(output, probe.pixels.data() +
        (static_cast<std::size_t>(sample_y) * width + sample_x) * pixel_bytes, pixel_bytes);
  };
  AegpRenderOptionsValue pixel_options{};
  pixel_options.time = {7, 30};
  render_probe(pixel_options, 1, 0, 0, g_render_options_baseline8.data());
  pixel_options.time = {1, 32};
  render_probe(pixel_options, 1, 0, 0, g_render_options_time8.data());
  pixel_options.time = {7, 30};
  pixel_options.downsample_x = 3; pixel_options.downsample_y = 2;
  render_probe(pixel_options, 1, 1, 1, g_render_options_downsample8.data());
  pixel_options = AegpRenderOptionsValue{}; pixel_options.time = {7, 30};
  pixel_options.roi = {1, 0, 17, 9};
  render_probe(pixel_options, 1, 0, 0, g_render_options_roi_outside8.data());
  render_probe(pixel_options, 1, 1, 0, g_render_options_roi_inside8.data());
  pixel_options.roi = {}; pixel_options.field = 1;
  render_probe(pixel_options, 1, 0, 1, g_render_options_field_excluded8.data());
  pixel_options.field = 0; pixel_options.matte = 1;
  render_probe(pixel_options, 1, 0, 0, g_render_options_matte8.data());
  pixel_options.matte = 0;
  render_probe(pixel_options, 2, 0, 0, g_render_options_argb16.data());
  render_probe(pixel_options, 3, 0, 0, g_render_options_argb32f.data());
  const std::array<uint8_t, 4> baseline{{123, 59, 177, 157}};
  const std::array<float, 4> expected_float{{123.0f / 255.0f, 59.0f / 255.0f,
                                             177.0f / 255.0f, 157.0f / 255.0f}};
  if (g_render_options_baseline8 != baseline ||
      g_render_options_time8 != std::array<uint8_t, 4>{{72, 8, 24, 56}} ||
      g_render_options_downsample8 != std::array<uint8_t, 4>{{170, 110, 235, 198}} ||
      g_render_options_roi_outside8 != std::array<uint8_t, 4>{} ||
      g_render_options_roi_inside8 != std::array<uint8_t, 4>{{134, 76, 177, 162}} ||
      g_render_options_field_excluded8 != std::array<uint8_t, 4>{} ||
      g_render_options_matte8 != std::array<uint8_t, 4>{{255, 59, 177, 157}} ||
      g_render_options_argb16 != std::array<uint16_t, 4>{{31611, 15163, 45489, 40349}} ||
      g_render_options_argb32f != expected_float) return false;
  void* options = nullptr;
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &options) != 0 || !options) return false;
  AegpTime time{}, step{};
  int32_t field = -1, world_type = -1, matte = -1;
  int16_t dx = 0, dy = 0;
  int8_t channel_order = -1, render_quality = -1;
  uint8_t guide_layers = 2;
  AegpRect roi{-1, -1, -1, -1};
  const bool defaults = render_options_get_time(options, &time) == 0 &&
      time.value == 0 && time.scale == 1 &&
      render_options_get_time_step(options, &step) == 0 && step.value == 1 && step.scale == 30 &&
      render_options_get_field(options, &field) == 0 && field == 0 &&
      render_options_get_world_type(options, &world_type) == 0 && world_type == 1 &&
      render_options_get_downsample(options, &dx, &dy) == 0 && dx == 1 && dy == 1 &&
      render_options_get_roi(options, &roi) == 0 && roi.left == 0 && roi.top == 0 &&
      roi.right == 0 && roi.bottom == 0 &&
      render_options_get_matte(options, &matte) == 0 && matte == 0 &&
      render_options_get_channel_order(options, &channel_order) == 0 && channel_order == 0 &&
      render_options_get_guide_layers(options, &guide_layers) == 0 && guide_layers == 0 &&
      render_options_get_quality(options, &render_quality) == 0 && render_quality == 1;
  const AegpRect selected_roi{1, 2, 16, 8};
  if (!defaults || render_options_set_time(options, {-5, 24}) != 0 ||
      render_options_set_time_step(options, {2, 48}) != 0 ||
      render_options_set_field(options, 2) != 0 ||
      render_options_set_world_type(options, 2) != 0 ||
      render_options_set_downsample(options, 3, 2) != 0 ||
      render_options_set_roi(options, &selected_roi) != 0 ||
      render_options_set_matte(options, 1) != 0 ||
      render_options_set_channel_order(options, 1) != 0 ||
      render_options_set_guide_layers(options, 1) != 0 ||
      render_options_set_quality(options, 0) != 0) return false;
  if (render_options_get_time(options, &time) != 0 || time.value != -5 || time.scale != 24 ||
      render_options_get_time_step(options, &step) != 0 || step.value != 2 || step.scale != 48 ||
      render_options_get_field(options, &field) != 0 || field != 2 ||
      render_options_get_world_type(options, &world_type) != 0 || world_type != 2 ||
      render_options_get_downsample(options, &dx, &dy) != 0 || dx != 3 || dy != 2 ||
      render_options_get_roi(options, &roi) != 0 || roi.left != 1 || roi.bottom != 8 ||
      render_options_get_matte(options, &matte) != 0 || matte != 1 ||
      render_options_get_channel_order(options, &channel_order) != 0 || channel_order != 1 ||
      render_options_get_guide_layers(options, &guide_layers) != 0 || guide_layers != 1 ||
      render_options_get_quality(options, &render_quality) != 0 || render_quality != 0)
    return false;
  void* duplicate = nullptr;
  if (render_options_duplicate(1, options, &duplicate) != 0 || !duplicate ||
      render_options_set_time(duplicate, {7, 30}) != 0 ||
      render_options_get_time(options, &time) != 0 || time.value != -5 || time.scale != 24)
    return false;

  void* receipt = nullptr;
  if (checkout_item_frame_async(&g_async_manager, 1, options, &receipt) != 0 || !receipt ||
      render_options_dispose(options) != 0 || render_options_dispose(options) == 0) return false;
  void** world = nullptr;
  int32_t width = 0, height = 0, type = 0;
  AegpRect rendered{};
  ReceiptSnapshot receipt_snapshot{};
  const bool snapshot_ok =
      aexcompat::render_receipts::snapshot(receipt, receipt_snapshot) &&
      receipt_snapshot.has_render_options &&
      receipt_snapshot.render_options.time.value == -5 &&
      receipt_snapshot.render_options.time_step.value == 2 &&
      receipt_snapshot.render_options.field == 2 &&
      receipt_snapshot.render_options.matte == 1 &&
      receipt_snapshot.render_options.channel_order == 1 &&
      receipt_snapshot.render_options.render_guide_layers == 1 &&
      receipt_snapshot.render_options.render_quality == 0 &&
      receipt_snapshot.rendered_region.left == 1 &&
      receipt_snapshot.rendered_region.top == 1;
  if (!snapshot_ok || get_receipt_world(receipt, &world) != 0 ||
      aegp_world_get_type(world, &type) != 0 || type != 2 ||
      aegp_world_get_size(world, &width, &height) != 0 || width != 6 || height != 5 ||
      render_get_region_reject(receipt, &rendered) != 0 || rendered.right != 6 ||
      rendered.bottom != 4 ||
      checkin_frame(receipt) != 0) return false;

  for (int32_t depth = 1; depth <= 3; ++depth) {
    if (render_options_set_world_type(duplicate, depth) != 0 ||
        render_options_set_downsample(duplicate, static_cast<int16_t>(depth + 1), 4) != 0)
      return false;
    receipt = nullptr;
    if (render_checkout_frame_reject(duplicate, nullptr, nullptr, &receipt) != 0 || !receipt ||
        get_receipt_world(receipt, &world) != 0 || aegp_world_get_type(world, &type) != 0 ||
        type != depth || aegp_world_get_size(world, &width, &height) != 0 ||
        width != (kSyntheticCompWidth + depth) / (depth + 1) || height != 3 ||
        checkin_frame(receipt) != 0) return false;
  }
  if (render_options_set_matte(duplicate, 2) != 0) return false;
  receipt = reinterpret_cast<void*>(1);
  if (render_checkout_frame_reject(duplicate, nullptr, nullptr, &receipt) == 0 || receipt != nullptr ||
      render_options_set_time(duplicate, {1, 0}) == 0 ||
      render_options_set_time_step(duplicate, {0, 1}) == 0 ||
      render_options_set_field(duplicate, 3) == 0 ||
      render_options_set_world_type(duplicate, 0) == 0 ||
      render_options_set_downsample(duplicate, 0, 1) == 0 ||
      render_options_set_matte(duplicate, 3) == 0 ||
      render_options_set_channel_order(duplicate, 2) == 0 ||
      render_options_set_guide_layers(duplicate, 2) == 0 ||
      render_options_set_quality(duplicate, -1) == 0) return false;
  const AegpRect invalid_roi{2, 2, 1, 1};
  if (render_options_set_roi(duplicate, &invalid_roi) == 0) return false;
  if (render_options_dispose(duplicate) != 0 || render_options_get_time(duplicate, &time) == 0)
    return false;

  std::array<void*, kMaxRenderOptions> capacity_handles{};
  for (auto& handle : capacity_handles) {
    if (render_options_new_from_item(1, aegp_comp_item_handle(), &handle) != 0 || !handle)
      return false;
  }
  void* overflow = reinterpret_cast<void*>(1);
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &overflow) == 0 ||
      overflow != nullptr) return false;
  const void* stale_capacity_handle = capacity_handles.front();
  for (void* handle : capacity_handles) {
    if (render_options_dispose(handle) != 0) return false;
  }
  void* replacement = nullptr;
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &replacement) != 0 ||
      replacement == stale_capacity_handle || render_options_get_time(
          const_cast<void*>(stale_capacity_handle), &time) == 0 ||
      render_options_dispose(replacement) != 0) return false;

  void* invalid_output = reinterpret_cast<void*>(1);
  if (render_options_new_from_item(2, aegp_comp_item_handle(), &invalid_output) == 0 ||
      invalid_output != nullptr ||
      render_options_new_from_item(1, nullptr, &invalid_output) == 0 ||
      invalid_output != nullptr || render_options_duplicate(2, nullptr, &invalid_output) == 0 ||
      invalid_output != nullptr) return false;

  std::atomic<bool> concurrent_ok{true};
  std::array<std::thread, 3> threads;
  for (int index = 0; index < 3; ++index) {
    threads[index] = std::thread([index, &concurrent_ok] {
      void* local = nullptr;
      void* local_receipt = nullptr;
      void** local_world = nullptr;
      int32_t local_type = 0;
      if (render_options_new_from_item(1, aegp_comp_item_handle(), &local) != 0 ||
          render_options_set_world_type(local, index + 1) != 0 ||
          publish_item_receipt(local, &local_receipt) != 0 ||
          render_options_dispose(local) != 0 ||
          get_receipt_world(local_receipt, &local_world) != 0 ||
          aegp_world_get_type(local_world, &local_type) != 0 || local_type != index + 1 ||
          checkin_frame(local_receipt) != 0) concurrent_ok = false;
    });
  }
  for (auto& thread : threads) thread.join();
  return concurrent_ok && render_options_lifetimes_balanced() &&
      async_receipt_lifetimes_balanced();
}

bool verify_aegp_item_staged_worlds() {
  if (!render_options_lifetimes_balanced() || !async_receipt_lifetimes_balanced())
    return false;
  {
    std::lock_guard<std::mutex> lock(g_staged_item_world_mutex);
    g_staged_item_worlds.clear();
  }
  const AegpTime time{5, 24}, step{1, 24};
  constexpr int32_t width = 5, height = 4;
  auto fixture = [=](int32_t pixel_bytes) {
    std::vector<std::byte> pixels(width * height * pixel_bytes);
    for (int32_t y = 0; y < height; ++y) for (int32_t x = 0; x < width; ++x) {
      const std::array<float, 4> value{{(64 + x * 17 + y * 9) / 255.0f,
          (11 + x * 31) / 255.0f, (23 + y * 47) / 255.0f,
          (37 + x * 13 + y * 19) / 255.0f}};
      std::byte* pixel = pixels.data() + (static_cast<std::size_t>(y) * width + x) *
          pixel_bytes;
      for (int channel = 0; channel < 4; ++channel)
        write_staged_channel(pixel, pixel_bytes, channel, value[channel]);
    }
    return pixels;
  };
  const std::array<int32_t, 3> formats{{
      kPixelFormatArgb32, kPixelFormatArgb64, kPixelFormatArgb128}};
  for (int32_t depth = 0; depth < 3; ++depth) {
    const int32_t pixel_bytes = 4 << depth;
    const auto pixels = fixture(pixel_bytes);
    if (!publish_staged_item_world(aegp_comp_item_handle(), time, step, 1, 0,
            formats[depth], width, height, width * pixel_bytes, pixels.data())) return false;
  }
  void* options = nullptr;
  if (render_options_new_from_item(1, aegp_comp_item_handle(), &options) != 0 ||
      render_options_set_time(options, time) != 0 ||
      render_options_set_time_step(options, step) != 0) return false;
  auto checkout = [&](int32_t world_type, int16_t dx, int16_t dy, AegpRect roi,
                      int32_t field, int32_t matte, int8_t order,
                      void** out_receipt, void*** out_world) {
    return render_options_set_world_type(options, world_type) == 0 &&
        render_options_set_downsample(options, dx, dy) == 0 &&
        render_options_set_roi(options, &roi) == 0 &&
        render_options_set_field(options, field) == 0 &&
        render_options_set_matte(options, matte) == 0 &&
        render_options_set_channel_order(options, order) == 0 &&
        render_checkout_frame_reject(options, nullptr, nullptr, out_receipt) == 0 &&
        *out_receipt && get_receipt_world(*out_receipt, out_world) == 0 && *out_world;
  };
  for (int32_t depth = 1; depth <= 3; ++depth) {
    void* receipt = nullptr;
    void** world = nullptr;
    if (!checkout(depth, 1, 1, {}, 0, 0, 0, &receipt, &world)) return false;
    int32_t type = 0, actual_width = 0, actual_height = 0;
    void* pixels = nullptr;
    const int32_t base_error = depth == 1 ? aegp_world_get_base_addr8(world, &pixels) :
        (depth == 2 ? aegp_world_get_base_addr16(world, &pixels) :
                      aegp_world_get_base_addr32(world, &pixels));
    if (aegp_world_get_type(world, &type) != 0 || type != depth ||
        aegp_world_get_size(world, &actual_width, &actual_height) != 0 ||
        actual_width != width || actual_height != height || base_error != 0 || !pixels ||
        checkin_frame(receipt) != 0) return false;
  }
  const auto verify_pixel8 = [&](int32_t matte, int8_t order,
                                  std::array<uint8_t, 4> expected) {
    void* receipt = nullptr;
    void** world = nullptr;
    void* pixels = nullptr;
    const bool valid = checkout(1, 1, 1, {}, 0, matte, order, &receipt, &world) &&
        aegp_world_get_base_addr8(world, &pixels) == 0 && pixels &&
        std::memcmp(pixels, expected.data(), expected.size()) == 0;
    return receipt && checkin_frame(receipt) == 0 && valid;
  };
  if (!verify_pixel8(0, 0, {{64, 11, 23, 37}}) ||
      !verify_pixel8(0, 1, {{37, 23, 11, 64}}) ||
      !verify_pixel8(1, 0, {{64, 3, 6, 9}}) ||
      !verify_pixel8(2, 0, {{64, 39, 42, 45}})) return false;
  void* bgra16_receipt = nullptr;
  void** bgra16_world = nullptr;
  void* bgra16_pixels = nullptr;
  if (!checkout(2, 1, 1, {}, 0, 0, 1, &bgra16_receipt, &bgra16_world) ||
      aegp_world_get_base_addr16(bgra16_world, &bgra16_pixels) != 0 || !bgra16_pixels ||
      std::memcmp(bgra16_pixels,
          std::array<uint16_t, 4>{{4755, 2956, 1414, 8224}}.data(), 8) != 0 ||
      checkin_frame(bgra16_receipt) != 0) return false;
  void* bgra32_receipt = nullptr;
  void** bgra32_world = nullptr;
  void* bgra32_pixels = nullptr;
  if (!checkout(3, 1, 1, {}, 0, 0, 1, &bgra32_receipt, &bgra32_world) ||
      aegp_world_get_base_addr32(bgra32_world, &bgra32_pixels) != 0 || !bgra32_pixels)
    return false;
  const auto* bgra32 = static_cast<const float*>(bgra32_pixels);
  if (std::abs(bgra32[0] - 37.0f / 255.0f) > 1e-7f ||
      std::abs(bgra32[1] - 23.0f / 255.0f) > 1e-7f ||
      std::abs(bgra32[2] - 11.0f / 255.0f) > 1e-7f ||
      std::abs(bgra32[3] - 64.0f / 255.0f) > 1e-7f ||
      checkin_frame(bgra32_receipt) != 0) return false;
  const AegpRect roi{1, 1, 5, 4};
  void* first = nullptr;
  void* second = nullptr;
  void** first_world = nullptr;
  void** second_world = nullptr;
  if (!checkout(1, 2, 2, roi, 2, 2, 1, &first, &first_world) ||
      !checkout(1, 2, 2, roi, 2, 2, 1, &second, &second_world)) return false;
  AegpRect region{};
  int32_t transformed_width = 0, transformed_height = 0;
  uint8_t* transformed = nullptr;
  if (render_get_region_reject(first, &region) != 0 || region.left != 1 ||
      region.top != 1 || region.right != 3 || region.bottom != 2 ||
      aegp_world_get_size(first_world, &transformed_width, &transformed_height) != 0 ||
      transformed_width != 3 || transformed_height != 2 ||
      aegp_world_get_base_addr8(first_world,
          reinterpret_cast<void**>(&transformed)) != 0 || !transformed) return false;
  // (2,2) is inside lower-field request only when field=2 selects odd source rows;
  // this request therefore leaves the first sampled row transparent and renders row 2 false.
  const bool field_oracle = transformed[0] == 0 && transformed[1] == 0 &&
      transformed[2] == 0 && transformed[3] == 0;
  {
    std::lock_guard<std::mutex> lock(g_staged_item_world_mutex);
    g_staged_item_worlds.clear();
  }
  // Clearing the registry must not invalidate either live receipt's pinned source.
  int32_t pinned_type = 0;
  if (!field_oracle || aegp_world_get_type(second_world, &pinned_type) != 0 ||
      pinned_type != 1 || checkin_frame(second) != 0 || checkin_frame(first) != 0 ||
      checkin_frame(first) == 0 || aegp_world_get_type(first_world, &pinned_type) == 0)
    return false;

  auto pixels8 = fixture(4);
  if (!publish_staged_item_world(aegp_comp_item_handle(), time, step, 1, 0,
          kPixelFormatArgb32, width, height, width * 4, pixels8.data())) return false;
  void* rejected = reinterpret_cast<void*>(1);
  if (render_options_set_quality(options, 0) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_set_quality(options, 1) != 0 ||
      render_options_set_guide_layers(options, 1) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_set_guide_layers(options, 0) != 0 ||
      render_options_set_time_step(options, {2, 24}) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected)
    return false;
  if (render_options_set_time_step(options, step) != 0 ||
      render_options_set_time(options, {6, 24}) != 0 ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_set_time(options, time) != 0) return false;

  const ItemRenderStackKey direct{aegp_comp_item_handle(), time,
      g_render_project_timestamp.load()};
  {
    ItemRenderStackScope outer(direct);
    rejected = reinterpret_cast<void*>(1);
    if (!outer.entered || render_checkout_frame_reject(
            options, nullptr, nullptr, &rejected) == 0 || rejected) return false;
    ItemRenderStackScope nested_other({reinterpret_cast<void*>(0x7770), time,
        g_render_project_timestamp.load()});
    if (!nested_other.entered || !item_stack_contains(direct)) return false;
  }
  const uint32_t old_generation = g_render_project_timestamp.load();
  bump_render_project_timestamp();
  rejected = reinterpret_cast<void*>(1);
  if (g_render_project_timestamp.load() == old_generation ||
      render_checkout_frame_reject(options, nullptr, nullptr, &rejected) == 0 || rejected ||
      render_options_dispose(options) != 0) return false;
  return render_options_lifetimes_balanced() && async_receipt_lifetimes_balanced();
}

bool world_lifetimes_balanced() {
  return aexcompat::world_registry::lifetimes_balanced();
}

int32_t get_typed_pixel_data(void* world, void* pixels0, void** output,
                             int32_t required_format, int32_t pixel_bytes) {
  if (!output) return 4;
  *output = nullptr;
  if (!world) return 4;
  DispatchWorldFormat resolved{};
  if (!resolve_dispatch_world_format(world, resolved)) return 4;
  const int32_t format = resolved.pixel_format;
  const int32_t rowbytes = resolved.rowbytes;
  const int32_t width = resolved.width;
  const int32_t height = resolved.height;
  if (format != required_format) return 0;
  if (pixel_bytes <= 0 || width <= 0 || height <= 0 ||
      width > 4096 || height > 4096 ||
      rowbytes == (std::numeric_limits<int32_t>::min)() ||
      std::abs(rowbytes) < width * pixel_bytes) return 4;
  void* data = pixels0 ? pixels0 : resolved.data;
  if (!data) return 4;
  *output = data;
  return 0;
}

int32_t __cdecl get_pixel_data8(void* world, void* pixels0, void** output) {
  return get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb32, 4);
}

int32_t __cdecl get_pixel_data16(void* world, void* pixels0, void** output) {
  return get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb64, 8);
}

int32_t __cdecl get_pixel_data_float(void* world, void* pixels0, void** output) {
  return get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb128, 16);
}

int32_t __cdecl get_pixel_data_float_gpu(void* world, void** output) {
  return get_typed_pixel_data(world, nullptr, output, kPixelFormatGpuBgra128, 16);
}

struct PixelDataSuite1 {
  decltype(&get_pixel_data8) get_pixel_data8;
  decltype(&get_pixel_data16) get_pixel_data16;
  decltype(&get_pixel_data_float) get_pixel_data_float;
};

struct PixelDataSuite2 {
  decltype(&get_pixel_data8) get_pixel_data8;
  decltype(&get_pixel_data16) get_pixel_data16;
  decltype(&get_pixel_data_float) get_pixel_data_float;
  decltype(&get_pixel_data_float_gpu) get_pixel_data_float_gpu;
};

static_assert(sizeof(PixelDataSuite1) == 3 * sizeof(void*));
static_assert(offsetof(PixelDataSuite1, get_pixel_data8) == 0 * sizeof(void*));
static_assert(offsetof(PixelDataSuite1, get_pixel_data16) == 1 * sizeof(void*));
static_assert(offsetof(PixelDataSuite1, get_pixel_data_float) == 2 * sizeof(void*));
static_assert(sizeof(PixelDataSuite2) == 4 * sizeof(void*));
static_assert(offsetof(PixelDataSuite2, get_pixel_data_float_gpu) == 3 * sizeof(void*));

PixelDataSuite1 g_pixel_data_suite1{
    &get_pixel_data8, &get_pixel_data16, &get_pixel_data_float};
PixelDataSuite2 g_pixel_data_suite2{
    &get_pixel_data8, &get_pixel_data16, &get_pixel_data_float,
    &get_pixel_data_float_gpu};

bool verify_pixel_data_suites() {
  const std::array<int32_t, 4> formats{
      kPixelFormatArgb32, kPixelFormatArgb64, kPixelFormatArgb128,
      kPixelFormatGpuBgra128};
  std::array<std::array<std::byte, kEffectWorldSize>, 4> worlds{};
  std::array<void*, 4> world_pixels{};
  bool valid = true;
  std::size_t created = 0;
  for (; created < formats.size(); ++created) {
    if (new_world(nullptr, 3, 2, 1, formats[created], worlds[created].data()) != 0) {
      valid = false;
      break;
    }
    std::memcpy(&world_pixels[created], worlds[created].data() + 24,
                sizeof(world_pixels[created]));
  }
  if (created == formats.size()) {
    void* output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data8(
                worlds[0].data(), nullptr, &output) == 0 &&
            output == world_pixels[0] && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data16(
                worlds[1].data(), nullptr, &output) == 0 &&
            output == world_pixels[1] && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data_float(
                worlds[2].data(), nullptr, &output) == 0 &&
            output == world_pixels[2] && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite2.get_pixel_data_float_gpu(
                worlds[3].data(), &output) == 0 &&
            output == world_pixels[3] && valid;

    std::array<std::byte, 16> alternate_pixels{};
    output = nullptr;
    valid = g_pixel_data_suite2.get_pixel_data8(
                worlds[0].data(), alternate_pixels.data(), &output) == 0 &&
            output == alternate_pixels.data() && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite2.get_pixel_data_float(
                worlds[3].data(), nullptr, &output) == 0 &&
            output == nullptr && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite2.get_pixel_data_float_gpu(
                worlds[2].data(), &output) == 0 &&
            output == nullptr && valid;

    std::array<std::byte, kEffectWorldSize> unregistered = worlds[0];
    void* unknown_pixels = alternate_pixels.data();
    std::memcpy(unregistered.data() + 24, &unknown_pixels, sizeof(unknown_pixels));
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data8(
                unregistered.data(), nullptr, &output) != 0 &&
            output == nullptr && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data8(nullptr, nullptr, &output) != 0 &&
            output == nullptr && valid;
  }
  while (created > 0) {
    --created;
    valid = dispose_world(nullptr, worlds[created].data()) == 0 && valid;
  }
  return valid && world_lifetimes_balanced();
}

struct WorldSuite {
  decltype(&new_world) new_world;
  decltype(&dispose_world) dispose_world;
  decltype(&get_pixel_format) get_pixel_format;
};
WorldSuite g_world_suite{&new_world, &dispose_world, &get_pixel_format};
std::array<void*, 2> g_world_suite1{
    reinterpret_cast<void*>(&legacy_new_world), reinterpret_cast<void*>(&dispose_world)};

std::mutex g_pixel_format_mutex;
std::vector<int32_t> g_supported_pixel_formats;
uint32_t g_pixel_format_add_calls{};
uint32_t g_pixel_format_clear_calls{};
uint32_t g_invalid_pixel_format_operations{};
std::atomic_bool g_global_setup_active{false};

bool supported_cpu_pixel_format(int32_t pixel_format) {
  return pixel_format == kPixelFormatArgb32 || pixel_format == kPixelFormatArgb64 ||
      pixel_format == kPixelFormatArgb128;
}

int32_t __cdecl add_supported_pixel_format(void*, int32_t pixel_format) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active || !supported_cpu_pixel_format(pixel_format)) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  ++g_pixel_format_add_calls;
  if (std::find(g_supported_pixel_formats.begin(), g_supported_pixel_formats.end(),
                pixel_format) == g_supported_pixel_formats.end()) {
    g_supported_pixel_formats.push_back(pixel_format);
  }
  return 0;
}

int32_t __cdecl clear_supported_pixel_formats(void*) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  g_supported_pixel_formats.clear();
  ++g_pixel_format_clear_calls;
  return 0;
}

struct PixelFormatSuite {
  decltype(&add_supported_pixel_format) add_supported_pixel_format;
  decltype(&clear_supported_pixel_formats) clear_supported_pixel_formats;
};
PixelFormatSuite g_pixel_format_suite{&add_supported_pixel_format,
                                      &clear_supported_pixel_formats};

int32_t __cdecl floating_point_from_point(void*, const void* definition, void* output) {
  if (!definition || !output) return 4;
  const auto* bytes = static_cast<const std::byte*>(definition);
  auto* values = static_cast<double*>(output);
  int32_t x{}, y{};
  std::memcpy(&x, bytes + 56, sizeof(x));
  std::memcpy(&y, bytes + 60, sizeof(y));
  values[0] = x / 65536.0;
  values[1] = y / 65536.0;
  return 0;
}

int32_t __cdecl floating_point_from_angle(void*, const void* definition, double* output) {
  if (!definition || !output) return 4;
  int32_t value{};
  std::memcpy(&value, static_cast<const std::byte*>(definition) + 56, sizeof(value));
  *output = value / 65536.0;
  return 0;
}

struct PfColorParamPixelFloat {
  float alpha;
  float red;
  float green;
  float blue;
};

int32_t __cdecl floating_point_from_color(void* effect_ref, const void* definition,
                                          PfColorParamPixelFloat* output) {
  if (effect_ref != &g_effect || !definition || !output) return kPfBadCallbackParam;
  const auto* bytes = static_cast<const std::byte*>(definition);
  int32_t disk_id{}, type{};
  std::memcpy(&disk_id, bytes, sizeof(disk_id));
  std::memcpy(&type, bytes + kParamType, sizeof(type));
  const auto found = std::find_if(g_params.begin(), g_params.end(), [disk_id](const ParamRecord& p) {
    return p.disk_id == disk_id;
  });
  if (found == g_params.end()) return kPfInvalidIndex;
  if (type != 5 || found->type != 5 || !found->has_color)
    return kPfUnrecognizedParamType;

  std::array<unsigned char, 4> value{};
  std::memcpy(value.data(), bytes + 56, value.size());
  const std::array<float, 4>* resolved = nullptr;
  if (value == found->current_color) resolved = &found->current_float_color;
  else if (value == found->default_color) resolved = &found->default_float_color;
  else return kPfBadCallbackParam;

  const PfColorParamPixelFloat result{(*resolved)[0], (*resolved)[1],
                                      (*resolved)[2], (*resolved)[3]};
  std::memcpy(output, &result, sizeof(result));
  return 0;
}

struct PointParamSuite { decltype(&floating_point_from_point) get_floating_point_value; };
struct AngleParamSuite { decltype(&floating_point_from_angle) get_floating_point_value; };
struct PfColorParamSuite1 {
  decltype(&floating_point_from_color) PF_GetFloatingPointColorFromColorDef;
};
static_assert(sizeof(PfColorParamPixelFloat) == 4 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, alpha) == 0 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, red) == 1 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, green) == 2 * sizeof(float));
static_assert(offsetof(PfColorParamPixelFloat, blue) == 3 * sizeof(float));
static_assert(sizeof(PfColorParamSuite1) == 1 * sizeof(void*));
static_assert(offsetof(PfColorParamSuite1, PF_GetFloatingPointColorFromColorDef) ==
              0 * sizeof(void*));
int32_t __cdecl update_param_ui(void* effect_ref, int32_t index, const void* definition) {
  if (effect_ref != &g_effect || (!g_update_params_ui_active && !g_user_changed_param_active) || !definition || index <= 0 ||
      static_cast<std::size_t>(index) >= g_active_ui_param_count || !g_active_ui_params ||
      !g_active_ui_params[index]) return kPfBadCallbackParam;
  const auto* source = static_cast<const std::byte*>(definition);
  auto* target = static_cast<std::byte*>(g_active_ui_params[index]);
  int32_t source_type{}, target_type{};
  std::memcpy(&source_type, source + kParamType, sizeof(source_type));
  std::memcpy(&target_type, target + kParamType, sizeof(target_type));
  if (source_type != target_type) return kPfUnrecognizedParamType;

  constexpr uint32_t kMutableUiFlags = (1u << 4) | (1u << 5) | (1u << 9);
  constexpr uint32_t kCollapseTwirly = 1u << 5;
  uint32_t source_ui{}, target_ui{}, source_flags{}, target_flags{};
  std::memcpy(&source_ui, source + kParamUiFlags, sizeof(source_ui));
  std::memcpy(&target_ui, target + kParamUiFlags, sizeof(target_ui));
  std::memcpy(&source_flags, source + kParamFlags, sizeof(source_flags));
  std::memcpy(&target_flags, target + kParamFlags, sizeof(target_flags));
  target_ui = (target_ui & ~kMutableUiFlags) | (source_ui & kMutableUiFlags);
  target_flags = (target_flags & ~kCollapseTwirly) | (source_flags & kCollapseTwirly);
  std::memcpy(target + kParamUiFlags, &target_ui, sizeof(target_ui));
  std::memcpy(target + 8, source + 8, 2 * sizeof(int16_t));
  std::memcpy(target + kParamName, source + kParamName, kParamNameSize);
  target[kParamName + kParamNameSize - 1] = std::byte{0};
  std::memcpy(target + kParamFlags, &target_flags, sizeof(target_flags));

  constexpr std::size_t u = 56;
  if (target_type == 1 || target_type == 2) {
    std::memcpy(target + u + 76, source + u + 76, 2 * sizeof(int32_t));
    if (target_type == 2) std::memcpy(target + u + 88, source + u + 88, 8);
  } else if (target_type == 10) {
    std::memcpy(target + u + 56, source + u + 56, 2 * sizeof(float));
    std::memcpy(target + u + 68, source + u + 68, 8);
  }
  ++g_update_param_ui_calls;
  return 0;
}
struct PfState { int32_t reserved[4]; };
struct PfTime { int32_t value; uint32_t scale; };

bool valid_param_utils_index(int32_t index, bool allow_groups = false) {
  if (allow_groups && index >= -4 && index <= -1) return true;
  return std::any_of(g_params.begin(), g_params.end(),
      [index](const ParamRecord& param) { return param.index == index; });
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N> &bytes, std::size_t offset, T value);

ParameterAnimationKey evaluate_animation(const ParameterTimeline &timeline,
                                         int32_t time, uint32_t scale) {
  if (!rational_less(timeline.keys.front().time, timeline.keys.front().scale,
                     time, scale))
    return timeline.keys.front();
  for (std::size_t i = 1; i < timeline.keys.size(); ++i) {
    const auto &right = timeline.keys[i];
    if (rational_less(time, scale, right.time, right.scale)) {
      const auto &left = timeline.keys[i - 1];
      if (left.hold || left.kind != right.kind ||
          left.component_count != right.component_count)
        return left;
      const long double now = static_cast<long double>(time) / scale,
                        a = static_cast<long double>(left.time) / left.scale,
                        b = static_cast<long double>(right.time) / right.scale;
      const double f = static_cast<double>((now - a) / (b - a));
      AnimationKey value = left;
      if (value.kind == AnimationValueKind::Scalar)
        value.scalar += (right.scalar - value.scalar) * f;
      else if (value.kind == AnimationValueKind::Color)
        for (std::size_t c = 0; c < 4; ++c)
          value.color[c] = static_cast<unsigned char>(
              std::clamp(std::lround(value.color[c] +
                                     (right.color[c] - value.color[c]) * f),
                         0l, 255l));
      else
        for (int c = 0; c < value.component_count; ++c)
          value.components[c] +=
              (right.components[c] - value.components[c]) * f;
      return value;
    }
  }
  return timeline.keys.back();
}

bool write_animation_value(std::array<std::byte, kParamSize> &definition,
                           const ParamRecord &param,
                           const ParameterAnimationKey &key) {
  if (key.kind == AnimationValueKind::Scalar) {
    if (param.type == 1 || param.type == 4 || param.type == 7) {
      if (!std::isfinite(key.scalar) || std::floor(key.scalar) != key.scalar ||
          key.scalar < INT32_MIN || key.scalar > INT32_MAX)
        return false;
      write<int32_t>(definition, 56, static_cast<int32_t>(key.scalar));
    } else if (param.type == 2) {
      const double encoded = key.scalar * 65536.0;
      if (encoded < INT32_MIN || encoded > INT32_MAX)
        return false;
      write<int32_t>(definition, 56, static_cast<int32_t>(std::round(encoded)));
    } else if (param.type == 10) {
      write<double>(definition, 56, key.scalar);
    } else {
      return false;
    }
  } else if (key.kind == AnimationValueKind::Color) {
    if (param.type != 5)
      return false;
    std::memcpy(definition.data() + 56, key.color.data(), key.color.size());
  } else {
    const int component_count =
        param.type == 3 ? 1
                        : (param.type == 6 ? 2 : (param.type == 18 ? 3 : 0));
    if (component_count == 0 || key.component_count != component_count)
      return false;
    for (int component = 0; component < component_count; ++component) {
      if (param.type == 18) {
        write<double>(definition, 56 + component * 8,
                      key.components[component]);
      } else {
        const double encoded = key.components[component] * 65536.0;
        if (encoded < INT32_MIN || encoded > INT32_MAX)
          return false;
        write<int32_t>(definition, 56 + component * 4,
                       static_cast<int32_t>(std::round(encoded)));
      }
    }
  }
  return true;
}

bool apply_parameter_animation(
    std::vector<std::array<std::byte, kParamSize>> &definitions, int32_t time,
    uint32_t scale) {
  if (scale == 0)
    return false;
  for (const auto &timeline : g_parameter_timelines) {
    if (timeline.slot <= 0 ||
        static_cast<std::size_t>(timeline.slot) >= definitions.size())
      return false;
    const auto &param = g_params[timeline.slot - 1];
    if (timeline.keys.front().kind == AnimationValueKind::Arbitrary) {
      if (param.type != 11 || std::any_of(timeline.keys.begin(), timeline.keys.end(),
          [](const auto &key) { return key.kind != AnimationValueKind::Arbitrary; }))
        return false;
      continue;
    }
    if (param.type == 11 || !write_animation_value(definitions[timeline.slot], param,
                                                   evaluate_animation(timeline, time, scale)))
      return false;
  }
  return true;
}

using PfStateToken = std::array<unsigned char, sizeof(PfState)>;
struct PfStateTokenHash {
  std::size_t operator()(const PfStateToken& token) const noexcept {
    std::size_t value = 0;
    std::memcpy(&value, token.data(), (std::min)(sizeof(value), token.size()));
    return value;
  }
};
struct PfStateRegistryEntry {
  void* owner{};
  uint64_t generation{};
  int32_t param_index{};
  bool has_range{};
  PfTime start{};
  PfTime duration{};
  std::vector<unsigned char> canonical_snapshot;
};
constexpr std::size_t kMaxPfStateRegistryEntries = 4096;
std::mutex g_pf_state_registry_mutex;
uint32_t g_pf_get_current_state_calls{};
uint32_t g_pf_are_states_identical_calls{};
std::unordered_map<PfStateToken, PfStateRegistryEntry, PfStateTokenHash>
    g_pf_state_registry;
uint64_t g_pf_state_effect_generation = 1;
bool g_pf_state_effect_live = true;

template <typename T>
void append_param_state_bytes(std::vector<unsigned char>& snapshot, const T& value) {
  const auto* bytes = reinterpret_cast<const unsigned char*>(&value);
  snapshot.insert(snapshot.end(), bytes, bytes + sizeof(value));
}

bool canonical_param_state_snapshot(int32_t index, const PfTime* start,
                                    const PfTime* duration,
                                    std::vector<unsigned char>& snapshot) {
  try {
    snapshot.clear();
    append_param_state_bytes(snapshot, index);
    const uint8_t has_range = start ? 1 : 0;
    append_param_state_bytes(snapshot, has_range);
    if (start) {
      append_param_state_bytes(snapshot, *start);
      append_param_state_bytes(snapshot, *duration);
    }
  for (const auto &param : g_params) {
    if (index >= 0 && param.index != index)
      continue;
    if (index == -3 && param.type == 0)
      continue;
    append_param_state_bytes(snapshot, param.disk_id);
    append_param_state_bytes(snapshot, param.type);
    const auto* raw = reinterpret_cast<const unsigned char*>(param.raw.data());
    snapshot.insert(snapshot.end(), raw, raw + param.raw.size());
  }
  for (const auto &timeline : g_parameter_timelines) {
    if (index >= 0 && timeline.slot != index)
      continue;
    append_param_state_bytes(snapshot, timeline.slot);
    const std::size_t key_count = timeline.keys.size();
    append_param_state_bytes(snapshot, key_count);
    for (const auto &key : timeline.keys) {
      append_param_state_bytes(snapshot, key.time);
      append_param_state_bytes(snapshot, key.scale);
      append_param_state_bytes(snapshot, key.hold);
      append_param_state_bytes(snapshot, key.kind);
      append_param_state_bytes(snapshot, key.scalar);
      snapshot.insert(snapshot.end(), key.color.begin(), key.color.end());
      const auto* components = reinterpret_cast<const unsigned char*>(key.components.data());
      snapshot.insert(snapshot.end(), components,
                      components + sizeof(double) * key.components.size());
      append_param_state_bytes(snapshot, key.component_count);
    }
  }
  } catch (const std::bad_alloc&) {
    snapshot.clear();
    return false;
  }
  return true;
}

void purge_pf_state_registry_locked(void* owner) {
  for (auto it = g_pf_state_registry.begin(); it != g_pf_state_registry.end();) {
    if (!owner || it->second.owner == owner) it = g_pf_state_registry.erase(it);
    else ++it;
  }
}

void reset_pf_state_effect_lifetime(void* owner, bool live) {
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  purge_pf_state_registry_locked(nullptr);
  if (++g_pf_state_effect_generation == 0) ++g_pf_state_effect_generation;
  g_pf_state_effect_live = live;
}

int32_t invoke_global_setdown(EffectEntry entry, void* input, void* output) {
  uint32_t exception_code{};
  const int32_t error = invoke_entry_seh(entry, kGlobalSetdown, input, output, nullptr,
                                         nullptr, nullptr, &exception_code);
  reset_pf_state_effect_lifetime(&g_effect, false);
  aexcompat::pf_helper::reset();
  return error;
}

int32_t __cdecl get_current_param_state(void *effect_ref, int32_t index,
                                        const PfTime *start,
                                        const PfTime *duration,
                                        PfState *state) {
  if (effect_ref != &g_effect || !state ||
      !valid_param_utils_index(index, true) || ((!start) != (!duration)) ||
      (start && (start->scale == 0 || duration->scale == 0)))
    return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  if (!g_pf_state_effect_live || g_pf_state_registry.size() >= kMaxPfStateRegistryEntries)
    return kPfBadCallbackParam;
  PfStateRegistryEntry entry{effect_ref, g_pf_state_effect_generation, index,
                             start != nullptr, start ? *start : PfTime{},
                             duration ? *duration : PfTime{}, {}};
  if (!canonical_param_state_snapshot(index, start, duration,
                                      entry.canonical_snapshot))
    return kPfBadCallbackParam;
  PfStateToken token{};
  do {
    if (BCryptGenRandom(nullptr, token.data(), static_cast<ULONG>(token.size()),
                        BCRYPT_USE_SYSTEM_PREFERRED_RNG) < 0)
      return kPfBadCallbackParam;
  } while (std::all_of(token.begin(), token.end(), [](unsigned char byte) { return byte == 0; }) ||
           g_pf_state_registry.find(token) != g_pf_state_registry.end());
  try {
    g_pf_state_registry.emplace(token, std::move(entry));
  } catch (const std::bad_alloc&) {
    return kPfBadCallbackParam;
  }
  std::memcpy(state, token.data(), token.size());
  ++g_pf_get_current_state_calls;
  return 0;
}

int32_t __cdecl are_param_states_identical(void *effect_ref,
                                           const PfState *first,
                                           const PfState *second,
                                           uint8_t *same) {
  if (effect_ref != &g_effect || !first || !second || !same)
    return kPfBadCallbackParam;
  PfStateToken first_token{}, second_token{};
  std::memcpy(first_token.data(), first, first_token.size());
  std::memcpy(second_token.data(), second, second_token.size());
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  const auto left = g_pf_state_registry.find(first_token);
  const auto right = g_pf_state_registry.find(second_token);
  if (!g_pf_state_effect_live || left == g_pf_state_registry.end() ||
      right == g_pf_state_registry.end() || left->second.owner != effect_ref ||
      right->second.owner != effect_ref ||
      left->second.generation != g_pf_state_effect_generation ||
      right->second.generation != g_pf_state_effect_generation)
    return kPfBadCallbackParam;
  *same = left->second.param_index == right->second.param_index &&
          left->second.has_range == right->second.has_range &&
          (!left->second.has_range ||
           (left->second.start.value == right->second.start.value &&
            left->second.start.scale == right->second.start.scale &&
            left->second.duration.value == right->second.duration.value &&
            left->second.duration.scale == right->second.duration.scale)) &&
           left->second.canonical_snapshot == right->second.canonical_snapshot ? 1 : 0;
  ++g_pf_are_states_identical_calls;
  return 0;
}

int32_t __cdecl is_identical_param_checkout(void *effect_ref, int32_t index,
                                            int32_t time1, int32_t step1,
                                            uint32_t scale1, int32_t time2,
                                            int32_t step2, uint32_t scale2,
                                            uint8_t *identical) {
  if (effect_ref != &g_effect || !identical ||
      !valid_param_utils_index(index) || scale1 == 0 || scale2 == 0 ||
      step1 < 0 || step2 < 0)
    return kPfBadCallbackParam;
  const auto *timeline = parameter_timeline(index);
  if (!timeline) {
    *identical = 1;
    return 0;
  }
  std::array<std::byte, kParamSize> first = g_params[index - 1].raw,
                                    second = first;
  if (!write_animation_value(first, g_params[index - 1],
                             evaluate_animation(*timeline, time1, scale1)) ||
      !write_animation_value(second, g_params[index - 1],
                             evaluate_animation(*timeline, time2, scale2)))
    return kPfBadCallbackParam;
  *identical =
      std::memcmp(first.data() + 56, second.data() + 56, kParamSize - 56) == 0
          ? 1
          : 0;
  return 0;
}

int32_t __cdecl find_param_keyframe_time(void *effect_ref, int32_t index,
                                         int32_t time, uint32_t scale,
                                         int32_t direction, uint8_t *found,
                                         int32_t *key_index, int32_t *key_time,
                                         uint32_t *key_scale) {
  if (effect_ref != &g_effect || !found || !valid_param_utils_index(index) ||
      scale == 0 ||
      (direction != 0 && direction != 1 && direction != 0x1000 &&
       direction != 0x1001) ||
      ((!key_time) != (!key_scale)))
    return kPfBadCallbackParam;
  const auto *timeline = parameter_timeline(index);
  *found = 0;
  if (timeline) {
    const bool greater = direction == 0 || direction == 0x1000;
    const bool inclusive = direction == 0x1000 || direction == 0x1001;
    const auto matches = [&](const ParameterAnimationKey &key) {
      const bool key_less = rational_less(key.time, key.scale, time, scale);
      const bool time_less = rational_less(time, scale, key.time, key.scale);
      const bool equal = !key_less && !time_less;
      return greater ? (time_less || (inclusive && equal))
                     : (key_less || (inclusive && equal));
    };
    for (std::size_t offset = 0; offset < timeline->keys.size(); ++offset) {
      const std::size_t i = greater ? offset : timeline->keys.size() - 1 - offset;
      const auto &key = timeline->keys[i];
      if (matches(key)) {
        *found = 1;
        if (key_index)
          *key_index = static_cast<int32_t>(i);
        if (key_time) {
          *key_time = key.time;
          *key_scale = key.scale;
        }
        return 0;
      }
    }
  }
  if (key_index)
    *key_index = -1;
  if (key_time) {
    *key_time = 0;
    *key_scale = scale;
  }
  return 0;
}

int32_t __cdecl get_param_keyframe_count(void *effect_ref, int32_t index,
                                         int32_t *count) {
  if (effect_ref != &g_effect || !count || !valid_param_utils_index(index))
    return kPfBadCallbackParam;
  *count = -1;
  const auto *timeline = parameter_timeline(index);
  if (timeline)
    *count = static_cast<int32_t>(timeline->keys.size());
  return 0;
}

int32_t __cdecl checkout_param_keyframe(void *effect_ref, int32_t index,
                                        int32_t key_index, int32_t *key_time,
                                        uint32_t *key_scale, void *definition) {
  if (effect_ref != &g_effect || !valid_param_utils_index(index) ||
      key_index < 0 || ((!key_time) != (!key_scale)) ||
      (!definition && !key_time))
    return kPfBadCallbackParam;
  const auto *timeline = parameter_timeline(index);
  if (!timeline || static_cast<std::size_t>(key_index) >= timeline->keys.size())
    return kPfInvalidIndex;
  const auto &key = timeline->keys[key_index];
  if (key_time) {
    *key_time = key.time;
    *key_scale = key.scale;
  }
  if (definition) {
    auto bytes = g_params[index - 1].raw;
    if (!write_animation_value(bytes, g_params[index - 1], key))
      return kPfBadCallbackParam;
    std::memcpy(definition, bytes.data(), bytes.size());
    std::lock_guard<std::mutex> lock(g_keyframe_checkout_mutex);
    if (!g_keyframe_checkout_ledger.emplace(definition, bytes).second)
      return kPfBadCallbackParam;
  }
  return 0;
}

int32_t __cdecl checkin_param_keyframe(void *effect_ref, void *definition) {
  if (effect_ref != &g_effect || !definition)
    return kPfBadCallbackParam;
  std::lock_guard<std::mutex> lock(g_keyframe_checkout_mutex);
  const auto found = g_keyframe_checkout_ledger.find(definition);
  if (found == g_keyframe_checkout_ledger.end())
    return kPfInvalidIndex;
  g_keyframe_checkout_ledger.erase(found);
  return 0;
}

int32_t __cdecl param_key_index_to_time(void* effect_ref, int32_t index, int32_t key_index,
                                         int32_t* key_time, uint32_t* key_scale) {
  if (effect_ref != &g_effect || !valid_param_utils_index(index) || key_index < 0 ||
      !key_time || !key_scale)
    return kPfBadCallbackParam;
  const auto* timeline = parameter_timeline(index);
  if (!timeline || static_cast<std::size_t>(key_index) >= timeline->keys.size())
    return kPfInvalidIndex;
  *key_time = timeline->keys[key_index].time;
  *key_scale = timeline->keys[key_index].scale;
  return 0;
}

bool valid_obsolete_param_state(void* effect_ref, const PfState* state) {
  if (effect_ref != &g_effect || !state) return false;
  PfStateToken token{};
  std::memcpy(token.data(), state, token.size());
  std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
  const auto found = g_pf_state_registry.find(token);
  return g_pf_state_effect_live && found != g_pf_state_registry.end() &&
      found->second.owner == effect_ref &&
      found->second.generation == g_pf_state_effect_generation;
}

int32_t __cdecl get_current_param_state_obsolete(void* effect_ref, PfState* state) {
  return get_current_param_state(effect_ref, -1, nullptr, nullptr, state);
}

int32_t __cdecl has_param_changed_obsolete(void* effect_ref, const PfState* state,
                                           int32_t, uint8_t* changed) {
  if (!changed || !valid_obsolete_param_state(effect_ref, state))
    return kPfBadCallbackParam;
  *changed = 1;
  return 0;
}

int32_t __cdecl have_inputs_changed_over_time_span_obsolete(
    void* effect_ref, const PfState* state, const PfTime* start,
    const PfTime* duration, uint8_t* changed) {
  if (!changed || ((!start) != (!duration)) ||
      (start && (start->scale == 0 || duration->scale == 0)) ||
      !valid_obsolete_param_state(effect_ref, state))
    return kPfBadCallbackParam;
  *changed = 1;
  return 0;
}

struct ParamUtilsSuite1 {
  decltype(&update_param_ui) PF_UpdateParamUI;
  decltype(&get_current_param_state_obsolete) PF_GetCurrentStateObsolete;
  decltype(&has_param_changed_obsolete) PF_HasParamChangedObsolete;
  decltype(&have_inputs_changed_over_time_span_obsolete)
      PF_HaveInputsChangedOverTimeSpanObsolete;
  decltype(&is_identical_param_checkout) PF_IsIdenticalCheckout;
  decltype(&find_param_keyframe_time) PF_FindKeyframeTime;
  decltype(&get_param_keyframe_count) PF_GetKeyframeCount;
  decltype(&checkout_param_keyframe) PF_CheckoutKeyframe;
  decltype(&checkin_param_keyframe) PF_CheckinKeyframe;
  decltype(&param_key_index_to_time) PF_KeyIndexToTime;
};

struct ParamUtilsSuite3 {
  decltype(&update_param_ui) PF_UpdateParamUI;
  decltype(&get_current_param_state) PF_GetCurrentState;
  decltype(&are_param_states_identical) PF_AreStatesIdentical;
  decltype(&is_identical_param_checkout) PF_IsIdenticalCheckout;
  decltype(&find_param_keyframe_time) PF_FindKeyframeTime;
  decltype(&get_param_keyframe_count) PF_GetKeyframeCount;
  decltype(&checkout_param_keyframe) PF_CheckoutKeyframe;
  decltype(&checkin_param_keyframe) PF_CheckinKeyframe;
  decltype(&param_key_index_to_time) PF_KeyIndexToTime;
};
static_assert(sizeof(PfState) == 16);
static_assert(sizeof(ParamUtilsSuite1) == 10 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_UpdateParamUI) == 0 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_GetCurrentStateObsolete) == 1 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_HasParamChangedObsolete) == 2 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_HaveInputsChangedOverTimeSpanObsolete) ==
              3 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite1, PF_KeyIndexToTime) == 9 * sizeof(void*));
static_assert(sizeof(ParamUtilsSuite3) == 9 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_UpdateParamUI) == 0 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_GetCurrentState) == 1 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_AreStatesIdentical) == 2 * sizeof(void*));
static_assert(offsetof(ParamUtilsSuite3, PF_KeyIndexToTime) == 8 * sizeof(void*));
PointParamSuite g_point_param_suite{&floating_point_from_point};
AngleParamSuite g_angle_param_suite{&floating_point_from_angle};
PfColorParamSuite1 g_color_param_suite1{&floating_point_from_color};
ParamUtilsSuite1 g_param_utils_suite1{&update_param_ui, &get_current_param_state_obsolete,
    &has_param_changed_obsolete, &have_inputs_changed_over_time_span_obsolete,
    &is_identical_param_checkout, &find_param_keyframe_time, &get_param_keyframe_count,
    &checkout_param_keyframe, &checkin_param_keyframe, &param_key_index_to_time};
ParamUtilsSuite3 g_param_utils_suite{&update_param_ui, &get_current_param_state,
    &are_param_states_identical, &is_identical_param_checkout, &find_param_keyframe_time,
    &get_param_keyframe_count, &checkout_param_keyframe, &checkin_param_keyframe,
    &param_key_index_to_time};

bool verify_pixel_format_registry_rejection() {
  const uint32_t invalid_before = g_invalid_pixel_format_operations;
  const uint32_t add_before = g_pixel_format_add_calls;
  const uint32_t clear_before = g_pixel_format_clear_calls;
  const bool phase_rejected = clear_supported_pixel_formats(&g_effect) != 0;
  g_global_setup_active = true;
  if (clear_supported_pixel_formats(&g_effect) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb128) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb64) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb128) != 0) {
    g_global_setup_active = false;
    return false;
  }
  bool order_valid = false;
  {
    std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
    order_valid = g_supported_pixel_formats ==
        std::vector<int32_t>{kPixelFormatArgb128, kPixelFormatArgb64};
  }
  const bool rejected = add_supported_pixel_format(&g_effect, 1717854562) != 0;
  const bool cleared = clear_supported_pixel_formats(&g_effect) == 0;
  g_global_setup_active = false;
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  return phase_rejected && order_valid && rejected && cleared &&
      g_supported_pixel_formats.empty() &&
      g_invalid_pixel_format_operations == invalid_before + 2 &&
      g_pixel_format_add_calls == add_before + 3 &&
      g_pixel_format_clear_calls == clear_before + 2;
}

bool verify_world_double_dispose_rejected() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  const uint64_t invalid_before =
      aexcompat::world_registry::statistics().invalid_operations;
  if (new_world(&g_effect, 7, 5, 1, kPixelFormatArgb128, world.data()) != 0) return false;
  void* pixels{};
  int32_t flags{}, rowbytes{}, width{}, height{}, format{};
  std::memcpy(&flags, world.data() + 16, sizeof(flags));
  std::memcpy(&pixels, world.data() + 24, sizeof(pixels));
  std::memcpy(&rowbytes, world.data() + 32, sizeof(rowbytes));
  std::memcpy(&width, world.data() + 36, sizeof(width));
  std::memcpy(&height, world.data() + 40, sizeof(height));
  const bool layout_valid = pixels && flags == 3 && rowbytes == 112 && width == 7 && height == 5 &&
      get_pixel_format(world.data(), &format) == 0 && format == kPixelFormatArgb128 &&
      std::all_of(static_cast<const unsigned char*>(pixels),
                  static_cast<const unsigned char*>(pixels) + 560,
                  [](unsigned char value) { return value == 0; });
  const int32_t first = dispose_world(&g_effect, world.data());
  const int32_t second = dispose_world(&g_effect, world.data());
  return layout_valid && first == 0 && second != 0 &&
      aexcompat::world_registry::statistics().invalid_operations ==
          invalid_before + 1 && world_lifetimes_balanced();
}

bool verify_world_allocation_limit_rejected() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  world.fill(std::byte{0x5a});
  const auto before = world;
  const uint64_t invalid_before =
      aexcompat::world_registry::statistics().invalid_operations;
  const int32_t error = new_world(&g_effect, 32768, 32768, 1,
                                  kPixelFormatArgb128, world.data());
  return error != 0 && world == before &&
      aexcompat::world_registry::statistics().invalid_operations ==
          invalid_before + 1 && world_lifetimes_balanced();
}

bool verify_owned_world_snapshot_is_atomic() {
  alignas(8) std::array<std::byte, kEffectWorldSize> world{};
  if (new_world(&g_effect, 2, 2, 1, kPixelFormatArgb32, world.data()) != 0)
    return false;
  aexcompat::world_registry::OwnedWorldSnapshot snapshot{};
  if (!aexcompat::world_registry::snapshot_owned_world(world.data(), snapshot) ||
      !snapshot.world.data || snapshot.pixel_format != kPixelFormatArgb32 ||
      snapshot.world.width != 2 || snapshot.world.height != 2 ||
      snapshot.world.rowbytes != 8) {
    dispose_world(&g_effect, world.data());
    return false;
  }
  const void* captured_data = snapshot.world.data;
  if (dispose_world(&g_effect, world.data()) != 0 || !world_lifetimes_balanced())
    return false;
  aexcompat::world_registry::OwnedWorldSnapshot stale{};
  return captured_data && snapshot.world.data == captured_data &&
      snapshot.world.width == 2 && snapshot.pixel_format == kPixelFormatArgb32 &&
      !aexcompat::world_registry::snapshot_owned_world(world.data(), stale);
}

bool verify_owned_world_snapshot_concurrent_dispose() {
  for (int iteration = 0; iteration < 64; ++iteration) {
    alignas(8) std::array<std::byte, kEffectWorldSize> world{};
    if (new_world(&g_effect, 3, 2, 1, kPixelFormatArgb64, world.data()) != 0)
      return false;
    std::atomic_bool ready{false};
    std::atomic_bool go{false};
    bool resolved = false;
    aexcompat::world_registry::OwnedWorldSnapshot snapshot{};
    std::thread reader([&] {
      ready.store(true, std::memory_order_release);
      while (!go.load(std::memory_order_acquire)) std::this_thread::yield();
      resolved = aexcompat::world_registry::snapshot_owned_world(
          world.data(), snapshot);
    });
    while (!ready.load(std::memory_order_acquire)) std::this_thread::yield();
    go.store(true, std::memory_order_release);
    const int32_t dispose_error = dispose_world(&g_effect, world.data());
    reader.join();
    if (dispose_error != 0 || !world_lifetimes_balanced()) return false;
    if (resolved && (!snapshot.world.data || snapshot.world.width != 3 ||
        snapshot.world.height != 2 || snapshot.world.rowbytes != 24 ||
        snapshot.pixel_format != kPixelFormatArgb64)) return false;
  }
  return true;
}

std::string missing_suites_report_json() {
  return suite_registry().missing_suites_report_json();
}

bool suite_leases_balanced() {
  return suite_registry().balanced();
}

std::size_t live_suite_lease_count() {
  return suite_registry().live_lease_count();
}

uint32_t live_suite_reference_count() {
  return suite_registry().live_reference_count();
}

uint32_t suite_acquire_count() { return suite_registry().acquire_count(); }
uint32_t suite_release_count() { return suite_registry().release_count(); }

int32_t __cdecl aegp_get_unique_command(int32_t* command) {
  if (!command || g_aegp_commands_created >= 64) return 4;
  *command = g_next_aegp_command++;
  ++g_aegp_commands_created;
  return 0;
}
int32_t __cdecl aegp_insert_menu_command(int32_t command, const char* name,
                                         int32_t menu, int32_t) {
  if (command < 10000 || !name || strnlen_s(name, 1024) == 0 || menu < 0 || menu > 16 ||
      g_aegp_menu_commands_inserted >= 64) return 4;
  ++g_aegp_menu_commands_inserted;
  g_aegp_inserted_commands.push_back(command);
  return 0;
}
int32_t __cdecl aegp_remove_menu_command(int32_t) { return 0; }
int32_t __cdecl aegp_set_menu_command_name(int32_t, const char* name) {
  return name && strnlen_s(name, 1024) > 0 ? 0 : 4;
}
int32_t __cdecl aegp_command_state(int32_t command) {
  if (command < 10000) return 4;
  ++g_aegp_command_enable_calls;
  return 0;
}
int32_t __cdecl aegp_check_menu_command(int32_t command, uint8_t checked) {
  if (command < 10000) return 4;
  ++g_aegp_command_check_calls;
  if (checked) ++g_aegp_command_checked_true_calls;
  else ++g_aegp_command_checked_false_calls;
  return 0;
}
struct AegpCommandSuite {
  decltype(&aegp_get_unique_command) get_unique_command;
  decltype(&aegp_insert_menu_command) insert_menu_command;
  decltype(&aegp_remove_menu_command) remove_menu_command;
  decltype(&aegp_set_menu_command_name) set_menu_command_name;
  decltype(&aegp_command_state) enable_command;
  decltype(&aegp_command_state) disable_command;
  decltype(&aegp_check_menu_command) check_menu_command;
  decltype(&aegp_command_state) do_command;
};
AegpCommandSuite g_aegp_command_suite{
    &aegp_get_unique_command, &aegp_insert_menu_command, &aegp_remove_menu_command,
    &aegp_set_menu_command_name, &aegp_command_state, &aegp_command_state,
    &aegp_check_menu_command, &aegp_command_state};

int32_t __cdecl aegp_register_command_hook(int32_t plugin_id, int32_t priority,
                                           int32_t command, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_command_hook(
      plugin_id, priority, command, hook, refcon);
}
int32_t __cdecl aegp_register_update_menu_hook(int32_t plugin_id, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_update_menu_hook(
      plugin_id, hook, refcon);
}
int32_t __cdecl aegp_register_death_hook(int32_t plugin_id, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_death_hook(plugin_id, hook, refcon);
}
int32_t __cdecl aegp_register_idle_hook(int32_t plugin_id, void* hook, void* refcon) {
  return aexcompat::worker_runtime::aegp_init::register_idle_hook(plugin_id, hook, refcon);
}
struct AegpRegisterSuite {
  decltype(&aegp_register_command_hook) register_command_hook;
  decltype(&aegp_register_update_menu_hook) register_update_menu_hook;
  decltype(&aegp_register_death_hook) register_death_hook;
  void* register_version_hook{};
  void* register_about_string_hook{};
  void* register_about_hook{};
  void* register_artisan{};
  void* register_io{};
  decltype(&aegp_register_idle_hook) register_idle_hook;
  void* register_tracker{};
  void* register_interactive_artisan{};
  void* register_preset_localization{};
};
AegpRegisterSuite g_aegp_register_suite{
    &aegp_register_command_hook, &aegp_register_update_menu_hook,
    &aegp_register_death_hook, nullptr, nullptr, nullptr, nullptr, nullptr,
    &aegp_register_idle_hook, nullptr, nullptr, nullptr};

AegpSceneObject& g_aegp_comp_item = scene_runtime_state().composition_item;
AegpSceneObject& g_aegp_comp = scene_runtime_state().composition;
void* aegp_comp_item_handle() { return composition_item_handle(); }
struct AegpColorVal { double alpha, red, green, blue; };
static_assert(sizeof(AegpColorVal) == 4 * sizeof(double));
int32_t __cdecl aegp_get_comp_bg_color(void* comp, AegpColorVal* color) {
  if (comp != &g_aegp_comp || !color) {
    g_comp_bg_color_rejections.fetch_add(1, std::memory_order_relaxed);
    return 4;
  }
  const AegpColorVal headless_color{1.0, 0.0, 0.0, 0.0};
  *color = headless_color;
  g_comp_bg_color_successes.fetch_add(1, std::memory_order_relaxed);
  return 0;
}

std::array<AegpSceneObject, 3>& g_aegp_layers = scene_runtime_state().layers;
AegpSceneObject& g_aegp_effect = scene_runtime_state().effect;
int32_t& g_aegp_active_camera_layer_index =
    scene_runtime_state().active_camera_layer_index;

bool valid_comp_time(const AegpTime& time) {
  if (time.scale == 0) return false;
  constexpr int64_t kCompDurationValue = 300;
  constexpr uint32_t kCompDurationScale = 30;
  const int64_t scaled_time = static_cast<int64_t>(time.value) * kCompDurationScale;
  const int64_t scaled_duration = kCompDurationValue * static_cast<int64_t>(time.scale);
  return scaled_time >= 0 && scaled_time < scaled_duration;
}

bool layer_active_at_time(std::size_t index, const AegpTime& time) {
  if (index >= g_aegp_layer_in_points.size() || index >= g_aegp_layer_durations.size())
    return false;
  const auto& in_point = g_aegp_layer_in_points[index];
  const auto& duration = g_aegp_layer_durations[index];
  if (in_point.scale == 0 || duration.scale == 0 || duration.value <= 0) return false;
  const long double seconds =
      static_cast<long double>(time.value) / static_cast<long double>(time.scale);
  const long double in_seconds =
      static_cast<long double>(in_point.value) / static_cast<long double>(in_point.scale);
  const long double duration_seconds =
      static_cast<long double>(duration.value) / static_cast<long double>(duration.scale);
  return seconds >= in_seconds && seconds < in_seconds + duration_seconds;
}

int32_t __cdecl get_effect_camera(
    void* effect, const AegpTime* comp_time, void** camera_layer) {
  if (effect != &g_effect || !g_pf_state_effect_live || !comp_time || !camera_layer ||
      !valid_comp_time(*comp_time)) return 4;
  void* result = nullptr;
  if (g_aegp_active_camera_layer_index >= 0) {
    const auto index = static_cast<std::size_t>(g_aegp_active_camera_layer_index);
    if (index >= g_aegp_layers.size()) return 4;
    if (layer_active_at_time(index, *comp_time)) result = &g_aegp_layers[index];
  }
  *camera_layer = result;
  return 0;
}

int32_t __cdecl get_effect_camera_matrix(void* effect, const AegpTime* comp_time,
    AegpMatrix4* camera_matrix, double* distance_to_image_plane,
    int16_t* image_plane_width, int16_t* image_plane_height) {
  if (effect != &g_effect || !g_pf_state_effect_live || !comp_time ||
      !camera_matrix || !distance_to_image_plane || !image_plane_width ||
      !image_plane_height || !valid_comp_time(*comp_time)) return 4;
  const int32_t width = g_full_resolution_width > 0
      ? g_full_resolution_width : smart_state().width;
  const int32_t height = g_full_resolution_height > 0
      ? g_full_resolution_height : smart_state().height;
  if (width <= 0 || height <= 0 || width > INT16_MAX || height > INT16_MAX)
    return 4;

  // The headless scene uses an unrotated default camera and a deterministic
  // image-plane distance until project camera transforms are modeled.
  AegpMatrix4 result{};
  for (std::size_t index = 0; index < 4; ++index) result.mat[index][index] = 1.0;
  *camera_matrix = result;
  *distance_to_image_plane = static_cast<double>(width);
  *image_plane_width = static_cast<int16_t>(width);
  *image_plane_height = static_cast<int16_t>(height);
  return 0;
}
auto& g_aegp_selection = scene_runtime_state().selection;
#pragma pack(push, 1)
struct TimelinePacketHeader {
  uint32_t magic{0x52414558u};
  uint16_t version{2};
  uint16_t type{13};
  uint64_t sequence{1};
};
struct TimelineKeyframeRequest {
  TimelinePacketHeader header{};
  uint64_t comp_id{1001};
  uint64_t layer_id{2001};
};
struct TimelineKeyframesSnapshotHeader {
  TimelinePacketHeader header{};
  uint64_t comp_id{};
  uint64_t layer_id{};
  uint32_t total_data_bytes{};
  uint32_t prop_count{};
};
struct TimelineKeyframedPropHeader {
  char name[64]{};
  char effect_match[32]{};
  uint32_t keyframe_count{};
  uint8_t reserved[4]{};
};
struct TimelineKeyframeEntry {
  int32_t frame{};
  uint8_t interpolation{};
  uint8_t reserved[3]{};
  float value[4]{};
};
struct TimelineHostSeekRequest {
  TimelinePacketHeader header{0x52414558u, 2, 5, 7};
  int64_t frame{75};
  double time_seconds{2.5};
  double fps{30.0};
  uint32_t flags{};
  uint32_t reserved{};
};
struct TimelineHostSeekAck {
  TimelinePacketHeader header{};
  int32_t status{};
  int64_t accepted_frame{};
  double accepted_time_seconds{};
  double accepted_fps{};
};
struct TimelineHostTrimRequest {
  TimelinePacketHeader header{0x52414558u, 2, 15, 9};
  uint64_t comp_id{1001};
  uint64_t layer_id{2001};
  int32_t in_frame{30};
  int32_t out_frame{240};
  uint64_t reserved{};
};
struct TimelineHostTrimAck {
  TimelinePacketHeader header{};
  uint32_t status{};
  uint32_t reserved{};
  uint64_t layer_id{};
  int32_t in_frame{};
  int32_t out_frame{};
};
struct TimelineHostSwitchRequest {
  TimelinePacketHeader header{0x52414558u, 2, 22, 11};
  uint64_t comp_id{1001};
  uint64_t layer_id{2001};
  uint32_t flags_to_toggle{0x00000036u};
  uint32_t reserved{};
};
struct TimelineHostSwitchAck {
  TimelinePacketHeader header{};
  uint32_t status{};
  uint32_t applied_flags{};
  uint64_t layer_id{};
  uint32_t reserved{};
};
#pragma pack(pop)
static_assert(sizeof(TimelineKeyframeRequest) == 32);
static_assert(sizeof(TimelineKeyframesSnapshotHeader) == 40);
static_assert(sizeof(TimelineKeyframedPropHeader) == 104);
static_assert(sizeof(TimelineKeyframeEntry) == 24);
static_assert(sizeof(TimelineHostSeekRequest) == 48);
static_assert(sizeof(TimelineHostSeekAck) == 44);
static_assert(sizeof(TimelineHostTrimRequest) == 48);
static_assert(sizeof(TimelineHostTrimAck) == 40);
static_assert(sizeof(TimelineHostSwitchRequest) == 40);
static_assert(sizeof(TimelineHostSwitchAck) == 36);

struct KeyframePipeProbe {
  HANDLE pipe{INVALID_HANDLE_VALUE};
  std::thread reader;
  std::atomic_bool connected{false};
  std::atomic_bool request_sent{false};
  std::atomic_bool response_received{false};
  std::atomic_bool response_valid{false};
  std::atomic_uint32_t response_bytes{0};

  bool start() {
    pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
        PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        1, 1024 * 1024, 1024 * 1024, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return false;
    reader = std::thread([this]() {
      const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
          GetLastError() == ERROR_PIPE_CONNECTED;
      if (!connected_now) return;
      connected = true;
      const TimelineKeyframeRequest request{};
      DWORD written = 0;
      if (WriteFile(pipe, &request, sizeof(request), &written, nullptr) &&
          written == sizeof(request)) request_sent = true;
      else return;

      std::vector<uint8_t> inbound;
      inbound.reserve(4096);
      std::array<uint8_t, 4096> chunk{};
      while (inbound.size() < 1024 * 1024) {
        DWORD bytes_read = 0;
        if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()), &bytes_read, nullptr) ||
            bytes_read == 0) return;
        inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
        for (std::size_t offset = 0;
             offset + sizeof(TimelineKeyframesSnapshotHeader) <= inbound.size(); ++offset) {
          TimelineKeyframesSnapshotHeader header{};
          std::memcpy(&header, inbound.data() + offset, sizeof(header));
          if (header.header.magic != 0x52414558u || header.header.version != 2 ||
              header.header.type != 14) continue;
          if (header.total_data_bytes > 1024 * 1024 - sizeof(header) ||
              offset + sizeof(header) + header.total_data_bytes > inbound.size()) continue;
          response_received = true;
          response_bytes = static_cast<uint32_t>(sizeof(header) + header.total_data_bytes);
          if (header.comp_id != 1001 || header.layer_id != 2001 || header.prop_count != 1 ||
              header.total_data_bytes != sizeof(TimelineKeyframedPropHeader) +
                  2 * sizeof(TimelineKeyframeEntry)) return;
          TimelineKeyframedPropHeader property{};
          TimelineKeyframeEntry first{}, second{};
          const auto* payload = inbound.data() + offset + sizeof(header);
          std::memcpy(&property, payload, sizeof(property));
          std::memcpy(&first, payload + sizeof(property), sizeof(first));
          std::memcpy(&second, payload + sizeof(property) + sizeof(first), sizeof(second));
          const bool strings_valid = std::strncmp(property.name, "Amount", sizeof(property.name)) == 0 &&
              std::strncmp(property.effect_match, "AEXCompat.Probe", sizeof(property.effect_match)) == 0;
          const bool keys_valid = property.keyframe_count == 2 &&
              first.frame == 0 && first.interpolation == 0 && first.value[0] == 10.0f &&
              second.frame == 60 && second.interpolation == 2 && second.value[0] == 90.0f;
          response_valid = strings_valid && keys_valid;
          return;
        }
      }
    });
    return true;
  }

  void stop() {
    if (reader.joinable())
      CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
    if (reader.joinable()) reader.join();
    if (pipe != INVALID_HANDLE_VALUE) {
      DisconnectNamedPipe(pipe);
      CloseHandle(pipe);
      pipe = INVALID_HANDLE_VALUE;
    }
  }
  ~KeyframePipeProbe() { stop(); }
};

struct SeekPipeProbe {
  HANDLE pipe{INVALID_HANDLE_VALUE};
  std::thread reader;
  std::atomic_bool connected{false};
  std::atomic_bool request_sent{false};
  std::atomic_bool ack_received{false};
  std::atomic_bool ack_valid{false};

  bool start() {
    pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
        PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        1, 1024 * 1024, 1024 * 1024, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return false;
    reader = std::thread([this]() {
      const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
          GetLastError() == ERROR_PIPE_CONNECTED;
      if (!connected_now) return;
      connected = true;
      const TimelineHostSeekRequest request{};
      DWORD written = 0;
      if (!WriteFile(pipe, &request, sizeof(request), &written, nullptr) ||
          written != sizeof(request)) return;
      request_sent = true;

      std::vector<uint8_t> inbound;
      inbound.reserve(4096);
      std::array<uint8_t, 4096> chunk{};
      while (inbound.size() < 1024 * 1024) {
        DWORD bytes_read = 0;
        if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()), &bytes_read, nullptr) ||
            bytes_read == 0) return;
        inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
        for (std::size_t offset = 0; offset + sizeof(TimelineHostSeekAck) <= inbound.size(); ++offset) {
          TimelineHostSeekAck ack{};
          std::memcpy(&ack, inbound.data() + offset, sizeof(ack));
          if (ack.header.magic != 0x52414558u || ack.header.version != 2 ||
              ack.header.type != 6) continue;
          ack_received = true;
          ack_valid = ack.header.sequence == 7 && ack.status == 0 &&
              ack.accepted_frame == 75 && ack.accepted_time_seconds == 2.5 &&
              ack.accepted_fps == 30.0;
          return;
        }
      }
    });
    return true;
  }

  void stop() {
    if (reader.joinable()) CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
    if (reader.joinable()) reader.join();
    if (pipe != INVALID_HANDLE_VALUE) {
      DisconnectNamedPipe(pipe);
      CloseHandle(pipe);
      pipe = INVALID_HANDLE_VALUE;
    }
  }
  ~SeekPipeProbe() { stop(); }
};

struct TrimPipeProbe {
  HANDLE pipe{INVALID_HANDLE_VALUE};
  std::thread reader;
  std::atomic_bool connected{false};
  std::atomic_bool request_sent{false};
  std::atomic_bool ack_received{false};
  std::atomic_bool ack_valid{false};

  bool start() {
    pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
        PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        1, 1024 * 1024, 1024 * 1024, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return false;
    reader = std::thread([this]() {
      const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
          GetLastError() == ERROR_PIPE_CONNECTED;
      if (!connected_now) return;
      connected = true;
      const TimelineHostTrimRequest request{};
      DWORD written = 0;
      if (!WriteFile(pipe, &request, sizeof(request), &written, nullptr) ||
          written != sizeof(request)) return;
      request_sent = true;

      std::vector<uint8_t> inbound;
      inbound.reserve(4096);
      std::array<uint8_t, 4096> chunk{};
      while (inbound.size() < 1024 * 1024) {
        DWORD bytes_read = 0;
        if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()), &bytes_read, nullptr) ||
            bytes_read == 0) return;
        inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
        for (std::size_t offset = 0; offset + sizeof(TimelineHostTrimAck) <= inbound.size(); ++offset) {
          TimelineHostTrimAck ack{};
          std::memcpy(&ack, inbound.data() + offset, sizeof(ack));
          if (ack.header.magic != 0x52414558u || ack.header.version != 2 ||
              ack.header.type != 16) continue;
          ack_received = true;
          ack_valid = ack.header.sequence == 9 && ack.status == 0 &&
              ack.layer_id == 2001 && ack.in_frame == 30 && ack.out_frame == 240;
          return;
        }
      }
    });
    return true;
  }

  void stop() {
    if (reader.joinable()) CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
    if (reader.joinable()) reader.join();
    if (pipe != INVALID_HANDLE_VALUE) {
      DisconnectNamedPipe(pipe);
      CloseHandle(pipe);
      pipe = INVALID_HANDLE_VALUE;
    }
  }
  ~TrimPipeProbe() { stop(); }
};

struct SwitchPipeProbe {
  HANDLE pipe{INVALID_HANDLE_VALUE};
  std::thread reader;
  std::atomic_bool connected{false};
  std::atomic_bool request_sent{false};
  std::atomic_bool ack_received{false};
  std::atomic_bool ack_valid{false};

  bool start() {
    pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
        PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        1, 1024 * 1024, 1024 * 1024, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) return false;
    reader = std::thread([this]() {
      const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
          GetLastError() == ERROR_PIPE_CONNECTED;
      if (!connected_now) return;
      connected = true;
      const TimelineHostSwitchRequest request{};
      DWORD written = 0;
      if (!WriteFile(pipe, &request, sizeof(request), &written, nullptr) ||
          written != sizeof(request)) return;
      request_sent = true;

      std::vector<uint8_t> inbound;
      inbound.reserve(4096);
      std::array<uint8_t, 4096> chunk{};
      while (inbound.size() < 1024 * 1024) {
        DWORD bytes_read = 0;
        if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()), &bytes_read, nullptr) ||
            bytes_read == 0) return;
        inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
        for (std::size_t offset = 0; offset + sizeof(TimelineHostSwitchAck) <= inbound.size(); ++offset) {
          TimelineHostSwitchAck ack{};
          std::memcpy(&ack, inbound.data() + offset, sizeof(ack));
          if (ack.header.magic != 0x52414558u || ack.header.version != 2 ||
              ack.header.type != 23) continue;
          ack_received = true;
          ack_valid = ack.header.sequence == 11 && ack.status == 0 &&
              ack.applied_flags == 0x00000032u && ack.layer_id == 2001;
          return;
        }
      }
    });
    return true;
  }

  void stop() {
    if (reader.joinable()) CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
    if (reader.joinable()) reader.join();
    if (pipe != INVALID_HANDLE_VALUE) {
      DisconnectNamedPipe(pipe);
      CloseHandle(pipe);
      pipe = INVALID_HANDLE_VALUE;
    }
  }
  ~SwitchPipeProbe() { stop(); }
};
// AEGP project/item/comp/layer/effect/collection/stream/keyframe callbacks are
// compiled in worker_aegp_scene.cpp.
bool __cdecl validate_render_options_item(int32_t plugin_id, void* item) {
  return plugin_id == 1 && item == &g_aegp_comp_item;
}
bool __cdecl initialize_layer_render_options(
    int32_t plugin_id, void* source, AegpLayerEffectBoundary boundary,
    AegpLayerRenderOptionsValue* value) {
  if (plugin_id <= 0 || !value) return false;
  AegpLayerRenderOptionsValue initialized{};
  initialized.owner_plugin_id = plugin_id;
  initialized.effect_boundary = boundary;
  if (boundary == AegpLayerEffectBoundary::all) {
    if (aegp_layer_index(source) < 0) return false;
    initialized.layer = source;
  } else {
    const auto* instance = resolve_effect_instance(source, plugin_id);
    if (!instance) return false;
    initialized.layer = instance->layer;
    initialized.upstream_effect = source;
  }
  *value = initialized;
  return true;
}

bool __cdecl scene_initialize_layer_render_options(
    int32_t plugin_id, void* source, int32_t boundary, void* value) noexcept {
  if (boundary < static_cast<int32_t>(AegpLayerEffectBoundary::all) ||
      boundary > static_cast<int32_t>(AegpLayerEffectBoundary::downstream)) return false;
  return initialize_layer_render_options(
      plugin_id, source, static_cast<AegpLayerEffectBoundary>(boundary),
      static_cast<AegpLayerRenderOptionsValue*>(value));
}

std::string live_suite_lease_summary() {
  return suite_registry().live_summary();
}

bool isolated_aegp_read_cache_is_bounded() {
  const auto snapshot = suite_registry().snapshot();
  uint32_t total = 0;
  for (const auto& [key, count] : snapshot.live_leases) {
    const bool allowed =
        (key.first == "AEGP Item Suite" && key.second == 14) ||
        (key.first == "AEGP Comp Suite" && (key.second == 25 || key.second == 26)) ||
        (key.first == "AEGP Layer Suite" &&
         (key.second == 11 || key.second == 14 || key.second == 15)) ||
        (key.first == "AEGP Effect Suite" && key.second == 4) ||
        (key.first == "AEGP Collection Suite" && key.second == 2) ||
        (key.first == "AEGP Stream Suite" && key.second == 11) ||
        (key.first == "AEGP Keyframe Suite" && key.second == 5);
    if (!allowed || count > 16) return false;
    total += count;
  }
  return total > 0 && total <= 32;
}

constexpr uint32_t kMaxCudaDevices = kMaxGpuDevices;


namespace gpu_transport = aexcompat::gpu_runtime::memory_world_transport;
using CudaRenderTransport = gpu_transport::RenderTransport;
auto& g_gpu_device_suite1 = gpu_transport::gpu_device_suite1;
auto& g_cuda_upload_bytes = gpu_transport::cuda_upload_bytes;
auto& g_cuda_download_bytes = gpu_transport::cuda_download_bytes;
auto& g_cuda_sync_failures = gpu_transport::cuda_sync_failures;
auto& g_last_cuda_device_count = gpu_transport::last_cuda_device_count;
auto& g_last_cuda_device_index = gpu_transport::last_cuda_device_index;
auto& g_opencl_upload_bytes = gpu_transport::opencl_upload_bytes;
auto& g_opencl_download_bytes = gpu_transport::opencl_download_bytes;
auto& g_opencl_sync_failures = gpu_transport::opencl_sync_failures;
auto& g_gpu_allocations_created = gpu_transport::allocations_created;
auto& g_gpu_allocations_freed = gpu_transport::allocations_freed;
auto& g_invalid_gpu_memory_operations = gpu_transport::invalid_memory_operations;

bool host_recognizes_smart_gpu_world(void* world) {
  return world &&
      (world == smart_state().input_world || world == smart_state().output_world);
}

const bool g_gpu_transport_configured = [] {
  aexcompat::world_registry::configure_gpu_fallback_bridge(
      &host_recognizes_smart_gpu_world);
  return true;
}();

using gpu_transport::active_gpu_device_index;
using gpu_transport::gpu_acquire_exclusive;
using gpu_transport::gpu_allocate_device_memory;
using gpu_transport::gpu_allocate_host_memory;
using gpu_transport::gpu_create_world;
using gpu_transport::gpu_dispose_world;
using gpu_transport::gpu_free_device_memory;
using gpu_transport::gpu_free_host_memory;
using gpu_transport::gpu_get_world_data;
using gpu_transport::gpu_get_world_device_index;
using gpu_transport::gpu_get_world_size;
using gpu_transport::gpu_memory_lifetimes_balanced;
using gpu_transport::gpu_purge_memory;
using gpu_transport::gpu_release_exclusive;

bool prepare_cuda_render_transport(void* input, void* output,
                                   CudaRenderTransport& transport) {
  return gpu_transport::prepare_render_transport(input, output, transport);
}

bool finish_cuda_render_transport(CudaRenderTransport& transport) {
  return gpu_transport::finish_render_transport(transport);
}

SuiteResolveResult resolve_scene_suite_provider(
    void*, const char* name, int32_t version, const void** suite) {
  if (scene_context()) {
    const SceneSuiteAcquireResult scene_result =
        scene_acquire_suite(name, version, suite);
    if (scene_result == SceneSuiteAcquireResult::acquired) {
      return SuiteResolveResult::acquired;
    }
    if (scene_result == SceneSuiteAcquireResult::rejected)
      return SuiteResolveResult::rejected_bad_param;
  }
  return SuiteResolveResult::not_found;
}

SuiteResolveResult resolve_legacy_host_suite(
    void*, const char* name, int32_t version, const void** suite) {
  if (name && version == 1 && std::strcmp(name, "PF Color Suite") == 0) {
    *suite = &g_color_suite8; return SuiteResolveResult::acquired;
  }
  if (name && version == 1 && std::strcmp(name, "PF Color16 Suite") == 0) {
    *suite = &g_color_suite16; return SuiteResolveResult::acquired;
  }
  if (name && version == 1 && std::strcmp(name, "PF ColorFloat Suite") == 0) {
    *suite = &g_color_suite_float; return SuiteResolveResult::acquired;
  }
  if (name && version == 1 && std::strcmp(name, "PF Batch Sampling Suite") == 0) {
    g_batch_sampling_suite1 = {&begin_sampling8, &end_sampling8,
                               &unsupported_batch_sample_func,
                               &unsupported_batch_sample_func};
    *suite = &g_batch_sampling_suite1;
    return SuiteResolveResult::acquired;
  }
  if (g_mask_model_enabled && name && std::strcmp(name, "PF Path Query Suite") == 0 && version == 1) {
    g_pf_path_query_suite1 = {
        reinterpret_cast<void*>(&pf_num_paths),
        reinterpret_cast<void*>(&pf_path_info),
        reinterpret_cast<void*>(&pf_checkout_path),
        reinterpret_cast<void*>(&pf_checkin_path)};
    *suite = g_pf_path_query_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (g_mask_model_enabled && name && std::strcmp(name, "PF Path Data Suite") == 0 && version == 1) {
    g_pf_path_data_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_pf_path_data_suite1[0] = reinterpret_cast<void*>(&pf_path_is_open);
    g_pf_path_data_suite1[1] = reinterpret_cast<void*>(&pf_path_num_segments);
    g_pf_path_data_suite1[2] = reinterpret_cast<void*>(&pf_path_vertex_info);
    g_pf_path_data_suite1[3] = reinterpret_cast<void*>(&pf_path_prepare_seg_length);
    g_pf_path_data_suite1[4] = reinterpret_cast<void*>(&pf_path_get_seg_length);
    g_pf_path_data_suite1[5] = reinterpret_cast<void*>(&pf_path_eval_seg_length);
    g_pf_path_data_suite1[6] = reinterpret_cast<void*>(&pf_path_eval_seg_length_deriv1);
    g_pf_path_data_suite1[7] = reinterpret_cast<void*>(&pf_path_cleanup_seg_length);
    g_pf_path_data_suite1[8] = reinterpret_cast<void*>(&pf_path_is_inverted);
    g_pf_path_data_suite1[9] = reinterpret_cast<void*>(&pf_path_get_mask_mode);
    g_pf_path_data_suite1[10] = reinterpret_cast<void*>(&pf_path_get_name);
    *suite = g_pf_path_data_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Duck Suite") == 0 && version == 1) {
    g_duck_suite1[0] = reinterpret_cast<void*>(&duck_quack);
    *suite = g_duck_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (g_aegp_init_mode && name && std::strcmp(name, "AEGP Command Suite") == 0 && version == 1) {
    *suite = &g_aegp_command_suite;
    return SuiteResolveResult::acquired;
  }
  if (g_aegp_init_mode && name && std::strcmp(name, "AEGP Register Suite") == 0 && version == 6) {
    *suite = &g_aegp_register_suite;
    return SuiteResolveResult::acquired;
  }

  if (name && std::strcmp(name, "PF Effect UI Suite") == 0 && version == 1) {
    g_effect_ui_suite1[0] = reinterpret_cast<void*>(&set_options_button_name);
    *suite = g_effect_ui_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF AE Adv App Suite") == 0 && version == 1) {
    g_adv_app_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_adv_app_suite1[6] = reinterpret_cast<void*>(&adv_app_info_text);
    g_adv_app_suite1[8] = reinterpret_cast<void*>(&adv_app_info_text3);
    *suite = g_adv_app_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF AE Adv App Suite") == 0 && version == 2) {
    g_adv_app_suite2.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_adv_app_suite2[6] = reinterpret_cast<void*>(&adv_app_info_text);
    g_adv_app_suite2[8] = reinterpret_cast<void*>(&adv_app_info_text3);
    *suite = g_adv_app_suite2.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "DRAWBOT Draw Suite") == 0 && version == 1) {
    g_drawbot_draw_suite1 = {reinterpret_cast<void*>(&drawbot_get_supplier),
                             reinterpret_cast<void*>(&drawbot_get_surface)};
    *suite = g_drawbot_draw_suite1.data(); return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "DRAWBOT Supplier Suite") == 0 && version == 1) {
    g_drawbot_supplier_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_drawbot_supplier_suite1[0] = reinterpret_cast<void*>(&drawbot_new_pen);
    g_drawbot_supplier_suite1[1] = reinterpret_cast<void*>(&drawbot_new_brush);
    g_drawbot_supplier_suite1[6] = reinterpret_cast<void*>(&drawbot_new_path);
    g_drawbot_supplier_suite1[12] = reinterpret_cast<void*>(&drawbot_release_object);
    *suite = g_drawbot_supplier_suite1.data(); return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "DRAWBOT Surface Suite") == 0 && version == 2) {
    g_drawbot_surface_suite2.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_drawbot_surface_suite2[2] = reinterpret_cast<void*>(&drawbot_paint_rect);
    g_drawbot_surface_suite2[3] = reinterpret_cast<void*>(&drawbot_fill_path);
    g_drawbot_surface_suite2[4] = reinterpret_cast<void*>(&drawbot_stroke_path);
    *suite = g_drawbot_surface_suite2.data(); return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "DRAWBOT Path Suite") == 0 && version == 1) {
    g_drawbot_path_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_drawbot_path_suite1[0] = reinterpret_cast<void*>(&drawbot_path_point);
    g_drawbot_path_suite1[1] = reinterpret_cast<void*>(&drawbot_path_point);
    g_drawbot_path_suite1[3] = reinterpret_cast<void*>(&drawbot_add_rect);
    *suite = g_drawbot_path_suite1.data(); return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Effect Custom UI Suite") == 0 && version == 1) {
    g_effect_custom_ui_suite1[0] = reinterpret_cast<void*>(&get_drawing_reference);
    *suite = g_effect_custom_ui_suite1.data(); return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Effect Custom UI Overlay Theme Suite") == 0 && version == 1) {
    g_effect_overlay_theme_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_effect_overlay_theme_suite1[0] = reinterpret_cast<void*>(&overlay_foreground);
    g_effect_overlay_theme_suite1[5] = reinterpret_cast<void*>(&overlay_stroke_path);
    *suite = g_effect_overlay_theme_suite1.data(); return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF AE App Suite") == 0 &&
      (version == 6 || version == 7 || version == 1)) {
    auto populate_app_suite = [&](auto& app_suite, bool has_language, bool has_progress) {
      app_suite.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
      std::size_t slot = 0;
      app_suite[slot++] = reinterpret_cast<void*>(&app_get_background_color);
      app_suite[slot++] = reinterpret_cast<void*>(&app_get_color);
      if (has_language) app_suite[slot++] = reinterpret_cast<void*>(&app_get_language);
      app_suite[slot++] = reinterpret_cast<void*>(&app_get_personal_info);
      app_suite[slot++] = reinterpret_cast<void*>(&app_get_font_style);
      app_suite[slot++] = reinterpret_cast<void*>(&app_set_cursor);
      app_suite[slot++] = reinterpret_cast<void*>(&app_is_render_engine);
      app_suite[slot++] = reinterpret_cast<void*>(&app_color_picker);
      app_suite[slot++] = reinterpret_cast<void*>(&app_get_mouse);
      app_suite[slot++] = reinterpret_cast<void*>(&app_invalidate_rect);
      app_suite[slot++] = reinterpret_cast<void*>(&app_convert_local_to_global);
      app_suite[slot++] = reinterpret_cast<void*>(&app_get_color_at_global_point);
      if (has_progress) {
        app_suite[slot++] = reinterpret_cast<void*>(&app_create_progress_dialog);
        app_suite[slot++] = reinterpret_cast<void*>(&app_update_progress_dialog);
        app_suite[slot++] = reinterpret_cast<void*>(&app_dispose_progress_dialog);
      }
    };
    if (version == 6) {
      populate_app_suite(g_app_suite4, false, false); *suite = g_app_suite4.data();
    } else if (version == 7) {
      populate_app_suite(g_app_suite5, true, false); *suite = g_app_suite5.data();
    } else {
      populate_app_suite(g_app_suite6, true, true); *suite = g_app_suite6.data();
    }
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF AE Channel Suite") == 0 && version == 1) {
    *suite = &g_channel_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Effect Sequence Data Suite") == 0 && version == 1) {
    *suite = &g_effect_sequence_data_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Handle Suite") == 0 && version == 2) {
    *suite = &g_handle_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF GPU Device Suite") == 0 && version == 1) {
    *suite = g_gpu_device_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF ANSI Suite") == 0 && version == 1) {
    g_ansi_suite1[0] = reinterpret_cast<void*>(&ansi_atan);
    g_ansi_suite1[1] = reinterpret_cast<void*>(&ansi_atan2);
    g_ansi_suite1[2] = reinterpret_cast<void*>(&ansi_ceil);
    g_ansi_suite1[3] = reinterpret_cast<void*>(&ansi_cos);
    g_ansi_suite1[4] = reinterpret_cast<void*>(&ansi_exp);
    g_ansi_suite1[5] = reinterpret_cast<void*>(&ansi_fabs);
    g_ansi_suite1[6] = reinterpret_cast<void*>(&ansi_floor);
    g_ansi_suite1[7] = reinterpret_cast<void*>(&ansi_fmod);
    g_ansi_suite1[8] = reinterpret_cast<void*>(&ansi_hypot);
    g_ansi_suite1[9] = reinterpret_cast<void*>(&ansi_log);
    g_ansi_suite1[10] = reinterpret_cast<void*>(&ansi_log10);
    g_ansi_suite1[11] = reinterpret_cast<void*>(&ansi_pow);
    g_ansi_suite1[12] = reinterpret_cast<void*>(&ansi_sin);
    g_ansi_suite1[13] = reinterpret_cast<void*>(&ansi_sqrt);
    g_ansi_suite1[14] = reinterpret_cast<void*>(&ansi_tan);
    g_ansi_suite1[15] = reinterpret_cast<void*>(&ansi_sprintf);
    g_ansi_suite1[16] = reinterpret_cast<void*>(&ansi_strcpy);
    g_ansi_suite1[17] = reinterpret_cast<void*>(&ansi_asin);
    g_ansi_suite1[18] = reinterpret_cast<void*>(&ansi_acos);
    *suite = g_ansi_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (is_render_worker() && name &&
      std::strcmp(name, "PF AE Adv Item Suite") == 0 && version == 1) {
    *suite = &g_adv_item_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Pixel Data Suite") == 0 && version == 2) {
    *suite = &g_pixel_data_suite2;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Pixel Data Suite") == 0 && version == 1) {
    *suite = &g_pixel_data_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF World Suite") == 0 && version == 2) {
    *suite = &g_world_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF World Suite") == 0 && version == 1) {
    *suite = g_world_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Pixel Format Suite") == 0 && version == 2) {
    *suite = &g_pixel_format_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF PointParamSuite") == 0 && version == 1) {
    *suite = &g_point_param_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF AngleParamSuite") == 0 && version == 1) {
    *suite = &g_angle_param_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF ColorParamSuite") == 0 && version == 1) {
    *suite = &g_color_param_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Param Utils Suite") == 0 && version == 3) {
    *suite = &g_param_utils_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Param Utils Suite") == 0 && version == 2) {
    *suite = &g_param_utils_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Color Settings Suite") == 0 && version == 7) {
    g_color_settings_suite6 = {&color_get_blending_tables, &color_does_view_have_xform,
        &color_xform_working_to_view, &color_get_new_working_space_profile,
        &color_get_new_profile_from_icc, &color_get_new_icc_from_profile,
        &color_get_new_profile_description, &color_dispose_profile,
        &color_get_profile_approximate_gamma, &color_is_rgb_profile,
        &color_set_working_color_space, &color_is_ocio_used,
        &color_get_ocio_configuration_file, &color_get_ocio_configuration_file_path,
        &color_get_ocio_working_colorspace, &color_get_ocio_display_colorspace,
        &color_is_colorspace_aware_effects_enabled, &color_get_lut_interpolation_method,
        &color_get_graphics_white_luminance, &color_get_working_colorspace_id};
    *suite = &g_color_settings_suite6;
    return SuiteResolveResult::acquired;
  }
  if (g_mask_model_enabled && name && std::strcmp(name, "AEGP Mask Suite") == 0 &&
      version == 1) {
    *suite = &g_pf_mask_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Iterate8 Suite") == 0 &&
      (version == 1 || version == 2)) {
    g_iterate8_suite2.iterate = reinterpret_cast<void*>(&iterate_world8);
    *suite = &g_iterate8_suite2;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF iterate16 Suite") == 0 &&
      (version == 1 || version == 2)) {
    *suite = &g_iterate16_suite2;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF iterateFloat Suite") == 0 &&
      (version == 1 || version == 2)) {
    *suite = &g_iterate_float_suite2;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Sampling8 Suite") == 0 && version == 1) {
    g_sampling8_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_sampling8_suite1[0] = reinterpret_cast<void*>(&nearest_sample8);
    g_sampling8_suite1[1] = reinterpret_cast<void*>(&subpixel_sample8);
    g_sampling8_suite1[2] = reinterpret_cast<void*>(&area_sample8);
    *suite = g_sampling8_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Sampling16 Suite") == 0 &&
      version == 1) {
    g_sampling16_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_sampling16_suite1[0] = reinterpret_cast<void*>(&nearest_sample16);
    g_sampling16_suite1[1] = reinterpret_cast<void*>(&subpixel_sample16);
    g_sampling16_suite1[2] = reinterpret_cast<void*>(&area_sample16);
    *suite = g_sampling16_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF SamplingFloat Suite") == 0 &&
      version == 1) {
    g_sampling_float_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_sampling_float_suite1[0] = reinterpret_cast<void*>(&nearest_sample_float);
    g_sampling_float_suite1[1] = reinterpret_cast<void*>(&subpixel_sample_float);
    g_sampling_float_suite1[2] = reinterpret_cast<void*>(&area_sample_float);
    *suite = g_sampling_float_suite1.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF World Transform Suite") == 0 &&
      version == 1) {
    g_world_transform_suite1.composite_rect = &composite_rect8;
    g_world_transform_suite1.blend = &blend_world;
    g_world_transform_suite1.convolve = &convolve_world;
    g_world_transform_suite1.copy = &copy_world8;
    g_world_transform_suite1.copy_hq = &copy_world_hq;
    g_world_transform_suite1.transfer_rect = &transfer_rect;
    g_world_transform_suite1.transform_world = &transform_world;
    *suite = &g_world_transform_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Fill Matte Suite") == 0 &&
      version == 2) {
    g_fill_matte_suite2.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_fill_matte_suite2[0] = reinterpret_cast<void*>(&fill_world8);
    g_fill_matte_suite2[1] = reinterpret_cast<void*>(&fill_world16);
    g_fill_matte_suite2[2] = reinterpret_cast<void*>(&fill_world_float);
    g_fill_matte_suite2[3] = reinterpret_cast<void*>(&premultiply_world8);
    g_fill_matte_suite2[4] = reinterpret_cast<void*>(&premultiply_color8);
    g_fill_matte_suite2[5] = reinterpret_cast<void*>(&premultiply_color16);
    g_fill_matte_suite2[6] = reinterpret_cast<void*>(&premultiply_color_float);
    *suite = g_fill_matte_suite2.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Dynamic Stream Suite") == 0 && version == 2) {
    g_aegp_dynamic_stream_suite2.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
    g_aegp_dynamic_stream_suite2[5] = reinterpret_cast<void*>(&aegp_set_dynamic_stream_flag_v2);
    *suite = g_aegp_dynamic_stream_suite2.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP PF Interface Suite") == 0 && version == 1) {
    *suite = &g_pf_interface_suite;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Layer Render Options Suite") == 0 && version == 1) {
    g_layer_render_options_suite1 = {&new_layer_render_options,
        &new_from_upstream_of_effect, &duplicate_layer_render_options,
        &dispose_layer_render_options, &set_layer_render_time, &get_layer_render_time,
        &set_layer_render_time_step, &get_layer_render_time_step,
        &set_layer_render_world_type, &get_layer_render_world_type,
        &set_layer_render_downsample, &get_layer_render_downsample,
        &set_layer_render_matte, &get_layer_render_matte};
    *suite = &g_layer_render_options_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Render Options Suite") == 0 && version == 1) {
    g_render_options_suite1 = {&render_options_new_from_item, &render_options_duplicate,
        &render_options_dispose, &render_options_set_time, &render_options_get_time,
        &render_options_set_time_step, &render_options_get_time_step,
        &render_options_set_field, &render_options_get_field,
        &render_options_set_world_type, &render_options_get_world_type,
        &render_options_set_downsample, &render_options_get_downsample,
        &render_options_set_roi, &render_options_get_roi,
        &render_options_set_matte, &render_options_get_matte};
    *suite = &g_render_options_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Layer Render Options Suite") == 0 && version == 2) {
    g_layer_render_options_suite2 = {&new_layer_render_options,
        &new_from_upstream_of_effect, &new_from_downstream_of_effect,
        &duplicate_layer_render_options, &dispose_layer_render_options,
        &set_layer_render_time, &get_layer_render_time,
        &set_layer_render_time_step, &get_layer_render_time_step,
        &set_layer_render_world_type, &get_layer_render_world_type,
        &set_layer_render_downsample, &get_layer_render_downsample,
        &set_layer_render_matte, &get_layer_render_matte};
    *suite = &g_layer_render_options_suite2;
    return SuiteResolveResult::acquired;
  }
  if (is_render_worker() && g_loaded_effect_receipt_context.entry && name &&
      std::strcmp(name, "AEGP Render Options Suite") == 0 && version == 4) {
    g_render_options_suite4 = {&render_options_new_from_item, &render_options_duplicate,
        &render_options_dispose, &render_options_set_time, &render_options_get_time,
        &render_options_set_time_step, &render_options_get_time_step,
        &render_options_set_field, &render_options_get_field,
        &render_options_set_world_type, &render_options_get_world_type,
        &render_options_set_downsample, &render_options_get_downsample,
        &render_options_set_roi, &render_options_get_roi,
        &render_options_set_matte, &render_options_get_matte,
        &render_options_set_channel_order, &render_options_get_channel_order,
        &render_options_get_guide_layers, &render_options_set_guide_layers,
        &render_options_get_quality, &render_options_set_quality};
    *suite = &g_render_options_suite4;
    return SuiteResolveResult::acquired;
  }
  bool allow_render_suite2 = g_aegp_command_roundtrip_mode;
  allow_render_suite2 = allow_render_suite2 ||
      (is_render_worker() && g_loaded_effect_receipt_context.entry != nullptr);
  if (allow_render_suite2 && name &&
      std::strcmp(name, "AEGP Render Suite") == 0 && version == 2) {
    g_aegp_render_suite2 = {&render_checkout_frame_reject, &checkin_frame,
        &get_receipt_world, &render_get_region_reject, &render_sufficient_reject,
        &render_sound_reject, &render_timestamp_reject, &render_changed_reject,
        &render_worthwhile_reject, &render_checkin_rendered};
    *suite = &g_aegp_render_suite2;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "PF Effect Custom UI Suite") == 0 && version == 2) {
    g_effect_custom_ui_suite2[0] = reinterpret_cast<void*>(&get_drawing_reference);
    g_effect_custom_ui_suite2[1] = reinterpret_cast<void*>(&get_context_async_manager);
    *suite = g_effect_custom_ui_suite2.data();
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Render Asyc Manager Suite") == 0 && version == 1) {
    g_render_async_manager_suite1 = {&checkout_item_frame_async, &checkout_layer_frame_async};
    *suite = &g_render_async_manager_suite1;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Render Suite") == 0 && version == 5) {
    g_aegp_render_suite4 = {&render_checkout_frame_reject, &render_checkout_layer_reject,
        &checkin_frame, &get_receipt_world, &render_get_region_reject,
        &render_sufficient_reject, &render_sound_reject, &render_timestamp_reject,
        &render_changed_reject, &render_worthwhile_reject,
        &render_checkin_rendered, &render_guid_reject};
    *suite = &g_aegp_render_suite4;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP Render Suite") == 0 && version == 8) {
    g_aegp_render_suite5 = {&render_checkout_frame_reject, &render_checkout_layer_v5,
        &render_checkout_layer_async_reject, &render_cancel_async_reject,
        &checkin_frame, &get_receipt_world, &render_get_region_reject,
        &render_sufficient_reject, &render_sound_reject, &render_timestamp_reject,
        &render_changed_reject, &render_worthwhile_reject,
        &render_checkin_rendered, &render_guid_reject};
    *suite = &g_aegp_render_suite5;
    return SuiteResolveResult::acquired;
  }
  if (name && std::strcmp(name, "AEGP World Suite") == 0 && version == 3) {
    auto& world_suite3 = aexcompat::suite_abi::aegp_world_suite3_table();
    world_suite3 = {&aegp_world_new_owned, &aegp_world_dispose,
        &aegp_world_get_type, &aegp_world_get_size, &aegp_world_get_rowbytes,
        &aegp_world_get_base_addr8, &aegp_world_get_base_addr16,
        &aegp_world_get_base_addr32, &aegp_world_fill_pf_world,
        &aegp_world_fast_blur, &aegp_world_new_platform,
        &aegp_world_dispose_platform, &aegp_world_reference_platform};
    *suite = &world_suite3;
    return SuiteResolveResult::acquired;
  }
  if (!g_mask_model_enabled || !name) return SuiteResolveResult::not_found;
  if (std::strcmp(name, "AEGP PF Interface Suite") == 0 && version == 1)
    *suite = &g_pf_interface_suite;
  else if (std::strcmp(name, "AEGP Layer Mask Suite") == 0 && version == 6)
    *suite = &g_mask_suite5;
  else if (std::strcmp(name, "AEGP Layer Mask Suite") == 0 && version == 7)
    *suite = &g_mask_suite;
  else if (std::strcmp(name, "AEGP Stream Suite") == 0 && version == 11)
    *suite = &g_stream_suite;
  else if (std::strcmp(name, "AEGP Keyframe Suite") == 0 && version == 5)
    *suite = &g_keyframe_suite;
  else if (std::strcmp(name, "AEGP Dynamic Stream Suite") == 0 && version == 5)
    *suite = &g_dynamic_stream_suite;
  else if (std::strcmp(name, "AEGP Mask Outline Suite") == 0 && version == 5)
    *suite = &g_mask_outline_suite;
  else {
    return SuiteResolveResult::not_found;
  }
  return SuiteResolveResult::acquired;
}

int32_t __cdecl acquire_suite(const char* name, int32_t version,
                              const void** suite) {
  using namespace aexcompat::worker_runtime::host_suites;
  const StaticSuite component_suites[] = {
      {"AE Plugin Helper Suite", 1, aexcompat::pf_helper::suite1()},
      {"AE Plugin Helper Suite2", 2, aexcompat::pf_helper::suite2()},
      {"PF Cache On Load Suite", 1, &cache_on_load_suite()},
      {"PF AE Adv Time Suite", 1,
       aexcompat::worker_runtime::pf_adv_time::suite(1)},
      {"PF AE Adv Time Suite", 2,
       aexcompat::worker_runtime::pf_adv_time::suite(2)},
      {"PF AE Adv Time Suite", 3,
       aexcompat::worker_runtime::pf_adv_time::suite(3)},
      {"PF AE Adv Time Suite", 4,
       aexcompat::worker_runtime::pf_adv_time::suite(4)},
      {"AEGP Memory Suite", 1, &g_aegp_memory_suite},
      {"AEGP Utility Suite", 7, &g_utility_suite3},
      {"AEGP Utility Suite", 13, &g_utility_suite},
  };
  StaticProviderCatalog component_catalog{
      component_suites, std::size(component_suites)};
  const Provider providers[] = {
      {&resolve_scene_suite_provider, nullptr},
      {&resolve_static_provider, &component_catalog},
  };
  const ProviderCatalog catalog{providers, std::size(providers),
                                &resolve_legacy_host_suite, nullptr};
  return acquire_host_suite(catalog, name, version, suite, g_trace_writer);
}

int32_t __cdecl release_suite(const char* name, int32_t version) {
  return aexcompat::worker_runtime::host_suites::release_host_suite(
      name, version, g_trace_writer);
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

struct BasicSuite {
  decltype(&acquire_suite) acquire;
  decltype(&release_suite) release;
  void* unsupported[5]{};
};
BasicSuite g_basic_suite{&acquire_suite, &release_suite};

int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr) {
  uint32_t local_exception{};
  uint32_t* observed_exception = exception_code ? exception_code : &local_exception;
  if (selector == kSequenceSetdown) invalidate_effect_sequence(&g_effect);
  const int32_t error = invoke_entry_seh(entry, selector, input, output, nullptr, nullptr,
                                         nullptr, observed_exception);
  if (selector == kSequenceSetup || selector == kSequenceResetup) {
    if (error == 0 && *observed_exception == 0) {
      void* sequence_handle{};
      std::memcpy(&sequence_handle,
                  static_cast<const std::byte*>(output) + kOutSequenceData,
                  sizeof(sequence_handle));
      if (!sequence_handle) {
        invalidate_effect_sequence(&g_effect);
      } else if (!publish_effect_sequence(&g_effect, sequence_handle)) {
        invalidate_effect_sequence(&g_effect);
        return kPfBadCallbackParam;
      }
    } else {
      invalidate_effect_sequence(&g_effect);
    }
  }
  return error;
}

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

bool dispatch_render_click(EffectEntry entry, std::array<std::byte, kInSize>& input,
                           std::array<std::byte, kOutSize>& output,
                           std::vector<std::array<std::byte, kParamSize>>& definitions) {
  if (!g_render_click_enabled) return true;
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  std::array<std::byte, 208> extra{};
  write<void*>(extra, 0, &g_ui_context_pointer);
  g_ui_context.window_type = 2;
  const auto dispatch_lifecycle_event = [&](int32_t event_type, std::size_t result_index) {
    write<int32_t>(extra, 8, event_type);
    uint32_t exception_code = 0;
    g_render_ui_lifecycle_errors[result_index] = invoke_entry_seh(entry, kEvent,
        input.data(), output.data(), params.data(), nullptr, extra.data(), &exception_code);
    if (exception_code != 0) g_render_ui_lifecycle_errors[result_index] = 512;
    return g_render_ui_lifecycle_errors[result_index] == 0;
  };
  if (!dispatch_lifecycle_event(0, 0) || !dispatch_lifecycle_event(1, 1)) return false;
  g_render_ui_context_active = true;
  write<int32_t>(extra, 8, 2);
  write<uint32_t>(extra, 16, 1);
  write<int32_t>(extra, 20, g_render_click_y);
  write<int32_t>(extra, 24, g_render_click_x);
  write<int32_t>(extra, 28, 1);
  write<int32_t>(extra, 80, 1);
  write<int32_t>(extra, 84, 2);
  write_rect(extra.data() + 88, 203, 203);
  uint32_t exception_code = 0;
  g_render_click_error = invoke_entry_seh(entry, kEvent, input.data(), output.data(),
      params.data(), nullptr, extra.data(), &exception_code);
  if (exception_code != 0) g_render_click_error = 512;
  g_render_click_out_flags = read<int32_t>(extra, 204);
  g_render_click_changed_value = definitions.size() > 1 &&
      (read<uint32_t>(definitions[1], 0) & 1u) != 0;
  return g_render_click_error == 0 && (g_render_click_out_flags & 9) == 9 &&
      g_render_click_changed_value && g_app_color_picker_calls == 1 &&
      g_app_invalidate_rect_calls == 1;
}

bool dispatch_render_draw(EffectEntry entry, std::array<std::byte, kInSize>& input,
                          std::array<std::byte, kOutSize>& output,
                          std::vector<std::array<std::byte, kParamSize>>& definitions) {
  if (!g_render_draw_enabled) return true;
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  std::array<std::byte, 208> extra{};
  write<void*>(extra, 0, &g_ui_context_pointer);
  g_ui_context.window_type = 2;
  const auto dispatch_lifecycle_event = [&](int32_t event_type, std::size_t result_index) {
    write<int32_t>(extra, 8, event_type);
    uint32_t exception_code = 0;
    g_render_ui_lifecycle_errors[result_index] = invoke_entry_seh(entry, kEvent,
        input.data(), output.data(), params.data(), nullptr, extra.data(), &exception_code);
    if (exception_code != 0) g_render_ui_lifecycle_errors[result_index] = 512;
    return g_render_ui_lifecycle_errors[result_index] == 0;
  };
  if (!dispatch_lifecycle_event(0, 0) || !dispatch_lifecycle_event(1, 1)) return false;
  g_render_ui_context_active = true;
  write<int32_t>(extra, 8, 4);
  write_rect(extra.data() + 16, 203, 203);
  write<int32_t>(extra, 32, 32);
  uint32_t exception_code = 0;
  g_render_draw_error = invoke_entry_seh(entry, kEvent, input.data(), output.data(),
      params.data(), nullptr, extra.data(), &exception_code);
  if (exception_code != 0) g_render_draw_error = 512;
  g_render_draw_out_flags = read<int32_t>(extra, 204);
  return g_render_draw_error == 0 && (g_render_draw_out_flags & 1) != 0 &&
      (g_drawbot_paint_rect_calls + g_drawbot_fill_path_calls +
       g_drawbot_stroke_path_calls + g_overlay_stroke_path_calls) > 0 &&
      g_drawbot_objects_created == g_drawbot_objects_released &&
      g_drawbot_objects.empty() && g_drawbot_invalid_operations == 0;
}

bool close_render_ui_context(EffectEntry entry, std::array<std::byte, kInSize>& input,
                             std::array<std::byte, kOutSize>& output,
                             std::vector<std::array<std::byte, kParamSize>>& definitions) {
  if ((!g_render_click_enabled && !g_render_draw_enabled) || !g_render_ui_context_active)
    return !g_render_click_enabled && !g_render_draw_enabled;
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  std::array<std::byte, 208> extra{};
  write<void*>(extra, 0, &g_ui_context_pointer);
  for (const auto [event_type, result_index] :
       std::array<std::pair<int32_t, std::size_t>, 2>{{{5, 2}, {6, 3}}}) {
    write<int32_t>(extra, 8, event_type);
    uint32_t exception_code = 0;
    g_render_ui_lifecycle_errors[result_index] = invoke_entry_seh(entry, kEvent,
        input.data(), output.data(), params.data(), nullptr, extra.data(), &exception_code);
    if (exception_code != 0) g_render_ui_lifecycle_errors[result_index] = 512;
  }
  g_render_ui_context_active = false;
  for (auto& state : g_ui_context.plugin_state) state = 0;
  g_render_ui_context_closed = std::all_of(g_render_ui_lifecycle_errors.begin(),
      g_render_ui_lifecycle_errors.end(), [](int32_t error) { return error == 0; });
  return g_render_ui_context_closed;
}

int32_t __cdecl add_param(void*, int32_t index, void* definition) {
  if (!definition || g_params.size() >= kMaxParams) return 4;
  std::array<std::byte, kParamSize> bytes{};
  std::memcpy(bytes.data(), definition, bytes.size());
  const char* name = reinterpret_cast<const char*>(bytes.data() + kParamName);
  const auto length = strnlen_s(name, kParamNameSize);
  const int32_t host_index = index < 0 ? static_cast<int32_t>(g_params.size() + 1) : index;
  if (host_index <= 0 || host_index > static_cast<int32_t>(kMaxParams) ||
      std::any_of(g_params.begin(), g_params.end(),
                  [host_index](const auto& param) { return param.index == host_index; })) return 4;
  ParamRecord record{host_index, read<int32_t>(bytes, 0), read<int32_t>(bytes, kParamType),
                     read<uint32_t>(bytes, kParamFlags), std::string(name, length)};
  constexpr std::size_t u = 56;
  if (record.type == 1) {
    record.has_numeric = true;
    record.valid_min = read<int32_t>(bytes, u + 68);
    record.valid_max = read<int32_t>(bytes, u + 72);
    record.slider_min = read<int32_t>(bytes, u + 76);
    record.slider_max = read<int32_t>(bytes, u + 80);
    record.default_value = read<int32_t>(bytes, u + 84);
  } else if (record.type == 2) {
    record.has_numeric = true;
    record.has_current = true;
    record.current_value = read<int32_t>(bytes, u) / 65536.0;
    record.valid_min = read<int32_t>(bytes, u + 68) / 65536.0;
    record.valid_max = read<int32_t>(bytes, u + 72) / 65536.0;
    record.slider_min = read<int32_t>(bytes, u + 76) / 65536.0;
    record.slider_max = read<int32_t>(bytes, u + 80) / 65536.0;
    record.default_value = read<int32_t>(bytes, u + 84) / 65536.0;
    record.precision = read<int16_t>(bytes, u + 88);
  } else if (record.type == 7) {
    record.has_numeric = true;
    record.valid_min = 1;
    record.valid_max = read<int16_t>(bytes, u + 4);
    record.slider_min = record.valid_min;
    record.slider_max = record.valid_max;
    record.default_value = read<int16_t>(bytes, u + 6);
    const char* choices = read<const char*>(bytes, u + 8);
    if (choices) record.choices.assign(choices, strnlen_s(choices, 4096));
  } else if (record.type == 4) {
    record.has_numeric = true;
    record.has_current = true;
    record.valid_min = 0;
    record.valid_max = 1;
    record.slider_min = 0;
    record.slider_max = 1;
    record.default_value = read<uint8_t>(bytes, u + 4) ? 1 : 0;
    record.current_value = read<int32_t>(bytes, u) != 0 ? 1 : 0;
    const char* label = read<const char*>(bytes, u + 8);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  } else if (record.type == 0) {
    record.layer_default = read<int32_t>(bytes, u + 116);
  } else if (record.type == 12) {
    record.has_numeric = true;
    record.valid_min = 0;
    record.valid_max = 1024;
    record.slider_min = 0;
    record.slider_max = 1024;
    record.default_value = read<int32_t>(bytes, u + 8);
  } else if (record.type == 10) {
    record.has_numeric = true;
    record.valid_min = read<float>(bytes, u + 48);
    record.valid_max = read<float>(bytes, u + 52);
    record.slider_min = read<float>(bytes, u + 56);
    record.slider_max = read<float>(bytes, u + 60);
    record.default_value = read<float>(bytes, u + 64);
    record.precision = read<int16_t>(bytes, u + 68);
  } else if (record.type == 5) {
    record.has_color = true;
    std::memcpy(record.current_color.data(), bytes.data() + u, record.current_color.size());
    std::memcpy(record.default_color.data(), bytes.data() + u + 4, record.default_color.size());
    for (std::size_t channel = 0; channel < 4; ++channel) {
      record.current_float_color[channel] = record.current_color[channel] / 255.0f;
      record.default_float_color[channel] = record.default_color[channel] / 255.0f;
    }
  } else if (record.type == 3) {
    record.component_count = 1;
    record.current_components[0] = read<int32_t>(bytes, u) / 65536.0;
    record.default_components[0] = read<int32_t>(bytes, u + 4) / 65536.0;
  } else if (record.type == 6) {
    record.component_count = 2;
    record.current_components[0] = read<int32_t>(bytes, u) / 65536.0;
    record.current_components[1] = read<int32_t>(bytes, u + 4) / 65536.0;
    record.default_components[0] = read<int32_t>(bytes, u + 12) / 65536.0;
    record.default_components[1] = read<int32_t>(bytes, u + 16) / 65536.0;
  } else if (record.type == 18) {
    record.component_count = 3;
    for (int component = 0; component < 3; ++component) {
      record.current_components[component] = read<double>(bytes, u + component * 8);
      record.default_components[component] = read<double>(bytes, u + 24 + component * 8);
    }
  } else if (record.type == 15) {
    const char* label = read<const char*>(bytes, u + 8);
    if (label) record.label.assign(label, strnlen_s(label, 4096));
  }
  record.raw = bytes;
  g_params.push_back(std::move(record));
  return 0;
}

int32_t __cdecl checkout_param(void*, int32_t index, int32_t what_time, int32_t time_step,
                               uint32_t time_scale, void* definition) {
  if (!definition || time_step <= 0 || time_scale == 0) return 4;
  auto* classic_context = aexcompat::worker_runtime::classic::active_context();
  if (!classic_context && aexcompat::worker_runtime::classic::dispatch_active()) return 4;
  if (classic_context && !classic_context->checkout_time_allowed(what_time, time_scale))
    return 4;
  if (!classic_context) {
    const bool current_time = static_cast<int64_t>(what_time) * g_checkout_current_time_scale ==
        static_cast<int64_t>(g_checkout_current_time) * time_scale;
    if (!current_time && !g_wide_time_checkout_allowed) {
      ++g_rejected_temporal_param_checkouts;
      return 4;
    }
  }
  const auto record_checkout = [&] {
    if (classic_context) {
      classic_context->record_checkout(definition, index, what_time, time_step, time_scale);
      return;
    }
    std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
    ++g_live_param_checkouts[definition];
    ++g_param_checkout_calls;
    g_last_param_checkout_index = index;
    g_last_param_checkout_time = what_time;
    g_last_param_checkout_time_step = time_step;
    g_last_param_checkout_time_scale = time_scale;
  };
  if (classic_context && classic_context->copy_timed_layer(
          index, what_time, time_scale, definition, kParamSize)) {
    record_checkout();
    return 0;
  }
  if (classic_context && classic_context->has_timed_slot(index)) return 4;
  if (classic_context) {
    if (classic_context->copy_definition(index, definition, kParamSize) ||
        classic_context->copy_fallback_definition(index, definition, kParamSize)) {
      record_checkout();
      return 0;
    }
    return 4;
  }
  const auto hosted = g_checkout_layer_definitions.find(index);
  if (hosted != g_checkout_layer_definitions.end()) {
    std::memcpy(definition, hosted->second.data(), hosted->second.size());
    record_checkout();
    return 0;
  }
  return 4;
}

constexpr int32_t kPfErrBadCallbackParam = 516;

uint8_t composite_divide_255(uint32_t numerator) {
  return static_cast<uint8_t>(std::min<uint32_t>(255, (numerator + 127) / 255));
}

uint8_t composite_divide_65025(uint64_t numerator) {
  return static_cast<uint8_t>(std::min<uint64_t>(255, (numerator + 32'512) / 65'025));
}

int32_t __cdecl composite_rect8_legacy(void* effect_ref, LegacyRect* source_rect,
                                int32_t source_opacity, void* source_world,
                                int32_t destination_x, int32_t destination_y,
                                int32_t field, int32_t transfer_mode,
                                void* destination_world) {
  constexpr int32_t kFieldFrame = 0;
  constexpr int32_t kFieldUpper = 1;
  constexpr int32_t kFieldLower = 2;
  constexpr int32_t kTransferCopy = 0;
  constexpr int32_t kTransferBehind = 1;
  constexpr int32_t kTransferInFront = 2;
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      (field != kFieldFrame && field != kFieldUpper && field != kFieldLower) ||
      (transfer_mode != kTransferCopy && transfer_mode != kTransferBehind &&
       transfer_mode != kTransferInFront)) {
    return kPfErrBadCallbackParam;
  }

  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!bounded_argb8_world(source_world, source, source_rowbytes, source_width, source_height) ||
      !bounded_argb8_world(destination_world, destination, destination_rowbytes,
                           destination_width, destination_height)) {
    return kPfErrBadCallbackParam;
  }
  if (source_rect->right < source_rect->left || source_rect->bottom < source_rect->top) {
    return kPfErrBadCallbackParam;
  }

  // Map the requested source rectangle's upper-left to the destination, then clip in 64-bit.
  const int64_t source_left = std::max<int64_t>(source_rect->left, 0);
  const int64_t source_top = std::max<int64_t>(source_rect->top, 0);
  const int64_t source_right = std::min<int64_t>(source_rect->right, source_width);
  const int64_t source_bottom = std::min<int64_t>(source_rect->bottom, source_height);
  const int64_t destination_left = static_cast<int64_t>(destination_x) +
      source_left - source_rect->left;
  const int64_t destination_top = static_cast<int64_t>(destination_y) +
      source_top - source_rect->top;
  const int64_t clipped_destination_left = std::max<int64_t>(destination_left, 0);
  const int64_t clipped_destination_top = std::max<int64_t>(destination_top, 0);
  const int64_t clipped_destination_right = std::min<int64_t>(
      destination_left + (source_right - source_left), destination_width);
  const int64_t clipped_destination_bottom = std::min<int64_t>(
      destination_top + (source_bottom - source_top), destination_height);
  if (source_right <= source_left || source_bottom <= source_top ||
      clipped_destination_right <= clipped_destination_left ||
      clipped_destination_bottom <= clipped_destination_top || source_opacity == 0) {
    return 0;
  }

  const int64_t clipped_source_left = source_left + clipped_destination_left - destination_left;
  const int64_t clipped_source_top = source_top + clipped_destination_top - destination_top;
  const std::size_t width = static_cast<std::size_t>(
      clipped_destination_right - clipped_destination_left);
  const std::size_t height = static_cast<std::size_t>(
      clipped_destination_bottom - clipped_destination_top);
  if (width > 4096 || height > 4096 || width > SIZE_MAX / 4 ||
      height > SIZE_MAX / (width * 4)) {
    return kPfErrBadCallbackParam;
  }

  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(width * height * 4);
  } catch (const std::bad_alloc&) {
    return 4;
  }
  for (std::size_t row = 0; row < height; ++row) {
    std::memcpy(snapshot.data() + row * width * 4,
                source + static_cast<std::size_t>(clipped_source_top + row) * source_rowbytes +
                    static_cast<std::size_t>(clipped_source_left) * 4,
                width * 4);
  }

  const uint32_t opacity = static_cast<uint32_t>(source_opacity);
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t output_y = clipped_destination_top + static_cast<int64_t>(row);
    if ((field == kFieldUpper && (output_y & 1) != 0) ||
        (field == kFieldLower && (output_y & 1) == 0)) {
      continue;
    }
    for (std::size_t column = 0; column < width; ++column) {
      const auto* input = snapshot.data() + (row * width + column) * 4;
      auto* output = destination + static_cast<std::size_t>(output_y) * destination_rowbytes +
          static_cast<std::size_t>(clipped_destination_left + column) * 4;
      if (transfer_mode == kTransferCopy) {
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_255(
              static_cast<uint32_t>(input[channel]) * opacity +
              static_cast<uint32_t>(output[channel]) * (255 - opacity));
        }
      } else if (transfer_mode == kTransferInFront) {
        const uint32_t destination_weight = 65'025 - input[0] * opacity;
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_65025(
              static_cast<uint64_t>(input[channel]) * opacity * 255 +
              static_cast<uint64_t>(output[channel]) * destination_weight);
        }
      } else {
        const uint32_t source_weight = opacity * (255 - output[0]);
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_65025(
              static_cast<uint64_t>(output[channel]) * 65'025 +
              static_cast<uint64_t>(input[channel]) * source_weight);
        }
      }
    }
  }
  return 0;
}

template <typename Channel, uint32_t Maximum>
int32_t composite_rect_registered(void* effect_ref, LegacyRect* source_rect,
                                  int32_t source_opacity, void* source_world,
                                  int32_t destination_x, int32_t destination_y,
                                  int32_t field, int32_t transfer_mode,
                                  void* destination_world) {
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      field < 0 || field > 2 || transfer_mode < 0 || transfer_mode > 2 ||
      source_rect->right < source_rect->left || source_rect->bottom < source_rect->top)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format ||
      source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * sizeof(Channel) * 4 ||
      destination_info.rowbytes <
          static_cast<int64_t>(destination_info.width) * sizeof(Channel) * 4)
    return kPfErrBadCallbackParam;

  const int64_t sl = std::max<int64_t>(source_rect->left, 0);
  const int64_t st = std::max<int64_t>(source_rect->top, 0);
  const int64_t sr = std::min<int64_t>(source_rect->right, source_info.width);
  const int64_t sb = std::min<int64_t>(source_rect->bottom, source_info.height);
  const int64_t dl = static_cast<int64_t>(destination_x) + sl - source_rect->left;
  const int64_t dt = static_cast<int64_t>(destination_y) + st - source_rect->top;
  const int64_t cdl = std::max<int64_t>(dl, 0);
  const int64_t cdt = std::max<int64_t>(dt, 0);
  const int64_t cdr = std::min<int64_t>(dl + sr - sl, destination_info.width);
  const int64_t cdb = std::min<int64_t>(dt + sb - st, destination_info.height);
  if (sr <= sl || sb <= st || cdr <= cdl || cdb <= cdt || source_opacity == 0) return 0;
  const int64_t csl = sl + cdl - dl;
  const int64_t cst = st + cdt - dt;
  const std::size_t width = static_cast<std::size_t>(cdr - cdl);
  const std::size_t height = static_cast<std::size_t>(cdb - cdt);
  if (width > SIZE_MAX / height || width * height > 16'777'216) return kPfErrBadCallbackParam;

  using Pixel = std::array<Channel, 4>;
  std::vector<Pixel> snapshot;
  try { snapshot.resize(width * height); } catch (const std::bad_alloc&) { return 4; }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  for (std::size_t row = 0; row < height; ++row)
    std::memcpy(snapshot.data() + row * width,
                source + static_cast<std::size_t>(cst + row) * source_info.rowbytes +
                    static_cast<std::size_t>(csl) * sizeof(Pixel),
                width * sizeof(Pixel));

  const uint64_t opacity = static_cast<uint64_t>(source_opacity);
  const uint64_t denominator = static_cast<uint64_t>(Maximum) * 255;
  auto rounded = [](uint64_t numerator, uint64_t divisor) -> Channel {
    return static_cast<Channel>(std::min<uint64_t>(Maximum, (numerator + divisor / 2) / divisor));
  };
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t output_y = cdt + static_cast<int64_t>(row);
    if ((field == 1 && (output_y & 1)) || (field == 2 && !(output_y & 1))) continue;
    for (std::size_t column = 0; column < width; ++column) {
      const Pixel& input = snapshot[row * width + column];
      auto* output = reinterpret_cast<Pixel*>(destination +
          static_cast<std::size_t>(output_y) * destination_info.rowbytes +
          static_cast<std::size_t>(cdl + column) * sizeof(Pixel));
      const uint64_t destination_alpha = (*output)[0];
      for (std::size_t channel = 0; channel < 4; ++channel) {
        uint64_t numerator{};
        uint64_t divisor{};
        if (transfer_mode == 0) {
          numerator = static_cast<uint64_t>(input[channel]) * opacity +
              static_cast<uint64_t>((*output)[channel]) * (255 - opacity);
          divisor = 255;
        } else if (transfer_mode == 2) {
          const uint64_t destination_weight = denominator -
              static_cast<uint64_t>(input[0]) * opacity;
          numerator = static_cast<uint64_t>(input[channel]) * opacity * Maximum +
              static_cast<uint64_t>((*output)[channel]) * destination_weight;
          divisor = denominator;
        } else {
          const uint64_t source_weight = opacity * (Maximum - destination_alpha);
          numerator = static_cast<uint64_t>((*output)[channel]) * denominator +
              static_cast<uint64_t>(input[channel]) * source_weight;
          divisor = denominator;
        }
        (*output)[channel] = rounded(numerator, divisor);
      }
    }
  }
  return 0;
}
int32_t __cdecl aegp_get_new_effect_stream_by_index_v2(
    int32_t plugin_id, void* effect, int32_t index, void** stream) {
  std::size_t instance_index = 0;
  const auto* instance = resolve_effect_instance(effect, plugin_id, &instance_index);
  const auto* parameter = instance
      ? find_effect_parameter(instance->installed_key, index) : nullptr;
  if (plugin_id <= 0 || !instance || !stream || !parameter) return 4;
  const auto free_slot = std::find_if(g_aegp_legacy_effect_streams.begin(),
      g_aegp_legacy_effect_streams.end(), [](const auto& value) { return !value.live; });
  if (free_slot == g_aegp_legacy_effect_streams.end()) return 4;
  auto& value = *free_slot;
  const std::size_t slot = static_cast<std::size_t>(
      std::distance(g_aegp_legacy_effect_streams.begin(), free_slot));
  uint32_t generation = ++g_aegp_legacy_effect_stream_generation;
  if (generation == 0) generation = ++g_aegp_legacy_effect_stream_generation;
  const uintptr_t encoded = (static_cast<uintptr_t>(generation) << 8) |
      (static_cast<uintptr_t>(slot) << 2) | 3;
  if (encoded <= 3) return 4;
  value.param_index = index;
  value.live = true;
  value.hidden = false;
  value.value_live = false;
  value.effect_instance_index = static_cast<uint32_t>(instance_index);
  value.effect_instance_generation = instance->generation;
  value.generation = generation;
  value.owner_plugin_id = plugin_id;
  ++g_aegp_stream_acquires;
  *stream = reinterpret_cast<void*>(encoded);
  return 0;
}
AegpLegacyEffectStream* legacy_effect_stream(void* stream) {
  const uintptr_t encoded = reinterpret_cast<uintptr_t>(stream);
  if (!stream || (encoded & 3) != 3) return nullptr;
  const std::size_t slot = (encoded >> 2) & 0x3f;
  const uint32_t generation = static_cast<uint32_t>(encoded >> 8);
  if (slot >= g_aegp_legacy_effect_streams.size()) return nullptr;
  auto& value = g_aegp_legacy_effect_streams[slot];
  return value.live && value.generation == generation ? &value : nullptr;
}
bool legacy_effect_stream_parent_live(const AegpLegacyEffectStream& stream) {
  if (stream.effect_instance_index >= g_aegp_effect_instances.size()) return false;
  const auto& instance = g_aegp_effect_instances[stream.effect_instance_index];
  return instance.occupied && instance.generation == stream.effect_instance_generation;
}
int32_t __cdecl aegp_get_stream_name_v2(void* stream, uint8_t, char* name) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) || !name) return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance.installed_key, value->param_index);
  if (!parameter) return 4;
  std::strcpy(name, parameter->name);
  return 0;
}
int32_t __cdecl aegp_get_stream_type_v2(void* stream, int32_t* type) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) || !type) return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance.installed_key, value->param_index);
  if (!parameter) return 4;
  *type = parameter->type;
  return 0;
}
int32_t __cdecl aegp_get_new_stream_value_v2(
    int32_t plugin_id, void* stream, int32_t, const AegpTime* time,
    uint8_t, AegpStreamValue* output) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      plugin_id != value->owner_plugin_id || value->value_live || !time ||
      time->scale == 0 || !output)
    return 4;
  const auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(instance.installed_key, value->param_index);
  if (!parameter) return 4;
  output->stream = stream;
  output->value.fill(std::byte{});
  if (value->param_index == 0) {
    std::memcpy(output->value.data(), &instance.layer, sizeof(instance.layer));
  } else {
    std::memcpy(output->value.data(),
                instance.parameter_values[static_cast<std::size_t>(value->param_index - 1)].data(),
                sizeof(instance.parameter_values[0]));
  }
  value->value_live = true;
  value->checked_out_value = output;
  ++g_aegp_stream_value_acquires;
  return 0;
}
int32_t __cdecl aegp_dispose_stream_value_v2(AegpStreamValue* output) {
  if (!output) return 4;
  auto* stream = legacy_effect_stream(output->stream);
  if (!stream || !stream->value_live || stream->checked_out_value != output) return 4;
  output->stream = nullptr;
  stream->value_live = false;
  stream->checked_out_value = nullptr;
  ++g_aegp_stream_value_disposes;
  return 0;
}
int32_t __cdecl aegp_set_stream_value_v2(
    int32_t plugin_id, void* stream, AegpStreamValue* input) {
  auto* value = legacy_effect_stream(stream);
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      plugin_id != value->owner_plugin_id || !value->value_live || !input ||
      input->stream != stream || value->checked_out_value != input)
    return 4;
  const auto& current = g_aegp_effect_instances[value->effect_instance_index];
  const auto* parameter = find_effect_parameter(current.installed_key, value->param_index);
  if (!parameter || value->param_index == 0 || !parameter->writable) return 4;
  std::array<double, 4> candidate{};
  std::memcpy(candidate.data(), input->value.data(), sizeof(candidate));
  for (std::size_t index = 0; index < candidate.size(); ++index)
    if (!std::isfinite(candidate[static_cast<std::size_t>(index)])) return 4;
  auto& instance = g_aegp_effect_instances[value->effect_instance_index];
  instance.parameter_values[static_cast<std::size_t>(value->param_index - 1)] = candidate;
  bump_render_project_timestamp();
  return 0;
}
int32_t __cdecl aegp_dispose_stream_v2(void* stream) {
  auto* value = legacy_effect_stream(stream);
  if (!value || value->value_live) return 4;
  value->live = false;
  value->param_index = -1;
  value->effect_instance_index = 0;
  value->effect_instance_generation = 0;
  value->checked_out_value = nullptr;
  value->owner_plugin_id = 0;
  ++g_aegp_stream_disposes;
  return 0;
}
int32_t __cdecl aegp_set_dynamic_stream_flag_v2(
    void* stream, uint32_t one_flag, uint8_t undoable, uint8_t set) {
  auto* value = legacy_effect_stream(stream);
  constexpr uint32_t kHidden = 1u << 1;
  if (!value || !legacy_effect_stream_parent_live(*value) ||
      one_flag != kHidden || undoable > 1 || set > 1) return 4;
  value->hidden = set != 0;
  return 0;
}
int32_t __cdecl aegp_get_effect_param_union_by_index_v3(
    int32_t plugin_id, void* effect, int32_t index, int32_t* type, void* param_union) {
  if (plugin_id <= 0 || !resolve_effect_instance(effect, plugin_id) || !type ||
      !param_union || index < 0 || index >= 5) return 4;
  // AEGP effect inspection is independent of PF selector-local parameter
  // buffers. These are definition unions for the bounded synthetic scene,
  // never current values (which belong to the Stream Suite).
  static constexpr std::array<int32_t, 5> kTypes{0, 1, 4, 5, 10};
  static constexpr std::array<std::array<std::byte, kParamSize - 56>, 5> kUnions{};
  *type = kTypes[static_cast<std::size_t>(index)];
  std::memcpy(param_union, kUnions[static_cast<std::size_t>(index)].data(),
              kUnions[static_cast<std::size_t>(index)].size());
  ++g_aegp_effect_param_union_calls;
  return 0;
}

int32_t composite_rect_float(void* effect_ref, LegacyRect* source_rect,
                             int32_t source_opacity, void* source_world,
                             int32_t destination_x, int32_t destination_y,
                             int32_t field, int32_t transfer_mode,
                             void* destination_world) {
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      field < 0 || field > 2 || transfer_mode < 0 || transfer_mode > 2)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != kPixelFormatArgb128 ||
      destination_info.pixel_format != kPixelFormatArgb128 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * 16 ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * 16)
    return kPfErrBadCallbackParam;
  const int64_t sl=(std::max<int64_t>)(source_rect->left,0), st=(std::max<int64_t>)(source_rect->top,0),
      sr=(std::min<int64_t>)(source_rect->right,source_info.width), sb=(std::min<int64_t>)(source_rect->bottom,source_info.height);
  const int64_t dl=destination_x+sl-source_rect->left, dt=destination_y+st-source_rect->top;
  const int64_t cdl=(std::max<int64_t>)(dl,0), cdt=(std::max<int64_t>)(dt,0),
      cdr=(std::min<int64_t>)(dl+sr-sl,destination_info.width), cdb=(std::min<int64_t>)(dt+sb-st,destination_info.height);
  if(sr<=sl||sb<=st||cdr<=cdl||cdb<=cdt||source_opacity==0) return 0;
  const int64_t csl=sl+cdl-dl,cst=st+cdt-dt;
  const std::size_t width=static_cast<std::size_t>(cdr-cdl),height=static_cast<std::size_t>(cdb-cdt);
  if(width>SIZE_MAX/height||width*height>16'777'216) return kPfErrBadCallbackParam;
  using Pixel=std::array<float,4>; std::vector<Pixel> snapshot;
  try{snapshot.resize(width*height);}catch(...){return kPfErrBadCallbackParam;}
  const auto* source=static_cast<const unsigned char*>(source_info.data); auto* destination=static_cast<unsigned char*>(destination_info.data);
  for(std::size_t row=0;row<height;++row) std::memcpy(snapshot.data()+row*width,
      source+static_cast<std::size_t>(cst+row)*source_info.rowbytes+static_cast<std::size_t>(csl)*sizeof(Pixel),width*sizeof(Pixel));
  const double opacity=source_opacity/255.0;
  for(std::size_t row=0;row<height;++row){const int64_t output_y=cdt+static_cast<int64_t>(row);
    if((field==1&&(output_y&1))||(field==2&&!(output_y&1)))continue;
    for(std::size_t column=0;column<width;++column){const Pixel& input=snapshot[row*width+column];
      auto* output=reinterpret_cast<Pixel*>(destination+static_cast<std::size_t>(output_y)*destination_info.rowbytes+static_cast<std::size_t>(cdl+column)*sizeof(Pixel));
      const double destination_alpha=(*output)[0];
      for(int channel=0;channel<4;++channel){
        if(transfer_mode==0)(*output)[channel]=static_cast<float>(input[channel]*opacity+(*output)[channel]*(1.0-opacity));
        else if(transfer_mode==2)(*output)[channel]=static_cast<float>(input[channel]*opacity+(*output)[channel]*(1.0-input[0]*opacity));
        else (*output)[channel]=static_cast<float>((*output)[channel]+input[channel]*opacity*(1.0-destination_alpha));
      }
    }
  }
  return 0;
}

int32_t __cdecl composite_rect8(void* effect_ref, LegacyRect* source_rect,
                                int32_t source_opacity, void* source_world,
                                int32_t destination_x, int32_t destination_y,
                                int32_t field, int32_t transfer_mode,
                                void* destination_world) {
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format)
    return kPfErrBadCallbackParam;
  if (source_info.pixel_format == kPixelFormatArgb32)
    return composite_rect_registered<uint8_t, 255>(effect_ref, source_rect, source_opacity,
        source_world, destination_x, destination_y, field, transfer_mode, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb64)
    return composite_rect_registered<uint16_t, 32768>(effect_ref, source_rect, source_opacity,
        source_world, destination_x, destination_y, field, transfer_mode, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb128)
    return composite_rect_float(effect_ref, source_rect, source_opacity, source_world,
        destination_x, destination_y, field, transfer_mode, destination_world);
  return kPfErrBadCallbackParam;
}

bool verify_world_transform_composite_rect() {
  DispatchWorldFormatScope dispatch_worlds;
  auto make_world = [&](std::array<std::byte, 64>& world, unsigned char* pixels,
                       int32_t rowbytes, int32_t width, int32_t height) {
    world.fill(std::byte{});
    std::memcpy(world.data() + 24, &pixels, sizeof(pixels));
    std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + 36, &width, sizeof(width));
    std::memcpy(world.data() + 40, &height, sizeof(height));
    dispatch_worlds.register_world(world.data(), kPixelFormatArgb32);
  };
  auto run_pixel_case = [&](int32_t mode, const std::array<uint8_t, 4>& expected) {
    std::array<unsigned char, 4> source{128, 64, 32, 16};
    std::array<unsigned char, 4> destination{64, 20, 10, 5};
    std::array<std::byte, 64> source_world{}, destination_world{};
    make_world(source_world, source.data(), 4, 1, 1);
    make_world(destination_world, destination.data(), 4, 1, 1);
    LegacyRect rect{0, 0, 1, 1};
    return composite_rect8(&source_world, &rect, 128, &source_world, 0, 0, 0, mode,
                           &destination_world) == 0 && destination == expected;
  };
  if (!run_pixel_case(0, {96, 42, 21, 11}) ||
      !run_pixel_case(1, {112, 44, 22, 11}) ||
      !run_pixel_case(2, {112, 47, 24, 12})) {
    std::cerr << "composite diagnostic: pixel matrix\n";
    return false;
  }

  constexpr int32_t rowbytes = 16;
  std::array<unsigned char, rowbytes * 3 + 16> source_guarded{};
  std::array<unsigned char, rowbytes * 3 + 16> destination_guarded{};
  source_guarded.fill(0xA5);
  destination_guarded.fill(0xCC);
  auto* source = source_guarded.data() + 8;
  auto* destination = destination_guarded.data() + 8;
  for (int y = 0; y < 3; ++y) {
    for (int x = 0; x < 3; ++x) {
      const std::array<unsigned char, 4> pixel{
          255, static_cast<unsigned char>(10 + y * 3 + x), 0, 0};
      std::memcpy(source + y * rowbytes + x * 4, pixel.data(), 4);
    }
  }
  std::array<std::byte, 64> source_world{}, destination_world{};
  make_world(source_world, source, rowbytes, 3, 3);
  make_world(destination_world, destination, rowbytes, 3, 3);
  LegacyRect rect{0, 0, 3, 3};
  if (composite_rect8(&source_world, &rect, 255, &source_world, -1, 0, 1, 0,
                      &destination_world) != 0) {
    std::cerr << "composite diagnostic: clipped upper field call\n";
    return false;
  }
  // Clipping drops source column zero; upper field updates destination rows 0 and 2 only.
  if (destination[1] != 11 || destination[5] != 12 || destination[rowbytes] != 0xCC ||
      destination[2 * rowbytes + 1] != 17 || destination[2 * rowbytes + 5] != 18) {
    std::cerr << "composite diagnostic: clipped upper field values\n";
    return false;
  }
  for (int y = 0; y < 3; ++y) {
    for (int x = 12; x < rowbytes; ++x) {
      if (destination[y * rowbytes + x] != 0xCC) return false;
    }
  }
  std::memset(destination, 0xCC, rowbytes * 3);
  if (composite_rect8(&source_world, &rect, 255, &source_world, 0, 0, 2, 0,
                      &destination_world) != 0 || destination[0] != 0xCC ||
      destination[rowbytes] != 255 || destination[rowbytes + 1] != 13 ||
      destination[2 * rowbytes] != 0xCC) {
    std::cerr << "composite diagnostic: lower field\n";
    return false;
  }
  if (!std::all_of(source_guarded.begin(), source_guarded.begin() + 8,
                   [](unsigned char value) { return value == 0xA5; }) ||
      !std::all_of(source_guarded.end() - 8, source_guarded.end(),
                   [](unsigned char value) { return value == 0xA5; }) ||
      !std::all_of(destination_guarded.begin(), destination_guarded.begin() + 8,
                   [](unsigned char value) { return value == 0xCC; }) ||
      !std::all_of(destination_guarded.end() - 8, destination_guarded.end(),
                   [](unsigned char value) { return value == 0xCC; })) {
    std::cerr << "composite diagnostic: guards\n";
    return false;
  }

  std::array<unsigned char, 12> alias_pixels{255, 1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0};
  std::array<std::byte, 64> alias_world{};
  make_world(alias_world, alias_pixels.data(), 12, 3, 1);
  LegacyRect alias_rect{0, 0, 2, 1};
  if (composite_rect8(&alias_world, &alias_rect, 255, &alias_world, 1, 0, 0, 0,
                      &alias_world) != 0 || alias_pixels[5] != 1 || alias_pixels[9] != 2) {
    std::cerr << "composite diagnostic: alias\n";
    return false;
  }
  if (composite_rect8(nullptr, &alias_rect, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, nullptr, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, &alias_rect, 256, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, &alias_rect, 255, &alias_world, 0, 0, 3, 0,
                      &alias_world) != kPfErrBadCallbackParam) {
    std::cerr << "composite diagnostic: invalid arguments\n";
    return false;
  }

  // A padded ARGB16 row can resemble 32F by width; provenance must win over layout.
  std::array<uint16_t, 12> source16{32768, 0, 32768, 1, 32768, 32768, 0, 32767};
  std::array<uint16_t, 12> destination16{};
  std::array<std::byte, 64> registered16{}, shallow16{}, output16{};
  auto make16 = [](auto& world, void* data) {
    world.fill(std::byte{});
    const int32_t flags = 1, rowbytes = 24, width = 2, height = 1;
    std::memcpy(world.data() + 16, &flags, sizeof(flags));
    std::memcpy(world.data() + 24, &data, sizeof(data));
    std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + 36, &width, sizeof(width));
    std::memcpy(world.data() + 40, &height, sizeof(height));
  };
  make16(registered16, source16.data());
  shallow16 = registered16;
  make16(output16, destination16.data());
  if (!dispatch_worlds.register_world(registered16.data(), kPixelFormatArgb64) ||
      !dispatch_worlds.register_world(output16.data(), kPixelFormatArgb64)) {
    std::cerr << "composite diagnostic: register 16\n";
    return false;
  }
  LegacyRect rect16{0, 0, 2, 1};
  if (composite_rect8(registered16.data(), &rect16, 255, shallow16.data(), 0, 0, 0, 0,
      output16.data()) != 0 ||
      !std::equal(source16.begin(), source16.begin() + 8, destination16.begin())) {
    std::cerr << "composite diagnostic: copy 16\n";
    return false;
  }

  std::atomic_bool thread16{false}, thread32{false};
  std::thread deep_thread([&] {
    DispatchWorldFormatScope scope;
    scope.register_world(registered16.data(), kPixelFormatArgb64);
    scope.register_world(output16.data(), kPixelFormatArgb64);
    thread16 = composite_rect8(registered16.data(), &rect16, 255, registered16.data(),
                               0, 0, 0, 0, output16.data()) == 0;
  });
  std::thread float_thread([&] {
    DispatchWorldFormatScope scope;
    scope.register_world(registered16.data(), kPixelFormatArgb128);
    scope.register_world(output16.data(), kPixelFormatArgb128);
    thread32 = composite_rect8(registered16.data(), &rect16, 255, registered16.data(),
                               0, 0, 0, 0, output16.data()) == kPfErrBadCallbackParam;
  });
  deep_thread.join();
  float_thread.join();
  if (!thread16 || !thread32) {
    std::cerr << "composite diagnostic: concurrency " << thread16 << ',' << thread32 << '\n';
    return false;
  }
  std::array<float, 8> source32{{0.5f,2.0f,-0.5f,4.0f, 1.0f,8.0f,0.25f,-2.0f}};
  std::array<float, 8> destination32{};
  LocalEffectWorld source_world32{}, destination_world32{};
  source_world32.data=source32.data(); source_world32.rowbytes=32;
  source_world32.width=2; source_world32.height=1;
  destination_world32.data=destination32.data(); destination_world32.rowbytes=32;
  destination_world32.width=2; destination_world32.height=1;
  const bool source32_registered =
      dispatch_worlds.register_world(&source_world32, kPixelFormatArgb128);
  const bool destination32_registered =
      dispatch_worlds.register_world(&destination_world32, kPixelFormatArgb128);
  const int32_t composite32_result = composite_rect8(
      &source_world32, &rect16, 255, &source_world32, 0, 0, 0, 0, &destination_world32);
  if (!source32_registered || !destination32_registered || composite32_result != 0 ||
      destination32 != source32) {
    std::cerr << "float composite diagnostic: source_registered=" << source32_registered
              << ", destination_registered=" << destination32_registered
              << ", result=" << composite32_result;
    for (float value : destination32) std::cerr << ',' << value;
    std::cerr << '\n';
    return false;
  }
  return true;
}

bool normalize_legacy_rect(const LegacyRect* requested, int32_t width, int32_t height,
                           LegacyRect& result) {
  result = requested ? *requested : LegacyRect{0, 0, width, height};
  return result.top >= 0 && result.left >= 0 && result.bottom >= result.top &&
      result.right >= result.left && result.bottom <= height && result.right <= width;
}

struct PfRasterPoint { double x, y; };

bool flatten_pf_path(const HostMask& path, int32_t quality,
                     std::vector<PfRasterPoint>& points) {
  const std::size_t count = distinct_vertex_count(path);
  if (count < 3 || count > kMaxOutlineVertices) return false;
  const int subdivisions = quality == 1 ? 16 : 8;
  points.clear();
  points.reserve(count * subdivisions);
  for (std::size_t segment = 0; segment < count; ++segment) {
    const auto& a = path.vertices[segment];
    const auto& b = path.vertices[(segment + 1) % count];
    const double c1x = a.x + a.tangent_out_x;
    const double c1y = a.y + a.tangent_out_y;
    const double c2x = b.x + b.tangent_in_x;
    const double c2y = b.y + b.tangent_in_y;
    for (int sample = 0; sample < subdivisions; ++sample) {
      const double t = static_cast<double>(sample) / subdivisions;
      const double u = 1.0 - t;
      points.push_back({u * u * u * a.x + 3.0 * u * u * t * c1x +
                            3.0 * u * t * t * c2x + t * t * t * b.x,
                        u * u * u * a.y + 3.0 * u * u * t * c1y +
                            3.0 * u * t * t * c2y + t * t * t * b.y});
    }
  }
  return points.size() >= 3 && points.size() <= kMaxOutlineVertices * 16;
}

bool point_inside_pf_path(const std::vector<PfRasterPoint>& points, double x, double y) {
  const std::size_t count = points.size();
  bool inside = false;
  for (std::size_t current = 0, previous = count - 1; current < count;
       previous = current++) {
    const auto& a = points[current];
    const auto& b = points[previous];
    if ((a.y > y) != (b.y > y)) {
      const double crossing = (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x;
      if (x < crossing) inside = !inside;
    }
  }
  return inside;
}

double pf_path_distance(const std::vector<PfRasterPoint>& points, double x, double y,
                        double feather_x, double feather_y) {
  const double scale_x = std::max(feather_x, 1.0e-6);
  const double scale_y = std::max(feather_y, 1.0e-6);
  double minimum = std::numeric_limits<double>::infinity();
  for (std::size_t current = 0, previous = points.size() - 1;
       current < points.size(); previous = current++) {
    const double ax = points[previous].x / scale_x;
    const double ay = points[previous].y / scale_y;
    const double bx = points[current].x / scale_x;
    const double by = points[current].y / scale_y;
    const double px = x / scale_x;
    const double py = y / scale_y;
    const double dx = bx - ax;
    const double dy = by - ay;
    const double length_squared = dx * dx + dy * dy;
    const double t = length_squared == 0.0 ? 0.0 :
        std::clamp(((px - ax) * dx + (py - ay) * dy) / length_squared, 0.0, 1.0);
    minimum = std::min(minimum, std::hypot(px - (ax + t * dx), py - (ay + t * dy)));
  }
  return minimum;
}

int32_t __cdecl pf_mask_world_with_path(void* effect_ref, void** path, double feather_x,
                                        double feather_y, int32_t invert, double opacity,
                                        int32_t quality, void* world, LegacyRect* bounds) {
  auto* mask = path ? static_cast<HostMask*>(*path) : nullptr;
  g_pf_path_last_feather_x = feather_x;
  g_pf_path_last_feather_y = feather_y;
  g_pf_path_last_opacity = opacity;
  g_pf_path_last_quality = quality;
  g_pf_path_reject_reason = !effect_ref || !world ? 1 :
      (!valid_checked_pf_path(mask) || mask->open ? 2 :
      (!std::isfinite(feather_x) || !std::isfinite(feather_y) ||
       feather_x < 0.0 || feather_y < 0.0 || feather_x > 32768.0 || feather_y > 32768.0 ? 3 :
      (!std::isfinite(opacity) || opacity < 0.0 || opacity > 1.0 ? 4 :
      ((invert != 0 && invert != 1) || (quality != 0 && quality != 1) ? 5 : 0))));
  if (g_pf_path_reject_reason != 0) {
    ++g_invalid_pf_path_operations;
    return 4;
  }
  int32_t flags{};
  std::memcpy(&flags, static_cast<std::byte*>(world) + 16, sizeof(flags));
  const int32_t pixel_bytes = (flags & 1) != 0 ? 8 : 4;
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!bounded_typed_world(world, pixel_bytes, pixels, rowbytes, width, height)) {
    g_pf_path_reject_reason = 6;
    ++g_invalid_pf_path_operations;
    return 4;
  }
  LegacyRect area{};
  if (bounds) std::memcpy(g_pf_path_last_bounds.data(), bounds, sizeof(LegacyRect));
  if (!normalize_legacy_rect(bounds, width, height, area)) {
    g_pf_path_reject_reason = 7;
    ++g_invalid_pf_path_operations;
    return 4;
  }
  std::vector<PfRasterPoint> flattened;
  if (!flatten_pf_path(*mask, quality, flattened)) {
    g_pf_path_reject_reason = 8;
    ++g_invalid_pf_path_operations;
    return 4;
  }
  for (int32_t y = area.top; y < area.bottom; ++y) {
    for (int32_t x = area.left; x < area.right; ++x) {
      const bool inside = point_inside_pf_path(flattened, x + 0.5, y + 0.5);
      double coverage = inside ? 1.0 : 0.0;
      if (feather_x > 0.0 || feather_y > 0.0) {
        const double distance = pf_path_distance(flattened, x + 0.5, y + 0.5,
                                                 feather_x, feather_y);
        coverage = std::clamp(0.5 + (inside ? distance : -distance), 0.0, 1.0);
      }
      if (invert != 0) coverage = 1.0 - coverage;
      auto* pixel = pixels + static_cast<std::size_t>(y) * rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      if (pixel_bytes == 4) {
        pixel[0] = static_cast<uint8_t>(std::lround(pixel[0] * opacity * coverage));
      } else {
        auto* pixel16 = reinterpret_cast<uint16_t*>(pixel);
        pixel16[0] = static_cast<uint16_t>(std::lround(pixel16[0] * opacity * coverage));
      }
    }
  }
  ++g_pf_path_mask_calls;
  g_pf_path_reject_reason = 0;
  return 0;
}

template <typename Operation>
double finite_ansi_unary(double value, Operation operation) noexcept {
  if (!std::isfinite(value)) return 0.0;
  const double result = operation(value);
  return std::isfinite(result) ? result : 0.0;
}

template <typename Operation>
double finite_ansi_binary(double left, double right, Operation operation) noexcept {
  if (!std::isfinite(left) || !std::isfinite(right)) return 0.0;
  const double result = operation(left, right);
  return std::isfinite(result) ? result : 0.0;
}

double __cdecl ansi_atan(double value) {
  return finite_ansi_unary(value, [](double x) { return std::atan(x); });
}

double __cdecl ansi_atan2(double y, double x) {
  return finite_ansi_binary(y, x, [](double a, double b) { return std::atan2(a, b); });
}

double __cdecl ansi_ceil(double value) {
  return finite_ansi_unary(value, [](double x) { return std::ceil(x); });
}

double __cdecl ansi_cos(double value) {
  return finite_ansi_unary(value, [](double x) { return std::cos(x); });
}

double __cdecl ansi_exp(double value) {
  return finite_ansi_unary(value, [](double x) { return std::exp(x); });
}

double __cdecl ansi_fabs(double value) {
  return finite_ansi_unary(value, [](double x) { return std::fabs(x); });
}

double __cdecl ansi_floor(double value) {
  return finite_ansi_unary(value, [](double x) { return std::floor(x); });
}

double __cdecl ansi_fmod(double value, double divisor) {
  if (divisor == 0.0) return 0.0;
  return finite_ansi_binary(value, divisor, [](double x, double y) { return std::fmod(x, y); });
}

double __cdecl ansi_hypot(double x, double y) {
  return finite_ansi_binary(x, y, [](double a, double b) { return std::hypot(a, b); });
}

double __cdecl ansi_log(double value) {
  if (!(value > 0.0)) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::log(x); });
}

double __cdecl ansi_log10(double value) {
  if (!(value > 0.0)) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::log10(x); });
}

double __cdecl ansi_pow(double base, double exponent) {
  return finite_ansi_binary(base, exponent, [](double x, double y) { return std::pow(x, y); });
}

double __cdecl ansi_sin(double value) {
  return finite_ansi_unary(value, [](double x) { return std::sin(x); });
}

double __cdecl ansi_sqrt(double value) {
  if (value < 0.0) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::sqrt(x); });
}

double __cdecl ansi_tan(double value) {
  return finite_ansi_unary(value, [](double x) { return std::tan(x); });
}

int __cdecl ansi_sprintf(char* destination, const char* format, ...) {
  if (!destination || !format || strnlen_s(format, 256) == 256) return -1;
  va_list arguments;
  va_start(arguments, format);
  va_list measure;
  va_copy(measure, arguments);
  const int required = _vscprintf(format, measure);
  va_end(measure);
  const int written = required >= 0 && required <= 4096
      ? vsprintf_s(destination, static_cast<std::size_t>(required) + 1, format, arguments)
      : -1;
  va_end(arguments);
  return written;
}

char* __cdecl ansi_strcpy(char* destination, const char* source) {
  if (!destination || !source) return nullptr;
  const std::size_t length = strnlen_s(source, 4096);
  if (length == 4096) return nullptr;
  std::memmove(destination, source, length + 1);
  return destination;
}

double __cdecl ansi_asin(double value) {
  if (value < -1.0 || value > 1.0) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::asin(x); });
}

double __cdecl ansi_acos(double value) {
  if (value < -1.0 || value > 1.0) return 0.0;
  return finite_ansi_unary(value, [](double x) { return std::acos(x); });
}

int32_t __cdecl get_platform_data(void* effect_ref, int32_t which, void* data) {
  constexpr int32_t kExeFilePathWide = 7;
  constexpr int32_t kResourceFilePathWide = 8;
  constexpr std::size_t kMaxPath = 260;
  if (!effect_ref || !data ||
      (which != kExeFilePathWide && which != kResourceFilePathWide) ||
      g_plugin_file_path.empty() || g_plugin_file_path.size() >= kMaxPath ||
      !std::filesystem::path(g_plugin_file_path).is_absolute()) return 4;
  auto* destination = static_cast<wchar_t*>(data);
  std::wmemcpy(destination, g_plugin_file_path.c_str(), g_plugin_file_path.size() + 1);
  return 0;
}

int32_t __cdecl checkin_param(void*, void* definition) {
  if (auto* context = aexcompat::worker_runtime::classic::active_context())
    return context->checkin(definition);
  if (aexcompat::worker_runtime::classic::dispatch_active()) return 4;
  std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
  if (!definition || g_live_param_checkouts.empty()) {
    ++g_invalid_param_checkins;
    return 4;
  }
  auto found = g_live_param_checkouts.find(definition);
  // PF_ParamDef is a value type. Wrappers may move the checked-out value before
  // checkin, so its address is not a stable checkout identity.
  if (found == g_live_param_checkouts.end()) found = g_live_param_checkouts.begin();
  if (--found->second == 0) g_live_param_checkouts.erase(found);
  ++g_param_checkin_calls;
  return 0;
}

bool param_checkouts_balanced() {
  if (auto* context = aexcompat::worker_runtime::classic::active_context())
    return context->checkouts_balanced();
  std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
  return g_live_param_checkouts.empty() && g_param_checkout_calls == g_param_checkin_calls &&
      g_invalid_param_checkins == 0;
}

void automatic_checkin_pre_render_params() {
  if (auto* context = aexcompat::worker_runtime::classic::active_context()) {
    context->automatic_checkin();
    return;
  }
  std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
  uint32_t checkout_count = 0;
  for (const auto& checkout : g_live_param_checkouts) checkout_count += checkout.second;
  g_automatic_param_checkins += checkout_count;
  g_param_checkin_calls += checkout_count;
  g_live_param_checkouts.clear();
}

int32_t __cdecl set_options_button_name(void* effect_ref, const char* name) {
  if (!effect_ref || !name) return 4;
  const std::size_t length = strnlen_s(name, 256);
  if (length == 256) return 4;
  g_options_button_name.assign(name, length);
  ++g_options_button_name_calls;
  return 0;
}

bool sha256(const std::filesystem::path& path, std::string& result);
constexpr std::size_t kMaxAuxChannels = 16, kMaxAuxChannelsPerParam = 8;
constexpr std::size_t kMaxAuxSamplesPerChannel = 64, kMaxAuxSamples = 256;
constexpr uint64_t kMaxAuxSampleBytes = 256ULL * 1024 * 1024;
constexpr uint64_t kMaxAuxTotalBytes = 512ULL * 1024 * 1024;

struct ExternalAuxSample {
  int32_t time{};
  uint32_t time_scale{};
  std::string sampling;
  std::shared_ptr<const std::vector<float>> values;
};
struct ExternalAuxChannel {
  int32_t param_index{}, channel_type{}, dimension{}, width{}, height{};
  int32_t native_data_type{0x464c5434};
  int32_t signed_row_bytes{};
  int32_t origin_x{}, origin_y{};
  int32_t downsample_x_num{1}, downsample_x_den{1};
  int32_t downsample_y_num{1}, downsample_y_den{1};
  std::string coordinate_space{"source_pixel"};
  std::string units{"unitless"};
  std::string name;
  std::vector<ExternalAuxSample> samples;
  uint64_t identity{};
};
struct ExternalAuxSet { std::vector<ExternalAuxChannel> channels; };
ExternalAuxSet g_external_aux_set;
thread_local ExternalAuxSet g_native_aux_set;
// Parsed once before native dispatch, then immutable for every MFR render thread.
std::vector<int32_t> g_alpha_as_coverage_params;
bool g_external_aux_loaded = false;
thread_local bool g_external_aux_active = false;

bool load_aux_manifest(const std::filesystem::path &manifest_path,
                       ExternalAuxSet &result) {
  std::error_code ec;
  const auto absolute = std::filesystem::absolute(manifest_path, ec);
  const auto canonical = std::filesystem::canonical(manifest_path, ec);
  if (ec || !manifest_path.is_absolute() ||
      absolute.lexically_normal() != canonical)
    return false;
  const auto manifest_size = std::filesystem::file_size(canonical, ec);
  if (ec || manifest_size == 0 || manifest_size > 1024 * 1024)
    return false;
  std::ifstream input(canonical, std::ios::binary);
  if (!input)
    return false;
  std::string text((std::istreambuf_iterator<char>(input)), {});
  JsonValue root;
  if (input.bad() || !StrictJsonParser(std::move(text)).parse(root) ||
      !std::holds_alternative<JsonValue::Object>(root.value))
    return false;
  const auto &object = std::get<JsonValue::Object>(root.value);
  std::string schema, nonce;
  if (!json_exact_keys(object, {"schema", "nonce", "channels"}) ||
      !json_string(object, "schema", schema) || schema != "aux-manifest-v1" ||
      !json_string(object, "nonce", nonce) || nonce.empty() ||
      !std::all_of(nonce.begin(), nonce.end(),
                   [](unsigned char c) { return std::isdigit(c) != 0; }))
    return false;
  const auto *channels_value = json_member(object, "channels");
  if (!channels_value ||
      !std::holds_alternative<JsonValue::Array>(channels_value->value))
    return false;
  const auto &channels = std::get<JsonValue::Array>(channels_value->value);
  if (channels.empty() || channels.size() > kMaxAuxChannels)
    return false;
  ExternalAuxSet parsed;
  std::map<int32_t, std::size_t> per_param;
  std::set<std::pair<int32_t, int32_t>> keys;
  std::set<std::filesystem::path> paths;
  std::size_t total_samples = 0;
  uint64_t total_bytes = 0, identity = 1;
  for (const auto &cv : channels) {
    if (!std::holds_alternative<JsonValue::Object>(cv.value))
      return false;
    const auto &co = std::get<JsonValue::Object>(cv.value);
    const bool rich_descriptor = json_exact_keys(co, {"param_index", "type", "name", "data_type",
        "dimension", "width", "height", "row_bytes", "origin_x", "origin_y",
        "downsample_x_num", "downsample_x_den", "downsample_y_num", "downsample_y_den",
        "coordinate_space", "units", "samples"});
    if (!rich_descriptor && !json_exact_keys(co, {"param_index", "type", "name", "data_type",
                                                   "dimension", "width", "height", "samples"}))
      return false;
    ExternalAuxChannel channel;
    int32_t param{}, dim{}, width{}, height{};
    std::string type;
    if (!json_i32(co, "param_index", param) || param < 0 ||
        !json_i32(co, "type", channel.channel_type) ||
        !json_string(co, "name", channel.name) || channel.name.size() > 63 ||
        channel.name.find('\0') != std::string::npos ||
        !json_string(co, "data_type", type) || type != "f32le" ||
        !json_i32(co, "dimension", dim) || dim < 1 || dim > 4 ||
        !json_i32(co, "width", width) || width < 1 || width > 4096 ||
        !json_i32(co, "height", height) || height < 1 || height > 4096)
      return false;
    channel.param_index = param;
    channel.dimension = dim;
    channel.width = width;
    channel.height = height;
    channel.signed_row_bytes = width * dim * 4;
    if (rich_descriptor) {
      if (!json_i32(co,"row_bytes",channel.signed_row_bytes) ||
          !json_i32(co,"origin_x",channel.origin_x) || !json_i32(co,"origin_y",channel.origin_y) ||
          !json_i32(co,"downsample_x_num",channel.downsample_x_num) ||
          !json_i32(co,"downsample_x_den",channel.downsample_x_den) ||
          !json_i32(co,"downsample_y_num",channel.downsample_y_num) ||
          !json_i32(co,"downsample_y_den",channel.downsample_y_den) ||
          !json_string(co,"coordinate_space",channel.coordinate_space) ||
          !json_string(co,"units",channel.units) || channel.signed_row_bytes == INT32_MIN ||
          std::abs(channel.signed_row_bytes) < width * dim * 4 ||
          channel.downsample_x_num <= 0 || channel.downsample_x_den <= 0 ||
          channel.downsample_y_num <= 0 || channel.downsample_y_den <= 0 ||
          channel.coordinate_space.empty() || channel.coordinate_space.size() > 64 ||
          channel.units.empty() || channel.units.size() > 64) return false;
    }
    channel.identity = identity++;
    if (++per_param[param] > kMaxAuxChannelsPerParam ||
        !keys.emplace(param, channel.channel_type).second)
      return false;
    const uint64_t bytes = static_cast<uint64_t>(std::abs(channel.signed_row_bytes)) * height;
    if (bytes > kMaxAuxSampleBytes)
      return false;
    const auto* sv=json_member(co,"samples"); if(!sv||!std::holds_alternative<JsonValue::Array>(sv->value))return false; const auto& samples=std::get<JsonValue::Array>(sv->value); if(samples.empty()||samples.size()>kMaxAuxSamplesPerChannel)return false; std::set<std::pair<int32_t,uint32_t>> times;
    for(const auto& sample_value:samples){ if(++total_samples>kMaxAuxSamples||total_bytes>kMaxAuxTotalBytes-bytes)return false; total_bytes+=bytes; if(!std::holds_alternative<JsonValue::Object>(sample_value.value))return false; const auto& so=std::get<JsonValue::Object>(sample_value.value); if(!json_exact_keys(so,{"time","time_scale","path","sampling","interpretation","expected_byte_length","sha256"}))return false;
      ExternalAuxSample sample; int32_t scale{}; uint64_t declared_size{}; std::string path_text,sampling,interpretation,declared_hash;
      if(!json_i32(so,"time",sample.time)||!json_i32(so,"time_scale",scale)||scale<=0||!times.emplace(sample.time,static_cast<uint32_t>(scale)).second||!json_string(so,"path",path_text)||!json_string(so,"sampling",sampling)||(sampling!="exact"&&sampling!="hold")||!json_string(so,"interpretation",interpretation)||(interpretation!="depth"&&interpretation!="normals"&&interpretation!="motion_vectors"&&interpretation!="generic")||!json_u64(so,"expected_byte_length",declared_size)||declared_size!=bytes||!json_string(so,"sha256",declared_hash)||declared_hash.size()!=64||!std::all_of(declared_hash.begin(),declared_hash.end(),[](unsigned char c){return std::isxdigit(c)!=0;}))return false;
      if ((interpretation=="depth" && dim!=1) || (interpretation=="normals" && dim!=3) ||
          (interpretation=="motion_vectors" && dim!=2) ||
          (channel.channel_type==0x44505448 && (interpretation!="depth" || dim!=1))) return false;
      sample.sampling = sampling;
      sample.time_scale=static_cast<uint32_t>(scale); const std::filesystem::path path=std::filesystem::u8path(path_text); const auto abs=std::filesystem::absolute(path,ec); const auto canon=std::filesystem::canonical(path,ec); if(ec||!path.is_absolute()||abs.lexically_normal()!=canon||canon.parent_path()!=canonical.parent_path()||!paths.insert(canon).second||std::filesystem::file_size(canon,ec)!=bytes)return false; std::string actual_hash; if(ec||!sha256(canon,actual_hash)||_stricmp(actual_hash.c_str(),declared_hash.c_str())!=0)return false;
      std::ifstream raw(canon,std::ios::binary); std::vector<std::byte> raw_bytes(static_cast<std::size_t>(bytes)); if(!raw.read(reinterpret_cast<char*>(raw_bytes.data()),static_cast<std::streamsize>(bytes))||raw.peek()!=EOF)return false; auto values=std::make_shared<std::vector<float>>(static_cast<std::size_t>(width)*height*dim); const std::size_t stride=static_cast<std::size_t>(std::abs(channel.signed_row_bytes)), packed=static_cast<std::size_t>(width)*dim*sizeof(float); for(int32_t y=0;y<height;++y){const int32_t physical=channel.signed_row_bytes<0?height-1-y:y;std::memcpy(values->data()+static_cast<std::size_t>(y)*width*dim,raw_bytes.data()+static_cast<std::size_t>(physical)*stride,packed);} if(std::any_of(values->begin(),values->end(),[](float v){return !std::isfinite(v);}))return false; sample.values=std::move(values); channel.samples.push_back(std::move(sample)); }
    parsed.channels.push_back(std::move(channel));
  }
  result=std::move(parsed); return true;
}
constexpr uint64_t kChannelRefMagic = 0x504641454348414eULL;  // "PFAECHAN"
constexpr int32_t kChannelDepth = 0x44505448;                // 'DPTH'
constexpr int32_t kDataFloat = 0x464c5434;
constexpr int32_t kDataDouble = 0x44424c38;
constexpr int32_t kDataLong = 0x4c4f4e34;
constexpr int32_t kDataShort = 0x53485432;
constexpr int32_t kDataFixed = 0x46495834;
constexpr int32_t kDataChar = 0x43485231;
constexpr int32_t kDataUByte = 0x55425431;
constexpr int32_t kDataUShort = 0x55535432;
constexpr int32_t kDataUFixed = 0x55465834;

struct PfChannelRef {
  uint64_t magic;
  uint64_t generation;
  int32_t param_index;
  int32_t channel_index;
  int32_t channel_type;
  int32_t reserved;
  std::array<uint64_t, 4> seal;
};
struct PfChannelDesc {
  int32_t channel_type;
  char name[64];
  int32_t data_type;
  int32_t dimension;
};
struct PfChannelChunk {
  PfChannelRef channel_ref;
  int32_t width;
  int32_t height;
  int32_t dimension;
  int32_t row_bytes;
  int32_t data_type;
  void* data_handle;
  void* data;
};
static_assert(sizeof(PfChannelRef) == 64);
static_assert(sizeof(PfChannelDesc) == 76);
static_assert(offsetof(PfChannelDesc, data_type) == 68);
static_assert(sizeof(PfChannelChunk) == 104);
static_assert(offsetof(PfChannelChunk, width) == 64);
static_assert(offsetof(PfChannelChunk, data_handle) == 88);
static_assert(offsetof(PfChannelChunk, data) == 96);

struct LiveChannelChunk {
  PfChannelRef ref{};
  int32_t data_type{};
  int32_t row_bytes{};
  int32_t width{};
  int32_t height{};
  int32_t dimension{};
  std::size_t bytes{};
  void** handle{};
  void* locked_data{};
  std::shared_ptr<const std::vector<float>> sample_receipt;
  int32_t sample_time{};
  uint32_t sample_time_scale{};
  int32_t duration{};
  std::thread::id owner_thread{};
};
std::mutex g_channel_mutex;
std::unordered_map<PfChannelChunk*, LiveChannelChunk> g_live_channel_chunks;
thread_local uint64_t g_channel_generation = 1;
thread_local bool g_channel_fixture_enabled = false;
constexpr int32_t kChannelFixtureWidth = 3;
constexpr int32_t kChannelFixtureHeight = 2;
constexpr std::array<float, 6> kChannelFixture{0.0f, 0.25f, 1.0f, -1.0f, 2.0f, 0.5f};
int32_t g_channel_transport_row_bytes{}, g_channel_transport_origin_x{},
    g_channel_transport_origin_y{}, g_channel_transport_duration{};

template <typename Callback>
void for_each_active_aux_channel(Callback&& callback) {
  if (g_external_aux_active)
    for (const auto& channel : g_external_aux_set.channels) callback(channel);
  for (const auto& channel : g_native_aux_set.channels) callback(channel);
}

void publish_alpha_coverage_provider(const std::vector<unsigned char>& argb,
                                     int32_t width, int32_t height,
                                     int32_t pixel_bytes, int32_t time,
                                     uint32_t time_scale) {
  g_native_aux_set = {};
  if (g_alpha_as_coverage_params.empty() || width <= 0 || height <= 0 ||
      (pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16) ||
      argb.size() != static_cast<std::size_t>(width) * height * pixel_bytes)
    return;
  auto values = std::make_shared<std::vector<float>>(
      static_cast<std::size_t>(width) * height);
  for (std::size_t pixel = 0; pixel < values->size(); ++pixel) {
    const auto* source = argb.data() + pixel * pixel_bytes;
    if (pixel_bytes == 4) {
      (*values)[pixel] = static_cast<float>(source[0]) / 255.0f;
    } else if (pixel_bytes == 8) {
      uint16_t alpha{};
      std::memcpy(&alpha, source, sizeof(alpha));
      (*values)[pixel] = static_cast<float>(alpha) / 32768.0f;
    } else {
      float alpha{};
      std::memcpy(&alpha, source, sizeof(alpha));
      (*values)[pixel] = alpha;
    }
  }
  uint64_t identity = 0x434f565200000000ULL;
  for (const int32_t param : g_alpha_as_coverage_params) {
    if (param < 0 || static_cast<std::size_t>(param) > g_params.size() ||
        (param > 0 && g_params[static_cast<std::size_t>(param - 1)].type != 0) ||
        std::any_of(g_external_aux_set.channels.begin(), g_external_aux_set.channels.end(),
                    [&](const auto& existing) { return existing.param_index == param &&
                        existing.channel_type == 0x434f5652; }))
      continue;
    ExternalAuxChannel channel;
    channel.param_index = param;
    channel.channel_type = 0x434f5652;  // 'COVR'
    channel.dimension = 1;
    channel.width = width;
    channel.height = height;
    channel.signed_row_bytes = width * 4;
    channel.name = "Coverage";
    channel.coordinate_space = "pre_effect_source_pixel";
    channel.units = "normalized_coverage";
    channel.identity = ++identity;
    channel.samples.push_back(ExternalAuxSample{
        time, time_scale, "exact", values});
    g_native_aux_set.channels.push_back(std::move(channel));
  }
}

void clear_native_aux_provider() { g_native_aux_set = {}; }

bool parse_alpha_coverage_params(const wchar_t* text) {
  if (!text || !*text) return false;
  std::vector<int32_t> parsed;
  const std::wstring value(text);
  std::size_t begin = 0;
  while (begin < value.size()) {
    const std::size_t end = value.find(L',', begin);
    const std::wstring token = value.substr(begin, end - begin);
    try {
      std::size_t used = 0;
      const long slot = std::stol(token, &used);
      if (used != token.size() || slot < 0 || slot > 1024 ||
          std::find(parsed.begin(), parsed.end(), static_cast<int32_t>(slot)) != parsed.end())
        return false;
      parsed.push_back(static_cast<int32_t>(slot));
    } catch (...) { return false; }
    if (end == std::wstring::npos) break;
    begin = end + 1;
  }
  g_alpha_as_coverage_params = std::move(parsed);
  return !g_alpha_as_coverage_params.empty();
}

PfChannelRef make_channel_ref(int32_t param_index, int32_t channel_index = 0,
                              int32_t channel_type = kChannelDepth, uint64_t identity = 1) {
  PfChannelRef ref{kChannelRefMagic, g_channel_generation, param_index, channel_index, channel_type, 0};
  ref.seal = {kChannelRefMagic ^ ref.generation,
      (static_cast<uint64_t>(static_cast<uint32_t>(param_index)) << 32) ^ static_cast<uint32_t>(channel_index),
      static_cast<uint64_t>(static_cast<uint32_t>(channel_type)) ^ ref.generation,
      identity ^ ~kChannelRefMagic};
  return ref;
}

bool valid_channel_ref(const PfChannelRef& ref) {
  if (ref.magic != kChannelRefMagic || ref.generation != g_channel_generation) return false;
  if (g_channel_fixture_enabled) { const auto expected=make_channel_ref(0); return std::memcmp(&ref, &expected, sizeof(ref)) == 0; }
  int32_t index = 0;
  bool valid = false;
  for_each_active_aux_channel([&](const auto& channel) { if(channel.param_index == ref.param_index) {
    const PfChannelRef expected = make_channel_ref(ref.param_index, index, channel.channel_type, channel.identity);
    if (std::memcmp(&ref, &expected, sizeof(ref)) == 0) valid = true;
    ++index;
  }});
  return valid;
}

void describe_channel(PfChannelDesc* desc, int32_t type, const std::string& name,
                      int32_t dimension, int32_t native_data_type = kDataFloat) {
  std::memset(desc, 0, sizeof(*desc));
  desc->channel_type = type;
  std::memcpy(desc->name, name.data(), name.size());
  desc->data_type = native_data_type;
  desc->dimension = dimension;
}

const ExternalAuxChannel* channel_from_ref(const PfChannelRef& ref) {
  if (!valid_channel_ref(ref) || g_channel_fixture_enabled) return nullptr;
  int32_t index=0; const ExternalAuxChannel* result=nullptr;
  for_each_active_aux_channel([&](const auto& channel) { if (!result && channel.param_index==ref.param_index && index++==ref.channel_index) result=&channel; });
  return result;
}

int32_t channel_element_size(int32_t type) {
  switch (type) {
    case kDataFloat: case kDataLong: case kDataFixed: case kDataUFixed: return 4;
    case kDataDouble: return 8;
    case kDataShort: case kDataUShort: return 2;
    case kDataChar: case kDataUByte: return 1;
    default: return 0;
  }
}

struct NativeAuxSampleRequest { int32_t time{}, duration{}; uint32_t time_scale{}; };
const ExternalAuxSample* select_native_aux_sample(
    const ExternalAuxChannel& channel, const NativeAuxSampleRequest& request);

void store_channel_value(std::byte* destination, int32_t type, float value) {
  const double clamped_signed = std::max<double>(std::numeric_limits<int32_t>::min(),
      std::min<double>(std::numeric_limits<int32_t>::max(), value));
  switch (type) {
    case kDataFloat: std::memcpy(destination, &value, 4); break;
    case kDataDouble: { const double converted = value; std::memcpy(destination, &converted, 8); break; }
    case kDataLong: { const int32_t converted = static_cast<int32_t>(clamped_signed); std::memcpy(destination, &converted, 4); break; }
    case kDataShort: { const int16_t converted = static_cast<int16_t>(std::max(-32768.0f, std::min(32767.0f, value))); std::memcpy(destination, &converted, 2); break; }
    case kDataFixed: { const double scaled = static_cast<double>(value) * 65536.0; const int32_t converted = static_cast<int32_t>(std::max<double>(INT32_MIN, std::min<double>(INT32_MAX, scaled))); std::memcpy(destination, &converted, 4); break; }
    case kDataChar: { const int8_t converted = static_cast<int8_t>(std::max(-128.0f, std::min(127.0f, value))); std::memcpy(destination, &converted, 1); break; }
    case kDataUByte: { const uint8_t converted = static_cast<uint8_t>(std::max(0.0f, std::min(255.0f, value))); std::memcpy(destination, &converted, 1); break; }
    case kDataUShort: { const uint16_t converted = static_cast<uint16_t>(std::max(0.0f, std::min(65535.0f, value))); std::memcpy(destination, &converted, 2); break; }
    case kDataUFixed: { const double scaled = std::max(0.0, static_cast<double>(value) * 65536.0); const uint32_t converted = static_cast<uint32_t>(std::min<double>(UINT32_MAX, scaled)); std::memcpy(destination, &converted, 4); break; }
  }
}

int32_t __cdecl get_layer_channel_count(void* effect_ref, int32_t param_index,
                                        int32_t* channel_count) {
  if (effect_ref != &g_effect || !channel_count) return kPfBadCallbackParam;
  if (param_index < 0 || static_cast<std::size_t>(param_index) > g_params.size())
    return kPfInvalidIndex;
  *channel_count = 0;
  if (g_channel_fixture_enabled && param_index == 0) *channel_count = 1;
  else for_each_active_aux_channel([&](const auto& channel) {
    if (channel.param_index == param_index) ++*channel_count;
  });
  ++g_channel_count_queries;
  return 0;
}

int32_t __cdecl get_layer_channel_indexed(void* effect_ref, int32_t param_index,
    int32_t channel_index, uint8_t* found, void* channel_ref, void* channel_desc) {
  if (found) *found = 0;
  if (effect_ref != &g_effect || !found || !channel_ref || !channel_desc) return kPfBadCallbackParam;
  if (param_index < 0 || static_cast<std::size_t>(param_index) > g_params.size())
    return kPfInvalidIndex;
  if (g_channel_fixture_enabled) {
    if (param_index != 0) return 0; if (channel_index < 0 || channel_index >= 1) return kPfInvalidIndex;
    *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(0); describe_channel(static_cast<PfChannelDesc*>(channel_desc),kChannelDepth,"Depth",1);
  } else {
    int32_t index=0; const ExternalAuxChannel* selected=nullptr;
    for_each_active_aux_channel([&](const auto& channel) { if(!selected && channel.param_index==param_index && index++==channel_index) selected=&channel; });
    if(!selected)return kPfInvalidIndex; *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(param_index,channel_index,selected->channel_type,selected->identity); describe_channel(static_cast<PfChannelDesc*>(channel_desc),selected->channel_type,selected->name,selected->dimension,selected->native_data_type);
  }
  *found = 1;
  return 0;
}

int32_t __cdecl get_layer_channel_typed(void* effect_ref, int32_t param_index,
    int32_t channel_type, uint8_t* found, void* channel_ref, void* channel_desc) {
  if (found) *found = 0;
  if (effect_ref != &g_effect || !found || !channel_ref || !channel_desc) return kPfBadCallbackParam;
  if (param_index < 0 || static_cast<std::size_t>(param_index) > g_params.size())
    return kPfInvalidIndex;
  if (g_channel_fixture_enabled) { if(param_index!=0||channel_type!=kChannelDepth)return 0; *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(0); describe_channel(static_cast<PfChannelDesc*>(channel_desc),kChannelDepth,"Depth",1); }
  else { int32_t index=0; const ExternalAuxChannel* selected=nullptr; for_each_active_aux_channel([&](const auto& channel){if(channel.param_index==param_index){if(!selected&&channel.channel_type==channel_type)selected=&channel;else if(!selected)++index;}}); if(!selected)return 0; *static_cast<PfChannelRef*>(channel_ref)=make_channel_ref(param_index,index,selected->channel_type,selected->identity); describe_channel(static_cast<PfChannelDesc*>(channel_desc),selected->channel_type,selected->name,selected->dimension,selected->native_data_type); }
  *found = 1;
  return 0;
}

int32_t __cdecl checkout_layer_channel(void* effect_ref, void* channel_ref, int32_t time,
    int32_t duration, uint32_t time_scale, int32_t data_type, void* channel_chunk) {
  if (effect_ref != &g_effect || !channel_ref || !channel_chunk || time_scale == 0)
    return kPfBadCallbackParam;
  const auto& ref = *static_cast<const PfChannelRef*>(channel_ref);
  if (!valid_channel_ref(ref)) return kPfBadCallbackParam;
  const int32_t element_size = channel_element_size(data_type);
  if (element_size == 0) return kPfUnrecognizedParamType;
  const ExternalAuxChannel* external=channel_from_ref(ref);
  const int32_t width=external?external->width:kChannelFixtureWidth, height=external?external->height:kChannelFixtureHeight, dimension=external?external->dimension:1;
  const std::size_t pixels = static_cast<std::size_t>(width)*height*dimension;
  if (pixels > std::numeric_limits<std::size_t>::max() / static_cast<std::size_t>(element_size)) return 4;
  const int32_t native_packed_row = width * dimension * 4;
  const int32_t native_stride = external ? external->signed_row_bytes : native_packed_row;
  const int32_t padding = std::abs(native_stride) - native_packed_row;
  const int64_t converted_stride64 = static_cast<int64_t>(width) * dimension * element_size + padding;
  if (converted_stride64 <= 0 || converted_stride64 > INT32_MAX) return 4;
  const int32_t converted_stride = static_cast<int32_t>(converted_stride64);
  const int32_t signed_converted_stride = native_stride < 0 ? -converted_stride : converted_stride;
  const std::size_t bytes = static_cast<std::size_t>(converted_stride) * height;
  const float* source=kChannelFixture.data();
  std::shared_ptr<const std::vector<float>> sample_receipt;
  int32_t selected_time=0; uint32_t selected_scale=1;
  if (external) {
    const ExternalAuxSample* selected = select_native_aux_sample(
        *external, NativeAuxSampleRequest{time, duration, time_scale});
    if (!selected || !selected->values) return kPfInvalidIndex;
    sample_receipt = selected->values;
    source = sample_receipt->data();
    selected_time = selected->time;
    selected_scale = selected->time_scale;
  }
  auto* chunk = static_cast<PfChannelChunk*>(channel_chunk);
  { std::lock_guard<std::mutex> lock(g_channel_mutex); if(g_live_channel_chunks.count(chunk))return kPfBadCallbackParam; }
  void** handle=new_handle(bytes); if(!handle)return 4; void* locked=lock_handle(handle); if(!locked){dispose_handle(handle);return 4;}
  std::memset(locked, 0, bytes);
  auto* exposed = static_cast<std::byte*>(locked) +
      (signed_converted_stride < 0 ? static_cast<std::size_t>(height - 1) * converted_stride : 0);
  for (int32_t y = 0; y < height; ++y)
    for (int32_t x = 0; x < width * dimension; ++x)
      store_channel_value(exposed + static_cast<std::ptrdiff_t>(y) * signed_converted_stride +
                              static_cast<std::size_t>(x) * element_size,
                          data_type, source[static_cast<std::size_t>(y) * width * dimension + x]);
  bool duplicate = false;
  try {
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    duplicate = g_live_channel_chunks.find(chunk) != g_live_channel_chunks.end();
    if (!duplicate) {
      *chunk = {ref,width,height,dimension,signed_converted_stride,data_type,handle,exposed};
      LiveChannelChunk live{ref,data_type,chunk->row_bytes,width,height,dimension,bytes,handle,exposed,
                            std::move(sample_receipt),selected_time,selected_scale,duration,
                            std::this_thread::get_id()};
      g_live_channel_chunks.emplace(chunk, std::move(live));
    }
  } catch (const std::bad_alloc&) {
    unlock_handle(handle); dispose_handle(handle); *chunk={}; return 4;
  }
  if (duplicate) { unlock_handle(handle); dispose_handle(handle); return kPfBadCallbackParam; }
  return 0;
}

int32_t __cdecl checkin_layer_channel(void* effect_ref, void* channel_ref, void* channel_chunk) {
  if (effect_ref != &g_effect || !channel_ref || !channel_chunk) return kPfBadCallbackParam;
  auto* chunk = static_cast<PfChannelChunk*>(channel_chunk);
  LiveChannelChunk live;
  {
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    const auto found = g_live_channel_chunks.find(chunk);
    if (found == g_live_channel_chunks.end()) return kPfBadCallbackParam;
    live = found->second;
    if (live.owner_thread != std::this_thread::get_id() ||
        std::memcmp(channel_ref, &live.ref, sizeof(live.ref)) != 0 ||
        std::memcmp(&chunk->channel_ref, &live.ref, sizeof(live.ref)) != 0 ||
        chunk->data != live.locked_data || chunk->data_handle != live.handle ||
        chunk->row_bytes != live.row_bytes || chunk->width != live.width ||
        chunk->height != live.height || chunk->dimension != live.dimension ||
        chunk->data_type != live.data_type) return kPfBadCallbackParam;
    g_live_channel_chunks.erase(found);
  }
  unlock_handle(live.handle); dispose_handle(live.handle);
  *chunk = {};
  return 0;
}

void reclaim_layer_channels() {
  std::vector<LiveChannelChunk> live;
  {
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    for (auto item = g_live_channel_chunks.begin(); item != g_live_channel_chunks.end();) {
      if (item->second.owner_thread == std::this_thread::get_id()) {
        live.push_back(item->second);
        item = g_live_channel_chunks.erase(item);
      } else {
        ++item;
      }
    }
  }
  for(auto& item:live){unlock_handle(item.handle);dispose_handle(item.handle);}
  ++g_channel_generation;
  if (g_channel_generation == 0) ++g_channel_generation;
}

bool validate_external_aux_parameters() {
  if (!g_external_aux_loaded) return true;
  return std::all_of(g_external_aux_set.channels.begin(), g_external_aux_set.channels.end(),
      [](const auto& channel) {
        return channel.param_index == 0 ||
            (channel.param_index > 0 && static_cast<std::size_t>(channel.param_index) <= g_params.size() &&
             g_params[static_cast<std::size_t>(channel.param_index - 1)].type == 0);
      });
}

bool verify_pf_ae_channel_suite() {
  reclaim_layer_channels();
  g_channel_fixture_enabled = true;
  int32_t count = -1;
  uint8_t found = 7;
  PfChannelRef ref{};
  PfChannelDesc desc{};
  bool passed = get_layer_channel_count(&g_effect, 0, &count) == 0 && count == 1 &&
      get_layer_channel_indexed(&g_effect, 0, 0, &found, &ref, &desc) == 0 && found == 1 &&
      desc.channel_type == kChannelDepth && desc.data_type == kDataFloat && desc.dimension == 1;
  found = 7;
  passed = passed && get_layer_channel_typed(&g_effect, 0, 0x554e4b4e, &found,
      &ref, &desc) == 0 && found == 0;
  passed = passed && get_layer_channel_indexed(&g_effect, 0, 1, &found,
      &ref, &desc) == kPfInvalidIndex && found == 0;
  found = 0;
  passed = passed && get_layer_channel_typed(&g_effect, 0, kChannelDepth, &found,
      &ref, &desc) == 0 && found == 1;
  const std::array<int32_t, 9> types{kDataFloat, kDataDouble, kDataLong, kDataShort,
      kDataFixed, kDataChar, kDataUByte, kDataUShort, kDataUFixed};
  for (const int32_t type : types) {
    PfChannelChunk chunk{};
    passed = passed && checkout_layer_channel(&g_effect, &ref, 0, 1, 1, type, &chunk) == 0 &&
        chunk.data != nullptr && chunk.row_bytes == kChannelFixtureWidth * channel_element_size(type);
    if (type == kDataFloat && chunk.data) {
      float value = -1.0f;
      std::memcpy(&value, chunk.data, sizeof(value));
      passed = passed && value == 0.0f;
    }
    PfChannelChunk forged = chunk;
    passed = passed && checkin_layer_channel(&g_effect, &ref, &forged) == kPfBadCallbackParam &&
        checkin_layer_channel(&g_effect, &ref, &chunk) == 0 &&
        checkin_layer_channel(&g_effect, &ref, &chunk) == kPfBadCallbackParam;
  }
  PfChannelChunk unsupported{};
  passed = passed && checkout_layer_channel(&g_effect, &ref, 0, 1, 1,
      0x52424720, &unsupported) == kPfUnrecognizedParamType;
  PfChannelRef forged_ref = ref;
  forged_ref.magic ^= 1;
  passed = passed && checkout_layer_channel(&g_effect, &forged_ref, 0, 1, 1,
      kDataFloat, &unsupported) == kPfBadCallbackParam;
  PfChannelChunk leaked{};
  passed = passed && checkout_layer_channel(&g_effect, &ref, 0, 1, 1,
      kDataFloat, &leaked) == 0;
  reclaim_layer_channels();
  passed = passed && g_live_channel_chunks.empty() &&
      checkout_layer_channel(&g_effect, &ref, 0, 1, 1,
          kDataFloat, &leaked) == kPfBadCallbackParam;
  g_channel_fixture_enabled = false;
  count = -1;
  passed = passed && get_layer_channel_count(&g_effect, 0, &count) == 0 && count == 0;
  return passed;
}

bool verify_pf_ae_channel_transport(const std::filesystem::path& manifest) {
  reclaim_layer_channels();
  g_channel_fixture_enabled = false;
  if (!load_aux_manifest(manifest, g_external_aux_set)) return false;
  g_external_aux_loaded = g_external_aux_active = true;
  int32_t count=-1; uint8_t found=0; PfChannelRef ref{}; PfChannelDesc desc{};
  bool passed=get_layer_channel_count(&g_effect,0,&count)==0&&count>0&&
      get_layer_channel_indexed(&g_effect,0,0,&found,&ref,&desc)==0&&found==1;
  PfChannelChunk exact{}, held{};
  passed=passed&&checkout_layer_channel(&g_effect,&ref,0,1,30,kDataFloat,&exact)==0&&
      exact.data_handle!=nullptr&&exact.data!=nullptr;
  if (!g_external_aux_set.channels.empty()) {
    g_channel_transport_row_bytes = exact.row_bytes;
    g_channel_transport_origin_x = g_external_aux_set.channels.front().origin_x;
    g_channel_transport_origin_y = g_external_aux_set.channels.front().origin_y;
    std::lock_guard<std::mutex> lock(g_channel_mutex);
    const auto live = g_live_channel_chunks.find(&exact);
    if (live != g_live_channel_chunks.end()) g_channel_transport_duration = live->second.duration;
  }
  if (passed && !g_external_aux_set.channels.empty() &&
      !g_external_aux_set.channels.front().samples.empty()) {
    const auto& expected = g_external_aux_set.channels.front().samples.front().values;
    passed = expected && expected->size() == static_cast<std::size_t>(exact.width) * exact.height * exact.dimension;
    for (int32_t y = 0; passed && y < exact.height; ++y)
      for (int32_t x = 0; passed && x < exact.width * exact.dimension; ++x) {
        float actual{};
        std::memcpy(&actual, static_cast<std::byte*>(exact.data) +
                    static_cast<std::ptrdiff_t>(y) * exact.row_bytes + x * 4, 4);
        passed = actual == (*expected)[static_cast<std::size_t>(y) * exact.width * exact.dimension + x];
      }
  }
  passed=passed&&
      checkin_layer_channel(&g_effect,&ref,&exact)==0&&
      checkout_layer_channel(&g_effect,&ref,15,1,30,kDataDouble,&held)==0&&
      held.data_handle!=nullptr&&held.data!=nullptr&&
      checkin_layer_channel(&g_effect,&ref,&held)==0;
  reclaim_layer_channels(); g_external_aux_active=g_external_aux_loaded=false;
  g_external_aux_set={};
  return passed&&g_live_channel_chunks.empty();
}

int32_t __cdecl duck_quack(uint16_t times) {
  if (times > 64) return 4;
  g_duck_quacks += times;
  return 0;
}

int32_t __cdecl abort_render(void* effect_ref) {
  if (!effect_ref) return 4;
  ++g_abort_calls;
  return 0;
}

int32_t __cdecl report_progress(void* effect_ref, int32_t current, int32_t total) {
  if (!effect_ref || total <= 0 || current < 0 || current > total) return 4;
  ++g_progress_calls;
  g_last_progress_current = current;
  g_last_progress_total = total;
  return 0;
}

int32_t __cdecl register_custom_ui(void* effect_ref, const void* custom_ui_info) {
  if (effect_ref != &g_effect || !custom_ui_info) return 4;
  std::array<std::byte, 44> bytes{};
  std::memcpy(bytes.data(), custom_ui_info, bytes.size());
  CustomUiRegistration registration{
      read<uint32_t>(bytes, 4), read<int32_t>(bytes, 8), read<int32_t>(bytes, 12),
      read<int32_t>(bytes, 16), read<int32_t>(bytes, 20), read<int32_t>(bytes, 24),
      read<int32_t>(bytes, 28), read<int32_t>(bytes, 32), read<int32_t>(bytes, 36),
      read<int32_t>(bytes, 40)};
  const auto valid_dimension = [](int32_t value) { return value >= 0 && value <= 8192; };
  if ((registration.events & ~15u) != 0 ||
      !valid_dimension(registration.comp_width) ||
      !valid_dimension(registration.comp_height) ||
      !valid_dimension(registration.layer_width) ||
      !valid_dimension(registration.layer_height) ||
      !valid_dimension(registration.preview_width) ||
      !valid_dimension(registration.preview_height)) {
    ++g_invalid_custom_ui_registrations;
    return 4;
  }
  g_custom_ui_registration = registration;
  ++g_register_ui_calls;
  return 0;
}

int32_t __cdecl adv_app_info_text(const char* first, const char* second) {
  if (!first || !second || strnlen_s(first, 256) == 256 || strnlen_s(second, 256) == 256)
    return 4;
  g_last_adv_app_info_text = std::string(first) + " | " + second;
  ++g_adv_app_info_text_calls;
  return 0;
}

int32_t __cdecl adv_app_info_text3(const char* first, const char* second,
                                   const char* third) {
  if (!first || !second || (third && strnlen_s(third, 256) == 256) ||
      strnlen_s(first, 256) == 256 || strnlen_s(second, 256) == 256) return 4;
  g_last_adv_app_info_text = std::string(first) + " | " + second;
  if (third) g_last_adv_app_info_text += std::string(" | ") + third;
  ++g_adv_app_info_text_calls;
  return 0;
}

bool dispatch_conditional_ui_selectors(EffectEntry entry,
                                       std::array<std::byte, kInSize>& input,
                                       std::array<std::byte, kOutSize>& output,
                                       void** params) {
  g_conditional_ui_selectors_dispatched = false;
  g_update_params_ui_error = -1;
  g_query_dynamic_flags_error = -1;
  if (g_update_params_ui_advertised) {
    g_active_ui_params = params;
    g_active_ui_param_count = g_params.size() + 1;
    g_update_params_ui_active = true;
    g_update_params_ui_error = entry(kUpdateParamsUi, input.data(), output.data(), params,
                                     nullptr, nullptr);
    g_update_params_ui_active = false;
    g_active_ui_params = nullptr;
    g_active_ui_param_count = 0;
    g_conditional_ui_selectors_dispatched = true;
  }
  if (g_query_dynamic_flags_advertised) {
    g_query_dynamic_flags_error = entry(kQueryDynamicFlags, input.data(), output.data(), params,
                                        nullptr, nullptr);
    g_conditional_ui_selectors_dispatched = true;
  }
  return (!g_update_params_ui_advertised || g_update_params_ui_error == 0) &&
      (!g_query_dynamic_flags_advertised || g_query_dynamic_flags_error == 0);
}

std::string escape(const std::string& input) {
  std::string output;
  for (unsigned char ch : input) {
    if (ch == '"' || ch == '\\') output.push_back('\\');
    if (ch >= 0x20 && ch < 0x7f) output.push_back(static_cast<char>(ch));
  }
  return output;
}

bool sha256(const std::filesystem::path& path, std::string& result) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0 ||
      BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                        reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size),
                        &returned, 0) < 0) return false;
  std::vector<unsigned char> object(object_size);
  if (BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0) < 0) return false;
  std::ifstream input(path, std::ios::binary);
  std::array<unsigned char, 65536> buffer{};
  while (input) {
    input.read(reinterpret_cast<char*>(buffer.data()), buffer.size());
    if (input.gcount() > 0 && BCryptHashData(hash, buffer.data(), static_cast<ULONG>(input.gcount()), 0) < 0) return false;
  }
  std::array<unsigned char, 32> digest{};
  const bool ok = input.eof() && BCryptFinishHash(hash, digest.data(), digest.size(), 0) >= 0;
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  if (!ok) return false;
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  result = text.str();
  return true;
}

std::string sha256_bytes(const unsigned char* data, std::size_t size) {
  BCRYPT_ALG_HANDLE algorithm{};
  BCRYPT_HASH_HANDLE hash{};
  DWORD object_size{}, returned{};
  BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0);
  BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                    reinterpret_cast<PUCHAR>(&object_size), sizeof(object_size), &returned, 0);
  std::vector<unsigned char> object(object_size);
  BCryptCreateHash(algorithm, &hash, object.data(), object_size, nullptr, 0, 0);
  BCryptHashData(hash, const_cast<PUCHAR>(data), static_cast<ULONG>(size), 0);
  std::array<unsigned char, 32> digest{};
  BCryptFinishHash(hash, digest.data(), digest.size(), 0);
  BCryptDestroyHash(hash);
  BCryptCloseAlgorithmProvider(algorithm, 0);
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (auto byte : digest) text << std::setw(2) << static_cast<unsigned>(byte);
  return text.str();
}

const ExternalAuxSample* select_native_aux_sample(
    const ExternalAuxChannel& channel, const NativeAuxSampleRequest& request) {
  if (request.time_scale == 0) return nullptr;
  const ExternalAuxSample* selected = nullptr;
  for (const auto& sample : channel.samples) {
    const int64_t lhs = static_cast<int64_t>(sample.time) * request.time_scale;
    const int64_t rhs = static_cast<int64_t>(request.time) * sample.time_scale;
    if (lhs == rhs) return &sample;
    if (sample.sampling == "hold" && lhs < rhs &&
        (!selected || static_cast<int64_t>(selected->time) * sample.time_scale <
                         static_cast<int64_t>(sample.time) * selected->time_scale))
      selected = &sample;
  }
  (void)request.duration;
  return selected;
}

bool verify_pf_ae_channel_native_provider() {
  reclaim_layer_channels();
  g_channel_fixture_enabled = false;
  g_external_aux_active = false;
  g_alpha_as_coverage_params = {0};
  const std::array<float, 4> expected{0.0f, 0.25f, 0.5f, 1.0f};
  bool passed = true;
  for (const int32_t pixel_bytes : {4, 8, 16}) {
    std::vector<unsigned char> argb(static_cast<std::size_t>(4) * pixel_bytes, 0);
    for (std::size_t pixel = 0; pixel < expected.size(); ++pixel) {
      if (pixel_bytes == 4) {
        argb[pixel * 4] = static_cast<uint8_t>(std::lround(expected[pixel] * 255.0f));
      } else if (pixel_bytes == 8) {
        const uint16_t alpha = static_cast<uint16_t>(std::lround(expected[pixel] * 32768.0f));
        std::memcpy(argb.data() + pixel * 8, &alpha, sizeof(alpha));
      } else {
        std::memcpy(argb.data() + pixel * 16, &expected[pixel], sizeof(float));
      }
    }
    publish_alpha_coverage_provider(argb, 2, 2, pixel_bytes, 7, 30);
    int32_t count = 0; uint8_t found = 0; PfChannelRef ref{}; PfChannelDesc desc{};
    passed = passed && get_layer_channel_count(&g_effect, 0, &count) == 0 && count == 1 &&
        get_layer_channel_typed(&g_effect, 0, 0x434f5652, &found, &ref, &desc) == 0 &&
        found == 1 && desc.channel_type == 0x434f5652 && desc.data_type == kDataFloat &&
        desc.dimension == 1;
    PfChannelChunk chunk{};
    passed = passed && checkout_layer_channel(&g_effect, &ref, 7, 5, 30, kDataFloat, &chunk) == 0;
    if (chunk.data) {
      for (std::size_t pixel = 0; pixel < expected.size(); ++pixel) {
        float value{}; std::memcpy(&value, static_cast<std::byte*>(chunk.data) + pixel * 4, 4);
        const float tolerance = pixel_bytes == 4 ? 1.0f / 255.0f : 0.00001f;
        passed = passed && std::abs(value - expected[pixel]) <= tolerance;
      }
    }
    {
      std::lock_guard<std::mutex> lock(g_channel_mutex);
      const auto live = g_live_channel_chunks.find(&chunk);
      passed = passed && live != g_live_channel_chunks.end() && live->second.sample_receipt &&
          live->second.duration == 5 && live->second.sample_time == 7 &&
          live->second.sample_time_scale == 30;
    }
    passed = passed && checkin_layer_channel(&g_effect, &ref, &chunk) == 0 &&
        checkin_layer_channel(&g_effect, &ref, &chunk) == kPfBadCallbackParam;
    clear_native_aux_provider();
  }
  std::atomic<bool> mfr_passed{true};
  std::vector<std::thread> mfr_threads;
  for (int thread_index = 0; thread_index < 8; ++thread_index) {
    mfr_threads.emplace_back([&] {
      const std::vector<unsigned char> argb{128, 1, 2, 3};
      for (int iteration = 0; iteration < 256; ++iteration) {
        publish_alpha_coverage_provider(argb, 1, 1, 4, iteration, 30);
        int32_t count = 0; uint8_t found = 0; PfChannelRef ref{}; PfChannelDesc desc{};
        PfChannelChunk chunk{};
        if (get_layer_channel_count(&g_effect, 0, &count) != 0 || count != 1 ||
            get_layer_channel_typed(&g_effect, 0, 0x434f5652, &found, &ref, &desc) != 0 ||
            !found || checkout_layer_channel(&g_effect, &ref, iteration, 3, 30,
                                              kDataFloat, &chunk) != 0 ||
            checkin_layer_channel(&g_effect, &ref, &chunk) != 0) {
          mfr_passed = false;
          break;
        }
        clear_native_aux_provider();
      }
      reclaim_layer_channels();
      clear_native_aux_provider();
    });
  }
  for (auto& thread : mfr_threads) thread.join();
  passed = passed && mfr_passed.load();
  g_alpha_as_coverage_params.clear();
  reclaim_layer_channels();
  return passed && g_live_channel_chunks.empty();
}

bool verify_pf_effect_sequence_data_suite1() {
  invalidate_effect_sequence(&g_effect);
  const void* acquired{};
  PfConstHandle observed = reinterpret_cast<PfConstHandle>(1);
  int payload = 0x53455131;
  void* handle_value = &payload;
  void** handle = &handle_value;
  OpaqueHostObject foreign{0x4652474e};
  bool ok = acquire_suite("PF Effect Sequence Data Suite", 1, &acquired) == 0 &&
      acquired == &g_effect_sequence_data_suite1 &&
      g_effect_sequence_data_suite1.get_effect_sequence_data(&g_effect, &observed) ==
          kPfBadCallbackParam && observed == nullptr &&
      publish_effect_sequence(&g_effect, handle) &&
      g_effect_sequence_data_suite1.get_effect_sequence_data(&g_effect, &observed) == 0 &&
      observed == reinterpret_cast<PfConstHandle>(handle) && *observed == &payload;
  std::atomic<bool> concurrent_ok{true};
  std::vector<std::thread> readers;
  for (int thread_index = 0; thread_index < 8; ++thread_index) {
    readers.emplace_back([&] {
      for (int iteration = 0; iteration < 256; ++iteration) {
        PfConstHandle concurrent_observed{};
        if (g_effect_sequence_data_suite1.get_effect_sequence_data(
                &g_effect, &concurrent_observed) != 0 ||
            concurrent_observed != reinterpret_cast<PfConstHandle>(handle) ||
            *concurrent_observed != &payload) {
          concurrent_ok.store(false, std::memory_order_relaxed);
          break;
        }
      }
    });
  }
  for (auto& reader : readers) reader.join();
  ok = ok && concurrent_ok.load(std::memory_order_relaxed);
  PfConstHandle foreign_observed = reinterpret_cast<PfConstHandle>(1);
  ok = ok && g_effect_sequence_data_suite1.get_effect_sequence_data(
                    &foreign, &foreign_observed) == kPfBadCallbackParam &&
      foreign_observed == nullptr &&
      g_effect_sequence_data_suite1.get_effect_sequence_data(nullptr, &observed) ==
          kPfBadCallbackParam &&
      g_effect_sequence_data_suite1.get_effect_sequence_data(&g_effect, nullptr) ==
          kPfBadCallbackParam;
  invalidate_effect_sequence(&g_effect);
  observed = reinterpret_cast<PfConstHandle>(1);
  ok = ok && g_effect_sequence_data_suite1.get_effect_sequence_data(&g_effect, &observed) ==
                    kPfBadCallbackParam && observed == nullptr &&
      release_suite("PF Effect Sequence Data Suite", 1) == 0;
  return ok;
}

std::string hex_bytes(const unsigned char* data, std::size_t size) {
  std::ostringstream text;
  text << std::hex << std::setfill('0');
  for (std::size_t index = 0; index < size; ++index)
    text << std::setw(2) << static_cast<unsigned>(data[index]);
  return text.str();
}

auto& g_user_changed_parameters = g_parameter_runtime.user_changed_parameters;

bool parse_i32_arg(const wchar_t* text, int32_t minimum, int32_t maximum, int32_t& output) {
  if (!text || !*text) return false;
  wchar_t* end = nullptr;
  errno = 0;
  const long long value = std::wcstoll(text, &end, 10);
  if (errno != 0 || !end || *end != L'\0' || value < minimum || value > maximum) return false;
  output = static_cast<int32_t>(value);
  return true;
}

bool parse_double_arg(const wchar_t* text, double minimum, double maximum, double& output) {
  if (!text || !*text) return false;
  wchar_t* end = nullptr;
  errno = 0;
  const double value = std::wcstod(text, &end);
  if (errno != 0 || !end || *end != L'\0' || !std::isfinite(value) || value < minimum || value > maximum)
    return false;
  output = value;
  return true;
}

bool parse_mask_context_payload(const wchar_t* text) {
  if (!text) return false;
  if (!g_stream_refs.empty() || !g_stream_values.empty() ||
      !g_add_keyframe_transactions.empty()) return false;
  const std::wstring encoded(text);
  if (encoded.size() < 3 || encoded.size() > 8192 || encoded.compare(0, 3, L"v2|") != 0)
    return false;
  std::vector<HostMask> masks;
  std::size_t total_vertices = 0;
  const std::wstring payload = encoded.substr(3);
  if (payload.empty()) {
    g_mask_scene.clear();
    g_mask_lifetime = {};
    g_mask_scene_id = "request_v4";
    return true;
  }
  std::size_t mask_offset = 0;
  while (mask_offset < payload.size()) {
    const std::size_t mask_separator = payload.find(L';', mask_offset);
    const std::size_t mask_end = mask_separator == std::wstring::npos
        ? payload.size() : mask_separator;
    const std::wstring item = payload.substr(mask_offset, mask_end - mask_offset);
    if (item.size() < 5 || (item.compare(0, 2, L"0:") != 0 &&
                            item.compare(0, 2, L"1:") != 0)) return false;
    HostMask mask;
    mask.id = g_next_mask_id++;
    mask.outline_stream_id = g_next_stream_id++;
    mask.feather_stream_id = g_next_stream_id++;
    mask.opacity_stream_id = g_next_stream_id++;
    mask.expansion_stream_id = g_next_stream_id++;
    mask.dynamic_order = static_cast<int32_t>(masks.size());
    mask.open = item[0] == L'1';
    std::size_t vertex_offset = 2;
    while (vertex_offset < item.size()) {
      const std::size_t vertex_separator = item.find(L'/', vertex_offset);
      const std::size_t vertex_end = vertex_separator == std::wstring::npos
          ? item.size() : vertex_separator;
      const std::wstring point = item.substr(vertex_offset, vertex_end - vertex_offset);
      std::array<double, 6> components{};
      std::size_t component_offset = 0;
      for (std::size_t component = 0; component < components.size(); ++component) {
        const std::size_t comma = point.find(L',', component_offset);
        const bool final_component = component + 1 == components.size();
        if ((final_component && comma != std::wstring::npos) ||
            (!final_component && comma == std::wstring::npos)) return false;
        const std::size_t component_end = final_component ? point.size() : comma;
        if (!parse_double_arg(point.substr(component_offset, component_end - component_offset).c_str(),
                              -32768.0, 32768.0, components[component])) return false;
        component_offset = component_end + 1;
      }
      mask.vertices.push_back({components[0], components[1], components[2],
                               components[3], components[4], components[5]});
      if (mask.vertices.size() > 64 || ++total_vertices > 128) return false;
      if (vertex_separator == std::wstring::npos) break;
      vertex_offset = vertex_separator + 1;
      if (vertex_offset == item.size()) return false;
    }
    if (mask.vertices.size() < (mask.open ? 2u : 3u)) return false;
    if (!mask.open) mask.vertices.push_back(mask.vertices.front());
    masks.push_back(std::move(mask));
    if (masks.size() > 8) return false;
    if (mask_separator == std::wstring::npos) break;
    mask_offset = mask_separator + 1;
    if (mask_offset == payload.size()) return false;
  }
  g_mask_scene = std::move(masks);
  g_mask_scene.reserve(kMaxHostMasks);
  g_mask_lifetime = {};
  g_mask_scene_id = "request_v4";
  return true;
}

bool parse_spatial_context_payload(const wchar_t* text) {
  if (!text) return false;
  const std::wstring encoded(text);
  const bool version3 = encoded.compare(0, 11, L"spatial:v3|") == 0;
  const bool version2 = encoded.compare(0, 11, L"spatial:v2|") == 0;
  if ((!version3 && !version2 && encoded.compare(0, 11, L"spatial:v1|") != 0) || encoded.size() > 160) return false;
  const std::wstring payload = encoded.substr(11);
  std::array<int32_t, 10> values{};
  const std::size_t value_count = version3 ? 10 : (version2 ? 8 : 6);
  std::size_t offset = 0;
  for (std::size_t index = 0; index < value_count; ++index) {
    const auto comma = payload.find(L',', offset);
    const bool final = index + 1 == value_count;
    if ((final && comma != std::wstring::npos) || (!final && comma == std::wstring::npos)) return false;
    const auto end = final ? payload.size() : comma;
    const int32_t minimum = index < 6 ? 1 : (index < 8 ? (version3 ? 0 : 1) : -32768);
    const int32_t maximum = index < 6 ? 1'000'000 : 32768;
    if (!parse_i32_arg(payload.substr(offset, end - offset).c_str(), minimum, maximum, values[index])) return false;
    offset = end + 1;
  }
  g_downsample_x = {values[0], static_cast<uint32_t>(values[1])};
  g_downsample_y = {values[2], static_cast<uint32_t>(values[3])};
  g_pixel_aspect_ratio = {values[4], static_cast<uint32_t>(values[5])};
  g_full_resolution_width = version2 || version3 ? values[6] : 0;
  g_full_resolution_height = version2 || version3 ? values[7] : 0;
  g_pre_effect_source_origin_x = version3 ? values[8] : 0;
  g_pre_effect_source_origin_y = version3 ? values[9] : 0;
  if (g_full_resolution_width > 32768 || g_full_resolution_height > 32768) return false;
  return true;
}

bool parse_render_environment_payload(const wchar_t* text) {
  if (!text) return false;
  const std::wstring encoded(text);
  if (encoded.compare(0, 10, L"render:v1|") != 0 || encoded.size() > 96) return false;
  const std::wstring payload = encoded.substr(10);
  std::array<int32_t, 4> values{};
  std::size_t offset = 0;
  for (std::size_t index = 0; index < values.size(); ++index) {
    const auto comma = payload.find(L',', offset);
    const bool final = index + 1 == values.size();
    if ((final && comma != std::wstring::npos) || (!final && comma == std::wstring::npos)) return false;
    const auto end = final ? payload.size() : comma;
    if (!parse_i32_arg(payload.substr(offset, end - offset).c_str(),
                       index == 3 ? -65536 : 0,
                       index < 2 ? (index == 0 ? 1 : 2) : 65536,
                       values[index])) return false;
    offset = end + 1;
  }
  g_render_quality = values[0];
  g_render_field = values[1];
  g_shutter_angle = values[2];
  g_shutter_phase = values[3];
  return true;
}

bool valid_parameter_id(const std::wstring& id) {
  if (id.empty() || id.size() > 64 || id.front() < L'a' || id.front() > L'z') return false;
  return std::all_of(id.begin(), id.end(), [](wchar_t character) {
    return (character >= L'a' && character <= L'z') ||
           (character >= L'0' && character <= L'9') || character == L'_';
  });
}

bool parse_parameter_payload(const wchar_t* text, RequestedAssignments& output) {
  if (!text) return false;
  const std::wstring encoded(text);
  const bool version5 = encoded.compare(0, 3, L"v5|") == 0;
  const bool version4 = encoded.compare(0, 3, L"v4|") == 0;
  const bool version3 = encoded.compare(0, 3, L"v3|") == 0;
  if (encoded.size() < 3 || encoded.size() > 16384 ||
      (!version5 && !version4 && !version3 && encoded.compare(0, 3, L"v2|") != 0)) return false;
  const std::wstring payload = encoded.substr(3);
  if (payload.empty()) return true;
  std::unordered_set<std::wstring> seen_ids;
  std::unordered_set<int32_t> seen_indices;
  std::size_t offset = 0;
  while (offset < payload.size()) {
    const std::size_t separator = payload.find(L';', offset);
    const std::size_t end = separator == std::wstring::npos ? payload.size() : separator;
    const std::wstring assignment = payload.substr(offset, end - offset);
    const std::size_t at = assignment.find(L'@');
    const std::size_t colon = assignment.find(L':', at == std::wstring::npos ? 0 : at + 1);
    const std::size_t equals = assignment.find(L'=', colon == std::wstring::npos ? 0 : colon + 1);
    if (at == std::wstring::npos || colon == std::wstring::npos || equals == std::wstring::npos ||
        at == 0 || colon <= at + 1 || equals <= colon + 1 || equals + 1 >= assignment.size() ||
        assignment.find(L'=', equals + 1) != std::wstring::npos) return false;
    const std::wstring id = assignment.substr(0, at);
    const std::wstring index_text = assignment.substr(at + 1, colon - at - 1);
    const std::wstring kind_text = assignment.substr(colon + 1, equals - colon - 1);
    const std::wstring value = assignment.substr(equals + 1);
    int32_t index{};
    if (!valid_parameter_id(id) || value.size() > 8192 ||
        !parse_i32_arg(index_text.c_str(), 1, static_cast<int32_t>(kMaxParams), index) ||
        !seen_ids.insert(id).second || !seen_indices.insert(index).second) return false;
    RequestedKind kind{};
    if (kind_text == L"i32") kind = RequestedKind::Integer;
    else if (kind_text == L"f64") kind = RequestedKind::Float;
    else if ((version3 || version4 || version5) && kind_text == L"argb8") kind = RequestedKind::Color;
    else if ((version4 || version5) && kind_text == L"angle") kind = RequestedKind::Angle;
    else if ((version4 || version5) && kind_text == L"point") kind = RequestedKind::Point;
    else if ((version4 || version5) && kind_text == L"point3d") kind = RequestedKind::Point3D;
    else if (version5 && kind_text == L"arbhex") kind = RequestedKind::ArbitraryText;
    else return false;
    double parsed{};
    std::array<unsigned char, 4> color{};
    std::array<double, 3> components{};
    std::string arbitrary_text;
    if (kind == RequestedKind::Color) {
      std::size_t start = 0;
      for (std::size_t channel = 0; channel < color.size(); ++channel) {
        const std::size_t comma = value.find(L',', start);
        const bool final_channel = channel + 1 == color.size();
        if ((final_channel && comma != std::wstring::npos) ||
            (!final_channel && comma == std::wstring::npos)) return false;
        const std::size_t finish = final_channel ? value.size() : comma;
        int32_t component{};
        if (!parse_i32_arg(value.substr(start, finish - start).c_str(), 0, 255, component))
          return false;
        color[channel] = static_cast<unsigned char>(component);
        start = finish + 1;
      }
    } else if (kind == RequestedKind::Angle || kind == RequestedKind::Point || kind == RequestedKind::Point3D) {
      const std::size_t count = kind == RequestedKind::Point3D ? 3 : (kind == RequestedKind::Point ? 2 : 1);
      std::size_t start = 0;
      for (std::size_t component = 0; component < count; ++component) {
        const std::size_t comma = value.find(L',', start);
        const bool final_component = component + 1 == count;
        if ((final_component && comma != std::wstring::npos) || (!final_component && comma == std::wstring::npos)) return false;
        const std::size_t finish = final_component ? value.size() : comma;
        if (!parse_double_arg(value.substr(start, finish - start).c_str(), -32768.0, 32768.0, components[component])) return false;
        start = finish + 1;
      }
    } else if (kind == RequestedKind::ArbitraryText) {
      if (value.empty() || value.size() > 8192 || value.size() % 2 != 0) return false;
      arbitrary_text.reserve(value.size() / 2);
      const auto nibble = [](wchar_t ch) -> int {
        if (ch >= L'0' && ch <= L'9') return ch - L'0';
        if (ch >= L'a' && ch <= L'f') return ch - L'a' + 10;
        return -1;
      };
      for (std::size_t pos = 0; pos < value.size(); pos += 2) {
        const int high = nibble(value[pos]), low = nibble(value[pos + 1]);
        if (high < 0 || low < 0) return false;
        arbitrary_text.push_back(static_cast<char>((high << 4) | low));
      }
      if (arbitrary_text.empty() || arbitrary_text.size() > 4096 ||
          arbitrary_text.find('\0') != std::string::npos) return false;
    } else {
      const double minimum = kind == RequestedKind::Integer
          ? static_cast<double>((std::numeric_limits<int32_t>::min)())
          : -(std::numeric_limits<double>::max)();
      const double maximum = kind == RequestedKind::Integer
          ? static_cast<double>((std::numeric_limits<int32_t>::max)())
          : (std::numeric_limits<double>::max)();
      if (!parse_double_arg(value.c_str(), minimum, maximum, parsed) ||
          (kind == RequestedKind::Integer && std::trunc(parsed) != parsed)) return false;
    }
    output.push_back({id, index, kind, parsed, color, components, arbitrary_text});
    if (output.size() > kMaxParams) return false;
    if (separator == std::wstring::npos) break;
    offset = separator + 1;
    if (offset == payload.size()) return false;
  }
  return true;
}

bool validate_requested_assignments(const RequestedAssignments& requested) {
  for (const auto& assignment : requested) {
    if (assignment.index < 1 || static_cast<std::size_t>(assignment.index) > g_params.size()) return false;
    const auto& descriptor = g_params[static_cast<std::size_t>(assignment.index - 1)];
    const bool integer_compatible = descriptor.type == 1 || descriptor.type == 4 ||
        descriptor.type == 7 || descriptor.type == 12;
    const bool float_compatible = descriptor.type == 2 || descriptor.type == 10;
    const bool color_compatible = descriptor.type == 5;
    const bool angle_compatible = descriptor.type == 3;
    const bool point_compatible = descriptor.type == 6;
    const bool point3d_compatible = descriptor.type == 18;
    const bool arbitrary_compatible = descriptor.type == 11;
    if ((assignment.kind == RequestedKind::Integer && !integer_compatible) ||
        (assignment.kind == RequestedKind::Float && !float_compatible) ||
        (assignment.kind == RequestedKind::Color && !color_compatible) ||
        (assignment.kind == RequestedKind::Angle && !angle_compatible) ||
        (assignment.kind == RequestedKind::Point && !point_compatible) ||
        (assignment.kind == RequestedKind::Point3D && !point3d_compatible) ||
        (assignment.kind == RequestedKind::ArbitraryText && !arbitrary_compatible) ||
        (descriptor.type == 12 && (assignment.value < 0 ||
         assignment.value > static_cast<double>(ordered_active_masks().size()) ||
         std::trunc(assignment.value) != assignment.value)) ||
        ((assignment.kind == RequestedKind::Integer || assignment.kind == RequestedKind::Float) && descriptor.type != 12 && (!descriptor.has_numeric ||
         assignment.value < descriptor.valid_min || assignment.value > descriptor.valid_max))) return false;
  }
  return true;
}

void initialize_parameter_definitions(
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    definitions[i + 1] = g_params[i].raw;
    if (g_params[i].type == 0)
      std::memset(definitions[i + 1].data() + 56, 0, kEffectWorldSize);
    else if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(definitions[i + 1], 56, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 2)
      write<int32_t>(definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_value * 65536.0)));
    else if (g_params[i].type == 10)
      write<double>(definitions[i + 1], 56, g_params[i].default_value);
    else if (g_params[i].type == 3)
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(std::round(g_params[i].default_components[0] * 65536.0)));
    else if (g_params[i].type == 6) {
      write<int32_t>(definitions[i + 1], 56, static_cast<int32_t>(std::round(g_params[i].default_components[0] * 65536.0)));
      write<int32_t>(definitions[i + 1], 60, static_cast<int32_t>(std::round(g_params[i].default_components[1] * 65536.0)));
    } else if (g_params[i].type == 18)
      for (int component = 0; component < 3; ++component)
        write<double>(definitions[i + 1], 56 + component * 8, g_params[i].default_components[component]);
    else if (g_params[i].type == 12) {
      const auto masks = ordered_active_masks();
      const int32_t index = static_cast<int32_t>(g_params[i].default_value);
      write<int32_t>(definitions[i + 1], 56,
          index > 0 && static_cast<std::size_t>(index) <= masks.size()
              ? masks[static_cast<std::size_t>(index - 1)]->id : 0);
    }
  }
}

bool initialize_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (g_params[i].type != 11) continue;
    const std::size_t u = 56;
    const int16_t id = read<int16_t>(definitions[i + 1], u);
    void* source = read<void*>(definitions[i + 1], u + 8);
    void* refcon = read<void*>(definitions[i + 1], u + 24);
    void* destination = nullptr;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 2);
    write<int16_t>(extra, 4, id);
    write<void*>(extra, 8, refcon);
    write<void*>(extra, 16, source);
    write<void*>(extra, 24, &destination);
    const int32_t error = source
        ? entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr, extra.data())
        : 4;
    if (error != 0 || !destination || destination == source) {
      ++g_arbitrary_print_failures;
      return false;
    }
    write<void*>(definitions[i + 1], u + 16, destination);
    ++g_arbitrary_copy_calls;
  }
  return true;
}

bool apply_arbitrary_text_assignments(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions,
    const RequestedAssignments& requested) {
  for (const auto& assignment : requested) {
    if (assignment.kind != RequestedKind::ArbitraryText) continue;
    constexpr std::size_t u = 56;
    auto& definition = definitions[static_cast<std::size_t>(assignment.index)];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    void* previous = read<void*>(definition, u + 16);
    void* scanned = nullptr;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 10);
    write<int16_t>(extra, 4, id);
    write<void*>(extra, 8, refcon);
    write<const char*>(extra, 16, assignment.text.data());
    write<uint32_t>(extra, 24, static_cast<uint32_t>(assignment.text.size()));
    write<void*>(extra, 32, &scanned);
    uint32_t exception_code = 0;
    const bool scan_error = invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(),
        nullptr, nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !host_handle_is_live(scanned) || scanned == previous;
    auto dispose_scanned = [&]() {
      if (!host_handle_is_live(scanned) || scanned == previous) return true;
      std::array<std::byte, 48> scanned_dispose_extra{};
      write<int32_t>(scanned_dispose_extra, 0, 1);
      write<int16_t>(scanned_dispose_extra, 4, id);
      write<void*>(scanned_dispose_extra, 8, refcon);
      write<void*>(scanned_dispose_extra, 16, scanned);
      if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
                scanned_dispose_extra.data()) != 0) {
        ++g_invalid_arbitrary_operations;
        return false;
      }
      ++g_arbitrary_dispose_calls;
      return true;
    };
    if (scan_error) {
      dispose_scanned();
      ++g_arbitrary_scan_failures;
      return false;
    }
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, 0, 1);
    write<int16_t>(dispose_extra, 4, id);
    write<void*>(dispose_extra, 8, refcon);
    write<void*>(dispose_extra, 16, previous);
    if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data()) != 0) {
      dispose_scanned();
      ++g_invalid_arbitrary_operations;
      return false;
    }
    ++g_arbitrary_dispose_calls;
    ++g_arbitrary_scan_calls;
    write<void*>(definition, u + 16, scanned);
  }
  return true;
}

bool dispose_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  bool valid = true;
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (g_params[i].type != 11) continue;
    const std::size_t u = 56;
    void* value = read<void*>(definitions[i + 1], u + 16);
    if (!value) continue;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 1);
    write<int16_t>(extra, 4, read<int16_t>(definitions[i + 1], u));
    write<void*>(extra, 8, read<void*>(definitions[i + 1], u + 24));
    write<void*>(extra, 16, value);
    const int32_t error = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, extra.data());
    write<void*>(definitions[i + 1], u + 16, nullptr);
    if (error == 0) ++g_arbitrary_dispose_calls;
    else { ++g_invalid_arbitrary_operations; valid = false; }
  }
  return valid;
}

bool dispose_arbitrary_defaults(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output) {
  bool valid = true;
  for (auto& param : g_params) {
    if (param.type != 11) continue;
    const std::size_t u = 56;
    void* value = read<void*>(param.raw, u + 8);
    if (!value) continue;
    std::array<std::byte, 48> extra{};
    write<int32_t>(extra, 0, 1);
    write<int16_t>(extra, 4, read<int16_t>(param.raw, u));
    write<void*>(extra, 8, read<void*>(param.raw, u + 24));
    write<void*>(extra, 16, value);
    const int32_t error = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, extra.data());
    write<void*>(param.raw, u + 8, nullptr);
    if (error == 0) ++g_arbitrary_dispose_calls;
    else { ++g_invalid_arbitrary_operations; valid = false; }
  }
  return valid;
}

bool apply_arbitrary_parameter_animation(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions,
    int32_t time, uint32_t scale) {
  if (scale == 0) return false;
  for (const auto &timeline : g_parameter_timelines) {
    if (timeline.keys.front().kind != AnimationValueKind::Arbitrary) continue;
    if (timeline.slot <= 0 || static_cast<std::size_t>(timeline.slot) >= definitions.size() ||
        g_params[timeline.slot - 1].type != 11 ||
        std::any_of(timeline.keys.begin(), timeline.keys.end(), [](const auto &key) {
          return key.kind != AnimationValueKind::Arbitrary || key.arbitrary.empty() ||
                 key.arbitrary.size() > 64 * 1024;
        })) return false;
    auto &definition = definitions[timeline.slot];
    constexpr std::size_t u = 56;
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    const auto dispose = [&](void* value) {
      if (!host_handle_is_live(value)) return false;
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, 0, 1); write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon); write<void*>(extra, 16, value);
      uint32_t exception_code = 0;
      const bool ok = invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(),
          nullptr, nullptr, extra.data(), &exception_code) == 0 && exception_code == 0;
      if (ok) ++g_arbitrary_dispose_calls; else ++g_invalid_arbitrary_operations;
      return ok;
    };
    std::vector<void*> owned;
    const auto cleanup = [&]() {
      bool ok = true;
      for (void* value : owned) if (value) ok = dispose(value) && ok;
      owned.clear();
      return ok;
    };
    for (const auto &key : timeline.keys) {
      void* value = nullptr;
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, 0, 5); write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon);
      write<uint32_t>(extra, 16, static_cast<uint32_t>(key.arbitrary.size()));
      write<const unsigned char*>(extra, 24, key.arbitrary.data());
      write<void*>(extra, 32, &value);
      uint32_t exception_code = 0;
      if (invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
          nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
          !host_handle_is_live(value) ||
          std::find(owned.begin(), owned.end(), value) != owned.end()) {
        if (host_handle_is_live(value) && std::find(owned.begin(), owned.end(), value) == owned.end())
          dispose(value);
        cleanup();
        return false;
      }
      owned.push_back(value);
    }
    std::size_t selected = 0;
    void* replacement = nullptr;
    if (!rational_less(timeline.keys.front().time, timeline.keys.front().scale, time, scale)) {
      replacement = owned.front();
    } else if (!rational_less(time, scale, timeline.keys.back().time, timeline.keys.back().scale)) {
      selected = timeline.keys.size() - 1; replacement = owned.back();
    } else {
      std::size_t right = 1;
      while (!rational_less(time, scale, timeline.keys[right].time, timeline.keys[right].scale)) ++right;
      selected = right - 1;
      const auto &left_key = timeline.keys[selected];
      if (left_key.hold) replacement = owned[selected];
      else {
        const long double now = static_cast<long double>(time) / scale;
        const long double left = static_cast<long double>(left_key.time) / left_key.scale;
        const long double right_time = static_cast<long double>(timeline.keys[right].time) /
                                       timeline.keys[right].scale;
        const double amount = static_cast<double>((now - left) / (right_time - left));
        g_last_arbitrary_interpolation_amount = amount;
        // AE supplies an owned NEW value for INTERP to fill. Some SDK samples
        // (including ColorGrid) require this even though the callback contract
        // also permits replacing the output handle.
        void* preallocated = nullptr;
        std::array<std::byte, 48> new_extra{};
        write<int32_t>(new_extra, 0, 0); write<int16_t>(new_extra, 4, id);
        write<void*>(new_extra, 8, refcon); write<void*>(new_extra, 16, &preallocated);
        uint32_t exception_code = 0;
        if (invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
            nullptr, new_extra.data(), &exception_code) != 0 || exception_code != 0 ||
            !host_handle_is_live(preallocated) ||
            std::find(owned.begin(), owned.end(), preallocated) != owned.end()) {
          if (host_handle_is_live(preallocated) &&
              std::find(owned.begin(), owned.end(), preallocated) == owned.end())
            dispose(preallocated);
          cleanup();
          return false;
        }
        ++g_arbitrary_new_calls;
        replacement = preallocated;
        std::array<std::byte, 48> extra{};
        write<int32_t>(extra, 0, 6); write<int16_t>(extra, 4, id);
        write<void*>(extra, 8, refcon); write<void*>(extra, 16, owned[selected]);
        write<void*>(extra, 24, owned[right]); write<double>(extra, 32, amount);
        write<void*>(extra, 40, &replacement);
        exception_code = 0;
        if (invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
            nullptr, extra.data(), &exception_code) != 0 || exception_code != 0 ||
            !host_handle_is_live(replacement) ||
            std::find(owned.begin(), owned.end(), replacement) != owned.end()) {
          if (replacement != preallocated && host_handle_is_live(replacement) &&
              std::find(owned.begin(), owned.end(), replacement) == owned.end()) dispose(replacement);
          if (host_handle_is_live(preallocated)) dispose(preallocated);
          cleanup();
          return false;
        }
        if (replacement != preallocated && host_handle_is_live(preallocated) &&
            !dispose(preallocated)) {
          dispose(replacement);
          cleanup();
          return false;
        }
        ++g_arbitrary_interpolation_calls;
      }
    }
    void* previous = read<void*>(definition, u + 16);
    if (!dispose(previous)) {
      if (std::find(owned.begin(), owned.end(), replacement) == owned.end()) dispose(replacement);
      cleanup();
      return false;
    }
    write<void*>(definition, u + 16, replacement);
    const auto transferred = std::find(owned.begin(), owned.end(), replacement);
    if (transferred != owned.end()) *transferred = nullptr;
    if (!cleanup()) return false;
  }
  return true;
}

void observe_arbitrary_defaults(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output) {
  constexpr uint32_t kMaxArbitraryPrintBytes = 64 * 1024;
  constexpr std::size_t kMaxSummaryBytes = 4096;
  for (auto& param : g_params) {
    if (param.type != 11) continue;
    const std::size_t u = 56;
    void* value = read<void*>(param.raw, u + 8);
    void* refcon = read<void*>(param.raw, u + 24);
    uint32_t print_size = 0;
    std::array<std::byte, 48> size_extra{};
    write<int32_t>(size_extra, 0, 8);
    write<int16_t>(size_extra, 4, read<int16_t>(param.raw, u));
    write<void*>(size_extra, 8, refcon);
    write<void*>(size_extra, 16, value);
    write<void*>(size_extra, 24, &print_size);
    uint32_t exception_code = 0;
    if (!value || invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(),
                        nullptr, nullptr, size_extra.data(), &exception_code) != 0 ||
        exception_code != 0 || print_size == 0 ||
        print_size > kMaxArbitraryPrintBytes) {
      ++g_arbitrary_print_failures;
      continue;
    }
    std::vector<char> buffer(static_cast<std::size_t>(print_size) + 1, '\0');
    std::array<std::byte, 48> print_extra{};
    write<int32_t>(print_extra, 0, 9);
    write<int16_t>(print_extra, 4, read<int16_t>(param.raw, u));
    write<void*>(print_extra, 8, refcon);
    write<int32_t>(print_extra, 16, 0);
    write<void*>(print_extra, 24, value);
    write<uint32_t>(print_extra, 32, print_size);
    write<void*>(print_extra, 40, buffer.data());
    if (invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
              nullptr, print_extra.data(), &exception_code) != 0 || exception_code != 0) {
      ++g_arbitrary_print_failures;
      continue;
    }
    const auto terminator = std::find(buffer.begin(), buffer.begin() + print_size, '\0');
    if (terminator == buffer.begin() + print_size) {
      ++g_arbitrary_print_failures;
      continue;
    }
    const auto length = std::min<std::size_t>(
        static_cast<std::size_t>(terminator - buffer.begin()), kMaxSummaryBytes);
    param.arbitrary_summary.assign(buffer.data(), length);
    ++g_arbitrary_print_calls;
  }
}

struct ArbitraryValuesScope {
  EffectEntry entry{};
  std::array<std::byte, kInSize>* input{};
  std::array<std::byte, kOutSize>* output{};
  std::vector<std::array<std::byte, kParamSize>>* definitions{};
  ~ArbitraryValuesScope() {
    if (entry && input && output && definitions)
      dispose_arbitrary_values(entry, *input, *output, *definitions);
  }
};

void probe_arbitrary_scan(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (g_params[i].type != 11 || g_params[i].arbitrary_summary.empty()) continue;
    constexpr std::size_t u = 56;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* source = read<void*>(definition, u + 16);
    void* refcon = read<void*>(definition, u + 24);
    void* scanned = nullptr;
    std::array<std::byte, 48> scan_extra{};
    write<int32_t>(scan_extra, 0, 10);
    write<int16_t>(scan_extra, 4, id);
    write<void*>(scan_extra, 8, refcon);
    write<const char*>(scan_extra, 16, g_params[i].arbitrary_summary.c_str());
    write<uint32_t>(scan_extra, 24,
        static_cast<uint32_t>(g_params[i].arbitrary_summary.size()));
    write<void*>(scan_extra, 32, &scanned);
    uint32_t exception_code = 0;
    const bool created = invoke_entry_seh(entry, kArbitraryCallback, input.data(),
        output.data(), nullptr, nullptr, scan_extra.data(), &exception_code) == 0 &&
        exception_code == 0 && host_handle_is_live(scanned) && scanned != source;
    if (!created) {
      if (host_handle_is_live(scanned)) {
        std::array<std::byte, 48> dispose_extra{};
        write<int32_t>(dispose_extra, 0, 1);
        write<int16_t>(dispose_extra, 4, id);
        write<void*>(dispose_extra, 8, refcon);
        write<void*>(dispose_extra, 16, scanned);
        entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data());
        ++g_arbitrary_dispose_calls;
      }
      ++g_arbitrary_scan_failures;
      continue;
    }
    int32_t comparison = 3;
    std::array<std::byte, 48> compare_extra{};
    write<int32_t>(compare_extra, 0, 7);
    write<int16_t>(compare_extra, 4, id);
    write<void*>(compare_extra, 8, refcon);
    write<void*>(compare_extra, 16, source);
    write<void*>(compare_extra, 24, scanned);
    write<void*>(compare_extra, 32, &comparison);
    exception_code = 0;
    const bool equal = invoke_entry_seh(entry, kArbitraryCallback, input.data(),
        output.data(), nullptr, nullptr, compare_extra.data(), &exception_code) == 0 &&
        exception_code == 0 && comparison == 0;
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, 0, 1);
    write<int16_t>(dispose_extra, 4, id);
    write<void*>(dispose_extra, 8, refcon);
    write<void*>(dispose_extra, 16, scanned);
    const bool disposed = entry(kArbitraryCallback, input.data(), output.data(),
                                nullptr, nullptr, dispose_extra.data()) == 0;
    if (disposed) ++g_arbitrary_dispose_calls;
    if (equal && disposed) ++g_arbitrary_scan_calls;
    else ++g_arbitrary_scan_failures;
  }
}

bool interpolate_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  const int32_t current_time = read<int32_t>(input, kInCurrentTime);
  const int32_t total_time = read<int32_t>(input, 232);
  const double amount = total_time > 0
      ? std::clamp(static_cast<double>(current_time) / total_time, 0.0, 1.0)
      : 0.0;
  g_last_arbitrary_interpolation_amount = amount;
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (g_params[i].type != 11) continue;
    const bool timeline_controls_slot = std::any_of(
        g_parameter_timelines.begin(), g_parameter_timelines.end(),
        [i](const auto& timeline) {
          return timeline.slot == static_cast<int32_t>(i + 1) &&
              !timeline.keys.empty() &&
              timeline.keys.front().kind == AnimationValueKind::Arbitrary;
        });
    if (timeline_controls_slot) continue;
    const std::size_t u = 56;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    void* source = read<void*>(definition, u + 16);
    if (!host_handle_is_live(source)) {
      ++g_arbitrary_interpolation_failures;
      return false;
    }
    const auto dispose = [&](void* value) {
      if (!host_handle_is_live(value)) return false;
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, 0, 1);
      write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon);
      write<void*>(extra, 16, value);
      const bool ok = entry(kArbitraryCallback, input.data(), output.data(), nullptr,
                            nullptr, extra.data()) == 0;
      if (ok) ++g_arbitrary_dispose_calls;
      return ok;
    };
    void* created = nullptr;
    std::array<std::byte, 48> new_extra{};
    write<int32_t>(new_extra, 0, 0);
    write<int16_t>(new_extra, 4, id);
    write<void*>(new_extra, 8, refcon);
    write<void*>(new_extra, 16, &created);
    uint32_t exception_code = 0;
    if (invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, new_extra.data(), &exception_code) == 0 && exception_code == 0 &&
        host_handle_is_live(created) && created != source) {
      ++g_arbitrary_new_calls;
    } else if (host_handle_is_live(created)) {
      dispose(created);
      ++g_arbitrary_interpolation_failures;
      continue;
    } else {
      ++g_arbitrary_interpolation_failures;
      continue;
    }
    void* interpolated = created;
    std::array<std::byte, 48> interp_extra{};
    write<int32_t>(interp_extra, 0, 6);
    write<int16_t>(interp_extra, 4, id);
    write<void*>(interp_extra, 8, refcon);
    write<void*>(interp_extra, 16, source);
    write<void*>(interp_extra, 24, source);
    write<double>(interp_extra, 32, amount);
    write<void*>(interp_extra, 40, &interpolated);
    exception_code = 0;
    if (invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, interp_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !host_handle_is_live(interpolated) || interpolated == source) {
      if (interpolated != created && host_handle_is_live(interpolated)) dispose(interpolated);
      if (host_handle_is_live(created)) dispose(created);
      ++g_arbitrary_interpolation_failures;
      continue;
    }
    if (interpolated != created && host_handle_is_live(created) && !dispose(created)) {
      dispose(interpolated);
      ++g_invalid_arbitrary_operations;
      return false;
    }
    if (!dispose(source)) {
      dispose(interpolated);
      ++g_invalid_arbitrary_operations;
      return false;
    }
    write<void*>(definition, u + 16, interpolated);
    ++g_arbitrary_interpolation_calls;
  }
  return true;
}

bool roundtrip_arbitrary_values(EffectEntry entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output,
    std::vector<std::array<std::byte, kParamSize>>& definitions) {
  constexpr uint32_t kMaxFlatBytes = 16 * 1024 * 1024;
  constexpr std::size_t kGuardBytes = 32;
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    if (g_params[i].type != 11) continue;
    const std::size_t u = 56;
    auto& definition = definitions[i + 1];
    const int16_t id = read<int16_t>(definition, u);
    void* refcon = read<void*>(definition, u + 24);
    void* source = read<void*>(definition, u + 16);
    uint32_t exception_code = 0;
    uint32_t flat_size = 0;
    std::array<std::byte, 48> size_extra{};
    write<int32_t>(size_extra, 0, 3);
    write<int16_t>(size_extra, 4, id);
    write<void*>(size_extra, 8, refcon);
    write<void*>(size_extra, 16, source);
    write<void*>(size_extra, 24, &flat_size);
    if (!host_handle_is_live(source) || invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(),
        nullptr, nullptr, size_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        flat_size == 0 || flat_size > kMaxFlatBytes) {
      ++g_arbitrary_roundtrip_failures;
      continue;
    }
    const auto flatten = [&](void* value, std::vector<std::byte>& guarded) {
      guarded.assign(static_cast<std::size_t>(flat_size) + kGuardBytes * 2, std::byte{0xA5});
      auto* buffer = guarded.data() + kGuardBytes;
      std::fill(buffer, buffer + flat_size, std::byte{});
      std::array<std::byte, 48> extra{};
      write<int32_t>(extra, 0, 4);
      write<int16_t>(extra, 4, id);
      write<void*>(extra, 8, refcon);
      write<void*>(extra, 16, value);
      write<uint32_t>(extra, 24, flat_size);
      write<void*>(extra, 32, buffer);
      exception_code = 0;
      const int32_t error = invoke_entry_seh(entry, kArbitraryCallback, input.data(),
          output.data(), nullptr, nullptr, extra.data(), &exception_code);
      return error == 0 && exception_code == 0 &&
          std::all_of(guarded.begin(), guarded.begin() + kGuardBytes,
              [](std::byte value) { return value == std::byte{0xA5}; }) &&
          std::all_of(guarded.end() - kGuardBytes, guarded.end(),
              [](std::byte value) { return value == std::byte{0xA5}; });
    };
    std::vector<std::byte> original_flat;
    if (!flatten(source, original_flat)) {
      ++g_arbitrary_roundtrip_failures;
      continue;
    }
    void* restored = nullptr;
    std::array<std::byte, 48> unflatten_extra{};
    write<int32_t>(unflatten_extra, 0, 5);
    write<int16_t>(unflatten_extra, 4, id);
    write<void*>(unflatten_extra, 8, refcon);
    write<uint32_t>(unflatten_extra, 16, flat_size);
    write<void*>(unflatten_extra, 24, original_flat.data() + kGuardBytes);
    write<void*>(unflatten_extra, 32, &restored);
    exception_code = 0;
    const int32_t unflatten_error = invoke_entry_seh(entry, kArbitraryCallback, input.data(), output.data(), nullptr,
        nullptr, unflatten_extra.data(), &exception_code) != 0 || exception_code != 0 ||
        !host_handle_is_live(restored) || restored == source;
    if (unflatten_error) {
      if (host_handle_is_live(restored)) {
        std::array<std::byte, 48> dispose_extra{};
        write<int32_t>(dispose_extra, 0, 1);
        write<int16_t>(dispose_extra, 4, id);
        write<void*>(dispose_extra, 8, refcon);
        write<void*>(dispose_extra, 16, restored);
        entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data());
        ++g_arbitrary_dispose_calls;
      }
      ++g_arbitrary_roundtrip_failures;
      continue;
    }
    std::vector<std::byte> restored_flat;
    const bool flattened = flatten(restored, restored_flat);
    const bool bytes_equal = flattened && std::equal(
        original_flat.begin() + kGuardBytes, original_flat.begin() + kGuardBytes + flat_size,
        restored_flat.begin() + kGuardBytes);
    if (!bytes_equal) {
      std::array<std::byte, 48> dispose_extra{};
      write<int32_t>(dispose_extra, 0, 1);
      write<int16_t>(dispose_extra, 4, id);
      write<void*>(dispose_extra, 8, refcon);
      write<void*>(dispose_extra, 16, restored);
      entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr, dispose_extra.data());
      ++g_arbitrary_dispose_calls;
      ++g_arbitrary_roundtrip_failures;
      continue;
    }
    std::array<std::byte, 48> dispose_extra{};
    write<int32_t>(dispose_extra, 0, 1);
    write<int16_t>(dispose_extra, 4, id);
    write<void*>(dispose_extra, 8, refcon);
    write<void*>(dispose_extra, 16, source);
    if (entry(kArbitraryCallback, input.data(), output.data(), nullptr, nullptr,
              dispose_extra.data()) != 0) {
      ++g_invalid_arbitrary_operations;
      return false;
    }
    ++g_arbitrary_dispose_calls;
    write<void*>(definition, u + 16, restored);
    ++g_arbitrary_roundtrip_calls;
  }
  return true;
}

bool apply_requested_assignments(
    std::vector<std::array<std::byte, kParamSize>>& definitions,
    const RequestedAssignments& requested) {
  if (!validate_requested_assignments(requested)) return false;
  for (const auto& assignment : requested) {
    const auto slot = static_cast<std::size_t>(assignment.index);
    const auto type = g_params[slot - 1].type;
    if (type == 11 && assignment.kind == RequestedKind::ArbitraryText) continue;
    if (type == 1 || type == 4 || type == 7)
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(assignment.value));
    else if (type == 12) {
      const int32_t index = static_cast<int32_t>(assignment.value);
      const auto masks = ordered_active_masks();
      write<int32_t>(definitions[slot], 56, index == 0 ? 0 : masks[index - 1]->id);
    }
    else if (type == 2) {
      const double fixed = assignment.value * 65536.0;
      if (!std::isfinite(fixed) || fixed < INT32_MIN || fixed > INT32_MAX) return false;
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(std::round(fixed)));
    } else if (type == 10)
      write<double>(definitions[slot], 56, assignment.value);
    else if (type == 5) {
      std::memcpy(definitions[slot].data() + 56, assignment.color.data(), assignment.color.size());
      g_params[slot - 1].current_color = assignment.color;
      for (std::size_t channel = 0; channel < 4; ++channel)
        g_params[slot - 1].current_float_color[channel] = assignment.color[channel] / 255.0f;
    }
    else if (type == 3)
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(std::round(assignment.components[0] * 65536.0)));
    else if (type == 6) {
      write<int32_t>(definitions[slot], 56, static_cast<int32_t>(std::round(assignment.components[0] * 65536.0)));
      write<int32_t>(definitions[slot], 60, static_cast<int32_t>(std::round(assignment.components[1] * 65536.0)));
    } else if (type == 18)
      for (int component = 0; component < 3; ++component)
        write<double>(definitions[slot], 56 + component * 8, assignment.components[component]);
    else
      return false;
  }
  return true;
}

double requested_value(const RequestedAssignments& requested, const wchar_t* id) {
  const auto found = std::find_if(requested.begin(), requested.end(), [id](const auto& assignment) {
    return assignment.id == id;
  });
  return found == requested.end() || found->kind == RequestedKind::Color ? 0.0 : found->value;
}

std::string requested_parameters_json(const RequestedAssignments& requested) {
  std::ostringstream output;
  output << "[";
  for (std::size_t i = 0; i < requested.size(); ++i) {
    if (i != 0) output << ",";
    const auto& assignment = requested[i];
    std::string id;
    id.reserve(assignment.id.size());
    for (const wchar_t character : assignment.id) id.push_back(static_cast<char>(character));
    output << "{\"id\":\"" << id << "\",\"slot\":" << assignment.index
           << ",\"kind\":\""
           << (assignment.kind == RequestedKind::Integer ? "integer" :
               assignment.kind == RequestedKind::Float ? "float" :
               assignment.kind == RequestedKind::Color ? "color" :
               assignment.kind == RequestedKind::Angle ? "angle" :
               assignment.kind == RequestedKind::Point ? "point" :
               assignment.kind == RequestedKind::Point3D ? "point3d" : "arbitrary_text")
           << "\",\"value\":";
    if (assignment.kind == RequestedKind::Integer)
      output << static_cast<int32_t>(assignment.value);
    else if (assignment.kind == RequestedKind::Float)
      output << std::setprecision(17) << assignment.value;
    else if (assignment.kind == RequestedKind::Color)
      output << "{\"alpha\":" << static_cast<unsigned>(assignment.color[0])
             << ",\"red\":" << static_cast<unsigned>(assignment.color[1])
             << ",\"green\":" << static_cast<unsigned>(assignment.color[2])
             << ",\"blue\":" << static_cast<unsigned>(assignment.color[3]) << "}";
    else if (assignment.kind == RequestedKind::ArbitraryText)
      output << "{\"bytes\":" << assignment.text.size() << "}";
    else {
      const int count = assignment.kind == RequestedKind::Point3D ? 3 :
          (assignment.kind == RequestedKind::Point ? 2 : 1);
      output << "[";
      for (int component = 0; component < count; ++component) {
        if (component) output << ",";
        output << std::setprecision(17) << assignment.components[component];
      }
      output << "]";
    }
    output << "}";
  }
  output << "]";
  return output.str();
}
// Write one packed-ARGB world as raw RGBA in the byte layout that
// tools/compare-pixel-oracles.py consumes (rgba8 / rgba16le / rgba32f-le,
// row-major, no stride padding). Little-endian is the only supported target.
void dump_world_snapshot(const std::string& stage, const unsigned char* packed_argb,
                         int32_t width, int32_t height, int32_t pixel_bytes) {
  aexcompat::render::RenderTelemetry telemetry{
      &g_dump_worlds_dir, &g_world_dumps_written, &g_world_dumps_skipped,
      &g_world_dump_bytes, g_output_checksum_detail, &g_output_row_crc32,
      &g_output_channel_sha256, {&argb_to_rgba_native, &sha256_bytes}};
  aexcompat::render::dump_world_snapshot(telemetry, stage, packed_argb, width,
                                          height, pixel_bytes);
}

// Record per-row CRC32 and per-channel SHA-256 of the RGBA-ordered output
// transport bytes, so a differing region can be narrowed to rows and channels
// without shipping any pixel content in the report.
void record_output_checksum_detail(const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes) {
  aexcompat::render::RenderTelemetry telemetry{
      &g_dump_worlds_dir, &g_world_dumps_written, &g_world_dumps_skipped,
      &g_world_dump_bytes, g_output_checksum_detail, &g_output_row_crc32,
      &g_output_channel_sha256, {&argb_to_rgba_native, &sha256_bytes}};
  aexcompat::render::record_output_checksum_detail(telemetry, rgba, width,
                                                    height, pixel_bytes);
}

// Single insertion point for both image report emitters: dump counters are
// always present, checksum detail only when the opt-in trailer enabled it.
std::string world_debug_report_json() {
  const aexcompat::render::RenderTelemetry telemetry{
      &g_dump_worlds_dir, &g_world_dumps_written, &g_world_dumps_skipped,
      &g_world_dump_bytes, g_output_checksum_detail, &g_output_row_crc32,
      &g_output_channel_sha256, {&argb_to_rgba_native, &sha256_bytes}};
  return aexcompat::render::world_debug_report_json(telemetry);
}

struct LifecycleContext { EffectEntry effect_entry; };

constexpr aexcompat::render_lifecycle::Layout kRenderLifecycleLayout{
    kInSequenceData, kOutSequenceData, kInFrameData, kOutFrameData,
    kSequenceSetup, kSequenceSetdown, kFrameSetup, kFrameSetdown};

int32_t lifecycle_invoke_frame(void* opaque, int32_t selector, void* input,
                               void* output, void** params, void* world) {
  const EffectEntry effect_entry = static_cast<LifecycleContext*>(opaque)->effect_entry;
  return guarded_effect_call(effect_entry, selector, input, output, params, world, nullptr);
}

int32_t lifecycle_invoke_sequence(void* opaque, int32_t selector, void* input,
                                  void* output) {
  return invoke_sequence_selector(
      static_cast<LifecycleContext*>(opaque)->effect_entry, selector, input, output);
}

void lifecycle_activate_aux(void*) {
  g_external_aux_active = g_external_aux_loaded;
}

void lifecycle_cleanup_aux(void*) {
  // Aux channel chunks are host-owned and cannot outlive a render lifecycle.
  reclaim_layer_channels();
  g_external_aux_active = false;
  clear_native_aux_provider();
}

aexcompat::render_lifecycle::Hooks lifecycle_hooks(LifecycleContext& context) {
  return {&context, &lifecycle_invoke_frame, &lifecycle_invoke_sequence,
          &lifecycle_activate_aux, &lifecycle_cleanup_aux};
}

RenderLifecycle begin_frame_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::begin_frame(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world);
}

int32_t end_frame_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::end_frame(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world, lifecycle, primary_error);
}

RenderLifecycle begin_render_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::begin_render(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world);
}

int32_t end_render_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::end_render(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world, lifecycle, primary_error);
}
int32_t classic_render_runtime(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& command_output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes,
                    std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    const std::filesystem::path* external_output = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4,
                    bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr) {
  auto* classic_context = aexcompat::worker_runtime::classic::active_context();
  if (!classic_context) return -1;
  aexcompat::render::ImageRequest image_request;
  const int request_error = aexcompat::render::prepare_image_request(
      case_id, external_rgba != nullptr, external_width, external_height,
      external_pixel_bytes, image_request);
  if (request_error != 0) return request_error;
  const bool connected_map = image_request.connected_map;
  const bool partial_extent_hint = image_request.partial_extent_hint;
  width = image_request.width;
  height = image_request.height;
  const int32_t pixel_bytes = image_request.pixel_bytes;
  smart_state().pixel_format = pixel_bytes == 16 ? "argb32f" :
      (pixel_bytes == 8 ? "argb16" : "argb8");
  rowbytes = image_request.rowbytes;
  const aexcompat::render::ParameterProfile parameter_profile =
      aexcompat::render::prepare_parameter_profile(case_id);
  std::vector<unsigned char> logical_source(width * height * pixel_bytes);
  InputPixelBuffer source(static_cast<std::size_t>(rowbytes) * height);
  if (!source) return -3;
  std::memset(source.data(), 0x5A, static_cast<std::size_t>(rowbytes) * height);
  if (!aexcompat::render::build_argb_input(image_request, external_rgba,
                                            logical_source, source.data())) return -3;
  dump_world_snapshot("classic-input", logical_source.data(), width, height, pixel_bytes);
  const bool input_write_advertised =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagIWriteInputBuffer) != 0;
  if (!source.set_plugin_writable(input_write_advertised)) return -3;
  OutputPixelBuffer guarded(static_cast<std::size_t>(rowbytes) * height);
  if (!guarded) return -3;
  unsigned char* destination = guarded.data();
  guards_intact = true;

  std::array<std::byte, 120> input_world{}, output_world{};
  const aexcompat::render::WorldLayout primary_world{
      pixel_bytes == 4 ? 0 : 1, pixel_bytes, width, height, rowbytes};
  if (!aexcompat::render::prepare_world_layout(input_world, primary_world, source.data()) ||
      !aexcompat::render::prepare_world_layout(output_world, primary_world, destination)) return -3;
  const int32_t dispatch_pixel_format = pixel_bytes == 4 ? kPixelFormatArgb32 :
      (pixel_bytes == 8 ? kPixelFormatArgb64 : kPixelFormatArgb128);
  DispatchWorldFormatScope dispatch_worlds;
  if (!dispatch_worlds.register_world(input_world.data(), dispatch_pixel_format) ||
      !dispatch_worlds.register_world(output_world.data(), dispatch_pixel_format)) return -3;

  aexcompat::render::MapWorld map_world;
  if (connected_map) {
    if (!aexcompat::render::prepare_connected_map_world(case_id, width, height, map_world) ||
        !dispatch_worlds.register_world(map_world.world.data(), kPixelFormatArgb32)) return -3;
    aexcompat::worker_runtime::classic::ParameterDefinition checkout_definition{};
    write<int32_t>(checkout_definition, 12, 0);
    std::memcpy(checkout_definition.data() + 56, map_world.world.data(), map_world.world.size());
    classic_context->set_fallback_definition(g_secondary_layer_slot,
                                             checkout_definition);
  }

  std::vector<std::array<std::byte, kParamSize>> definitions(g_params.size() + 1);
  std::vector<std::vector<unsigned char>> hosted_pixels;
  std::vector<std::array<std::byte, 120>> hosted_worlds;
  if (external_layers) {
    hosted_pixels.resize(external_layers->size());
    hosted_worlds.resize(external_layers->size());
  }
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (!initialize_arbitrary_values(entry, input, command_output, definitions)) return -5;
  ArbitraryValuesScope arbitrary_scope{entry, &input, &command_output, &definitions};
  if (requested && !apply_arbitrary_text_assignments(entry, input, command_output, definitions, *requested)) return -5;
  probe_arbitrary_scan(entry, input, command_output, definitions);
  for (std::size_t slot = 1; slot < definitions.size(); ++slot)
    if (g_params[slot - 1].type == 0 && g_params[slot - 1].layer_default == -1)
      std::memcpy(definitions[slot].data() + 56, input_world.data(), input_world.size());
  if (external_layers) for (std::size_t layer_index = 0; layer_index < external_layers->size(); ++layer_index) {
    const auto& layer = (*external_layers)[layer_index];
    if (layer.slot <= 0 || static_cast<std::size_t>(layer.slot) >= definitions.size() ||
        g_params[layer.slot - 1].type != 0 || layer.rgba.size() !=
            static_cast<std::size_t>(layer.width) * layer.height * 4) return -3;
    auto& pixels = hosted_pixels[layer_index];
      pixels.resize(static_cast<std::size_t>(layer.width) * layer.height * pixel_bytes);
      for (std::size_t offset = 0; offset < layer.rgba.size(); offset += 4) {
      rgba8_to_argb(pixels.data() + (offset / 4) * pixel_bytes,
                    layer.rgba.data() + offset, pixel_bytes);
    }
    dump_world_snapshot("classic-layer-slot" + std::to_string(layer.slot),
                        pixels.data(), layer.width, layer.height, pixel_bytes);
    auto& world = hosted_worlds[layer_index];
    if (!aexcompat::render::prepare_world_layout(
            world, {pixel_bytes == 4 ? 0 : 1, pixel_bytes, layer.width, layer.height,
                    layer.width * pixel_bytes}, pixels.data()) ||
        !dispatch_worlds.register_world(world.data(), dispatch_pixel_format)) return -3;
    if (!layer.timed || same_rational_time(layer.time, layer.time_scale,
            external_current_time, external_time_scale))
      std::memcpy(definitions[layer.slot].data() + 56, world.data(), world.size());
    std::array<std::byte, kParamSize> checkout{};
    write<int32_t>(checkout, 12, 0);
    std::memcpy(checkout.data() + 56, world.data(), world.size());
    if (layer.timed) {
      if (!classic_context->add_timed_layer(
              {layer.slot, layer.time, layer.time_scale, checkout})) return -3;
    }
  }
  if (requested) {
    if (!apply_requested_assignments(definitions, *requested)) return -3;
  } else if (definitions.size() > 7) {
    write<int32_t>(definitions[1], 56, parameter_profile.amount);
    write<int32_t>(definitions[2], 56, parameter_profile.direction);
    write<int32_t>(definitions[3], 56, parameter_profile.seed);
    write<int32_t>(definitions[4], 56, parameter_profile.repeat);
    write<double>(definitions[5], 56, parameter_profile.mix);
    if (parameter_profile.inverted_map) write<int32_t>(definitions[7], 56, 1);
  }
  if (!apply_parameter_animation(definitions, external_current_time, external_time_scale)) return -3;
  if (!apply_arbitrary_parameter_animation(entry, input, command_output, definitions,
                                            external_current_time, external_time_scale)) return -3;
  for (std::size_t slot = 0; slot < definitions.size(); ++slot)
    classic_context->set_definition(static_cast<int32_t>(slot), definitions[slot]);
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  write<int32_t>(input, 224, external_current_time);
  write<int32_t>(input, 228, external_time_step);
  write<int32_t>(input, 232, external_total_time);
  write<int32_t>(input, 236, external_time_step);
  write<uint32_t>(input, 240, external_time_scale);
  write<int32_t>(input, 252, g_full_resolution_width > 0 ? g_full_resolution_width : width);
  write<int32_t>(input, 256, g_full_resolution_height > 0 ? g_full_resolution_height : height);
  const int32_t full_extent[4] = {0, 0, width, height};
  std::memcpy(input.data() + 260, full_extent, sizeof(full_extent));
  if (partial_extent_hint) {
    const int32_t extent[4] = {3, 2, 11, 8};
    std::memcpy(input.data() + 260, extent, sizeof(extent));
  }
  struct RenderUiContextScope {
    EffectEntry entry;
    std::array<std::byte, kInSize>& input;
    std::array<std::byte, kOutSize>& output;
    std::vector<std::array<std::byte, kParamSize>>& definitions;
    ~RenderUiContextScope() {
      if (g_render_ui_context_active)
        close_render_ui_context(entry, input, output, definitions);
    }
  } render_ui_context_scope{entry, input, command_output, definitions};
  publish_alpha_coverage_provider(logical_source, width, height, pixel_bytes,
                                  external_current_time, external_time_scale);
  const RenderLifecycle lifecycle = manage_sequence
      ? begin_render_lifecycle(entry, input, command_output, params.data(), output_world.data())
      : begin_frame_lifecycle(entry, input, command_output, params.data(), output_world.data());
  int32_t error = lifecycle.setup_error;
  if (error == 0 && !dispatch_render_click(entry, input, command_output, definitions)) error = -5;
  if (error == 0 && !interpolate_arbitrary_values(entry, input, command_output, definitions)) error = -5;
  if (error == 0 && !roundtrip_arbitrary_values(entry, input, command_output, definitions)) error = -5;
  if (error == 0 && !dispatch_conditional_ui_selectors(entry, input, command_output, params.data())) error = -5;
  const uint32_t effective_out_flags = read<uint32_t>(command_output, kOutFlags);
  const uint32_t effective_out_flags2 = read<uint32_t>(command_output, kOutFlags2);
  const bool classic_wide_time_allowed =
      (effective_out_flags & kOutFlagWideTimeInput) != 0 ||
      ((effective_out_flags2 & kOutFlag2AutomaticWideTimeInput) != 0 &&
       (effective_out_flags2 & kOutFlag2SupportsSmartRender) == 0);
  const bool classic_shutter_dependency_advertised =
      (effective_out_flags & kOutFlagIUseShutterAngle) != 0;
  classic_context->configure_checkout_time(
      read<int32_t>(input, kInCurrentTime), read<uint32_t>(input, kInTimeScale),
      classic_wide_time_allowed, classic_shutter_dependency_advertised);
  input_hash = sha256_bytes(logical_source.data(), logical_source.size());
  const bool nop_render =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagNopRender) != 0;
  if (nop_render) {
    if (error == 0) {
      for (int32_t y = 0; y < height; ++y)
        std::memcpy(destination + y * rowbytes,
                    logical_source.data() + y * width * pixel_bytes,
                    width * pixel_bytes);
    }
    error = manage_sequence
        ? end_render_lifecycle(entry, input, command_output, params.data(), output_world.data(), lifecycle, error)
        : end_frame_lifecycle(entry, input, command_output, params.data(), output_world.data(), lifecycle, error);
  } else {
    if (error == 0 && !dispatch_render_draw(entry, input, command_output, definitions))
      error = -5;
    if (error == 0) {
      const int32_t requested_width = read<int32_t>(command_output, kOutWidth);
      const int32_t requested_height = read<int32_t>(command_output, kOutHeight);
      if (!aexcompat::render::validate_output_extent(
              width, height, requested_width, requested_height,
              read<uint32_t>(command_output, kOutFlags))) {
        error = 4;
      } else if (requested_width > 0 && requested_height > 0) {
        {
          width = requested_width;
          height = requested_height;
          rowbytes = width * pixel_bytes;
          if (!guarded.reset(static_cast<std::size_t>(rowbytes) * height)) return -3;
          destination = guarded.data();
          if (!aexcompat::render::prepare_world_layout(
                  output_world, {pixel_bytes == 4 ? 0 : 1, pixel_bytes, width, height, rowbytes},
                  destination)) return -3;
          if (!dispatch_worlds.register_world(output_world.data(), dispatch_pixel_format))
            error = 4;
          write<int32_t>(input, 276, read<int32_t>(command_output, kOutOrigin));
          write<int32_t>(input, 280, read<int32_t>(command_output, kOutOrigin + 4));
        }
      }
    }
    if (error == 0) {
      struct LoadedEffectReceiptContextScope {
        LoadedEffectReceiptContext previous;
        explicit LoadedEffectReceiptContextScope(LoadedEffectReceiptContext context)
            : previous(std::move(g_loaded_effect_receipt_context)) {
          g_loaded_effect_receipt_context = std::move(context);
        }
        ~LoadedEffectReceiptContextScope() {
          g_loaded_effect_receipt_context = std::move(previous);
        }
      } receipt_context_scope({entry, &input, &command_output, external_current_time,
          static_cast<int32_t>(external_time_scale), case_id, requested, external_rgba,
          external_layers, external_width, external_height, external_time_step,
          external_total_time, pixel_bytes, &logical_source, width, height});
      classic_context->mark_selector_dispatched();
      error = entry(kRender, input.data(), command_output.data(), params.data(),
                    output_world.data(), nullptr);
    }
    if (g_render_ui_context_active &&
        !close_render_ui_context(entry, input, command_output, definitions) && error == 0)
      error = -5;
    error = manage_sequence
        ? end_render_lifecycle(entry, input, command_output, params.data(),
                               output_world.data(), lifecycle, error)
        : end_frame_lifecycle(entry, input, command_output, params.data(),
                              output_world.data(), lifecycle, error);
  }
  std::vector<unsigned char> logical_output;
  if (!aexcompat::render::copy_packed_world(destination, rowbytes, width, height,
                                             pixel_bytes, logical_output)) return -3;
  output_hash = sha256_bytes(logical_output.data(), logical_output.size());
  if (error == 0 && !publish_staged_item_world(aegp_comp_item_handle(),
          {external_current_time, external_time_scale},
          {external_time_step, external_time_scale},
          static_cast<int8_t>(read<int32_t>(input, kInQuality) == 0 ? 0 : 1), 0,
          dispatch_pixel_format, width, height, width * pixel_bytes,
          logical_output.data())) {
    error = 4;
  }
  if (captured_argb) *captured_argb = logical_output;
  dump_world_snapshot("classic-output", logical_output.data(), width, height, pixel_bytes);
  if (external_output && error == 0) {
    std::vector<unsigned char> rgba(static_cast<std::size_t>(width) * height * pixel_bytes);
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel)
      argb_to_rgba_native(rgba.data() + pixel * pixel_bytes,
                    logical_output.data() + pixel * pixel_bytes, pixel_bytes);
    record_output_checksum_detail(rgba.data(), width, height, pixel_bytes);
    std::ofstream file(*external_output, std::ios::binary | std::ios::out);
    if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size())) return -4;
  }
  const bool padding_intact = rowbytes == width * 4 || [&] {
    for (int32_t y = 0; y < height; ++y)
      for (int32_t x = width * pixel_bytes; x < rowbytes; ++x)
        if (destination[y * rowbytes + x] != 0xCC) return false;
    return true;
  }();
  guards_intact = padding_intact && guarded.sentinels_intact();
  smart_state().pixel_format = pixel_bytes == 16 ? "argb32f" :
      (pixel_bytes == 8 ? "argb16" : "argb8");
  if (!close_render_ui_context(entry, input, command_output, definitions)) return -5;
  return error;
}

// The request keeps render-local state out of wmain.  The shared subsystem
// controls admission and failure priority; this hook retains the audited host
// implementation that prepares PF worlds, params, suites, and lifecycle data.
struct ClassicRenderRequest {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  const std::string& case_id;
  int32_t& width;
  int32_t& height;
  int32_t& rowbytes;
  std::string& input_hash;
  std::string& output_hash;
  bool& guards_intact;
  const RequestedAssignments* requested;
  const std::vector<unsigned char>* external_rgba;
  const std::filesystem::path* external_output;
  int32_t external_width;
  int32_t external_height;
  const std::vector<ExternalLayerInput>* external_layers;
  int32_t external_current_time;
  int32_t external_time_step;
  int32_t external_total_time;
  uint32_t external_time_scale;
  int32_t external_pixel_bytes;
  bool manage_sequence;
  std::vector<unsigned char>* captured_argb;
};

bool classic_render_dependencies_ready(void* opaque) {
  const auto& request = *static_cast<ClassicRenderRequest*>(opaque);
  return request.entry && request.width >= 0 && request.height >= 0 &&
      request.external_time_scale != 0;
}

int classic_render_guarded_effect_main(void* opaque) {
  auto& request = *static_cast<ClassicRenderRequest*>(opaque);
  return classic_render_runtime(request.entry, request.input, request.output, request.case_id,
      request.width, request.height, request.rowbytes, request.input_hash, request.output_hash,
      request.guards_intact, request.requested, request.external_rgba, request.external_output,
      request.external_width, request.external_height, request.external_layers,
      request.external_current_time, request.external_time_step, request.external_total_time,
      request.external_time_scale, request.external_pixel_bytes, request.manage_sequence,
      request.captured_argb);
}

int classic_render_cleanup(void*) {
  // classic_render_runtime performs sequence/frame/UI/world cleanup before it
  // returns.  This explicit hook documents the completed cleanup boundary.
  return 0;
}

int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes, std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    const std::filesystem::path* external_output = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4, bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr) {
  ClassicRenderRequest request{entry, input, output, case_id, width, height, rowbytes,
      input_hash, output_hash, guards_intact, requested, external_rgba, external_output,
      external_width, external_height, external_layers, external_current_time,
      external_time_step, external_total_time, external_time_scale, external_pixel_bytes,
      manage_sequence, captured_argb};
  aexcompat::worker_runtime::classic::Request context{
      &request,
      {&classic_render_guarded_effect_main, &classic_render_cleanup,
       &classic_render_dependencies_ready},
      g_module_audit.required};
  return aexcompat::worker_runtime::classic::dispatch(context);
}

bool exercise_loaded_effect_item_receipt(EffectEntry entry,
    std::array<std::byte, kInSize>& input, std::array<std::byte, kOutSize>& output) {
  g_loaded_effect_receipt_context = {
      entry, &input, &output, read<int32_t>(input, kInCurrentTime),
      static_cast<int32_t>(read<uint32_t>(input, kInTimeScale))};
  std::vector<uint8_t> final_stage(16 * 12 * 4);
  for (std::size_t pixel = 0; pixel < final_stage.size() / 4; ++pixel) {
    final_stage[pixel * 4] = static_cast<uint8_t>(64 + pixel % 191);
    final_stage[pixel * 4 + 1] = static_cast<uint8_t>(pixel * 17);
    final_stage[pixel * 4 + 2] = static_cast<uint8_t>(pixel * 29);
    final_stage[pixel * 4 + 3] = static_cast<uint8_t>(pixel * 43);
  }
  const bool stage_published = publish_staged_item_world(aegp_comp_item_handle(),
      {read<int32_t>(input, kInCurrentTime),
       read<uint32_t>(input, kInTimeScale)},
      {1, 30}, 1, 0, kPixelFormatArgb32, 16, 12, 16 * 4, final_stage.data());
  void* options = nullptr;
  void* receipt = nullptr;
  void** world = nullptr;
  void* pixels = nullptr;
  int32_t width = 0, height = 0, type = 0;
  const bool checked_out = render_options_new_from_item(1, aegp_comp_item_handle(), &options) == 0 &&
      checkout_item_frame_async(&g_async_manager, 1, options, &receipt) == 0 && receipt &&
      get_receipt_world(receipt, &world) == 0 && world &&
      aegp_world_get_type(world, &type) == 0 && type == 1 &&
      aegp_world_get_size(world, &width, &height) == 0 && width == 16 && height == 12 &&
      aegp_world_get_base_addr8(world, &pixels) == 0 && pixels;
  if (options) {
    render_options_set_world_type(options, 2);
    void* unsupported = reinterpret_cast<void*>(1);
    g_loaded_effect_receipt_unsupported_rejected =
        checkout_item_frame_async(&g_async_manager, 1, options, &unsupported) != 0 &&
        unsupported == nullptr;
  }
  const bool checked_in = receipt && checkin_frame(receipt) == 0;
  if (world) {
    int32_t stale_type = 0;
    g_loaded_effect_receipt_stale_world_rejected =
        aegp_world_get_type(world, &stale_type) != 0;
  }
  const bool options_disposed = options && render_options_dispose(options) == 0;
  g_loaded_effect_receipt_context = {};
  g_loaded_effect_receipt_fixture_passed = stage_published && checked_out && checked_in && options_disposed &&
      g_loaded_effect_receipt_unsupported_rejected &&
      g_loaded_effect_receipt_stale_world_rejected &&
      async_receipt_lifetimes_balanced() && render_options_lifetimes_balanced();
  return g_loaded_effect_receipt_fixture_passed;
}
struct SmartResult {
  std::shared_ptr<const aexcompat::worker_runtime::smart::Snapshot> runtime{
      std::make_shared<aexcompat::worker_runtime::smart::Snapshot>()};
  int32_t gpu_setup_error{};
  int32_t pre_error{-1};
  int32_t selector_error{-1};
  int32_t render_error{-1};
  int32_t gpu_setdown_error{};
  uint32_t gpu_setdown_exception_code{};
  std::string input_hash;
  std::string output_hash;
  bool rects_valid{};
  bool guards_intact{};
  bool output_pixels_valid{};
  bool gpu_render_possible{};
  bool gpu_render_dispatched{};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  bool roi_contract_valid{};
  std::array<int32_t, 4> result_rect{};
  std::array<int32_t, 4> max_result_rect{};
  int32_t output_width{};
  int32_t output_height{};
  int32_t output_rowbytes{};
};

SmartResult smart_render_runtime(EffectEntry entry, std::array<std::byte, kInSize>& input,
                              std::array<std::byte, kOutSize>& command_output,
                              const std::string& case_id,
                              const RequestedAssignments* requested = nullptr,
                              const std::vector<unsigned char>* external_rgba = nullptr,
                              const std::filesystem::path* external_output = nullptr,
                              int32_t external_width = 0, int32_t external_height = 0,
                              const std::vector<ExternalLayerInput>* external_layers = nullptr,
                              int32_t external_current_time = 0, int32_t external_time_step = 1,
                              int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                              int32_t external_pixel_bytes = 4) {
  SmartRuntimeSession smart_session;
  reset_smart_host_telemetry();
  SmartResult result;
  result.runtime = smart_session.snapshot();
  const uint32_t effective_out_flags = read<uint32_t>(command_output, kOutFlags);
  const uint32_t effective_out_flags2 = read<uint32_t>(command_output, kOutFlags2);
  smart_state().wide_time_checkout_allowed =
      (effective_out_flags & kOutFlagWideTimeInput) != 0 ||
      (effective_out_flags2 & kOutFlag2AutomaticWideTimeInput) != 0;
  smart_state().current_time = external_current_time;
  smart_state().current_time_scale = external_time_scale;
  smart_state().rejected_temporal_checkouts = 0;
  smart_state().secondary_layer_slot = g_secondary_layer_slot;
  smart_state().full_resolution_width = g_full_resolution_width;
  smart_state().full_resolution_height = g_full_resolution_height;
  smart_state().pixel_aspect_numerator = g_pixel_aspect_ratio.numerator;
  smart_state().pixel_aspect_denominator = g_pixel_aspect_ratio.denominator;
  const bool deep16 = case_id == "deep16_default" || (external_rgba && external_pixel_bytes == 8);
  const bool fixture_gpu_negotiation = case_id == "gpu_fallback_float32";
  const bool opencl_gpu_negotiation = case_id == "gpu_opencl_float32";
  const bool directx_gpu_negotiation = case_id == "gpu_directx_float32";
  uint32_t gpu_device_index = 0;
  const std::string gpu_device_prefix = "gpu_device_";
  const bool explicit_gpu_device = case_id.rfind(gpu_device_prefix, 0) == 0;
  if (explicit_gpu_device) {
    const std::string ordinal = case_id.substr(gpu_device_prefix.size());
    if (ordinal.empty() || ordinal.size() > 2 ||
        !std::all_of(ordinal.begin(), ordinal.end(), [](unsigned char ch) {
          return ch >= '0' && ch <= '9';
        })) return result;
    gpu_device_index = static_cast<uint32_t>(std::stoul(ordinal));
    if (gpu_device_index >= kMaxCudaDevices) return result;
  }
  const bool force_cpu_image = case_id == "request_cpu";
  const bool advertised_gpu_support =
      (read<uint32_t>(command_output, kOutFlags2) & (1u << 25)) != 0;
  const bool gpu_negotiation = fixture_gpu_negotiation || opencl_gpu_negotiation ||
      directx_gpu_negotiation ||
      explicit_gpu_device ||
      (external_rgba && external_pixel_bytes == 16 && advertised_gpu_support &&
       !force_cpu_image);
  const bool missing_input = case_id == "error_missing_input";
  const bool crash_null_output = case_id == "crash_null_output_world";
  const bool temporal_context = case_id == "temporal_context";
  const bool partial_output_request = case_id == "partial_output_request";
  const bool float32 = case_id == "float32_default" || gpu_negotiation ||
      (external_rgba && external_pixel_bytes == 16);
  const bool connected_map = case_id == "connected_map" || case_id == "inverted_map";
  const int32_t width = external_rgba ? external_width :
      (connected_map ? 11 : ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 13 : 16));
  const int32_t height = external_rgba ? external_height :
      (connected_map ? 7 : ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 9 : 12));
  if (width <= 0 || height <= 0 || width > 4096 || height > 4096) return result;
  const int32_t pixel_bytes = float32 ? 16 : (deep16 ? 8 : 4);
  const int32_t rowbytes = case_id == "padded_stride" ? 64 : width * pixel_bytes;
  const aexcompat::render::ParameterProfile parameter_profile =
      aexcompat::render::prepare_parameter_profile(case_id);
  if (case_id != "default" && case_id != "request" && !deep16 && !float32 && !missing_input && !crash_null_output && !temporal_context && !partial_output_request && !connected_map) return result;
  InputPixelBuffer source(static_cast<std::size_t>(rowbytes) * height);
  if (!source) return result;
  std::memset(source.data(), 0x5A, static_cast<std::size_t>(rowbytes) * height);
  if (external_rgba && external_rgba->size() != static_cast<std::size_t>(width) * height * 4) return result;
  for (int32_t y = 0; y < height; ++y) for (int32_t x = 0; x < width; ++x) {
    auto* pixel = &source[y * rowbytes + x * pixel_bytes];
    if (external_rgba) {
      const auto* rgba = &(*external_rgba)[(y * width + x) * 4];
      rgba8_to_argb(pixel, rgba, pixel_bytes);
    } else if (float32) {
      const float values[4] = {1.0f, static_cast<float>(x) / static_cast<float>(width - 1),
          static_cast<float>(y) / static_cast<float>(height - 1),
          static_cast<float>(x + y) / static_cast<float>(width + height - 2)};
      std::memcpy(pixel, values, sizeof(values));
    } else if (deep16) {
      const uint16_t values[4] = {32768, static_cast<uint16_t>(x * 32768 / (width - 1)),
          static_cast<uint16_t>(y * 32768 / (height - 1)),
          static_cast<uint16_t>((x + y) * 32768 / (width + height - 2))};
      std::memcpy(pixel, values, sizeof(values));
    } else {
      pixel[0] = 255; pixel[1] = static_cast<unsigned char>(x * 255 / (width - 1));
      pixel[2] = static_cast<unsigned char>(y * 255 / (height - 1));
      pixel[3] = static_cast<unsigned char>((x + y) * 255 / (width + height - 2));
    }
  }
  const bool input_write_advertised =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagIWriteInputBuffer) != 0;
  if (!source.set_plugin_writable(input_write_advertised)) return result;
  OutputPixelBuffer guarded(static_cast<std::size_t>(rowbytes) * height);
  if (!guarded) return result;
  auto* destination = guarded.data();
  result.guards_intact = true;
  std::array<std::byte, 120> input_world{}, output_world{};
  const aexcompat::render::WorldLayout primary_world{
      (deep16 || float32) ? 1 : 0, pixel_bytes, width, height, rowbytes};
  if (!aexcompat::render::prepare_world_layout(input_world, primary_world, source.data()) ||
      !aexcompat::render::prepare_world_layout(output_world, primary_world, destination)) return result;
  const int32_t dispatch_pixel_format = float32 ? kPixelFormatArgb128 :
      (deep16 ? kPixelFormatArgb64 : kPixelFormatArgb32);
  DispatchWorldFormatScope dispatch_worlds;
  if (!dispatch_worlds.register_world(input_world.data(), dispatch_pixel_format) ||
      !dispatch_worlds.register_world(output_world.data(), dispatch_pixel_format)) return result;

  aexcompat::render::MapWorld map_world;
  if (connected_map) {
    if (!aexcompat::render::prepare_connected_map_world(case_id, width, height, map_world) ||
        !dispatch_worlds.register_world(map_world.world.data(), kPixelFormatArgb32)) return result;
    smart_state().map_width = map_world.width;
    smart_state().map_height = map_world.height;
    smart_state().map_world = map_world.world.data();
  }

  std::vector<std::array<std::byte, kParamSize>> definitions(g_params.size() + 1);
  std::vector<std::vector<unsigned char>> hosted_pixels;
  std::vector<std::array<std::byte, 120>> hosted_worlds;
  if (external_layers) {
    hosted_pixels.resize(external_layers->size());
    hosted_worlds.resize(external_layers->size());
  }
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (!initialize_arbitrary_values(entry, input, command_output, definitions)) return result;
  ArbitraryValuesScope arbitrary_scope{entry, &input, &command_output, &definitions};
  if (requested && !apply_arbitrary_text_assignments(entry, input, command_output, definitions, *requested)) return result;
  probe_arbitrary_scan(entry, input, command_output, definitions);
  for (std::size_t slot = 1; slot < definitions.size(); ++slot)
    if (g_params[slot - 1].type == 0 && g_params[slot - 1].layer_default == -1)
      std::memcpy(definitions[slot].data() + 56, input_world.data(), input_world.size());
  smart_state().hosted_layers.clear();
  if (external_layers) for (std::size_t layer_index = 0; layer_index < external_layers->size(); ++layer_index) {
    const auto& layer = (*external_layers)[layer_index];
    if (layer.slot <= 0 || static_cast<std::size_t>(layer.slot) >= definitions.size() ||
        g_params[layer.slot - 1].type != 0 || layer.rgba.size() !=
            static_cast<std::size_t>(layer.width) * layer.height * 4) return result;
    auto& pixels = hosted_pixels[layer_index];
    pixels.resize(static_cast<std::size_t>(layer.width) * layer.height * pixel_bytes);
    for (std::size_t offset = 0; offset < layer.rgba.size(); offset += 4) {
      rgba8_to_argb(pixels.data() + (offset / 4) * pixel_bytes,
                    layer.rgba.data() + offset, pixel_bytes);
    }
    dump_world_snapshot("smart-layer-slot" + std::to_string(layer.slot),
                        pixels.data(), layer.width, layer.height, pixel_bytes);
    auto& world = hosted_worlds[layer_index];
    if (!aexcompat::render::prepare_world_layout(
            world, {pixel_bytes == 4 ? 0 : 1, pixel_bytes, layer.width, layer.height,
                    layer.width * pixel_bytes}, pixels.data()) ||
        !dispatch_worlds.register_world(world.data(), dispatch_pixel_format)) return result;
    const int32_t requested_time = temporal_context ? 42 : external_current_time;
    const uint32_t requested_scale = temporal_context ? 24 : external_time_scale;
    if (!layer.timed || same_rational_time(layer.time, layer.time_scale,
            requested_time, requested_scale))
      std::memcpy(definitions[layer.slot].data() + 56, world.data(), world.size());
    smart_state().hosted_layers.push_back({layer.slot, layer.time, layer.time_scale, layer.timed,
        layer.width, layer.height, -1, world.data()});
  }
  if (requested) {
    if (!apply_requested_assignments(definitions, *requested)) return result;
  } else if (definitions.size() > 7) {
    write<int32_t>(definitions[1], 56, parameter_profile.amount);
    write<int32_t>(definitions[2], 56, parameter_profile.direction);
    write<int32_t>(definitions[3], 56, parameter_profile.seed);
    write<int32_t>(definitions[4], 56, parameter_profile.repeat);
    write<double>(definitions[5], 56, parameter_profile.mix);
    if (parameter_profile.inverted_map) write<int32_t>(definitions[7], 56, 1);
  }
  const int32_t animation_time = temporal_context ? 42 : external_current_time;
  const uint32_t animation_scale = temporal_context ? 24 : external_time_scale;
  if (!apply_parameter_animation(definitions, animation_time, animation_scale)) return result;
  if (!apply_arbitrary_parameter_animation(entry, input, command_output, definitions,
                                            animation_time, animation_scale)) return result;
  g_checkout_layer_definitions.clear();
  for (std::size_t slot = 0; slot < definitions.size(); ++slot)
    g_checkout_layer_definitions.emplace(static_cast<int32_t>(slot), definitions[slot]);
  struct CheckoutDefinitionsScope {
    ~CheckoutDefinitionsScope() { g_checkout_layer_definitions.clear(); }
  } checkout_definitions_scope;
  {
    std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
    g_live_param_checkouts.clear();
    g_param_checkout_calls = g_param_checkin_calls = g_invalid_param_checkins = 0;
    g_automatic_param_checkins = 0;
    g_last_param_checkout_index = -1;
    g_last_param_checkout_time = g_last_param_checkout_time_step = 0;
    g_last_param_checkout_time_scale = 0;
  }
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  const int32_t current_time = temporal_context ? 42 : external_current_time;
  const int32_t time_step = temporal_context ? 2 : external_time_step;
  const uint32_t time_scale = temporal_context ? 24 : external_time_scale;
  write<int32_t>(input, 224, current_time); write<int32_t>(input, 228, time_step);
  write<int32_t>(input, 232, temporal_context ? 240 : external_total_time);
  write<int32_t>(input, 236, time_step);
  write<uint32_t>(input, 240, time_scale);
  write<int32_t>(input, 252, g_full_resolution_width > 0 ? g_full_resolution_width : width);
  write<int32_t>(input, 256, g_full_resolution_height > 0 ? g_full_resolution_height : height);
  const int32_t full_extent[4] = {0, 0, width, height};
  std::memcpy(input.data() + 260, full_extent, sizeof(full_extent));
  if (partial_output_request) {
    const int32_t extent[4] = {3, 2, 11, 8};
    std::memcpy(input.data() + 260, extent, sizeof(extent));
  }
  struct SmartRenderUiContextScope {
    EffectEntry entry;
    std::array<std::byte, kInSize>& input;
    std::array<std::byte, kOutSize>& output;
    std::vector<std::array<std::byte, kParamSize>>& definitions;
    ~SmartRenderUiContextScope() {
      if (g_render_ui_context_active)
        close_render_ui_context(entry, input, output, definitions);
    }
  } render_ui_context_scope{entry, input, command_output, definitions};
  smart_state().checkout_time = 0;
  smart_state().checkout_time_step = 0;
  smart_state().checkout_time_scale = 0;
  std::vector<unsigned char> pre_render_source(
      static_cast<std::size_t>(width) * height * pixel_bytes);
  for (int32_t row = 0; row < height; ++row)
    std::memcpy(pre_render_source.data() + static_cast<std::size_t>(row) * width * pixel_bytes,
                source.data() + static_cast<std::size_t>(row) * rowbytes,
                static_cast<std::size_t>(width) * pixel_bytes);
  dump_world_snapshot("smart-input", pre_render_source.data(), width, height, pixel_bytes);
  // This immutable provider is published before Smart Pre-Render and remains pinned
  // through Smart Render; output pixels are never used to infer auxiliary planes.
  publish_alpha_coverage_provider(pre_render_source, width, height, pixel_bytes,
                                  external_current_time, external_time_scale);
  const RenderLifecycle lifecycle = begin_render_lifecycle(
      entry, input, command_output, params.data(), output_world.data());
  if (lifecycle.setup_error != 0) {
    result.pre_error = lifecycle.setup_error;
    result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                                output_world.data(), lifecycle,
                                                lifecycle.setup_error);
    return result;
  }
  if (!dispatch_render_click(entry, input, command_output, definitions)) {
    result.pre_error = -5;
    result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                                output_world.data(), lifecycle, -5);
    return result;
  }
  if (!interpolate_arbitrary_values(entry, input, command_output, definitions) ||
      !roundtrip_arbitrary_values(entry, input, command_output, definitions)) {
    result.pre_error = -5;
    result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                                output_world.data(), lifecycle, -5);
    return result;
  }
  if (!dispatch_conditional_ui_selectors(entry, input, command_output, params.data())) {
    result.pre_error = -5;
    result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                                output_world.data(), lifecycle, -5);
    return result;
  }
  const uint32_t dynamic_out_flags = read<uint32_t>(command_output, kOutFlags);
  const uint32_t dynamic_out_flags2 = read<uint32_t>(command_output, kOutFlags2);
  smart_state().wide_time_checkout_allowed =
      (dynamic_out_flags & kOutFlagWideTimeInput) != 0 ||
      (dynamic_out_flags2 & kOutFlag2AutomaticWideTimeInput) != 0;
  smart_state().shutter_dependency_advertised =
      (dynamic_out_flags & kOutFlagIUseShutterAngle) != 0;
  const bool nop_render =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagNopRender) != 0;
  if (nop_render) {
    for (int32_t y = 0; y < height; ++y)
      std::memcpy(destination + y * rowbytes, source.data() + y * rowbytes,
                  width * pixel_bytes);
    result.pre_error = 0;
    result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                                output_world.data(), lifecycle, 0);
    result.rects_valid = true;
    result.roi_contract_valid = true;
    result.output_width = width;
    result.output_height = height;
    result.output_rowbytes = rowbytes;
    result.result_rect = {0, 0, width, height};
    result.max_result_rect = result.result_rect;
    std::vector<unsigned char> logical_input(width * height * pixel_bytes);
    std::vector<unsigned char> logical_output(width * height * pixel_bytes);
    for (int32_t y = 0; y < height; ++y) {
      std::memcpy(logical_input.data() + y * width * pixel_bytes,
                  source.data() + y * rowbytes, width * pixel_bytes);
      std::memcpy(logical_output.data() + y * width * pixel_bytes,
                  destination + y * rowbytes, width * pixel_bytes);
    }
    result.input_hash = sha256_bytes(logical_input.data(), logical_input.size());
    result.output_hash = sha256_bytes(logical_output.data(), logical_output.size());
    dump_world_snapshot("smart-output", logical_output.data(), width, height, pixel_bytes);
    if (external_output && result.render_error == 0) {
      std::vector<unsigned char> rgba(static_cast<std::size_t>(width) * height * pixel_bytes);
      for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(width) * height; ++pixel)
        argb_to_rgba_native(rgba.data() + pixel * pixel_bytes,
                      logical_output.data() + pixel * pixel_bytes, pixel_bytes);
      record_output_checksum_detail(rgba.data(), width, height, pixel_bytes);
      std::ofstream file(*external_output, std::ios::binary | std::ios::out);
      if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size()))
        result.render_error = -4;
    }
    result.guards_intact = guarded.sentinels_intact();
    if (g_render_ui_context_active &&
        !close_render_ui_context(entry, input, command_output, definitions) &&
        result.render_error == 0)
      result.render_error = -5;
    return result;
  }
  if (crash_null_output)
    entry(kFrameSetup, input.data(), command_output.data(), params.data(), nullptr, nullptr);

  if (!dispatch_render_draw(entry, input, command_output, definitions)) {
    result.pre_error = -5;
    if (g_render_ui_context_active)
      close_render_ui_context(entry, input, command_output, definitions);
    result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                                output_world.data(), lifecycle, -5);
    return result;
  }

  std::array<std::byte, 8> gpu_setup_input{}, gpu_setup_output{};
  std::array<std::byte, 16> gpu_setup_extra{};
  const int32_t gpu_framework = (fixture_gpu_negotiation || directx_gpu_negotiation) ? 4 :
      (opencl_gpu_negotiation ? 1 : 3);
  const bool use_cuda = gpu_negotiation && gpu_framework == 3;
  const bool use_opencl = gpu_negotiation && gpu_framework == 1;
  const bool use_directx = gpu_negotiation && gpu_framework == 4;
  if (gpu_negotiation) capture_module_audit();
  const bool gpu_context_started = gpu_transport::begin_backend_context(
      gpu_framework, gpu_device_index);
  if (gpu_negotiation) {
    write<int32_t>(gpu_setup_input, 0, gpu_framework);
    write<uint32_t>(gpu_setup_input, 4, gpu_device_index);
    write<void*>(gpu_setup_extra, 0, gpu_setup_input.data()); write<void*>(gpu_setup_extra, 8, gpu_setup_output.data());
    std::cerr << "stage:gpu_device_setup_begin\n" << std::flush;
    result.gpu_setup_error = gpu_context_started
        ? entry(kGpuDeviceSetup, input.data(), command_output.data(), params.data(),
                nullptr, gpu_setup_extra.data())
        : -6;
    std::cerr << "stage:gpu_device_setup_end error=" << result.gpu_setup_error << "\n" << std::flush;
  }

  std::array<std::byte, 64> pre_input{}; std::array<std::byte, 56> pre_output{};
  std::array<std::byte, 16> pre_callbacks{}; std::array<std::byte, 24> pre_extra{};
  const std::array<int32_t, 4> expected_request = partial_output_request
      ? std::array<int32_t, 4>{3, 2, 11, 8}
      : std::array<int32_t, 4>{0, 0, width, height};
  std::memcpy(pre_input.data(), expected_request.data(), sizeof(expected_request));
  write<int16_t>(pre_input, 44, float32 ? 32 : (deep16 ? 16 : 8));
  if (gpu_negotiation) {
    write<void*>(pre_input, 48, read<void*>(gpu_setup_output, 0));
    write<int32_t>(pre_input, 56, gpu_framework);
    write<uint32_t>(pre_input, 60, gpu_device_index);
  }
  write<void*>(pre_callbacks, 0, reinterpret_cast<void*>(&pre_checkout_layer));
  write<void*>(pre_callbacks, 8, reinterpret_cast<void*>(&guid_mix_in_ptr));
  write<void*>(pre_extra, 0, pre_input.data()); write<void*>(pre_extra, 8, pre_output.data());
  write<void*>(pre_extra, 16, pre_callbacks.data());
  smart_state().input_checkout_request.fill(-1);
  smart_state().map_checkout_request.fill(-1);
  smart_state().secondary_checkout_id = -1;
  smart_state().width = width;
  smart_state().height = height;
  smart_state().rowbytes = rowbytes;
  smart_state().pixel_format = float32 ? "argb32f" :
      (deep16 ? "argb16" : "argb8");
  std::cerr << "stage:smart_pre_render_begin\n" << std::flush;
  result.pre_error = (!gpu_negotiation || result.gpu_setup_error == 0)
      ? entry(kSmartPreRender, input.data(), command_output.data(), params.data(),
              nullptr, pre_extra.data())
      : -1;
  std::cerr << "stage:smart_pre_render_end error=" << result.pre_error << "\n" << std::flush;
  automatic_checkin_pre_render_params();
  const aexcompat::render::SmartOutputBounds smart_bounds =
      aexcompat::render::prepare_smart_output_bounds(pre_output.data(), pre_output.size(),
                                                      pixel_bytes);
  const auto& result_rect = smart_bounds.result_rect;
  const auto& max_result_rect = smart_bounds.max_result_rect;
  result.result_rect = result_rect;
  result.max_result_rect = max_result_rect;
  result.rects_valid = result.pre_error == 0 && smart_bounds.valid;
  if (result.rects_valid) {
    if (!guarded.reset(static_cast<std::size_t>(smart_bounds.rowbytes) * smart_bounds.height)) {
      result.rects_valid = false;
      result.pre_error = -3;
    }
    destination = guarded.data();
    if (!aexcompat::render::prepare_world_layout(
            output_world, {(deep16 || float32) ? 1 : 0, pixel_bytes, smart_bounds.width,
                           smart_bounds.height, smart_bounds.rowbytes}, destination) ||
        !dispatch_worlds.register_world(output_world.data(), dispatch_pixel_format))
      result.rects_valid = false;
    write<int32_t>(input, 276, -max_result_rect[0]);
    write<int32_t>(input, 280, -max_result_rect[1]);
    result.output_width = smart_bounds.width;
    result.output_height = smart_bounds.height;
    result.output_rowbytes = smart_bounds.rowbytes;
  }
  result.roi_contract_valid = !partial_output_request ||
      (smart_state().input_checkout_request == expected_request &&
       smart_state().map_checkout_request == expected_request &&
       result_rect == expected_request && max_result_rect == expected_request);
  result.gpu_render_possible = (read<uint16_t>(pre_output, 34) & 0x2u) != 0;
  result.checkout_time = smart_state().checkout_time;
  result.checkout_time_step = smart_state().checkout_time_step;
  result.checkout_time_scale = smart_state().checkout_time_scale;

  std::array<std::byte, 72> smart_input{}; std::array<std::byte, 24> callbacks{};
  std::array<std::byte, 16> smart_extra{};
  // PF_PreRenderOutput::pre_render_data -> PF_SmartRenderInput::pre_render_data.
  write<void*>(smart_input, 48, read<void*>(pre_output, 40));
  if (gpu_negotiation) {
    write<void*>(smart_input, 56, read<void*>(gpu_setup_output, 0));
    write<int32_t>(smart_input, 64, gpu_framework);
    write<uint32_t>(smart_input, 68, gpu_device_index);
  }
  write<void*>(callbacks, 0, reinterpret_cast<void*>(&checkout_pixels));
  write<void*>(callbacks, 8, reinterpret_cast<void*>(&checkin_pixels));
  write<void*>(callbacks, 16, reinterpret_cast<void*>(&checkout_output));
  write<void*>(smart_extra, 0, smart_input.data()); write<void*>(smart_extra, 8, callbacks.data());
  smart_state().input_world = missing_input ? nullptr : input_world.data();
  smart_state().output_world = output_world.data();
  if (gpu_negotiation) {
    if ((!missing_input && !dispatch_worlds.register_world(input_world.data(), kPixelFormatGpuBgra128)) ||
        !dispatch_worlds.register_world(output_world.data(), kPixelFormatGpuBgra128))
      result.pre_error = 4;
  }
  const int32_t render_selector = gpu_negotiation && result.gpu_render_possible ? kSmartRenderGpu : kSmartRender;
  result.gpu_render_dispatched = render_selector == kSmartRenderGpu;
  CudaRenderTransport cuda_transport;
  const bool cuda_transport_ready = !result.gpu_render_dispatched ||
      (!use_cuda && !use_opencl && !use_directx) ||
      prepare_cuda_render_transport(smart_state().input_world,
                                    smart_state().output_world,
                                    cuda_transport);
  std::cerr << "stage:" << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_begin\n" << std::flush;
  if (result.pre_error == 0 && cuda_transport_ready) {
    if (gpu_negotiation) capture_module_audit();
    result.selector_error = entry(render_selector, input.data(), command_output.data(),
                                  params.data(), nullptr, smart_extra.data());
    result.render_error = result.selector_error;
  } else {
    result.render_error = result.pre_error == 0 ? -6 : -1;
  }
  if (result.gpu_render_dispatched && (use_cuda || use_opencl || use_directx) &&
      cuda_transport_ready &&
      !finish_cuda_render_transport(cuda_transport) && result.render_error == 0)
    result.render_error = -6;
  std::cerr << "stage:" << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_end error=" << result.render_error << "\n" << std::flush;
  if (gpu_negotiation && result.gpu_setup_error == 0) {
    std::array<std::byte, 16> setdown_input{}; std::array<std::byte, 8> setdown_extra{};
    write<void*>(setdown_input, 0, read<void*>(gpu_setup_output, 0));
    write<int32_t>(setdown_input, 8, gpu_framework);
    write<uint32_t>(setdown_input, 12, gpu_device_index);
    write<void*>(setdown_extra, 0, setdown_input.data());
    capture_module_audit();
    std::cerr << "stage:gpu_device_setdown_begin\n" << std::flush;
    result.gpu_setdown_error = invoke_entry_seh(
        entry, kGpuDeviceSetdown, input.data(), command_output.data(), params.data(),
        nullptr, setdown_extra.data(), &result.gpu_setdown_exception_code);
    std::cerr << "stage:gpu_device_setdown_end error=" << result.gpu_setdown_error << "\n" << std::flush;
  }
  if (gpu_negotiation) capture_module_audit();
  if (gpu_negotiation && gpu_context_started &&
      !gpu_transport::end_backend_context(gpu_framework) &&
      result.gpu_setdown_error == 0)
    result.gpu_setdown_error = -6;
  void* pre_render_data = read<void*>(pre_output, 40);
  if (auto delete_pre_render_data = read<void(__cdecl*)(void*)>(pre_output, 48)) {
    // A supplied callback owns cleanup even if it faults; host fallback would double-free.
    invoke_smart_pre_render_cleanup_seh(delete_pre_render_data, pre_render_data);
  } else if (pre_render_data) {
    const bool host_owned = host_handle_is_live(pre_render_data);
    if (host_owned) {
      dispose_handle(reinterpret_cast<void**>(pre_render_data));
      record_automatic_pre_render_disposal();
    }
  }
  if (g_render_ui_context_active &&
      !close_render_ui_context(entry, input, command_output, definitions) &&
      result.render_error == 0)
    result.render_error = -5;
  result.render_error = end_render_lifecycle(entry, input, command_output, params.data(),
                                              output_world.data(), lifecycle,
                                              result.render_error);
  smart_state().input_world = nullptr;
  smart_state().output_world = nullptr;
  smart_state().map_world = nullptr;
  smart_state().hosted_layers.clear();
  std::vector<unsigned char> logical_input;
  std::vector<unsigned char> logical_output;
  if (!aexcompat::render::copy_packed_world(source.data(), rowbytes, width, height,
                                             pixel_bytes, logical_input) ||
      !aexcompat::render::copy_packed_world(destination, result.output_rowbytes,
                                             result.output_width, result.output_height,
                                             pixel_bytes, logical_output)) return result;
  result.input_hash = sha256_bytes(logical_input.data(), logical_input.size());
  result.output_hash = sha256_bytes(logical_output.data(), logical_output.size());
  dump_world_snapshot("smart-output", logical_output.data(), result.output_width,
                      result.output_height, pixel_bytes);
  const bool output_untouched = !logical_output.empty() &&
      std::all_of(logical_output.begin(), logical_output.end(),
                  [](unsigned char byte) { return byte == 0xCC; });
  const bool output_finite = !float32 || aexcompat::render::finite_float_world(logical_output);
  result.output_pixels_valid = !logical_output.empty() && !output_untouched && output_finite;
  if (result.render_error == 0 && !result.output_pixels_valid) result.render_error = -6;
  if (external_output && result.render_error == 0) {
    std::vector<unsigned char> rgba(static_cast<std::size_t>(result.output_width) *
                                    result.output_height * pixel_bytes);
    for (std::size_t pixel = 0; pixel < static_cast<std::size_t>(result.output_width) *
                                      result.output_height; ++pixel)
      argb_to_rgba_native(rgba.data() + pixel * pixel_bytes,
                    logical_output.data() + pixel * pixel_bytes, pixel_bytes);
    record_output_checksum_detail(rgba.data(), result.output_width,
                                  result.output_height, pixel_bytes);
    std::ofstream file(*external_output, std::ios::binary | std::ios::out);
    if (!file || !file.write(reinterpret_cast<const char*>(rgba.data()), rgba.size())) result.render_error = -4;
  }
  result.guards_intact = guarded.sentinels_intact();
  return result;
}

struct SmartRenderRequest {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  const std::string& case_id;
  const RequestedAssignments* requested;
  const std::vector<unsigned char>* external_rgba;
  const std::filesystem::path* external_output;
  int32_t external_width;
  int32_t external_height;
  const std::vector<ExternalLayerInput>* external_layers;
  int32_t external_current_time;
  int32_t external_time_step;
  int32_t external_total_time;
  uint32_t external_time_scale;
  int32_t external_pixel_bytes;
  SmartResult result;
};

bool smart_render_dependencies_ready(void* opaque) {
  const auto& request = *static_cast<SmartRenderRequest*>(opaque);
  // SmartFX requires both a non-null entry point and valid rational time before
  // PF_Cmd_SMART_PRE_RENDER / CPU-or-GPU SMART_RENDER can be admitted.
  return request.entry && request.external_time_scale != 0 &&
      request.external_time_step > 0 && request.external_total_time >= request.external_current_time;
}

int smart_render_guarded_effect_main(void* opaque) {
  auto& request = *static_cast<SmartRenderRequest*>(opaque);
  request.result = smart_render_runtime(request.entry, request.input, request.output,
      request.case_id, request.requested, request.external_rgba, request.external_output,
      request.external_width, request.external_height, request.external_layers,
      request.external_current_time, request.external_time_step, request.external_total_time,
      request.external_time_scale, request.external_pixel_bytes);
  // GPU setup / pre-render / selector errors preserve their own precise field;
  // render_error is the lifecycle's primary result for shared error priority.
  if (request.result.gpu_setup_error != 0) return request.result.gpu_setup_error;
  if (request.result.pre_error != 0) return request.result.pre_error;
  return request.result.render_error;
}

int smart_render_cleanup(void*) {
  // smart_render_runtime performs GPU setdown, pre-render data cleanup,
  // automatic parameter checkins, suite releases, and world cleanup itself.
  return 0;
}

SmartResult smart_render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                              std::array<std::byte, kOutSize>& output,
                              const std::string& case_id,
                              const RequestedAssignments* requested = nullptr,
                              const std::vector<unsigned char>* external_rgba = nullptr,
                              const std::filesystem::path* external_output = nullptr,
                              int32_t external_width = 0, int32_t external_height = 0,
                              const std::vector<ExternalLayerInput>* external_layers = nullptr,
                              int32_t external_current_time = 0, int32_t external_time_step = 1,
                              int32_t external_total_time = 1,
                              uint32_t external_time_scale = 1,
                              int32_t external_pixel_bytes = 4) {
  SmartRenderRequest request{entry, input, output, case_id, requested, external_rgba,
      external_output, external_width, external_height, external_layers, external_current_time,
      external_time_step, external_total_time, external_time_scale, external_pixel_bytes, {}};
  aexcompat::render::RenderContext context{
      aexcompat::render::RenderKind::SmartPreRenderAndRender, &request,
      {&smart_render_guarded_effect_main, &smart_render_cleanup,
       &smart_render_dependencies_ready},
      g_module_audit.required};
  const int dispatch_error = aexcompat::render::dispatch(context);
  // Never mask the detailed native result. The generic result only supplies a
  // pre-admission failure when no selector executed.
  if (!context.selector_started && dispatch_error != 0) request.result.render_error = dispatch_error;
  return request.result;
}

void report(const char* status, int32_t global_error, int32_t params_error,
            int32_t setdown_error, const std::array<std::byte, kOutSize>& output,
            const std::string& about_message, const std::array<int32_t, 5>& lifecycle_errors,
            bool lifecycle_data_null) {
  restore_native_stdout();
  aexcompat::worker_report::L2ReportContext c;
  c.status = status ? status : ""; c.global_error = global_error; c.params_error = params_error;
  c.setdown_error = setdown_error; c.reported_num_params = read<int32_t>(output, kOutNumParams);
  c.register_ui_calls = g_register_ui_calls; c.invalid_custom_ui_registrations = g_invalid_custom_ui_registrations;
  c.custom_ui_events = g_custom_ui_registration.events;
  c.custom_ui_comp_size = {g_custom_ui_registration.comp_width, g_custom_ui_registration.comp_height};
  c.custom_ui_layer_size = {g_custom_ui_registration.layer_width, g_custom_ui_registration.layer_height};
  c.custom_ui_preview_size = {g_custom_ui_registration.preview_width, g_custom_ui_registration.preview_height};
  c.out_flags = read<uint32_t>(output, kOutFlags); c.out_flags2 = read<uint32_t>(output, kOutFlags2);
  c.update_params_ui_advertised = g_update_params_ui_advertised; c.query_dynamic_flags_advertised = g_query_dynamic_flags_advertised;
  c.conditional_ui_selectors_dispatched = g_conditional_ui_selectors_dispatched; c.update_params_ui_error = g_update_params_ui_error;
  c.query_dynamic_flags_error = g_query_dynamic_flags_error; c.update_param_ui_calls = g_update_param_ui_calls;
  c.pf_get_current_state_calls = g_pf_get_current_state_calls; c.pf_are_states_identical_calls = g_pf_are_states_identical_calls;
  c.suite_leases_balanced = suite_leases_balanced(); c.user_changed_param_requested = g_user_changed_param_requested;
  c.user_changed_param_slot = g_user_changed_param_slot; c.user_changed_param_error = g_user_changed_param_error;
  c.user_changed_parameters_json = requested_parameters_json(g_user_changed_parameters);
  const auto* message = reinterpret_cast<const char*>(output.data() + kOutMessage);
  c.return_message.assign(message, strnlen_s(message, 256)); c.about_message = about_message;
  c.about_selector_dispatched = !g_skip_about; c.last_seh_selector = g_last_seh_selector; c.last_seh_error = g_last_seh_error;
  c.lifecycle_errors = lifecycle_errors; c.lifecycle_data_null = lifecycle_data_null; c.module_audit_json = module_audit_json();
  c.parameters.reserve(g_params.size());
  for (const auto& p : g_params) {
    const auto* name = reinterpret_cast<const char*>(p.raw.data() + kParamName);
    c.parameters.push_back({p.index, p.disk_id, p.type, read<uint32_t>(p.raw, kParamUiFlags), read<int16_t>(p.raw, 8), read<int16_t>(p.raw, 10), read<uint32_t>(p.raw, kParamFlags), std::string(name, strnlen_s(name, kParamNameSize)), p.has_numeric, p.valid_min, p.valid_max, p.slider_min, p.slider_max, p.default_value, p.has_current, p.current_value, p.has_color, p.default_color, p.current_color, p.component_count, p.default_components, p.current_components, p.precision, p.choices, p.label, p.arbitrary_summary, p.layer_default});
  }
  std::cout << aexcompat::worker_report::serialize_l2_report(c);
}

struct EarlyModeBridge {
  EffectEntry entry{};
  std::array<std::byte, kInSize>* input{};
  std::array<std::byte, kOutSize>* output{};
  WorkerSession* session{};
  const std::string* about_message{};
};
uint32_t early_mode_out_flags(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return read<uint32_t>(*b.output, kOutFlags);
}
void early_mode_copy_sequence_data_to_input(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  write<void**>(*b.input, kInSequenceData, read<void**>(*b.output, kOutSequenceData));
}
int32_t early_mode_sequence_setup(void* opaque, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_sequence_selector(b.entry, kSequenceSetup, b.input->data(), b.output->data(), exception);
}
int32_t early_mode_sequence_setdown(void* opaque, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_sequence_selector(b.entry, kSequenceSetdown, b.input->data(), b.output->data(), exception);
}
int32_t early_mode_do_dialog(void* opaque, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_entry_seh(b.entry, kDoDialog, b.input->data(), b.output->data(),
                          nullptr, nullptr, nullptr, exception);
}
int32_t early_mode_global_setdown(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return invoke_global_setdown(b.entry, b.input->data(), b.output->data());
}
std::string early_mode_return_message(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  const char* message = reinterpret_cast<const char*>(b.output->data() + kOutMessage);
  return {message, strnlen_s(message, kOutSize - kOutMessage)};
}
bool early_mode_handle_lifetimes_balanced(void*) { return handle_lifetimes_balanced(); }
bool early_mode_prepare_protocol_report(void* opaque) {
  return static_cast<EarlyModeBridge*>(opaque)->session->prepare_protocol_report();
}
void* early_mode_external_dependencies(void* opaque, int32_t check_type,
                                       int32_t* error, uint32_t* exception) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  std::array<std::byte, 16> extra{};
  write<int32_t>(extra, 0, check_type);
  *error = invoke_entry_seh(b.entry, kGetExternalDependencies, b.input->data(), b.output->data(),
                            nullptr, nullptr, extra.data(), exception);
  return read<void**>(extra, 8);
}
bool early_mode_handle_is_live(void*, void* handle) { return host_handle_is_live(static_cast<void**>(handle)); }
uint64_t early_mode_handle_size(void*, void* handle) { return handle_size(static_cast<void**>(handle)); }
void* early_mode_lock_handle(void*, void* handle) { return lock_handle(static_cast<void**>(handle)); }
void early_mode_unlock_handle(void*, void* handle) { unlock_handle(static_cast<void**>(handle)); }
void early_mode_dispose_handle(void*, void* handle) { dispose_handle(static_cast<void**>(handle)); }
aexcompat::l2mode::HandleStatistics early_mode_handle_statistics(void*) {
  const auto s = statistics(); return {s.created, s.disposed};
}
bool early_mode_dispose_arbitrary_defaults(void* opaque) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  return dispose_arbitrary_defaults(b.entry, *b.input, *b.output);
}
void early_mode_report_parameters(void* opaque, const char* status, int32_t global_error,
                                  int32_t params_error, int32_t setdown_error) {
  const auto& b = *static_cast<EarlyModeBridge*>(opaque);
  report(status, global_error, params_error, setdown_error, *b.output, *b.about_message,
         {-1, -1, -1, -1, -1}, true);
}

bool verify_pf_color_settings_suite6() {
  g_working_color_space_kind = ColorProfileKind::Srgb;
  g_working_color_space_icc.clear();
  if (!color_settings_profiles_balanced() || !aegp_memory_balanced()) return false;
  const void* acquired = nullptr;
  if (acquire_suite("PF Color Settings Suite", 7, &acquired) != 0 ||
      acquired != &g_color_settings_suite6) return false;
  const auto& suite = g_color_settings_suite6;
  if (!suite.get_blending_tables || !suite.does_view_have_xform || !suite.xform_working_to_view ||
      !suite.get_new_working_space_profile || !suite.get_new_profile_from_icc ||
      !suite.get_new_icc_from_profile || !suite.get_new_profile_description ||
      !suite.dispose_profile || !suite.get_profile_approximate_gamma || !suite.is_rgb_profile ||
      !suite.set_working_color_space || !suite.is_ocio_used ||
      !suite.get_ocio_configuration_file || !suite.get_ocio_configuration_file_path ||
      !suite.get_ocio_working_colorspace || !suite.get_ocio_display_colorspace ||
      !suite.is_colorspace_aware_effects_enabled || !suite.get_lut_interpolation_method ||
      !suite.get_graphics_white_luminance || !suite.get_working_colorspace_id) return false;
  void* blending = reinterpret_cast<void*>(1);
  if (suite.get_blending_tables(nullptr, &blending) == 0 || blending != nullptr) return false;
  uint8_t has_xform = 2;
  if (suite.does_view_have_xform(&g_aegp_item_view, &has_xform) != 0 || has_xform != 0) return false;
  void* working = nullptr;
  if (suite.get_new_working_space_profile(1, &g_aegp_comp, &working) != 0 || !working) return false;
  float gamma = 0.0f;
  uint8_t is_rgb = 0;
  if (suite.get_profile_approximate_gamma(working, &gamma) != 0 || gamma != 2.2f ||
      suite.is_rgb_profile(working, &is_rgb) != 0 || is_rgb != 1) return false;
  void* icc_handle = nullptr;
  if (suite.get_new_icc_from_profile(1, working, &icc_handle) != 0 || !icc_handle) return false;
  void* icc_bytes = nullptr;
  uint32_t icc_size = 0;
  if (lock_aegp_mem_handle(icc_handle, &icc_bytes) != 0 || !icc_bytes ||
      get_aegp_mem_handle_size(icc_handle, &icc_size) != 0 ||
      icc_size != color_settings_builtin_srgb_icc().size() ||
      std::memcmp(icc_bytes, color_settings_builtin_srgb_icc().data(), icc_size) != 0 ||
      unlock_aegp_mem_handle(icc_handle) != 0 || free_aegp_mem_handle(icc_handle) != 0) return false;
  void* desc_handle = nullptr;
  if (suite.get_new_profile_description(1, working, &desc_handle) != 0 || !desc_handle) return false;
  void* desc_bytes = nullptr;
  const std::u16string expected_desc = u"sRGB IEC61966-2.1";
  if (lock_aegp_mem_handle(desc_handle, &desc_bytes) != 0 || !desc_bytes ||
      std::memcmp(desc_bytes, expected_desc.c_str(),
                  (expected_desc.size() + 1) * sizeof(char16_t)) != 0 ||
      unlock_aegp_mem_handle(desc_handle) != 0 || free_aegp_mem_handle(desc_handle) != 0) return false;
  void* imported = nullptr;
  const auto& linear_icc = color_settings_builtin_linear_icc();
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(linear_icc.size()),
                                     linear_icc.data(), &imported) != 0 || !imported) return false;
  if (suite.set_working_color_space(1, &g_aegp_comp, imported) != 0) return false;
  has_xform = 0;
  if (suite.does_view_have_xform(&g_aegp_item_view, &has_xform) != 0 || has_xform != 1) return false;
  AegpGuidValue guid{};
  if (suite.get_working_colorspace_id(1, &guid) != 0 || guid.bytes != kWorkingLinearSrgbGuid) return false;
  uint8_t ocio_used = 1;
  uint8_t aware = 1;
  uint16_t lut = 9;
  uint16_t white = 9;
  void* ocio_config = reinterpret_cast<void*>(1);
  void* ocio_path = reinterpret_cast<void*>(1);
  void* ocio_working = reinterpret_cast<void*>(1);
  void* ocio_display = reinterpret_cast<void*>(1);
  void* ocio_view = reinterpret_cast<void*>(1);
  if (suite.is_ocio_used(1, &ocio_used) != 0 || ocio_used != 0 ||
      suite.is_colorspace_aware_effects_enabled(1, &aware) != 0 || aware != 0 ||
      suite.get_lut_interpolation_method(1, &lut) != 0 || lut != 0 ||
      suite.get_graphics_white_luminance(1, &white) != 0 || white != 0 ||
      suite.get_ocio_configuration_file(1, &ocio_config) != 0 || !ocio_config ||
      suite.get_ocio_configuration_file_path(1, &ocio_path) != 0 || !ocio_path ||
      suite.get_ocio_working_colorspace(1, &ocio_working) != 0 || !ocio_working ||
      suite.get_ocio_display_colorspace(1, &ocio_display, &ocio_view) != 0 ||
      !ocio_display || !ocio_view) return false;
  for (void* handle : {ocio_config, ocio_path, ocio_working, ocio_display, ocio_view}) {
    void* empty = nullptr;
    uint32_t empty_size = 99;
    if (lock_aegp_mem_handle(handle, &empty) != 0 || !empty ||
        get_aegp_mem_handle_size(handle, &empty_size) != 0 || empty_size != sizeof(char16_t) ||
        unlock_aegp_mem_handle(handle) != 0 || free_aegp_mem_handle(handle) != 0) return false;
  }
  const uint8_t malformed[] = {0x00, 0x00, 0x00, 0x10, 'b', 'a', 'd', '!', 'b', 'a', 'd', '!', 'b', 'a', 'd', '!'};
  void* rejected = reinterpret_cast<void*>(1);
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(sizeof(malformed)), malformed,
                                     &rejected) == 0 || rejected != nullptr) return false;
  if (suite.dispose_profile(nullptr) == 0 || suite.dispose_profile(working) != 0 ||
      suite.dispose_profile(working) == 0 || suite.dispose_profile(imported) != 0) return false;
  float foreign_gamma = 99.0f;
  uint8_t foreign_rgb = 1;
  void* foreign = reinterpret_cast<void*>(static_cast<uintptr_t>(0x12347));
  if (suite.get_profile_approximate_gamma(foreign, &foreign_gamma) == 0 || foreign_gamma != 0.0f ||
      suite.is_rgb_profile(foreign, &foreign_rgb) == 0 || foreign_rgb != 0 ||
      suite.dispose_profile(foreign) == 0) return false;
  auto overflowing_icc = linear_icc;
  color_settings_write_be32(overflowing_icc, 136, static_cast<uint32_t>(overflowing_icc.size() - 2));
  color_settings_write_be32(overflowing_icc, 140, 20);
  rejected = reinterpret_cast<void*>(1);
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(overflowing_icc.size()),
                                     overflowing_icc.data(), &rejected) == 0 || rejected != nullptr)
    return false;
  std::array<void*, kMaxColorProfiles> bounded_profiles{};
  for (auto& profile : bounded_profiles) {
    if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(linear_icc.size()),
                                       linear_icc.data(), &profile) != 0 || !profile) return false;
  }
  void* over_capacity = reinterpret_cast<void*>(1);
  if (suite.get_new_profile_from_icc(1, static_cast<int32_t>(linear_icc.size()),
                                     linear_icc.data(), &over_capacity) == 0 || over_capacity != nullptr)
    return false;
  for (void* profile : bounded_profiles)
    if (suite.dispose_profile(profile) != 0) return false;
  void** oversized_world = reinterpret_cast<void**>(1);
  if (aegp_world_new_owned(1, 3, (std::numeric_limits<int32_t>::max)(), 2,
                           &oversized_world) == 0 || oversized_world != nullptr) return false;
  auto fill_world = [](void*** handle, int32_t type, int32_t width, int32_t height,
                       auto fill_pixel) {
    if (aegp_world_new_owned(1, type, width, height, handle) != 0 || !*handle) return false;
    void* base = nullptr;
    if ((type == 1 && aegp_world_get_base_addr8(*handle, &base) != 0) ||
        (type == 2 && aegp_world_get_base_addr16(*handle, &base) != 0) ||
        (type == 3 && aegp_world_get_base_addr32(*handle, &base) != 0) || !base) return false;
    fill_pixel(base, width, height);
    return true;
  };
  void** world8 = nullptr;
  void** world16 = nullptr;
  void** world32 = nullptr;
  if (!fill_world(&world8, 1, 2, 1, [](void* base, int32_t width, int32_t) {
        auto* pixels = static_cast<uint8_t*>(base);
        pixels[0] = 200; pixels[1] = 64; pixels[2] = 32; pixels[3] = 16;
        pixels[4] = 180; pixels[5] = 128; pixels[6] = 64; pixels[7] = 32;
      }) ||
      !fill_world(&world16, 2, 1, 2, [](void* base, int32_t, int32_t height) {
        auto* pixels = static_cast<uint16_t*>(base);
        pixels[0] = 30000; pixels[1] = 10000; pixels[2] = 5000; pixels[3] = 2500;
        pixels[4] = 20000; pixels[5] = 15000; pixels[6] = 7500; pixels[7] = 3750;
      }) ||
      !fill_world(&world32, 3, 1, 1, [](void* base, int32_t, int32_t) {
        auto* pixels = static_cast<float*>(base);
        pixels[0] = 0.5f; pixels[1] = 0.25f; pixels[2] = 0.125f; pixels[3] = 0.0625f;
      })) return false;
  if (suite.xform_working_to_view(&g_aegp_item_view, world8, world8) != 0 ||
      suite.xform_working_to_view(&g_aegp_item_view, world16, world16) != 0 ||
      suite.xform_working_to_view(&g_aegp_item_view, world32, world32) != 0) return false;
  uint8_t pixel8[8]{};
  uint16_t pixel16[8]{};
  float pixel32[4]{};
  void* base8 = nullptr; void* base16 = nullptr; void* base32 = nullptr;
  if (aegp_world_get_base_addr8(world8, &base8) != 0 || !base8 ||
      aegp_world_get_base_addr16(world16, &base16) != 0 || !base16 ||
      aegp_world_get_base_addr32(world32, &base32) != 0 || !base32) return false;
  std::memcpy(pixel8, base8, sizeof(pixel8));
  std::memcpy(pixel16, base16, sizeof(pixel16));
  std::memcpy(pixel32, base32, sizeof(pixel32));
  if (pixel8[0] != 200 || pixel8[4] != 180 || pixel16[0] != 30000 || pixel16[4] != 20000 ||
      pixel32[0] != 0.5f) return false;
  if (pixel8[1] <= 64 || pixel8[2] <= 32 || pixel8[3] <= 16 ||
      pixel16[1] <= 10000 || pixel32[1] <= 0.25f) return false;
  void** dst8 = nullptr;
  if (!fill_world(&dst8, 1, 2, 1, [](void* base, int32_t width, int32_t) {
        std::memset(base, 0xcd, static_cast<std::size_t>(width) * 4);
      })) return false;
  auto* restored8 = static_cast<uint8_t*>(base8);
  restored8[0] = 200; restored8[1] = 64; restored8[2] = 32; restored8[3] = 16;
  restored8[4] = 180; restored8[5] = 128; restored8[6] = 64; restored8[7] = 32;
  if (suite.xform_working_to_view(&g_aegp_item_view, world8, dst8) != 0) return false;
  void* dst_base8 = nullptr;
  if (aegp_world_get_base_addr8(dst8, &dst_base8) != 0 || !dst_base8 ||
      std::memcmp(dst_base8, pixel8, sizeof(pixel8)) != 0) return false;
  void** mismatch = world16;
  if (suite.xform_working_to_view(&g_aegp_item_view, world8, mismatch) == 0) return false;
  if (aegp_world_dispose(world8) != 0 || aegp_world_dispose(world16) != 0 ||
      aegp_world_dispose(world32) != 0 || aegp_world_dispose(dst8) != 0) return false;
  return color_settings_profiles_balanced() && aegp_memory_balanced() &&
      release_suite("PF Color Settings Suite", 7) == 0;
}

}  // namespace aexcompat::l2_detail

using namespace aexcompat::l2_detail;

bool verify_pf_color_suite() {
  const void* acquired8{}; const void* acquired16{}; const void* acquired_float{};
  const bool acquired = acquire_suite("PF Color Suite", 1, &acquired8) == 0 &&
      acquire_suite("PF Color16 Suite", 1, &acquired16) == 0 &&
      acquire_suite("PF ColorFloat Suite", 1, &acquired_float) == 0 &&
      acquired8 == &g_color_suite8 && acquired16 == &g_color_suite16 &&
      acquired_float == &g_color_suite_float;
  PfPixel8 red8{77, 255, 0, 0}, round8{91, 0, 0, 0};
  PfFixed hls[3]{}, yiq[3]{};
  int32_t lum8{}, hue8{}, light8{}, sat8{};
  bool ok = acquired && g_color_suite8.RGBtoHLS(nullptr, &red8, hls) == 0 &&
      hls[0] == 0 && hls[1] == pf_color_to_fixed(0.5) && hls[2] == pf_color_to_fixed(1.0) &&
      g_color_suite8.HLStoRGB(nullptr, hls, &round8) == 0 && round8.alpha == 91 &&
      round8.red == 255 && round8.green <= 1 && round8.blue <= 1 &&
      g_color_suite8.RGBtoYIQ(nullptr, &red8, yiq) == 0 &&
      g_color_suite8.Luminance(nullptr, &red8, &lum8) == 0 &&
      g_color_suite8.Hue(nullptr, &red8, &hue8) == 0 &&
      g_color_suite8.Lightness(nullptr, &red8, &light8) == 0 &&
      g_color_suite8.Saturation(nullptr, &red8, &sat8) == 0 &&
      lum8 == 7622 && hue8 == 0 && light8 == 128 && sat8 == 255;
  PfPixel16 green16{1234, 0, 32768, 0}, round16{4321, 0, 0, 0};
  ok = ok && g_color_suite16.RGBtoHLS(nullptr, &green16, hls) == 0 &&
      hls[0] == pf_color_to_fixed(120.0) &&
      g_color_suite16.HLStoRGB(nullptr, hls, &round16) == 0 && round16.alpha == 4321 &&
      round16.green >= 32767 && round16.red <= 1 && round16.blue <= 1;
  int32_t hue16{};
  ok = ok && g_color_suite16.Hue(nullptr, &green16, &hue16) == 0 && hue16 == 85;
  PfPixelFloat hdr{2.0f, 1.5f, 2.0f, -1.0f};
  float lumf{};
  ok = ok && g_color_suite_float.RGBtoYIQ(nullptr, &hdr, yiq) == 0 &&
      yiq[0] > 65536 && g_color_suite_float.Luminance(nullptr, &hdr, &lumf) == 0 &&
      lumf > 1.0f;
  PfPixelFloat invalid{1.0f, std::numeric_limits<float>::infinity(), 0.0f, 0.0f};
  PfFixed sentinel[3]{11, 22, 33};
  ok = ok && g_color_suite_float.RGBtoHLS(nullptr, &invalid, sentinel) == kPfBadCallbackParam &&
      sentinel[0] == 11 && sentinel[1] == 22 && sentinel[2] == 33 &&
      g_color_suite8.RGBtoHLS(nullptr, nullptr, sentinel) == kPfBadCallbackParam &&
      g_color_suite8.RGBtoHLS(nullptr, &red8, nullptr) == kPfBadCallbackParam;
  ok = release_suite("PF ColorFloat Suite", 1) == 0 &&
      release_suite("PF Color16 Suite", 1) == 0 &&
      release_suite("PF Color Suite", 1) == 0 && ok;
  return ok;
}

bool verify_pf_color_param_suite() {
  const auto saved_params = g_params;
  g_params.clear();
  auto add_color = [](int32_t disk_id, std::array<unsigned char, 4> current8,
                      std::array<unsigned char, 4> default8,
                      std::array<float, 4> current_float,
                      std::array<float, 4> default_float) {
    ParamRecord record{};
    record.index = static_cast<int32_t>(g_params.size() + 1);
    record.disk_id = disk_id;
    record.type = 5;
    record.has_color = true;
    record.current_color = current8;
    record.default_color = default8;
    record.current_float_color = current_float;
    record.default_float_color = default_float;
    g_params.push_back(record);
  };
  add_color(101, {255, 64, 128, 192}, {128, 10, 20, 30},
            {1.0f, 64.0f / 255.0f, 128.0f / 255.0f, 192.0f / 255.0f},
            {128.0f / 255.0f, 10.0f / 255.0f, 20.0f / 255.0f, 30.0f / 255.0f});
  add_color(102, {255, 17, 33, 65}, {255, 1, 2, 3},
            {1.0f, 4097.0f / 32768.0f, 8193.0f / 32768.0f, 16385.0f / 32768.0f},
            {1.0f, 1.0f / 32768.0f, 2.0f / 32768.0f, 3.0f / 32768.0f});
  add_color(103, {255, 200, 100, 50}, {255, 40, 50, 60},
            {0.75f, 1.5f, -0.25f, 2.0f}, {1.0f, 0.1f, 0.2f, 0.3f});

  const void* acquired = nullptr;
  bool ok = acquire_suite("PF ColorParamSuite", 1, &acquired) == 0 &&
      acquired == &g_color_param_suite1;
  auto definition = [](const ParamRecord& record, bool current) {
    std::array<std::byte, kParamSize> bytes{};
    write<int32_t>(bytes, 0, record.disk_id);
    write<int32_t>(bytes, kParamType, record.type);
    const auto& color = current ? record.current_color : record.default_color;
    std::memcpy(bytes.data() + 56, color.data(), color.size());
    return bytes;
  };
  PfColorParamPixelFloat out{};
  auto current8 = definition(g_params[0], true);
  auto default8 = definition(g_params[0], false);
  ok = ok && floating_point_from_color(&g_effect, current8.data(), &out) == 0 &&
      out.alpha == 1.0f && out.red == 64.0f / 255.0f &&
      out.green == 128.0f / 255.0f && out.blue == 192.0f / 255.0f &&
      floating_point_from_color(&g_effect, default8.data(), &out) == 0 &&
      out.alpha == 128.0f / 255.0f && out.red == 10.0f / 255.0f;
  auto current16 = definition(g_params[1], true);
  ok = ok && floating_point_from_color(&g_effect, current16.data(), &out) == 0 &&
      out.red == 4097.0f / 32768.0f && out.green == 8193.0f / 32768.0f &&
      out.blue == 16385.0f / 32768.0f;
  auto current_float = definition(g_params[2], true);
  ok = ok && floating_point_from_color(&g_effect, current_float.data(), &out) == 0 &&
      out.alpha == 0.75f && out.red == 1.5f && out.green == -0.25f && out.blue == 2.0f;

  const PfColorParamPixelFloat sentinel{9.0f, 8.0f, 7.0f, 6.0f};
  out = sentinel;
  auto invalid_index = current8;
  write<int32_t>(invalid_index, 0, 9999);
  auto invalid_type = current8;
  write<int32_t>(invalid_type, kParamType, 6);
  ok = ok && floating_point_from_color(nullptr, current8.data(), &out) == kPfBadCallbackParam &&
      floating_point_from_color(&g_effect, nullptr, &out) == kPfBadCallbackParam &&
      floating_point_from_color(&g_effect, current8.data(), nullptr) == kPfBadCallbackParam &&
      floating_point_from_color(&g_effect, invalid_index.data(), &out) == kPfInvalidIndex &&
      floating_point_from_color(&g_effect, invalid_type.data(), &out) ==
          kPfUnrecognizedParamType && std::memcmp(&out, &sentinel, sizeof(out)) == 0;
  ok = release_suite("PF ColorParamSuite", 1) == 0 && ok;
  g_params = saved_params;
  return ok;
}

bool verify_pf_param_utils_suite3() {
  const auto saved_params = g_params;
  g_params.clear();
  ParamRecord param{};
  param.index = 1;
  param.disk_id = 7001;
  param.type = 1;
  write<int32_t>(param.raw, 0, param.disk_id);
  write<int32_t>(param.raw, kParamType, param.type);
  write<int32_t>(param.raw, 56, 42);
  g_params.push_back(param);
  reset_pf_state_effect_lifetime(&g_effect, true);

  auto active = param.raw;
  auto local = active;
  write<uint32_t>(local, kParamUiFlags, (1u << 5));
  write<int16_t>(local, 8, 123);
  write<int16_t>(local, 10, 45);
  write<uint32_t>(local, kParamFlags, (1u << 5));
  std::memcpy(local.data() + kParamName, "Updated", 8);
  void* active_params[2]{nullptr, active.data()};
  g_active_ui_params = active_params;
  g_active_ui_param_count = 2;
  g_update_params_ui_active = true;

  const void* acquired{};
  PfState first{}, second{}, changed{};
  PfTime start{12, 24}, duration{1, 24};
  uint8_t same = 0, identical = 0, found = 1;
  int32_t count = 0, key_index = 9, key_time = 9;
  uint32_t key_scale = 0;
  bool ok = acquire_suite("PF Param Utils Suite", 3, &acquired) == 0 &&
      acquired == &g_param_utils_suite &&
      std::all_of(reinterpret_cast<void* const*>(&g_param_utils_suite),
                  reinterpret_cast<void* const*>(&g_param_utils_suite) + 9,
                  [](const void* slot) { return slot != nullptr; }) &&
      update_param_ui(&g_effect, 1, local.data()) == 0 &&
      read<uint32_t>(active, kParamUiFlags) == (1u << 5) &&
      read<int16_t>(active, 8) == 123 && read<int16_t>(active, 10) == 45 &&
      read<uint32_t>(active, kParamFlags) == (1u << 5) &&
      std::strcmp(reinterpret_cast<const char*>(active.data() + kParamName), "Updated") == 0 &&
      get_current_param_state(&g_effect, 1, &start, &duration, &first) == 0 &&
      get_current_param_state(&g_effect, 1, &start, &duration, &second) == 0 &&
      are_param_states_identical(&g_effect, &first, &second, &same) == 0 && same == 1;
  const void* acquired_v1{};
  PfState obsolete_state{};
  uint8_t obsolete_changed = 0;
  ok = ok && acquire_suite("PF Param Utils Suite", 2, &acquired_v1) == 0 &&
      acquired_v1 == &g_param_utils_suite1 && acquired_v1 != acquired &&
      std::all_of(reinterpret_cast<void* const*>(&g_param_utils_suite1),
                  reinterpret_cast<void* const*>(&g_param_utils_suite1) + 10,
                  [](const void* slot) { return slot != nullptr; }) &&
      g_param_utils_suite1.PF_GetCurrentStateObsolete(&g_effect, &obsolete_state) == 0 &&
      g_param_utils_suite1.PF_HasParamChangedObsolete(
          &g_effect, &obsolete_state, 999, &obsolete_changed) == 0 &&
      obsolete_changed == 1;
  obsolete_changed = 0;
  ok = ok && g_param_utils_suite1.PF_HaveInputsChangedOverTimeSpanObsolete(
                 &g_effect, &obsolete_state, &start, &duration, &obsolete_changed) == 0 &&
      obsolete_changed == 1;
  PfState foreign_state{{9, 8, 7, 6}};
  obsolete_changed = 0x5a;
  ok = ok && g_param_utils_suite1.PF_HasParamChangedObsolete(
                 &g_effect, &foreign_state, 1, &obsolete_changed) == kPfBadCallbackParam &&
      obsolete_changed == 0x5a &&
      g_param_utils_suite1.PF_HaveInputsChangedOverTimeSpanObsolete(
          nullptr, &obsolete_state, nullptr, nullptr, &obsolete_changed) ==
          kPfBadCallbackParam && obsolete_changed == 0x5a &&
      release_suite("PF Param Utils Suite", 2) == 0;
  write<int32_t>(g_params[0].raw, 56, 43);
  ok = ok && get_current_param_state(&g_effect, 1, &start, &duration, &changed) == 0 &&
      are_param_states_identical(&g_effect, &first, &changed, &same) == 0 && same == 0 &&
      is_identical_param_checkout(&g_effect, 1, 0, 1, 24, 10, 1, 24, &identical) == 0 &&
      identical == 1 &&
      find_param_keyframe_time(&g_effect, 1, 0, 24, 0, &found, &key_index,
                               &key_time, &key_scale) == 0 &&
      found == 0 && key_index == -1 && key_time == 0 && key_scale == 24 &&
      get_param_keyframe_count(&g_effect, 1, &count) == 0 && count == -1 &&
      checkout_param_keyframe(&g_effect, 1, 0, nullptr, nullptr, g_params[0].raw.data()) ==
          kPfInvalidIndex &&
      checkin_param_keyframe(&g_effect, g_params[0].raw.data()) == kPfInvalidIndex &&
      param_key_index_to_time(&g_effect, 1, 0, &key_time, &key_scale) == kPfInvalidIndex;
  PfState sentinel{{1, 2, 3, 4}};
  changed = sentinel;
  ok = ok && get_current_param_state(nullptr, 1, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam &&
      std::memcmp(&changed, &sentinel, sizeof(changed)) == 0 &&
      get_current_param_state(&g_effect, 999, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam &&
      are_param_states_identical(&g_effect, nullptr, &first, &same) == kPfBadCallbackParam;

  PfState zero{}, random{}, bit_flip = first;
  BCryptGenRandom(nullptr, reinterpret_cast<PUCHAR>(&random), sizeof(random),
                  BCRYPT_USE_SYSTEM_PREFERRED_RNG);
  reinterpret_cast<unsigned char*>(&bit_flip)[7] ^= 0x40;
  same = 0x5a;
  ok = ok && are_param_states_identical(&g_effect, &zero, &first, &same) ==
          kPfBadCallbackParam && same == 0x5a &&
      are_param_states_identical(&g_effect, &random, &first, &same) ==
          kPfBadCallbackParam && same == 0x5a &&
      are_param_states_identical(&g_effect, &bit_flip, &first, &same) ==
          kPfBadCallbackParam && same == 0x5a;
  {
    std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
    PfStateToken token{};
    std::memcpy(token.data(), &first, token.size());
    const auto found_entry = g_pf_state_registry.find(token);
    ok = ok && found_entry != g_pf_state_registry.end();
    if (found_entry != g_pf_state_registry.end()) found_entry->second.owner = &g_layer;
  }
  ok = ok && are_param_states_identical(&g_effect, &first, &second, &same) ==
          kPfBadCallbackParam && same == 0x5a;
  reset_pf_state_effect_lifetime(&g_effect, true);
  ok = ok && are_param_states_identical(&g_effect, &first, &second, &same) ==
          kPfBadCallbackParam && same == 0x5a;
  {
    std::lock_guard<std::mutex> lock(g_pf_state_registry_mutex);
    PfStateRegistryEntry filler{&g_effect, g_pf_state_effect_generation, 1,
                                false, {}, {}, {}};
    for (std::size_t i = 0; i < kMaxPfStateRegistryEntries; ++i) {
      PfStateToken token{};
      std::memcpy(token.data(), &i, sizeof(i));
      token.back() = 0xa5;
      g_pf_state_registry.emplace(token, filler);
    }
  }
  changed = sentinel;
  ok = ok && get_current_param_state(&g_effect, 1, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam && std::memcmp(&changed, &sentinel, sizeof(changed)) == 0;
  reset_pf_state_effect_lifetime(&g_effect, false);
  ok = ok && g_pf_state_registry.empty() &&
      get_current_param_state(&g_effect, 1, nullptr, nullptr, &changed) ==
          kPfBadCallbackParam && release_suite("PF Param Utils Suite", 3) == 0;
  g_update_params_ui_active = false;
  g_active_ui_params = nullptr;
  g_active_ui_param_count = 0;
  g_params = saved_params;
  return ok;
}

bool verify_parameter_animation_transport() {
  const auto saved_params = g_params;
  const auto saved_timelines = g_parameter_timelines;
  g_params.clear();
  g_parameter_timelines.clear();
  ParamRecord param{};
  param.index = 1;
  param.disk_id = 9001;
  param.type = 10;
  write<int32_t>(param.raw, 0, param.disk_id);
  write<int32_t>(param.raw, kParamType, param.type);
  g_params.push_back(param);
  ParameterTimeline timeline;
  timeline.slot = 1;
  AnimationKey a{};
  a.time = 0;
  a.scale = 24;
  a.kind = AnimationValueKind::Scalar;
  a.scalar = 10.0;
  AnimationKey b = a;
  b.time = 24;
  b.scalar = 20.0;
  AnimationKey c = b;
  c.time = 48;
  c.scalar = 40.0;
  b.hold = true;
  timeline.keys = {a, b, c};
  g_parameter_timelines.push_back(timeline);
  std::vector<std::array<std::byte, kParamSize>> definitions(2);
  definitions[1] = param.raw;
  uint8_t identical = 1, found = 0;
  int32_t count = 0, index = -1, time = 0;
  uint32_t scale = 0;
  bool ok = apply_parameter_animation(definitions, 12, 24) &&
            std::abs(read<double>(definitions[1], 56) - 15.0) < 1e-12 &&
            apply_parameter_animation(definitions, 36, 24) &&
            std::abs(read<double>(definitions[1], 56) - 20.0) < 1e-12 &&
            get_param_keyframe_count(&g_effect, 1, &count) == 0 && count == 3 &&
            find_param_keyframe_time(&g_effect, 1, 12, 24, 0, &found, &index,
                                     &time, &scale) == 0 &&
            found == 1 && index == 1 && time == 24 && scale == 24 &&
            param_key_index_to_time(&g_effect, 1, 2, &time, &scale) == 0 &&
            time == 48 && scale == 24 &&
            is_identical_param_checkout(&g_effect, 1, 24, 1, 24, 36, 1, 24,
                                        &identical) == 0 &&
            identical == 1 &&
            is_identical_param_checkout(&g_effect, 1, 0, 1, 24, 12, 1, 24,
                                        &identical) == 0 &&
            identical == 0;
  g_params = saved_params;
  g_parameter_timelines = saved_timelines;
  g_keyframe_checkout_ledger.clear();
  return ok;
}

bool verify_aegp_effect_param_union_suite4() {
  const bool saved_live = g_aegp_effect_live;
  g_aegp_effect_live = true;
  std::array<std::byte, kParamSize - 56> value{};
  std::array<std::byte, kParamSize - 56> sentinel{};
  sentinel.fill(std::byte{0x5a});
  int32_t type = -1;
  bool ok = aegp_get_effect_param_union_by_index_v3(
                1, &g_aegp_effect, 0, &type, value.data()) == 0 &&
            type == 0 &&
            std::all_of(value.begin(), value.end(),
                        [](std::byte byte) { return byte == std::byte{0}; });
  value = sentinel;
  type = -1;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 1, &type, value.data()) == 0 &&
       type == 1;
  for (const int32_t index : {-1, 5}) {
    value = sentinel;
    type = 0x12345678;
    ok = ok && aegp_get_effect_param_union_by_index_v3(
                   1, &g_aegp_effect, index, &type, value.data()) == 4 &&
         type == 0x12345678 && value == sentinel;
  }
  value = sentinel;
  type = 0x12345678;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 0, nullptr, value.data()) == 4 &&
       value == sentinel &&
       aegp_get_effect_param_union_by_index_v3(
           1, &g_aegp_effect, 0, &type, nullptr) == 4 &&
       type == 0x12345678;
  g_aegp_effect_live = false;
  ok = ok && aegp_get_effect_param_union_by_index_v3(
                 1, &g_aegp_effect, 0, &type, value.data()) == 4;
  g_aegp_effect_live = saved_live;
  return ok;
}

bool verify_aegp_installed_effect_catalog_suite4() {
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* acquired = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 4, &acquired) == 0 &&
            acquired == g_aegp_effect_suite4.data();
  int32_t count = -1;
  ok = ok && aegp_get_num_installed_effects(&count) == 0 &&
       count == static_cast<int32_t>(kAegpInstalledEffects.size());
  count = 0x12345678;
  ok = ok && aegp_get_num_installed_effects(nullptr) == 4 && count == 0x12345678;

  int32_t key = -1;
  ok = ok && aegp_get_next_installed_effect(kAegpInstalledEffectKeyNone, &key) == 0 &&
       key == kAegpInstalledEffects[0].key;
  const int32_t installed_key = key;
  for (std::size_t index = 1; index < kAegpInstalledEffects.size(); ++index)
    ok = ok && aegp_get_next_installed_effect(key, &key) == 0 &&
         key == kAegpInstalledEffects[index].key;
  ok = ok && aegp_get_next_installed_effect(key, &key) == 0 &&
       key == kAegpInstalledEffectKeyNone;
  key = 0x12345678;
  ok = ok && aegp_get_next_installed_effect(9999, &key) == 4 &&
       key == 0x12345678 &&
       aegp_get_next_installed_effect(kAegpInstalledEffectKeyNone, nullptr) == 4;

  std::array<char, kAegpMaxEffectCategoryNameSize> name{};
  std::array<char, kAegpMaxEffectCategoryNameSize> match_name{};
  std::array<char, kAegpMaxEffectCategoryNameSize> category{};
  ok = ok && aegp_get_effect_name(installed_key, name.data()) == 0 &&
       std::strcmp(name.data(), kAegpInstalledEffects[0].name) == 0 &&
       aegp_get_effect_match_name(installed_key, match_name.data()) == 0 &&
       std::strcmp(match_name.data(), kAegpInstalledEffects[0].match_name) == 0 &&
       aegp_get_effect_category(installed_key, category.data()) == 0 &&
       std::strcmp(category.data(), kAegpInstalledEffects[0].category) == 0 &&
       category[std::strlen(kAegpInstalledEffects[0].category)] == '\0';
  category.fill('Z');
  ok = ok && aegp_get_effect_category(9999, category.data()) == 4 &&
       std::all_of(category.begin(), category.end(), [](char value) { return value == 'Z'; }) &&
       aegp_get_effect_category(installed_key, nullptr) == 4;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  return ok;
}

bool verify_aegp_layer_source_item() {
  const uint32_t calls_before = g_aegp_layer_source_item_calls;
  void* item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  bool ok = aegp_get_layer_source_item(&g_layer, &item) == 0 &&
            item == aegp_comp_item_handle();
  for (auto& layer : g_aegp_layers) {
    item = nullptr;
    ok = ok && aegp_get_layer_source_item(&layer, &item) == 0 &&
         item == aegp_comp_item_handle();
  }
  AegpSceneObject foreign{0x464f5245};
  item = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_source_item(&foreign, &item) == 4 &&
       item == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
       aegp_get_layer_source_item(nullptr, &item) == 4 &&
       item == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
       aegp_get_layer_source_item(&g_layer, nullptr) == 4 &&
       g_aegp_layer_source_item_calls == calls_before + 4;
  const uint32_t item_type_calls_before = g_aegp_item_type_calls;
  const bool saved_comp_idle_mode = g_aegp_comp_idle_roundtrip_mode;
  g_aegp_comp_idle_roundtrip_mode = true;
  const void* acquired{};
  ok = ok && acquire_suite("AEGP Item Suite", 14, &acquired) == 0 &&
       acquired == &g_aegp_item_suite && g_aegp_item_suite.get_item_type != nullptr;
  int16_t item_type = -1;
  ok = ok && g_aegp_item_suite.get_item_type(&g_aegp_comp_item, &item_type) == 0 &&
       item_type == 2;
  AegpSceneObject foreign_item{0x464f5249};
  item_type = 0x1234;
  ok = ok && g_aegp_item_suite.get_item_type(&foreign_item, &item_type) == 4 &&
       item_type == 0x1234 &&
       g_aegp_item_suite.get_item_type(nullptr, &item_type) == 4 &&
       item_type == 0x1234 &&
       g_aegp_item_suite.get_item_type(&g_aegp_comp_item, nullptr) == 4 &&
       g_aegp_item_type_calls == item_type_calls_before + 1;
  g_aegp_comp_idle_roundtrip_mode = saved_comp_idle_mode;
  return ok;
}

struct LayerSuite2AsyncTestResult {
  std::atomic<bool> done{};
  int32_t error{4};
  void* receipt{};
};
int32_t __cdecl layer_suite2_async_test_callback(
    uint64_t, uint8_t canceled, int32_t error, void* receipt, void* refcon) {
  auto* result = static_cast<LayerSuite2AsyncTestResult*>(refcon);
  if (!result) return 4;
  result->error = canceled ? 4 : error;
  result->receipt = receipt;
  result->done.store(true);
  return 0;
}

bool verify_aegp_layer_render_options_suite2() {
  const bool saved_effect_live = g_aegp_effect_live;
  const auto saved_context = g_loaded_effect_receipt_context;
  const uint32_t created_before = layer_created_count();
  const uint32_t disposed_before = layer_disposed_count();
  g_aegp_effect_live = true;
  const auto make_world = [](uint8_t red, uint8_t green, uint8_t blue) {
    std::vector<unsigned char> pixels(4 * 2 * 4);
    for (std::size_t pixel = 0; pixel < pixels.size() / 4; ++pixel) {
      pixels[pixel * 4 + 0] = 128;
      pixels[pixel * 4 + 1] = red;
      pixels[pixel * 4 + 2] = green;
      pixels[pixel * 4 + 3] = blue;
    }
    return pixels;
  };
  auto source = make_world(100, 50, 25);
  auto all_effects = make_world(40, 120, 30);
  auto downstream_pixels = make_world(20, 60, 140);
  LoadedEffectReceiptContext context{};
  context.entry = reinterpret_cast<EffectEntry>(&verify_aegp_layer_render_options_suite2);
  context.current_time = 0;
  context.time_scale = 1;
  context.time_step = 1;
  context.total_time = 1;
  context.pixel_bytes = 4;
  context.source_argb = &source;
  context.source_width = 4;
  context.source_height = 2;
  context.all_effects_argb = &all_effects;
  context.all_effects_width = 4;
  context.all_effects_height = 2;
  context.all_effects_pixel_bytes = 4;
  context.all_effects_finalized = true;
  g_loaded_effect_receipt_context = context;

  const void* acquired = nullptr;
  bool ok = acquire_suite("AEGP Layer Render Options Suite", 2, &acquired) == 0 &&
      acquired == &g_layer_render_options_suite2;
  void* upstream = nullptr;
  ok = ok && new_from_upstream_of_effect(1, &g_aegp_effect, &upstream) == 0 && upstream &&
      set_layer_render_downsample(upstream, 2, 2) == 0 &&
      set_layer_render_world_type(upstream, 2) == 0 &&
      set_layer_render_matte(upstream, 1) == 0;
  auto checkout_hash = [&](void* options, std::string& hash) {
    void* receipt = nullptr;
    void** world = nullptr;
    int32_t type = 0, width = 0, height = 0;
    void* pixels = nullptr;
    const bool checked_out = render_checkout_layer_v5(
        options, nullptr, nullptr, &receipt) == 0 && receipt &&
        get_receipt_world(receipt, &world) == 0 && world &&
        aegp_world_get_type(world, &type) == 0 && type == 2 &&
        aegp_world_get_size(world, &width, &height) == 0 && width == 2 && height == 1 &&
        aegp_world_get_base_addr16(world, &pixels) == 0 && pixels;
    if (checked_out) hash = sha256_bytes(static_cast<const unsigned char*>(pixels),
        static_cast<std::size_t>(width) * height * 8);
    return checked_out && checkin_frame(receipt) == 0;
  };
  std::string upstream_hash, all_hash, downstream_hash, async_hash;
  ok = ok && checkout_hash(upstream, upstream_hash);

  void* all = nullptr;
  ok = ok && new_layer_render_options(1, &g_aegp_layers[0], &all) == 0 && all &&
      set_layer_render_downsample(all, 2, 2) == 0 &&
      set_layer_render_world_type(all, 2) == 0 &&
      set_layer_render_matte(all, 1) == 0 && checkout_hash(all, all_hash);

  void* downstream = nullptr;
  ok = ok && new_from_downstream_of_effect(1, &g_aegp_effect, &downstream) == 0 && downstream;
  void* rejected = reinterpret_cast<void*>(1);
  ok = ok && render_checkout_layer_v5(downstream, nullptr, nullptr, &rejected) != 0 &&
      rejected == nullptr;
  g_loaded_effect_receipt_context.downstream_argb = &downstream_pixels;
  g_loaded_effect_receipt_context.downstream_width = 4;
  g_loaded_effect_receipt_context.downstream_height = 2;
  g_loaded_effect_receipt_context.downstream_pixel_bytes = 4;
  g_loaded_effect_receipt_context.downstream_finalized = true;
  ok = ok && set_layer_render_downsample(downstream, 2, 2) == 0 &&
      set_layer_render_world_type(downstream, 2) == 0 &&
      set_layer_render_matte(downstream, 1) == 0 &&
      checkout_hash(downstream, downstream_hash) &&
      upstream_hash != all_hash && upstream_hash != downstream_hash &&
      all_hash != downstream_hash;

  LayerSuite2AsyncTestResult async_result{};
  uint64_t request_id = 0;
  ok = ok && render_checkout_layer_async_reject(downstream,
      &layer_suite2_async_test_callback, &async_result, &request_id) == 0 && request_id != 0;
  drain_async_layer_requests();
  void** async_world = nullptr;
  void* async_pixels = nullptr;
  int32_t async_width = 0, async_height = 0;
  ok = ok && async_result.done.load() && async_result.error == 0 && async_result.receipt &&
      get_receipt_world(async_result.receipt, &async_world) == 0 && async_world &&
      aegp_world_get_size(async_world, &async_width, &async_height) == 0 &&
      aegp_world_get_base_addr16(async_world, &async_pixels) == 0 && async_pixels;
  if (async_pixels)
    async_hash = sha256_bytes(static_cast<const unsigned char*>(async_pixels),
        static_cast<std::size_t>(async_width) * async_height * 8);
  ok = ok && async_hash == downstream_hash &&
      checkin_frame(async_result.receipt) == 0;
  g_aegp_effect_live = false;
  rejected = reinterpret_cast<void*>(1);
  ok = ok && render_checkout_layer_v5(upstream, nullptr, nullptr, &rejected) != 0 &&
      rejected == nullptr;
  ok = ok && dispose_layer_render_options(upstream) == 0 &&
      dispose_layer_render_options(all) == 0 &&
      dispose_layer_render_options(downstream) == 0;
  g_loaded_effect_receipt_context = saved_context;
  g_aegp_effect_live = saved_effect_live;
  return ok && async_receipt_lifetimes_balanced() &&
      layer_created_count() == created_before + 3 &&
      layer_disposed_count() == disposed_before + 3;
}

bool verify_pf_adv_app_suite_versions() {
  const void* suite1 = nullptr;
  const void* suite2 = nullptr;
  const uint32_t acquires_before = suite_acquire_count();
  const uint32_t releases_before = suite_release_count();
  bool ok = acquire_suite("PF AE Adv App Suite", 1, &suite1) == 0 &&
      acquire_suite("PF AE Adv App Suite", 2, &suite2) == 0;
  auto* slots1 = static_cast<void* const*>(suite1);
  auto* slots2 = static_cast<void* const*>(suite2);
  ok = ok && suite1 == g_adv_app_suite1.data() && suite2 == g_adv_app_suite2.data() &&
      suite1 != suite2 && slots1 && slots2;
  if (slots1 && slots2) {
    ok = ok && std::all_of(slots1, slots1 + g_adv_app_suite1.size(),
                           [](void* callback) { return callback != nullptr; }) &&
        std::all_of(slots2, slots2 + g_adv_app_suite2.size(),
                    [](void* callback) { return callback != nullptr; });
    using UnsupportedProjectOperation = int32_t(__cdecl*)();
    using UnsupportedInfoColor = int32_t(__cdecl*)(uint32_t);
    using UnsupportedInfoText3Plus = int32_t(__cdecl*)(
        const char*, const char*, const char*, const char*, const char*);
    using InfoDrawText = int32_t(__cdecl*)(const char*, const char*);
    using InfoDrawText3 = int32_t(__cdecl*)(const char*, const char*, const char*);
    for (std::size_t slot = 0; slot < 6; ++slot)
      ok = reinterpret_cast<UnsupportedProjectOperation>(slots1[slot])() != 0 && ok;
    ok = ok && reinterpret_cast<UnsupportedInfoColor>(slots1[7])(0) != 0 &&
        reinterpret_cast<UnsupportedInfoText3Plus>(slots1[9])(
            nullptr, nullptr, nullptr, nullptr, nullptr) != 0 &&
        reinterpret_cast<InfoDrawText>(slots1[6])("suite1-line1", "suite1-line2") == 0 &&
        reinterpret_cast<InfoDrawText3>(slots1[8])(
            "suite1-line1", "suite1-line2", "suite1-line3") == 0;
  }
  ok = release_suite("PF AE Adv App Suite", 2) == 0 &&
      release_suite("PF AE Adv App Suite", 1) == 0 && ok;
  return ok && suite_acquire_count() == acquires_before + 2 &&
      suite_release_count() == releases_before + 2 && suite_leases_balanced();
}

bool verify_suite_entry_guards_and_utility13() {
  const uint32_t acquires_before = suite_acquire_count();
  const uint32_t releases_before = suite_release_count();
  const uint32_t live_before = live_suite_reference_count();
  const void* acquired = reinterpret_cast<const void*>(1);
  bool ok = acquire_suite(nullptr, 13, &acquired) != 0 && acquired == nullptr &&
      acquire_suite("AEGP Utility Suite", 13, nullptr) != 0 &&
      release_suite(nullptr, 13) != 0 && suite_acquire_count() == acquires_before &&
      suite_release_count() == releases_before && live_suite_reference_count() == live_before;
  const bool saved_mask_model_enabled = g_mask_model_enabled;
  g_mask_model_enabled = false;
  const void* utility13 = nullptr;
  const void* rejected12 = reinterpret_cast<const void*>(1);
  const void* rejected14 = reinterpret_cast<const void*>(1);
  ok = acquire_suite("AEGP Utility Suite", 13, &utility13) == 0 &&
      acquire_suite("AEGP Utility Suite", 12, &rejected12) != 0 && rejected12 == nullptr &&
      acquire_suite("AEGP Utility Suite", 14, &rejected14) != 0 && rejected14 == nullptr && ok;
  g_mask_model_enabled = saved_mask_model_enabled;
  const auto* utility = static_cast<const UtilitySuite*>(utility13);
  HWND main_window = reinterpret_cast<HWND>(static_cast<uintptr_t>(0xCDCDCDCD));
  ok = ok && utility13 == &g_utility_suite && utility13 != &g_utility_suite3 && utility &&
      std::all_of(std::begin(utility->unsupported), std::end(utility->unsupported),
                  [](void* callback) { return callback == nullptr; }) &&
      std::all_of(std::begin(utility->unsupported_tail), std::end(utility->unsupported_tail),
                  [](void* callback) { return callback == nullptr; }) &&
      utility->register_with_aegp == &register_with_aegp &&
      utility->get_main_hwnd == &get_main_hwnd &&
      utility->get_main_hwnd(nullptr) != 0 &&
      utility->get_main_hwnd(&main_window) == 0 &&
      main_window == GetDesktopWindow() &&
      release_suite("AEGP Utility Suite", 13) == 0;
  return ok && suite_acquire_count() == acquires_before + 1 &&
      suite_release_count() == releases_before + 1 && suite_leases_balanced();
}

uint32_t g_cleanup_safety_selftest_calls{};
void __cdecl cleanup_safety_selftest_fault(void*) {
  ++g_cleanup_safety_selftest_calls;
  RaiseException(EXCEPTION_ACCESS_VIOLATION, 0, 0, nullptr);
}

bool verify_render_output_safety() {
  OutputPixelBuffer output(257);
  if (!output || !output.sentinels_intact() || !output.guard_pages_intact()) return false;
  std::memset(output.data(), 0x11, output.size());
  if (!output.sentinels_intact()) return false;
  output.data()[output.size() + OutputPixelBuffer::kSentinelBytes + 16] = 0x22;
  const bool oversized_overrun_detected = !output.sentinels_intact();
  const bool reset_ok = output.reset(8193) && output.sentinels_intact() &&
      output.guard_pages_intact();

  g_cleanup_safety_selftest_calls = 0;
  g_last_seh_selector.clear();
  g_last_seh_error = 0;
  const int32_t original_render_error = -37;
  const int32_t cleanup_error = invoke_smart_pre_render_cleanup_seh(
      &cleanup_safety_selftest_fault, reinterpret_cast<void*>(1));
  return oversized_overrun_detected && reset_ok && cleanup_error == 512 &&
      original_render_error == -37 && g_cleanup_safety_selftest_calls == 1 &&
      g_last_seh_selector == "SMART_PRE_RENDER_CLEANUP" && g_last_seh_error == 512;
}

bool verify_legacy_effect_compat_suites() {
  const void* comp_suite = nullptr;
  const void* interface_suite = nullptr;
  const void* helper_suite = nullptr;
  bool ok = aexcompat::pf_helper::selftest() &&
      acquire_suite("AEGP Comp Suite", 21, &comp_suite) == 0 &&
      comp_suite == g_aegp_comp_suite10.data() &&
      acquire_suite("AEGP PF Interface Suite", 1, &interface_suite) == 0 &&
      interface_suite == &g_pf_interface_suite &&
      acquire_suite("AE Plugin Helper Suite", 1, &helper_suite) == 0 &&
      helper_suite == aexcompat::pf_helper::suite1();

  AegpColorVal color{-1.0, -2.0, -3.0, -4.0};
  const AegpColorVal color_sentinel = color;
  ok = ok && aegp_get_comp_bg_color(&g_aegp_comp, &color) == 0 &&
      color.alpha == 1.0 && color.red == 0.0 && color.green == 0.0 && color.blue == 0.0;
  color = color_sentinel;
  ok = ok && aegp_get_comp_bg_color(nullptr, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      aegp_get_comp_bg_color(&g_aegp_comp_item, &color) != 0 &&
      std::memcmp(&color, &color_sentinel, sizeof(color)) == 0 &&
      aegp_get_comp_bg_color(&g_aegp_comp, nullptr) != 0;

  AegpTime time{77, 99};
  const AegpTime time_sentinel = time;
  ok = ok && convert_effect_to_comp_time(&g_effect, -17, 24000, &time) == 0 &&
      time.value == -17 && time.scale == 24000;
  time = time_sentinel;
  ok = ok && convert_effect_to_comp_time(nullptr, 1, 30, &time) != 0 &&
      time.value == time_sentinel.value && time.scale == time_sentinel.scale &&
      convert_effect_to_comp_time(&g_effect, 1, 0, &time) != 0 &&
      time.value == time_sentinel.value && time.scale == time_sentinel.scale &&
      convert_effect_to_comp_time(&g_effect, (std::numeric_limits<int32_t>::min)(),
                                  (std::numeric_limits<uint32_t>::max)(), &time) == 0 &&
      time.value == (std::numeric_limits<int32_t>::min)() &&
      time.scale == (std::numeric_limits<uint32_t>::max)() &&
      convert_effect_to_comp_time(&g_effect, 0, 1, nullptr) != 0;

  aexcompat::pf_helper::set_effect_tool_for_test(14);
  int32_t tool = -1;
  ok = ok && aexcompat::pf_helper::get_current_tool(&tool) == 0 && tool == kPfSuiteToolNone &&
      aexcompat::pf_helper::get_current_tool(nullptr) == kPfBadCallbackParam &&
      aexcompat::pf_helper::effect_tool_for_test() == 14;
  aexcompat::pf_helper::reset();

  ok = release_suite("AE Plugin Helper Suite", 1) == 0 && ok;
  ok = release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  ok = release_suite("AEGP Comp Suite", 21) == 0 && ok;
  return ok;
}

bool verify_aegp_get_effect_camera_case(bool smart_case) {
  const int32_t saved_camera_index = g_aegp_active_camera_layer_index;
  const bool saved_effect_live = g_pf_state_effect_live;
  const auto saved_in_points = g_aegp_layer_in_points;
  const auto saved_durations = g_aegp_layer_durations;
  reset_pf_state_effect_lifetime(&g_effect, true);
  g_aegp_active_camera_layer_index = -1;

  const AegpTime active_time{smart_case ? 45 : 15, 30};
  void* camera = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  bool ok = get_effect_camera(&g_effect, &active_time, &camera) == 0 && camera == nullptr;

  g_aegp_active_camera_layer_index = 2;
  g_aegp_layer_in_points[2] = {smart_case ? 30 : 10, 30};
  g_aegp_layer_durations[2] = {60, 30};
  camera = nullptr;
  ok = ok && get_effect_camera(&g_effect, &active_time, &camera) == 0 &&
      camera == &g_aegp_layers[2] && aegp_layer_index(camera) == 2;

  const auto unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  camera = unchanged;
  AegpTime invalid_scale{active_time.value, 0};
  AegpTime before_in{smart_case ? 29 : 9, 30};
  AegpTime after_out{smart_case ? 90 : 70, 30};
  OpaqueHostObject foreign_effect{0x464f5247};
  ok = ok && get_effect_camera(nullptr, &active_time, &camera) != 0 && camera == unchanged &&
      get_effect_camera(&foreign_effect, &active_time, &camera) != 0 && camera == unchanged &&
      get_effect_camera(&g_effect, nullptr, &camera) != 0 && camera == unchanged &&
      get_effect_camera(&g_effect, &invalid_scale, &camera) != 0 && camera == unchanged;
  camera = unchanged;
  ok = ok && get_effect_camera(&g_effect, &before_in, &camera) == 0 && camera == nullptr;
  camera = unchanged;
  ok = ok && get_effect_camera(&g_effect, &after_out, &camera) == 0 && camera == nullptr &&
      get_effect_camera(&g_effect, &active_time, nullptr) != 0;
  camera = unchanged;
  reset_pf_state_effect_lifetime(&g_effect, false);
  ok = ok && get_effect_camera(&g_effect, &active_time, &camera) != 0 && camera == unchanged;

  g_aegp_active_camera_layer_index = saved_camera_index;
  g_aegp_layer_in_points = saved_in_points;
  g_aegp_layer_durations = saved_durations;
  reset_pf_state_effect_lifetime(&g_effect, saved_effect_live);
  return ok;
}

bool verify_aegp_get_effect_camera_matrix_case(bool smart_case) {
  const bool saved_effect_live = g_pf_state_effect_live;
  const int32_t saved_width = g_full_resolution_width;
  const int32_t saved_height = g_full_resolution_height;
  reset_pf_state_effect_lifetime(&g_effect, true);
  g_full_resolution_width = smart_case ? 1920 : 640;
  g_full_resolution_height = smart_case ? 1080 : 480;
  const AegpTime time{smart_case ? 45 : 15, 30};
  AegpMatrix4 matrix{};
  double distance = -1.0;
  int16_t width = -1, height = -1;
  bool ok = get_effect_camera_matrix(&g_effect, &time, &matrix, &distance,
      &width, &height) == 0 && distance == g_full_resolution_width &&
      width == g_full_resolution_width && height == g_full_resolution_height;
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column) {
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
    }
  }

  AegpMatrix4 sentinel{};
  std::memset(&sentinel, 0x5a, sizeof(sentinel));
  matrix = sentinel;
  distance = -2.0; width = -2; height = -2;
  AegpTime invalid_time{time.value, 0};
  ok = ok && get_effect_camera_matrix(nullptr, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0 &&
      distance == -2.0 && width == -2 && height == -2 &&
      get_effect_camera_matrix(&g_effect, &invalid_time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;
  reset_pf_state_effect_lifetime(&g_effect, false);
  ok = ok && get_effect_camera_matrix(&g_effect, &time, &matrix, &distance,
      &width, &height) != 0 && std::memcmp(&matrix, &sentinel, sizeof(matrix)) == 0;

  g_full_resolution_width = saved_width;
  g_full_resolution_height = saved_height;
  reset_pf_state_effect_lifetime(&g_effect, saved_effect_live);
  return ok;
}

bool verify_aegp_resizer_3d_chain() {
  const void* layer_suite = nullptr;
  const void* stream_suite = nullptr;
  const void* comp_suite = nullptr;
  const void* item_suite = nullptr;
  const int32_t saved_camera_index = g_aegp_active_camera_layer_index;
  const int32_t saved_width = g_full_resolution_width;
  const int32_t saved_height = g_full_resolution_height;
  const auto saved_in_points = g_aegp_layer_in_points;
  const auto saved_durations = g_aegp_layer_durations;
  g_aegp_active_camera_layer_index = 2;
  g_aegp_layer_in_points[2] = {0, 30};
  g_aegp_layer_durations[2] = {300, 30};
  g_full_resolution_width = 1920;
  g_full_resolution_height = 1080;
  const AegpTime time{45, 30};

  bool ok = acquire_suite("AEGP Layer Suite", 14, &layer_suite) == 0 &&
      layer_suite == g_aegp_layer_suite8.data() &&
      g_aegp_layer_suite8[38] == reinterpret_cast<void*>(&aegp_get_layer_to_world_xform) &&
      acquire_suite("AEGP Stream Suite", 7, &stream_suite) == 0 &&
      stream_suite == g_aegp_stream_suite2.data() &&
      g_aegp_stream_suite2[16] == reinterpret_cast<void*>(&aegp_get_layer_stream_value_v2) &&
      acquire_suite("AEGP Comp Suite", 9, &comp_suite) == 0 &&
      comp_suite == g_aegp_comp_suite4.data() &&
      g_aegp_comp_suite4[1] == reinterpret_cast<void*>(&aegp_get_item_from_comp) &&
      acquire_suite("AEGP Item Suite", 10, &item_suite) == 0 &&
      item_suite == &g_aegp_legacy_item_suite6 &&
      reinterpret_cast<void**>(&g_aegp_legacy_item_suite6)[16] ==
          reinterpret_cast<void*>(&aegp_get_item_dimensions);
  AegpMatrix4 matrix{};
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &time, &matrix) == 0;
  for (std::size_t row = 0; row < 4; ++row) {
    for (std::size_t column = 0; column < 4; ++column)
      ok = ok && matrix.mat[row][column] == (row == column ? 1.0 : 0.0);
  }
  AegpLegacyStreamVal zoom{-1.0};
  int32_t type = -1;
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 11, 1,
      &time, 0, &zoom, &type) == 0 && zoom.one_d == 1920.0 && type == 5;
  void* item = nullptr;
  int32_t width = -1;
  int32_t height = -1;
  ok = ok && aegp_get_item_from_comp(&g_aegp_comp, &item) == 0 &&
      item == &g_aegp_comp_item &&
      aegp_get_item_dimensions(item, &width, &height) == 0 &&
      width == 1920 && height == 1080;

  AegpLegacyStreamVal sentinel{-2.0};
  type = -2;
  AegpTime invalid{45, 0};
  ok = ok && aegp_get_layer_stream_value_v2(&g_aegp_layers[1], 11, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2 &&
      aegp_get_layer_stream_value_v2(&g_aegp_layers[2], 10, 1,
      &time, 0, &sentinel, &type) != 0 && sentinel.one_d == -2.0 && type == -2;
  AegpMatrix4 matrix_sentinel{};
  std::memset(&matrix_sentinel, 0x5a, sizeof(matrix_sentinel));
  matrix = matrix_sentinel;
  ok = ok && aegp_get_layer_to_world_xform(&g_aegp_layers[2], &invalid, &matrix) != 0 &&
      std::memcmp(&matrix, &matrix_sentinel, sizeof(matrix)) == 0;
  void* item_sentinel = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  width = -2;
  height = -3;
  ok = ok && aegp_get_item_from_comp(&g_layer, &item_sentinel) != 0 &&
      item_sentinel == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234)) &&
      aegp_get_item_dimensions(&g_layer, &width, &height) != 0 &&
      width == -2 && height == -3;

  ok = release_suite("AEGP Item Suite", 10) == 0 && ok;
  ok = release_suite("AEGP Comp Suite", 9) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Layer Suite", 14) == 0 && ok;
  g_aegp_active_camera_layer_index = saved_camera_index;
  g_full_resolution_width = saved_width;
  g_full_resolution_height = saved_height;
  g_aegp_layer_in_points = saved_in_points;
  g_aegp_layer_durations = saved_durations;
  return ok && suite_leases_balanced();
}

bool verify_aegp_get_effect_camera() {
  const void* suite = nullptr;
  bool ok = acquire_suite("AEGP PF Interface Suite", 1, &suite) == 0 &&
      suite == &g_pf_interface_suite &&
      g_pf_interface_suite.get_effect_camera == &get_effect_camera &&
      g_pf_interface_suite.get_effect_camera_matrix == &get_effect_camera_matrix &&
      verify_aegp_get_effect_camera_case(false) &&
      verify_aegp_get_effect_camera_case(true) &&
      verify_aegp_get_effect_camera_matrix_case(false) &&
      verify_aegp_get_effect_camera_matrix_case(true);
  ok = release_suite("AEGP PF Interface Suite", 1) == 0 && ok;
  return ok && suite_leases_balanced();
}

bool verify_aegp_apply_effect() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const bool saved_live = g_aegp_effect_live;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t timestamp_before = g_render_project_timestamp.load();
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_effect_live = false;
  g_aegp_comp_idle_roundtrip_mode = true;

  const void* suite2 = nullptr;
  const void* suite3 = nullptr;
  const void* suite4 = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 2, &suite2) == 0 &&
      acquire_suite("AEGP Effect Suite", 3, &suite3) == 0 &&
      acquire_suite("AEGP Effect Suite", 4, &suite4) == 0;
  if (suite2 && suite3 && suite4) {
    ok = ok && static_cast<void* const*>(const_cast<void*>(suite2))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect) &&
        static_cast<void* const*>(const_cast<void*>(suite3))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect) &&
        static_cast<void* const*>(const_cast<void*>(suite4))[9] ==
                   reinterpret_cast<void*>(&aegp_apply_effect);
  }

  int32_t layer0_count = -1;
  int32_t layer1_count = -1;
  void* applied = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_num_effects(&g_aegp_layers[0], &layer0_count) == 0 &&
      layer0_count == 1 && aegp_get_layer_num_effects(&g_aegp_layers[1], &layer1_count) == 0 &&
      layer1_count == 0 && aegp_apply_effect(7, &g_aegp_layers[1],
          kAegpInstalledEffects[0].key, &applied) == 0 && applied &&
      applied != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_layer_num_effects(&g_aegp_layers[1], &layer1_count) == 0 &&
      layer1_count == 1;

  void* stream = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index(7, applied, 1, &stream) == 0 && stream &&
      g_aegp_transform_stream.layer == &g_aegp_layers[1] &&
      g_aegp_transform_stream.effect_instance_index == 1 &&
      g_aegp_transform_stream.effect_instance_generation ==
          g_aegp_effect_instances[1].generation &&
      aegp_dispose_stream(stream) == 0;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  const auto scene_before_failures = g_aegp_effect_instances;
  const uint32_t timestamp_after_apply = g_render_project_timestamp.load();
  ok = ok && aegp_get_new_effect_stream_by_index(8, applied, 1, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      aegp_apply_effect(7, nullptr, kAegpInstalledEffects[0].key, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], 9999, &unchanged) == 4 &&
      aegp_apply_effect(7, &g_aegp_layers[1], kAegpInstalledEffects[0].key, nullptr) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(g_aegp_effect_instances.data(), scene_before_failures.data(),
                  sizeof(g_aegp_effect_instances)) == 0 &&
      g_render_project_timestamp.load() == timestamp_after_apply;

  std::array<void*, kAegpEffectInstanceCapacity - 2> more{};
  for (std::size_t i = 0; ok && i < more.size(); ++i)
    ok = aegp_apply_effect(7, &g_aegp_layers[2], kAegpInstalledEffects[0].key,
                          &more[i]) == 0;
  unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc));
  const auto full_scene = g_aegp_effect_instances;
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[2], kAegpInstalledEffects[0].key,
                              &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x9abc)) &&
      std::memcmp(g_aegp_effect_instances.data(), full_scene.data(),
                  sizeof(g_aegp_effect_instances)) == 0;

  const void* stale = applied;
  int32_t key = -1;
  ok = ok && aegp_dispose_effect(applied) == 0 &&
      aegp_get_installed_key_from_layer_effect(const_cast<void*>(stale), &key) == 4 &&
      aegp_dispose_effect(const_cast<void*>(stale)) == 4 &&
      aegp_dispose_effect(&g_aegp_effect) == 4;
  void* reacquired = nullptr;
  ok = ok && aegp_get_layer_effect_by_index(7, &g_aegp_layers[1], 0, &reacquired) == 0 &&
      reacquired != stale && aegp_get_installed_key_from_layer_effect(reacquired, &key) == 0 &&
      key == kAegpInstalledEffects[0].key && aegp_dispose_effect(reacquired) == 0;
  for (void* lease : more) if (lease) ok = aegp_dispose_effect(lease) == 0 && ok;

  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 3) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 2) == 0 && ok;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_effect_live = saved_live;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok && timestamp_before != timestamp_after_apply && suite_leases_balanced();
}

bool verify_aegp_effect_stack() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const std::size_t saved_param_count = g_active_ui_param_count;
  g_aegp_effect_instances = {};
  g_aegp_effect_instances[0] = {
      &g_aegp_layers[0], kAegpInstalledEffects[0].key, 0, 1, 1, true};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_comp_idle_roundtrip_mode = true;
  g_active_ui_param_count = 5;

  const void* suite2 = nullptr;
  const void* suite3 = nullptr;
  const void* suite4 = nullptr;
  const void* stream_suite2 = nullptr;
  bool ok = acquire_suite("AEGP Effect Suite", 2, &suite2) == 0 &&
      acquire_suite("AEGP Effect Suite", 3, &suite3) == 0 &&
      acquire_suite("AEGP Effect Suite", 4, &suite4) == 0 &&
      acquire_suite("AEGP Stream Suite", 7, &stream_suite2) == 0;
  for (const void* suite : {suite2, suite3, suite4}) {
    if (!suite) { ok = false; continue; }
    const auto* slots = static_cast<void* const*>(const_cast<void*>(suite));
    ok = ok && slots[5] == reinterpret_cast<void*>(&aegp_set_effect_flags) &&
        slots[6] == reinterpret_cast<void*>(&aegp_reorder_effect) &&
        slots[10] == reinterpret_cast<void*>(&aegp_delete_layer_effect) &&
        slots[16] == reinterpret_cast<void*>(&aegp_duplicate_effect);
  }

  void* first = nullptr;
  void* second = nullptr;
  void* duplicate = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[1],
                 kAegpInstalledEffects[0].key, &first) == 0 &&
      aegp_apply_effect(7, &g_aegp_layers[1],
                 kAegpInstalledEffects[0].key, &second) == 0 &&
      aegp_duplicate_effect(first, &duplicate) == 0 && duplicate &&
      duplicate != reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  std::size_t first_index = 0;
  std::size_t second_index = 0;
  std::size_t duplicate_index = 0;
  ok = ok && resolve_effect_instance(first, 7, &first_index) &&
      resolve_effect_instance(second, 7, &second_index) &&
      resolve_effect_instance(duplicate, 7, &duplicate_index) &&
      g_aegp_effect_instances[first_index].stack_order == 0 &&
      g_aegp_effect_instances[duplicate_index].stack_order == 1 &&
      g_aegp_effect_instances[second_index].stack_order == 2;

  uint32_t flags = 0;
  ok = ok && aegp_set_effect_flags(duplicate, 3, 2) == 0 &&
      aegp_get_effect_flags(duplicate, &flags) == 0 && flags == 2 &&
      aegp_reorder_effect(duplicate, 2) == 0 &&
      g_aegp_effect_instances[second_index].stack_order == 1 &&
      g_aegp_effect_instances[duplicate_index].stack_order == 2;

  void* stream = nullptr;
  AegpTime time{0, 30};
  AegpStreamValue value{};
  char name[64]{};
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, duplicate, 1, &stream) == 0 &&
      aegp_get_stream_name_v2(stream, 1, name) == 0 &&
      std::strcmp(name, "Amount") == 0 &&
      aegp_get_new_stream_value_v2(8, stream, 1, &time, 0, &value) == 4 &&
      aegp_get_new_stream_value_v2(7, stream, 1, &time, 0, &value) == 0;
  double changed = 62.745098;
  std::memcpy(value.value.data(), &changed, sizeof(changed));
  ok = ok && aegp_set_stream_value_v2(7, stream, &value) == 0 &&
      aegp_dispose_stream_value_v2(&value) == 0 &&
      std::abs(g_aegp_effect_instances[duplicate_index].parameter_values[0][0] -
               changed) < 0.000001 &&
      aegp_delete_layer_effect(duplicate) == 0;
  int32_t type = -1;
  int32_t count = -1;
  ok = ok && aegp_get_stream_type_v2(stream, &type) == 4 &&
      aegp_get_layer_num_effects(&g_aegp_layers[1], &count) == 0 && count == 2 &&
      aegp_dispose_stream_v2(stream) == 0 &&
      aegp_get_effect_flags(duplicate, &flags) == 4 &&
      aegp_dispose_effect(duplicate) == 0;

  const auto snapshot = g_aegp_effect_instances;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678));
  ok = ok && aegp_reorder_effect(first, 2) == 4 &&
      aegp_set_effect_flags(first, 1, 2) == 4 &&
      aegp_duplicate_effect(nullptr, &unchanged) == 4 &&
      unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x5678)) &&
      std::memcmp(snapshot.data(), g_aegp_effect_instances.data(), sizeof(snapshot)) == 0;

  ok = aegp_dispose_effect(first) == 0 && ok;
  ok = aegp_dispose_effect(second) == 0 && ok;
  ok = release_suite("AEGP Stream Suite", 7) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 4) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 3) == 0 && ok;
  ok = release_suite("AEGP Effect Suite", 2) == 0 && ok;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  g_active_ui_param_count = saved_param_count;
  return ok && suite_leases_balanced();
}

bool verify_aegp_projector_levels() {
  const auto saved_instances = g_aegp_effect_instances;
  const auto saved_leases = g_aegp_effect_leases;
  const auto saved_streams = g_aegp_legacy_effect_streams;
  const bool saved_mode = g_aegp_comp_idle_roundtrip_mode;
  const uint32_t stream_acquires_before = g_aegp_stream_acquires;
  const uint32_t stream_disposes_before = g_aegp_stream_disposes;
  const uint32_t value_acquires_before = g_aegp_stream_value_acquires;
  const uint32_t value_disposes_before = g_aegp_stream_value_disposes;
  g_aegp_effect_instances = {};
  g_aegp_effect_leases = {};
  g_aegp_legacy_effect_streams = {};
  g_aegp_comp_idle_roundtrip_mode = true;

  bool ok = true;
  int32_t count = -1;
  int32_t key = kAegpInstalledEffectKeyNone;
  int32_t easy_levels_key = kAegpInstalledEffectKeyNone;
  std::array<char, kAegpMaxEffectCategoryNameSize> match_name{};
  ok = aegp_get_num_installed_effects(&count) == 0 && count == 3;
  for (int32_t index = 0; ok && index < count; ++index) {
    ok = aegp_get_next_installed_effect(key, &key) == 0 && key != 0 &&
         aegp_get_effect_match_name(key, match_name.data()) == 0;
    if (ok && std::strcmp(match_name.data(), "ADBE Easy Levels") == 0)
      easy_levels_key = key;
  }
  int32_t end_key = -1;
  ok = ok && easy_levels_key != 0 &&
       aegp_get_next_installed_effect(key, &end_key) == 0 && end_key == 0;

  void* effect = nullptr;
  ok = ok && aegp_apply_effect(7, &g_aegp_layers[1], easy_levels_key, &effect) == 0;
  ok = ok && aegp_get_effect_num_param_streams_v2(effect, &count) == 0 && count == 7;
  void* input = nullptr;
  void* black = nullptr;
  void* white = nullptr;
  void* black_duplicate = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 0, &input) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &black) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 5, &white) == 0 &&
       aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &black_duplicate) == 0 &&
       input != black && black != white && black != black_duplicate;

  char name[64]{};
  int32_t type = -1;
  AegpTime time{0, 30};
  AegpStreamValue input_value{}, black_value{}, white_value{}, duplicate_value{};
  ok = ok && aegp_get_stream_name_v2(input, 1, name) == 0 &&
       std::strcmp(name, "Input") == 0 && aegp_get_stream_type_v2(input, &type) == 0 &&
       type == 9 && aegp_get_new_stream_value_v2(7, input, 1, &time, 1, &input_value) == 0;
  void* input_layer = nullptr;
  std::memcpy(&input_layer, input_value.value.data(), sizeof(input_layer));
  ok = ok && input_layer == &g_aegp_layers[1] &&
       aegp_set_stream_value_v2(7, input, &input_value) == 4;

  ok = ok && aegp_get_stream_name_v2(black, 1, name) == 0 &&
       std::strcmp(name, "Input Black") == 0 && aegp_get_stream_type_v2(black, &type) == 0 &&
       type == 5 && aegp_get_new_stream_value_v2(7, black, 1, &time, 1, &black_value) == 0 &&
       aegp_get_stream_name_v2(white, 1, name) == 0 &&
       std::strcmp(name, "Input White") == 0 && aegp_get_stream_type_v2(white, &type) == 0 &&
       type == 5 && aegp_get_new_stream_value_v2(7, white, 1, &time, 1, &white_value) == 0 &&
       aegp_get_new_stream_value_v2(7, black_duplicate, 1, &time, 1, &duplicate_value) == 0;
  double black_default = -1.0, white_default = -1.0, duplicate_default = -1.0;
  std::memcpy(&black_default, black_value.value.data(), sizeof(double));
  std::memcpy(&white_default, white_value.value.data(), sizeof(double));
  std::memcpy(&duplicate_default, duplicate_value.value.data(), sizeof(double));
  const double black_changed = .62745098;
  const double white_changed = .92156862745;
  std::memcpy(black_value.value.data(), &black_changed, sizeof(double));
  std::memcpy(white_value.value.data(), &white_changed, sizeof(double));
  ok = ok && black_default == 0.0 && white_default == 1.0 && duplicate_default == 0.0 &&
       aegp_set_stream_value_v2(8, black, &black_value) == 4 &&
       aegp_set_stream_value_v2(7, black, &black_value) == 0 &&
       aegp_set_stream_value_v2(7, white, &white_value) == 0;

  const AegpStreamValue stale_black_value = black_value;
  ok = ok && aegp_dispose_stream_value_v2(&duplicate_value) == 0 &&
       aegp_dispose_stream_value_v2(&white_value) == 0 &&
       aegp_dispose_stream_value_v2(&black_value) == 0;
  AegpStreamValue black_readback{}, white_readback{};
  ok = ok && aegp_get_new_stream_value_v2(7, black_duplicate, 1, &time, 1,
                                           &black_readback) == 0 &&
       aegp_get_new_stream_value_v2(7, white, 1, &time, 1, &white_readback) == 0;
  double black_actual = 0.0, white_actual = 0.0;
  std::memcpy(&black_actual, black_readback.value.data(), sizeof(double));
  std::memcpy(&white_actual, white_readback.value.data(), sizeof(double));
  ok = ok && std::abs(black_actual - black_changed) < 1e-12 &&
       std::abs(white_actual - white_changed) < 1e-12 &&
       aegp_dispose_stream_value_v2(&white_readback) == 0 &&
       aegp_dispose_stream_value_v2(&black_readback) == 0 &&
       aegp_dispose_stream_value_v2(&input_value) == 0;
  AegpStreamValue current_black_value{};
  AegpStreamValue stale_copy = stale_black_value;
  ok = ok && aegp_get_new_stream_value_v2(7, black, 1, &time, 1,
                                           &current_black_value) == 0 &&
       aegp_dispose_stream_value_v2(&stale_copy) == 4 &&
       stale_copy.stream == black &&
       aegp_dispose_stream_value_v2(&current_black_value) == 0;

  std::array<void*, kAegpLegacyEffectStreamCapacity - 4> capacity_streams{};
  for (std::size_t index = 0; ok && index < capacity_streams.size(); ++index)
    ok = aegp_get_new_effect_stream_by_index_v2(7, effect, 4,
                                                &capacity_streams[index]) == 0;
  void* unchanged = reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &unchanged) == 4 &&
       unchanged == reinterpret_cast<void*>(static_cast<uintptr_t>(0x1234));
  for (auto iterator = capacity_streams.rbegin(); iterator != capacity_streams.rend(); ++iterator)
    if (*iterator) ok = aegp_dispose_stream_v2(*iterator) == 0 && ok;
  ok = aegp_dispose_stream_v2(black_duplicate) == 0 && ok;
  ok = aegp_dispose_stream_v2(white) == 0 && ok;
  ok = aegp_dispose_stream_v2(black) == 0 && ok;
  ok = aegp_dispose_stream_v2(input) == 0 && ok;

  void* reused_stream = nullptr;
  type = 0x12345678;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &reused_stream) == 0 &&
       reused_stream != black && aegp_get_stream_type_v2(black, &type) == 4 &&
       type == 0x12345678 && aegp_get_stream_type_v2(reused_stream, &type) == 0 &&
       type == 5 && aegp_dispose_stream_v2(reused_stream) == 0;

  void* stale_stream = nullptr;
  ok = ok && aegp_get_new_effect_stream_by_index_v2(7, effect, 4, &stale_stream) == 0 &&
       aegp_delete_layer_effect(effect) == 0;
  type = 0x12345678;
  ok = ok && aegp_get_stream_type_v2(stale_stream, &type) == 4 &&
       type == 0x12345678 && aegp_dispose_stream_v2(stale_stream) == 0 &&
       aegp_dispose_effect(effect) == 0;

  ok = ok && g_aegp_stream_acquires - stream_acquires_before ==
                 g_aegp_stream_disposes - stream_disposes_before &&
       g_aegp_stream_value_acquires - value_acquires_before ==
                 g_aegp_stream_value_disposes - value_disposes_before;
  g_aegp_effect_instances = saved_instances;
  g_aegp_effect_leases = saved_leases;
  g_aegp_legacy_effect_streams = saved_streams;
  g_aegp_comp_idle_roundtrip_mode = saved_mode;
  return ok;
}

bool set_l2_dump_worlds_dir(void*, const wchar_t* value) {
  g_dump_worlds_dir = std::filesystem::path(value ? value : L"");
  std::error_code error;
  return !g_dump_worlds_dir.empty() && std::filesystem::is_directory(g_dump_worlds_dir, error);
}

bool enable_l2_checksum_detail(void*) {
  g_output_checksum_detail = true;
  return true;
}

bool load_l2_aux_manifest(void*, const wchar_t* value) {
  if (!load_aux_manifest(std::filesystem::path(value ? value : L""), g_external_aux_set))
    return false;
  g_external_aux_loaded = true;
  return true;
}

bool parse_l2_alpha_coverage(void*, const wchar_t* value) {
  return parse_alpha_coverage_params(value ? value : L"");
}

bool load_l2_parameter_animation(void*, const wchar_t* value) {
  return load_parameter_animation(std::filesystem::path(value ? value : L""),
                                  g_parameter_timelines);
}

bool __cdecl scene_render_receipt_enabled() {
  return is_render_worker() && g_loaded_effect_receipt_context.entry != nullptr;
}

int worker_main_impl(int argc, wchar_t **argv) {
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
  if (!configure_scene_context(scene_host) || !scene_translation_unit_linked() ||
      !configure_scene_runtime_context(scene_runtime_host) ||
      !scene_runtime_translation_unit_linked() ||
      !scene_selftests_translation_unit_linked()) return 23;
  configure_validators(&validate_render_options_item, &initialize_layer_render_options);
  configure_cache_on_load_suite(&g_effect);
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
      {&g_transform_world_calls, &g_last_transform_x, &g_last_transform_y,
       &g_last_transform_opacity},
  };
  configure_pf_host_context(pf_host_context);
  if (!pf_host_context_configured()) return 72;
  if (!aexcompat::worker_runtime::pf_adv_time::configure_verification_hooks(
          {&acquire_suite, &release_suite, &suite_acquire_count,
           &suite_release_count, &suite_leases_balanced}))
    return 73;
  configure_runtime_module_hash(&sha256);
  configure_selector_dispatch_audit(&capture_module_audit_phase,
                                    &module_audit_passed);
  configure_selector_dispatch_trace(&record_selector_dispatch);
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
  g_async_layer_cancel_test_gate = is_render_worker() &&
      GetEnvironmentVariableW(L"AEXCOMPAT_TEST_ASYNC_CANCEL_GATE", cancel_gate,
                              2) == 1 && cancel_gate[0] == L'1';
  SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-render-output-safety") {
    const bool passed = verify_render_output_safety();
    std::cout << "{\"render_output_safety\":\"" << (passed ? "passed" : "failed")
              << "\",\"cleanup_selector\":\"" << escape(g_last_seh_selector)
              << "\",\"cleanup_error\":" << g_last_seh_error
              << ",\"cleanup_calls\":" << g_cleanup_safety_selftest_calls
              << ",\"guard_pages\":true,\"overrun_beyond_64_detected\":true}\n";
    return passed ? 0 : 1;
  }
  if (argc == 3 && std::wstring(argv[1]) == L"--self-test-crash-minidump") {
    // End-to-end proof that the SEH-guarded path writes a minidump when the
    // opt-in directory is set. Raises a real access violation under the same
    // __except filter production uses, then reports whether the dump landed.
    if (!aexcompat::worker_runtime::minidump::configure_directory(
            std::filesystem::path(argv[2]))) {
      std::cout << "{\"crash_minidump\":\"failed\",\"reason\":\"bad_directory\"}\n";
      return 1;
    }
    const uint32_t exception_code = selftest_trigger_guarded_crash();
    const std::filesystem::path dump_path =
        aexcompat::worker_runtime::minidump::current_process_dump_path();
    std::error_code dump_size_error;
    const auto dump_size =
        std::filesystem::file_size(dump_path, dump_size_error);
    const bool written = !dump_size_error && dump_size > 0;
    std::cout << "{\"crash_minidump\":\"" << (written ? "passed" : "failed")
              << "\",\"exception_code\":" << exception_code
              << ",\"dump_bytes\":" << (written ? dump_size : 0)
              << ",\"attempted\":"
              << (aexcompat::worker_runtime::minidump::attempted()
                      ? "true" : "false")
              << "}\n";
    return written ? 0 : 1;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-adv-time-suite1") {
    const bool passed =
        aexcompat::worker_runtime::pf_adv_time::verify_suite_versions();
    std::cout << "{\"pf_adv_time_suite_versions\":\"" << (passed ? "passed" : "failed")
              << "\",\"v1_slots\":4,\"v2_slots\":4,\"v3_slots\":4,\"v4_slots\":5,\"independent_identity\":true"
              << ",\"guard_intact\":true,\"reverse_release\":true,\"suite_leases_balanced\":"
              << (suite_leases_balanced() ? "true" : "false") << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-suite-entry-utility13") {
    const bool passed = verify_suite_entry_guards_and_utility13();
    std::cout << "{\"suite_entry_utility13\":\"" << (passed ? "passed" : "failed")
              << "\",\"null_fail_closed\":true,\"normal_effect_available\":true"
              << ",\"versions_12_14_rejected\":true,\"mask_callbacks_exposed\":false"
              << ",\"suite_leases_balanced\":"
              << (suite_leases_balanced() ? "true" : "false") << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-adv-app-suite") {
    const bool passed = verify_pf_adv_app_suite_versions();
    std::cout << "{\"pf_adv_app_suite_versions\":\""
              << (passed ? "passed" : "failed")
              << "\",\"v1_slots\":10,\"v2_slots\":11"
              << ",\"independent_identity\":true"
              << ",\"suite_leases_balanced\":"
              << (suite_leases_balanced() ? "true" : "false") << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-effect-param-union-suite4") {
    const bool passed = verify_aegp_effect_param_union_suite4();
    std::cout << "{\"aegp_effect_param_union_suite4\":\""
              << (passed ? "passed" : "failed")
              << "\",\"successful_calls\":" << g_aegp_effect_param_union_calls
              << "}\n";
    return passed ? 0 : 1;
  }
  const std::array<aexcompat::worker_runtime::selftest::SimpleCommand, 20> simple_selftests{{
      {L"--self-test-aegp-installed-effect-catalog", "aegp_installed_effect_catalog", &verify_aegp_installed_effect_catalog_suite4},
      {L"--self-test-parameter-animation", "parameter_animation_transport", &verify_parameter_animation_transport},
      {L"--self-test-pf-param-utils-suite", "pf_param_utils_suite3", &verify_pf_param_utils_suite3},
      {L"--self-test-pf-pre-checkout-result", "pf_pre_checkout_result", &verify_pre_checkout_result_contract},
      {L"--self-test-smart-runtime-concurrency", "smart_runtime_concurrency", &aexcompat::worker_runtime::smart::concurrency_self_test},
      {L"--self-test-smart-result-skipped", "smart_result_skipped", +[] {
         const SmartResult skipped{};
         return skipped.runtime && skipped.runtime->pixel_format.empty() &&
             skipped.runtime->input_checkout_request[0] == -1 &&
             skipped.runtime->map_checkout_request[0] == -1;
       }},
      {L"--self-test-pf-pixel-data", "pf_pixel_data_suite", &verify_pixel_data_suites},
      {L"--self-test-pf-fill-matte-legacy", "pf_fill_matte_legacy_callbacks", &verify_legacy_fill_matte_callbacks},
      {L"--self-test-pf-ae-channel-suite", "pf_ae_channel_suite", &verify_pf_ae_channel_suite},
      {L"--self-test-pf-color-suite", "pf_color_suite", &verify_pf_color_suite},
      {L"--self-test-pf-color-param-suite", "pf_color_param_suite", &verify_pf_color_param_suite},
      {L"--self-test-pf-iterate", "pf_iterate_suite", &verify_iterate_suites},
      {L"--self-test-world-transform-composite", "world_transform_composite_rect", &verify_world_transform_composite_rect},
      {L"--self-test-world-transform-affine", "world_transform_affine", &verify_world_transform_affine},
      {L"--self-test-world-transform-blend", "world_transform_blend", &verify_world_transform_blend},
      {L"--self-test-world-transform-transfer-mask", "world_transform_transfer_mask", &verify_world_transform_transfer_mask},
      {L"--self-test-aegp-world-suite3", "aegp_world_suite3", +[] { return verify_aegp_world_suite3() && verify_aegp_world_mfr_safety(); }},
      {L"--self-test-pf-batch-sampling-suite", "pf_batch_sampling_suite", &verify_pf_batch_sampling_suite, 35, ",\"opaque_callable_exposed\":false"},
      {L"--self-test-pf-ae-channel-native-provider", "pf_ae_channel_native_provider", &verify_pf_ae_channel_native_provider, 37, ",\"coverage_depths\":[8,16,32],\"mfr_checkouts\":2048,\"fabricated_planes\":false"},
      {L"--self-test-aegp-layer-render-options-suite2", "aegp_layer_render_options_suite2", +[] { return is_render_worker() && verify_aegp_layer_render_options_suite2(); }, 1, ",\"downstream_cycle_rejected\":true"},
  }};
  if (!(argc == 2 && std::wstring(argv[1]) ==
            L"--self-test-aegp-layer-render-options-suite2" && !is_render_worker())) {
    if (const auto selftest_exit = aexcompat::worker_runtime::selftest::dispatch_simple(
            argc, argv, simple_selftests.data(), simple_selftests.size()))
      return *selftest_exit;
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-keyframe-mutations") {
    const bool passed = verify_aegp_keyframe_suite5_mutations();
    std::cout << "{\"aegp_keyframe_mutations\":\""
              << (passed ? "passed" : "failed")
              << "\",\"mutations\":" << g_keyframe_mutations
              << ",\"ownership_rejections\":" << g_invalid_keyframe_operations
              << ",\"lifetimes_balanced\":"
              << (mask_lifetimes_balanced() ? "true" : "false") << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-layer-source-item") {
    const bool passed = verify_aegp_layer_source_item();
    std::cout << "{\"aegp_layer_source_item\":\""
              << (passed ? "passed" : "failed")
              << "\",\"successful_calls\":" << g_aegp_layer_source_item_calls
              << ",\"item_type_calls\":" << g_aegp_item_type_calls
              << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 3 && std::wstring(argv[1]) == L"--self-test-parameter-animation-sidecar") {
    std::vector<ParameterTimeline> timelines;
    const bool passed =
        load_parameter_animation(std::filesystem::path(argv[2]), timelines);
    std::cout << "{\"parameter_animation_sidecar\":\""
              << (passed ? "accepted" : "rejected")
              << "\",\"timelines\":" << timelines.size() << "}\n";
    return passed ? 0 : 3;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-path-data-hardening") {
    const bool passed = verify_pf_path_data_hardening();
    std::cout << "{\"pf_path_data_hardening\":\"" << (passed ? "passed" : "failed")
              << "\",\"created\":" << g_pf_path_preps_created
              << ",\"disposed\":" << g_pf_path_preps_disposed
              << ",\"live\":" << g_pf_path_segment_preps.size()
              << ",\"balanced\":" << (pf_path_lifetimes_balanced() ? "true" : "false")
              << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-world-registry") {
    const bool double_dispose = verify_world_double_dispose_rejected();
    const bool allocation_limit = verify_world_allocation_limit_rejected();
    const bool snapshot_atomic = verify_owned_world_snapshot_is_atomic();
    const bool concurrent_snapshot =
        verify_owned_world_snapshot_concurrent_dispose();
    const auto world_stats = aexcompat::world_registry::statistics();
    const bool passed = double_dispose && allocation_limit && snapshot_atomic &&
        concurrent_snapshot &&
        world_lifetimes_balanced() && world_stats.live_count == 0 &&
        world_stats.live_bytes == 0;
    std::cout << "{\"pf_world_registry\":\""
              << (passed ? "passed" : "failed")
              << "\",\"double_dispose_rejected\":"
              << (double_dispose ? "true" : "false")
              << ",\"allocation_limit_rejected\":"
              << (allocation_limit ? "true" : "false")
              << ",\"owned_snapshot_atomic\":"
              << (snapshot_atomic ? "true" : "false")
              << ",\"concurrent_snapshot_dispose\":"
              << (concurrent_snapshot ? "true" : "false")
              << ",\"live_count\":" << world_stats.live_count
              << ",\"live_bytes\":" << world_stats.live_bytes << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 4 && std::wstring(argv[1]) == L"--self-test-pf-ae-channel-transport" &&
      std::wstring(argv[2]) == L"--aux-manifest-v1") {
    const bool passed = verify_pf_ae_channel_transport(argv[3]);
    std::cout << "{\"pf_ae_channel_transport\":\""
              << (passed ? "passed" : "failed")
              << "\",\"row_bytes\":" << g_channel_transport_row_bytes
              << ",\"origin\":[" << g_channel_transport_origin_x << ','
              << g_channel_transport_origin_y << "]"
              << ",\"duration\":" << g_channel_transport_duration << "}\n";
    return passed ? 0 : 3;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-color-settings-suite6") {
    const bool passed = verify_pf_color_settings_suite6();
    const auto& srgb_icc = color_settings_builtin_srgb_icc();
    const auto& linear_icc = color_settings_builtin_linear_icc();
    std::size_t live_profiles = 0;
    {
      std::lock_guard<std::mutex> lock(g_color_settings_mutex);
      live_profiles = g_color_profiles.size();
    }
    uint32_t memory_created = 0;
    uint32_t memory_freed = 0;
    uint64_t memory_residual = 0;
    const auto memory_stats = aegp_memory_statistics();
    memory_created = memory_stats.created;
    memory_freed = memory_stats.freed;
    memory_residual = memory_stats.live_bytes;
    std::cout << "{\"pf_color_settings_suite6\":\"" << (passed ? "passed" : "failed")
              << "\",\"profiles_created\":" << g_color_profiles_created
              << ",\"profiles_disposed\":" << g_color_profiles_disposed
              << ",\"profiles_live\":" << live_profiles
              << ",\"invalid_operations\":" << g_invalid_color_profile_operations
              << ",\"xform_calls\":" << g_color_xform_calls
              << ",\"memory_created\":" << memory_created
              << ",\"memory_freed\":" << memory_freed
              << ",\"memory_residual_bytes\":" << memory_residual
              << ",\"memory_balanced\":" << (aegp_memory_balanced() ? "true" : "false")
              << ",\"srgb_icc_bytes\":" << srgb_icc.size()
              << ",\"srgb_icc_sha256\":\"" << sha256_bytes(srgb_icc.data(), srgb_icc.size())
              << "\",\"linear_icc_bytes\":" << linear_icc.size()
              << ",\"linear_icc_sha256\":\"" << sha256_bytes(linear_icc.data(), linear_icc.size())
              << "\",\"linear_icc_hex\":\"" << hex_bytes(linear_icc.data(), linear_icc.size())
              << "\",\"ocio_enabled\":false}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-pf-effect-sequence-data-suite") {
    const bool passed = verify_pf_effect_sequence_data_suite1();
    std::cout << "{\"pf_effect_sequence_data_suite1\":\""
              << (passed ? "passed" : "failed")
              << "\",\"borrowed_handle\":true,\"mfr_concurrent_reads\":2048"
              << ",\"live_sequences\":"
              << live_effect_sequence_count()
              << ",\"publications\":" << effect_sequence_publications()
              << ",\"invalidations\":" << effect_sequence_invalidations() << "}\n";
    return passed ? 0 : 36;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-aegp-async-receipt") {
    const bool passed = verify_aegp_async_receipts();
    std::cout << "{\"aegp_async_receipt\":\"" << (passed ? "passed" : "failed")
              << "\",\"created\":" << aexcompat::render_receipts::statistics().created
              << ",\"checked_in\":" << aexcompat::render_receipts::statistics().checked_in
              << ",\"live\":" << aexcompat::render_receipts::statistics().live_count
              << ",\"live_bytes\":" << aexcompat::render_receipts::statistics().live_bytes
              << ",\"invalid_operations\":"
              << aexcompat::render_receipts::statistics().invalid_operations
              << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-aegp-render-options-suite1") {
    const bool passed = verify_aegp_render_options_suite1();
    std::cout << "{\"aegp_render_options_suite1\":\""
              << (passed ? "passed" : "failed")
              << "\",\"created\":" << item_created_count()
              << ",\"disposed\":" << item_disposed_count()
              << ",\"live\":" << item_live_count()
              << ",\"receipts_created\":" << aexcompat::render_receipts::statistics().created
              << ",\"receipts_checked_in\":" << aexcompat::render_receipts::statistics().checked_in
              << ",\"invalid_operations\":" << item_invalid_count()
              << ",\"baseline_argb8\":[" << static_cast<int>(g_render_options_baseline8[0]) << ','
              << static_cast<int>(g_render_options_baseline8[1]) << ',' << static_cast<int>(g_render_options_baseline8[2]) << ',' << static_cast<int>(g_render_options_baseline8[3]) << ']'
              << ",\"time_argb8\":[" << static_cast<int>(g_render_options_time8[0]) << ',' << static_cast<int>(g_render_options_time8[1]) << ',' << static_cast<int>(g_render_options_time8[2]) << ',' << static_cast<int>(g_render_options_time8[3]) << ']'
              << ",\"downsample_argb8\":[" << static_cast<int>(g_render_options_downsample8[0]) << ',' << static_cast<int>(g_render_options_downsample8[1]) << ',' << static_cast<int>(g_render_options_downsample8[2]) << ',' << static_cast<int>(g_render_options_downsample8[3]) << ']'
              << ",\"roi_outside_argb8\":[0,0,0,0],\"field_excluded_argb8\":[0,0,0,0]"
              << ",\"roi_inside_argb8\":[" << static_cast<int>(g_render_options_roi_inside8[0]) << ',' << static_cast<int>(g_render_options_roi_inside8[1]) << ',' << static_cast<int>(g_render_options_roi_inside8[2]) << ',' << static_cast<int>(g_render_options_roi_inside8[3]) << ']'
              << ",\"matte_argb8\":[" << static_cast<int>(g_render_options_matte8[0]) << ',' << static_cast<int>(g_render_options_matte8[1]) << ',' << static_cast<int>(g_render_options_matte8[2]) << ',' << static_cast<int>(g_render_options_matte8[3]) << ']'
              << ",\"argb16\":[" << g_render_options_argb16[0] << ',' << g_render_options_argb16[1] << ',' << g_render_options_argb16[2] << ',' << g_render_options_argb16[3] << ']'
              << std::setprecision(17) << ",\"argb32f\":[" << g_render_options_argb32f[0] << ',' << g_render_options_argb32f[1] << ',' << g_render_options_argb32f[2] << ',' << g_render_options_argb32f[3] << ']'
              << "}\n";
    return passed ? 0 : 1;
  }
  if (argc == 2 &&
      std::wstring(argv[1]) == L"--self-test-aegp-item-staged-worlds") {
    const bool passed = verify_aegp_item_staged_worlds();
    std::cout << "{\"aegp_item_staged_worlds\":\""
              << (passed ? "passed" : "failed")
              << "\",\"immutable_stage\":true,\"reentrant_render_used\":false"
              << ",\"published\":" << g_staged_item_worlds_published
              << ",\"cache_hits\":" << g_staged_item_world_cache_hits
              << ",\"cache_misses\":" << g_staged_item_world_cache_misses
              << ",\"cycles_rejected\":" << g_staged_item_world_cycles_rejected
              << "}\n";
    return passed ? 0 : 1;
  }
  // Consume an optional trailing --minidump-v1 <dir> pair for every worker
  // kind (render, smart, and the L2 inspection/params paths below) before any
  // kind-specific, argc-exact dispatch runs. Reducing argc hides the pair from
  // those checks; the crash path is opt-in and off by default (issue #18).
  if (argc >= 3 && std::wstring(argv[argc - 2]) == L"--minidump-v1") {
    if (!aexcompat::worker_runtime::minidump::configure_directory(
            std::filesystem::path(argv[argc - 1])))
      return 3;
    SetUnhandledExceptionFilter(
        aexcompat::worker_runtime::minidump::top_level_crash_filter);
    argc -= 2;
  }
  struct InvocationState {
    bool request_mode{};
    bool audio_mode{};
    bool image_audio_mode{};
    bool image_mode{};
    bool layered_image_mode{};
    bool smart_force_cpu{};
    bool smart_opencl{};
    bool smart_directx{};
    bool smart_image_mode{};
    bool smart_layered_image_mode{};
    int32_t external_pixel_bytes{4};
    int transport_argc{};
    int image_click_argc{};
    int image_environment_argc{};
    int image_trailer_argc{};
    int image_argc{};
    int smart_image_click_argc{};
    int smart_image_environment_argc{};
    int smart_image_trailer_argc{};
    int smart_image_argc{};
    bool image_click_context{};
    bool image_draw_context{};
    bool image_render_environment{};
    bool image_spatial_context{};
    bool image_mask_context{};
    bool smart_image_click_context{};
    bool smart_image_draw_context{};
    bool smart_image_render_environment{};
    bool smart_image_spatial_context{};
    bool smart_image_mask_context{};
    bool mask_request_mode{};
    bool mask_scene_request_mode{};
    bool mask_context_request_mode{};
    bool mask_count_error_mode{};
    bool mask_count_crash_mode{};
    bool mask_double_dispose_mode{};
    bool stream_live_value_dispose_mode{};
    bool stream_metadata_ownership_mode{};
    bool keyframe_ownership_mode{};
    bool dynamic_stream_tree_mode{};
    bool aegp_memory_strings_mode{};
    bool suite_release_without_acquire_mode{};
    bool handle_resize_while_locked_mode{};
    bool world_double_dispose_mode{};
    bool world_allocation_limit_mode{};
    bool pixel_format_registry_mode{};
    bool outline_mutation_mode{};
    bool mask_attribute_mode{};
    bool user_changed_mode{};
    bool params_only_mode{};
    bool runtime_module_authorization_mode{};
    bool external_dependencies_mode{};
    bool do_dialog_mode{};
    bool auto_dialog_mode{};
    bool adjust_cursor_mode{};
    bool draw_event_mode{};
    bool click_event_mode{};
    bool drag_event_mode{};
    bool ui_lifecycle_mode{};
    bool ui_idle_mode{};
    bool ui_keydown_mode{};
    bool ui_mouse_exited_mode{};
    bool ui_event_assignment_mode{};
    RequestedAssignments requested_parameters;
    RequestedAssignments ui_event_assignments;
    std::vector<unsigned char> external_rgba;
    std::vector<ExternalLayerInput> external_layers;
    std::filesystem::path external_output;
    std::vector<float> external_audio;
    std::filesystem::path external_audio_output;
    int32_t external_width{};
    int32_t external_height{};
    int32_t external_current_time{};
    int32_t external_time_step{1};
    int32_t external_total_time{1};
    uint32_t external_time_scale{1};
    int32_t external_audio_samples{};
    int32_t external_audio_rate{};
    int32_t click_x{101};
    int32_t click_y{101};
    int32_t drag_end_x{101};
    int32_t drag_end_y{101};
    int32_t drag_steps{};
    uint32_t keydown_code{};
    uint32_t keydown_modifiers{};
  } invocation;

  auto& request_mode = invocation.request_mode;
  auto& audio_mode = invocation.audio_mode;
  auto& image_audio_mode = invocation.image_audio_mode;
  auto& image_mode = invocation.image_mode;
  auto& layered_image_mode = invocation.layered_image_mode;
  auto& smart_force_cpu = invocation.smart_force_cpu;
  auto& smart_opencl = invocation.smart_opencl;
  auto& smart_directx = invocation.smart_directx;
  auto& smart_image_mode = invocation.smart_image_mode;
  auto& smart_layered_image_mode = invocation.smart_layered_image_mode;
  auto& external_pixel_bytes = invocation.external_pixel_bytes;
  auto& transport_argc = invocation.transport_argc;
  auto& image_click_argc = invocation.image_click_argc;
  auto& image_environment_argc = invocation.image_environment_argc;
  auto& image_trailer_argc = invocation.image_trailer_argc;
  auto& image_argc = invocation.image_argc;
  auto& smart_image_click_argc = invocation.smart_image_click_argc;
  auto& smart_image_environment_argc = invocation.smart_image_environment_argc;
  auto& smart_image_trailer_argc = invocation.smart_image_trailer_argc;
  auto& smart_image_argc = invocation.smart_image_argc;
  auto& image_click_context = invocation.image_click_context;
  auto& image_draw_context = invocation.image_draw_context;
  auto& image_render_environment = invocation.image_render_environment;
  auto& image_spatial_context = invocation.image_spatial_context;
  auto& image_mask_context = invocation.image_mask_context;
  auto& smart_image_click_context = invocation.smart_image_click_context;
  auto& smart_image_draw_context = invocation.smart_image_draw_context;
  auto& smart_image_render_environment = invocation.smart_image_render_environment;
  auto& smart_image_spatial_context = invocation.smart_image_spatial_context;
  auto& smart_image_mask_context = invocation.smart_image_mask_context;
  auto& mask_request_mode = invocation.mask_request_mode;
  auto& mask_scene_request_mode = invocation.mask_scene_request_mode;
  auto& mask_context_request_mode = invocation.mask_context_request_mode;
  auto& mask_count_error_mode = invocation.mask_count_error_mode;
  auto& mask_count_crash_mode = invocation.mask_count_crash_mode;
  auto& mask_double_dispose_mode = invocation.mask_double_dispose_mode;
  auto& stream_live_value_dispose_mode = invocation.stream_live_value_dispose_mode;
  auto& stream_metadata_ownership_mode = invocation.stream_metadata_ownership_mode;
  auto& keyframe_ownership_mode = invocation.keyframe_ownership_mode;
  auto& dynamic_stream_tree_mode = invocation.dynamic_stream_tree_mode;
  auto& aegp_memory_strings_mode = invocation.aegp_memory_strings_mode;
  auto& suite_release_without_acquire_mode = invocation.suite_release_without_acquire_mode;
  auto& handle_resize_while_locked_mode = invocation.handle_resize_while_locked_mode;
  auto& world_double_dispose_mode = invocation.world_double_dispose_mode;
  auto& world_allocation_limit_mode = invocation.world_allocation_limit_mode;
  auto& pixel_format_registry_mode = invocation.pixel_format_registry_mode;
  auto& outline_mutation_mode = invocation.outline_mutation_mode;
  auto& mask_attribute_mode = invocation.mask_attribute_mode;
  auto& user_changed_mode = invocation.user_changed_mode;
  auto& params_only_mode = invocation.params_only_mode;
  auto& runtime_module_authorization_mode = invocation.runtime_module_authorization_mode;
  auto& external_dependencies_mode = invocation.external_dependencies_mode;
  auto& do_dialog_mode = invocation.do_dialog_mode;
  auto& auto_dialog_mode = invocation.auto_dialog_mode;
  auto& adjust_cursor_mode = invocation.adjust_cursor_mode;
  auto& draw_event_mode = invocation.draw_event_mode;
  auto& click_event_mode = invocation.click_event_mode;
  auto& drag_event_mode = invocation.drag_event_mode;
  auto& ui_lifecycle_mode = invocation.ui_lifecycle_mode;
  auto& ui_idle_mode = invocation.ui_idle_mode;
  auto& ui_keydown_mode = invocation.ui_keydown_mode;
  auto& ui_mouse_exited_mode = invocation.ui_mouse_exited_mode;
  auto& ui_event_assignment_mode = invocation.ui_event_assignment_mode;
  auto& requested_parameters = invocation.requested_parameters;
  auto& ui_event_assignments = invocation.ui_event_assignments;
  auto& external_rgba = invocation.external_rgba;
  auto& external_layers = invocation.external_layers;
  auto& external_output = invocation.external_output;
  auto& external_audio = invocation.external_audio;
  auto& external_audio_output = invocation.external_audio_output;
  auto& external_width = invocation.external_width;
  auto& external_height = invocation.external_height;
  auto& external_current_time = invocation.external_current_time;
  auto& external_time_step = invocation.external_time_step;
  auto& external_total_time = invocation.external_total_time;
  auto& external_time_scale = invocation.external_time_scale;
  auto& external_audio_samples = invocation.external_audio_samples;
  auto& external_audio_rate = invocation.external_audio_rate;
  auto& click_x = invocation.click_x;
  auto& click_y = invocation.click_y;
  auto& drag_end_x = invocation.drag_end_x;
  auto& drag_end_y = invocation.drag_end_y;
  auto& drag_steps = invocation.drag_steps;
  auto& keydown_code = invocation.keydown_code;
  auto& keydown_modifiers = invocation.keydown_modifiers;

  const auto parse_requested_payload = +[](const wchar_t* text, void* context) {
    return parse_parameter_payload(text, *static_cast<RequestedAssignments*>(context));
  };

  if (is_render_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
      aexcompat::worker_runtime::request_parser::Kind::Render, argc, argv,
      {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
        load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation},
       &parse_layer_transport_key, &parse_mask_context_payload,
       &parse_spatial_context_payload, &parse_render_environment_payload,
       &requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    const auto& worker_mode = parsed.invocation.mode;
  audio_mode = worker_mode.audio_mode; image_audio_mode = worker_mode.image_audio_mode;
  external_pixel_bytes = worker_mode.external_pixel_bytes; transport_argc = worker_mode.transport_argc;
  image_click_context = worker_mode.image_click_context; image_draw_context = worker_mode.image_draw_context;
  image_click_argc = worker_mode.image_click_argc; image_render_environment = worker_mode.image_render_environment;
  image_environment_argc = worker_mode.image_environment_argc; image_spatial_context = worker_mode.image_spatial_context;
  image_trailer_argc = worker_mode.image_trailer_argc; image_mask_context = worker_mode.image_mask_context;
  image_argc = worker_mode.image_argc; layered_image_mode = worker_mode.layered_image_mode;
  image_mode = worker_mode.image_mode; request_mode = worker_mode.request_mode;
  external_rgba = parsed.invocation.rgba; external_layers = parsed.invocation.layers;
  external_output = parsed.invocation.output; external_audio = parsed.invocation.audio;
  external_audio_output = parsed.invocation.audio_output;
  external_width = parsed.invocation.width; external_height = parsed.invocation.height;
  external_current_time = parsed.invocation.current_time; external_time_step = parsed.invocation.time_step;
  external_total_time = parsed.invocation.total_time; external_time_scale = parsed.invocation.time_scale;
  external_audio_samples = parsed.invocation.audio_samples; external_audio_rate = parsed.invocation.audio_rate;
  if (worker_mode.image_click_context) {
    g_render_click_x = parsed.invocation.click_x; g_render_click_y = parsed.invocation.click_y;
    g_app_picker_color = parsed.invocation.picker_color; g_render_click_enabled = true;
  }
  if (worker_mode.image_draw_context) g_render_draw_enabled = true;
    if (image_audio_mode)
      aexcompat::host_audio::runtime().set_source(&external_audio, external_audio_samples);
  } else if (is_smart_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
        aexcompat::worker_runtime::request_parser::Kind::Smart, argc, argv,
        {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
          load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation},
         &parse_layer_transport_key, &parse_mask_context_payload,
         &parse_spatial_context_payload, &parse_render_environment_payload,
         &requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    const auto& worker_mode = parsed.invocation.mode;
  smart_force_cpu = worker_mode.force_cpu;
  smart_opencl = worker_mode.opencl;
  smart_directx = worker_mode.directx;
  external_pixel_bytes = worker_mode.external_pixel_bytes;
  smart_image_click_context = worker_mode.image_click_context;
  smart_image_draw_context = worker_mode.image_draw_context;
  smart_image_click_argc = worker_mode.image_click_argc;
  smart_image_render_environment = worker_mode.image_render_environment;
  smart_image_environment_argc = worker_mode.image_environment_argc;
  smart_image_spatial_context = worker_mode.image_spatial_context;
  smart_image_trailer_argc = worker_mode.image_trailer_argc;
  smart_image_mask_context = worker_mode.image_mask_context;
  smart_image_argc = worker_mode.image_argc;
  smart_layered_image_mode = worker_mode.layered_image_mode;
  smart_image_mode = worker_mode.image_mode;
  mask_request_mode = worker_mode.mask_request_mode;
  mask_scene_request_mode = worker_mode.mask_scene_request_mode;
  mask_context_request_mode = worker_mode.mask_context_request_mode;
  mask_count_error_mode = worker_mode.mask_count_error_mode;
  mask_count_crash_mode = worker_mode.mask_count_crash_mode;
  mask_double_dispose_mode = worker_mode.mask_double_dispose_mode;
  stream_live_value_dispose_mode = worker_mode.stream_live_value_dispose_mode;
  stream_metadata_ownership_mode = worker_mode.stream_metadata_ownership_mode;
  keyframe_ownership_mode = worker_mode.keyframe_ownership_mode;
  dynamic_stream_tree_mode = worker_mode.dynamic_stream_tree_mode;
  aegp_memory_strings_mode = worker_mode.aegp_memory_strings_mode;
  suite_release_without_acquire_mode = worker_mode.suite_release_without_acquire_mode;
  handle_resize_while_locked_mode = worker_mode.handle_resize_while_locked_mode;
  world_double_dispose_mode = worker_mode.world_double_dispose_mode;
  world_allocation_limit_mode = worker_mode.world_allocation_limit_mode;
  pixel_format_registry_mode = worker_mode.pixel_format_registry_mode;
  outline_mutation_mode = worker_mode.outline_mutation_mode;
  mask_attribute_mode = worker_mode.mask_attribute_mode;
  request_mode = worker_mode.request_mode;
  g_mask_model_enabled = worker_mode.mask_model_enabled;
  aexcompat::mask_runtime::set_fault(mask_count_error_mode
      ? aexcompat::mask_runtime::Fault::CountError
      : mask_count_crash_mode ? aexcompat::mask_runtime::Fault::CountCrash
                              : aexcompat::mask_runtime::Fault::None);
    external_rgba = parsed.invocation.rgba;
    external_layers = parsed.invocation.layers;
    external_output = parsed.invocation.output;
    external_width = parsed.invocation.width;
    external_height = parsed.invocation.height;
    external_current_time = parsed.invocation.current_time;
    external_time_step = parsed.invocation.time_step;
    external_total_time = parsed.invocation.total_time;
    external_time_scale = parsed.invocation.time_scale;
    if (smart_image_click_context) {
      g_render_click_x = parsed.invocation.click_x;
      g_render_click_y = parsed.invocation.click_y;
      g_app_picker_color = parsed.invocation.picker_color;
      g_render_click_enabled = true;
    }
    if (smart_image_draw_context) g_render_draw_enabled = true;
  } else {
  user_changed_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--user-changed";
  g_aegp_update_menu_mode = argc == 4 && std::wstring(argv[1]) == L"--aegp-update-menu";
  g_aegp_idle_mode = argc == 4 && std::wstring(argv[1]) == L"--aegp-idle";
  g_aegp_command_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-command-roundtrip";
  g_aegp_active_idle_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-active-idle-roundtrip";
  g_aegp_keyframe_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-keyframe-roundtrip";
  g_aegp_seek_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-seek-roundtrip";
  g_aegp_trim_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-trim-roundtrip";
  g_aegp_switch_roundtrip_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--aegp-switch-roundtrip";
  g_aegp_comp_idle_roundtrip_mode = argc == 4 &&
      (std::wstring(argv[1]) == L"--aegp-comp-idle-roundtrip" ||
       g_aegp_keyframe_roundtrip_mode || g_aegp_seek_roundtrip_mode ||
       g_aegp_trim_roundtrip_mode || g_aegp_switch_roundtrip_mode);
  g_aegp_init_mode = (argc == 4 && std::wstring(argv[1]) == L"--aegp-init") ||
      g_aegp_update_menu_mode || g_aegp_idle_mode || g_aegp_command_roundtrip_mode ||
      g_aegp_active_idle_roundtrip_mode || g_aegp_comp_idle_roundtrip_mode;
  params_only_mode = (argc == 4 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-params-only";
  runtime_module_authorization_mode = params_only_mode && argc == 6 &&
      std::wstring(argv[4]) == L"--runtime-module-authorization-v1";
  external_dependencies_mode = argc == 5 &&
      std::wstring(argv[1]) == L"--l2-external-dependencies";
  do_dialog_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--l2-do-dialog";
  auto_dialog_mode = argc == 4 &&
      std::wstring(argv[1]) == L"--l2-auto-dialog";
  adjust_cursor_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-adjust-cursor";
  draw_event_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-draw-event";
  click_event_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-click-event";
  drag_event_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-drag-event";
  ui_lifecycle_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-lifecycle";
  ui_idle_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-idle";
  ui_keydown_mode = (argc == 5 || argc == 6) &&
      std::wstring(argv[1]) == L"--l2-ui-keydown";
  ui_mouse_exited_mode = (argc == 4 || argc == 5) &&
      std::wstring(argv[1]) == L"--l2-ui-mouse-exited";
  ui_event_assignment_mode =
      ((adjust_cursor_mode || draw_event_mode || ui_lifecycle_mode || ui_idle_mode ||
        ui_mouse_exited_mode) && argc == 5) ||
      ((click_event_mode || drag_event_mode || ui_keydown_mode) && argc == 6);
  if (ui_event_assignment_mode &&
      !parse_parameter_payload(argv[argc - 1], ui_event_assignments)) return 3;
  if (click_event_mode) {
    float red{}, green{}, blue{}, alpha{};
    if (swscanf_s(argv[4], L"%d,%d,%f,%f,%f,%f", &click_x, &click_y,
                  &red, &green, &blue, &alpha) != 6 ||
        click_x < 0 || click_x > 8192 || click_y < 0 || click_y > 8192 ||
        !std::isfinite(red) || !std::isfinite(green) || !std::isfinite(blue) ||
        !std::isfinite(alpha) || red < 0 || red > 1 || green < 0 || green > 1 ||
        blue < 0 || blue > 1 || alpha < 0 || alpha > 1) return 3;
    g_app_picker_color = {red, green, blue, alpha};
  }
  if (drag_event_mode &&
      (swscanf_s(argv[4], L"%d,%d,%d,%d,%d", &click_x, &click_y,
                 &drag_end_x, &drag_end_y, &drag_steps) != 5 ||
       click_x < 0 || click_x > 8192 || click_y < 0 || click_y > 8192 ||
       drag_end_x < 0 || drag_end_x > 8192 || drag_end_y < 0 || drag_end_y > 8192 ||
       drag_steps < 1 || drag_steps > 32)) return 3;
  if (ui_keydown_mode &&
      (swscanf_s(argv[4], L"%d,%d,%u,%u", &click_x, &click_y,
                 &keydown_code, &keydown_modifiers) != 4 ||
       click_x < 0 || click_x > 8192 || click_y < 0 || click_y > 8192 ||
       (keydown_code & 0x3fff0000u) != 0 || keydown_modifiers > 0xffffu)) return 3;
  g_skip_about = (argc == 4 && std::wstring(argv[1]) == L"--l2-no-about") ||
      params_only_mode || external_dependencies_mode || do_dialog_mode || auto_dialog_mode || adjust_cursor_mode || draw_event_mode || click_event_mode ||
      drag_event_mode || ui_lifecycle_mode || ui_idle_mode || ui_keydown_mode ||
      ui_mouse_exited_mode;
  if (!user_changed_mode && !g_aegp_init_mode && !g_skip_about &&
      (argc != 4 || std::wstring(argv[1]) != L"--l2")) return 2;
  if (params_only_mode && argc == 6 && !runtime_module_authorization_mode) return 2;
  if (user_changed_mode) {
    try { g_user_changed_param_slot = std::stoi(argv[4]); } catch (...) { return 3; }
    if (g_user_changed_param_slot <= 0 || g_user_changed_param_slot > static_cast<int32_t>(kMaxParams)) return 3;
    if (argc == 6 && !parse_parameter_payload(argv[5], g_user_changed_parameters)) return 3;
    g_user_changed_param_requested = true;
  }
  }
  // Every rendered effect instance belongs to a layer, even when that layer has no masks.
  if (is_rendering_worker()) g_mask_model_enabled = true;
  std::string expected;
  for (const wchar_t* p = argv[3]; *p; ++p) {
    if (*p > 0x7f) return 2;
    expected.push_back(static_cast<char>(*p));
  }
  RuntimeHostHooks runtime_hooks{&sha256, &redirect_native_stdout,
                                 &restore_native_stdout};
  RuntimeAdmissionRequest runtime_request;
  runtime_request.plugin_argument = argv[2];
  runtime_request.expected_sha256 = expected;
  if (!is_rendering_worker() && runtime_module_authorization_mode) {
    runtime_request.authorize_runtime_modules = true;
    runtime_request.authorization_manifest = argv[5];
  }
  aexcompat::TraceWriter trace_writer(
      "minihost", trace_worker_label(),
      std::filesystem::path(argv[2]).filename().string());
  if (trace_writer.requested() && !trace_writer.enabled()) return 16;
  RuntimeContext runtime_context;
  const int admission_error = admit_runtime(runtime_hooks, runtime_request, runtime_context);
  if (admission_error != 0) return admission_error;
  WorkerSession session(runtime_context, &trace_writer, &g_trace_writer);
  g_plugin_file_path = session.plugin_path().wstring();
  HMODULE module = session.module();
  if (g_aegp_init_mode) {
    g_synthetic_receipt_test_mode = g_aegp_command_roundtrip_mode;
    auto aegp_entry = reinterpret_cast<AegpEntry>(GetProcAddress(module, "EntryPointFunc"));
    if (!aegp_entry) return session.finish(12);
    void* global_refcon = nullptr;
    const int32_t init_error = aegp_entry(&g_basic_suite, 24, 0, 1, &global_refcon);
    int32_t event_error = 0;
    int32_t death_error = 0;
    uint32_t hooks_invoked = 0;
    uint32_t menu_hooks_invoked = 0;
    uint32_t death_hooks_invoked = 0;
    uint32_t command_hooks_invoked = 0;
    uint32_t command_handled_count = 0;
    int32_t idle_max_sleep = -1;
    KeyframePipeProbe keyframe_probe;
    SeekPipeProbe seek_probe;
    TrimPipeProbe trim_probe;
    SwitchPipeProbe switch_probe;
    if (init_error == 0 && g_aegp_update_menu_mode) {
      const auto event = aexcompat::worker_runtime::aegp_init::dispatch_update_menu(
          global_refcon, 0);
      hooks_invoked += event.invoked;
      if (event.error != 0 && event_error == 0) event_error = event.error;
    }
    if (init_error == 0 && g_aegp_idle_mode) {
      const auto event = aexcompat::worker_runtime::aegp_init::dispatch_idle(global_refcon);
      hooks_invoked += event.invoked;
      idle_max_sleep = event.idle_max_sleep;
      if (event.error != 0 && event_error == 0) event_error = event.error;
    }
    if (init_error == 0 && g_aegp_command_roundtrip_mode) {
      if (g_aegp_inserted_commands.empty() || g_aegp_command_registrations.empty()) {
        event_error = 4;
      } else {
        const int32_t command = g_aegp_inserted_commands.front();
        for (int pass = 0; pass < 2; ++pass) {
          const auto event = aexcompat::worker_runtime::aegp_init::dispatch_command(
              global_refcon, command, 0, 0);
          command_hooks_invoked += event.invoked;
          command_handled_count += event.handled_count;
          if (event.error != 0 && event_error == 0) event_error = event.error;
        }
      }
    }
    if (init_error == 0 &&
        (g_aegp_active_idle_roundtrip_mode || g_aegp_comp_idle_roundtrip_mode)) {
      if (g_aegp_inserted_commands.empty() || g_aegp_command_registrations.empty() ||
          g_aegp_idle_registrations.empty() ||
          (g_aegp_comp_idle_roundtrip_mode && g_aegp_update_menu_registrations.empty())) {
        event_error = 4;
      } else {
        if (g_aegp_keyframe_roundtrip_mode && !keyframe_probe.start()) event_error = 4;
        if (g_aegp_seek_roundtrip_mode && !seek_probe.start()) event_error = 4;
        if (g_aegp_trim_roundtrip_mode && !trim_probe.start()) event_error = 4;
        if (g_aegp_switch_roundtrip_mode && !switch_probe.start()) event_error = 4;
        const int32_t command = g_aegp_inserted_commands.front();
        const auto dispatch_command = [&]() {
          uint8_t already_handled = 0;
          for (const auto& registration : g_aegp_command_registrations) {
            if (registration.command != 0 && registration.command != command) continue;
            uint8_t handled = 0;
            const int32_t error = registration.hook(global_refcon, registration.refcon,
                command, registration.priority, already_handled, &handled);
            ++command_hooks_invoked;
            if (error != 0 && event_error == 0) event_error = error;
            if (handled > 1 && event_error == 0) event_error = 4;
            if (handled) {
              ++command_handled_count;
              already_handled = 1;
            }
          }
          if (!already_handled && event_error == 0) event_error = 4;
        };
        dispatch_command();
        const auto dispatch_update_menu = [&]() {
          for (const auto& registration : g_aegp_update_menu_registrations) {
            const int32_t error = registration.hook(global_refcon, registration.refcon, 0);
            ++menu_hooks_invoked;
            if (error != 0 && event_error == 0) event_error = error;
          }
        };
        const int32_t idle_tick_count = g_aegp_comp_idle_roundtrip_mode ? 3 : 1;
        for (int32_t tick = 0; tick < idle_tick_count; ++tick) {
          if (g_aegp_comp_idle_roundtrip_mode &&
              !(g_aegp_seek_roundtrip_mode && tick > 1)) g_aegp_scene_frame = tick + 1;
          if (g_aegp_comp_idle_roundtrip_mode) dispatch_update_menu();
          if (g_aegp_keyframe_roundtrip_mode || g_aegp_seek_roundtrip_mode ||
              g_aegp_trim_roundtrip_mode || g_aegp_switch_roundtrip_mode) Sleep(30);
          for (const auto& registration : g_aegp_idle_registrations) {
            int32_t requested_sleep = 0;
            const int32_t error = registration.hook(
                global_refcon, registration.refcon, &requested_sleep);
            ++hooks_invoked;
            if (error != 0 && event_error == 0) event_error = error;
            if (requested_sleep < 0 || requested_sleep > 3600) {
              if (event_error == 0) event_error = 4;
            } else if (idle_max_sleep < 0 || requested_sleep < idle_max_sleep) {
              idle_max_sleep = requested_sleep;
            }
          }
          if (g_aegp_keyframe_roundtrip_mode && tick == 0) {
            for (int attempt = 0; attempt < 250 && !keyframe_probe.request_sent; ++attempt)
              Sleep(10);
            if (!keyframe_probe.request_sent && event_error == 0) event_error = 4;
          }
          if (g_aegp_seek_roundtrip_mode && tick == 0) {
            for (int attempt = 0; attempt < 250 && !seek_probe.request_sent; ++attempt)
              Sleep(10);
            if (!seek_probe.request_sent && event_error == 0) event_error = 4;
          }
          if (g_aegp_trim_roundtrip_mode && tick == 0) {
            for (int attempt = 0; attempt < 250 && !trim_probe.request_sent; ++attempt)
              Sleep(10);
            if (!trim_probe.request_sent && event_error == 0) event_error = 4;
          }
          if (g_aegp_switch_roundtrip_mode && tick == 0) {
            for (int attempt = 0; attempt < 250 && !switch_probe.request_sent; ++attempt)
              Sleep(10);
            if (!switch_probe.request_sent && event_error == 0) event_error = 4;
          }
        }
        if (g_aegp_keyframe_roundtrip_mode) {
          for (int attempt = 0; attempt < 100 && !keyframe_probe.response_received; ++attempt)
            Sleep(10);
          keyframe_probe.stop();
          if ((!keyframe_probe.response_received || !keyframe_probe.response_valid ||
               g_aegp_keyframe_time_calls != 2 || g_aegp_keyframe_value_calls != 2 ||
               g_aegp_keyframe_interpolation_calls != 2) && event_error == 0) event_error = 4;
        }
        if (g_aegp_seek_roundtrip_mode) {
          for (int attempt = 0; attempt < 100 && !seek_probe.ack_received; ++attempt)
            Sleep(10);
          seek_probe.stop();
          if ((!seek_probe.ack_received || !seek_probe.ack_valid ||
               g_aegp_item_set_current_time_calls != 1 ||
               g_aegp_item_last_set_time_value != 75 ||
               g_aegp_item_last_set_time_scale != 30 ||
               g_aegp_scene_frame != 75) && event_error == 0) event_error = 4;
        }
        if (g_aegp_trim_roundtrip_mode) {
          for (int attempt = 0; attempt < 100 && !trim_probe.ack_received; ++attempt)
            Sleep(10);
          trim_probe.stop();
          const auto& in_point = g_aegp_layer_in_points[0];
          const auto& duration = g_aegp_layer_durations[0];
          if ((!trim_probe.ack_received || !trim_probe.ack_valid ||
               g_aegp_layer_trim_set_calls != 1 || in_point.value != 30 ||
               in_point.scale != 30 || duration.value != 210 || duration.scale != 30) &&
              event_error == 0) event_error = 4;
        }
        if (g_aegp_switch_roundtrip_mode) {
          for (int attempt = 0; attempt < 100 && !switch_probe.ack_received; ++attempt)
            Sleep(10);
          switch_probe.stop();
          if ((!switch_probe.ack_received || !switch_probe.ack_valid ||
               g_aegp_layer_flag_set_calls != 4 || g_aegp_layer_flags[0] != 0x00004026u ||
               g_aegp_layer_flags[1] != 0x00000005u ||
               g_aegp_layer_flags[2] != 0x00000005u) && event_error == 0) event_error = 4;
        }
        // Disconnect external probes before OFF so target reader threads can join.
        // Always toggle OFF before unload so plug-in worker threads are joined.
        dispatch_command();
        if (g_aegp_comp_idle_roundtrip_mode) dispatch_update_menu();
      }
    }
    if (init_error == 0) {
      const auto event = aexcompat::worker_runtime::aegp_init::dispatch_death(global_refcon);
      death_hooks_invoked += event.invoked;
      if (event.error != 0 && death_error == 0) death_error = event.error;
    }
    // Capture the event-complete and terminal loaded-module sets before the
    // AEGP is unloaded so secure broker launches can validate this early path.
    // Stop and unload first. Some AEGP_SuiteHandler builds retain exactly one
    // Item Suite cache until process teardown; the isolated worker owns that
    // final reclamation, but every other outstanding lease remains a failure.
    capture_module_audit_phase();
    const bool module_audit_ok = session.shutdown_before_report();
    module = nullptr;
    const uint32_t live_suite_references = live_suite_reference_count();
    const std::string live_suite_summary = live_suite_lease_summary();
    const bool isolated_item_cache = g_aegp_active_idle_roundtrip_mode &&
        live_suite_references == 1 && live_suite_summary == "AEGP Item Suite@14=1";
    const bool isolated_comp_cache = g_aegp_comp_idle_roundtrip_mode &&
        isolated_aegp_read_cache_is_bounded();
    const bool leases_balanced = live_suite_references == 0 || isolated_item_cache || isolated_comp_cache;
    const bool effect_lifetimes_balanced = !g_aegp_effect_live &&
        !any_effect_lease_live() && g_aegp_effect_acquires == g_aegp_effect_disposes;
    const bool stream_lifetimes_balanced = !g_aegp_transform_stream.live &&
        !g_aegp_transform_stream.value_live &&
        g_aegp_stream_acquires == g_aegp_stream_disposes &&
        g_aegp_stream_value_acquires == g_aegp_stream_value_disposes;
    const bool collection_lifetimes_balanced = !g_aegp_selection.live &&
        g_aegp_collection_creates == g_aegp_collection_disposes;
    const bool aegp_memory_lifetimes_balanced = aegp_memory_balanced();
    const bool passed = init_error == 0 && event_error == 0 && death_error == 0 &&
        leases_balanced && effect_lifetimes_balanced && stream_lifetimes_balanced &&
        collection_lifetimes_balanced &&
        aegp_memory_lifetimes_balanced && async_receipt_lifetimes_balanced() &&
        module_audit_ok;
    std::cout << "{\"schema_version\":1,\"stage\":\"aegp_init\",\"status\":\""
              << (passed ? ((g_aegp_update_menu_mode || g_aegp_idle_mode || g_aegp_command_roundtrip_mode || g_aegp_active_idle_roundtrip_mode || g_aegp_comp_idle_roundtrip_mode) ? "event_completed" : "initialized") : "initialization_failed")
              << "\",\"identity_verified\":true,\"entrypoint\":\"EntryPointFunc\""
              << ",\"driver_major_version\":24,\"driver_minor_version\":0"
              << ",\"plugin_id\":1,\"init_error\":" << init_error
              << ",\"global_refcon_nonnull\":" << (global_refcon ? "true" : "false")
              << ",\"commands_created\":" << g_aegp_commands_created
              << ",\"menu_commands_inserted\":" << g_aegp_menu_commands_inserted
              << ",\"command_hooks_registered\":" << g_aegp_command_hooks
              << ",\"update_menu_hooks_registered\":" << g_aegp_update_menu_hooks
              << ",\"idle_hooks_registered\":" << g_aegp_idle_hooks
              << ",\"death_hooks_registered\":" << g_aegp_death_hooks
              << ",\"death_hooks_invoked\":" << death_hooks_invoked
              << ",\"death_error\":" << death_error
              << ",\"event_requested\":\"" << (g_aegp_update_menu_mode ? "update_menu" : (g_aegp_idle_mode ? "idle" : (g_aegp_command_roundtrip_mode ? "command_roundtrip" : (g_aegp_active_idle_roundtrip_mode ? "active_idle_roundtrip" : (g_aegp_keyframe_roundtrip_mode ? "keyframe_roundtrip" : (g_aegp_seek_roundtrip_mode ? "seek_roundtrip" : (g_aegp_trim_roundtrip_mode ? "trim_roundtrip" : (g_aegp_switch_roundtrip_mode ? "switch_roundtrip" : (g_aegp_comp_idle_roundtrip_mode ? "comp_idle_roundtrip" : "none"))))))))) << "\""
              << ",\"event_error\":" << event_error
              << ",\"hooks_invoked\":" << hooks_invoked
              << ",\"menu_hooks_invoked\":" << menu_hooks_invoked
              << ",\"scene_first_observed_frame\":" << g_aegp_first_observed_frame
              << ",\"scene_last_observed_frame\":" << g_aegp_last_observed_frame
              << ",\"scene_current_frame\":" << g_aegp_scene_frame
              << ",\"scene_layer_count\":" << g_aegp_layers.size()
              << ",\"scene_selected_layer_count\":2"
              << ",\"idle_max_sleep\":" << idle_max_sleep
              << ",\"command_hooks_invoked\":" << command_hooks_invoked
              << ",\"command_handled_count\":" << command_handled_count
              << ",\"command_enable_calls\":" << g_aegp_command_enable_calls
              << ",\"command_check_calls\":" << g_aegp_command_check_calls
              << ",\"command_checked_true_calls\":"
              << g_aegp_command_checked_true_calls
              << ",\"command_checked_false_calls\":"
              << g_aegp_command_checked_false_calls
              << ",\"item_current_time_calls\":" << g_aegp_item_current_time_calls
              << ",\"item_set_current_time_calls\":" << g_aegp_item_set_current_time_calls
              << ",\"item_last_set_time_value\":" << g_aegp_item_last_set_time_value
              << ",\"item_last_set_time_scale\":" << g_aegp_item_last_set_time_scale
              << ",\"item_name_calls\":" << g_aegp_item_name_calls
              << ",\"item_duration_calls\":" << g_aegp_item_duration_calls
              << ",\"comp_from_item_calls\":" << g_aegp_comp_from_item_calls
              << ",\"comp_framerate_calls\":" << g_aegp_comp_framerate_calls
              << ",\"layer_count_calls\":" << g_aegp_layer_count_calls
              << ",\"layer_by_index_calls\":" << g_aegp_layer_by_index_calls
              << ",\"layer_source_item_calls\":" << g_aegp_layer_source_item_calls
              << ",\"layer_id_calls\":" << g_aegp_layer_id_calls
              << ",\"layer_attribute_calls\":" << g_aegp_layer_attribute_calls
              << ",\"layer_trim_set_calls\":" << g_aegp_layer_trim_set_calls
              << ",\"layer_flag_set_calls\":" << g_aegp_layer_flag_set_calls
              << ",\"layer_1_flags\":" << g_aegp_layer_flags[0]
              << ",\"layer_2_flags\":" << g_aegp_layer_flags[1]
              << ",\"layer_3_flags\":" << g_aegp_layer_flags[2]
              << ",\"layer_1_in_point_value\":" << g_aegp_layer_in_points[0].value
              << ",\"layer_1_in_point_scale\":" << g_aegp_layer_in_points[0].scale
              << ",\"layer_1_duration_value\":" << g_aegp_layer_durations[0].value
              << ",\"layer_1_duration_scale\":" << g_aegp_layer_durations[0].scale
              << ",\"layer_name_calls\":" << g_aegp_layer_name_calls
              << ",\"effect_count_calls\":" << g_aegp_effect_count_calls
              << ",\"effect_acquires\":" << g_aegp_effect_acquires
              << ",\"effect_disposes\":" << g_aegp_effect_disposes
              << ",\"effect_metadata_calls\":" << g_aegp_effect_metadata_calls
              << ",\"effect_lifetimes_balanced\":"
              << (effect_lifetimes_balanced ? "true" : "false")
              << ",\"stream_acquires\":" << g_aegp_stream_acquires
              << ",\"stream_disposes\":" << g_aegp_stream_disposes
              << ",\"stream_value_acquires\":" << g_aegp_stream_value_acquires
              << ",\"stream_value_disposes\":" << g_aegp_stream_value_disposes
              << ",\"stream_sampled_selector_mask\":"
              << g_aegp_stream_sampled_selector_mask
              << ",\"stream_lifetimes_balanced\":"
              << (stream_lifetimes_balanced ? "true" : "false")
              << ",\"effect_param_name_calls\":" << g_aegp_effect_param_name_calls
              << ",\"effect_param_value_calls\":" << g_aegp_effect_param_value_calls
              << ",\"effect_param_union_calls\":" << g_aegp_effect_param_union_calls
              << ",\"keyframe_count_calls\":" << g_aegp_keyframe_count_calls
              << ",\"keyframed_stream_reports\":" << g_aegp_keyframed_stream_reports
              << ",\"keyframe_time_calls\":" << g_aegp_keyframe_time_calls
              << ",\"keyframe_value_calls\":" << g_aegp_keyframe_value_calls
              << ",\"keyframe_interpolation_calls\":"
              << g_aegp_keyframe_interpolation_calls
              << ",\"keyframe_pipe_connected\":"
              << (keyframe_probe.connected ? "true" : "false")
              << ",\"keyframe_pipe_request_sent\":"
              << (keyframe_probe.request_sent ? "true" : "false")
              << ",\"keyframe_pipe_response_received\":"
              << (keyframe_probe.response_received ? "true" : "false")
              << ",\"keyframe_pipe_response_valid\":"
              << (keyframe_probe.response_valid ? "true" : "false")
              << ",\"keyframe_pipe_response_bytes\":"
              << keyframe_probe.response_bytes
              << ",\"seek_pipe_connected\":" << (seek_probe.connected ? "true" : "false")
              << ",\"seek_pipe_request_sent\":" << (seek_probe.request_sent ? "true" : "false")
              << ",\"seek_pipe_ack_received\":" << (seek_probe.ack_received ? "true" : "false")
              << ",\"seek_pipe_ack_valid\":" << (seek_probe.ack_valid ? "true" : "false")
              << ",\"trim_pipe_connected\":" << (trim_probe.connected ? "true" : "false")
              << ",\"trim_pipe_request_sent\":" << (trim_probe.request_sent ? "true" : "false")
              << ",\"trim_pipe_ack_received\":" << (trim_probe.ack_received ? "true" : "false")
              << ",\"trim_pipe_ack_valid\":" << (trim_probe.ack_valid ? "true" : "false")
              << ",\"switch_pipe_connected\":" << (switch_probe.connected ? "true" : "false")
              << ",\"switch_pipe_request_sent\":" << (switch_probe.request_sent ? "true" : "false")
              << ",\"switch_pipe_ack_received\":" << (switch_probe.ack_received ? "true" : "false")
              << ",\"switch_pipe_ack_valid\":" << (switch_probe.ack_valid ? "true" : "false")
              << ",\"collection_creates\":" << g_aegp_collection_creates
              << ",\"collection_disposes\":" << g_aegp_collection_disposes
              << ",\"collection_item_reads\":" << g_aegp_collection_item_reads
              << ",\"collection_lifetimes_balanced\":"
              << (collection_lifetimes_balanced ? "true" : "false")
              << ",\"aegp_memory_created\":" << aegp_memory_statistics().created
              << ",\"aegp_memory_freed\":" << aegp_memory_statistics().freed
              << ",\"aegp_memory_lifetimes_balanced\":"
              << (aegp_memory_lifetimes_balanced ? "true" : "false")
              << ",\"suite_acquires\":" << suite_acquire_count()
              << ",\"suite_releases\":" << suite_release_count()
              << ",\"live_suite_reference_count\":" << live_suite_references
              << ",\"live_suite_leases\":\"" << live_suite_summary << "\""
              << ",\"suite_cache_reclaimed_at_process_exit\":"
              << ((isolated_item_cache || isolated_comp_cache) ? "true" : "false")
              << ",\"suite_leases_balanced\":" << (leases_balanced ? "true" : "false")
              << ",\"receipts_created\":" << aexcompat::render_receipts::statistics().created
              << ",\"receipts_checked_in\":" << aexcompat::render_receipts::statistics().checked_in
              << ",\"live_receipts\":" << aexcompat::render_receipts::statistics().live_count
              << ",\"render_performed\":"
              << (aexcompat::render_receipts::statistics().created > 0 ? "true" : "false")
              << ",\"module_audit\":" << module_audit_json() << "}\n";
    return session.finish_integrated_report(passed ? 0 : 23);
  }
  auto entry = reinterpret_cast<EffectEntry>(GetProcAddress(module, "EffectMain"));
  if (!entry) {
    const bool has_aegp_entry = GetProcAddress(module, "EntryPointFunc") != nullptr;
    std::cerr << "plugin_kind:"
              << (has_aegp_entry ? "aegp_candidate" : "unknown_no_effect_entrypoint")
              << "\n" << std::flush;
    return session.finish(12);
  }

  alignas(8) std::array<std::byte, kInSize> input{};
  alignas(8) std::array<std::byte, kOutSize> output{};
  alignas(8) std::array<std::byte, kUtilsSize> utils{};
  write(utils, kUtilsBeginSampling, &begin_sampling8);
  write(utils, kUtilsSubpixelSample, &subpixel_sample8);
  write(utils, kUtilsAreaSample, &area_sample8);
  write(utils, kUtilsEndSampling, &end_sampling8);
  write(input, 0, &checkout_param);
  write(input, 8, &checkin_param);
  write(input, kInAddParam, static_cast<AddParamCallback>(&add_param));
  write(input, 24, &abort_render);
  write(input, 32, &report_progress);
  write(input, 40, &register_custom_ui);
  write(input, 48, &checkout_layer_audio);
  write(input, 56, &checkin_layer_audio);
  write(input, 64, &get_audio_data);
  write(utils, kUtilsBlend, &blend_world);
  write(utils, kUtilsConvolve, &convolve_world);
  write(utils, kUtilsCopy, &copy_world8);
  write(utils, kUtilsFill, &fill_world8);
  write(utils, kUtilsPremultiply, &premultiply_world8);
  write(utils, kUtilsPremultiplyColor, &premultiply_color8);
  write(utils, kUtilsFill16, &fill_world16);
  write(utils, kUtilsPremultiplyColor16, &premultiply_color16);
  write(utils, kUtilsIterate, &iterate_world8);
  write(utils, kUtilsNewWorld, &legacy_new_world);
  write(utils, kUtilsDisposeWorld, &dispose_world);
  write(utils, kUtilsTransformWorld, &transform_world);
  write(utils, kUtilsAnsiCeil, &ansi_ceil);
  write(utils, kUtilsAnsiFabs, &ansi_fabs);
  write(utils, kUtilsAnsiPow, &ansi_pow);
  write(utils, kUtilsAnsiSin, &ansi_sin);
  write(utils, kUtilsAnsiSprintf, &ansi_sprintf);
  write(utils, kUtilsAnsiStrcpy, &ansi_strcpy);
  std::memcpy(utils.data() + kUtilsColorCallbacks, &g_color_suite8,
              sizeof(g_color_suite8));
  write(utils, kUtilsGetPlatformData, &get_platform_data);
  write(utils, kUtilsGetPixelData8, &get_pixel_data8);
  write(utils, kUtilsGetPixelData16, &get_pixel_data16);
  write(utils, kUtilsNewHandle, &new_handle);
  write(utils, kUtilsLockHandle, &lock_handle);
  write(utils, kUtilsUnlockHandle, &unlock_handle);
  write(utils, kUtilsDisposeHandle, &dispose_handle);
  write<void*>(input, kInUtils, utils.data());
  write<void*>(input, kInPicaBasic, &g_basic_suite);
  write<void*>(input, kInEffectRef, &g_effect);
  write<int32_t>(input, kInQuality, g_render_quality);
  write<int16_t>(input, kInVersion, kHostSpecMajor);
  write<int16_t>(input, kInVersion + sizeof(int16_t), kHostSpecMinor);
  write<uint32_t>(input, kInApplicationId, 0x46585443u);
  write<int32_t>(input, kInNumParams, 1);
  write<int32_t>(input, kInCurrentTime, 0);
  write<int32_t>(input, kInTimeStep, 1);
  write<int32_t>(input, kInLocalTimeStep, 1);
  write<uint32_t>(input, kInTimeScale, 1);
  write<int32_t>(input, 244, g_render_field);
  write<int32_t>(input, 248, g_shutter_angle);
  write<int32_t>(input, 392, g_pre_effect_source_origin_x);
  write<int32_t>(input, 396, g_pre_effect_source_origin_y);
  write<int32_t>(input, 400, g_shutter_phase);
  write<int32_t>(input, 284, g_downsample_x.numerator);
  write<uint32_t>(input, 288, g_downsample_x.denominator);
  write<int32_t>(input, 292, g_downsample_y.numerator);
  write<uint32_t>(input, 296, g_downsample_y.denominator);
  write<int32_t>(input, 300, g_pixel_aspect_ratio.numerator);
  write<uint32_t>(input, 304, g_pixel_aspect_ratio.denominator);
  std::array<std::byte, kOutSize> about_output{};
  int32_t about_error = -1;
  uint32_t about_exception_code{};
  std::string about_message;
  std::cerr << "stage:global_setup_begin\n" << std::flush;
  reset_pf_state_effect_lifetime(&g_effect, true);
  g_global_setup_active = true;
  uint32_t global_setup_exception_code{};
  const int32_t global_error = invoke_entry_seh(
      entry, kGlobalSetup, input.data(), output.data(), nullptr, nullptr, nullptr,
      &global_setup_exception_code);
  g_global_setup_active = false;
  std::cerr << "stage:global_setup_end error=" << global_error << "\n" << std::flush;
  const uint32_t advertised_out_flags = read<uint32_t>(output, kOutFlags);
  const uint32_t advertised_out_flags2 = read<uint32_t>(output, kOutFlags2);
  if (is_render_worker()) {
    aexcompat::host_audio::runtime().configure_admission(
        audio_mode, (advertised_out_flags & kOutFlagIUseAudio) != 0);
  }
  const bool image_render_supported =
      (advertised_out_flags & kOutFlagAudioEffectOnly) == 0;
  const bool nop_render_advertised =
      (advertised_out_flags & kOutFlagNopRender) != 0;
  const bool input_write_advertised =
      (advertised_out_flags & kOutFlagIWriteInputBuffer) != 0;
  const bool expand_buffer_advertised =
      (advertised_out_flags & kOutFlagIExpandBuffer) != 0;
  const bool shrink_buffer_advertised =
      (advertised_out_flags & kOutFlagIShrinkBuffer) != 0;
  const bool depth_supported = external_pixel_bytes == 4 ||
      (external_pixel_bytes == 8 && (advertised_out_flags & kOutFlagDeepColorAware) != 0) ||
      (external_pixel_bytes == 16 && (advertised_out_flags2 & kOutFlag2FloatColorAware) != 0);
  const bool smart_render_supported =
      (advertised_out_flags2 & kOutFlag2SupportsSmartRender) != 0;
  g_update_params_ui_advertised = (read<uint32_t>(output, kOutFlags) & (1u << 26)) != 0;
  g_query_dynamic_flags_advertised = (read<uint32_t>(output, kOutFlags2) & 1u) != 0;
  write<void*>(input, kInGlobalData, read<void*>(output, kOutGlobalData));
  if (!is_rendering_worker()) {
    about_error = g_skip_about ? 0 :
        (global_error == 0 ? invoke_entry_seh(
            entry, kAbout, input.data(), about_output.data(), nullptr, nullptr, nullptr,
            &about_exception_code) : -1);
    const char* about_text = reinterpret_cast<const char*>(about_output.data() + kOutMessage);
    about_message.assign(about_text, strnlen_s(about_text, 256));
  }
  std::cerr << "stage:params_setup_begin\n" << std::flush;
  uint32_t params_setup_exception_code{};
  const int32_t params_error = global_error == 0
      ? invoke_entry_seh(entry, kParamsSetup, input.data(), output.data(), nullptr,
                         nullptr, nullptr, &params_setup_exception_code) : -1;
  std::cerr << "stage:params_setup_end error=" << params_error << "\n" << std::flush;
  if (params_error == 0) observe_arbitrary_defaults(entry, input, output);
  const int32_t expected_num_params = static_cast<int32_t>(g_params.size() + 1);
  const bool parameter_count_contract_valid = params_error == 0 &&
      read<int32_t>(output, kOutNumParams) == expected_num_params;
  if (parameter_count_contract_valid)
    write<int32_t>(input, kInNumParams, expected_num_params);
  if (!is_rendering_worker() &&
      (adjust_cursor_mode || draw_event_mode || click_event_mode || drag_event_mode ||
       ui_lifecycle_mode || ui_idle_mode || ui_keydown_mode || ui_mouse_exited_mode)) {
    int32_t event_error = -1;
    int32_t cursor = 0;
    int32_t event_out_flags = 0;
    bool changed_value = false;
    const bool registered_effect_ui = (g_custom_ui_registration.events & 4u) != 0;
    const bool registered_layer_ui = (g_custom_ui_registration.events & 2u) != 0;
    const bool registered_comp_ui = (g_custom_ui_registration.events & 1u) != 0;
    if (ui_mouse_exited_mode && !registered_layer_ui && !registered_comp_ui) {
      dispose_arbitrary_defaults(entry, input, output);
      if (global_error == 0)
        invoke_global_setdown(entry, input.data(), output.data());
      return session.finish(19);
    }
    const char* event_target = drag_event_mode || ui_mouse_exited_mode ||
        (!registered_effect_ui && registered_layer_ui)
        ? "layer" : (!registered_effect_ui && !registered_layer_ui &&
                     registered_comp_ui ? "comp" : "effect_controls");
    if (ui_mouse_exited_mode && !registered_layer_ui) event_target = "comp";
    bool arbitrary_values_disposed = false;
    std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
    std::array<uintptr_t, 4> plugin_state_before_close{};
    bool lifecycle_context_stable = true;
    bool lifecycle_host_state_cleared = false;
    bool event_assignments_applied = !ui_event_assignment_mode;
    {
      std::vector<std::array<std::byte, kParamSize>> definitions(g_params.size() + 1);
      initialize_parameter_definitions(definitions);
      const bool initialized = params_error == 0 && parameter_count_contract_valid &&
          initialize_arbitrary_values(entry, input, output, definitions);
      ArbitraryValuesScope arbitrary_scope{initialized ? entry : nullptr, &input, &output,
                                            &definitions};
      event_assignments_applied = initialized &&
          (!ui_event_assignment_mode ||
           (validate_requested_assignments(ui_event_assignments) &&
            apply_requested_assignments(definitions, ui_event_assignments)));
      std::vector<void*> params(definitions.size());
      for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
      std::array<std::byte, 208> extra{};
      g_ui_context.window_type = std::strcmp(event_target, "layer") == 0 ? 1 :
          (std::strcmp(event_target, "comp") == 0 ? 0 : 2);
      PfHelperUiContextScope helper_ui_scope(g_ui_context.window_type);
      write<void*>(extra, 0, &g_ui_context_pointer);
      write<int32_t>(extra, 8, (ui_lifecycle_mode || ui_idle_mode || ui_keydown_mode ||
          ui_mouse_exited_mode) ? 0 : (draw_event_mode ? 4 :
          ((click_event_mode || drag_event_mode) ? 2 : 9)));
      if (draw_event_mode) {
        write_rect(extra.data() + 16, 203, 203);
        write<int32_t>(extra, 32, 32);
      } else if (click_event_mode || drag_event_mode) {
        write<uint32_t>(extra, 16, 1);
        write<int32_t>(extra, 20, click_y);
        write<int32_t>(extra, 24, click_x);
        write<int32_t>(extra, 28, 1);
        write<int32_t>(extra, 32, 0);
      } else {
        write<int32_t>(extra, 16, 101);
        write<int32_t>(extra, 20, 101);
        write<int32_t>(extra, 24, 0);
        write<int32_t>(extra, 28, 0);
      }
      if (g_ui_context.window_type == 2) {
        write<int32_t>(extra, 80, 1);
        write<int32_t>(extra, 84, 2);
        write_rect(extra.data() + 88, 203, 203);
      } else {
        write_rect(extra.data() + 80, 200, 200);
      }
      if (g_ui_context.window_type != 2) {
        write<int32_t>(input, 252, 200);
        write<int32_t>(input, 256, 200);
        write<void*>(extra, 128, &g_ui_context);
        write<void*>(extra, 136, &ui_transform_point);
        write<void*>(extra, 144, &ui_transform_point);
        write<void*>(extra, 168, &ui_transform_point_simple);
        write<void*>(extra, 176, &ui_transform_point_simple);
      }
      write_rect(extra.data() + 88, 203, 203);
      uint32_t exception_code = 0;
      event_error = event_assignments_applied
          ? invoke_entry_seh(entry, kEvent, input.data(), output.data(), params.data(), nullptr,
                             extra.data(), &exception_code)
          : -1;
      if (exception_code != 0) event_error = 512;
      if ((ui_lifecycle_mode || ui_idle_mode || ui_keydown_mode || ui_mouse_exited_mode) &&
          event_assignments_applied) {
        lifecycle_errors[0] = event_error;
        const std::array<int32_t, 5> lifecycle_events = ui_idle_mode
            ? std::array<int32_t, 5>{0, 1, 7, 5, 6}
            : (ui_keydown_mode ? std::array<int32_t, 5>{0, 1, 10, 5, 6}
              : (ui_mouse_exited_mode ? std::array<int32_t, 5>{0, 1, 11, 5, 6}
                               : std::array<int32_t, 5>{0, 1, 5, 6, -1}));
        const int lifecycle_event_count =
            (ui_idle_mode || ui_keydown_mode || ui_mouse_exited_mode) ? 5 : 4;
        for (int lifecycle_index = 1; lifecycle_index < lifecycle_event_count; ++lifecycle_index) {
          lifecycle_context_stable = lifecycle_context_stable &&
              read<void*>(extra, 0) == &g_ui_context_pointer &&
              g_ui_context_pointer == &g_ui_context;
          if (lifecycle_index == lifecycle_event_count - 1) {
            for (std::size_t slot = 0; slot < plugin_state_before_close.size(); ++slot)
              plugin_state_before_close[slot] =
                  static_cast<uintptr_t>(g_ui_context.plugin_state[slot]);
          }
          write<int32_t>(extra, 8, lifecycle_events[lifecycle_index]);
          if (ui_keydown_mode && lifecycle_index == 2) {
            write<uint32_t>(extra, 16, 1);
            write<int32_t>(extra, 20, click_y);
            write<int32_t>(extra, 24, click_x);
            write<uint32_t>(extra, 28, keydown_code);
            write<uint32_t>(extra, 32, keydown_modifiers);
          }
          exception_code = 0;
          lifecycle_errors[lifecycle_index] = invoke_entry_seh(entry, kEvent, input.data(),
              output.data(), params.data(), nullptr, extra.data(), &exception_code);
          if (exception_code != 0) lifecycle_errors[lifecycle_index] = 512;
        }
        event_error = 0;
        for (const int32_t lifecycle_error : lifecycle_errors) {
          if (lifecycle_error != 0) {
            event_error = lifecycle_error;
            break;
          }
        }
        for (auto& state : g_ui_context.plugin_state) state = 0;
        aexcompat::pf_helper::set_context_tool(
            g_ui_context.window_type, aexcompat::pf_helper::kExtendedToolMin);
        lifecycle_host_state_cleared = std::all_of(std::begin(g_ui_context.plugin_state),
            std::end(g_ui_context.plugin_state), [](auto state) { return state == 0; });
      }
      if (drag_event_mode && event_error == 0) {
        g_ui_drag_requested = read<uint8_t>(extra, 72) != 0;
        for (int32_t step = 1; g_ui_drag_requested && step <= drag_steps; ++step) {
          write<int32_t>(extra, 8, 3);
          write<int32_t>(extra, 20, click_y + (drag_end_y - click_y) * step / drag_steps);
          write<int32_t>(extra, 24, click_x + (drag_end_x - click_x) * step / drag_steps);
          write<uint8_t>(extra, 73, step == drag_steps ? 1 : 0);
          write<int32_t>(extra, 204, 0);
          exception_code = 0;
          event_error = invoke_entry_seh(entry, kEvent, input.data(), output.data(),
              params.data(), nullptr, extra.data(), &exception_code);
          ++g_ui_drag_calls;
          if (exception_code != 0) { event_error = 512; break; }
        }
        g_ui_drag_terminated = g_ui_drag_calls == static_cast<uint32_t>(drag_steps) &&
            read<uint8_t>(extra, 72) == 0 && read<uint8_t>(extra, 73) != 0;
      }
      cursor = adjust_cursor_mode ? read<int32_t>(extra, 28) : 0;
      event_out_flags = read<int32_t>(extra, 204);
      changed_value = definitions.size() > 1 &&
          (read<uint32_t>(definitions[1], 0) & 1u) != 0;
    }
    int32_t event_sequence_setdown_error = 0;
    if (void* sequence_data = read<void*>(output, kOutSequenceData)) {
      write<void*>(input, kInSequenceData, sequence_data);
      uint32_t exception_code = 0;
      event_sequence_setdown_error = invoke_sequence_selector(
          entry, kSequenceSetdown, input.data(), output.data(), &exception_code);
      if (exception_code != 0) event_sequence_setdown_error = 512;
      write<void*>(input, kInSequenceData, nullptr);
      write<void*>(output, kOutSequenceData, nullptr);
    }
    const auto handle_stats = statistics();
    arbitrary_values_disposed = handle_stats.created == handle_stats.disposed + 1;
    const bool defaults_disposed = dispose_arbitrary_defaults(entry, input, output);
    const int32_t event_setdown_error = global_error == 0
        ? invoke_global_setdown(entry, input.data(), output.data()) : -1;
    if (!session.prepare_protocol_report()) return session.finish(14);
    restore_native_stdout();
    const bool event_contract = (ui_lifecycle_mode || ui_idle_mode || ui_keydown_mode ||
        ui_mouse_exited_mode)
        ? std::all_of(lifecycle_errors.begin(),
              lifecycle_errors.begin() +
                  ((ui_idle_mode || ui_keydown_mode || ui_mouse_exited_mode) ? 5 : 4),
              [](int32_t error) { return error == 0; }) &&
            lifecycle_context_stable && lifecycle_host_state_cleared
        : draw_event_mode
        ? event_error == 0 && (event_out_flags & 1) != 0 &&
            (g_drawbot_paint_rect_calls + g_drawbot_fill_path_calls +
             g_drawbot_stroke_path_calls + g_overlay_stroke_path_calls) > 0 &&
            g_drawbot_fill_colors.size() == g_drawbot_fill_path_calls &&
            std::all_of(g_drawbot_fill_colors.begin(), g_drawbot_fill_colors.end(),
                [](const auto& color) { return std::all_of(color.begin(), color.end(),
                    [](float value) { return std::isfinite(value) && value >= 0 && value <= 1; }); }) &&
            g_drawbot_objects_created == g_drawbot_objects_released && g_drawbot_objects.empty() &&
            g_drawbot_invalid_operations == 0
        : drag_event_mode
            ? event_error == 0 && g_ui_drag_requested &&
                g_ui_drag_calls == static_cast<uint32_t>(drag_steps) && g_ui_drag_terminated
            : click_event_mode
            ? event_error == 0 && (event_out_flags & 9) == 9 &&
                g_app_color_picker_calls == 1 && g_app_invalidate_rect_calls == 1
            : event_error == 0 && cursor == 13;
    std::cout << "{\"schema_version\":1,\"stage\":\"custom_ui_event\",\"status\":\""
              << (event_contract && event_sequence_setdown_error == 0 &&
                  defaults_disposed && handle_lifetimes_balanced()
                  ? "event_completed" : "event_failed")
              << "\",\"event_type\":\"" << (ui_mouse_exited_mode ? "ui_mouse_exited" :
                  (ui_keydown_mode ? "ui_keydown" :
                  (ui_idle_mode ? "ui_idle" :
                  (ui_lifecycle_mode ? "ui_lifecycle" :
                  (draw_event_mode ? "draw" :
                  (drag_event_mode ? "drag_sequence" :
                   (click_event_mode ? "do_click" : "adjust_cursor")))))))
              << "\",\"event_target\":\"" << event_target
              << "\",\"event_error\":" << event_error
              << ",\"cursor\":" << cursor << ",\"event_out_flags\":" << event_out_flags
              << ",\"adv_app_info_text_calls\":" << g_adv_app_info_text_calls
              << ",\"adv_app_info_text\":\"" << escape(g_last_adv_app_info_text)
              << "\",\"arbitrary_values_disposed\":"
              << (arbitrary_values_disposed ? "true" : "false")
              << ",\"handle_lifetimes_balanced\":"
              << (handle_lifetimes_balanced() ? "true" : "false")
              << ",\"suite_leases_balanced\":"
              << (suite_leases_balanced() ? "true" : "false")
              << ",\"drawbot_paint_rect_calls\":" << g_drawbot_paint_rect_calls
              << ",\"drawbot_fill_path_calls\":" << g_drawbot_fill_path_calls
              << ",\"drawbot_stroke_path_calls\":" << g_drawbot_stroke_path_calls
              << ",\"overlay_stroke_path_calls\":" << g_overlay_stroke_path_calls
              << ",\"drawbot_objects_created\":" << g_drawbot_objects_created
              << ",\"drawbot_objects_released\":" << g_drawbot_objects_released
              << ",\"drawbot_invalid_operations\":" << g_drawbot_invalid_operations
              << ",\"drawbot_fill_color_count\":" << g_drawbot_fill_colors.size()
              << ",\"drawbot_first_fill_color\":["
              << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][0]) << ','
              << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][1]) << ','
              << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][2]) << ','
              << (g_drawbot_fill_colors.empty() ? 0.0f : g_drawbot_fill_colors[0][3]) << ']'
              << ",\"drawbot_get_drawing_ref_calls\":" << g_drawbot_get_drawing_ref_calls
              << ",\"drawbot_get_supplier_calls\":" << g_drawbot_get_supplier_calls
              << ",\"drawbot_get_surface_calls\":" << g_drawbot_get_surface_calls
              << ",\"app_get_background_color_calls\":" << g_app_get_background_color_calls
              << ",\"app_color_picker_calls\":" << g_app_color_picker_calls
              << ",\"app_invalidate_rect_calls\":" << g_app_invalidate_rect_calls
              << ",\"picker_color_rgba\":[" << g_app_picker_color[0] << ','
              << g_app_picker_color[1] << ',' << g_app_picker_color[2] << ','
              << g_app_picker_color[3] << ']'
              << ",\"invalidated_rect\":[" << g_app_invalidated_rect[0] << ','
              << g_app_invalidated_rect[1] << ',' << g_app_invalidated_rect[2] << ','
              << g_app_invalidated_rect[3] << ']'
              << ",\"changed_value\":" << (changed_value ? "true" : "false")
              << ",\"drag_requested\":" << (g_ui_drag_requested ? "true" : "false")
              << ",\"drag_calls\":" << g_ui_drag_calls
              << ",\"drag_terminated\":" << (g_ui_drag_terminated ? "true" : "false")
              << ",\"coordinate_transform_calls\":" << g_ui_coordinate_transform_calls
              << ",\"lifecycle_errors\":[" << lifecycle_errors[0] << ','
              << lifecycle_errors[1] << ',' << lifecycle_errors[2] << ','
              << lifecycle_errors[3] << ',' << lifecycle_errors[4] << ']'
              << ",\"lifecycle_context_stable\":"
              << (lifecycle_context_stable ? "true" : "false")
              << ",\"plugin_state_before_close\":[" << plugin_state_before_close[0] << ','
              << plugin_state_before_close[1] << ',' << plugin_state_before_close[2] << ','
              << plugin_state_before_close[3] << ']'
              << ",\"lifecycle_host_state_cleared\":"
              << (lifecycle_host_state_cleared ? "true" : "false")
              << ",\"keydown_code\":" << keydown_code
              << ",\"keydown_modifiers\":" << keydown_modifiers
              << ",\"event_assignments_applied\":"
              << (event_assignments_applied ? "true" : "false")
              << ",\"sequence_setdown_error\":" << event_sequence_setdown_error
              << ",\"requested_parameters\":"
              << requested_parameters_json(ui_event_assignments)
              << ",\"global_setdown_error\":" << event_setdown_error << "}\n";
    return session.finish(event_contract && event_sequence_setdown_error == 0 &&
        defaults_disposed && handle_lifetimes_balanced() && event_setdown_error == 0
            ? 0 : 20);
  }
  if (is_rendering_worker() && request_mode &&
      (params_error != 0 || !parameter_count_contract_valid ||
                       !validate_requested_assignments(requested_parameters) ||
                       !validate_external_aux_parameters())) {
    dispose_arbitrary_defaults(entry, input, output);
    if (global_error == 0)
      invoke_global_setdown(entry, input.data(), output.data());
    return session.finish(3);
  }
  aexcompat::l2mode::EarlyMode early_mode = aexcompat::l2mode::EarlyMode::None;
  if (auto_dialog_mode) early_mode = aexcompat::l2mode::EarlyMode::AutomaticDialog;
  else if (do_dialog_mode) early_mode = aexcompat::l2mode::EarlyMode::DoDialog;
  else if (external_dependencies_mode) early_mode = aexcompat::l2mode::EarlyMode::ExternalDependencies;
  else if (params_only_mode) early_mode = aexcompat::l2mode::EarlyMode::ParametersOnly;
  if (!is_rendering_worker() && early_mode != aexcompat::l2mode::EarlyMode::None) {
    EarlyModeBridge bridge{entry, &input, &output, &session, &about_message};
    const aexcompat::l2mode::Hooks hooks{
        early_mode_out_flags, early_mode_copy_sequence_data_to_input,
        early_mode_sequence_setup, early_mode_sequence_setdown, early_mode_do_dialog,
        early_mode_global_setdown, early_mode_return_message, early_mode_handle_lifetimes_balanced,
        early_mode_prepare_protocol_report, early_mode_external_dependencies,
        early_mode_handle_is_live, early_mode_handle_size, early_mode_lock_handle,
        early_mode_unlock_handle, early_mode_dispose_handle, early_mode_handle_statistics,
        early_mode_dispose_arbitrary_defaults, early_mode_report_parameters};
    const int early_result = aexcompat::l2mode::run_early_mode(
        {early_mode, &bridge, hooks, global_error, params_error,
         parameter_count_contract_valid,
         external_dependencies_mode ? argv[4] : nullptr});
    return session.finish(early_result);

  }
  if (is_render_worker() && audio_mode) {
    constexpr std::size_t kAudioGuardSamples = 8;
    constexpr float kAudioGuardValue = 1234567.0f;
    std::vector<std::array<std::byte, kParamSize>> audio_definitions(g_params.size() + 1);
    initialize_parameter_definitions(audio_definitions);
    const bool assignments_applied =
        apply_requested_assignments(audio_definitions, requested_parameters);
    std::vector<std::array<std::byte, kParamSize>> audio_values(audio_definitions.size() * 2);
    for (std::size_t index = 0; index < audio_definitions.size(); ++index) {
      audio_values[index] = audio_definitions[index];
      audio_values[index + audio_definitions.size()] = audio_definitions[index];
    }
    std::vector<void*> audio_params(audio_values.size());
    for (std::size_t index = 0; index < audio_values.size(); ++index)
      audio_params[index] = audio_values[index].data();

    write<int32_t>(input, 336, 0);
    write<int32_t>(input, 340, external_audio_samples);
    write<int32_t>(input, 344, external_audio_samples);
    write<uint32_t>(input, kInTimeScale, 44100);
    write<double>(input, 352, 44100.0);
    write<int16_t>(input, 360, 1);
    write<int16_t>(input, 362, 2);
    write<int16_t>(input, 364, 4);
    write<int32_t>(input, 368, external_audio_samples);
    write<void*>(input, 376, external_audio.data());
    aexcompat::host_audio::runtime().set_source(&external_audio, external_audio_samples);

    std::cerr << "stage:audio_setup_begin\n" << std::flush;
    const int32_t audio_setup_error = assignments_applied
        ? entry(kAudioSetup, input.data(), output.data(), audio_params.data(), nullptr, nullptr)
        : -1;
    std::cerr << "stage:audio_setup_end error=" << audio_setup_error << "\n" << std::flush;
    const int32_t output_start = read<int32_t>(output, 356);
    const int32_t output_samples = read<int32_t>(output, 360);
    const bool setup_range_valid = output_start >= 0 && output_samples >= 0 &&
        output_start <= external_audio_samples &&
        output_samples <= external_audio_samples - output_start;

    std::vector<float> guarded_output(
        kAudioGuardSamples + static_cast<std::size_t>(external_audio_samples) +
        kAudioGuardSamples, kAudioGuardValue);
    auto* audio_destination = guarded_output.data() + kAudioGuardSamples;
    if (setup_range_valid) {
      std::fill_n(audio_destination, external_audio_samples, 0.0f);
      write<double>(output, 368, 44100.0);
      write<int16_t>(output, 376, 1);
      write<int16_t>(output, 378, 2);
      write<int16_t>(output, 380, 4);
      write<int32_t>(output, 384, output_samples);
      write<void*>(output, 392, audio_destination);
    }
    std::cerr << "stage:audio_render_begin\n" << std::flush;
    const int32_t audio_render_error = audio_setup_error == 0 && setup_range_valid
        ? entry(kAudioRender, input.data(), output.data(), audio_params.data(), nullptr, nullptr)
        : -1;
    std::cerr << "stage:audio_render_end error=" << audio_render_error << "\n" << std::flush;
    std::cerr << "stage:audio_setdown_begin\n" << std::flush;
    const int32_t audio_setdown_error = audio_setup_error == 0
        ? entry(kAudioSetdown, input.data(), output.data(), audio_params.data(), nullptr, nullptr)
        : -1;
    std::cerr << "stage:audio_setdown_end error=" << audio_setdown_error << "\n" << std::flush;

    const bool guards_intact = std::all_of(guarded_output.begin(),
        guarded_output.begin() + kAudioGuardSamples,
        [=](float value) { return value == kAudioGuardValue; }) &&
        std::all_of(guarded_output.end() - kAudioGuardSamples, guarded_output.end(),
        [=](float value) { return value == kAudioGuardValue; });
    const bool samples_finite = setup_range_valid && std::all_of(
        audio_destination, audio_destination + output_samples,
        [](float value) { return std::isfinite(value); });
    const bool audio_lifetimes_balanced = audio_handle_lifetimes_balanced();
    const bool arbitrary_defaults_disposed = dispose_arbitrary_defaults(entry, input, output);
    std::cerr << "stage:global_setdown_begin\n" << std::flush;
    const int32_t audio_global_setdown_error = global_error == 0
        ? invoke_global_setdown(entry, input.data(), output.data()) : -1;
    std::cerr << "stage:global_setdown_end error=" << audio_global_setdown_error
              << "\n" << std::flush;
    const bool passed = global_error == 0 && params_error == 0 && assignments_applied &&
        audio_setup_error == 0 && audio_render_error == 0 && audio_setdown_error == 0 &&
        setup_range_valid && guards_intact && samples_finite && audio_lifetimes_balanced &&
        audio_telemetry().invalid_operations == 0 && arbitrary_defaults_disposed &&
        handle_lifetimes_balanced() && audio_global_setdown_error == 0;
    bool output_created = false;
    if (passed) {
      HANDLE file = CreateFileW(external_audio_output.c_str(), GENERIC_WRITE, 0, nullptr,
                                CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
      if (file != INVALID_HANDLE_VALUE) {
        const DWORD bytes = static_cast<DWORD>(output_samples * sizeof(float));
        DWORD written = 0;
        output_created = WriteFile(file, audio_destination, bytes, &written, nullptr) &&
            written == bytes && FlushFileBuffers(file);
        CloseHandle(file);
        if (!output_created) DeleteFileW(external_audio_output.c_str());
      }
    }
    if (!session.prepare_protocol_report()) return session.finish(14);
    restore_native_stdout();
    std::cout << "{\"schema_version\":1,\"stage\":\"audio_render\",\"status\":\""
              << (passed && output_created ? "render_completed" : "render_failed")
              << "\",\"global_setup_error\":" << global_error
              << ",\"params_setup_error\":" << params_error
              << ",\"audio_setup_error\":" << audio_setup_error
              << ",\"audio_render_error\":" << audio_render_error
              << ",\"audio_setdown_error\":" << audio_setdown_error
              << ",\"global_setdown_error\":" << audio_global_setdown_error
              << ",\"sample_rate\":44100,\"channels\":1,\"sample_format\":\"float32\""
              << ",\"input_samples\":" << external_audio_samples
              << ",\"output_start_sample\":" << output_start
              << ",\"output_samples\":" << output_samples
              << ",\"setup_range_valid\":" << (setup_range_valid ? "true" : "false")
              << ",\"guard_bytes_intact\":" << (guards_intact ? "true" : "false")
              << ",\"samples_finite\":" << (samples_finite ? "true" : "false")
              << ",\"audio_checkout_calls\":" << audio_telemetry().checkout_calls
              << ",\"audio_usage_advertised\":" << (audio_telemetry().usage_advertised ? "true" : "false")
              << ",\"audio_checkout_allowed\":" << (audio_telemetry().checkout_allowed ? "true" : "false")
              << ",\"rejected_unadvertised_audio_checkouts\":" << audio_telemetry().rejected_unadvertised_checkouts
              << ",\"rejected_audio_format_requests\":" << audio_telemetry().rejected_format_requests
              << ",\"audio_handle_exhaustions\":" << audio_telemetry().handle_exhaustions
              << ",\"peak_live_audio_handles\":" << audio_telemetry().peak_live_handles
              << ",\"audio_checkin_calls\":" << audio_telemetry().checkin_calls
              << ",\"audio_get_data_calls\":" << audio_telemetry().get_data_calls
              << ",\"invalid_audio_operations\":" << audio_telemetry().invalid_operations
              << ",\"last_audio_checkout_start_time\":" << audio_telemetry().last_checkout_start_time
              << ",\"last_audio_checkout_duration\":" << audio_telemetry().last_checkout_duration
              << ",\"last_audio_checkout_time_scale\":" << audio_telemetry().last_checkout_time_scale
              << ",\"last_audio_window_start_sample\":" << audio_telemetry().last_window_start_sample
              << ",\"last_audio_window_sample_count\":" << audio_telemetry().last_window_sample_count
              << ",\"last_audio_window_silence_samples\":" << audio_telemetry().last_window_silence_samples
              << ",\"last_audio_output_rate_fixed\":" << audio_telemetry().last_output_rate
              << ",\"last_audio_output_bytes_per_sample\":" << audio_telemetry().last_output_bytes_per_sample
              << ",\"last_audio_output_channels\":" << audio_telemetry().last_output_channels
              << ",\"last_audio_output_format\":" << audio_telemetry().last_output_format
              << ",\"last_audio_returned_sample_frames\":" << audio_telemetry().last_returned_sample_frames
              << ",\"audio_lifetimes_balanced\":"
              << (audio_lifetimes_balanced ? "true" : "false")
              << ",\"output_created\":" << (output_created ? "true" : "false") << "}\n";
    return session.finish(passed && output_created ? 0 : 20);
  }
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  bool lifecycle_data_null = false;
  bool user_changed_ok = false;
  bool conditional_ui_ok = false;
  if (!is_rendering_worker()) {
  std::vector<std::array<std::byte, kParamSize>> lifecycle_definitions(g_params.size() + 1);
  std::array<unsigned char, 4> lifecycle_pixel{255, 0, 0, 0};
  std::array<std::byte, 120> lifecycle_world{};
  write<void*>(lifecycle_world, 24, lifecycle_pixel.data());
  write<int32_t>(lifecycle_world, 32, 4); write<int32_t>(lifecycle_world, 36, 1);
  write<int32_t>(lifecycle_world, 40, 1); write_rect(lifecycle_world.data() + 44, 1, 1);
  std::memcpy(lifecycle_definitions[0].data() + 56, lifecycle_world.data(), lifecycle_world.size());
  for (std::size_t i = 0; i < g_params.size(); ++i) {
    lifecycle_definitions[i + 1] = g_params[i].raw;
    if (g_params[i].type == 1 || g_params[i].type == 7)
      write<int32_t>(lifecycle_definitions[i + 1], 56, static_cast<int32_t>(g_params[i].default_value));
    else if (g_params[i].type == 4)
      write<int32_t>(lifecycle_definitions[i + 1], 56, g_params[i].default_value != 0 ? 1 : 0);
    else if (g_params[i].type == 2)
      write<int32_t>(lifecycle_definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_value * 65536.0)));
    else if (g_params[i].type == 10)
      write<double>(lifecycle_definitions[i + 1], 56, g_params[i].default_value);
    else if (g_params[i].type == 3)
      write<int32_t>(lifecycle_definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_components[0] * 65536.0)));
    else if (g_params[i].type == 6) {
      write<int32_t>(lifecycle_definitions[i + 1], 56,
          static_cast<int32_t>(std::round(g_params[i].default_components[0] * 65536.0)));
      write<int32_t>(lifecycle_definitions[i + 1], 60,
          static_cast<int32_t>(std::round(g_params[i].default_components[1] * 65536.0)));
    } else if (g_params[i].type == 18) {
      for (int component = 0; component < 3; ++component)
        write<double>(lifecycle_definitions[i + 1], 56 + component * 8,
            g_params[i].default_components[component]);
    }
  }
  if (g_user_changed_param_requested &&
      !apply_requested_assignments(lifecycle_definitions, g_user_changed_parameters)) {
    if (global_error == 0)
      invoke_global_setdown(entry, input.data(), output.data());
    return session.finish(3);
  }
  std::vector<void*> lifecycle_params(lifecycle_definitions.size());
  for (std::size_t i = 0; i < lifecycle_definitions.size(); ++i)
    lifecycle_params[i] = lifecycle_definitions[i].data();
  struct LifecycleCheckoutDefinitionsScope {
    ~LifecycleCheckoutDefinitionsScope() {
      g_checkout_layer_definitions.clear();
      std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
      g_live_param_checkouts.clear();
    }
  } lifecycle_checkout_definitions_scope;
  g_checkout_layer_definitions.clear();
  for (std::size_t i = 0; i < lifecycle_definitions.size(); ++i)
    g_checkout_layer_definitions[static_cast<int32_t>(i)] = lifecycle_definitions[i];
  {
    std::lock_guard<std::mutex> lock(g_param_checkout_mutex);
    g_live_param_checkouts.clear();
    g_param_checkout_calls = g_param_checkin_calls = g_invalid_param_checkins = 0;
    g_automatic_param_checkins = 0;
  }
  std::cerr << "stage:sequence_setup_begin\n" << std::flush;
  lifecycle_errors[0] = params_error == 0
      ? invoke_sequence_selector(entry, kSequenceSetup, input.data(), output.data())
      : -1;
  std::cerr << "stage:sequence_setup_end error=" << lifecycle_errors[0] << "\n" << std::flush;
  write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
  if (lifecycle_errors[0] == 0 && g_user_changed_param_requested) {
    const auto offset = static_cast<std::size_t>(g_user_changed_param_slot - 1);
    if (offset >= g_params.size() || (g_params[offset].flags & (1u << 6)) == 0) {
      g_user_changed_param_error = 4;
    } else {
      std::array<std::byte, 4> changed_extra{};
      write<int32_t>(changed_extra, 0, g_user_changed_param_slot);
      g_user_changed_param_active = true;
      g_active_ui_params = lifecycle_params.data();
      g_active_ui_param_count = lifecycle_params.size();
      g_user_changed_param_error = entry(kUserChangedParam, input.data(), output.data(),
          lifecycle_params.data(), nullptr, changed_extra.data());
      g_active_ui_params = nullptr;
      g_active_ui_param_count = 0;
      g_user_changed_param_active = false;
    }
  }
  user_changed_ok = lifecycle_errors[0] == 0 &&
      (!g_user_changed_param_requested || g_user_changed_param_error == 0);
  conditional_ui_ok = user_changed_ok && dispatch_conditional_ui_selectors(
      entry, input, output, lifecycle_params.data());
  if (conditional_ui_ok) {
    for (std::size_t i = 0; i < g_params.size(); ++i)
      g_params[i].raw = lifecycle_definitions[i + 1];
  }
  std::cerr << "stage:sequence_resetup_begin\n" << std::flush;
  lifecycle_errors[1] = lifecycle_errors[0] == 0
      ? invoke_sequence_selector(entry, kSequenceResetup, input.data(), output.data()) : -1;
  std::cerr << "stage:sequence_resetup_end error=" << lifecycle_errors[1] << "\n" << std::flush;
  write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
  std::cerr << "stage:frame_setup_begin\n" << std::flush;
  lifecycle_errors[2] = lifecycle_errors[1] == 0 ? entry(kFrameSetup, input.data(), output.data(), lifecycle_params.data(), lifecycle_world.data(), nullptr) : -1;
  std::cerr << "stage:frame_setup_end error=" << lifecycle_errors[2] << "\n" << std::flush;
  write<void*>(input, kInFrameData, read<void*>(output, kOutFrameData));
  std::cerr << "stage:frame_setdown_begin\n" << std::flush;
  lifecycle_errors[3] = lifecycle_errors[2] == 0 ? entry(kFrameSetdown, input.data(), output.data(), lifecycle_params.data(), lifecycle_world.data(), nullptr) : -1;
  std::cerr << "stage:frame_setdown_end error=" << lifecycle_errors[3] << "\n" << std::flush;
  if (lifecycle_errors[3] == 0) write<void*>(output, kOutFrameData, nullptr);
  std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
  lifecycle_errors[4] = lifecycle_errors[3] == 0
      ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
  std::cerr << "stage:sequence_setdown_end error=" << lifecycle_errors[4] << "\n" << std::flush;
  if (lifecycle_errors[4] == 0) write<void*>(output, kOutSequenceData, nullptr);
  lifecycle_data_null = read<void*>(output, kOutSequenceData) == nullptr &&
      read<void*>(output, kOutFrameData) == nullptr;
  }
  std::string case_id;
  std::string input_hash;
  std::string output_hash;
  bool guards_intact = false;
  int32_t render_width = 0;
  int32_t render_height = 0;
  int32_t render_rowbytes = 0;
  std::array<int32_t, 2> thread_errors{-1, -1};
  std::array<std::string, 2> thread_hashes{};
  std::array<bool, 2> thread_guards{false, false};
  bool concurrent_render = false;
  bool persistent_sequence = false;
  bool flattened_sequence = false;
  bool copied_flattened_sequence = false;
  int32_t persistent_sequence_setup_error = -1;
  int32_t persistent_sequence_setdown_error = -1;
  std::array<int32_t, 2> persistent_frame_errors{-1, -1};
  std::array<std::string, 2> persistent_frame_hashes{};
  int32_t sequence_flatten_error = -1;
  int32_t sequence_resetup_error = -1;
  bool flattened_handle_replaced = false;
  bool resetup_handle_replaced = false;
  bool flattened_handle_host_disposed = false;
  int32_t get_flattened_sequence_data_error = -1;
  bool original_sequence_preserved = false;
  int32_t render_error = -1;
  SmartResult smart{};
  bool lifetime_fault_observed = false;
  bool suite_fault_observed = false;
  bool handle_fault_observed = false;
  bool world_fault_observed = false;
  bool pixel_format_fault_observed = false;
  bool outline_fault_observed = false;
  bool mask_attribute_fault_observed = false;
  bool stream_metadata_fault_observed = false;
  bool keyframe_fault_observed = false;
  bool dynamic_stream_fault_observed = false;
  bool aegp_memory_fault_observed = false;

  if (is_render_worker()) {
  aexcompat::worker_runtime::classic::reset_selector_diagnostic();
  case_id = request_mode ? "request" : "";
  if (!request_mode) {
    for (const wchar_t* p = argv[4]; *p; ++p) {
      if (*p > 0x7f) return session.finish(2);
      case_id.push_back(static_cast<char>(*p));
    }
  }
  concurrent_render = case_id == "threaded_default";
  persistent_sequence = case_id == "persistent_sequence";
  flattened_sequence = case_id == "flattened_sequence";
  copied_flattened_sequence = case_id == "copied_flattened_sequence";
  std::cerr << "stage:render_begin\n" << std::flush;
  render_error = !image_render_supported ? -7 : (depth_supported ? -1 : -6);
  if (params_error == 0 && image_render_supported && depth_supported && copied_flattened_sequence) {
    g_mask_model_enabled = true;
    configure_mask_scene("rectangle");
    std::cerr << "stage:sequence_setup_begin\n" << std::flush;
    persistent_sequence_setup_error = invoke_sequence_selector(
        entry, kSequenceSetup, input.data(), output.data());
    std::cerr << "stage:sequence_setup_end error=" << persistent_sequence_setup_error
              << "\n" << std::flush;
    void* original_handle = read<void*>(output, kOutSequenceData);
    write<void*>(input, kInSequenceData, original_handle);
    std::cerr << "stage:get_flattened_sequence_data_begin\n" << std::flush;
    get_flattened_sequence_data_error = persistent_sequence_setup_error == 0
        ? entry(kGetFlattenedSequenceData, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
    std::cerr << "stage:get_flattened_sequence_data_end error="
              << get_flattened_sequence_data_error << "\n" << std::flush;
    void* flattened_copy = read<void*>(output, kOutSequenceData);
    original_sequence_preserved = get_flattened_sequence_data_error == 0 &&
        original_handle && flattened_copy && original_handle != flattened_copy &&
        host_handle_is_live(original_handle) && host_handle_is_live(flattened_copy);
    if (original_sequence_preserved) {
      dispose_handle(reinterpret_cast<void**>(flattened_copy));
      flattened_handle_host_disposed = !host_handle_is_live(flattened_copy);
      write<void*>(input, kInSequenceData, original_handle);
      write<void*>(output, kOutSequenceData, original_handle);
    }
    bool frame_guards = false;
    persistent_frame_errors[0] = original_sequence_preserved && flattened_handle_host_disposed
        ? render_once(entry, input, output, "default", render_width, render_height,
                      render_rowbytes, input_hash, persistent_frame_hashes[0], frame_guards,
                      nullptr, nullptr, nullptr, 0, 0, nullptr, 0, 1, 1, 1, 4, false)
        : -1;
    guards_intact = frame_guards;
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    persistent_sequence_setdown_error = persistent_frame_errors[0] == 0
        ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_setdown_end error=" << persistent_sequence_setdown_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, nullptr);
    output_hash = persistent_frame_hashes[0];
    render_error = persistent_sequence_setup_error == 0 &&
        get_flattened_sequence_data_error == 0 && original_sequence_preserved &&
        flattened_handle_host_disposed && persistent_frame_errors[0] == 0 &&
        persistent_sequence_setdown_error == 0 ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported && flattened_sequence) {
    g_mask_model_enabled = true;
    configure_mask_scene("rectangle");
    std::cerr << "stage:sequence_setup_begin\n" << std::flush;
    persistent_sequence_setup_error = invoke_sequence_selector(
        entry, kSequenceSetup, input.data(), output.data());
    std::cerr << "stage:sequence_setup_end error=" << persistent_sequence_setup_error
              << "\n" << std::flush;
    void* unflattened_handle = read<void*>(output, kOutSequenceData);
    write<void*>(input, kInSequenceData, unflattened_handle);
    std::cerr << "stage:sequence_flatten_begin\n" << std::flush;
    sequence_flatten_error = persistent_sequence_setup_error == 0
        ? entry(kSequenceFlatten, input.data(), output.data(), nullptr, nullptr, nullptr) : -1;
    std::cerr << "stage:sequence_flatten_end error=" << sequence_flatten_error
              << "\n" << std::flush;
    void* flattened_handle = read<void*>(output, kOutSequenceData);
    flattened_handle_replaced = sequence_flatten_error == 0 && flattened_handle &&
        flattened_handle != unflattened_handle && !host_handle_is_live(unflattened_handle);
    write<void*>(input, kInSequenceData, flattened_handle);
    std::cerr << "stage:sequence_resetup_begin\n" << std::flush;
    sequence_resetup_error = flattened_handle_replaced
        ? invoke_sequence_selector(entry, kSequenceResetup, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_resetup_end error=" << sequence_resetup_error
              << "\n" << std::flush;
    void* resetup_handle = read<void*>(output, kOutSequenceData);
    resetup_handle_replaced = sequence_resetup_error == 0 && resetup_handle &&
        resetup_handle != flattened_handle && host_handle_is_live(flattened_handle) &&
        host_handle_is_live(resetup_handle);
    if (resetup_handle_replaced) {
      dispose_handle(reinterpret_cast<void**>(flattened_handle));
      flattened_handle_host_disposed = !host_handle_is_live(flattened_handle);
      write<void*>(input, kInSequenceData, resetup_handle);
    }
    bool frame_guards = false;
    persistent_frame_errors[0] = resetup_handle_replaced && flattened_handle_host_disposed
        ? render_once(entry, input, output, "default", render_width, render_height,
                      render_rowbytes, input_hash, persistent_frame_hashes[0], frame_guards,
                      nullptr, nullptr, nullptr, 0, 0, nullptr, 0, 1, 1, 1, 4, false)
        : -1;
    guards_intact = frame_guards;
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    persistent_sequence_setdown_error = persistent_frame_errors[0] == 0
        ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_setdown_end error=" << persistent_sequence_setdown_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, nullptr);
    output_hash = persistent_frame_hashes[0];
    render_error = persistent_sequence_setup_error == 0 && sequence_flatten_error == 0 &&
        sequence_resetup_error == 0 && flattened_handle_replaced &&
        resetup_handle_replaced && flattened_handle_host_disposed &&
        persistent_frame_errors[0] == 0 && persistent_sequence_setdown_error == 0 ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported && persistent_sequence) {
    std::cerr << "stage:sequence_setup_begin\n" << std::flush;
    persistent_sequence_setup_error = invoke_sequence_selector(
        entry, kSequenceSetup, input.data(), output.data());
    std::cerr << "stage:sequence_setup_end error=" << persistent_sequence_setup_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData));
    for (int frame = 0; frame < 2 && persistent_sequence_setup_error == 0; ++frame) {
      int32_t frame_width = 0, frame_height = 0, frame_rowbytes = 0;
      std::string frame_input_hash;
      bool frame_guards = false;
      persistent_frame_errors[frame] = render_once(
          entry, input, output, "default", frame_width, frame_height, frame_rowbytes,
          frame_input_hash, persistent_frame_hashes[frame], frame_guards,
          nullptr, nullptr, nullptr, 0, 0, nullptr, frame, 1, 2, 1, 4, false);
      if (frame == 0) {
        render_width = frame_width; render_height = frame_height;
        render_rowbytes = frame_rowbytes; input_hash = frame_input_hash;
      }
      guards_intact = frame == 0 ? frame_guards : guards_intact && frame_guards;
    }
    std::cerr << "stage:sequence_setdown_begin\n" << std::flush;
    persistent_sequence_setdown_error = persistent_sequence_setup_error == 0
        ? invoke_sequence_selector(entry, kSequenceSetdown, input.data(), output.data()) : -1;
    std::cerr << "stage:sequence_setdown_end error=" << persistent_sequence_setdown_error
              << "\n" << std::flush;
    write<void*>(input, kInSequenceData, nullptr);
    output_hash = persistent_frame_hashes[1];
    render_error = persistent_sequence_setup_error == 0 &&
        persistent_frame_errors[0] == 0 && persistent_frame_errors[1] == 0 &&
        persistent_sequence_setdown_error == 0 ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported && concurrent_render) {
    std::array<int32_t, 2> widths{}, heights{}, rowbytes{};
    std::array<std::string, 2> input_hashes{};
    auto run_thread = [&](std::size_t index) {
      auto thread_input = input;
      auto thread_output = output;
      thread_errors[index] = render_once(entry, thread_input, thread_output, "default",
          widths[index], heights[index], rowbytes[index], input_hashes[index],
          thread_hashes[index], thread_guards[index], nullptr);
    };
    std::thread first(run_thread, 0); std::thread second(run_thread, 1);
    first.join(); second.join();
    render_width = widths[0]; render_height = heights[0]; render_rowbytes = rowbytes[0];
    input_hash = input_hashes[0]; output_hash = thread_hashes[0];
    guards_intact = thread_guards[0] && thread_guards[1];
    render_error = thread_errors[0] == 0 && thread_errors[1] == 0 &&
        widths[0] == widths[1] && heights[0] == heights[1] && rowbytes[0] == rowbytes[1] &&
        input_hashes[0] == input_hashes[1] && thread_hashes[0] == thread_hashes[1] ? 0 : -1;
  } else if (params_error == 0 && image_render_supported && depth_supported) {
    render_error = render_once(entry, input, output, case_id, render_width, render_height,
                               render_rowbytes, input_hash, output_hash, guards_intact,
                               request_mode ? &requested_parameters : nullptr,
                               image_mode ? &external_rgba : nullptr,
                               image_mode ? &external_output : nullptr,
                               external_width, external_height,
                               layered_image_mode ? &external_layers : nullptr,
                               external_current_time, external_time_step,
                               external_total_time, external_time_scale,
                               external_pixel_bytes);
  }
  std::cerr << "stage:render_end error=" << render_error << "\n" << std::flush;
  } else if (is_smart_worker()) {
  case_id = request_mode ? (smart_force_cpu ? "request_cpu" :
      (smart_opencl ? "gpu_opencl_float32" :
       (smart_directx ? "gpu_directx_float32" : "request"))) : "";
  if (!request_mode)
    for (const wchar_t* p = argv[4]; *p; ++p) {
      if (*p > 0x7f) return session.finish(2);
      case_id.push_back(static_cast<char>(*p));
    }
  std::cerr << "stage:smart_render_begin\n" << std::flush;
  smart = params_error == 0 && image_render_supported && depth_supported &&
      smart_render_supported
      ? smart_render_once(entry, input, output, case_id,
                          request_mode ? &requested_parameters : nullptr,
                          smart_image_mode ? &external_rgba : nullptr,
                          smart_image_mode ? &external_output : nullptr,
                          external_width, external_height,
                          smart_layered_image_mode ? &external_layers : nullptr,
                          external_current_time, external_time_step,
                          external_total_time, external_time_scale,
                          external_pixel_bytes)
      : SmartResult{};
  lifetime_fault_observed = mask_double_dispose_mode
      ? verify_mask_double_dispose_rejected()
      : stream_live_value_dispose_mode
          ? verify_stream_dispose_with_live_value_rejected()
          : false;
  std::cerr << "stage:smart_render_end pre_error=" << smart.pre_error
            << " render_error=" << smart.render_error << "\n" << std::flush;
  }
  if (is_render_worker()) drain_async_layer_requests();
  const bool arbitrary_defaults_disposed = dispose_arbitrary_defaults(entry, input, output);
  std::cerr << "stage:global_setdown_begin\n" << std::flush;
  const int32_t setdown_error = global_error == 0
      ? invoke_global_setdown(entry, input.data(), output.data()) : -1;
  if (is_smart_worker() && suite_release_without_acquire_mode)
    suite_fault_observed = verify_suite_release_without_acquire_rejected();
  if (is_smart_worker() && handle_resize_while_locked_mode)
    handle_fault_observed = verify_handle_resize_while_locked_rejected();
  if (is_smart_worker() && world_double_dispose_mode)
    world_fault_observed = verify_world_double_dispose_rejected();
  if (is_smart_worker() && world_allocation_limit_mode)
    world_fault_observed = verify_world_allocation_limit_rejected();
  if (is_smart_worker() && pixel_format_registry_mode)
    pixel_format_fault_observed = verify_pixel_format_registry_rejection();
  if (is_smart_worker() && outline_mutation_mode)
    outline_fault_observed = verify_outline_mutation_rejection();
  if (is_smart_worker() && mask_attribute_mode)
    mask_attribute_fault_observed = verify_mask_attribute_and_ownership_rejection();
  if (is_smart_worker() && stream_metadata_ownership_mode)
    stream_metadata_fault_observed = verify_stream_metadata_and_ownership_rejection();
  if (is_smart_worker() && keyframe_ownership_mode)
    keyframe_fault_observed = verify_keyframe_ownership_rejection();
  if (is_smart_worker() && dynamic_stream_tree_mode)
    dynamic_stream_fault_observed = verify_dynamic_stream_tree_rejection();
  if (is_smart_worker() && aegp_memory_strings_mode)
    aegp_memory_fault_observed = verify_aegp_memory_and_strings_rejection();
  std::cerr << "stage:global_setdown_end error=" << setdown_error << "\n" << std::flush;
  if (!session.prepare_protocol_report()) return session.finish(14);
  if (is_render_worker()) {
  const auto classic_diagnostics =
      aexcompat::worker_runtime::classic::diagnostics();
  restore_native_stdout();
  aexcompat::worker_render_report::ReportSnapshot report_snapshot(std::cout);
  aexcompat::worker_render_report::ClassicReport classic_report;
  classic_report.head = {
      render_error == 0 && parameter_count_contract_valid && guards_intact &&
                arbitrary_defaults_disposed && g_invalid_arbitrary_operations == 0 &&
                handle_lifetimes_balanced() && world_lifetimes_balanced() &&
                gpu_memory_lifetimes_balanced() &&
                pf_path_lifetimes_balanced() &&
                async_receipt_lifetimes_balanced() &&
                async_layer_requests_balanced() &&
                audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
                classic_diagnostics.balanced &&
                ((!g_render_click_enabled && !g_render_draw_enabled) ||
                 g_render_ui_context_closed),
      global_error, params_error, advertised_out_flags, advertised_out_flags2,
      image_render_supported, nop_render_advertised, input_write_advertised,
      expand_buffer_advertised, shrink_buffer_advertised,
      classic_diagnostics.wide_time_allowed, classic_diagnostics.rejected_temporal_checkouts,
      classic_diagnostics.shutter_dependency_advertised};
  aexcompat::worker_render_report::begin_classic(report_snapshot, classic_report.head);
  report_snapshot.stream()
            << ",\"audio_usage_advertised\":" << (audio_telemetry().usage_advertised ? "true" : "false")
            << ",\"audio_checkout_allowed\":" << (audio_telemetry().checkout_allowed ? "true" : "false")
            << ",\"audio_source_available\":" << (audio_telemetry().source_available ? "true" : "false")
            << ",\"rejected_unadvertised_audio_checkouts\":" << audio_telemetry().rejected_unadvertised_checkouts
            << ",\"rejected_audio_format_requests\":" << audio_telemetry().rejected_format_requests
            << ",\"audio_handle_exhaustions\":" << audio_telemetry().handle_exhaustions
            << ",\"peak_live_audio_handles\":" << audio_telemetry().peak_live_handles
            << ",\"audio_checkout_calls\":" << audio_telemetry().checkout_calls
            << ",\"audio_checkin_calls\":" << audio_telemetry().checkin_calls
            << ",\"audio_get_data_calls\":" << audio_telemetry().get_data_calls
            << ",\"invalid_audio_operations\":" << audio_telemetry().invalid_operations
            << ",\"last_audio_checkout_start_time\":" << audio_telemetry().last_checkout_start_time
            << ",\"last_audio_checkout_duration\":" << audio_telemetry().last_checkout_duration
            << ",\"last_audio_checkout_time_scale\":" << audio_telemetry().last_checkout_time_scale
            << ",\"last_audio_window_start_sample\":" << audio_telemetry().last_window_start_sample
            << ",\"last_audio_window_sample_count\":" << audio_telemetry().last_window_sample_count
            << ",\"last_audio_window_silence_samples\":" << audio_telemetry().last_window_silence_samples
            << ",\"last_audio_output_rate_fixed\":" << audio_telemetry().last_output_rate
            << ",\"last_audio_output_bytes_per_sample\":" << audio_telemetry().last_output_bytes_per_sample
            << ",\"last_audio_output_channels\":" << audio_telemetry().last_output_channels
            << ",\"last_audio_output_format\":" << audio_telemetry().last_output_format
            << ",\"last_audio_returned_sample_frames\":" << audio_telemetry().last_returned_sample_frames
            << ",\"audio_lifetimes_balanced\":"
            << (audio_handle_lifetimes_balanced() ? "true" : "false")
            << ",\"render_selector_dispatched\":"
            << (aexcompat::worker_runtime::classic::last_selector_dispatched()
                    ? "true" : "false")
            << ",\"depth_supported\":" << (depth_supported ? "true" : "false")
            << ",\"render_error\":" << render_error;
  classic_report.sequence = {
      persistent_sequence, persistent_sequence_setup_error, persistent_sequence_setdown_error,
      persistent_frame_errors, persistent_frame_hashes, flattened_sequence,
      sequence_flatten_error, sequence_resetup_error, flattened_handle_replaced,
      resetup_handle_replaced, flattened_handle_host_disposed, copied_flattened_sequence,
      get_flattened_sequence_data_error, original_sequence_preserved};
  aexcompat::worker_render_report::append_classic_sequence(report_snapshot, classic_report.sequence);
  report_snapshot.stream()
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"return_message\":\"" << escape(std::string(
                reinterpret_cast<const char*>(output.data() + kOutMessage),
                strnlen_s(reinterpret_cast<const char*>(output.data() + kOutMessage), 256)))
            << "\""
            << ",\"case_id\":\"" << case_id << "\",\"pixel_format\":\"" << smart_state().pixel_format << "\",\"width\":"
            << render_width << ",\"height\":" << render_height << ",\"rowbytes\":" << render_rowbytes
            << ",\"bytes_written_per_row\":" << render_width *
                (smart_state().pixel_format == "argb32f" ? 16 :
                 (smart_state().pixel_format == "argb16" ? 8 : 4))
            << ",\"undefined_tail_bytes_per_row\":" << std::max(0, render_rowbytes - render_width *
                (smart_state().pixel_format == "argb32f" ? 16 :
                 (smart_state().pixel_format == "argb16" ? 8 : 4)))
            << ",\"input_sha256\":\"" << input_hash << "\",\"output_sha256\":\""
            << output_hash << "\",\"guard_bytes_intact\":" << (guards_intact ? "true" : "false")
            << world_debug_report_json();
  aexcompat::worker_render_report::append_custom_ui(report_snapshot, {
      g_render_click_enabled, g_render_click_error, g_render_click_out_flags,
      g_render_click_changed_value, g_render_draw_enabled, g_render_draw_error,
      g_render_draw_out_flags, g_render_ui_lifecycle_errors, g_render_ui_context_closed,
      g_app_color_picker_calls, g_app_invalidate_rect_calls, g_app_picker_color});
  report_snapshot.stream()
            << ",\"suite_leases_balanced\":" << (suite_leases_balanced() ? "true" : "false")
            << ",\"suite_lease_warning\":" << (!suite_leases_balanced() ? "true" : "false")
            << ",\"suite_acquires\":" << suite_acquire_count()
            << ",\"suite_releases\":" << suite_release_count()
            << missing_suites_report_json()
            << ",\"live_suite_lease_count\":" << live_suite_lease_count()
            << ",\"live_suite_reference_count\":" << live_suite_reference_count()
            << ",\"live_suite_leases\":\"" << live_suite_lease_summary() << "\""
            << ",\"handle_lifetimes_balanced\":" << (handle_lifetimes_balanced() ? "true" : "false")
            << ",\"pf_path_lifetimes_balanced\":" << (pf_path_lifetimes_balanced() ? "true" : "false")
            << ",\"pf_path_checkout_calls\":" << g_pf_path_checkout_calls
            << ",\"pf_path_checkin_calls\":" << g_pf_path_checkin_calls
            << ",\"pf_path_mask_calls\":" << g_pf_path_mask_calls
            << ",\"pf_path_preps_created\":" << g_pf_path_preps_created
            << ",\"pf_path_preps_disposed\":" << g_pf_path_preps_disposed
            << ",\"invalid_pf_path_operations\":" << g_invalid_pf_path_operations
            << ",\"pf_path_reject_reason\":" << g_pf_path_reject_reason
            << ",\"pf_path_last_feather\":[" << g_pf_path_last_feather_x << ','
            << g_pf_path_last_feather_y << ']'
            << ",\"pf_path_last_opacity\":" << g_pf_path_last_opacity
            << ",\"pf_path_last_quality\":" << g_pf_path_last_quality
            << ",\"pf_path_last_bounds\":[" << g_pf_path_last_bounds[0] << ','
            << g_pf_path_last_bounds[1] << ',' << g_pf_path_last_bounds[2] << ','
            << g_pf_path_last_bounds[3] << ']'
            << ",\"handles_created\":" << statistics().created
            << ",\"handles_disposed\":" << statistics().disposed
            << ",\"arbitrary_copy_calls\":" << g_arbitrary_copy_calls
            << ",\"arbitrary_dispose_calls\":" << g_arbitrary_dispose_calls
            << ",\"arbitrary_print_calls\":" << g_arbitrary_print_calls
            << ",\"arbitrary_print_failures\":" << g_arbitrary_print_failures
            << ",\"arbitrary_roundtrip_calls\":" << g_arbitrary_roundtrip_calls
            << ",\"arbitrary_roundtrip_failures\":" << g_arbitrary_roundtrip_failures
            << ",\"arbitrary_scan_calls\":" << g_arbitrary_scan_calls
            << ",\"arbitrary_scan_failures\":" << g_arbitrary_scan_failures
            << ",\"arbitrary_compare_disagreements\":" << g_arbitrary_compare_disagreements
            << ",\"arbitrary_new_calls\":" << g_arbitrary_new_calls
            << ",\"arbitrary_interpolation_calls\":" << g_arbitrary_interpolation_calls
            << ",\"arbitrary_interpolation_failures\":" << g_arbitrary_interpolation_failures
            << ",\"arbitrary_interpolation_amount\":" << g_last_arbitrary_interpolation_amount
            << ",\"invalid_arbitrary_operations\":" << g_invalid_arbitrary_operations
            << ",\"world_lifetimes_balanced\":" << (world_lifetimes_balanced() ? "true" : "false")
            << ",\"worlds_created\":" << aexcompat::world_registry::statistics().created
            << ",\"worlds_disposed\":" << aexcompat::world_registry::statistics().disposed
            << ",\"receipt_lifetimes_balanced\":"
            << (async_receipt_lifetimes_balanced() ? "true" : "false")
            << ",\"receipts_created\":" << aexcompat::render_receipts::statistics().created
            << ",\"receipts_checked_in\":" << aexcompat::render_receipts::statistics().checked_in
            << ",\"live_receipts\":" << aexcompat::render_receipts::statistics().live_count
            << ",\"live_receipt_bytes\":" << aexcompat::render_receipts::statistics().live_bytes
            << ",\"invalid_receipt_operations\":"
            << aexcompat::render_receipts::statistics().invalid_operations
            << ",\"async_layer_requests_balanced\":"
            << (async_layer_requests_balanced() ? "true" : "false")
            << ",\"async_layer_requests_created\":" << g_async_layer_requests_created
            << ",\"async_layer_requests_completed\":" << g_async_layer_requests_completed
            << ",\"async_layer_requests_canceled\":" << g_async_layer_requests_canceled
            << ",\"async_layer_callback_failures\":" << g_async_layer_callback_failures
            << ",\"async_layer_callback_exceptions\":" << g_async_layer_callback_exceptions
            << ",\"live_async_layer_requests\":" << g_async_layer_requests.size()
            << ",\"async_layer_reserved_bytes\":" << g_async_layer_reserved_bytes
            << ",\"gpu_memory_lifetimes_balanced\":" << (gpu_memory_lifetimes_balanced() ? "true" : "false")
            << ",\"cuda_context_used\":" << (g_cuda_upload_bytes > 0 ? "true" : "false")
            << ",\"cuda_upload_bytes\":" << g_cuda_upload_bytes
            << ",\"cuda_download_bytes\":" << g_cuda_download_bytes
            << ",\"cuda_sync_failures\":" << g_cuda_sync_failures
            << ",\"cuda_device_count\":" << g_last_cuda_device_count
            << ",\"cuda_device_index\":" << g_last_cuda_device_index
            << ",\"opencl_context_used\":" << (g_opencl_upload_bytes > 0 ? "true" : "false")
            << ",\"opencl_upload_bytes\":" << g_opencl_upload_bytes
            << ",\"opencl_download_bytes\":" << g_opencl_download_bytes
            << ",\"opencl_sync_failures\":" << g_opencl_sync_failures
            << ",\"opencl_device_count\":" << opencl::last_device_count()
            << ",\"opencl_device_index\":" << opencl::last_device_index()
            << ",\"directx_context_used\":" << (directx_backend::diagnostics().context_used ? "true" : "false")
            << ",\"directx_device_count\":" << directx_backend::diagnostics().device_count
            << ",\"directx_device_index\":" << directx_backend::diagnostics().device_index
            << ",\"directx_upload_bytes\":" << directx_backend::diagnostics().upload_bytes
            << ",\"directx_download_bytes\":" << directx_backend::diagnostics().download_bytes
            << ",\"directx_sync_failures\":" << directx_backend::diagnostics().sync_failures
            << ",\"gpu_allocations_created\":" << g_gpu_allocations_created
            << ",\"gpu_allocations_freed\":" << g_gpu_allocations_freed
            << ",\"live_gpu_allocation_count\":" << gpu_transport::live_allocation_count()
            << ",\"live_gpu_memory_bytes\":" << gpu_transport::live_memory_bytes()
            << ",\"gpu_exclusive_access_depth\":" << gpu_transport::exclusive_access_depth()
            << ",\"invalid_gpu_memory_operations\":" << g_invalid_gpu_memory_operations
            << ",\"last_seh_exception_code\":" << g_last_seh_exception_code
            << ",\"last_seh_exception_address\":" << g_last_seh_exception_address
            << ",\"last_seh_exception_module\":\"" << escape(g_last_seh_exception_module) << "\""
            << ",\"last_seh_selector\":\"" << escape(g_last_seh_selector) << "\""
            << ",\"last_seh_error\":" << g_last_seh_error
            << ",\"param_checkouts_balanced\":" << (classic_diagnostics.balanced ? "true" : "false")
            << ",\"param_checkout_calls\":" << classic_diagnostics.checkout_calls
            << ",\"param_checkin_calls\":" << classic_diagnostics.checkin_calls
            << ",\"automatic_param_checkins\":" << classic_diagnostics.automatic_checkins
            << ",\"invalid_param_checkins\":" << classic_diagnostics.invalid_checkins
            << ",\"last_param_checkout_index\":" << classic_diagnostics.last_index
            << ",\"last_param_checkout_time\":" << classic_diagnostics.last_time
            << ",\"last_param_checkout_time_step\":" << classic_diagnostics.last_time_step
            << ",\"last_param_checkout_time_scale\":" << classic_diagnostics.last_time_scale
            << ",\"options_button_name\":\"" << escape(g_options_button_name) << "\""
            << ",\"options_button_name_calls\":" << g_options_button_name_calls
            << ",\"channel_count_queries\":" << g_channel_count_queries.load()
            << ",\"transform_world_calls\":" << g_transform_world_calls
            << ",\"last_transform_x\":" << g_last_transform_x
            << ",\"last_transform_y\":" << g_last_transform_y
            << ",\"last_transform_opacity\":" << static_cast<unsigned>(g_last_transform_opacity)
            << ",\"abort_calls\":" << g_abort_calls
            << ",\"progress_calls\":" << g_progress_calls
            << ",\"register_ui_calls\":" << g_register_ui_calls
            << ",\"last_progress_current\":" << g_last_progress_current
            << ",\"last_progress_total\":" << g_last_progress_total;
  classic_report.threads = {concurrent_render, thread_errors, thread_hashes, thread_guards};
  aexcompat::worker_render_report::append_classic_threads(report_snapshot, classic_report.threads);
  report_snapshot.stream()
            << ",\"request_mode\":" << (request_mode ? "true" : "false")
            << ",\"downsample_x\":[" << g_downsample_x.numerator << "," << g_downsample_x.denominator << "]"
            << ",\"downsample_y\":[" << g_downsample_y.numerator << "," << g_downsample_y.denominator << "]"
            << ",\"pixel_aspect_ratio\":[" << g_pixel_aspect_ratio.numerator << "," << g_pixel_aspect_ratio.denominator << "]"
            << ",\"full_resolution_dimensions\":[" << (g_full_resolution_width > 0 ? g_full_resolution_width : external_width)
            << "," << (g_full_resolution_height > 0 ? g_full_resolution_height : external_height) << "]"
            << ",\"quality\":" << read<int32_t>(input, kInQuality)
            << ",\"in_data_num_params\":" << read<int32_t>(input, kInNumParams)
            << ",\"local_time_step\":" << read<int32_t>(input, kInLocalTimeStep)
            << ",\"field\":" << read<int32_t>(input, 244)
            << ",\"shutter_angle_fixed\":" << read<int32_t>(input, 248)
            << ",\"shutter_phase_fixed\":" << read<int32_t>(input, 400)
            << ",\"in_data_dimensions\":[" << read<int32_t>(input, 252) << "," << read<int32_t>(input, 256) << "]"
            << ",\"pre_effect_source_origin\":[" << read<int32_t>(input, 392) << "," << read<int32_t>(input, 396) << "]"
            << ",\"output_origin\":[" << read<int32_t>(input, 276) << "," << read<int32_t>(input, 280) << "]";
  aexcompat::worker_render_report::finish_requested_parameters(report_snapshot, {
      requested_parameters_json(requested_parameters),
      static_cast<int32_t>(requested_value(requested_parameters, L"amount")),
      static_cast<int32_t>(requested_value(requested_parameters, L"direction")),
      static_cast<int32_t>(requested_value(requested_parameters, L"seed")),
      requested_value(requested_parameters, L"mix"),
      static_cast<int32_t>(requested_value(requested_parameters, L"invert_map")),
      !nop_render_advertised, module_audit_json()});
  aexcompat::worker_render_report::emit(report_snapshot, std::cout);
  } else if (is_smart_worker()) {
  restore_native_stdout();
  const auto mask_report = aexcompat::mask_runtime::snapshot();
  aexcompat::worker_render_report::ReportSnapshot report_snapshot(std::cout);
  report_snapshot.stream() << "{\"schema_version\":1,\"stage\":\"smartfx_render\",\"status\":\""
            << (smart.pre_error == 0 && smart.render_error == 0 &&
                parameter_count_contract_valid && smart.rects_valid &&
                arbitrary_defaults_disposed && g_invalid_arbitrary_operations == 0 &&
                smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0 &&
                smart.guards_intact && handle_lifetimes_balanced() &&
                world_lifetimes_balanced() && gpu_memory_lifetimes_balanced() &&
                audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
                param_checkouts_balanced() &&
                ((!g_render_click_enabled && !g_render_draw_enabled) ||
                 g_render_ui_context_closed)
                ? "render_completed" : "render_failed")
            << "\",\"global_setup_error\":" << global_error
            << ",\"params_setup_error\":" << params_error
            << ",\"advertised_out_flags\":" << advertised_out_flags
            << ",\"advertised_out_flags2\":" << advertised_out_flags2
            << ",\"image_render_supported\":" << (image_render_supported ? "true" : "false")
            << ",\"smart_render_supported\":" << (smart_render_supported ? "true" : "false")
            << ",\"nop_render_advertised\":" << (nop_render_advertised ? "true" : "false")
            << ",\"input_write_advertised\":" << (input_write_advertised ? "true" : "false")
            << ",\"input_buffer_writable\":" << (input_write_advertised ? "true" : "false")
            << ",\"wide_time_checkout_allowed\":" << (smart.runtime->wide_time_checkout_allowed ? "true" : "false")
            << ",\"rejected_temporal_param_checkouts\":" << smart.runtime->rejected_temporal_checkouts
            << ",\"shutter_dependency_advertised\":" << (smart.runtime->shutter_dependency_advertised ? "true" : "false")
            << ",\"smart_pre_render_dispatched\":" << (nop_render_advertised ? "false" : "true")
            << ",\"smart_render_selector_dispatched\":" << (nop_render_advertised ? "false" : "true")
            << ",\"comp_bg_color_success_count\":"
            << g_comp_bg_color_successes.load(std::memory_order_relaxed)
            << ",\"comp_bg_color_rejection_count\":"
            << g_comp_bg_color_rejections.load(std::memory_order_relaxed)
            << ",\"guid_mix_in_call_count\":"
            << g_guid_mix_in_calls.load(std::memory_order_relaxed)
            << ",\"guid_mix_in_success_count\":"
            << g_guid_mix_in_successes.load(std::memory_order_relaxed)
            << ",\"guid_mix_in_rejection_count\":"
            << g_guid_mix_in_rejections.load(std::memory_order_relaxed)
            << ",\"guid_mix_in_last_size\":"
            << g_guid_mix_in_last_size.load(std::memory_order_relaxed)
            << ",\"guid_mix_in_max_size\":"
            << g_guid_mix_in_max_size.load(std::memory_order_relaxed)
            << ",\"guid_mix_in_size_limit\":" << kMaxGuidMixInBytes
            << ",\"guid_mix_in_last_result\":"
            << g_guid_mix_in_last_result.load(std::memory_order_relaxed)
            << ",\"depth_supported\":" << (depth_supported ? "true" : "false")
            << ",\"pre_render_error\":" << smart.pre_error
            << ",\"smart_render_error\":" << smart.render_error
            << ",\"smart_render_selector_error\":" << smart.selector_error
            << ",\"gpu_device_setup_error\":" << smart.gpu_setup_error
            << ",\"gpu_device_setdown_error\":" << smart.gpu_setdown_error
            << ",\"gpu_device_setdown_exception_code\":" << smart.gpu_setdown_exception_code
            << ",\"gpu_render_possible\":" << (smart.gpu_render_possible ? "true" : "false")
            << ",\"gpu_render_dispatched\":" << (smart.gpu_render_dispatched ? "true" : "false")
            << ",\"checkout_time\":" << smart.checkout_time
            << ",\"checkout_time_step\":" << smart.checkout_time_step
            << ",\"checkout_time_scale\":" << smart.checkout_time_scale
            << ",\"roi_contract_valid\":" << (smart.roi_contract_valid ? "true" : "false")
            << ",\"input_checkout_request\":[" << smart.runtime->input_checkout_request[0] << "," << smart.runtime->input_checkout_request[1]
            << "," << smart.runtime->input_checkout_request[2] << "," << smart.runtime->input_checkout_request[3] << "]"
            << ",\"map_checkout_request\":[" << smart.runtime->map_checkout_request[0] << "," << smart.runtime->map_checkout_request[1]
            << "," << smart.runtime->map_checkout_request[2] << "," << smart.runtime->map_checkout_request[3] << "]"
            << ",\"global_setdown_error\":" << setdown_error
            << ",\"case_id\":\"" << case_id << "\",\"pixel_format\":\"" << smart.runtime->pixel_format << "\",\"width\":"
            << smart.output_width << ",\"height\":" << smart.output_height << ",\"rowbytes\":"
            << smart.output_rowbytes
            << ",\"bytes_written_per_row\":" << smart.output_rowbytes
            << ",\"undefined_tail_bytes_per_row\":0"
            << ",\"input_sha256\":\"" << smart.input_hash << "\",\"output_sha256\":\""
            << smart.output_hash << "\",\"result_rects_valid\":" << (smart.rects_valid ? "true" : "false")
            << world_debug_report_json();
  aexcompat::worker_render_report::append_custom_ui(report_snapshot, {
      g_render_click_enabled, g_render_click_error, g_render_click_out_flags,
      g_render_click_changed_value, g_render_draw_enabled, g_render_draw_error,
      g_render_draw_out_flags, g_render_ui_lifecycle_errors, g_render_ui_context_closed,
      g_app_color_picker_calls, g_app_invalidate_rect_calls, g_app_picker_color});
  report_snapshot.stream()
            << ",\"result_rect\":[" << smart.result_rect[0] << "," << smart.result_rect[1]
            << "," << smart.result_rect[2] << "," << smart.result_rect[3] << "]"
            << ",\"max_result_rect\":[" << smart.max_result_rect[0] << "," << smart.max_result_rect[1]
            << "," << smart.max_result_rect[2] << "," << smart.max_result_rect[3] << "]"
            << ",\"guard_bytes_intact\":" << (smart.guards_intact ? "true" : "false")
            << ",\"output_pixels_valid\":" << (smart.output_pixels_valid ? "true" : "false")
            << ",\"param_checkouts_balanced\":" << (param_checkouts_balanced() ? "true" : "false")
            << ",\"param_checkout_calls\":" << g_param_checkout_calls
            << ",\"param_checkin_calls\":" << g_param_checkin_calls
            << ",\"automatic_param_checkins\":" << g_automatic_param_checkins
            << ",\"invalid_param_checkins\":" << g_invalid_param_checkins
            << ",\"request_mode\":" << (request_mode ? "true" : "false")
            << ",\"downsample_x\":[" << g_downsample_x.numerator << "," << g_downsample_x.denominator << "]"
            << ",\"downsample_y\":[" << g_downsample_y.numerator << "," << g_downsample_y.denominator << "]"
            << ",\"pixel_aspect_ratio\":[" << g_pixel_aspect_ratio.numerator << "," << g_pixel_aspect_ratio.denominator << "]"
            << ",\"full_resolution_dimensions\":[" << (g_full_resolution_width > 0 ? g_full_resolution_width : external_width)
            << "," << (g_full_resolution_height > 0 ? g_full_resolution_height : external_height) << "]"
            << ",\"quality\":" << read<int32_t>(input, kInQuality)
            << ",\"in_data_num_params\":" << read<int32_t>(input, kInNumParams)
            << ",\"local_time_step\":" << read<int32_t>(input, kInLocalTimeStep)
            << ",\"field\":" << read<int32_t>(input, 244)
            << ",\"shutter_angle_fixed\":" << read<int32_t>(input, 248)
            << ",\"shutter_phase_fixed\":" << read<int32_t>(input, 400)
            << ",\"in_data_dimensions\":[" << read<int32_t>(input, 252) << "," << read<int32_t>(input, 256) << "]"
            << ",\"pre_effect_source_origin\":[" << read<int32_t>(input, 392) << "," << read<int32_t>(input, 396) << "]"
            << ",\"output_origin\":[" << read<int32_t>(input, 276) << "," << read<int32_t>(input, 280) << "]"
            << ",\"mask_scene_id\":\"" << g_mask_scene_id << "\""
            << ",\"mask_count\":" << mask_report.active_masks
            << ",\"mask_open_count\":" << mask_open_count()
            << ",\"mask_tangent_vertex_count\":" << mask_tangent_vertex_count()
            << ",\"mask_lifetimes_balanced\":" << (mask_lifetimes_balanced() ? "true" : "false")
            << ",\"mask_handles_acquired\":" << mask_report.masks_acquired
            << ",\"mask_handles_disposed\":" << mask_report.masks_disposed
            << ",\"stream_handles_acquired\":" << mask_report.streams_acquired
            << ",\"stream_handles_disposed\":" << mask_report.streams_disposed
            << ",\"stream_values_acquired\":" << mask_report.values_acquired
            << ",\"stream_values_disposed\":" << mask_report.values_disposed
            << ",\"lifetime_fault_observed\":" << (lifetime_fault_observed ? "true" : "false")
            << ",\"suite_leases_balanced\":" << (suite_leases_balanced() ? "true" : "false")
            << ",\"suite_lease_warning\":" << (!suite_leases_balanced() ? "true" : "false")
            << ",\"suite_acquires\":" << suite_acquire_count()
            << ",\"suite_releases\":" << suite_release_count()
            << missing_suites_report_json()
            << ",\"live_suite_lease_count\":" << live_suite_lease_count()
            << ",\"live_suite_reference_count\":" << live_suite_reference_count()
            << ",\"live_suite_leases\":\"" << live_suite_lease_summary() << "\""
            << ",\"suite_fault_observed\":" << (suite_fault_observed ? "true" : "false")
            << ",\"handle_lifetimes_balanced\":" << (handle_lifetimes_balanced() ? "true" : "false")
            << ",\"handles_created\":" << statistics().created
            << ",\"handles_disposed\":" << statistics().disposed
            << ",\"arbitrary_copy_calls\":" << g_arbitrary_copy_calls
            << ",\"arbitrary_dispose_calls\":" << g_arbitrary_dispose_calls
            << ",\"arbitrary_print_calls\":" << g_arbitrary_print_calls
            << ",\"arbitrary_print_failures\":" << g_arbitrary_print_failures
            << ",\"arbitrary_roundtrip_calls\":" << g_arbitrary_roundtrip_calls
            << ",\"arbitrary_roundtrip_failures\":" << g_arbitrary_roundtrip_failures
            << ",\"arbitrary_scan_calls\":" << g_arbitrary_scan_calls
            << ",\"arbitrary_scan_failures\":" << g_arbitrary_scan_failures
            << ",\"arbitrary_compare_disagreements\":" << g_arbitrary_compare_disagreements
            << ",\"arbitrary_new_calls\":" << g_arbitrary_new_calls
            << ",\"arbitrary_interpolation_calls\":" << g_arbitrary_interpolation_calls
            << ",\"arbitrary_interpolation_failures\":" << g_arbitrary_interpolation_failures
            << ",\"arbitrary_interpolation_amount\":" << g_last_arbitrary_interpolation_amount
            << ",\"invalid_arbitrary_operations\":" << g_invalid_arbitrary_operations
            << ",\"automatic_pre_render_handle_disposals\":"
            << statistics().automatic_pre_render_disposals
            << ",\"handle_locks\":" << statistics().locks
            << ",\"handle_unlocks\":" << statistics().unlocks
            << ",\"live_handle_count\":" << statistics().live_count
            << ",\"live_handle_bytes\":" << statistics().live_bytes
            << ",\"invalid_handle_operations\":" << statistics().invalid_operations
            << ",\"handle_fault_observed\":" << (handle_fault_observed ? "true" : "false")
            << ",\"world_fault_observed\":" << (world_fault_observed ? "true" : "false")
            << ",\"world_lifetimes_balanced\":" << (world_lifetimes_balanced() ? "true" : "false")
            << ",\"worlds_created\":" << aexcompat::world_registry::statistics().created
            << ",\"worlds_disposed\":" << aexcompat::world_registry::statistics().disposed
            << ",\"live_world_count\":" << aexcompat::world_registry::statistics().live_count
            << ",\"live_world_bytes\":" << aexcompat::world_registry::statistics().live_bytes
            << ",\"invalid_world_operations\":"
            << aexcompat::world_registry::statistics().invalid_operations
            << ",\"gpu_memory_lifetimes_balanced\":" << (gpu_memory_lifetimes_balanced() ? "true" : "false")
            << ",\"gpu_allocations_created\":" << g_gpu_allocations_created
            << ",\"gpu_allocations_freed\":" << g_gpu_allocations_freed
            << ",\"live_gpu_allocation_count\":" << gpu_transport::live_allocation_count()
            << ",\"live_gpu_memory_bytes\":" << gpu_transport::live_memory_bytes()
            << ",\"gpu_exclusive_access_depth\":" << gpu_transport::exclusive_access_depth()
            << ",\"invalid_gpu_memory_operations\":" << g_invalid_gpu_memory_operations
            << ",\"last_seh_exception_code\":" << g_last_seh_exception_code
            << ",\"last_seh_exception_address\":" << g_last_seh_exception_address
            << ",\"last_seh_exception_module\":\"" << escape(g_last_seh_exception_module) << "\""
            << ",\"last_seh_selector\":\"" << escape(g_last_seh_selector) << "\""
            << ",\"last_seh_error\":" << g_last_seh_error
            << ",\"cuda_context_used\":" << (g_cuda_upload_bytes > 0 ? "true" : "false")
            << ",\"cuda_upload_bytes\":" << g_cuda_upload_bytes
            << ",\"cuda_download_bytes\":" << g_cuda_download_bytes
            << ",\"cuda_sync_failures\":" << g_cuda_sync_failures
            << ",\"cuda_device_count\":" << g_last_cuda_device_count
            << ",\"cuda_device_index\":" << g_last_cuda_device_index
            << ",\"opencl_context_used\":" << (g_opencl_upload_bytes > 0 ? "true" : "false")
            << ",\"opencl_upload_bytes\":" << g_opencl_upload_bytes
            << ",\"opencl_download_bytes\":" << g_opencl_download_bytes
            << ",\"opencl_sync_failures\":" << g_opencl_sync_failures
            << ",\"opencl_device_count\":" << opencl::last_device_count()
            << ",\"opencl_device_index\":" << opencl::last_device_index()
            << ",\"directx_context_used\":" << (directx_backend::diagnostics().context_used ? "true" : "false")
            << ",\"directx_device_count\":" << directx_backend::diagnostics().device_count
            << ",\"directx_device_index\":" << directx_backend::diagnostics().device_index
            << ",\"directx_upload_bytes\":" << directx_backend::diagnostics().upload_bytes
            << ",\"directx_download_bytes\":" << directx_backend::diagnostics().download_bytes
            << ",\"directx_sync_failures\":" << directx_backend::diagnostics().sync_failures
            << ",\"pixel_format_fault_observed\":" << (pixel_format_fault_observed ? "true" : "false")
            << ",\"pixel_format_add_calls\":" << g_pixel_format_add_calls
            << ",\"pixel_format_clear_calls\":" << g_pixel_format_clear_calls
            << ",\"supported_pixel_format_count\":" << g_supported_pixel_formats.size()
            << ",\"invalid_pixel_format_operations\":" << g_invalid_pixel_format_operations
            << ",\"outline_fault_observed\":" << (outline_fault_observed ? "true" : "false")
            << ",\"outline_mutations\":" << mask_report.outline_mutations
            << ",\"invalid_outline_operations\":" << mask_report.invalid_outline_operations
            << ",\"mask_attribute_fault_observed\":" << (mask_attribute_fault_observed ? "true" : "false")
            << ",\"mask_mutations\":" << mask_report.mask_mutations
            << ",\"invalid_mask_operations\":" << mask_report.invalid_mask_operations
            << ",\"stream_metadata_fault_observed\":" << (stream_metadata_fault_observed ? "true" : "false")
            << ",\"stream_metadata_queries\":" << g_stream_metadata_queries
            << ",\"stream_duplicates\":" << g_stream_duplicates
            << ",\"invalid_stream_operations\":" << g_invalid_stream_operations
            << ",\"keyframe_fault_observed\":" << (keyframe_fault_observed ? "true" : "false")
            << ",\"keyframe_mutations\":" << mask_report.keyframe_mutations
            << ",\"invalid_keyframe_operations\":" << mask_report.invalid_keyframe_operations
            << ",\"dynamic_stream_fault_observed\":" << (dynamic_stream_fault_observed ? "true" : "false")
            << ",\"dynamic_stream_queries\":" << g_dynamic_stream_queries
            << ",\"dynamic_stream_mutations\":" << g_dynamic_stream_mutations
            << ",\"invalid_dynamic_stream_operations\":" << g_invalid_dynamic_stream_operations
            << ",\"aegp_memory_fault_observed\":" << (aegp_memory_fault_observed ? "true" : "false")
            << ",\"aegp_memory_created\":" << aegp_memory_statistics().created
            << ",\"aegp_memory_freed\":" << aegp_memory_statistics().freed
            << ",\"live_aegp_memory_handles\":" << aegp_memory_statistics().live_count
            << ",\"live_aegp_memory_bytes\":" << aegp_memory_statistics().live_bytes
            << ",\"invalid_aegp_memory_operations\":" << aegp_memory_statistics().invalid_operations;
  aexcompat::worker_render_report::finish_requested_parameters(report_snapshot, {
      requested_parameters_json(requested_parameters),
      static_cast<int32_t>(requested_value(requested_parameters, L"amount")),
      static_cast<int32_t>(requested_value(requested_parameters, L"direction")),
      static_cast<int32_t>(requested_value(requested_parameters, L"seed")),
      requested_value(requested_parameters, L"mix"),
      static_cast<int32_t>(requested_value(requested_parameters, L"invert_map")),
      !nop_render_advertised, module_audit_json()});
  aexcompat::worker_render_report::emit(report_snapshot, std::cout);
  } else {
  report(global_error == 0 && params_error == 0 ? "selectors_completed" : "selector_error",
         global_error, params_error, setdown_error, output, about_message, lifecycle_errors, lifecycle_data_null);
  }
  if (is_render_worker()) {
    return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
      image_render_supported && depth_supported && render_error == 0 && guards_intact &&
      handle_lifetimes_balanced() && world_lifetimes_balanced() &&
      async_receipt_lifetimes_balanced() &&
      async_layer_requests_balanced() &&
      gpu_memory_lifetimes_balanced() &&
      audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
      param_checkouts_balanced() &&
      ((!g_render_click_enabled && !g_render_draw_enabled) ||
       g_render_ui_context_closed) ? 0 : 21);
  }
  if (is_smart_worker()) {
    return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
      image_render_supported && depth_supported && smart.pre_error == 0 && smart.render_error == 0 &&
      smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0 &&
      smart.rects_valid && smart.guards_intact &&
      handle_lifetimes_balanced() && world_lifetimes_balanced() &&
      gpu_memory_lifetimes_balanced() &&
      audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
      param_checkouts_balanced() &&
      ((!g_render_click_enabled && !g_render_draw_enabled) ||
       g_render_ui_context_closed) ? 0 : 22);
  }
  return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
      user_changed_ok && conditional_ui_ok && about_error == 0 && lifecycle_data_null &&
      std::all_of(lifecycle_errors.begin(), lifecycle_errors.end(), [](auto error) { return error == 0; }) ? 0 : 20);
}

int aexcompat::worker_target::run(Kind kind, int argc, wchar_t** argv) {
  l2_detail::g_worker_target = kind;
  return worker_main_impl(argc, argv);
}
