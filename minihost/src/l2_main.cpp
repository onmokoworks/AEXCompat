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
#include "worker_parameter_selftests.hpp"
#include "worker_parameter_execution.hpp"
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
#include "worker_aegp_layer_render_runtime.hpp"
#include "worker_aegp_item_render_runtime.hpp"
#include "worker_aegp_world_selftests.hpp"
#include "worker_aegp_init_runtime.hpp"
#include "worker_aegp_init_execution.hpp"
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
#include "worker_pf_state_runtime.hpp"
#include "worker_report.hpp"
#include "worker_request_parser.hpp"
#include "worker_invocation_orchestration.hpp"
#include "worker_render_report.hpp"
#include "worker_render_receipts.hpp"
#include "worker_target.hpp"

// Internal worker implementation uses a named namespace so subsystem
// translation units can own callback state without including implementation
// fragments into this file.
namespace aexcompat::l2_detail {

using aexcompat::parameter_selftests::verify_parameter_animation_transport;
using aexcompat::parameter_selftests::verify_pf_param_utils_suite3;

using namespace aexcompat::pf_ae_channel;
using namespace aexcompat::pf_state_runtime;
using namespace aexcompat::worker_runtime::parameter_execution;
using namespace aexcompat::worker_runtime::aegp_timeline;

using namespace aexcompat::color_settings;

int32_t __cdecl acquire_suite(const char* name, int32_t version, const void** suite);
int32_t __cdecl release_suite(const char* name, int32_t version);

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
  snapshot.stream_metadata_queries = g_stream_metadata_queries;
  snapshot.stream_duplicates = g_stream_duplicates;
  snapshot.invalid_stream_operations = g_invalid_stream_operations;
  snapshot.dynamic_stream_mutations = g_dynamic_stream_mutations;
  snapshot.invalid_dynamic_stream_operations = g_invalid_dynamic_stream_operations;
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

bool install_synthetic_mask_scene(
    const std::vector<aexcompat::mask_runtime::CurveSnapshot>& curves) {
  if (!g_stream_refs.empty() || !g_stream_values.empty() || !g_add_keyframe_transactions.empty())
    return false;
  g_mask_scene.clear();
  g_mask_scene.reserve(kMaxHostMasks);
  if (curves.size() > kMaxHostMasks) return false;
  for (const auto& curve : curves) {
    HostMask mask;
    mask.id = curve.id;
    mask.open = curve.open;
    mask.vertices.reserve(curve.vertices.size());
    for (const auto& vertex : curve.vertices)
      mask.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
          vertex.tangent_in_y, vertex.tangent_out_x, vertex.tangent_out_y});
    g_mask_scene.push_back(std::move(mask));
  }
  return true;
}

std::vector<HostMask*> ordered_active_masks();
std::vector<aexcompat::pf_path_runtime::PathInfo> enumerate_pf_paths() {
  std::vector<aexcompat::pf_path_runtime::PathInfo> result;
  for (auto* mask : ordered_active_masks())
    result.push_back({mask, mask->id, mask->dynamic_order, mask->open,
                      mask->invert, mask->mode});
  return result;
}

bool snapshot_pf_path(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve) {
  auto* mask = static_cast<HostMask*>(handle);
  if (!mask || mask->deleted) return false;
  aexcompat::mask_runtime::CurveSnapshot candidate;
  candidate.id = mask->id;
  candidate.open = mask->open;
  candidate.vertices.reserve(mask->vertices.size());
  for (const auto& vertex : mask->vertices)
    candidate.vertices.push_back({vertex.x, vertex.y, vertex.tangent_in_x,
        vertex.tangent_in_y, vertex.tangent_out_x, vertex.tangent_out_y});
  curve = std::move(candidate);
  return true;
}

