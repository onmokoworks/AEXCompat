≠rá^—f•ñÿ¶{MÏy 'v√Æ∂õ≠#include <windows.h>
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
#include <cstdlib>
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
#include "aex_string_table.hpp"
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
constexpr uint32_t kOutFlagIUseAudio = 1u „}w÷⁄$z{-ÆÈ‹j◊ùFvS¶v∆ˆ&≈˜6WFF˜vÂˆVÊBW'&˜#“"√¬6WFF˜vÂˆW'&˜"√¬%∆‚"√¬7FC£¶f«W6É∞–¢ñbÇ6W76ñˆ‚Á&W&U˜&˜Fˆ6ˆ≈˜&W˜'BÇíí&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÉBì∞–¢ñbÜó5˜&VÊFW%˜v˜&∂W"Çíí∞–¢&W7F˜&UˆÊFófU˜7FF˜WBÇì∞–¢6∆76ñ46ˆ◊∆WFñˆ‰ñÁWG26∆76ñ5ˆñÁWG3∞–¢6∆76ñ5ˆñÁWG2Á&VÊFW%ˆW'&˜"“&VÊFW%ˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Êv∆ˆ&≈ˆW'&˜"“v∆ˆ&≈ˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Á&◊5ˆW'&˜"“&◊5ˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Á6WFF˜vÂˆW'&˜"“6WFF˜vÂˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Á&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñB“&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñC∞–¢6∆76ñ5ˆñÁWG2ÊwV&G5ˆñÁF7B“wV&G5ˆñÁF7C∞–¢6∆76ñ5ˆñÁWG2Ê&&óG&'ïˆFVfV«G5ˆFó7˜6VB“&&óG&'ïˆFVfV«G5ˆFó7˜6VC∞–¢6∆76ñ5ˆñÁWG2ÊFWFÖ˜7W˜'FVB“FWFÖ˜7W˜'FVC∞–¢6∆76ñ5ˆñÁWG2ÊGfW'Fó6VEˆ˜WEˆf∆w2“GfW'Fó6VEˆ˜WEˆf∆w3∞–¢6∆76ñ5ˆñÁWG2ÊGfW'Fó6VEˆ˜WEˆf∆w3"“GfW'Fó6VEˆ˜WEˆf∆w3#∞–¢6∆76ñ5ˆñÁWG2Êñ÷vU˜&VÊFW%˜7W˜'FVB“ñ÷vU˜&VÊFW%˜7W˜'FVC∞–¢6∆76ñ5ˆñÁWG2ÊÊ˜˜&VÊFW%ˆGfW'Fó6VB“Ê˜˜&VÊFW%ˆGfW'Fó6VC∞–¢6∆76ñ5ˆñÁWG2ÊñÁWE˜w&óFUˆGfW'Fó6VB“ñÁWE˜w&óFUˆGfW'Fó6VC∞–¢6∆76ñ5ˆñÁWG2ÊWáÊEˆ'VffW%ˆGfW'Fó6VB“WáÊEˆ'VffW%ˆGfW'Fó6VC∞–¢6∆76ñ5ˆñÁWG2Á6á&ñÊµˆ'VffW%ˆGfW'Fó6VB“6á&ñÊµˆ'VffW%ˆGfW'Fó6VC∞–¢6∆76ñ5ˆñÁWG2Ê66UˆñB“66UˆñC∞–¢6∆76ñ5ˆñÁWG2ÊñÁWEˆÜ6Ç“ñÁWEˆÜ6É∞–¢6∆76ñ5ˆñÁWG2Ê˜WGWEˆÜ6Ç“˜WGWEˆÜ6É∞–¢6∆76ñ5ˆñÁWG2Á&VÊFW%˜vñGFÇ“&VÊFW%˜vñGFÉ∞–¢6∆76ñ5ˆñÁWG2Á&VÊFW%ˆÜVñváB“&VÊFW%ˆÜVñváC∞–¢6∆76ñ5ˆñÁWG2Á&VÊFW%˜&˜v'óFW2“&VÊFW%˜&˜v'óFW3∞–¢6∆76ñ5ˆñÁWG2ÁFá&VEˆW'&˜'2“Fá&VEˆW'&˜'3∞–¢6∆76ñ5ˆñÁWG2ÁFá&VEˆÜ6ÜW2“Fá&VEˆÜ6ÜW3∞–¢6∆76ñ5ˆñÁWG2ÁFá&VEˆwV&G2“Fá&VEˆwV&G3∞–¢6∆76ñ5ˆñÁWG2Ê6ˆÊ7W'&VÁE˜&VÊFW"“6ˆÊ7W'&VÁE˜&VÊFW#∞–¢6∆76ñ5ˆñÁWG2ÁW'6ó7FVÁE˜6WVVÊ6R“W'6ó7FVÁE˜6WVVÊ6S∞–¢6∆76ñ5ˆñÁWG2ÁW'6ó7FVÁE˜6WVVÊ6U˜6WGWˆW'&˜"“W'6ó7FVÁE˜6WVVÊ6U˜6WGWˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2ÁW'6ó7FVÁE˜6WVVÊ6U˜6WFF˜vÂˆW'&˜"“W'6ó7FVÁE˜6WVVÊ6U˜6WFF˜vÂˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2ÁW'6ó7FVÁEˆg&÷UˆW'&˜'2“W'6ó7FVÁEˆg&÷UˆW'&˜'3∞–¢6∆76ñ5ˆñÁWG2ÁW'6ó7FVÁEˆg&÷UˆÜ6ÜW2“W'6ó7FVÁEˆg&÷UˆÜ6ÜW3∞–¢6∆76ñ5ˆñÁWG2Êf∆GFVÊVE˜6WVVÊ6R“f∆GFVÊVE˜6WVVÊ6S∞–¢6∆76ñ5ˆñÁWG2Á6WVVÊ6Uˆf∆GFVÂˆW'&˜"“6WVVÊ6Uˆf∆GFVÂˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Á6WVVÊ6U˜&W6WGWˆW'&˜"“6WVVÊ6U˜&W6WGWˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Êf∆GFVÊVEˆÜÊF∆U˜&W∆6VB“f∆GFVÊVEˆÜÊF∆U˜&W∆6VC∞–¢6∆76ñ5ˆñÁWG2Á&W6WGWˆÜÊF∆U˜&W∆6VB“&W6WGWˆÜÊF∆U˜&W∆6VC∞–¢6∆76ñ5ˆñÁWG2Êf∆GFVÊVEˆÜÊF∆UˆÜ˜7EˆFó7˜6VB“f∆GFVÊVEˆÜÊF∆UˆÜ˜7EˆFó7˜6VC∞–¢6∆76ñ5ˆñÁWG2Ê6˜ñVEˆf∆GFVÊVE˜6WVVÊ6R“6˜ñVEˆf∆GFVÊVE˜6WVVÊ6S∞–¢6∆76ñ5ˆñÁWG2ÊvWEˆf∆GFVÊVE˜6WVVÊ6UˆFFˆW'&˜"“vWEˆf∆GFVÊVE˜6WVVÊ6UˆFFˆW'&˜#∞–¢6∆76ñ5ˆñÁWG2Ê˜&ñvñÊ≈˜6WVVÊ6U˜&W6W'fVB“˜&ñvñÊ≈˜6WVVÊ6U˜&W6W'fVC∞–¢6∆76ñ5ˆñÁWG2Êg&÷U˜&WGW&Âˆ÷W76vRÊ76ñv‚Ä–¢&VñÁFW'&WEˆ67C∆6ˆÁ7B6Ü"£‚Ü˜WGWBÊFFÇí≤¥˜WD÷W76vRí¿–¢7G&Ê∆VÂ˜2á&VñÁFW'&WEˆ67C∆6ˆÁ7B6Ü"£‚Ü˜WGWBÊFFÇí≤¥˜WD÷W76vRí¬#Sbíì∞–¢6∆76ñ5ˆñÁWG2Á&WVW7Eˆ÷ˆFR“ñÁfˆ6Fñˆ‚Á&WVW7Eˆ÷ˆFS∞–¢6∆76ñ5ˆñÁWG2Á&WVW7FVE˜&÷WFW'2“fñÁfˆ6Fñˆ‚Á&WVW7FVE˜&÷WFW'3∞–¢6∆76ñ5ˆñÁWG2ÊF˜vÁ6◊∆U˜Ç“∑7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜ÇÊÁV÷W&F˜"í¿–¢7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜ÇÊFVÊˆ÷ñÊF˜"ó”∞–¢6∆76ñ5ˆñÁWG2ÊF˜vÁ6◊∆U˜í“∑7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜íÊÁV÷W&F˜"í¿–¢7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜íÊFVÊˆ÷ñÊF˜"ó”∞–¢6∆76ñ5ˆñÁWG2ÁóÜV≈ˆ7V7E˜&FñÚ“∑7FFñ5ˆ67C∆ñÁC3%˜C‚Üu˜óÜV≈ˆ7V7E˜&FñÚÊÁV÷W&F˜"í¿–¢7FFñ5ˆ67C∆ñÁC3%˜C‚Üu˜óÜV≈ˆ7V7E˜&FñÚÊFVÊˆ÷ñÊF˜"ó”∞–¢6∆76ñ5ˆñÁWG2Á&W6ˆ«WFñˆ‚“∞–¢uˆgV∆≈˜&W6ˆ«WFñˆÂ˜vñGFÇ‚ÚuˆgV∆≈˜&W6ˆ«WFñˆÂ˜vñGFÇ¢ñÁfˆ6Fñˆ‚ÊWáFW&Ê≈˜vñGFÇ¿–¢uˆgV∆≈˜&W6ˆ«WFñˆÂˆÜVñváB‚ÚuˆgV∆≈˜&W6ˆ«WFñˆÂˆÜVñváB¢ñÁfˆ6Fñˆ‚ÊWáFW&Ê≈ˆÜVñváG”∞–¢6∆76ñ5ˆñÁWG2Ê6ˆÁFWáEˆÜVB“∑&VC∆ñÁC3%˜C‚ÜñÁWB¬¥ñÂV∆óGíí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬¥ñ‰ÁV’&◊2í¿–¢&VC∆ñÁC3%˜C‚ÜñÁWB¬¥ñ‰∆ˆ6≈Fñ÷U7FWí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬#CBí¿–¢&VC∆ñÁC3%˜C‚ÜñÁWB¬#CÇí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬Có”∞–¢6∆76ñ5ˆñÁWG2Ê6ˆÁFWáE˜¶ˆˆ““∑&VC∆ñÁC3%˜C‚ÜñÁWB¬#S"í¬&VC∆ñÁC3%˜C‚ÜñÁWB¬#Sbó”∞–¢6∆76ñ5ˆñÁWG2Ê6ˆÁFWáEˆ˜&ñvñ‚“∑&VC∆ñÁC3%˜C‚ÜñÁWB¬3ì"í¬&VC∆ñÁC3%˜C‚ÜñÁWB¬3ìbó”∞–¢6∆76ñ5ˆñÁWG2Ê6ˆÁFWáEˆWáFVÁB“∑&VC∆ñÁC3%˜C‚ÜñÁWB¬#sbí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬#Éó”∞–¢V÷óEˆ6∆76ñ5ˆ6ˆ◊∆WFñˆÂ˜&W˜'BÜ6∆76ñ5ˆñÁWG2ì∞–¢“V«6RñbÜó5˜6÷'E˜v˜&∂W"Çíí∞–¢&W7F˜&UˆÊFófU˜7FF˜WBÇì∞–¢6÷'D6ˆ◊∆WFñˆ‰ñÁWG26÷'EˆñÁWG3∞–¢6÷'EˆñÁWG2Á6÷'B“g6÷'C∞–¢6÷'EˆñÁWG2Ê∆ñfWFñ÷UˆfV«Eˆˆ'6W'fVB“∆ñfWFñ÷UˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Á7VóFUˆfV«Eˆˆ'6W'fVB“7VóFUˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2ÊÜÊF∆UˆfV«Eˆˆ'6W'fVB“ÜÊF∆UˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Áv˜&∆EˆfV«Eˆˆ'6W'fVB“v˜&∆EˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2ÁóÜV≈ˆf˜&÷EˆfV«Eˆˆ'6W'fVB“óÜV≈ˆf˜&÷EˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Ê˜WF∆ñÊUˆfV«Eˆˆ'6W'fVB“˜WF∆ñÊUˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Ê÷6µˆGG&ñ'WFUˆfV«Eˆˆ'6W'fVB“÷6µˆGG&ñ'WFUˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Á7G&V’ˆ÷WFFFˆfV«Eˆˆ'6W'fVB“7G&V’ˆ÷WFFFˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Ê∂Wñg&÷UˆfV«Eˆˆ'6W'fVB“∂Wñg&÷UˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2ÊGñÊ÷ñ5˜7G&V’ˆfV«Eˆˆ'6W'fVB“GñÊ÷ñ5˜7G&V’ˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2ÊVwˆ÷V÷˜'ïˆfV«Eˆˆ'6W'fVB“Vwˆ÷V÷˜'ïˆfV«Eˆˆ'6W'fVC∞–¢6÷'EˆñÁWG2Êv∆ˆ&≈ˆW'&˜"“v∆ˆ&≈ˆW'&˜#∞–¢6÷'EˆñÁWG2Á&◊5ˆW'&˜"“&◊5ˆW'&˜#∞–¢6÷'EˆñÁWG2Á6WFF˜vÂˆW'&˜"“6WFF˜vÂˆW'&˜#∞–¢6÷'EˆñÁWG2ÊGfW'Fó6VEˆ˜WEˆf∆w2“GfW'Fó6VEˆ˜WEˆf∆w3∞–¢6÷'EˆñÁWG2ÊGfW'Fó6VEˆ˜WEˆf∆w3"“GfW'Fó6VEˆ˜WEˆf∆w3#∞–¢6÷'EˆñÁWG2Á&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñB“&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñC∞–¢6÷'EˆñÁWG2Ê&&óG&'ïˆFVfV«G5ˆFó7˜6VB“&&óG&'ïˆFVfV«G5ˆFó7˜6VC∞–¢6÷'EˆñÁWG2ÊFWFÖ˜7W˜'FVB“FWFÖ˜7W˜'FVC∞–¢6÷'EˆñÁWG2Êñ÷vU˜&VÊFW%˜7W˜'FVB“ñ÷vU˜&VÊFW%˜7W˜'FVC∞–¢6÷'EˆñÁWG2Á6÷'E˜&VÊFW%˜7W˜'FVB“6÷'E˜&VÊFW%˜7W˜'FVC∞–¢6÷'EˆñÁWG2ÊÊ˜˜&VÊFW%ˆGfW'Fó6VB“Ê˜˜&VÊFW%ˆGfW'Fó6VC∞–¢6÷'EˆñÁWG2ÊñÁWE˜w&óFUˆGfW'Fó6VB“ñÁWE˜w&óFUˆGfW'Fó6VC∞–¢6÷'EˆñÁWG2Á&WVW7Eˆ÷ˆFR“ñÁfˆ6Fñˆ‚Á&WVW7Eˆ÷ˆFS∞–¢6÷'EˆñÁWG2Á6W76ñˆÂˆ÷ˆFR“ñÁfˆ6Fñˆ‚Á&VÊFW%˜6W76ñˆÂˆ÷ˆFS∞–¢6÷'EˆñÁWG2Á6W76ñˆÂˆg&÷W5ˆGFV◊FVB“6÷'E˜6W76ñˆÂˆg&÷W5ˆGFV◊FVC∞–¢6÷'EˆñÁWG2Á6W76ñˆÂ˜6WVVÊ6U˜6WGWˆW'&˜"“6÷'E˜6W76ñˆÂ˜6WVVÊ6U˜6WGWˆW'&˜#∞–¢6÷'EˆñÁWG2Á6W76ñˆÂ˜6WVVÊ6U˜6WFF˜vÂˆW'&˜"“6÷'E˜6W76ñˆÂ˜6WVVÊ6U˜6WFF˜vÂˆW'&˜#∞–¢6÷'EˆñÁWG2Á6W76ñˆÂ˜&VÊFW%ˆW'&˜"“6÷'E˜6W76ñˆÂ˜&VÊFW%ˆW'&˜#∞–¢6÷'EˆñÁWG2Á6W76ñˆÂ˜&˜Fˆ6ˆ≈˜fñˆ∆Fñˆ‚“6W76ñˆÂ˜&˜Fˆ6ˆ≈˜fñˆ∆Fñˆ„∞–¢6÷'EˆñÁWG2Á6W76ñˆÂˆñÁf&ñÁEˆfñ«W&R“6W76ñˆÂˆñÁf&ñÁEˆfñ«W&S∞–¢6÷'EˆñÁWG2Ê66UˆñB“66UˆñC∞–¢6÷'EˆñÁWG2ÊWáFW&Ê≈˜6ó¶R“∂ñÁfˆ6Fñˆ‚ÊWáFW&Ê≈˜vñGFÇ¬ñÁfˆ6Fñˆ‚ÊWáFW&Ê≈ˆÜVñváG”∞–¢6÷'EˆñÁWG2Á&WVW7FVE˜&÷WFW'2“fñÁfˆ6Fñˆ‚Á&WVW7FVE˜&÷WFW'3∞–¢6÷'EˆñÁWG2ÊF˜vÁ6◊∆U˜Ç“∑7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜ÇÊÁV÷W&F˜"í¿–¢7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜ÇÊFVÊˆ÷ñÊF˜"ó”∞–¢6÷'EˆñÁWG2ÊF˜vÁ6◊∆U˜í“∑7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜íÊÁV÷W&F˜"í¿–¢7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆF˜vÁ6◊∆U˜íÊFVÊˆ÷ñÊF˜"ó”∞–¢6÷'EˆñÁWG2ÁóÜV≈ˆ7V7E˜&FñÚ“∑7FFñ5ˆ67C∆ñÁC3%˜C‚Üu˜óÜV≈ˆ7V7E˜&FñÚÊÁV÷W&F˜"í¿–¢7FFñ5ˆ67C∆ñÁC3%˜C‚Üu˜óÜV≈ˆ7V7E˜&FñÚÊFVÊˆ÷ñÊF˜"ó”∞–¢6÷'EˆñÁWG2Á&W6ˆ«WFñˆ‚“∞–¢uˆgV∆≈˜&W6ˆ«WFñˆÂ˜vñGFÇ‚ÚuˆgV∆≈˜&W6ˆ«WFñˆÂ˜vñGFÇ¢ñÁfˆ6Fñˆ‚ÊWáFW&Ê≈˜vñGFÇ¿–¢uˆgV∆≈˜&W6ˆ«WFñˆÂˆÜVñváB‚ÚuˆgV∆≈˜&W6ˆ«WFñˆÂˆÜVñváB¢ñÁfˆ6Fñˆ‚ÊWáFW&Ê≈ˆÜVñváG”∞–¢6÷'EˆñÁWG2Ê6ˆÁFWáEˆÜVB“∑&VC∆ñÁC3%˜C‚ÜñÁWB¬¥ñÂV∆óGíí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬¥ñ‰ÁV’&◊2í¿–¢&VC∆ñÁC3%˜C‚ÜñÁWB¬¥ñ‰∆ˆ6≈Fñ÷U7FWí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬#CBí¿–¢&VC∆ñÁC3%˜C‚ÜñÁWB¬#CÇí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬Có”∞–¢6÷'EˆñÁWG2Ê6ˆÁFWáE˜¶ˆˆ““∑&VC∆ñÁC3%˜C‚ÜñÁWB¬#S"í¬&VC∆ñÁC3%˜C‚ÜñÁWB¬#Sbó”∞–¢6÷'EˆñÁWG2Ê6ˆÁFWáEˆ˜&ñvñ‚“∑&VC∆ñÁC3%˜C‚ÜñÁWB¬3ì"í¬&VC∆ñÁC3%˜C‚ÜñÁWB¬3ìbó”∞–¢6÷'EˆñÁWG2Ê6ˆÁFWáEˆWáFVÁB“∑&VC∆ñÁC3%˜C‚ÜñÁWB¬#sbí¬&VC∆ñÁC3%˜C‚ÜñÁWB¬#Éó”∞–¢V÷óE˜6÷'Eˆ6ˆ◊∆WFñˆÂ˜&W˜'Bá6÷'EˆñÁWG2ì∞–¢“V«6R∞–¢&W˜'BÜv∆ˆ&≈ˆW'&˜"”“bb&◊5ˆW'&˜"”“Ú'6V∆V7F˜'5ˆ6ˆ◊∆WFVB"¢'6V∆V7F˜%ˆW'&˜""¿–¢v∆ˆ&≈ˆW'&˜"¬&◊5ˆW'&˜"¬6WFF˜vÂˆW'&˜"¬˜WGWB¬&˜WEˆ÷W76vR¬∆ñfV7ñ6∆UˆW'&˜'2¬∆ñfV7ñ6∆UˆFFˆÁV∆¬ì∞–¢––¢ñbÜó5˜&VÊFW%˜v˜&∂W"Çíí∞–¢ÚÚ6W76ñˆ‚fñ¬÷6∆˜6VB6V∆b◊FW&÷ñÊFñˆ‚Fá2∂VWFVFñ6FVBWÜóB6ˆFW26–¢ÚÚFÜR'&ˆ∂W"6‚Fó7FñÊwVó6Ç&˜Fˆ6ˆ¬fñˆ∆FñˆÁ2É#2íÊBÜ˜7B◊&˜FV7Fñˆ‡–¢ÚÚñÁf&ñÁBfñ«W&W2É#Bíg&ˆ“˜&FñÊ'í&VÊFW"fñ«W&W2É#í‡–¢ñbá6W76ñˆÂ˜&˜Fˆ6ˆ≈˜fñˆ∆Fñˆ‚í&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÉ#2ì∞–¢ñbá6W76ñˆÂˆñÁf&ñÁEˆfñ«W&Rí&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÉ#Bì∞–¢&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÜv∆ˆ&≈ˆW'&˜"”“bb&◊5ˆW'&˜"”“bb&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñBb`–¢ñ÷vU˜&VÊFW%˜7W˜'FVBbbFWFÖ˜7W˜'FVBbb&VÊFW%ˆW'&˜"”“bbwV&G5ˆñÁF7Bb`–¢ÜÊF∆Uˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíbbv˜&∆Eˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢7ñÊ5˜&V6VóEˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢7ñÊ5ˆ∆ñW%˜&WVW7G5ˆ&∆Ê6VBÇíb`–¢wUˆ÷V÷˜'ïˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢VFñıˆÜÊF∆Uˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíbbVFñı˜FV∆V÷WG'íÇíÊñÁf∆ñEˆ˜W&FñˆÁ2”“b`–¢&’ˆ6ÜV6∂˜WG5ˆ&∆Ê6VBÇíb`–¢ÇÇu˜&VÊFW%ˆ6∆ñ6µˆVÊ&∆VBbbu˜&VÊFW%ˆG&uˆVÊ&∆VBí«¿–¢u˜&VÊFW%˜Vïˆ6ˆÁFWáEˆ6∆˜6VBíÚ¢#ì∞–¢––¢ñbÜó5˜6÷'E˜v˜&∂W"Çíí∞–¢ÚÚFÜR6÷'B6W76ñˆ‚6Ü&W2FÜR&VÊFW"v˜&∂W"w2FVFñ6FVBfñ¬÷6∆˜6VBWÜó@–¢ÚÚ6ˆFW3¢&˜Fˆ6ˆ¬fñˆ∆FñˆÁ2É#2íÊBÜ˜7B◊&˜FV7Fñˆ‚ñÁf&ñÁBfñ«W&W0–¢ÚÚÉ#Bí7FíFó7FñÊwVó6Ü&∆Rg&ˆ“˜&FñÊ'í6÷'Bfñ«W&W2É#"í‡–¢ñbá6W76ñˆÂ˜&˜Fˆ6ˆ≈˜fñˆ∆Fñˆ‚í&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÉ#2ì∞–¢ñbá6W76ñˆÂˆñÁf&ñÁEˆfñ«W&Rí&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÉ#Bì∞–¢ñbÜñÁfˆ6Fñˆ‚Á&VÊFW%˜6W76ñˆÂˆ÷ˆFRí∞–¢ÚÚg&÷R÷∆ˆ6¬6V∆V7F˜"W'&˜'2vW&R&W˜'FVBFá&˜VvÇg&÷UˆFˆÊRÊBFÜP–¢ÚÚ'&ˆ∂W"˜vÊVBFÜR6ˆÁFñÁVRFV6ó6ñˆ„≤ˆÊ«í6W76ñˆ‚÷V6ÜÊñ72ÊBÜ˜7@–¢ÚÚ7FFR6∆VÊ∆ñÊW72FV6ñFRFÜRWÜóB‡–¢&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÜv∆ˆ&≈ˆW'&˜"”“bb&◊5ˆW'&˜"”“bb&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñBb`–¢ñ÷vU˜&VÊFW%˜7W˜'FVBbbFWFÖ˜7W˜'FVBbb6÷'E˜&VÊFW%˜7W˜'FVBb`–¢6÷'E˜6W76ñˆÂ˜&VÊFW%ˆW'&˜"”“bb6÷'BÊwV&G5ˆñÁF7Bb`–¢ÜÊF∆Uˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíbbv˜&∆Eˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢wUˆ÷V÷˜'ïˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢VFñıˆÜÊF∆Uˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíbbVFñı˜FV∆V÷WG'íÇíÊñÁf∆ñEˆ˜W&FñˆÁ2”“b`–¢&’ˆ6ÜV6∂˜WG5ˆ&∆Ê6VBÇíb`–¢ÇÇu˜&VÊFW%ˆ6∆ñ6µˆVÊ&∆VBbbu˜&VÊFW%ˆG&uˆVÊ&∆VBí«¿–¢u˜&VÊFW%˜Vïˆ6ˆÁFWáEˆ6∆˜6VBíÚ¢#"ì∞–¢––¢&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÜv∆ˆ&≈ˆW'&˜"”“bb&◊5ˆW'&˜"”“bb&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñBb`–¢ñ÷vU˜&VÊFW%˜7W˜'FVBbbFWFÖ˜7W˜'FVBbb6÷'BÁ&UˆW'&˜"”“bb6÷'BÁ&VÊFW%ˆW'&˜"”“b`–¢6÷'BÊwU˜6WGWˆW'&˜"”“bb6÷'BÊwU˜6WFF˜vÂˆW'&˜"”“b`–¢6÷'BÁ&V7G5˜f∆ñBbb6÷'BÊwV&G5ˆñÁF7Bb`–¢ÜÊF∆Uˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíbbv˜&∆Eˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢wUˆ÷V÷˜'ïˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíb`–¢VFñıˆÜÊF∆Uˆ∆ñfWFñ÷W5ˆ&∆Ê6VBÇíbbVFñı˜FV∆V÷WG'íÇíÊñÁf∆ñEˆ˜W&FñˆÁ2”“b`–¢&’ˆ6ÜV6∂˜WG5ˆ&∆Ê6VBÇíb`–¢ÇÇu˜&VÊFW%ˆ6∆ñ6µˆVÊ&∆VBbbu˜&VÊFW%ˆG&uˆVÊ&∆VBí«¿–¢u˜&VÊFW%˜Vïˆ6ˆÁFWáEˆ6∆˜6VBíÚ¢#"ì∞–¢––¢&WGW&‚6W76ñˆ‚ÊfñÊó6ÇÜv∆ˆ&≈ˆW'&˜"”“bb&◊5ˆW'&˜"”“bb&÷WFW%ˆ6˜VÁEˆ6ˆÁG&7E˜f∆ñBb`–¢W6W%ˆ6ÜÊvVEˆˆ≤bb6ˆÊFóFñˆÊ≈˜Vïˆˆ≤bb&˜WEˆW'&˜"”“bb∆ñfV7ñ6∆UˆFFˆÁV∆¬b`–¢7FC£¶∆≈ˆˆbÜ∆ñfV7ñ6∆UˆW'&˜'2Ê&Vvñ‚Çí¬∆ñfV7ñ6∆UˆW'&˜'2ÊVÊBÇí¬µ“ÜWFÚW'&˜"í≤&WGW&‚W'&˜"”“≤“íÚ¢#ì∞–ß––†–¶ñÁBWÜ6ˆ◊C£ßv˜&∂W%˜F&vWC£ß'V‚Ñ∂ñÊB∂ñÊB¬ñÁB&v2¬v6Ü%˜B¢¢&wbí∞–¢√%ˆFWFñ√£¶u˜v˜&∂W%˜F&vWB“∂ñÊC∞–¢&WGW&‚v˜&∂W%ˆ÷ñÂˆñ◊¬Ü&v2¬&wbì∞–ß––¶6ˆÁ7B&ˆˆ¬uˆVwˆ6ˆ◊E˜6V∆gFW7G5ˆ6ˆÊfñwW&VB“µ“∞–¢ÚÚuˆVwˆóFV’˜GóUˆ6∆«2”“óFV’˜GóUˆ6∆«5ˆ&Vf˜&R≤–¢WÜ6ˆ◊C£¶√%ˆFWFñ√£¶6ˆÊfñwW&UˆVwˆ6ˆ◊E˜6V∆gFW7G2Ä–¢≤f7Vó&U˜7VóFR¬g&V∆V6U˜7VóFR¿–¢uˆVwˆ6ˆ◊˜7VóFSÊFFÇí¬fu˜eˆñÁFW&f6U˜7VóFR¿–¢WÜ6ˆ◊C£ßeˆÜV«W#£ß7VóFSÇí¬fuˆVwˆ6ˆ◊¬fuˆVwˆ6ˆ◊ˆóFV“¬fuˆVffV7B¿–¢µµ“áfˆñB¢6ˆ◊¬WÜ6ˆ◊C£¶√%ˆFWFñ√£§Vw6ˆ◊D6ˆ∆˜"¢6ˆ∆˜"í∞–¢&WGW&‚VwˆvWEˆ6ˆ◊ˆ&uˆ6ˆ∆˜"Ü6ˆ◊¬&VñÁFW'&WEˆ67CƒVw6ˆ∆˜%f¬£‚Ü6ˆ∆˜"íì∞–¢“¿–¢f6ˆÁfW'EˆVffV7E˜Fıˆ6ˆ◊˜Fñ÷R¬fvWEˆVffV7Eˆ6÷W&¬fvWEˆVffV7Eˆ6÷W&ˆ÷G&óÇ¿–¢µµ“ÜñÁC3%˜BñÊFWÇí≤uˆVwˆ7FófUˆ6÷W&ˆ∆ñW%ˆñÊFWÇ“ñÊFWÉ≤“¿–¢µµ“≤&WGW&‚uˆVwˆ7FófUˆ6÷W&ˆ∆ñW%ˆñÊFWÉ≤“¿–¢µµ“ÜñÁC3%˜BñÊFWÇí”‚fˆñB¢∞–¢&WGW&‚ñÊFWÇ„“bbñÊFWÇ¬7FFñ5ˆ67C∆ñÁC3%˜C‚ÜuˆVwˆ∆ñW'2Á6ó¶RÇíê–¢ÚfuˆVwˆ∆ñW'5∑7FFñ5ˆ67C«7FC£ß6ó¶U˜C‚ÜñÊFWÇï“¢ÁV∆«G#∞–¢“¿–¢fVwˆ∆ñW%ˆñÊFWÇ¿–¢µµ“ÜñÁC3%˜BvñGFÇ¬ñÁC3%˜BÜVñváBí∞–¢uˆgV∆≈˜&W6ˆ«WFñˆÂ˜vñGFÇ“vñGFÉ≤uˆgV∆≈˜&W6ˆ«WFñˆÂˆÜVñváB“ÜVñváC∞–¢“¿–¢µµ“ÜñÁC3%˜B¢vñGFÇ¬ñÁC3%˜B¢ÜVñváBí∞–¢ñbávñGFÇíßvñGFÇ“uˆgV∆≈˜&W6ˆ«WFñˆÂ˜vñGFÉ∞–¢ñbÜÜVñváBí¶ÜVñváB“uˆgV∆≈˜&W6ˆ«WFñˆÂˆÜVñváC∞–¢“¿–¢g7VóFUˆ∆V6W5ˆ&∆Ê6VB¬fuˆ∆ñW"¬fuˆVwˆ6ˆ◊ˆñF∆U˜&˜VÊGG&óˆ÷ˆFR¿–¢fuˆ7FófU˜Vï˜&’ˆ6˜VÁB¬fVwˆvWEˆÊWuˆVffV7E˜7G&V’ˆ'ïˆñÊFWÖ˜c"¿–¢fVwˆvWE˜7G&V’ˆÊ÷U˜c"¬fVwˆvWE˜7G&V’˜GóU˜c"¿–¢fVwˆvWEˆÊWu˜7G&V’˜f«VU˜c"¬fVw˜6WE˜7G&V’˜f«VU˜c"¿–¢fVwˆFó7˜6U˜7G&V’˜f«VU˜c"¬fVwˆFó7˜6U˜7G&V’˜c"¿–¢fVwˆvWEˆ∆ñW%˜6˜W&6UˆóFV“¬fVwˆvWEˆóFV’˜GóR¿–¢fuˆVwˆóFV’˜7VóFR¿–¢fuˆVwˆ∆ñW%˜6˜W&6UˆóFV’ˆ6∆«2¬fuˆVwˆóFV’˜GóUˆ6∆«2¿–¢fVwˆvWEˆVffV7E˜&’˜VÊñˆÂˆ'ïˆñÊFWÖ˜c7“ì∞–¢&WGW&‚G'VS∞–ß“Çì∞–¶6ˆÁ7B&ˆˆ¬uˆ6ˆ∆˜%˜6WGFñÊw5˜6V∆gFW7G5ˆ6ˆÊfñwW&VB“µ“∞–¢WÜ6ˆ◊C£¶6ˆ∆˜%˜6WGFñÊw3£ß6V∆gFW7G3£¶6ˆÊfñwW&RÄ–¢≤f7Vó&U˜7VóFR¬g&V∆V6U˜7VóFR¬fuˆVwˆ6ˆ◊“ì∞–¢&WGW&‚G'VS∞–ß“Çì∞–¶6ˆÁ7B&ˆˆ¬u˜&÷WFW%˜6V∆gFW7G5ˆ6ˆÊfñwW&VB“µ“∞–¢WÜ6ˆ◊C£ß&÷WFW%˜6V∆gFW7G3£¶6ˆÊfñwW&Rá∞–¢f7Vó&U˜7VóFR¬g&V∆V6U˜7VóFR¬fuˆVffV7B¬fuˆ∆ñW"¿–¢fu˜&’˜WFñ«5˜7VóFS¬fu˜&’˜WFñ«5˜7VóFR¿–¢gWFFU˜&’˜Ví¬fó5ˆñFVÁFñ6≈˜&’ˆ6ÜV6∂˜WB¿–¢ffñÊE˜&’ˆ∂Wñg&÷U˜Fñ÷R¬fvWE˜&’ˆ∂Wñg&÷Uˆ6˜VÁB¿–¢f6ÜV6∂˜WE˜&’ˆ∂Wñg&÷R¬f6ÜV6∂ñÂ˜&’ˆ∂Wñg&÷R¿–¢g&’ˆ∂WïˆñÊFWÖ˜Fı˜Fñ÷R¬fvWEˆ7W'&VÁE˜&’˜7FFUˆˆ'6ˆ∆WFR¿–¢fÜ5˜&’ˆ6ÜÊvVEˆˆ'6ˆ∆WFR¿–¢fÜfUˆñÁWG5ˆ6ÜÊvVEˆ˜fW%˜Fñ÷U˜7Âˆˆ'6ˆ∆WFR¿–¢f«ï˜&÷WFW%ˆÊñ÷FñˆÁ“ì∞–¢&WGW&‚G'VS∞–ß“Çì∞–¶6ˆÁ7B&ˆˆ¬uˆÜ˜7EˆwV&E˜6V∆gFW7G5ˆ6ˆÊfñwW&VB“µ“∞–¢WÜ6ˆ◊C£¶Ü˜7EˆwV&E˜6V∆gFW7G3£¶6ˆÊfñwW&RÄ–¢≤f7Vó&U˜7VóFR¬g&V∆V6U˜7VóFR¬g7VóFUˆ7Vó&Uˆ6˜VÁB¿–¢g7VóFU˜&V∆V6Uˆ6˜VÁB¬g7VóFUˆ∆V6W5ˆ&∆Ê6VG“ì∞–¢&WGW&‚G'VS∞–ß“Çì∞–¶6ˆÁ7B&ˆˆ¬u˜eˆ6ˆ∆˜%˜6V∆gFW7G5ˆ6ˆÊfñwW&VB“µ“∞–¢WÜ6ˆ◊C£ßeˆ6ˆ∆˜%˜6V∆gFW7G3£¶6ˆÊfñwW&RÄ–¢≤f7Vó&U˜7VóFR¬g&V∆V6U˜7VóFR¬fuˆVffV7B¬fuˆ6ˆ∆˜%˜&’˜7VóFS¿–¢fu˜&◊2¿–¢µµ“áfˆñB¢VffV7E˜&Vb¬6ˆÁ7BfˆñB¢FVfñÊóFñˆ‚¿–¢WÜ6ˆ◊C£ßeˆ6ˆ∆˜%˜6V∆gFW7G3£•óÜVƒf∆ˆB¢˜WGWBí”‚ñÁC3%˜B∞–¢&WGW&‚f∆ˆFñÊu˜ˆñÁEˆg&ˆ’ˆ6ˆ∆˜"Ä–¢VffV7E˜&Vb¬FVfñÊóFñˆ‚¿–¢&VñÁFW'&WEˆ67C≈d6ˆ∆˜%&’óÜVƒf∆ˆB£‚Ü˜WGWBíì∞–¢◊“ì∞–¢&WGW&‚G'VS∞–ß“Çì∞–†