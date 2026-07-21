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
#include "worker_parameter_selftest_routing.hpp"
#include "worker_pf_color_selftests.hpp"
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
#include "worker_aegp_layer_render_runtime.hpp"
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

// Retained worker-entry identity state (issue #126 Phase D). The worker-kind
// selector is set once by the entry shims before worker_main runs and is part
// of the entry/admission minimum that stays owned here. Lifetime: set at
// process entry, constant afterwards.
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
constexpr std::size_t kUtilsGetHandleSize = 440;
constexpr std::size_t kUtilsResizeHandle = 464;
constexpr std::size_t kUtilsGetPlatformData = 432;
static_assert(kUtilsColorCallbacks + sizeof(PfColorCallbacks8) == kUtilsGetPlatformData);
constexpr std::size_t kUtilsFill16 = 488;
constexpr std::size_t kUtilsPremultiplyColor16 = 496;
constexpr std::size_t kUtilsGetPixelData8 = 528;
constexpr std::size_t kUtilsGetPixelData16 = 536;

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using AegpEntry = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t, void**);

// PiPL-driven Effect entrypoint discovery (issue #84). The loader must decide
// the plug-in ABI from the plug-in's own PiPL resource (Kind + CodeWin64X86),
// not from a fixed export name, so an Effect whose entrypoint is not literally
// "EffectMain" (for example OLM's lowercase entryPointFunc) is discovered and
// an AEGP is never handed to the Effect selector. Bounded and fail-closed:
// malformed, ambiguous, or non-Effect PiPL is rejected before any selector runs.
// Ported from the reviewed/audited implementation on codex/issue84-pipl-entrypoint.
enum class PiplPluginKind { Effect, Aegp, Unknown, Invalid };

struct PiplEntrypoint {
  PiplPluginKind kind{PiplPluginKind::Unknown};
  std::string symbol;
};

uint32_t read_pipl_u32(const unsigned char* bytes) {
  return uint32_t(bytes[0]) | (uint32_t(bytes[1]) << 8) |
      (uint32_t(bytes[2]) << 16) | (uint32_t(bytes[3]) << 24);
}

bool pipl_tag(const unsigned char* bytes, const char (&tag)[5]) {
  return std::memcmp(bytes, tag, 4) == 0;
}

bool valid_export_symbol(const unsigned char* bytes, std::size_t size,
                         std::string& symbol) {
  if (size == 0 || size > 256) return false;
  const void* terminator = std::memchr(bytes, 0, size);
  if (!terminator) return false;
  const std::size_t length = static_cast<const unsigned char*>(terminator) - bytes;
  if (length == 0 || length > 127) return false;
  if (!(bytes[0] == '_' || (bytes[0] >= 'A' && bytes[0] <= 'Z') ||
        (bytes[0] >= 'a' && bytes[0] <= 'z'))) return false;
  for (std::size_t index = 1; index < length; ++index) {
    const unsigned char value = bytes[index];
    if (!(value == '_' || (value >= 'A' && value <= 'Z') ||
          (value >= 'a' && value <= 'z') || (value >= '0' && value <= '9')))
      return false;
  }
  for (std::size_t index = length + 1; index < size; ++index)
    if (bytes[index] != 0) return false;
  symbol.assign(reinterpret_cast<const char*>(bytes), length);
  return true;
}

PiplEntrypoint parse_pipl_entrypoint(const unsigned char* bytes, std::size_t size) {
  PiplEntrypoint result;
  if (!bytes || size < 10 || size > 1024 * 1024) {
    result.kind = PiplPluginKind::Invalid;
    return result;
  }
  // Adobe's Windows PiPL resource serialization has a 10-byte list header:
  // a little-endian 32-bit version followed by a padded 16-bit count tuple
  // (0, count, 0). This is the layout emitted by the SDK PiPL tool/RC files.
  const uint32_t version = read_pipl_u32(bytes);
  const uint32_t count = uint32_t(bytes[6]) | (uint32_t(bytes[7]) << 8);
  if (version > 1 || bytes[4] != 0 || bytes[5] != 0 || bytes[8] != 0 ||
      bytes[9] != 0 || count == 0 || count > 256) {
    result.kind = PiplPluginKind::Invalid;
    return result;
  }
  bool saw_kind = false;
  bool saw_code = false;
  std::array<unsigned char, 4> kind{};
  std::string symbol;
  std::size_t offset = 10;
  for (uint32_t index = 0; index < count; ++index) {
    if (offset > size || size - offset < 16) {
      result.kind = PiplPluginKind::Invalid;
      return result;
    }
    const unsigned char* property = bytes + offset;
    const uint32_t length = read_pipl_u32(property + 12);
    const bool adobe_vendor = pipl_tag(property, "MIB8");
    offset += 16;
    if (length > size - offset || length > std::numeric_limits<uint32_t>::max() - 3U) {
      result.kind = PiplPluginKind::Invalid;
      return result;
    }
    const uint32_t padded_length = (length + 3U) & ~3U;
    if (padded_length > size - offset) {
      result.kind = PiplPluginKind::Invalid;
      return result;
    }
    // PiPL four-character constants are stored as little-endian DWORD bytes
    // in Windows resources (for example PIKindProperty 'kind' is "dnik").
    if (adobe_vendor && pipl_tag(property + 4, "dnik")) {
      if (saw_kind || length != 4) {
        result.kind = PiplPluginKind::Invalid;
        return result;
      }
      std::copy_n(bytes + offset, 4, kind.begin());
      saw_kind = true;
    } else if (adobe_vendor && pipl_tag(property + 4, "4668")) {
      if (saw_code || !valid_export_symbol(bytes + offset, length, symbol)) {
        result.kind = PiplPluginKind::Invalid;
        return result;
      }
      saw_code = true;
    }
    for (uint32_t padding = length; padding < padded_length; ++padding)
      if (bytes[offset + padding] != 0) {
        result.kind = PiplPluginKind::Invalid;
        return result;
      }
    offset += padded_length;
  }
  if (offset != size || !saw_kind) {
    result.kind = PiplPluginKind::Invalid;
    return result;
  }
  if (std::memcmp(kind.data(), "TKFe", 4) == 0) {
    if (!saw_code) result.kind = PiplPluginKind::Invalid;
    else {
      result.kind = PiplPluginKind::Effect;
      result.symbol = std::move(symbol);
    }
  } else if (std::memcmp(kind.data(), "xgEA", 4) == 0) {
    result.kind = PiplPluginKind::Aegp;
  } else {
    result.kind = PiplPluginKind::Unknown;
  }
  return result;
}

struct PiplResourceName {
  bool integer{};
  WORD id{};
  std::wstring text;
};

BOOL CALLBACK collect_pipl_resource(HMODULE, LPCWSTR, LPWSTR name, LONG_PTR context) {
  auto* names = reinterpret_cast<std::vector<PiplResourceName>*>(context);
  if (names->size() >= 64) return FALSE;
  PiplResourceName copy;
  if (IS_INTRESOURCE(name)) {
    copy.integer = true;
    copy.id = LOWORD(reinterpret_cast<ULONG_PTR>(name));
  } else {
    if (!name || std::wcslen(name) > 255) return FALSE;
    copy.text = name;
  }
  names->push_back(std::move(copy));
  return TRUE;
}

BOOL CALLBACK collect_pipl_language(HMODULE, LPCWSTR, LPCWSTR, WORD language,
                                    LONG_PTR context) {
  auto* languages = reinterpret_cast<std::vector<WORD>*>(context);
  if (languages->size() >= 8) return FALSE;
  languages->push_back(language);
  return TRUE;
}

PiplEntrypoint discover_pipl_entrypoint(HMODULE module) {
  std::vector<PiplResourceName> names;
  SetLastError(ERROR_SUCCESS);
  if (!EnumResourceNamesW(module, L"PiPL", &collect_pipl_resource,
                          reinterpret_cast<LONG_PTR>(&names))) {
    const DWORD error = GetLastError();
    if (error != ERROR_RESOURCE_TYPE_NOT_FOUND && error != ERROR_RESOURCE_NAME_NOT_FOUND)
      return {PiplPluginKind::Invalid, {}};
  }
  if (names.empty()) return {PiplPluginKind::Unknown, {}};
  PiplEntrypoint selected;
  bool selected_effect = false;
  bool saw_aegp = false;
  for (const auto& name : names) {
    LPCWSTR resource_name = name.integer ? MAKEINTRESOURCEW(name.id) : name.text.c_str();
    std::vector<WORD> languages;
    SetLastError(ERROR_SUCCESS);
    if (!EnumResourceLanguagesW(module, L"PiPL", resource_name,
                                &collect_pipl_language,
                                reinterpret_cast<LONG_PTR>(&languages)) ||
        languages.size() != 1)
      return {PiplPluginKind::Invalid, {}};
    HRSRC resource = FindResourceExW(module, L"PiPL", resource_name, languages[0]);
    if (!resource) return {PiplPluginKind::Invalid, {}};
    const DWORD size = SizeofResource(module, resource);
    HGLOBAL loaded = LoadResource(module, resource);
    const auto* bytes = loaded ? static_cast<const unsigned char*>(LockResource(loaded)) : nullptr;
    PiplEntrypoint current = parse_pipl_entrypoint(bytes, size);
    if (current.kind == PiplPluginKind::Invalid) return current;
    if (current.kind == PiplPluginKind::Aegp) saw_aegp = true;
    if (current.kind == PiplPluginKind::Effect) {
      if (selected_effect) return {PiplPluginKind::Invalid, {}};
      selected = std::move(current);
      selected_effect = true;
    }
  }
  if (selected_effect && saw_aegp) return {PiplPluginKind::Invalid, {}};
  if (selected_effect) return selected;
  if (saw_aegp) return {PiplPluginKind::Aegp, {}};
  return {PiplPluginKind::Unknown, {}};
}