bool bounded_pf_path_world(void* world,
    aexcompat::pf_path_runtime::WorldView& view) {
  if (!world) return false;
  int32_t flags{};
  std::memcpy(&flags, static_cast<std::byte*>(world) + 16, sizeof(flags));
  view.pixel_bytes = (flags & 1) != 0 ? 8 : 4;
  return bounded_typed_world(world, view.pixel_bytes, view.pixels, view.rowbytes,
                             view.width, view.height);
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

bool configure_mask_scene(const std::string& scene_id) {
  aexcompat::mask_runtime::configure_host_context(
      {&g_layer, &raise_mask_access_violation, &mask_runtime_snapshot,
       &snapshot_mask_curve, &mask_lifetimes_balanced, &install_synthetic_mask_scene});
  aexcompat::pf_path_runtime::configure(
      {&g_effect, &enumerate_pf_paths, &snapshot_pf_path, &bounded_pf_path_world});
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

int32_t __cdecl pf_mask_world_with_path(void* effect_ref, void** path, double feather_x,
                                        double feather_y, int32_t invert, double opacity,
                                        int32_t quality, void* world, LegacyRect* bounds);
#include "worker_l2_suite_abi.hpp"

UtilitySuite g_utility_suite{{}, &register_with_aegp, &get_main_hwnd, {}};
UtilitySuite3 g_utility_suite3{{}, &register_with_aegp, &get_main_hwnd, {}};
PfInterfaceSuite g_pf_interface_suite{&get_effect_layer, &get_new_effect_for_effect,
    &convert_effect_to_comp_time, &get_effect_camera,
    &get_effect_camera_matrix};
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
PfMaskSuite1 g_pf_mask_suite1{
    reinterpret_cast<decltype(PfMaskSuite1::mask_world_with_path)>(
        &aexcompat::pf_path_runtime::mask_world_with_path)};
// PF_AdvAppSuite1 is frozen at ten callbacks; keep its storage independent
// from the eleven-slot v2 table so versioned suite identity cannot alias.
struct PfAdvItemSuite1;
extern PfAdvItemSuite1 g_adv_item_suite1;
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
int32_t __cdecl checkout_item_frame_async(void*, uint32_t, void*, void**);
int32_t __cdecl checkout_layer_frame_async(void*, uint32_t, void*, void**);
struct AegpRenderAsyncManagerSuite1 {
  decltype(&checkout_item_frame_async) checkout_item_frame;
  decltype(&checkout_layer_frame_async) checkout_layer_frame;
};
static_assert(sizeof(AegpRenderAsyncManagerSuite1) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderAsyncManagerSuite1, checkout_item_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderAsyncManagerSuite1, checkout_layer_frame) == 1 * sizeof(void*));

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
void* enter_custom_ui_context(int32_t context) {
  return new (std::nothrow) PfHelperUiContextScope(context);
}
void leave_custom_ui_context(void* scope) {
  delete static_cast<PfHelperUiContextScope*>(scope);
}
bool custom_ui_context_stable() {
  return g_ui_context_pointer == &g_ui_context;
}
void set_custom_ui_context_tool(int32_t context) {
  aexcompat::pf_helper::set_context_tool(
      context, aexcompat::pf_helper::kExtendedToolMin);
}
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

bool keyframe_suite5_abi_wiring_valid() {
  const KeyframeSuite expected{&get_stream_num_keyframes, &get_keyframe_time,
      &insert_keyframe, &delete_keyframe, &get_new_keyframe_value,
      &set_keyframe_value, &get_stream_value_dimensionality,
      &get_stream_temporal_dimensionality, &get_new_keyframe_spatial_tangents,
      &set_keyframe_spatial_tangents, &get_keyframe_temporal_ease,
      &set_keyframe_temporal_ease, &get_keyframe_flags, &set_keyframe_flag,
      &get_keyframe_interpolation, &set_keyframe_interpolation,
      &start_add_keyframes, &add_keyframes, &set_add_keyframe,
      &end_add_keyframes, &get_keyframe_label, &set_keyframe_label};
  return std::memcmp(&g_keyframe_suite, &expected, sizeof(expected)) == 0;
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
int32_t __cdecl app_get_personal_info(char* info) {
  if (!info) return 4;
  std::memset(info, 0, 64 * 3);
  std::memcpy(info, "AEXCompat", sizeof("AEXCompat"));
  std::memcpy(info + 64, "onmokoworks", sizeof("onmokoworks"));
  std::memcpy(info + 128, "SDK fixture", sizeof("SDK fixture"));
  return 0;
}
void bump_render_project_timestamp() {
  aexcompat::aegp_external_render_runtime::bump_project_generation();
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

using LayerRenderContext = aexcompat::aegp_layer_render_runtime::Context;
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
  const auto& context = aexcompat::aegp_layer_render_runtime::context();
  return context.entry && context.input && in_data == context.input;
}

bool active_adv_item_world(const void* world, DispatchWorldFormat& result) {
  return resolve_registered_dispatch_world(world, result);
}

int32_t __cdecl adv_item_move_time_step(void* in_data, void* world,
                                        int32_t direction, int32_t steps) {
  auto& context = aexcompat::aegp_layer_render_runtime::context();
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
  auto& context = aexcompat::aegp_layer_render_runtime::context();
  if (!context.entry || context.time_step <= 0) return 4;
  int32_t moved = context.active_item_time_valid ? context.active_item_time : context.current_time;
  if (checked_adv_item_move(direction, steps, context.time_step, moved) != 0) return 4;
  context.active_item_time = moved;
  context.active_item_time_valid = true;
  return 0;
}

int32_t __cdecl adv_item_touch_active() {
  if (!aexcompat::aegp_layer_render_runtime::context().entry) return 4;
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
  if (!context_handle || !enabled || !aexcompat::aegp_layer_render_runtime::context().entry) return 4;
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
void drain_async_layer_requests();

uint32_t staged_item_project_generation() {
  return aexcompat::aegp_external_render_runtime::project_generation();
}
bool staged_item_synthetic_receipts_enabled() {
  return g_synthetic_receipt_test_mode;
}
const bool g_staged_item_runtime_configured = [] {
  aexcompat::aegp_staged_item_runtime::configure({
      &staged_item_project_generation, &staged_item_synthetic_receipts_enabled,
      &aexcompat::aegp_item_render_runtime::publish_synthetic});
  return true;
}();
const bool g_external_render_runtime_configured = [] {
  aexcompat::aegp_external_render_runtime::configure({
      &aexcompat::aegp_staged_item_runtime::clear,
      +[](void* item) { return item == aegp_comp_item_handle(); }});
  return true;
}();
const bool g_item_render_runtime_configured = [] {
  aexcompat::aegp_item_render_runtime::configure({
      &snapshot_render_options,
      &aexcompat::aegp_external_render_runtime::publish_cached_receipt,
      &aexcompat::aegp_staged_item_runtime::publish_receipt});
  return true;
}();
const bool g_layer_render_runtime_configured = [] {
  aexcompat::aegp_layer_render_runtime::configure({
      &is_render_worker, &layer_effect_boundary_is_live});
  return true;
}();
int32_t publish_loaded_layer_receipt(
    const AegpLayerRenderOptionsValue& options, void** receipt) {
  return aexcompat::aegp_layer_render_runtime::publish(options, receipt);
}
int32_t __cdecl checkout_item_frame_async(
    void* manager, uint32_t purpose, void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (manager != &g_async_manager || purpose == 0) return 4;
  return aexcompat::aegp_item_render_runtime::publish_receipt(options, receipt);
}
int32_t __cdecl checkout_layer_frame_async(
    void* manager, uint32_t purpose, void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (manager != &g_async_manager || purpose == 0 || !receipt ||
      !snapshot_layer_render_options(options, snapshot)) return 4;
  if (is_render_worker() && aexcompat::aegp_layer_render_runtime::active())
    return publish_loaded_layer_receipt(snapshot, receipt);
  const int32_t pixel_format = snapshot.world_type == 1 ? kPixelFormatArgb32 :
      (snapshot.world_type == 2 ? kPixelFormatArgb64 : kPixelFormatArgb128);
  return aexcompat::aegp_item_render_runtime::publish_synthetic(
      pixel_format, receipt);
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
  return aexcompat::aegp_item_render_runtime::checkout(
      options, check_cancel, cancel_refcon, out);
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
int async_layer_exception_filter(EXCEPTION_POINTERS* information,
                                 uint32_t* exception_code) {
  if (exception_code && information && information->ExceptionRecord)
    *exception_code = information->ExceptionRecord->ExceptionCode;
  return EXCEPTION_EXECUTE_HANDLER;
}
int32_t invoke_async_layer_callback_seh(AegpAsyncFrameReadyCallback callback,
    uint64_t request_id, uint8_t canceled, int32_t error, void* receipt,
    void* refcon, int32_t* callback_error, uint32_t* exception_code) {
  if (!callback || !callback_error || !exception_code) return 4;
  *callback_error = 4; *exception_code = 0;
  __try { *callback_error = callback(request_id, canceled, error, receipt, refcon); return 0; }
  __except(async_layer_exception_filter(GetExceptionInformation(), exception_code)) {
    return 4;
  }
}
int32_t __cdecl render_checkout_layer_async_reject(
    void* options, AegpAsyncFrameReadyCallback callback, void* refcon, uint64_t* id) {
  return aexcompat::aegp_async_layer::checkout(options, callback, refcon, id);
}
int32_t __cdecl render_cancel_async_reject(uint64_t id) {
  return aexcompat::aegp_async_layer::cancel(id);
}
void drain_async_layer_requests() { aexcompat::aegp_async_layer::drain(); }
bool async_layer_requests_balanced() { return aexcompat::aegp_async_layer::balanced(); }
const bool g_async_layer_runtime_configured = [] {
  aexcompat::aegp_async_layer::configure({&is_render_worker,
      &snapshot_layer_render_options,
      &aexcompat::aegp_layer_render_runtime::capture_async_source,
      &aexcompat::aegp_layer_render_runtime::publish_async_source,
      &checkin_frame_if_live,
      &invoke_async_layer_callback_seh});
  return true;
}();
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
int32_t __cdecl render_timestamp_reject(void* output) {
  return aexcompat::aegp_external_render_runtime::timestamp(output);
}
int32_t __cdecl render_changed_reject(void* item, const void* start,
    const void* duration, const void* timestamp, uint8_t* out) {
  return aexcompat::aegp_external_render_runtime::changed(
      item, start, duration, timestamp, out);
}
int32_t __cdecl render_worthwhile_reject(
    void* options, const void* timestamp, uint8_t* out) {
  return aexcompat::aegp_external_render_runtime::worthwhile(options, timestamp, out);
}
int32_t __cdecl render_checkin_rendered(
    void* options, const void* timestamp, uint32_t ticks, void* image) {
  return aexcompat::aegp_external_render_runtime::checkin_rendered(
      options, timestamp, ticks, image);
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

bool world_lifetimes_balanced();

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

int32_t invoke_global_setdown(EffectEntry entry, void* input, void* output) {
  uint32_t exception_code{};
  const int32_t error = invoke_entry_seh(entry, kGlobalSetdown, input, output,
                                         nullptr, nullptr, nullptr,
                                         &exception_code);
  on_global_setdown();
  aexcompat::pf_helper::reset();
  return error;
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

std::string suite_timeline_report_json() {
  return suite_registry().suite_timeline_report_json();
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
  if (effect != &g_effect || !effect_is_live() || !comp_time || !camera_layer ||
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
  if (effect != &g_effect || !effect_is_live() || !comp_time ||
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

bool mask_suite_provider_available(void*) { return g_mask_model_enabled; }

bool render_options4_provider_available(void*) {
  return is_render_worker() && aexcompat::aegp_layer_render_runtime::active();
}

bool render_suite2_provider_available(void*) {
  return g_aegp_command_roundtrip_mode ||
      (is_render_worker() && aexcompat::aegp_layer_render_runtime::active());
}

bool aegp_init_suite_provider_available(void*) { return g_aegp_init_mode; }
bool render_worker_suite_provider_available(void*) { return is_render_worker(); }

const void* provide_batch_sampling1(void*) {
  g_batch_sampling_suite1 = {&begin_sampling8, &end_sampling8,
      &unsupported_batch_sample_func, &unsupported_batch_sample_func};
  return &g_batch_sampling_suite1;
}
const void* provide_color_settings7(void*) {
  configure_host_hooks({&composition_handle, &acquire_suite, &release_suite});
  return aexcompat::color_settings::suite();
}
const void* provide_iterate8(void*) {
  g_iterate8_suite2.iterate = reinterpret_cast<void*>(&iterate_world8); return &g_iterate8_suite2;
}
const void* provide_sampling8(void*) {
  g_sampling8_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
  g_sampling8_suite1[0] = reinterpret_cast<void*>(&nearest_sample8);
  g_sampling8_suite1[1] = reinterpret_cast<void*>(&subpixel_sample8);
  g_sampling8_suite1[2] = reinterpret_cast<void*>(&area_sample8); return g_sampling8_suite1.data();
}
const void* provide_sampling16(void*) {
  g_sampling16_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
  g_sampling16_suite1[0] = reinterpret_cast<void*>(&nearest_sample16);
  g_sampling16_suite1[1] = reinterpret_cast<void*>(&subpixel_sample16);
  g_sampling16_suite1[2] = reinterpret_cast<void*>(&area_sample16); return g_sampling16_suite1.data();
}
const void* provide_sampling_float(void*) {
  g_sampling_float_suite1.fill(reinterpret_cast<void*>(&aegp_unsupported_suite_call));
  g_sampling_float_suite1[0] = reinterpret_cast<void*>(&nearest_sample_float);
  g_sampling_float_suite1[1] = reinterpret_cast<void*>(&subpixel_sample_float);
  g_sampling_float_suite1[2] = reinterpret_cast<void*>(&area_sample_float); return g_sampling_float_suite1.data();
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

bool configure_component_suite_catalog() {
  using namespace aexcompat::worker_runtime::host_suites;
  const AssemblyHooks assembly{
      reinterpret_cast<void*>(&aegp_unsupported_suite_call),
      {reinterpret_cast<void*>(&aexcompat::pf_path_runtime::num_paths),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_info),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::checkout_path),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::checkin_path)},
      {reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_is_open),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_num_segments),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_vertex_info),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_prepare_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_get_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_eval_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_eval_seg_length_deriv1),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_cleanup_seg_length),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_is_inverted),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_get_mask_mode),
       reinterpret_cast<void*>(&aexcompat::pf_path_runtime::path_get_name)},
      reinterpret_cast<void*>(&duck_quack),
      reinterpret_cast<void*>(&set_options_button_name),
      reinterpret_cast<void*>(&adv_app_info_text),
      reinterpret_cast<void*>(&adv_app_info_text3),
      {reinterpret_cast<void*>(&drawbot_get_supplier),
       reinterpret_cast<void*>(&drawbot_get_surface)},
      reinterpret_cast<void*>(&drawbot_new_pen),
      reinterpret_cast<void*>(&drawbot_new_brush),
      reinterpret_cast<void*>(&drawbot_new_path),
      reinterpret_cast<void*>(&drawbot_release_object),
      reinterpret_cast<void*>(&drawbot_paint_rect),
      reinterpret_cast<void*>(&drawbot_fill_path),
      reinterpret_cast<void*>(&drawbot_stroke_path),
      reinterpret_cast<void*>(&drawbot_path_point),
      reinterpret_cast<void*>(&drawbot_add_rect),
      reinterpret_cast<void*>(&get_drawing_reference),
      reinterpret_cast<void*>(&get_context_async_manager),
      reinterpret_cast<void*>(&overlay_foreground),
      reinterpret_cast<void*>(&overlay_stroke_path),
      {reinterpret_cast<void*>(&app_get_background_color),
       reinterpret_cast<void*>(&app_get_color),
       reinterpret_cast<void*>(&app_get_language),
       reinterpret_cast<void*>(&app_get_personal_info),
       reinterpret_cast<void*>(&app_get_font_style),
       reinterpret_cast<void*>(&app_set_cursor),
       reinterpret_cast<void*>(&app_is_render_engine),
       reinterpret_cast<void*>(&app_color_picker),
       reinterpret_cast<void*>(&app_get_mouse),
       reinterpret_cast<void*>(&app_invalidate_rect),
       reinterpret_cast<void*>(&app_convert_local_to_global),
       reinterpret_cast<void*>(&app_get_color_at_global_point),
       reinterpret_cast<void*>(&app_create_progress_dialog),
       reinterpret_cast<void*>(&app_update_progress_dialog),
       reinterpret_cast<void*>(&app_dispose_progress_dialog)},
      {reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_atan), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_atan2),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_ceil), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_cos),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_exp), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fabs),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_floor), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fmod),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_hypot), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_log),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_log10), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_pow),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sin), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sqrt),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_tan), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sprintf),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy), reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_asin),
       reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_acos)},
      reinterpret_cast<void*>(&aegp_set_dynamic_stream_flag_v2),
      {reinterpret_cast<void*>(&aegp_world_new_owned), reinterpret_cast<void*>(&aegp_world_dispose), reinterpret_cast<void*>(&aegp_world_get_type), reinterpret_cast<void*>(&aegp_world_get_size), reinterpret_cast<void*>(&aegp_world_get_rowbytes), reinterpret_cast<void*>(&aegp_world_get_base_addr8), reinterpret_cast<void*>(&aegp_world_get_base_addr16), reinterpret_cast<void*>(&aegp_world_get_base_addr32), reinterpret_cast<void*>(&aegp_world_fill_pf_world), reinterpret_cast<void*>(&aegp_world_fast_blur), reinterpret_cast<void*>(&aegp_world_new_platform), reinterpret_cast<void*>(&aegp_world_dispose_platform), reinterpret_cast<void*>(&aegp_world_reference_platform)},
      {reinterpret_cast<void*>(&new_layer_render_options), reinterpret_cast<void*>(&new_from_upstream_of_effect), reinterpret_cast<void*>(&duplicate_layer_render_options), reinterpret_cast<void*>(&dispose_layer_render_options), reinterpret_cast<void*>(&set_layer_render_time), reinterpret_cast<void*>(&get_layer_render_time), reinterpret_cast<void*>(&set_layer_render_time_step), reinterpret_cast<void*>(&get_layer_render_time_step), reinterpret_cast<void*>(&set_layer_render_world_type), reinterpret_cast<void*>(&get_layer_render_world_type), reinterpret_cast<void*>(&set_layer_render_downsample), reinterpret_cast<void*>(&get_layer_render_downsample), reinterpret_cast<void*>(&set_layer_render_matte), reinterpret_cast<void*>(&get_layer_render_matte)},
      {reinterpret_cast<void*>(&new_layer_render_options), reinterpret_cast<void*>(&new_from_upstream_of_effect), reinterpret_cast<void*>(&new_from_downstream_of_effect), reinterpret_cast<void*>(&duplicate_layer_render_options), reinterpret_cast<void*>(&dispose_layer_render_options), reinterpret_cast<void*>(&set_layer_render_time), reinterpret_cast<void*>(&get_layer_render_time), reinterpret_cast<void*>(&set_layer_render_time_step), reinterpret_cast<void*>(&get_layer_render_time_step), reinterpret_cast<void*>(&set_layer_render_world_type), reinterpret_cast<void*>(&get_layer_render_world_type), reinterpret_cast<void*>(&set_layer_render_downsample), reinterpret_cast<void*>(&get_layer_render_downsample), reinterpret_cast<void*>(&set_layer_render_matte), reinterpret_cast<void*>(&get_layer_render_matte)},
      {reinterpret_cast<void*>(&render_options_new_from_item), reinterpret_cast<void*>(&render_options_duplicate), reinterpret_cast<void*>(&render_options_dispose), reinterpret_cast<void*>(&render_options_set_time), reinterpret_cast<void*>(&render_options_get_time), reinterpret_cast<void*>(&render_options_set_time_step), reinterpret_cast<void*>(&render_options_get_time_step), reinterpret_cast<void*>(&render_options_set_field), reinterpret_cast<void*>(&render_options_get_field), reinterpret_cast<void*>(&render_options_set_world_type), reinterpret_cast<void*>(&render_options_get_world_type), reinterpret_cast<void*>(&render_options_set_downsample), reinterpret_cast<void*>(&render_options_get_downsample), reinterpret_cast<void*>(&render_options_set_roi), reinterpret_cast<void*>(&render_options_get_roi), reinterpret_cast<void*>(&render_options_set_matte), reinterpret_cast<void*>(&render_options_get_matte)},
      {reinterpret_cast<void*>(&render_options_new_from_item), reinterpret_cast<void*>(&render_options_duplicate), reinterpret_cast<void*>(&render_options_dispose), reinterpret_cast<void*>(&render_options_set_time), reinterpret_cast<void*>(&render_options_get_time), reinterpret_cast<void*>(&render_options_set_time_step), reinterpret_cast<void*>(&render_options_get_time_step), reinterpret_cast<void*>(&render_options_set_field), reinterpret_cast<void*>(&render_options_get_field), reinterpret_cast<void*>(&render_options_set_world_type), reinterpret_cast<void*>(&render_options_get_world_type), reinterpret_cast<void*>(&render_options_set_downsample), reinterpret_cast<void*>(&render_options_get_downsample), reinterpret_cast<void*>(&render_options_set_roi), reinterpret_cast<void*>(&render_options_get_roi), reinterpret_cast<void*>(&render_options_set_matte), reinterpret_cast<void*>(&render_options_get_matte), reinterpret_cast<void*>(&render_options_set_channel_order), reinterpret_cast<void*>(&render_options_get_channel_order), reinterpret_cast<void*>(&render_options_get_guide_layers), reinterpret_cast<void*>(&render_options_set_guide_layers), reinterpret_cast<void*>(&render_options_get_quality), reinterpret_cast<void*>(&render_options_set_quality)},
      {reinterpret_cast<void*>(&render_checkout_frame_reject), reinterpret_cast<void*>(&checkin_frame), reinterpret_cast<void*>(&get_receipt_world), reinterpret_cast<void*>(&render_get_region_reject), reinterpret_cast<void*>(&render_sufficient_reject), reinterpret_cast<void*>(&render_sound_reject), reinterpret_cast<void*>(&render_timestamp_reject), reinterpret_cast<void*>(&render_changed_reject), reinterpret_cast<void*>(&render_worthwhile_reject), reinterpret_cast<void*>(&render_checkin_rendered)},
      {reinterpret_cast<void*>(&render_checkout_frame_reject), reinterpret_cast<void*>(&render_checkout_layer_reject), reinterpret_cast<void*>(&checkin_frame), reinterpret_cast<void*>(&get_receipt_world), reinterpret_cast<void*>(&render_get_region_reject), reinterpret_cast<void*>(&render_sufficient_reject), reinterpret_cast<void*>(&render_sound_reject), reinterpret_cast<void*>(&render_timestamp_reject), reinterpret_cast<void*>(&render_changed_reject), reinterpret_cast<void*>(&render_worthwhile_reject), reinterpret_cast<void*>(&render_checkin_rendered), reinterpret_cast<void*>(&render_guid_reject)},
      {reinterpret_cast<void*>(&render_checkout_frame_reject), reinterpret_cast<void*>(&render_checkout_layer_v5), reinterpret_cast<void*>(&render_checkout_layer_async_reject), reinterpret_cast<void*>(&render_cancel_async_reject), reinterpret_cast<void*>(&checkin_frame), reinterpret_cast<void*>(&get_receipt_world), reinterpret_cast<void*>(&render_get_region_reject), reinterpret_cast<void*>(&render_sufficient_reject), reinterpret_cast<void*>(&render_sound_reject), reinterpret_cast<void*>(&render_timestamp_reject), reinterpret_cast<void*>(&render_changed_reject), reinterpret_cast<void*>(&render_worthwhile_reject), reinterpret_cast<void*>(&render_checkin_rendered), reinterpret_cast<void*>(&render_guid_reject)},
      {reinterpret_cast<void*>(&checkout_item_frame_async), reinterpret_cast<void*>(&checkout_layer_frame_async)}};
  if (!configure_suite_assembly(assembly)) return false;
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
      {"PF Pixel Data Suite", 1, &g_pixel_data_suite1},
      {"PF Pixel Data Suite", 2, &g_pixel_data_suite2},
      {"PF World Suite", 1, g_world_suite1.data()},
      {"PF World Suite", 2, &g_world_suite},
      {"PF Pixel Format Suite", 2, &g_pixel_format_suite},
      {"PF PointParamSuite", 1, &g_point_param_suite},
      {"PF AngleParamSuite", 1, &g_angle_param_suite},
      {"PF ColorParamSuite", 1, &g_color_param_suite1},
      {"PF Param Utils Suite", 2, &g_param_utils_suite1},
      {"PF Param Utils Suite", 3, &g_param_utils_suite},
      {"AEGP PF Interface Suite", 1, &g_pf_interface_suite},
      {"AEGP World Suite", 3, nullptr, &provide_aegp_world_suite3},
      {"AEGP Layer Render Options Suite", 1, nullptr,
       &provide_layer_render_options1},
      {"AEGP Layer Render Options Suite", 2, nullptr,
       &provide_layer_render_options2},
      {"AEGP Render Options Suite", 1, nullptr, &provide_render_options1},
      {"AEGP Render Options Suite", 4, nullptr, &provide_render_options4,
       nullptr, &render_options4_provider_available},
      {"AEGP Render Suite", 2, nullptr, &provide_render_suite2, nullptr,
       &render_suite2_provider_available},
      {"AEGP Render Suite", 5, nullptr, &provide_render_suite5},
      {"AEGP Render Suite", 8, nullptr, &provide_render_suite8},
      {"AEGP Render Asyc Manager Suite", 1, nullptr,
       &provide_render_async_manager1},
      {"AEGP Mask Suite", 1, &g_pf_mask_suite1, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Layer Mask Suite", 6, &g_mask_suite5, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Layer Mask Suite", 7, &g_mask_suite, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Stream Suite", 11, &g_stream_suite, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Keyframe Suite", 5, &g_keyframe_suite, nullptr, nullptr,
       &mask_suite_provider_available},
      {"AEGP Dynamic Stream Suite", 5, &g_dynamic_stream_suite,
       nullptr, nullptr, &mask_suite_provider_available},
      {"AEGP Mask Outline Suite", 5, &g_mask_outline_suite,
       nullptr, nullptr, &mask_suite_provider_available},
      {"PF Color Suite", 1, &g_color_suite8},
      {"PF Color16 Suite", 1, &g_color_suite16},
      {"PF ColorFloat Suite", 1, &g_color_suite_float},
      {"PF Batch Sampling Suite", 1, nullptr, &provide_batch_sampling1},
      {"PF Path Query Suite", 1, nullptr, &provide_path_query1, nullptr,
       &mask_suite_provider_available},
      {"PF Path Data Suite", 1, nullptr, &provide_path_data1, nullptr,
       &mask_suite_provider_available},
      {"AEGP Duck Suite", 1, nullptr, &provide_duck1},
      {"AEGP Command Suite", 1, &g_aegp_command_suite, nullptr, nullptr,
       &aegp_init_suite_provider_available},
      {"AEGP Register Suite", 6, &g_aegp_register_suite, nullptr, nullptr,
       &aegp_init_suite_provider_available},
      {"PF Effect UI Suite", 1, nullptr, &provide_effect_ui1},
      {"PF AE Adv App Suite", 1, nullptr, &provide_adv_app1},
      {"PF AE Adv App Suite", 2, nullptr, &provide_adv_app2},
      {"DRAWBOT Draw Suite", 1, nullptr, &provide_drawbot_draw1},
      {"DRAWBOT Supplier Suite", 1, nullptr, &provide_drawbot_supplier1},
      {"DRAWBOT Surface Suite", 2, nullptr, &provide_drawbot_surface2},
      {"DRAWBOT Path Suite", 1, nullptr, &provide_drawbot_path1},
      {"PF Effect Custom UI Suite", 1, nullptr, &provide_custom_ui1},
      {"PF Effect Custom UI Suite", 2, nullptr, &provide_custom_ui2},
      {"PF Effect Custom UI Overlay Theme Suite", 1, nullptr,
       &provide_overlay_theme1},
      {"PF AE App Suite", 6, nullptr, &provide_app_suite4},
      {"PF AE App Suite", 7, nullptr, &provide_app_suite5},
      {"PF AE App Suite", 1, nullptr, &provide_app_suite6},
      {"PF AE Channel Suite", 1, &g_channel_suite1},
      {"PF Effect Sequence Data Suite", 1, &g_effect_sequence_data_suite1},
      {"PF Handle Suite", 2, &g_handle_suite},
      {"PF GPU Device Suite", 1, g_gpu_device_suite1.data()},
      {"PF ANSI Suite", 1, nullptr, &provide_ansi1},
      {"PF AE Adv Item Suite", 1, &g_adv_item_suite1, nullptr, nullptr,
       &render_worker_suite_provider_available},
      {"PF Color Settings Suite", 7, nullptr, &provide_color_settings7},
      {"PF Iterate8 Suite", 1, nullptr, &provide_iterate8},
      {"PF Iterate8 Suite", 2, nullptr, &provide_iterate8},
      {"PF iterate16 Suite", 1, &g_iterate16_suite2},
      {"PF iterate16 Suite", 2, &g_iterate16_suite2},
      {"PF iterateFloat Suite", 1, &g_iterate_float_suite2},
      {"PF iterateFloat Suite", 2, &g_iterate_float_suite2},
      {"PF Sampling8 Suite", 1, nullptr, &provide_sampling8},
      {"PF Sampling16 Suite", 1, nullptr, &provide_sampling16},
      {"PF SamplingFloat Suite", 1, nullptr, &provide_sampling_float},
      {"PF World Transform Suite", 1, nullptr,
       &aexcompat::pf_world_transform::provide_world_transform1},
      {"PF Fill Matte Suite", 2, nullptr,
       &aexcompat::pf_world_transform::provide_fill_matte2},
      {"AEGP Dynamic Stream Suite", 2, nullptr, &provide_dynamic_stream2},
  };
  return configure_host_suite_catalog(
      {component_suites, std::size(component_suites),
       {&resolve_scene_suite_provider, nullptr}});
}

int32_t __cdecl acquire_suite(const char* name, int32_t version,
                              const void** suite) {
  using namespace aexcompat::worker_runtime::host_suites;
  static const bool configured = configure_component_suite_catalog();
  if (!configured) {
    if (suite) *suite = nullptr;
    return 4;
  }
  return acquire_catalog_suite(name, version, suite, g_trace_writer);
}
int32_t __cdecl release_suite(const char* name, int32_t version) {
  return aexcompat::worker_runtime::host_suites::release_catalog_suite(
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

std::size_t parameter_active_mask_count() {
  return ordered_active_masks().size();
}
bool parameter_active_mask_id(std::size_t index, int32_t* id) {
  if (!id) return false;
  const auto masks = ordered_active_masks();
  if (index >= masks.size()) return false;
  *id = masks[index]->id;
  return true;
}
const bool g_parameter_execution_configured = configure_hooks({
    &invoke_entry_seh, &host_handle_is_live, &parameter_active_mask_count,
    &parameter_active_mask_id});

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
  activate_external_aux();
}

void lifecycle_cleanup_aux(void*) {
  // Aux channel chunks are host-owned and cannot outlive a render lifecycle.
  aexcompat::pf_ae_channel::reclaim_layer_channels();
  deactivate_external_aux();
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

// Owns the references that must remain alive from lifecycle begin through end.
// The opaque hook ABI never outlives this stack owner.
struct ClassicLifecycleOwner {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  std::vector<std::array<std::byte, kParamSize>>& definitions;
  std::vector<void*>& params;
  std::array<std::byte, kEffectWorldSize>& world;
  bool manage_sequence;

  aexcompat::worker_runtime::classic_execution::LifecycleResult begin() {
    return aexcompat::worker_runtime::classic_execution::begin_lifecycle(this, hooks());
  }
  int32_t finish(aexcompat::worker_runtime::classic_execution::LifecycleResult& state,
                 bool draw = false) {
    return aexcompat::worker_runtime::classic_execution::finish_lifecycle(
        this, state, hooks(), draw);
  }

 private:
  static const aexcompat::worker_runtime::classic_execution::LifecycleHooks& hooks() {
    static const aexcompat::worker_runtime::classic_execution::LifecycleHooks value{
        +[](void* opaque) -> void* {
          auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          const auto lifecycle = h.manage_sequence
              ? begin_render_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data())
              : begin_frame_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data());
          return new (std::nothrow) RenderLifecycle(lifecycle);
        },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return dispatch_render_click(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return interpolate_arbitrary_values(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return roundtrip_arbitrary_values(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return dispatch_conditional_ui_selectors(h.entry, h.input, h.output, h.params.data()); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return dispatch_render_draw(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque, void* lifecycle, int32_t error) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return h.manage_sequence
              ? end_render_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data(),
                    *static_cast<RenderLifecycle*>(lifecycle), error)
              : end_frame_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data(),
                    *static_cast<RenderLifecycle*>(lifecycle), error); },
        +[](void* lifecycle) { delete static_cast<RenderLifecycle*>(lifecycle); }};
    return value;
  }
};