void append_pipl_u32(std::vector<unsigned char>& bytes, uint32_t value) {
  for (unsigned shift = 0; shift < 32; shift += 8)
    bytes.push_back(static_cast<unsigned char>(value >> shift));
}

void append_pipl_property(std::vector<unsigned char>& bytes, const char (&key)[5],
                          const std::vector<unsigned char>& data) {
  bytes.insert(bytes.end(), {'M', 'I', 'B', '8'});
  bytes.insert(bytes.end(), key, key + 4);
  append_pipl_u32(bytes, 0);
  append_pipl_u32(bytes, static_cast<uint32_t>(data.size()));
  bytes.insert(bytes.end(), data.begin(), data.end());
  while ((bytes.size() - 10) % 4 != 0) bytes.push_back(0);
}

std::vector<unsigned char> synthetic_pipl(
    const std::array<unsigned char, 4>& kind, const std::string& symbol) {
  std::vector<unsigned char> bytes{1, 0, 0, 0, 0, 0, 2, 0, 0, 0};
  append_pipl_property(
      bytes, "dnik", std::vector<unsigned char>(kind.begin(), kind.end()));
  std::vector<unsigned char> code(symbol.begin(), symbol.end());
  code.push_back(0);
  append_pipl_property(bytes, "4668", code);
  return bytes;
}