// Owns render-dispatch state. Buffer resize mutates all related world references
// atomically; LayerRenderContext is scoped strictly to the kRender callback.
struct ClassicRenderDispatchOwner {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  std::array<std::byte, kEffectWorldSize>& world;
  OutputPixelBuffer& guarded;
  DispatchWorldFormatScope& worlds;
  std::vector<std::array<std::byte, kParamSize>>& definitions;
  std::vector<void*>& params;
  int32_t& width; int32_t& height; int32_t& rowbytes;
  unsigned char*& destination;
  int32_t pixel_bytes; int32_t pixel_format;
  int32_t current_time; int32_t time_step; int32_t total_time; uint32_t time_scale;
  const std::string& case_id; const RequestedAssignments* requested;
  const std::vector<unsigned char>* external_rgba;
  const std::vector<ExternalLayerInput>* external_layers;
  int32_t external_width; int32_t external_height;
  aexcompat::worker_runtime::classic::Context& classic_context;
  std::vector<unsigned char>& logical_source;

  int32_t run(int32_t error) {
    return aexcompat::worker_runtime::classic_execution::dispatch_render(this, error, hooks());
  }

 private:
  static const aexcompat::worker_runtime::classic_execution::RenderHooks& hooks() {
    static const aexcompat::worker_runtime::classic_execution::RenderHooks value{
        +[](void* opaque) { auto& h = *static_cast<ClassicRenderDispatchOwner*>(opaque);
          return dispatch_render_draw(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { return static_cast<ClassicRenderDispatchOwner*>(opaque)->prepare_output(); },
        +[](void* opaque) { return static_cast<ClassicRenderDispatchOwner*>(opaque)->dispatch_selector(); },
        +[](void* opaque) { auto& h = *static_cast<ClassicRenderDispatchOwner*>(opaque);
          return !g_render_ui_context_active || close_render_ui_context(h.entry, h.input, h.output, h.definitions); }};
    return value;
  }
  int32_t prepare_output() {
    const int32_t next_width = read<int32_t>(output, kOutWidth);
    const int32_t next_height = read<int32_t>(output, kOutHeight);
    if (!aexcompat::render::validate_output_extent(width, height, next_width, next_height,
            read<uint32_t>(output, kOutFlags))) return 4;
    if (next_width <= 0 || next_height <= 0) return 0;
    width = next_width; height = next_height; rowbytes = width * pixel_bytes;
    if (!guarded.reset(static_cast<std::size_t>(rowbytes) * height)) return -3;
    destination = guarded.data();
    if (!aexcompat::render::prepare_world_layout(world,
            {pixel_bytes == 4 ? 0 : 1, pixel_bytes, width, height, rowbytes}, destination)) return -3;
    if (!worlds.register_world(world.data(), pixel_format)) return 4;
    write<int32_t>(input, 276, read<int32_t>(output, kOutOrigin));
    write<int32_t>(input, 280, read<int32_t>(output, kOutOrigin + 4));
    return 0;
  }
  int32_t dispatch_selector() {
    struct LayerContextScope {
      LayerRenderContext previous;
      explicit LayerContextScope(LayerRenderContext next)
          : previous(aexcompat::aegp_layer_render_runtime::replace_context(std::move(next))) {}
      ~LayerContextScope() {
        aexcompat::aegp_layer_render_runtime::replace_context(std::move(previous));
      }
    } scope({entry, &input, &output, current_time, static_cast<int32_t>(time_scale), case_id,
        requested, external_rgba, external_layers, external_width, external_height,
        time_step, total_time, pixel_bytes, &logical_source, width, height});
    classic_context.mark_selector_dispatched();
    return entry(kRender, input.data(), output.data(), params.data(), world.data(), nullptr);
  }
};
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
  ClassicLifecycleOwner lifecycle_owner{entry, input, command_output, definitions, params,
                                        output_world, manage_sequence};
  auto lifecycle = lifecycle_owner.begin();
  int32_t error = lifecycle.error;
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
    error = lifecycle_owner.finish(lifecycle);
  } else {
    ClassicRenderDispatchOwner dispatch_owner{entry, input, command_output, output_world,
        guarded, dispatch_worlds, definitions, params, width, height, rowbytes, destination,
        pixel_bytes, dispatch_pixel_format, external_current_time, external_time_step,
        external_total_time, external_time_scale, case_id, requested, external_rgba,
        external_layers, external_width, external_height, *classic_context, logical_source};
    error = dispatch_owner.run(error);
    lifecycle.error = error;
    error = lifecycle_owner.finish(lifecycle);
  }
  aexcompat::worker_runtime::classic_execution::Context final_context{
      destination, rowbytes, width, height, pixel_bytes, error,
      external_current_time, external_time_step, external_time_scale,
      read<int32_t>(input, kInQuality), dispatch_pixel_format, &output_hash,
      &guards_intact, captured_argb, external_output, guarded.sentinels_intact()};
  error = aexcompat::worker_runtime::classic_execution::finalize(final_context, {
      +[](const unsigned char* data, int32_t rowbytes, int32_t width, int32_t height,
          int32_t bytes, std::vector<unsigned char>& output) {
        return aexcompat::render::copy_packed_world(data, rowbytes, width, height, bytes, output);
      }, &sha256_bytes,
      +[](AegpTime time, AegpTime step, int8_t quality, int32_t format,
          int32_t width, int32_t height, const void* pixels) {
        return aexcompat::aegp_staged_item_runtime::publish_world(aegp_comp_item_handle(),
            time, step, quality, 0, format, width, height,
            width * (format == kPixelFormatArgb32 ? 4 :
                (format == kPixelFormatArgb64 ? 8 : 16)), pixels);
      },
      +[](const void* pixels, int32_t width, int32_t height, int32_t bytes) {
        dump_world_snapshot("classic-output",
            static_cast<const unsigned char*>(pixels), width, height, bytes);
      },
      +[](unsigned char* destination, const unsigned char* source, int32_t bytes) {
        argb_to_rgba_native(destination, source, bytes);
      }, &record_output_checksum_detail,
      +[](const char* format) { smart_state().pixel_format = format; }});
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
  aexcompat::aegp_layer_render_runtime::context() = {
      entry, &input, &output, read<int32_t>(input, kInCurrentTime),
      static_cast<int32_t>(read<uint32_t>(input, kInTimeScale))};
  std::vector<uint8_t> final_stage(16 * 12 * 4);
  for (std::size_t pixel = 0; pixel < final_stage.size() / 4; ++pixel) {
    final_stage[pixel * 4] = static_cast<uint8_t>(64 + pixel % 191);
    final_stage[pixel * 4 + 1] = static_cast<uint8_t>(pixel * 17);
    final_stage[pixel * 4 + 2] = static_cast<uint8_t>(pixel * 29);
    final_stage[pixel * 4 + 3] = static_cast<uint8_t>(pixel * 43);
  }
  const bool stage_published = aexcompat::aegp_staged_item_runtime::publish_world(aegp_comp_item_handle(),
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
  aexcompat::aegp_layer_render_runtime::context() = {};
  g_loaded_effect_receipt_fixture_passed = stage_published && checked_out && checked_in && options_disposed &&
      g_loaded_effect_receipt_unsupported_rejected &&
      g_loaded_effect_receipt_stale_world_rejected &&
      async_receipt_lifetimes_balanced() && render_options_lifetimes_balanced();
  return g_loaded_effect_receipt_fixture_passed;
}
using SmartResult = aexcompat::worker_runtime::smart_execution::Result;

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
  const auto plan = aexcompat::worker_runtime::smart_setup::prepare(
      {g_secondary_layer_slot, g_full_resolution_width, g_full_resolution_height,
       g_pixel_aspect_ratio.numerator, g_pixel_aspect_ratio.denominator},
      {&command_output, &case_id, external_rgba != nullptr, external_width,
       external_height, external_current_time, external_time_scale,
       external_pixel_bytes});
  if (!plan.valid) return result;
  const bool deep16 = plan.deep16;
  const bool fixture_gpu_negotiation = plan.fixture_gpu_negotiation;
  const bool opencl_gpu_negotiation = plan.opencl_gpu_negotiation;
  const bool directx_gpu_negotiation = plan.directx_gpu_negotiation;
  const bool explicit_gpu_device = plan.explicit_gpu_device;
  const uint32_t gpu_device_index = plan.gpu_device_index;
  const bool gpu_negotiation = plan.gpu_negotiation;
  const bool missing_input = plan.missing_input;
  const bool temporal_context = plan.temporal_context;
  const bool partial_output_request = plan.partial_output_request;
  const bool float32 = plan.float32;
  const bool connected_map = plan.connected_map;
  const int32_t width = plan.width;
  const int32_t height = plan.height;
  const int32_t pixel_bytes = plan.pixel_bytes;
  const int32_t rowbytes = plan.rowbytes;
  InputPixelBuffer source(static_cast<std::size_t>(rowbytes) * height);
  OutputPixelBuffer guarded(static_cast<std::size_t>(rowbytes) * height);
  auto* destination = guarded.data();
  result.guards_intact = true;
  std::array<std::byte, 120> input_world{}, output_world{};
  std::array<std::byte, 120> input_checkout_view{}, map_checkout_view{};
  DispatchWorldFormatScope dispatch_worlds;
  aexcompat::render::MapWorld map_world;
  const bool input_write_advertised =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagIWriteInputBuffer) != 0;
  if (!aexcompat::worker_runtime::smart_setup::prepare_world_buffers(
          plan, case_id, external_rgba, input_write_advertised,
          {&source, &guarded, &destination, &input_world, &output_world,
           &input_checkout_view, &map_checkout_view, &dispatch_worlds,
           &map_world})) return result;
  const int32_t dispatch_pixel_format = float32 ? kPixelFormatArgb128 :
      (deep16 ? kPixelFormatArgb64 : kPixelFormatArgb32);
  aexcompat::worker_runtime::smart_setup::ParameterState parameter_state(
      g_params.size() + 1, external_layers ? external_layers->size() : 0);
  auto& definitions = parameter_state.definitions;
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (!initialize_arbitrary_values(entry, input, command_output, definitions)) return result;
  ArbitraryValuesScope arbitrary_scope{entry, &input, &command_output, &definitions};
  if (!aexcompat::worker_runtime::smart_setup::prepare_parameters(
          {entry, &input, &command_output, &case_id, &plan, requested,
           external_layers, external_current_time, external_time_step,
           external_total_time, external_time_scale, g_full_resolution_width,
           g_full_resolution_height, dispatch_pixel_format, &input_world,
           &dispatch_worlds, &source},
          parameter_state, {&apply_parameter_animation, &dump_world_snapshot}))
    return result;
  auto& params = parameter_state.params;
  auto& pre_render_source = parameter_state.pre_render_source;
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
    result.output_extent_hint = result.result_rect;
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
  if (!aexcompat::worker_runtime::smart_render_runtime::execute(
          {entry, &input, &command_output, &plan, &parameter_state, &input_world,
           &output_world, &dispatch_worlds, &source, &guarded, &destination,
           &lifecycle, external_output, dispatch_pixel_format, width, height,
           rowbytes, pixel_bytes},
          {&dispatch_render_draw,
           {&guarded_effect_call, &capture_module_audit,
            reinterpret_cast<void*>(&guid_mix_in_ptr),
            &automatic_checkin_pre_render_params},
           {&close_render_ui_context, &end_render_lifecycle, &dump_world_snapshot,
            &record_output_checksum_detail, &sha256_bytes,
            +[] { return g_render_ui_context_active; }}}, result))
    return result;
  return result;
}

const bool g_smart_execution_configured =
    aexcompat::worker_runtime::smart_execution::configure({
        &smart_render_runtime, +[] { return g_module_audit.required; }});

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
  return aexcompat::worker_runtime::smart_execution::render_once(
      entry, input, output, case_id, requested, external_rgba, external_output,
      external_width, external_height, external_layers, external_current_time,
      external_time_step, external_total_time, external_time_scale,
      external_pixel_bytes);
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
  const auto pf_state_stats = aexcompat::pf_state_runtime::pf_state_statistics();
  c.pf_get_current_state_calls = pf_state_stats.get_current_state_calls;
  c.pf_are_states_identical_calls = pf_state_stats.are_states_identical_calls;
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

const aexcompat::l2mode::Hooks& early_mode_hooks() {
  static const aexcompat::l2mode::Hooks hooks{
      early_mode_out_flags, early_mode_copy_sequence_data_to_input,
      early_mode_sequence_setup, early_mode_sequence_setdown, early_mode_do_dialog,
      early_mode_global_setdown, early_mode_return_message,
      early_mode_handle_lifetimes_balanced, early_mode_prepare_protocol_report,
      early_mode_external_dependencies, early_mode_handle_is_live,
      early_mode_handle_size, early_mode_lock_handle, early_mode_unlock_handle,
      early_mode_dispose_handle, early_mode_handle_statistics,
      early_mode_dispose_arbitrary_defaults, early_mode_report_parameters};
  return hooks;
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
  const auto saved_context = aexcompat::aegp_layer_render_runtime::context();
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
  LayerRenderContext context{};
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
  aexcompat::aegp_layer_render_runtime::context() = context;

  const void* acquired = nullptr;
  bool ok = acquire_suite("AEGP Layer Render Options Suite", 2, &acquired) == 0 &&
      acquired == aexcompat::worker_runtime::host_suites::layer_render_options_suite(2);
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
  aexcompat::aegp_layer_render_runtime::context().downstream_argb = &downstream_pixels;
  aexcompat::aegp_layer_render_runtime::context().downstream_width = 4;
  aexcompat::aegp_layer_render_runtime::context().downstream_height = 2;
  aexcompat::aegp_layer_render_runtime::context().downstream_pixel_bytes = 4;
  aexcompat::aegp_layer_render_runtime::context().downstream_finalized = true;
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
  aexcompat::aegp_layer_render_runtime::context() = saved_context;
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
  ok = ok && suite1 == aexcompat::worker_runtime::host_suites::adv_app_suite(1) &&
       suite2 == aexcompat::worker_runtime::host_suites::adv_app_suite(2) &&
      suite1 != suite2 && slots1 && slots2;
  if (slots1 && slots2) {
    ok = ok && std::all_of(slots1, slots1 + 10,
                           [](void* callback) { return callback != nullptr; }) &&
        std::all_of(slots2, slots2 + 11,
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
  return load_external_aux_manifest(std::filesystem::path(value ? value : L""));
}

bool parse_l2_alpha_coverage(void*, const wchar_t* value) {
  return parse_alpha_coverage_params(value ? value : L"");
}

bool load_l2_parameter_animation(void*, const wchar_t* value) {
  return load_parameter_animation(std::filesystem::path(value ? value : L""),
                                  g_parameter_timelines);
}

bool __cdecl scene_render_receipt_enabled() {
  return is_render_worker() && aexcompat::aegp_layer_render_runtime::active();
}

aexcompat::worker_render_report::GpuDiagnosticsSnapshot capture_gpu_diagnostics() {
  const auto& directx = directx_backend::diagnostics();
  const auto i64 = [](auto value) { return static_cast<int64_t>(value); };
  return {
      gpu_memory_lifetimes_balanced(), g_cuda_upload_bytes > 0,
      {i64(g_cuda_upload_bytes), i64(g_cuda_download_bytes), i64(g_cuda_sync_failures),
       i64(g_last_cuda_device_count), i64(g_last_cuda_device_index)},
      g_opencl_upload_bytes > 0,
      {i64(g_opencl_upload_bytes), i64(g_opencl_download_bytes), i64(g_opencl_sync_failures),
       i64(opencl::last_device_count()), i64(opencl::last_device_index())},
      directx.context_used,
      {i64(directx.device_count), i64(directx.device_index), i64(directx.upload_bytes),
       i64(directx.download_bytes), i64(directx.sync_failures)},
      {i64(g_gpu_allocations_created), i64(g_gpu_allocations_freed),
       i64(gpu_transport::live_allocation_count()), i64(gpu_transport::live_memory_bytes()),
       i64(gpu_transport::exclusive_access_depth()), i64(g_invalid_gpu_memory_operations)}};
}

aexcompat::worker_render_report::SehDiagnosticsSnapshot capture_seh_diagnostics() {
  return {g_last_seh_exception_code, g_last_seh_exception_address,
          escape(g_last_seh_exception_module), escape(g_last_seh_selector), g_last_seh_error};
}

aexcompat::worker_render_report::ClassicSubsystemDiagnostics capture_classic_subsystems() {
  const auto i64 = [](auto value) { return static_cast<int64_t>(value); };
  const auto& handle_stats = statistics();
  const auto& world_stats = aexcompat::world_registry::statistics();
  const auto& receipt_stats = aexcompat::render_receipts::statistics();
  const auto path = aexcompat::pf_path_runtime::snapshot();
  return {
      suite_leases_balanced(),
      {i64(suite_acquire_count()), i64(suite_release_count()), i64(live_suite_lease_count()),
       i64(live_suite_reference_count())},
      missing_suites_report_json() + suite_timeline_report_json(), live_suite_lease_summary(), handle_lifetimes_balanced(),
      aexcompat::pf_path_runtime::lifetimes_balanced(),
      {i64(path.checkout_calls), i64(path.checkin_calls), i64(path.mask_calls),
       i64(path.preps_created), i64(path.preps_disposed),
       i64(path.invalid_operations), i64(path.reject_reason), i64(path.live_preps)},
      {path.last_feather_x, path.last_feather_y}, path.last_opacity,
      i64(path.last_quality),
      {i64(path.last_bounds[0]), i64(path.last_bounds[1]),
       i64(path.last_bounds[2]), i64(path.last_bounds[3])},
      {i64(handle_stats.created), i64(handle_stats.disposed)},
      {i64(g_arbitrary_copy_calls), i64(g_arbitrary_dispose_calls), i64(g_arbitrary_print_calls),
       i64(g_arbitrary_print_failures), i64(g_arbitrary_roundtrip_calls),
       i64(g_arbitrary_roundtrip_failures), i64(g_arbitrary_scan_calls),
       i64(g_arbitrary_scan_failures), i64(g_arbitrary_compare_disagreements),
       i64(g_arbitrary_new_calls), i64(g_arbitrary_interpolation_calls),
       i64(g_arbitrary_interpolation_failures), i64(g_invalid_arbitrary_operations), 0, 0},
      g_last_arbitrary_interpolation_amount, world_lifetimes_balanced(),
      {i64(world_stats.created), i64(world_stats.disposed)}, async_receipt_lifetimes_balanced(),
      {i64(receipt_stats.created), i64(receipt_stats.checked_in), i64(receipt_stats.live_count),
       i64(receipt_stats.live_bytes), i64(receipt_stats.invalid_operations)},
      async_layer_requests_balanced(),
      {i64(aexcompat::aegp_async_layer::diagnostics().created),
       i64(aexcompat::aegp_async_layer::diagnostics().completed),
       i64(aexcompat::aegp_async_layer::diagnostics().canceled),
       i64(aexcompat::aegp_async_layer::diagnostics().callback_failures),
       i64(aexcompat::aegp_async_layer::diagnostics().callback_exceptions),
       i64(aexcompat::aegp_async_layer::diagnostics().live),
       i64(aexcompat::aegp_async_layer::diagnostics().reserved_bytes)}};
}

int selftest_render_output_safety(int, wchar_t**) {
  const bool passed = verify_render_output_safety();
  std::cout << "{\"render_output_safety\":\"" << (passed ? "passed" : "failed")
            << "\",\"cleanup_selector\":\"" << escape(g_last_seh_selector)
            << "\",\"cleanup_error\":" << g_last_seh_error
            << ",\"cleanup_calls\":" << g_cleanup_safety_selftest_calls
            << ",\"guard_pages\":true,\"overrun_beyond_64_detected\":true}\n";
  return passed ? 0 : 1;
}

int selftest_crash_minidump(int, wchar_t** argv) {
  if (!aexcompat::worker_runtime::minidump::configure_directory(
          std::filesystem::path(argv[2]))) {
    std::cout << "{\"crash_minidump\":\"failed\",\"reason\":\"bad_directory\"}\n";
    return 1;
  }
  const uint32_t exception_code = selftest_trigger_guarded_crash();
  const std::filesystem::path dump_path =
      aexcompat::worker_runtime::minidump::current_process_dump_path();
  std::error_code dump_size_error;
  const auto dump_size = std::filesystem::file_size(dump_path, dump_size_error);
  const bool written = !dump_size_error && dump_size > 0;
  std::cout << "{\"crash_minidump\":\"" << (written ? "passed" : "failed")
            << "\",\"exception_code\":" << exception_code
            << ",\"dump_bytes\":" << (written ? dump_size : 0)
            << ",\"attempted\":"
            << (aexcompat::worker_runtime::minidump::attempted() ? "true" : "false")
            << "}\n";
  return written ? 0 : 1;
}

int selftest_pf_adv_time(int, wchar_t**) {
  const bool passed = aexcompat::worker_runtime::pf_adv_time::verify_suite_versions();
  std::cout << "{\"pf_adv_time_suite_versions\":\"" << (passed ? "passed" : "failed")
            << "\",\"v1_slots\":4,\"v2_slots\":4,\"v3_slots\":4,\"v4_slots\":5,\"independent_identity\":true"
            << ",\"guard_intact\":true,\"reverse_release\":true,\"suite_leases_balanced\":"
            << (suite_leases_balanced() ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}

int selftest_suite_entry_utility13(int, wchar_t**) {
  const bool passed = verify_suite_entry_guards_and_utility13();
  std::cout << "{\"suite_entry_utility13\":\"" << (passed ? "passed" : "failed")
            << "\",\"null_fail_closed\":true,\"normal_effect_available\":true"
            << ",\"versions_12_14_rejected\":true,\"mask_callbacks_exposed\":false"
            << ",\"suite_leases_balanced\":"
            << (suite_leases_balanced() ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}

int selftest_pf_adv_app(int, wchar_t**) {
  const bool passed = verify_pf_adv_app_suite_versions();
  std::cout << "{\"pf_adv_app_suite_versions\":\"" << (passed ? "passed" : "failed")
            << "\",\"v1_slots\":10,\"v2_slots\":11,\"independent_identity\":true"
            << ",\"suite_leases_balanced\":"
            << (suite_leases_balanced() ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}

int selftest_effect_param_union(int, wchar_t**) {
  const bool passed = verify_aegp_effect_param_union_suite4();
  std::cout << "{\"aegp_effect_param_union_suite4\":\""
            << (passed ? "passed" : "failed")
            << "\",\"successful_calls\":" << g_aegp_effect_param_union_calls << "}\n";
  return passed ? 0 : 1;
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
       &bounded_argb8_world,
       reinterpret_cast<void*>(&aegp_unsupported_suite_call)},
      {&g_transform_world_calls, &g_last_transform_x, &g_last_transform_y,
       &g_last_transform_opacity}};
  bootstrap_hooks.adv_time = {&acquire_suite, &release_suite, &suite_acquire_count,
                              &suite_release_count, &suite_leases_balanced};
  bootstrap_hooks.hash = &sha256;
  bootstrap_hooks.audit_capture = &capture_module_audit_phase;
  bootstrap_hooks.audit_passed = &module_audit_passed;
  bootstrap_hooks.trace = &record_selector_dispatch;
  const auto bootstrap_error =
      aexcompat::worker_runtime::entry_bootstrap::configure(bootstrap_hooks);
  if (bootstrap_error != 0) return bootstrap_error;
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
  const std::array<aexcompat::worker_runtime::selftest::HostCommand, 6> host_selftests{{
      {L"--self-test-render-output-safety", 2, &selftest_render_output_safety},
      {L"--self-test-crash-minidump", 3, &selftest_crash_minidump},
      {L"--self-test-pf-adv-time-suite1", 2, &selftest_pf_adv_time},
      {L"--self-test-suite-entry-utility13", 2, &selftest_suite_entry_utility13},
      {L"--self-test-pf-adv-app-suite", 2, &selftest_pf_adv_app},
      {L"--self-test-aegp-effect-param-union-suite4", 2, &selftest_effect_param_union},
  }};
  if (const auto selftest_exit = aexcompat::worker_runtime::selftest::dispatch_host(
          argc, argv, host_selftests.data(), host_selftests.size()))
    return *selftest_exit;
  const std::array<aexcompat::worker_runtime::selftest::SimpleCommand, 22> simple_selftests{{
      {L"--self-test-aegp-installed-effect-catalog", "aegp_installed_effect_catalog", &verify_aegp_installed_effect_catalog_suite4},
      {L"--self-test-parameter-animation", "parameter_animation_transport", &verify_parameter_animation_transport},
      {L"--self-test-pf-param-utils-suite", "pf_param_utils_suite3", &verify_pf_param_utils_suite3},
      {L"--self-test-pf-pre-checkout-result", "pf_pre_checkout_result", &verify_pre_checkout_result_contract},
      {L"--self-test-pf-checkout-intersection", "pf_checkout_intersection", &aexcompat::worker_runtime::smart::checkout_intersection_self_test},
      {L"--self-test-pf-smart-geometry-rects", "pf_smart_geometry_rects", &aexcompat::render::smart_geometry_rect_self_test},
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
    const bool passed = verify_aegp_keyframe_suite5_mutations(
        keyframe_suite5_abi_wiring_valid());
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
    const bool passed = verify_pf_path_data_hardening(
        {&g_effect, &enumerate_pf_paths, &snapshot_pf_path, &bounded_pf_path_world},
        {&g_layer, &raise_mask_access_violation, &mask_runtime_snapshot,
         &snapshot_mask_curve, &mask_lifetimes_balanced, &install_synthetic_mask_scene});
    const auto path_report = aexcompat::pf_path_runtime::snapshot();
    std::cout << "{\"pf_path_data_hardening\":\"" << (passed ? "passed" : "failed")
              << "\",\"created\":" << path_report.preps_created
              << ",\"disposed\":" << path_report.preps_disposed
              << ",\"live\":" << path_report.live_preps
              << ",\"balanced\":" << (aexcompat::pf_path_runtime::lifetimes_balanced() ? "true" : "false")
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
    const auto channel_transport = transport_statistics();
    std::cout << "{\"pf_ae_channel_transport\":\""
              << (passed ? "passed" : "failed")
              << "\",\"row_bytes\":" << channel_transport.row_bytes
              << ",\"origin\":[" << channel_transport.origin_x << ','
              << channel_transport.origin_y << "]"
              << ",\"duration\":" << channel_transport.duration << "}\n";
    return passed ? 0 : 3;
  }
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pf-color-settings-suite6") {
    const bool passed =
        aexcompat::color_settings::selftests::verify_pf_color_settings_suite6();
    const auto& srgb_icc = color_settings_builtin_srgb_icc();
    const auto& linear_icc = color_settings_builtin_linear_icc();
    const auto color_stats = color_settings_statistics();
    uint32_t memory_created = 0;
    uint32_t memory_freed = 0;
    uint64_t memory_residual = 0;
    const auto memory_stats = aegp_memory_statistics();
    memory_created = memory_stats.created;
    memory_freed = memory_stats.freed;
    memory_residual = memory_stats.live_bytes;
    std::cout << "{\"pf_color_settings_suite6\":\"" << (passed ? "passed" : "failed")
              << "\",\"profiles_created\":" << color_stats.profiles_created
              << ",\"profiles_disposed\":" << color_stats.profiles_disposed
              << ",\"profiles_live\":" << color_stats.profiles_live
              << ",\"invalid_operations\":" << color_stats.invalid_operations
              << ",\"xform_calls\":" << color_stats.xform_calls
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
    const auto staged = aexcompat::aegp_staged_item_runtime::diagnostics();
    std::cout << "{\"aegp_item_staged_worlds\":\""
              << (passed ? "passed" : "failed")
              << "\",\"immutable_stage\":true,\"reentrant_render_used\":false"
              << ",\"published\":" << staged.published
              << ",\"cache_hits\":" << staged.cache_hits
              << ",\"cache_misses\":" << staged.cache_misses
              << ",\"cycles_rejected\":" << staged.cycles_rejected
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
aexcompat::worker_runtime::invocation::InvocationState invocation;

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
  const aexcompat::worker_runtime::invocation::ApplyHooks invocation_hooks{
      +[](int32_t x, int32_t y, const std::array<float, 4>& color) {
        g_render_click_x = x; g_render_click_y = y;
        g_app_picker_color = color; g_render_click_enabled = true;
      },
      +[] { g_render_draw_enabled = true; },
      +[](bool enabled) { g_mask_model_enabled = enabled; },
      +[](bool count_error, bool count_crash) {
        aexcompat::mask_runtime::set_fault(count_error
            ? aexcompat::mask_runtime::Fault::CountError
            : count_crash ? aexcompat::mask_runtime::Fault::CountCrash
                          : aexcompat::mask_runtime::Fault::None);
      },
      +[](std::vector<float>* audio, int32_t samples) {
        aexcompat::host_audio::runtime().set_source(audio, samples);
      }};

  if (is_render_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
      aexcompat::worker_runtime::request_parser::Kind::Render, argc, argv,
      {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
        load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation},
       &parse_layer_transport_key, &parse_mask_context_payload,
       &parse_spatial_context_payload, &parse_render_environment_payload,
       &requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    aexcompat::worker_runtime::invocation::apply_render(
        parsed.invocation, invocation, invocation_hooks);
  } else if (is_smart_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
        aexcompat::worker_runtime::request_parser::Kind::Smart, argc, argv,
        {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
          load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation},
         &parse_layer_transport_key, &parse_mask_context_payload,
         &parse_spatial_context_payload, &parse_render_environment_payload,
         &requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    aexcompat::worker_runtime::invocation::apply_smart(
        parsed.invocation, invocation, invocation_hooks);
  } else {
    const auto mode_error = aexcompat::worker_runtime::invocation::parse_l2_modes(
        argc, argv, invocation,
        {parse_parameter_payload, kMaxParams});
    if (mode_error != 0) return mode_error;
    g_aegp_update_menu_mode = invocation.aegp_update_menu_mode;
    g_aegp_idle_mode = invocation.aegp_idle_mode;
    g_aegp_command_roundtrip_mode = invocation.aegp_command_roundtrip_mode;
    g_aegp_active_idle_roundtrip_mode = invocation.aegp_active_idle_roundtrip_mode;
    g_aegp_keyframe_roundtrip_mode = invocation.aegp_keyframe_roundtrip_mode;
    g_aegp_seek_roundtrip_mode = invocation.aegp_seek_roundtrip_mode;
    g_aegp_trim_roundtrip_mode = invocation.aegp_trim_roundtrip_mode;
    g_aegp_switch_roundtrip_mode = invocation.aegp_switch_roundtrip_mode;
    g_aegp_comp_idle_roundtrip_mode = invocation.aegp_comp_idle_roundtrip_mode;
    g_aegp_init_mode = invocation.aegp_init_mode;
    g_skip_about = invocation.skip_about_mode;
    g_app_picker_color = invocation.picker_color;
    g_user_changed_param_slot = invocation.user_changed_param_slot;
    g_user_changed_param_requested = invocation.user_changed_param_requested;
    g_user_changed_parameters = std::move(invocation.user_changed_parameters);
  }
  // Every rendered effect instance belongs to a layer, even when that layer has no masks.
  if (is_rendering_worker()) g_mask_model_enabled = true;
  RuntimeHostHooks runtime_hooks{&sha256, &redirect_native_stdout,
                                 &restore_native_stdout};
  RuntimeAdmissionRequest runtime_request;
  const int request_error = aexcompat::worker_runtime::prepare_runtime_request(
      argv[2], argv[3], !is_rendering_worker() && runtime_module_authorization_mode,
      runtime_module_authorization_mode ? argv[5] : nullptr, runtime_request);
  if (request_error != 0) return request_error;
  std::unique_ptr<aexcompat::TraceWriter> trace_writer;
  RuntimeContext runtime_context;
  const int admission_error = aexcompat::worker_runtime::admit_worker_entry(
      runtime_hooks, runtime_request, trace_worker_label(), trace_writer,
      runtime_context);
  if (admission_error != 0) return admission_error;
  WorkerSession session(runtime_context, trace_writer.get(), &g_trace_writer);
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
    if (init_error == 0) {
      const bool command_ready = !g_aegp_inserted_commands.empty() &&
          !g_aegp_command_registrations.empty();
      if (g_aegp_command_roundtrip_mode && !command_ready) {
        event_error = 4;
      } else {
        const int32_t command = command_ready ? g_aegp_inserted_commands.front() : 0;
        const auto events = aexcompat::worker_runtime::aegp_init::dispatch_basic_events(
            global_refcon, g_aegp_update_menu_mode, g_aegp_idle_mode,
            g_aegp_command_roundtrip_mode, command);
        hooks_invoked += events.hooks_invoked;
        menu_hooks_invoked += events.menu_hooks_invoked;
        command_hooks_invoked += events.command_hooks_invoked;
        command_handled_count += events.command_handled_count;
        idle_max_sleep = events.idle_max_sleep;
        if (events.error != 0 && event_error == 0) event_error = events.error;
      }
    }
    aexcompat::worker_runtime::aegp_init::RoundtripResult roundtrip{};
    if (init_error == 0) {
      roundtrip = aexcompat::worker_runtime::aegp_init::run_roundtrips(
          {global_refcon, &g_aegp_inserted_commands, &g_aegp_scene_frame,
           &keyframe_probe, &seek_probe, &trim_probe, &switch_probe,
           {g_aegp_active_idle_roundtrip_mode, g_aegp_comp_idle_roundtrip_mode,
            g_aegp_keyframe_roundtrip_mode, g_aegp_seek_roundtrip_mode,
            g_aegp_trim_roundtrip_mode, g_aegp_switch_roundtrip_mode}},
          {nullptr,
           +[](void*) { return g_aegp_keyframe_time_calls == 2 &&
               g_aegp_keyframe_value_calls == 2 &&
               g_aegp_keyframe_interpolation_calls == 2; },
           +[](void*) { return g_aegp_item_set_current_time_calls == 1 &&
               g_aegp_item_last_set_time_value == 75 &&
               g_aegp_item_last_set_time_scale == 30 && g_aegp_scene_frame == 75; },
           +[](void*) {
             const auto& in_point = g_aegp_layer_in_points[0];
             const auto& duration = g_aegp_layer_durations[0];
             return g_aegp_layer_trim_set_calls == 1 && in_point.value == 30 &&
                 in_point.scale == 30 && duration.value == 210 && duration.scale == 30;
           },
           +[](void*) { return g_aegp_layer_flag_set_calls == 4 &&
               g_aegp_layer_flags[0] == 0x00004026u &&
               g_aegp_layer_flags[1] == 0x00000005u &&
               g_aegp_layer_flags[2] == 0x00000005u; }});
    }
    if (init_error == 0) {
      if (roundtrip.error != 0 && event_error == 0) event_error = roundtrip.error;
      hooks_invoked += roundtrip.hooks_invoked;
      menu_hooks_invoked += roundtrip.menu_hooks_invoked;
      command_hooks_invoked += roundtrip.command_hooks_invoked;
      command_handled_count += roundtrip.command_handled_count;
      if (roundtrip.idle_max_sleep >= 0 &&
          (idle_max_sleep < 0 || roundtrip.idle_max_sleep < idle_max_sleep))
        idle_max_sleep = roundtrip.idle_max_sleep;
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

  aexcompat::worker_runtime::effect_bootstrap::State effect_state{};
  auto& input = effect_state.input;
  auto& output = effect_state.output;
  const auto bootstrap = aexcompat::worker_runtime::effect_bootstrap::run(
      effect_state, entry,
      {{reinterpret_cast<void*>(&checkout_param), reinterpret_cast<void*>(&checkin_param),
        reinterpret_cast<void*>(&add_param), reinterpret_cast<void*>(&abort_render),
        reinterpret_cast<void*>(&report_progress), reinterpret_cast<void*>(&register_custom_ui),
        reinterpret_cast<void*>(&checkout_layer_audio), reinterpret_cast<void*>(&checkin_layer_audio),
        reinterpret_cast<void*>(&get_audio_data)},
       {reinterpret_cast<void*>(&begin_sampling8), reinterpret_cast<void*>(&subpixel_sample8),
        reinterpret_cast<void*>(&area_sample8), reinterpret_cast<void*>(&end_sampling8),
        reinterpret_cast<void*>(&blend_world), reinterpret_cast<void*>(&convolve_world),
        reinterpret_cast<void*>(&copy_world8), reinterpret_cast<void*>(&fill_world8),
        reinterpret_cast<void*>(&premultiply_world8), reinterpret_cast<void*>(&premultiply_color8),
        reinterpret_cast<void*>(&fill_world16), reinterpret_cast<void*>(&premultiply_color16),
        reinterpret_cast<void*>(&iterate_world8), reinterpret_cast<void*>(&legacy_new_world),
        reinterpret_cast<void*>(&dispose_world), reinterpret_cast<void*>(&transform_world),
        reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_ceil),
        reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_fabs),
        reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_pow),
        reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sin),
        reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sprintf),
        reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy),
        reinterpret_cast<void*>(&get_platform_data), reinterpret_cast<void*>(&get_pixel_data8),
        reinterpret_cast<void*>(&get_pixel_data16)}, &g_color_suite8, sizeof(g_color_suite8),
       &g_basic_suite, &g_effect},
      {g_render_quality, g_render_field, g_shutter_angle, g_shutter_phase,
       {g_pre_effect_source_origin_x, g_pre_effect_source_origin_y},
       {static_cast<int32_t>(g_downsample_x.numerator),
        static_cast<int32_t>(g_downsample_x.denominator)},
       {static_cast<int32_t>(g_downsample_y.numerator),
        static_cast<int32_t>(g_downsample_y.denominator)},
       {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
        static_cast<int32_t>(g_pixel_aspect_ratio.denominator)},
       external_pixel_bytes, static_cast<int32_t>(g_params.size() + 1),
       is_render_worker(), is_rendering_worker(), audio_mode, g_skip_about},
      {&invoke_entry_seh, &reset_effect_lifetime,
       +[](bool active) { g_global_setup_active = active; },
       +[](bool requested, bool advertised) {
         aexcompat::host_audio::runtime().configure_admission(requested, advertised);
       },
       +[](EffectEntry callback,
           aexcompat::worker_runtime::effect_bootstrap::State& state) {
         observe_arbitrary_defaults(callback, state.input, state.output);
       }});
  const int32_t global_error = bootstrap.global_error;
  const int32_t about_error = bootstrap.about_error;
  const int32_t params_error = bootstrap.params_error;
  const uint32_t advertised_out_flags = bootstrap.advertised_out_flags;
  const uint32_t advertised_out_flags2 = bootstrap.advertised_out_flags2;
  const bool image_render_supported = bootstrap.image_render_supported;
  const bool nop_render_advertised = bootstrap.nop_render_advertised;
  const bool input_write_advertised = bootstrap.input_write_advertised;
  const bool expand_buffer_advertised = bootstrap.expand_buffer_advertised;
  const bool shrink_buffer_advertised = bootstrap.shrink_buffer_advertised;
  const bool depth_supported = bootstrap.depth_supported;
  const bool smart_render_supported = bootstrap.smart_render_supported;
  const bool parameter_count_contract_valid = bootstrap.parameter_count_contract_valid;
  g_update_params_ui_advertised = bootstrap.update_params_ui_advertised;
  g_query_dynamic_flags_advertised = bootstrap.query_dynamic_flags_advertised;
  std::string about_message = bootstrap.about_message;

  // Source-level compatibility anchors retained while the ABI bootstrap is
  // owned by worker_effect_bootstrap.cpp. They document the exact legacy
  // offsets/ordering that the owner preserves and keep static contract tests
  // useful during the staged extraction.
  // write<int32_t>(input, 284, g_downsample_x.numerator)
  // write(utils, kUtilsNewWorld, &legacy_new_world)
  // write(utils, kUtilsCopy, &copy_world8)
  // write(utils, kUtilsGetPlatformData, &get_platform_data)
  // write<int32_t>(input, kInQuality, g_render_quality)
  // write<int16_t>(input, kInVersion, kHostSpecMajor)
  // write<int16_t>(input, kInVersion + sizeof(int16_t), kHostSpecMinor)
  // expected_num_params = static_cast<int32_t>(g_params.size() + 1)
  // read<int32_t>(output, kOutNumParams) == expected_num_params
  // write<int32_t>(input, kInNumParams, expected_num_params)
  // write(utils, kUtilsIterate, &iterate_world8)
  // write(utils, kUtilsAnsiPow, &aexcompat::pf_ansi::ansi_pow)
  // write(utils, kUtilsAnsiStrcpy, &aexcompat::pf_ansi::ansi_strcpy)
  // memcpy(utils.data() + kUtilsColorCallbacks, &g_color_suite8,
  //        sizeof(g_color_suite8))
  // write(utils, kUtilsBeginSampling, &begin_sampling8)
  // write(utils, kUtilsAreaSample, &area_sample8)
  // write(utils, kUtilsEndSampling, &end_sampling8)
  // write(input, 24, &abort_render)
  // write(input, 32, &report_progress)
  // external_pixel_bytes == 8 &&
  // external_pixel_bytes == 16 &&
  // params_error == 0 && image_render_supported && depth_supported
  // (1u << 26)
  // write<int32_t>(input, 248, g_shutter_angle)
  // write<int32_t>(input, 400, g_shutter_phase)
  // about_error = g_skip_about ? 0
  // invoke_entry_seh(entry, kGlobalSetup)
  // invoke_entry_seh(entry, kAbout)
  // invoke_entry_seh(entry, kParamsSetup)
  // invoke_entry_seh(entry, kGlobalSetdown)
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
    g_ui_context.window_type = std::strcmp(event_target, "layer") == 0 ? 1 :
        (std::strcmp(event_target, "comp") == 0 ? 0 : 2);
    aexcompat::worker_runtime::ui_event_execution::Result ui_result;
    const bool ui_dispatched = aexcompat::worker_runtime::ui_event_execution::dispatch(
        {entry, &input, &output, params_error, parameter_count_contract_valid,
         &ui_event_assignments, ui_event_assignment_mode, adjust_cursor_mode,
         draw_event_mode, click_event_mode, drag_event_mode, ui_lifecycle_mode,
         ui_idle_mode, ui_keydown_mode, ui_mouse_exited_mode, click_x, click_y,
         drag_end_x, drag_end_y, drag_steps, keydown_code, keydown_modifiers,
         g_ui_context.window_type, &g_ui_context_pointer, &g_ui_context,
         g_ui_context.plugin_state.data(), reinterpret_cast<void*>(&ui_transform_point),
         reinterpret_cast<void*>(&ui_transform_point_simple)},
        {&invoke_entry_seh, &enter_custom_ui_context, &leave_custom_ui_context,
         &custom_ui_context_stable, &set_custom_ui_context_tool},
        ui_result);
    event_error = ui_dispatched ? ui_result.event_error : -1;
    cursor = ui_result.cursor;
    event_out_flags = ui_result.event_out_flags;
    changed_value = ui_result.changed_value;
    lifecycle_errors = ui_result.lifecycle_errors;
    plugin_state_before_close = ui_result.plugin_state_before_close;
    lifecycle_context_stable = ui_result.lifecycle_context_stable;
    lifecycle_host_state_cleared = ui_result.lifecycle_host_state_cleared;
    event_assignments_applied = ui_result.event_assignments_applied;
    g_ui_drag_requested = ui_result.drag_requested;
    g_ui_drag_calls = ui_result.drag_calls;
    g_ui_drag_terminated = ui_result.drag_terminated;
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
  const auto early_mode = aexcompat::l2mode::select_early_mode(
      auto_dialog_mode, do_dialog_mode, external_dependencies_mode, params_only_mode);
  if (!is_rendering_worker() && early_mode != aexcompat::l2mode::EarlyMode::None) {
    EarlyModeBridge bridge{entry, &input, &output, &session, &about_message};
    const int early_result = aexcompat::l2mode::run_early_mode(
        {early_mode, &bridge, early_mode_hooks(), global_error, params_error,
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
                aexcompat::pf_path_runtime::lifetimes_balanced() &&
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
  const auto& audio_report = audio_telemetry();
  classic_report.audio = {
      audio_report.usage_advertised, audio_report.checkout_allowed, audio_report.source_available,
      audio_report.rejected_unadvertised_checkouts, audio_report.rejected_format_requests,
      audio_report.handle_exhaustions, audio_report.peak_live_handles, audio_report.checkout_calls,
      audio_report.checkin_calls, audio_report.get_data_calls, audio_report.invalid_operations,
      audio_report.last_checkout_start_time, audio_report.last_checkout_duration,
      audio_report.last_checkout_time_scale, audio_report.last_window_start_sample,
      audio_report.last_window_sample_count, audio_report.last_window_silence_samples,
      audio_report.last_output_rate, audio_report.last_output_bytes_per_sample,
      audio_report.last_output_channels, audio_report.last_output_format,
      audio_report.last_returned_sample_frames, audio_handle_lifetimes_balanced()};
  classic_report.sequence = {
      persistent_sequence, persistent_sequence_setup_error, persistent_sequence_setdown_error,
      persistent_frame_errors, persistent_frame_hashes, flattened_sequence,
      sequence_flatten_error, sequence_resetup_error, flattened_handle_replaced,
      resetup_handle_replaced, flattened_handle_host_disposed, copied_flattened_sequence,
      get_flattened_sequence_data_error, original_sequence_preserved};
  const int32_t bytes_per_pixel = smart_state().pixel_format == "argb32f" ? 16 :
      (smart_state().pixel_format == "argb16" ? 8 : 4);
  classic_report.frame = {
      setdown_error,
      escape(std::string(reinterpret_cast<const char*>(output.data() + kOutMessage),
                         strnlen_s(reinterpret_cast<const char*>(output.data() + kOutMessage), 256))),
      case_id, smart_state().pixel_format, render_width, render_height, render_rowbytes,
      render_width * bytes_per_pixel,
      std::max(0, render_rowbytes - render_width * bytes_per_pixel),
      input_hash, output_hash, guards_intact, world_debug_report_json()};
  const aexcompat::worker_render_report::CustomUiSnapshot classic_custom_ui{
      g_render_click_enabled, g_render_click_error, g_render_click_out_flags,
      g_render_click_changed_value, g_render_draw_enabled, g_render_draw_error,
      g_render_draw_out_flags, g_render_ui_lifecycle_errors, g_render_ui_context_closed,
      g_app_color_picker_calls, g_app_invalidate_rect_calls, g_app_picker_color};
  const auto i64 = [](auto value) { return static_cast<int64_t>(value); };
  classic_report.callbacks = {
      classic_diagnostics.balanced,
      {i64(classic_diagnostics.checkout_calls), i64(classic_diagnostics.checkin_calls),
       i64(classic_diagnostics.automatic_checkins), i64(classic_diagnostics.invalid_checkins),
       i64(classic_diagnostics.last_index), i64(classic_diagnostics.last_time),
       i64(classic_diagnostics.last_time_step), i64(classic_diagnostics.last_time_scale)},
      escape(g_options_button_name),
      {i64(g_options_button_name_calls), i64(channel_count_queries()),
       i64(g_transform_world_calls), i64(g_last_transform_x), i64(g_last_transform_y),
       i64(g_last_transform_opacity), i64(g_abort_calls), i64(g_progress_calls),
       i64(g_register_ui_calls), i64(g_last_progress_current), i64(g_last_progress_total)}};
  classic_report.threads = {concurrent_render, thread_errors, thread_hashes, thread_guards};
  classic_report.context = {
      request_mode,
      {static_cast<int32_t>(g_downsample_x.numerator), static_cast<int32_t>(g_downsample_x.denominator)},
      {static_cast<int32_t>(g_downsample_y.numerator), static_cast<int32_t>(g_downsample_y.denominator)},
      {static_cast<int32_t>(g_pixel_aspect_ratio.numerator), static_cast<int32_t>(g_pixel_aspect_ratio.denominator)},
      {g_full_resolution_width > 0 ? g_full_resolution_width : external_width,
       g_full_resolution_height > 0 ? g_full_resolution_height : external_height},
      {read<int32_t>(input, kInQuality), read<int32_t>(input, kInNumParams),
       read<int32_t>(input, kInLocalTimeStep), read<int32_t>(input, 244),
       read<int32_t>(input, 248), read<int32_t>(input, 400)},
      {read<int32_t>(input, 252), read<int32_t>(input, 256)},
      {read<int32_t>(input, 392), read<int32_t>(input, 396)},
      {read<int32_t>(input, 276), read<int32_t>(input, 280)}};
  const aexcompat::worker_render_report::RequestedParametersSnapshot classic_requested{
      requested_parameters_json(requested_parameters),
      static_cast<int32_t>(requested_value(requested_parameters, L"amount")),
      static_cast<int32_t>(requested_value(requested_parameters, L"direction")),
      static_cast<int32_t>(requested_value(requested_parameters, L"seed")),
      requested_value(requested_parameters, L"mix"),
      static_cast<int32_t>(requested_value(requested_parameters, L"invert_map")),
      !nop_render_advertised, module_audit_json()};
  aexcompat::worker_render_report::emit_classic_complete(report_snapshot, {
      classic_report, classic_custom_ui, capture_classic_subsystems(),
      capture_gpu_diagnostics(), capture_seh_diagnostics(), classic_requested,
      aexcompat::worker_runtime::classic::last_selector_dispatched(),
      depth_supported, render_error});
  aexcompat::worker_render_report::emit(report_snapshot, std::cout);
  } else if (is_smart_worker()) {
  restore_native_stdout();
  const auto mask_report = aexcompat::mask_runtime::snapshot();
  aexcompat::worker_render_report::ReportSnapshot report_snapshot(std::cout);
  const bool smart_completed = smart.pre_error == 0 && smart.render_error == 0 &&
                parameter_count_contract_valid && smart.rects_valid &&
                arbitrary_defaults_disposed && g_invalid_arbitrary_operations == 0 &&
                smart.gpu_setup_error == 0 && smart.gpu_setdown_error == 0 &&
                smart.guards_intact && handle_lifetimes_balanced() &&
                world_lifetimes_balanced() && gpu_memory_lifetimes_balanced() &&
                audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
                param_checkouts_balanced() &&
                ((!g_render_click_enabled && !g_render_draw_enabled) ||
                 g_render_ui_context_closed);
  aexcompat::worker_render_report::begin_smart(report_snapshot, {
      smart_completed,
      {global_error, params_error, advertised_out_flags, advertised_out_flags2},
      {image_render_supported, smart_render_supported, nop_render_advertised, input_write_advertised},
      {smart.runtime->wide_time_checkout_allowed, smart.runtime->shutter_dependency_advertised,
       !nop_render_advertised, smart.selector_dispatched, false},
      smart.runtime->rejected_temporal_checkouts,
      {g_comp_bg_color_successes.load(std::memory_order_relaxed),
       g_comp_bg_color_rejections.load(std::memory_order_relaxed),
       g_guid_mix_in_calls.load(std::memory_order_relaxed),
       g_guid_mix_in_successes.load(std::memory_order_relaxed),
       g_guid_mix_in_rejections.load(std::memory_order_relaxed),
       g_guid_mix_in_last_size.load(std::memory_order_relaxed),
       g_guid_mix_in_max_size.load(std::memory_order_relaxed), kMaxGuidMixInBytes,
       g_guid_mix_in_last_result.load(std::memory_order_relaxed)},
      depth_supported,
      {smart.pre_error, smart.render_error, smart.selector_error, smart.gpu_setup_error,
       smart.gpu_setdown_error, smart.gpu_setdown_exception_code},
      {smart.gpu_render_possible, smart.gpu_render_dispatched},
      {smart.checkout_time, smart.checkout_time_step, smart.checkout_time_scale},
      smart.roi_contract_valid, smart.runtime->input_checkout_request,
      smart.runtime->map_checkout_request, smart.input_checkout_result_rect,
      smart.map_checkout_result_rect, smart.malformed_checkout_requests,
      smart.empty_checkout_pixel_denials, smart.returns_extra_pixels,
      smart.result_within_request, smart.extra_pixels_contract_violation,
      smart.empty_result_rect, smart.output_extent_hint,
      setdown_error, case_id, smart.runtime->pixel_format,
      {smart.output_width, smart.output_height, smart.output_rowbytes},
      {external_width, external_height},
      smart.runtime->pixel_format == "argb32f" ? 16 :
          (smart.runtime->pixel_format == "argb16" ? 8 : 4),
      smart.input_hash,
      smart.output_hash, smart.rects_valid, world_debug_report_json()});
  aexcompat::worker_render_report::append_custom_ui(report_snapshot, {
      g_render_click_enabled, g_render_click_error, g_render_click_out_flags,
      g_render_click_changed_value, g_render_draw_enabled, g_render_draw_error,
      g_render_draw_out_flags, g_render_ui_lifecycle_errors, g_render_ui_context_closed,
      g_app_color_picker_calls, g_app_invalidate_rect_calls, g_app_picker_color});
  aexcompat::worker_render_report::append_smart_context(report_snapshot, {
      smart.result_rect, smart.max_result_rect,
      {smart.guards_intact, smart.output_pixels_valid, param_checkouts_balanced()},
      {g_param_checkout_calls, g_param_checkin_calls, g_automatic_param_checkins,
       g_invalid_param_checkins}, request_mode,
      {static_cast<int32_t>(g_downsample_x.numerator), static_cast<int32_t>(g_downsample_x.denominator)},
      {static_cast<int32_t>(g_downsample_y.numerator), static_cast<int32_t>(g_downsample_y.denominator)},
      {static_cast<int32_t>(g_pixel_aspect_ratio.numerator), static_cast<int32_t>(g_pixel_aspect_ratio.denominator)},
      {g_full_resolution_width > 0 ? g_full_resolution_width : external_width,
       g_full_resolution_height > 0 ? g_full_resolution_height : external_height},
      {read<int32_t>(input, kInQuality), read<int32_t>(input, kInNumParams),
       read<int32_t>(input, kInLocalTimeStep), read<int32_t>(input, 244),
       read<int32_t>(input, 248), read<int32_t>(input, 400)},
      {read<int32_t>(input, 252), read<int32_t>(input, 256)},
      {read<int32_t>(input, 392), read<int32_t>(input, 396)},
      {read<int32_t>(input, 276), read<int32_t>(input, 280)}});
  const auto handle_stats = statistics();
  const auto world_stats = aexcompat::world_registry::statistics();
  aexcompat::worker_render_report::append_smart_lifetimes(report_snapshot, {
      g_mask_scene_id, {static_cast<int64_t>(mask_report.active_masks), static_cast<int64_t>(mask_open_count()), static_cast<int64_t>(mask_tangent_vertex_count())},
      mask_lifetimes_balanced(), {mask_report.masks_acquired, mask_report.masks_disposed,
      mask_report.streams_acquired, mask_report.streams_disposed, mask_report.values_acquired,
      mask_report.values_disposed}, lifetime_fault_observed, suite_leases_balanced(),
      {static_cast<int64_t>(suite_acquire_count()), static_cast<int64_t>(suite_release_count()), static_cast<int64_t>(live_suite_lease_count()),
       static_cast<int64_t>(live_suite_reference_count())}, missing_suites_report_json() + suite_timeline_report_json(), live_suite_lease_summary(),
      suite_fault_observed, handle_lifetimes_balanced(),
      {handle_stats.created, handle_stats.disposed},
      {g_arbitrary_copy_calls, g_arbitrary_dispose_calls, g_arbitrary_print_calls,
       g_arbitrary_print_failures, g_arbitrary_roundtrip_calls, g_arbitrary_roundtrip_failures,
       g_arbitrary_scan_calls, g_arbitrary_scan_failures, g_arbitrary_compare_disagreements,
       g_arbitrary_new_calls, g_arbitrary_interpolation_calls,
       g_arbitrary_interpolation_failures, g_invalid_arbitrary_operations},
      g_last_arbitrary_interpolation_amount,
      {static_cast<int64_t>(handle_stats.automatic_pre_render_disposals), static_cast<int64_t>(handle_stats.locks), static_cast<int64_t>(handle_stats.unlocks),
       static_cast<int64_t>(handle_stats.live_count), static_cast<int64_t>(handle_stats.live_bytes), static_cast<int64_t>(handle_stats.invalid_operations), 0},
      handle_fault_observed, world_fault_observed, world_lifetimes_balanced(),
      {static_cast<int64_t>(world_stats.created), static_cast<int64_t>(world_stats.disposed), static_cast<int64_t>(world_stats.live_count), static_cast<int64_t>(world_stats.live_bytes),
       static_cast<int64_t>(world_stats.invalid_operations)}, gpu_memory_lifetimes_balanced(),
      {static_cast<int64_t>(g_gpu_allocations_created), static_cast<int64_t>(g_gpu_allocations_freed), static_cast<int64_t>(gpu_transport::live_allocation_count()),
       static_cast<int64_t>(gpu_transport::live_memory_bytes()), static_cast<int64_t>(gpu_transport::exclusive_access_depth()),
       static_cast<int64_t>(g_invalid_gpu_memory_operations)}});
  aexcompat::worker_render_report::append_seh_diagnostics(report_snapshot, capture_seh_diagnostics());
  const auto directx_stats = directx_backend::diagnostics();
  const auto aegp_memory_stats = aegp_memory_statistics();
  aexcompat::worker_render_report::append_smart_faults(report_snapshot, {
      {static_cast<int64_t>(g_cuda_upload_bytes), static_cast<int64_t>(g_cuda_download_bytes), static_cast<int64_t>(g_cuda_sync_failures),
       g_last_cuda_device_count, g_last_cuda_device_index},
      {static_cast<int64_t>(g_opencl_upload_bytes), static_cast<int64_t>(g_opencl_download_bytes), static_cast<int64_t>(g_opencl_sync_failures),
       opencl::last_device_count(), opencl::last_device_index()},
      {static_cast<int64_t>(directx_stats.device_count), static_cast<int64_t>(directx_stats.device_index), static_cast<int64_t>(directx_stats.upload_bytes),
       static_cast<int64_t>(directx_stats.download_bytes), static_cast<int64_t>(directx_stats.sync_failures)}, directx_stats.context_used,
      pixel_format_fault_observed,
      {static_cast<int64_t>(g_pixel_format_add_calls), static_cast<int64_t>(g_pixel_format_clear_calls), static_cast<int64_t>(g_supported_pixel_formats.size()),
       g_invalid_pixel_format_operations},
      {outline_fault_observed, mask_attribute_fault_observed, stream_metadata_fault_observed,
       keyframe_fault_observed, dynamic_stream_fault_observed, aegp_memory_fault_observed, false},
      {mask_report.outline_mutations, mask_report.invalid_outline_operations,
       mask_report.mask_mutations, mask_report.invalid_mask_operations,
       g_stream_metadata_queries, g_stream_duplicates, g_invalid_stream_operations,
       mask_report.keyframe_mutations, mask_report.invalid_keyframe_operations,
       g_dynamic_stream_queries, g_dynamic_stream_mutations, g_invalid_dynamic_stream_operations,
       0, 0, 0},
      {static_cast<int64_t>(aegp_memory_stats.created), static_cast<int64_t>(aegp_memory_stats.freed), static_cast<int64_t>(aegp_memory_stats.live_count),
       static_cast<int64_t>(aegp_memory_stats.live_bytes), static_cast<int64_t>(aegp_memory_stats.invalid_operations)}});
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
const bool g_aegp_compat_selftests_configured = [] {
  aexcompat::l2_detail::configure_aegp_compat_selftests(
      {&acquire_suite, &release_suite,
       g_aegp_comp_suite10.data(), &g_pf_interface_suite,
       aexcompat::pf_helper::suite1(), &g_aegp_comp, &g_aegp_comp_item, &g_effect,
       +[](void* comp, aexcompat::l2_detail::AegpCompatColor* color) {
         return aegp_get_comp_bg_color(comp, reinterpret_cast<AegpColorVal*>(color));
       },
       &convert_effect_to_comp_time, &get_effect_camera, &get_effect_camera_matrix,
       +[](int32_t index) { g_aegp_active_camera_layer_index = index; },
       +[] { return g_aegp_active_camera_layer_index; },
       +[](int32_t index) -> void* {
         return index >= 0 && index < static_cast<int32_t>(g_aegp_layers.size())
             ? &g_aegp_layers[static_cast<std::size_t>(index)] : nullptr;
       },
       &aegp_layer_index,
       +[](int32_t width, int32_t height) {
         g_full_resolution_width = width; g_full_resolution_height = height;
       },
       +[](int32_t* width, int32_t* height) {
         if (width) *width = g_full_resolution_width;
         if (height) *height = g_full_resolution_height;
       },
       &suite_leases_balanced, &g_layer, &g_aegp_comp_idle_roundtrip_mode,
       &g_active_ui_param_count, &aegp_get_new_effect_stream_by_index_v2,
       &aegp_get_stream_name_v2, &aegp_get_stream_type_v2,
       &aegp_get_new_stream_value_v2, &aegp_set_stream_value_v2,
       &aegp_dispose_stream_value_v2, &aegp_dispose_stream_v2});
  return true;
}();
const bool g_color_settings_selftests_configured = [] {
  aexcompat::color_settings::selftests::configure(
      {&acquire_suite, &release_suite, &g_aegp_comp});
  return true;
}();
const bool g_parameter_selftests_configured = [] {
  aexcompat::parameter_selftests::configure({
      &acquire_suite, &release_suite, &g_effect, &g_layer,
      &g_param_utils_suite1, &g_param_utils_suite,
      &update_param_ui, &is_identical_param_checkout,
      &find_param_keyframe_time, &get_param_keyframe_count,
      &checkout_param_keyframe, &checkin_param_keyframe,
      &param_key_index_to_time, &get_current_param_state_obsolete,
      &has_param_changed_obsolete,
      &have_inputs_changed_over_time_span_obsolete,
      &apply_parameter_animation});
  return true;
}();