bool verify_pipl_entrypoint_parser() {
  const auto effect = synthetic_pipl({'T', 'K', 'F', 'e'}, "entryPointFunc");
  const auto parsed_effect = parse_pipl_entrypoint(effect.data(), effect.size());
  if (parsed_effect.kind != PiplPluginKind::Effect ||
      parsed_effect.symbol != "entryPointFunc") return false;
  const auto aegp = synthetic_pipl({'x', 'g', 'E', 'A'}, "EntryPointFunc");
  if (parse_pipl_entrypoint(aegp.data(), aegp.size()).kind != PiplPluginKind::Aegp)
    return false;
  auto invalid_symbol = synthetic_pipl({'T', 'K', 'F', 'e'}, "bad-name");
  if (parse_pipl_entrypoint(invalid_symbol.data(), invalid_symbol.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto truncated = effect;
  truncated.pop_back();
  if (parse_pipl_entrypoint(truncated.data(), truncated.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto hostile_length = effect;
  std::fill(hostile_length.begin() + 22, hostile_length.begin() + 26, 0xff);
  if (parse_pipl_entrypoint(hostile_length.data(), hostile_length.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto vendor_private_duplicate = effect;
  vendor_private_duplicate[6] = 3;
  const std::size_t private_offset = vendor_private_duplicate.size();
  append_pipl_property(vendor_private_duplicate, "4668", {'O', 't', 'h', 'e', 'r', 0});
  std::copy_n("VEND", 4, vendor_private_duplicate.begin() + private_offset);
  const auto private_parsed = parse_pipl_entrypoint(
      vendor_private_duplicate.data(), vendor_private_duplicate.size());
  return private_parsed.kind == PiplPluginKind::Effect &&
      private_parsed.symbol == "entryPointFunc";
}

using AddParamCallback = int32_t(__cdecl*)(void*, int32_t, void*);

auto& g_last_seh_exception_code = selector_dispatch_telemetry().seh_code;
auto& g_last_seh_exception_address = selector_dispatch_telemetry().seh_address;
auto& g_last_seh_exception_module = selector_dispatch_telemetry().seh_module;
auto& g_last_seh_selector = selector_dispatch_telemetry().selector;
auto& g_last_seh_error = selector_dispatch_telemetry().error;
// Retained entry/admission state: the trace sink is installed by
// WorkerSession for the session's lifetime and cleared on teardown.
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
// Source-contract anchors: these owner bindings document that the arbitrary
// counters live in parameters::state() (issue #126 Phase D) and are asserted
// by the arbitrary-parameter source tests.
auto& g_arbitrary_print_failures = g_parameter_runtime.arbitrary.print_failures;
auto& g_arbitrary_roundtrip_failures = g_parameter_runtime.arbitrary.roundtrip_failures;
auto& g_arbitrary_compare_disagreements = g_parameter_runtime.arbitrary.compare_disagreements;
auto& g_arbitrary_new_calls = g_parameter_runtime.arbitrary.new_calls;
auto& g_last_arbitrary_interpolation_amount =
    g_parameter_runtime.arbitrary.last_interpolation_amount;
auto& g_params = g_parameter_runtime.records;
auto& g_parameter_timelines = g_parameter_runtime.timelines;
// Retained entry/admission state: the admitted plug-in path, set once after
// WorkerSession admission for diagnostics.
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
// AEGP command/menu bookkeeping and the event-mode latches moved to their
// owner, aexcompat::worker_runtime::aegp_init::state() (issue #126 Phase D).
auto& g_aegp_keyframe_roundtrip_mode = g_aegp_init_runtime.keyframe_roundtrip_mode;
auto& g_aegp_seek_roundtrip_mode = g_aegp_init_runtime.seek_roundtrip_mode;
auto& g_aegp_trim_roundtrip_mode = g_aegp_init_runtime.trim_roundtrip_mode;
auto& g_aegp_switch_roundtrip_mode = g_aegp_init_runtime.switch_roundtrip_mode;
auto& g_skip_about = g_aegp_init_runtime.skip_about;
uint32_t& g_aegp_item_set_current_time_calls = scene_runtime_state().item_set_current_time_calls;
int32_t& g_aegp_item_last_set_time_value = scene_runtime_state().item_last_set_time_value;
uint32_t& g_aegp_item_last_set_time_scale = scene_runtime_state().item_last_set_time_scale;
uint32_t& g_aegp_item_type_calls = scene_runtime_state().item_type_calls;
uint32_t& g_aegp_layer_source_item_calls = scene_runtime_state().layer_source_item_calls;
uint32_t& g_aegp_layer_trim_set_calls = scene_runtime_state().layer_trim_set_calls;
uint32_t& g_aegp_layer_flag_set_calls = scene_runtime_state().layer_flag_set_calls;
auto& g_aegp_layer_flags = scene_runtime_state().layer_flags;
uint32_t& g_aegp_keyframe_time_calls = scene_runtime_state().keyframe_time_calls;
uint32_t& g_aegp_keyframe_value_calls = scene_runtime_state().keyframe_value_calls;
uint32_t& g_aegp_keyframe_interpolation_calls = scene_runtime_state().keyframe_interpolation_calls;
int32_t& g_aegp_scene_frame = scene_runtime_state().scene_frame;
using AegpCommandRegistration =
    aexcompat::worker_runtime::aegp_init::CommandRegistration;
using AegpUpdateMenuRegistration =
    aexcompat::worker_runtime::aegp_init::UpdateMenuRegistration;
auto& g_aegp_inserted_commands = g_aegp_init_runtime.inserted_commands;
auto& g_checkout_layer_definitions = g_parameter_runtime.checkout.definitions;
auto& g_param_checkout_mutex = g_parameter_runtime.checkout.mutex;
auto& g_live_param_checkouts = g_parameter_runtime.checkout.live;
auto& g_param_checkout_calls = g_parameter_runtime.checkout.checkout_calls;
auto& g_param_checkin_calls = g_parameter_runtime.checkout.checkin_calls;
auto& g_automatic_param_checkins = g_parameter_runtime.checkout.automatic_checkins;
auto& g_invalid_param_checkins = g_parameter_runtime.checkout.invalid_checkins;

// Host-callback telemetry storage moved to its owner,
// aexcompat::worker_runtime::classic::host_callback_telemetry()
// (issue #126 Phase D); these references keep the g_* spellings.
auto& g_host_callback_telemetry =
    aexcompat::worker_runtime::classic::host_callback_telemetry();
auto& g_transform_world_calls = g_host_callback_telemetry.transform_world_calls;
auto& g_last_transform_x = g_host_callback_telemetry.last_transform_x;
auto& g_last_transform_y = g_host_callback_telemetry.last_transform_y;
auto& g_last_transform_opacity = g_host_callback_telemetry.last_transform_opacity;
// Custom-UI/Drawbot/App telemetry storage moved to its owner,
// aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry()
// (issue #126 Phase D); these references keep the g_* spellings.
using CustomUiRegistration =
    aexcompat::worker_runtime::ui_event_execution::CustomUiRegistration;
auto& g_custom_ui_telemetry =
    aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry();
auto& g_register_ui_calls = g_custom_ui_telemetry.register_ui_calls;
auto& g_custom_ui_registration = g_custom_ui_telemetry.registration;
auto& g_invalid_custom_ui_registrations = g_custom_ui_telemetry.invalid_custom_ui_registrations;
using ExternalLayerInput =
    aexcompat::worker_runtime::request_parser::LayerInput;

// Opt-in world snapshot dumps and output checksum detail (issue #19). Both
// default off; the broker enables them per run with the --dump-worlds-v1 and
// --output-checksum-detail-v1 argv trailers, and the dump directory is
// broker-managed. Raw pixel bytes never enter the JSON report; only counts,
// row CRCs, and channel digests do.
// World-dump and output-checksum telemetry storage moved to its owner
// aexcompat::render::telemetry_state() (issue #126 Phase D).
// Mask scene identity (model enabled + scene id) moved to its owner
// aexcompat::mask_runtime (issue #126 Phase D).
// Spatial/quality render context storage moved to its owner,
// aexcompat::render::render_context_state() (issue #126 Phase D); these
// references keep the established g_* spellings at the use sites.
auto& g_render_context_state = aexcompat::render::render_context_state();
auto& g_downsample_x = g_render_context_state.downsample_x;
auto& g_downsample_y = g_render_context_state.downsample_y;
auto& g_pixel_aspect_ratio = g_render_context_state.pixel_aspect_ratio;
auto& g_full_resolution_width = g_render_context_state.full_resolution_width;
auto& g_full_resolution_height = g_render_context_state.full_resolution_height;
auto& g_pre_effect_source_origin_x = g_render_context_state.pre_effect_source_origin_x;
auto& g_pre_effect_source_origin_y = g_render_context_state.pre_effect_source_origin_y;
auto& g_render_quality = g_render_context_state.render_quality;
auto& g_render_field = g_render_context_state.render_field;
auto& g_shutter_angle = g_render_context_state.shutter_angle;
auto& g_shutter_phase = g_render_context_state.shutter_phase;

static_assert(sizeof(MaskFeather) == 40);
static_assert(offsetof(MaskFeather, segment_s) == 8);
static_assert(offsetof(MaskFeather, radius) == 16);
static_assert(offsetof(MaskFeather, interp) == 32);
static_assert(offsetof(MaskFeather, type) == 33);
static_assert(sizeof(HostTime) == 8);
static_assert(sizeof(StreamValue) == 40);
OutlineData* sampled_outline(HostStreamRef* stream, const HostTime* time,
                             std::unique_ptr<OutlineData>& owned);
// Retained host-object identities (issue #126 Phase D): the tagged effect
// and layer refs every suite callback validates against. They are the
// dispatch-boundary identity the suites pin, so they stay owned beside the
// callback ABI in this TU. Lifetime: process-lifetime constants.
OpaqueHostObject g_effect{0x45464658};
OpaqueHostObject g_layer{0x4c415952};

// The mask scene helpers and configure_mask_scene moved to their state
// owner, worker_mask_runtime_callbacks.cpp (issue #170); worker_main keeps
// resolving them through these declarations.
void raise_mask_access_violation();
aexcompat::mask_runtime::Snapshot mask_runtime_snapshot();
bool snapshot_mask_curve(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve);
bool install_synthetic_mask_scene(
    const std::vector<aexcompat::mask_runtime::CurveSnapshot>& curves);
std::vector<aexcompat::pf_path_runtime::PathInfo> enumerate_pf_paths();
bool snapshot_pf_path(void* handle, aexcompat::mask_runtime::CurveSnapshot& curve);
bool bounded_pf_path_world(void* world, aexcompat::pf_path_runtime::WorldView& view);
bool mask_lifetimes_balanced();
bool configure_mask_scene(const std::string& scene_id);
std::size_t mask_open_count();
std::size_t mask_tangent_vertex_count();
constexpr int32_t kPfBadCallbackParam = 516;
constexpr int32_t kPfSuiteToolNone = 0;
using aexcompat::pf_helper::reset;



// PF_CheckoutResult is 76 bytes: result_rect @0, max_result_rect @16,
// par (rational num/den) @32, solid + 3 reserved bytes @40, ref_width @44,
// ref_height @48, 6 reserved longs @52. Callers pass the struct uninitialized,
// so every field must be written, not only the rects. ref_width/ref_height are
// the pre-downsample layer dimensions, which differ from the checkout world
// size when a spatial context supplies a full resolution.
constexpr size_t kCheckoutResultBytes = 76;

constexpr uint32_t kMaxGuidMixInBytes = 1024 * 1024;
// The comp-bg-color / GUID mix-in counters moved to their owner,
// aexcompat::worker_runtime::smart::host_telemetry() (issue #126 Phase D).

void reset_smart_host_telemetry() {
  auto& telemetry = aexcompat::worker_runtime::smart::host_telemetry();
  telemetry.comp_bg_color_successes.store(0, std::memory_order_relaxed);
  telemetry.comp_bg_color_rejections.store(0, std::memory_order_relaxed);
  telemetry.guid_mix_in_calls.store(0, std::memory_order_relaxed);
  telemetry.guid_mix_in_successes.store(0, std::memory_order_relaxed);
  telemetry.guid_mix_in_rejections.store(0, std::memory_order_relaxed);
  telemetry.guid_mix_in_last_size.store(0, std::memory_order_relaxed);
  telemetry.guid_mix_in_max_size.store(0, std::memory_order_relaxed);
  telemetry.guid_mix_in_last_result.store(0, std::memory_order_relaxed);
}

int32_t __cdecl guid_mix_in_ptr(void* effect_ref, uint32_t size, const void* bytes) {
  auto& telemetry = aexcompat::worker_runtime::smart::host_telemetry();
  telemetry.guid_mix_in_calls.fetch_add(1, std::memory_order_relaxed);
  telemetry.guid_mix_in_last_size.store(size, std::memory_order_relaxed);
  uint32_t observed = telemetry.guid_mix_in_max_size.load(std::memory_order_relaxed);
  while (observed < size && !telemetry.guid_mix_in_max_size.compare_exchange_weak(
      observed, size, std::memory_order_relaxed)) {}
  const int32_t result = effect_ref == &g_effect && bytes && size > 0 &&
      size <= kMaxGuidMixInBytes ? 0 : 4;
  (result == 0 ? telemetry.guid_mix_in_successes : telemetry.guid_mix_in_rejections)
      .fetch_add(1, std::memory_order_relaxed);
  telemetry.guid_mix_in_last_result.store(result, std::memory_order_relaxed);
  return result;
}




// register_with_aegp / get_main_hwnd live in worker_aegp_utility_suite.cpp.

// The PF Interface effect/camera callbacks and their declarations live in
// worker_aegp_pf_interface_suite.{hpp,cpp} (issue #170).











// The AEGP PF Interface Suite table lives in worker_aegp_pf_interface_suite.cpp.
int32_t __cdecl unsupported_path_mask() { return 4; }
struct LegacyRect { int32_t left, top, right, bottom; };

int32_t __cdecl pf_mask_world_with_path(void* effect_ref, void** path, double feather_x,
                                        double feather_y, int32_t invert, double opacity,
                                        int32_t quality, void* world, LegacyRect* bounds);
#include "worker_l2_suite_abi.hpp"

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
// The mask/stream/keyframe suite tables live in worker_mask_suite_tables.cpp.
// PF_AdvAppSuite1 is frozen at ten callbacks; keep its storage independent
// from the eleven-slot v2 table so versioned suite identity cannot alias.
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
// The render suite ABI structs and their asserts live in
// worker_l2_render_abi.hpp with the callback declarations they freeze.
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

// Drawbot/App custom-UI callbacks, their opaque object tables, and the
// HostUiContext event block moved to their owner, worker_drawbot_runtime.cpp
// (issue #170); the suite assembly below keeps resolving them through
// worker_drawbot_runtime.hpp.
auto& g_app_picker_color = g_custom_ui_telemetry.app_picker_color;
auto& g_ui_drag_calls = g_custom_ui_telemetry.ui_drag_calls;
auto& g_ui_drag_requested = g_custom_ui_telemetry.ui_drag_requested;
auto& g_ui_drag_terminated = g_custom_ui_telemetry.ui_drag_terminated;
auto& g_render_click_enabled = g_custom_ui_telemetry.render_click_enabled;
auto& g_render_draw_enabled = g_custom_ui_telemetry.render_draw_enabled;
auto& g_render_click_x = g_custom_ui_telemetry.render_click_x;
auto& g_render_click_y = g_custom_ui_telemetry.render_click_y;
auto& g_render_ui_context_closed = g_custom_ui_telemetry.render_ui_context_closed;

// Receipt test-mode storage moved to aexcompat::render_receipts::
// receipt_test_state() (issue #126 Phase D).
auto& g_async_manager = aexcompat::render_receipts::receipt_test_state().async_manager;
// get_context_async_manager moved to worker_l2_render_abi.cpp (issue #170).
int32_t __cdecl get_context_async_manager(void* input, void* extra, void** manager);


constexpr int32_t kSyntheticCompWidth = 17;
constexpr int32_t kSyntheticCompHeight = 9;
// The g_render_options_* probe fixtures moved to their writer,
// worker_aegp_render_selftests.cpp (issue #126 Phase D); the declarations in
// its header keep the custom-selftest hook wiring below resolving.
auto& g_synthetic_receipt_test_mode =
    aexcompat::render_receipts::receipt_test_state().synthetic_test_mode;
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

// The PF AE Adv Item Suite callbacks, ABI, and table live in
// worker_l2_render_abi.{hpp,cpp} (issue #170).
void drain_async_layer_requests();

bool async_layer_requests_balanced();
// The render receipt/async checkout callbacks and the render-runtime
// configure wiring moved to their owner, worker_l2_render_abi.cpp
// (issue #170); the suite assembly and selftest hooks below keep
// resolving them through worker_l2_render_abi.hpp declarations.

bool world_lifetimes_balanced();
// The worker-entry selftest verify bridges, the world/receipt selftest
// hooks, the loaded-effect receipt fixture, and the PF parameter state
// capture moved to worker_entry_wiring.cpp (issue #165); worker_main keeps
// resolving the survivors below through these declarations.
bool async_receipt_lifetimes_balanced();
bool verify_suite_release_without_acquire_rejected();


// The PF Pixel Data suites and their verify live in
// worker_pf_pixel_data_suite.cpp.


// The PF World Suite tables live in worker_pf_world_suite.cpp.

// The PF pixel format registry (registration callbacks, GLOBAL_SETUP gate,
// and accounting) lives in worker_pf_pixel_format_registry.cpp.


// The Point/Angle/ColorParam and ParamUtils production callbacks moved to
// their table owner, worker_pf_param_suites.cpp (issue #170); the shared
// animation helpers below stay here with the lifecycle apply path.

template <typename T, std::size_t N>
void write(std::array<std::byte, N> &bytes, std::size_t offset, T value);







// The PF param suite tables (Point/Angle/ColorParam/ParamUtils1/3) live in
// worker_pf_param_suites.cpp; their production callbacks stay here.

// verify_pixel_format_registry_rejection lives in
// worker_pf_pixel_format_registry.cpp.

// The world registry rejection/snapshot verifies live in
// worker_pf_world_suite.cpp.

std::string missing_suites_report_json() {
  return suite_registry().missing_suites_report_json();
}

std::string unsupported_suite_calls_report_json() {
  return suite_registry().unsupported_suite_calls_report_json();
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

// The AEGP Command/Register Suite callbacks and tables live in
// worker_aegp_command_suites.cpp (issue #170).

AegpSceneObject& g_aegp_comp_item = scene_runtime_state().composition_item;
AegpSceneObject& g_aegp_comp = scene_runtime_state().composition;
void* aegp_comp_item_handle() { return composition_item_handle(); }
struct AegpColorVal { double alpha, red, green, blue; };
static_assert(sizeof(AegpColorVal) == 4 * sizeof(double));
int32_t __cdecl aegp_get_comp_bg_color(void* comp, AegpColorVal* color) {
  if (comp != &g_aegp_comp || !color) {
    aexcompat::worker_runtime::smart::host_telemetry()
        .comp_bg_color_rejections.fetch_add(1, std::memory_order_relaxed);
    return 4;
  }
  const AegpColorVal headless_color{1.0, 0.0, 0.0, 0.0};
  *color = headless_color;
  aexcompat::worker_runtime::smart::host_telemetry()
      .comp_bg_color_successes.fetch_add(1, std::memory_order_relaxed);
  return 0;
}

std::array<AegpSceneObject, 3>& g_aegp_layers = scene_runtime_state().layers;
AegpSceneObject& g_aegp_effect = scene_runtime_state().effect;
int32_t& g_aegp_active_camera_layer_index =
    scene_runtime_state().active_camera_layer_index;


// The PF Interface effect/camera callbacks live in
// worker_aegp_pf_interface_suite.cpp (issue #170). AEGP project/item/comp/
// layer/effect/collection/stream/keyframe callbacks are compiled in
// worker_aegp_scene.cpp.
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

// The host suite catalog wiring (component providers, the assembly hook
// table, the static suite catalog, and acquire/release_suite) moved to
// worker_host_suite_wiring.cpp (issue #171); worker_main keeps resolving
// acquire/release through worker_l2_render_abi.hpp.


// The SPBasic-style BasicSuite ABI and table live in
// worker_l2_render_abi.{hpp,cpp}.


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

// The render-worker click/draw probes and close_render_ui_context moved to
// worker_drawbot_runtime.cpp with the UI context they arm (issue #170).
bool dispatch_render_click(EffectEntry entry, std::array<std::byte, kInSize>& input,
                           std::array<std::byte, kOutSize>& output,
                           std::vector<std::array<std::byte, kParamSize>>& definitions);
bool dispatch_render_draw(EffectEntry entry, std::array<std::byte, kInSize>& input,
                          std::array<std::byte, kOutSize>& output,
                          std::vector<std::array<std::byte, kParamSize>>& definitions);
bool close_render_ui_context(EffectEntry entry, std::array<std::byte, kInSize>& input,
                             std::array<std::byte, kOutSize>& output,
                             std::vector<std::array<std::byte, kParamSize>>& definitions);



// The legacy AEGP effect stream (v2) and effect-param-union (v3) callbacks
// moved to their owner, worker_aegp_scene.cpp (issue #170); the suite tables
// below keep resolving them through the declarations above.


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




// The PF parameter checkout/checkin callbacks and their balance accounting
// moved to their owner, worker_param_checkout_runtime.cpp (issue #170).

bool sha256(const std::filesystem::path& path, std::string& result);
// The shared worker-entry helpers (rational time, animation evaluation,
// guarded selector invokers, diagnostic escaping/hashing, and the world-dump
// telemetry wrappers) moved to worker_l2_shared_helpers.cpp (issue #165);
// worker_main keeps resolving them through these declarations.
const ParameterTimeline* parameter_timeline(int32_t slot);
bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale);
void write_rect(void* destination, int32_t width, int32_t height);
ParameterAnimationKey evaluate_animation(const ParameterTimeline& timeline,
                                         int32_t time, uint32_t scale);
bool write_animation_value(std::array<std::byte, kParamSize>& definition,
                           const ParamRecord& param,
                           const ParameterAnimationKey& key);
bool apply_parameter_animation(
    std::vector<std::array<std::byte, kParamSize>>& definitions, int32_t time,
    uint32_t scale);
int32_t invoke_global_setdown(EffectEntry entry, void* input, void* output);
int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr);
std::string escape(const std::string& input);
std::string sha256_bytes(const unsigned char* data, std::size_t size);
void dump_world_snapshot(const std::string& stage, const unsigned char* packed_argb,
                         int32_t width, int32_t height, int32_t pixel_bytes);
void record_output_checksum_detail(const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes);
std::string world_debug_report_json();
int32_t __cdecl add_param(void*, int32_t index, void* definition);
// The classic host callbacks (duck_quack/abort_render/report_progress), the
// custom-UI registration/adv-app-info callbacks, add_param, and
// set_options_button_name moved to their owners (issue #170):
// worker_classic_runtime.cpp, worker_drawbot_runtime.cpp, and
// worker_parameter_execution.cpp.


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






auto& g_user_changed_parameters = g_parameter_runtime.user_changed_parameters;


// The broker payload parsers moved to worker_l2_payload_parsers.cpp
// (issue #165); worker_main keeps resolving them through these declarations.
bool parse_layer_transport_key(const wchar_t* text, ExternalLayerInput& layer);
bool parse_mask_context_payload(const wchar_t* text);
bool parse_spatial_context_payload(const wchar_t* text);
bool parse_render_environment_payload(const wchar_t* text);
bool parse_parameter_payload(const wchar_t* text, RequestedAssignments& output);

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


// The classic/smart render runtimes (lifecycle wrappers, dispatch owners,
// classic_render_runtime, render_once, smart_render_runtime, and
// smart_render_once) moved to worker_classic_render_runtime.cpp
// (issue #170); the cross-TU declarations in
// worker_invocation_orchestration.cpp and worker_render_session.cpp keep
// resolving them there.
// run_render_session moved to worker_render_session.cpp (issue #169); the
// cross-TU declaration in worker_invocation_orchestration.cpp still resolves
// to that owner.


using SmartResult = aexcompat::worker_runtime::smart_execution::Result;


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
  c.lifecycle_errors = lifecycle_errors; c.lifecycle_data_null = lifecycle_data_null;
  c.unsupported_suite_calls_json = unsupported_suite_calls_report_json();
  c.module_audit_json = module_audit_json();
  c.parameters.reserve(g_params.size());
  for (const auto& p : g_params) {
    const auto* name = reinterpret_cast<const char*>(p.raw.data() + kParamName);
    c.parameters.push_back({p.index, p.disk_id, p.type, read<uint32_t>(p.raw, kParamUiFlags), read<int16_t>(p.raw, 8), read<int16_t>(p.raw, 10), read<uint32_t>(p.raw, kParamFlags), std::string(name, strnlen_s(name, kParamNameSize)), p.has_numeric, p.valid_min, p.valid_max, p.slider_min, p.slider_max, p.default_value, p.has_current, p.current_value, p.has_color, p.default_color, p.current_color, p.component_count, p.default_components, p.current_components, p.precision, p.choices, p.label, p.arbitrary_summary, p.layer_default});
  }
  std::cout << aexcompat::worker_report::serialize_l2_report(c);
}

// The early-mode bridge (EarlyModeBridge and its l2mode hook adapters)
// moved to worker_early_mode_bridge.cpp (issue #165); worker_main keeps
// resolving it through worker_early_mode_bridge.hpp.


}  // namespace aexcompat::l2_detail

using namespace aexcompat::l2_detail;

// verify_pf_color_suite / verify_pf_color_param_suite live in
// worker_pf_color_selftests.cpp; host state reaches them through the
// configure() hooks installed below.



// verify_aegp_effect_param_union_suite4 /
// verify_aegp_installed_effect_catalog_suite4 live in
// worker_aegp_compat_selftests.cpp behind configure_aegp_compat_selftests.

// verify_aegp_layer_render_options_suite2 and its async callback live in
// worker_aegp_render_selftests.cpp.

// verify_pf_adv_app_suite_versions / verify_render_output_safety live in
// worker_host_guard_selftests.cpp behind its configure() hooks.

// verify_suite_entry_guards_and_utility13 lives in
// worker_aegp_utility_suite.cpp with the UtilitySuite ABI it validates.










bool set_l2_dump_worlds_dir(void*, const wchar_t* value) {
  auto& state = aexcompat::render::telemetry_state();
  state.dump_worlds_dir = std::filesystem::path(value ? value : L"");
  std::error_code error;
  return !state.dump_worlds_dir.empty() && std::filesystem::is_directory(state.dump_worlds_dir, error);
}

bool enable_l2_checksum_detail(void*) {
  aexcompat::render::telemetry_state().output_checksum_detail = true;
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

bool parse_l2_conformance_render_settings(void*, const wchar_t* value) {
  return aexcompat::worker_render_report::parse_conformance_render_settings(value);
}

bool __cdecl scene_render_receipt_enabled() {
  return is_render_worker() && aexcompat::aegp_layer_render_runtime::active();
}

// The report diagnostics builders (gpu/SEH/classic subsystems) live in
// worker_render_report.cpp with the snapshot types they populate.
using aexcompat::worker_render_report::capture_gpu_diagnostics;
using aexcompat::worker_render_report::capture_seh_diagnostics;
using aexcompat::worker_render_report::capture_classic_subsystems;

// The six former host selftest wrappers and their JSON now live in
// worker_fixed_selftest_routing.cpp beside the command catalog.




// L2 parameter-lifecycle probe (issue #171): the non-rendering inspection
// path's sequence/frame selector walk, user-changed-param dispatch, and
// conditional-UI pass, extracted verbatim from worker_main_impl. Returns an
// exit code when the requested assignments are rejected (global setdown has
// already run), std::nullopt otherwise.
std::optional<int> run_l2_parameter_lifecycle(EffectEntry entry,
    std::array<std::byte, kInSize>& input, std::array<std::byte, kOutSize>& output,
    int32_t global_error, int32_t params_error,
    std::array<int32_t, 5>& lifecycle_errors, bool& lifecycle_data_null,
    bool& user_changed_ok, bool& conditional_ui_ok) {
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
    return 3;
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
  return std::nullopt;
}

// The worker-entry bootstrap wiring and the selftest command dispatch moved
// to worker_entry_wiring.cpp (issue #171 / #165); worker_main_impl keeps
// consuming them through these declarations.
int configure_worker_entry_bootstrap();
std::optional<int> dispatch_worker_selftests(int argc, wchar_t** argv);

int worker_main_impl(int argc, wchar_t **argv) {
  if (const int bootstrap_error = configure_worker_entry_bootstrap())
    return bootstrap_error;
  if (argc == 2 && std::wstring(argv[1]) == L"--self-test-pipl-entrypoint") {
    const bool passed = verify_pipl_entrypoint_parser();
    std::cout << "{\"pipl_entrypoint\":\"" << (passed ? "passed" : "failed")
              << "\",\"kind_discriminator\":true,\"code_win64_x86\":true,"
                 "\"bounded\":true,\"aegp_not_effect\":true}\n";
    return passed ? 0 : 72;
  }
  if (const auto selftest_exit = dispatch_worker_selftests(argc, argv))
    return *selftest_exit;
  // The broker passes only an authenticated inherited file handle via
  // environment (issue #18). No dump directory or dump path is accepted on the
  // worker command line, so nothing is consumed from argv here. Fails closed on
  // a real misconfiguration; a no-op when opt-in is off.
  if (!aexcompat::worker_runtime::minidump::configure_from_inherited_handle())
    return 3;
aexcompat::worker_runtime::invocation::InvocationState invocation;


  const auto parse_requested_payload = +[](const wchar_t* text, void* context) {
    return parse_parameter_payload(text, *static_cast<RequestedAssignments*>(context));
  };
  const aexcompat::worker_runtime::invocation::ApplyHooks invocation_hooks{
      +[](int32_t x, int32_t y, const std::array<float, 4>& color) {
        g_render_click_x = x; g_render_click_y = y;
        g_app_picker_color = color; g_render_click_enabled = true;
      },
      +[] { g_render_draw_enabled = true; },
      +[](bool enabled) { aexcompat::mask_runtime::set_model_enabled(enabled); },
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
        load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation,
        parse_l2_conformance_render_settings},
       &parse_layer_transport_key, &parse_mask_context_payload,
       &parse_spatial_context_payload, &parse_render_environment_payload,
       &invocation.requested_parameters, parse_requested_payload, &configure_mask_scene});
    if (parsed.error != 0) return parsed.error;
    aexcompat::worker_runtime::invocation::apply_render(
        parsed.invocation, invocation, invocation_hooks);
  } else if (is_smart_worker()) {
    const auto parsed = aexcompat::worker_runtime::request_parser::parse(
        aexcompat::worker_runtime::request_parser::Kind::Smart, argc, argv,
        {{nullptr, set_l2_dump_worlds_dir, enable_l2_checksum_detail,
          load_l2_aux_manifest, parse_l2_alpha_coverage, load_l2_parameter_animation,
          parse_l2_conformance_render_settings},
         &parse_layer_transport_key, &parse_mask_context_payload,
         &parse_spatial_context_payload, &parse_render_environment_payload,
         &invocation.requested_parameters, parse_requested_payload, &configure_mask_scene});
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
  if (is_rendering_worker()) aexcompat::mask_runtime::set_model_enabled(true);
  RuntimeHostHooks runtime_hooks{&sha256, &redirect_native_stdout,
                                 &restore_native_stdout};
  RuntimeAdmissionRequest runtime_request;
  const int request_error = aexcompat::worker_runtime::prepare_runtime_request(
      argv[2], argv[3], !is_rendering_worker() && invocation.runtime_module_authorization_mode,
      invocation.runtime_module_authorization_mode ? argv[5] : nullptr, runtime_request);
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
    KeyframePipeProbe keyframe_probe;
    SeekPipeProbe seek_probe;
    TrimPipeProbe trim_probe;
    SwitchPipeProbe switch_probe;
    // Compatibility source anchors: run_orchestration owns the former
    // aegp_init::dispatch_basic_events(...) and
    // aegp_init::dispatch_death(global_refcon) calls in that exact order.
    const auto aegp_init =
        aexcompat::worker_runtime::aegp_init::run_orchestration(
            {aegp_entry, &g_basic_suite, &g_aegp_inserted_commands,
             &g_aegp_scene_frame, &keyframe_probe, &seek_probe, &trim_probe,
             &switch_probe,
             {g_aegp_update_menu_mode, g_aegp_idle_mode,
              g_aegp_command_roundtrip_mode,
              {g_aegp_active_idle_roundtrip_mode,
               g_aegp_comp_idle_roundtrip_mode,
               g_aegp_keyframe_roundtrip_mode, g_aegp_seek_roundtrip_mode,
               g_aegp_trim_roundtrip_mode, g_aegp_switch_roundtrip_mode}}},
            {nullptr,
             +[](void*) { return g_aegp_keyframe_time_calls == 2 &&
                 g_aegp_keyframe_value_calls == 2 &&
                 g_aegp_keyframe_interpolation_calls == 2; },
             +[](void*) { return g_aegp_item_set_current_time_calls == 1 &&
                 g_aegp_item_last_set_time_value == 75 &&
                 g_aegp_item_last_set_time_scale == 30 &&
                 g_aegp_scene_frame == 75; },
             +[](void*) {
               const auto& in_point = g_aegp_layer_in_points[0];
               const auto& duration = g_aegp_layer_durations[0];
               return g_aegp_layer_trim_set_calls == 1 &&
                   in_point.value == 30 && in_point.scale == 30 &&
                   duration.value == 210 && duration.scale == 30;
             },
             +[](void*) { return g_aegp_layer_flag_set_calls == 4 &&
                 g_aegp_layer_flags[0] == 0x00004026u &&
                 g_aegp_layer_flags[1] == 0x00000005u &&
                 g_aegp_layer_flags[2] == 0x00000005u; }});
    // Capture the event-complete and terminal loaded-module sets before the
    // AEGP is unloaded so secure broker launches can validate this early path.
    // Stop and unload first. Some AEGP_SuiteHandler builds retain exactly one
    // Item Suite cache until process teardown; the isolated worker owns that
    // final reclamation, but every other outstanding lease remains a failure.
    capture_module_audit_phase();
    const bool module_audit_ok = session.shutdown_before_report();
    module = nullptr;
    const bool passed = emit_aegp_init_completion_report(
        {aegp_init.init_error, aegp_init.event_error, aegp_init.death_error,
         aegp_init.global_refcon != nullptr, aegp_init.hooks_invoked,
         aegp_init.menu_hooks_invoked, aegp_init.death_hooks_invoked,
         aegp_init.command_hooks_invoked, aegp_init.command_handled_count,
         aegp_init.idle_max_sleep, module_audit_ok,
         {keyframe_probe.connected, keyframe_probe.request_sent,
          keyframe_probe.response_received, keyframe_probe.response_valid,
          keyframe_probe.response_bytes},
         {seek_probe.connected, seek_probe.request_sent,
          seek_probe.ack_received, seek_probe.ack_valid},
         {trim_probe.connected, trim_probe.request_sent,
          trim_probe.ack_received, trim_probe.ack_valid},
         {switch_probe.connected, switch_probe.request_sent,
          switch_probe.ack_received, switch_probe.ack_valid}});
    return session.finish_integrated_report(passed ? 0 : 23);
  }
  // Resolve the Effect entrypoint from the plug-in's own PiPL (issue #84): the
  // Kind atom decides the ABI and CodeWin64X86 names the export, so an Effect
  // whose entrypoint is not literally "EffectMain" still dispatches and a
  // Kind=AEGP plug-in is never handed to the Effect selector. Fail closed on
  // AEGP, missing/invalid PiPL, ambiguity, or an unresolvable symbol.
  const PiplEntrypoint pipl_entrypoint = discover_pipl_entrypoint(module);
  if (pipl_entrypoint.kind != PiplPluginKind::Effect) {
    const char* plugin_kind =
        pipl_entrypoint.kind == PiplPluginKind::Aegp
            ? "aegp_candidate"
            : (pipl_entrypoint.kind == PiplPluginKind::Invalid
                   ? "invalid_pipl"
                   : "unknown_no_effect_entrypoint");
    std::cerr << "plugin_kind:" << plugin_kind << "\n" << std::flush;
    return session.finish(12);
  }
  auto entry = reinterpret_cast<EffectEntry>(
      GetProcAddress(module, pipl_entrypoint.symbol.c_str()));
  if (!entry) {
    std::cerr << "plugin_kind:unknown_no_effect_entrypoint\n" << std::flush;
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
        reinterpret_cast<void*>(&get_pixel_data16),
        // Handle callbacks in in_data->utils (issue #220): a conformant AE host
        // provides host_new_handle/lock/unlock/dispose/get_handle_size/resize
        // through the utility block, not only through the PF Handle Suite. The
        // index order here must match the tail of kUtilityCallbackOffsets
        // (160/168/176/184/440/464) in worker_effect_bootstrap.cpp.
        reinterpret_cast<void*>(&new_handle), reinterpret_cast<void*>(&lock_handle),
        reinterpret_cast<void*>(&unlock_handle), reinterpret_cast<void*>(&dispose_handle),
        reinterpret_cast<void*>(&handle_size), reinterpret_cast<void*>(&resize_handle)},
       &g_color_suite8, sizeof(g_color_suite8),
       &g_basic_suite, &g_effect},
      {g_render_quality, g_render_field, g_shutter_angle, g_shutter_phase,
       {g_pre_effect_source_origin_x, g_pre_effect_source_origin_y},
       {static_cast<int32_t>(g_downsample_x.numerator),
        static_cast<int32_t>(g_downsample_x.denominator)},
       {static_cast<int32_t>(g_downsample_y.numerator),
        static_cast<int32_t>(g_downsample_y.denominator)},
       {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
        static_cast<int32_t>(g_pixel_aspect_ratio.denominator)},
       invocation.external_pixel_bytes,
       is_render_worker(), is_rendering_worker(),
       // The audio session is an audio invocation too: without this the
       // admission "requested audio" flag stays false and host_audio::Runtime
       // rejects unadvertised audio checkouts on the session path, breaking
       // audio-only plug-ins that render fine under --render-audio (Codex #252).
       invocation.audio_mode || invocation.audio_session_mode, g_skip_about},
      {&invoke_entry_seh, &reset_effect_lifetime,
       +[](bool active) { g_global_setup_active = active; },
       +[](bool requested, bool advertised) {
         aexcompat::host_audio::runtime().configure_admission(requested, advertised);
       },
       +[](EffectEntry callback,
           aexcompat::worker_runtime::effect_bootstrap::State& state) {
         observe_arbitrary_defaults(callback, state.input, state.output);
       },
       +[]() { return static_cast<int32_t>(g_params.size()); }});
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
  // write(utils, kUtilsNewHandle, &new_handle)
  // write(utils, kUtilsLockHandle, &lock_handle)
  // write(utils, kUtilsUnlockHandle, &unlock_handle)
  // write(utils, kUtilsDisposeHandle, &dispose_handle)
  // write(utils, kUtilsGetHandleSize, &handle_size)
  // write(utils, kUtilsResizeHandle, &resize_handle)
  // memcpy(utils.data() + kUtilsColorCallbacks, &g_color_suite8, sizeof(g_color_suite8))
  // write(utils, kUtilsBeginSampling, &begin_sampling8)
  // write(utils, kUtilsAreaSample, &area_sample8)
  // write(utils, kUtilsEndSampling, &end_sampling8)
  // write(input, 24, &abort_render);
  // write(input, 32, &report_progress);
  // invocation.external_pixel_bytes == 8 &&
  // invocation.external_pixel_bytes == 16 &&
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
      (invocation.adjust_cursor_mode || invocation.draw_event_mode || invocation.click_event_mode || invocation.drag_event_mode ||
       invocation.ui_lifecycle_mode || invocation.ui_idle_mode || invocation.ui_keydown_mode || invocation.ui_mouse_exited_mode)) {
    int32_t event_error = -1;
    int32_t cursor = 0;
    int32_t event_out_flags = 0;
    bool changed_value = false;
    const bool registered_effect_ui = (g_custom_ui_registration.events & 4u) != 0;
    const bool registered_layer_ui = (g_custom_ui_registration.events & 2u) != 0;
    const bool registered_comp_ui = (g_custom_ui_registration.events & 1u) != 0;
    if (invocation.ui_mouse_exited_mode && !registered_layer_ui && !registered_comp_ui) {
      dispose_arbitrary_defaults(entry, input, output);
      if (global_error == 0)
        invoke_global_setdown(entry, input.data(), output.data());
      return session.finish(19);
    }
    const char* event_target = invocation.drag_event_mode || invocation.ui_mouse_exited_mode ||
        (!registered_effect_ui && registered_layer_ui)
        ? "layer" : (!registered_effect_ui && !registered_layer_ui &&
                     registered_comp_ui ? "comp" : "effect_controls");
    if (invocation.ui_mouse_exited_mode && !registered_layer_ui) event_target = "comp";
    bool arbitrary_values_disposed = false;
    std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
    std::array<uintptr_t, 4> plugin_state_before_close{};
    bool lifecycle_context_stable = true;
    bool lifecycle_host_state_cleared = false;
    bool event_assignments_applied = !invocation.ui_event_assignment_mode;
    g_ui_context.window_type = std::strcmp(event_target, "layer") == 0 ? 1 :
        (std::strcmp(event_target, "comp") == 0 ? 0 : 2);
    aexcompat::worker_runtime::ui_event_execution::Result ui_result;
    const bool ui_dispatched = aexcompat::worker_runtime::ui_event_execution::dispatch(
        {entry, &input, &output, params_error, parameter_count_contract_valid,
         &invocation.ui_event_assignments, invocation.ui_event_assignment_mode, invocation.adjust_cursor_mode,
         invocation.draw_event_mode, invocation.click_event_mode, invocation.drag_event_mode, invocation.ui_lifecycle_mode,
         invocation.ui_idle_mode, invocation.ui_keydown_mode, invocation.ui_mouse_exited_mode, invocation.click_x, invocation.click_y,
         invocation.drag_end_x, invocation.drag_end_y, invocation.drag_steps, invocation.keydown_code, invocation.keydown_modifiers,
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
    const bool ui_event_passed = emit_ui_event_completion_report(
        {invocation.ui_lifecycle_mode, invocation.ui_idle_mode,
         invocation.ui_keydown_mode, invocation.ui_mouse_exited_mode,
         invocation.draw_event_mode, invocation.drag_event_mode,
         invocation.click_event_mode, invocation.drag_steps,
         invocation.keydown_code, invocation.keydown_modifiers, event_target,
         event_error, cursor, event_out_flags, changed_value, lifecycle_errors,
         plugin_state_before_close, lifecycle_context_stable,
         lifecycle_host_state_cleared, event_assignments_applied,
         arbitrary_values_disposed, defaults_disposed, drawbot_objects_empty(),
         event_sequence_setdown_error, event_setdown_error,
         requested_parameters_json(invocation.ui_event_assignments)});
    return session.finish(ui_event_passed ? 0 : 20);
  }
  if (is_rendering_worker() && invocation.request_mode &&
      (params_error != 0 || !parameter_count_contract_valid ||
                       !validate_requested_assignments(invocation.requested_parameters) ||
                       !validate_external_aux_parameters())) {
    dispose_arbitrary_defaults(entry, input, output);
    if (global_error == 0)
      invoke_global_setdown(entry, input.data(), output.data());
    return session.finish(3);
  }
  const auto early_mode = aexcompat::l2mode::select_early_mode(
      invocation.auto_dialog_mode, invocation.do_dialog_mode, invocation.external_dependencies_mode, invocation.params_only_mode);
  if (!is_rendering_worker() && early_mode != aexcompat::l2mode::EarlyMode::None) {
    EarlyModeBridge bridge{entry, &input, &output, &session, &about_message};
    const int early_result = aexcompat::l2mode::run_early_mode(
        {early_mode, &bridge, early_mode_hooks(), global_error, params_error,
         parameter_count_contract_valid,
         invocation.external_dependencies_mode ? argv[4] : nullptr});
    return session.finish(early_result);
  }
  if (is_render_worker() && invocation.audio_session_mode) {
    const aexcompat::worker_audio_session::AudioSessionGeometry geometry{
        invocation.audio_session_max_samples, invocation.audio_session_channels};
    const auto session_outcome = run_audio_render_session(
        entry, input, output, global_error, params_error,
        invocation.requested_parameters, geometry, invocation.external_time_scale);
    if (!session.prepare_protocol_report()) return session.finish(14);
    restore_native_stdout();
    emit_audio_session_report(global_error, params_error, session_outcome);
    if (session_outcome.protocol_violation)
      return session.finish(aexcompat::worker_audio_session::kExitProtocolViolation);
    if (session_outcome.invariant_failure)
      return session.finish(aexcompat::worker_audio_session::kExitInvariantFailure);
    return session.finish(session_outcome.clean ? 0 : 20);
  }
  if (is_render_worker() && invocation.audio_mode) {
    const AudioModeRequest audio_request{
        entry, &input, &output, global_error, params_error,
        invocation.external_audio_samples, &invocation.external_audio,
        &invocation.external_audio_output, &invocation.requested_parameters};
    const auto audio_outcome = run_audio_mode(audio_request);
    if (!session.prepare_protocol_report()) return session.finish(14);
    restore_native_stdout();
    emit_audio_render_report(audio_request, audio_outcome);
    return session.finish(audio_outcome.passed && audio_outcome.output_created ? 0 : 20);
  }
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  bool lifecycle_data_null = false;
  bool user_changed_ok = false;
  bool conditional_ui_ok = false;
  if (!is_rendering_worker()) {
    if (const auto lifecycle_exit = run_l2_parameter_lifecycle(
            entry, input, output, global_error, params_error, lifecycle_errors,
            lifecycle_data_null, user_changed_ok, conditional_ui_ok))
      return session.finish(*lifecycle_exit);
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
  bool session_protocol_violation = false;
  bool session_invariant_failure = false;
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
  int32_t smart_session_frames_attempted = 0;
  int32_t smart_session_sequence_setup_error = -1;
  int32_t smart_session_sequence_setdown_error = -1;
  int32_t smart_session_render_error = -1;
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
    const auto dispatch = aexcompat::worker_runtime::invocation::run_classic_final_dispatch(
        {entry, &input, &output, &invocation, argv, params_error,
         image_render_supported, depth_supported, smart_render_supported});
    if (dispatch.case_id_rejected) return session.finish(2);
    case_id = dispatch.case_id;
    input_hash = dispatch.input_hash;
    output_hash = dispatch.output_hash;
    guards_intact = dispatch.guards_intact;
    render_width = dispatch.render_width;
    render_height = dispatch.render_height;
    render_rowbytes = dispatch.render_rowbytes;
    thread_errors = dispatch.thread_errors;
    thread_hashes = dispatch.thread_hashes;
    thread_guards = dispatch.thread_guards;
    concurrent_render = dispatch.concurrent_render;
    persistent_sequence = dispatch.persistent_sequence;
    session_protocol_violation = dispatch.session_protocol_violation;
    session_invariant_failure = dispatch.session_invariant_failure;
    flattened_sequence = dispatch.flattened_sequence;
    copied_flattened_sequence = dispatch.copied_flattened_sequence;
    persistent_sequence_setup_error = dispatch.persistent_sequence_setup_error;
    persistent_sequence_setdown_error = dispatch.persistent_sequence_setdown_error;
    persistent_frame_errors = dispatch.persistent_frame_errors;
    persistent_frame_hashes = dispatch.persistent_frame_hashes;
    sequence_flatten_error = dispatch.sequence_flatten_error;
    sequence_resetup_error = dispatch.sequence_resetup_error;
    flattened_handle_replaced = dispatch.flattened_handle_replaced;
    resetup_handle_replaced = dispatch.resetup_handle_replaced;
    flattened_handle_host_disposed = dispatch.flattened_handle_host_disposed;
    get_flattened_sequence_data_error = dispatch.get_flattened_sequence_data_error;
    original_sequence_preserved = dispatch.original_sequence_preserved;
    render_error = dispatch.render_error;
  } else if (is_smart_worker()) {
    const auto dispatch = aexcompat::worker_runtime::invocation::run_smart_final_dispatch(
        {entry, &input, &output, &invocation, argv, params_error,
         image_render_supported, depth_supported, smart_render_supported});
    if (dispatch.case_id_rejected) return session.finish(2);
    case_id = dispatch.case_id;
    smart = dispatch.smart;
    lifetime_fault_observed = dispatch.lifetime_fault_observed;
    session_protocol_violation = dispatch.session_protocol_violation;
    session_invariant_failure = dispatch.session_invariant_failure;
    smart_session_frames_attempted = dispatch.session_frames_attempted;
    smart_session_sequence_setup_error = dispatch.session_sequence_setup_error;
    smart_session_sequence_setdown_error = dispatch.session_sequence_setdown_error;
    smart_session_render_error = dispatch.session_render_error;
  }
  if (is_render_worker()) drain_async_layer_requests();
  const bool arbitrary_defaults_disposed = dispose_arbitrary_defaults(entry, input, output);
  std::cerr << "stage:global_setdown_begin\n" << std::flush;
  const int32_t setdown_error = global_error == 0
      ? invoke_global_setdown(entry, input.data(), output.data()) : -1;
  // Smart-worker fault-injection probes as a handler table (issue #171):
  // each requested mode runs its verifier in the same order as before.
  const struct SmartFaultProbe {
    bool requested;
    bool (*verify)();
    bool* observed;
  } smart_fault_probes[] = {
      {invocation.suite_release_without_acquire_mode,
       &verify_suite_release_without_acquire_rejected, &suite_fault_observed},
      {invocation.handle_resize_while_locked_mode,
       &verify_handle_resize_while_locked_rejected, &handle_fault_observed},
      {invocation.world_double_dispose_mode,
       &verify_world_double_dispose_rejected, &world_fault_observed},
      {invocation.world_allocation_limit_mode,
       &verify_world_allocation_limit_rejected, &world_fault_observed},
      {invocation.pixel_format_registry_mode,
       &verify_pixel_format_registry_rejection, &pixel_format_fault_observed},
      {invocation.outline_mutation_mode,
       &verify_outline_mutation_rejection, &outline_fault_observed},
      {invocation.mask_attribute_mode,
       &verify_mask_attribute_and_ownership_rejection, &mask_attribute_fault_observed},
      {invocation.stream_metadata_ownership_mode,
       &verify_stream_metadata_and_ownership_rejection, &stream_metadata_fault_observed},
      {invocation.keyframe_ownership_mode,
       &verify_keyframe_ownership_rejection, &keyframe_fault_observed},
      {invocation.dynamic_stream_tree_mode,
       &verify_dynamic_stream_tree_rejection, &dynamic_stream_fault_observed},
      {invocation.aegp_memory_strings_mode,
       &verify_aegp_memory_and_strings_rejection, &aegp_memory_fault_observed},
  };
  if (is_smart_worker())
    for (const auto& probe : smart_fault_probes)
      if (probe.requested) *probe.observed = probe.verify();
  std::cerr << "stage:global_setdown_end error=" << setdown_error << "\n" << std::flush;
  if (!session.prepare_protocol_report()) return session.finish(14);
  if (is_render_worker()) {
  restore_native_stdout();
  ClassicCompletionInputs classic_inputs;
  classic_inputs.render_error = render_error;
  classic_inputs.global_error = global_error;
  classic_inputs.params_error = params_error;
  classic_inputs.setdown_error = setdown_error;
  classic_inputs.parameter_count_contract_valid = parameter_count_contract_valid;
  classic_inputs.guards_intact = guards_intact;
  classic_inputs.arbitrary_defaults_disposed = arbitrary_defaults_disposed;
  classic_inputs.depth_supported = depth_supported;
  classic_inputs.advertised_out_flags = advertised_out_flags;
  classic_inputs.advertised_out_flags2 = advertised_out_flags2;
  classic_inputs.image_render_supported = image_render_supported;
  classic_inputs.nop_render_advertised = nop_render_advertised;
  classic_inputs.input_write_advertised = input_write_advertised;
  classic_inputs.expand_buffer_advertised = expand_buffer_advertised;
  classic_inputs.shrink_buffer_advertised = shrink_buffer_advertised;
  classic_inputs.case_id = case_id;
  classic_inputs.input_hash = input_hash;
  classic_inputs.output_hash = output_hash;
  classic_inputs.render_width = render_width;
  classic_inputs.render_height = render_height;
  classic_inputs.render_rowbytes = render_rowbytes;
  classic_inputs.thread_errors = thread_errors;
  classic_inputs.thread_hashes = thread_hashes;
  classic_inputs.thread_guards = thread_guards;
  classic_inputs.concurrent_render = concurrent_render;
  classic_inputs.persistent_sequence = persistent_sequence;
  classic_inputs.persistent_sequence_setup_error = persistent_sequence_setup_error;
  classic_inputs.persistent_sequence_setdown_error = persistent_sequence_setdown_error;
  classic_inputs.persistent_frame_errors = persistent_frame_errors;
  classic_inputs.persistent_frame_hashes = persistent_frame_hashes;
  classic_inputs.flattened_sequence = flattened_sequence;
  classic_inputs.sequence_flatten_error = sequence_flatten_error;
  classic_inputs.sequence_resetup_error = sequence_resetup_error;
  classic_inputs.flattened_handle_replaced = flattened_handle_replaced;
  classic_inputs.resetup_handle_replaced = resetup_handle_replaced;
  classic_inputs.flattened_handle_host_disposed = flattened_handle_host_disposed;
  classic_inputs.copied_flattened_sequence = copied_flattened_sequence;
  classic_inputs.get_flattened_sequence_data_error = get_flattened_sequence_data_error;
  classic_inputs.original_sequence_preserved = original_sequence_preserved;
  classic_inputs.frame_return_message.assign(
      reinterpret_cast<const char*>(output.data() + kOutMessage),
      strnlen_s(reinterpret_cast<const char*>(output.data() + kOutMessage), 256));
  classic_inputs.request_mode = invocation.request_mode;
  classic_inputs.requested_parameters = &invocation.requested_parameters;
  classic_inputs.downsample_x = {static_cast<int32_t>(g_downsample_x.numerator),
                                 static_cast<int32_t>(g_downsample_x.denominator)};
  classic_inputs.downsample_y = {static_cast<int32_t>(g_downsample_y.numerator),
                                 static_cast<int32_t>(g_downsample_y.denominator)};
  classic_inputs.pixel_aspect_ratio = {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
                                       static_cast<int32_t>(g_pixel_aspect_ratio.denominator)};
  classic_inputs.resolution = {
      g_full_resolution_width > 0 ? g_full_resolution_width : invocation.external_width,
      g_full_resolution_height > 0 ? g_full_resolution_height : invocation.external_height};
  classic_inputs.context_head = {read<int32_t>(input, kInQuality), read<int32_t>(input, kInNumParams),
      read<int32_t>(input, kInLocalTimeStep), read<int32_t>(input, 244),
      read<int32_t>(input, 248), read<int32_t>(input, 400)};
  classic_inputs.context_zoom = {read<int32_t>(input, 252), read<int32_t>(input, 256)};
  classic_inputs.context_origin = {read<int32_t>(input, 392), read<int32_t>(input, 396)};
  classic_inputs.context_extent = {read<int32_t>(input, 276), read<int32_t>(input, 280)};
  emit_classic_completion_report(classic_inputs);
  } else if (is_smart_worker()) {
  restore_native_stdout();
  SmartCompletionInputs smart_inputs;
  smart_inputs.smart = &smart;
  smart_inputs.lifetime_fault_observed = lifetime_fault_observed;
  smart_inputs.suite_fault_observed = suite_fault_observed;
  smart_inputs.handle_fault_observed = handle_fault_observed;
  smart_inputs.world_fault_observed = world_fault_observed;
  smart_inputs.pixel_format_fault_observed = pixel_format_fault_observed;
  smart_inputs.outline_fault_observed = outline_fault_observed;
  smart_inputs.mask_attribute_fault_observed = mask_attribute_fault_observed;
  smart_inputs.stream_metadata_fault_observed = stream_metadata_fault_observed;
  smart_inputs.keyframe_fault_observed = keyframe_fault_observed;
  smart_inputs.dynamic_stream_fault_observed = dynamic_stream_fault_observed;
  smart_inputs.aegp_memory_fault_observed = aegp_memory_fault_observed;
  smart_inputs.global_error = global_error;
  smart_inputs.params_error = params_error;
  smart_inputs.setdown_error = setdown_error;
  smart_inputs.advertised_out_flags = advertised_out_flags;
  smart_inputs.advertised_out_flags2 = advertised_out_flags2;
  smart_inputs.parameter_count_contract_valid = parameter_count_contract_valid;
  smart_inputs.arbitrary_defaults_disposed = arbitrary_defaults_disposed;
  smart_inputs.depth_supported = depth_supported;
  smart_inputs.image_render_supported = image_render_supported;
  smart_inputs.smart_render_supported = smart_render_supported;
  smart_inputs.nop_render_advertised = nop_render_advertised;
  smart_inputs.input_write_advertised = input_write_advertised;
  smart_inputs.request_mode = invocation.request_mode;
  smart_inputs.session_mode = invocation.render_session_mode;
  smart_inputs.session_frames_attempted = smart_session_frames_attempted;
  smart_inputs.session_sequence_setup_error = smart_session_sequence_setup_error;
  smart_inputs.session_sequence_setdown_error = smart_session_sequence_setdown_error;
  smart_inputs.session_render_error = smart_session_render_error;
  smart_inputs.session_protocol_violation = session_protocol_violation;
  smart_inputs.session_invariant_failure = session_invariant_failure;
  smart_inputs.case_id = case_id;
  smart_inputs.external_size = {invocation.external_width, invocation.external_height};
  smart_inputs.requested_parameters = &invocation.requested_parameters;
  smart_inputs.downsample_x = {static_cast<int32_t>(g_downsample_x.numerator),
                               static_cast<int32_t>(g_downsample_x.denominator)};
  smart_inputs.downsample_y = {static_cast<int32_t>(g_downsample_y.numerator),
                               static_cast<int32_t>(g_downsample_y.denominator)};
  smart_inputs.pixel_aspect_ratio = {static_cast<int32_t>(g_pixel_aspect_ratio.numerator),
                                     static_cast<int32_t>(g_pixel_aspect_ratio.denominator)};
  smart_inputs.resolution = {
      g_full_resolution_width > 0 ? g_full_resolution_width : invocation.external_width,
      g_full_resolution_height > 0 ? g_full_resolution_height : invocation.external_height};
  smart_inputs.context_head = {read<int32_t>(input, kInQuality), read<int32_t>(input, kInNumParams),
      read<int32_t>(input, kInLocalTimeStep), read<int32_t>(input, 244),
      read<int32_t>(input, 248), read<int32_t>(input, 400)};
  smart_inputs.context_zoom = {read<int32_t>(input, 252), read<int32_t>(input, 256)};
  smart_inputs.context_origin = {read<int32_t>(input, 392), read<int32_t>(input, 396)};
  smart_inputs.context_extent = {read<int32_t>(input, 276), read<int32_t>(input, 280)};
  emit_smart_completion_report(smart_inputs);
  } else {
  report(global_error == 0 && params_error == 0 ? "selectors_completed" : "selector_error",
         global_error, params_error, setdown_error, output, about_message, lifecycle_errors, lifecycle_data_null);
  }
  if (is_render_worker()) {
    // Session fail-closed self-termination paths keep dedicated exit codes so
    // the broker can distinguish protocol violations (23) and host-protection
    // invariant failures (24) from ordinary render failures (21).
    if (session_protocol_violation) return session.finish(23);
    if (session_invariant_failure) return session.finish(24);
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
    // The smart session shares the render worker's dedicated fail-closed exit
    // codes: protocol violations (23) and host-protection invariant failures
    // (24) stay distinguishable from ordinary smart failures (22).
    if (session_protocol_violation) return session.finish(23);
    if (session_invariant_failure) return session.finish(24);
    if (invocation.render_session_mode) {
      // Frame-local selector errors were reported through frame_done and the
      // broker owned the continue decision; only session mechanics and host
      // state cleanliness decide the exit.
      return session.finish(global_error == 0 && params_error == 0 && parameter_count_contract_valid &&
        image_render_supported && depth_supported && smart_render_supported &&
        smart_session_render_error == 0 && smart.guards_intact &&
        handle_lifetimes_balanced() && world_lifetimes_balanced() &&
        gpu_memory_lifetimes_balanced() &&
        audio_handle_lifetimes_balanced() && audio_telemetry().invalid_operations == 0 &&
        param_checkouts_balanced() &&
        ((!g_render_click_enabled && !g_render_draw_enabled) ||
         g_render_ui_context_closed) ? 0 : 22);
    }
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
  // g_aegp_item_type_calls == item_type_calls_before + 1
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
       &aegp_dispose_stream_value_v2, &aegp_dispose_stream_v2,
       &aegp_get_layer_source_item, &aegp_get_item_type,
       &g_aegp_item_suite,
       &g_aegp_layer_source_item_calls, &g_aegp_item_type_calls,
       &aegp_get_effect_param_union_by_index_v3});
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
const bool g_host_guard_selftests_configured = [] {
  aexcompat::host_guard_selftests::configure(
      {&acquire_suite, &release_suite, &suite_acquire_count,
       &suite_release_count, &suite_leases_balanced});
  return true;
}();
const bool g_pf_color_selftests_configured = [] {
  aexcompat::pf_color_selftests::configure(
      {&acquire_suite, &release_suite, &g_effect, &g_color_param_suite1,
       &g_params,
       +[](void* effect_ref, const void* definition,
           aexcompat::pf_color_selftests::PixelFloat* output) -> int32_t {
         return floating_point_from_color(
             effect_ref, definition,
             reinterpret_cast<PfColorParamPixelFloat*>(output));
       }});
  return true;
}();
