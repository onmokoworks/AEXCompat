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
bool teardown_bib_suite(void*) noexcept;
uint32_t bib_termination_attempt_count() noexcept;

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
using aexcompat::worker_runtime::ExtendedLookupOutcome;
using aexcompat::worker_runtime::ExtendedLookupOpaqueTableClassification;
using aexcompat::worker_runtime::ExtendedLookupStringTableState;
using aexcompat::worker_runtime::guarded_effect_call;
using aexcompat::worker_runtime::HostCallbackClassification;
using aexcompat::worker_runtime::invoke_entry_seh;
using aexcompat::worker_runtime::invoke_smart_pre_render_cleanup_seh;
using aexcompat::worker_runtime::module_audit_json;
using aexcompat::worker_runtime::observe_extended_allocation;
using aexcompat::worker_runtime::observe_extended_free;
using aexcompat::worker_runtime::record_extended_lookup_diagnostic;
using aexcompat::worker_runtime::record_host_callback_invocation;
using aexcompat::worker_runtime::module_audit_failure_json;
using aexcompat::worker_runtime::module_audit_passed;
using aexcompat::worker_runtime::module_audit_report;
using aexcompat::worker_runtime::selector_dispatch_telemetry;
using aexcompat::worker_runtime::selector_invocations_report_json;
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
constexpr std::size_t kMaxParams =
    aexcompat::worker_runtime::parameters::kMaxParameterCount;
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
enum class PiplPluginKind { Effect, Aegp, Unknown, Invalid, Missing };

const char* unresolved_entrypoint_plugin_kind(PiplPluginKind kind) {
  return kind == PiplPluginKind::Aegp
             ? "aegp_candidate"
             : (kind == PiplPluginKind::Invalid
                    ? "invalid_pipl"
                    : "unknown_no_effect_entrypoint");
}

std::string unresolved_entrypoint_report_json(PiplPluginKind kind) {
  return std::string("{\"plugin_kind\":\"") +
         unresolved_entrypoint_plugin_kind(kind) + "\"}";
}

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

bool terminal_zero_ae_reserved_property(const unsigned char* bytes,
                                        std::size_t size) {
  // Some shipping Boris Continuum PiPLs append the public AE_Reserved
  // (8BIM/aeRD) zero property without including it in PIPropertyList::count.
  // Accept only that exact, complete 20-byte record; all other trailing data
  // remains invalid and fail-closed.
  return bytes && size == 20 && pipl_tag(bytes, "MIB8") &&
      pipl_tag(bytes + 4, "DRea") && read_pipl_u32(bytes + 8) == 0 &&
      read_pipl_u32(bytes + 12) == 4 && read_pipl_u32(bytes + 16) == 0;
}

bool terminal_match_name_reserved_info_length_drift(
    const unsigned char* bytes, std::size_t size, uint32_t declared_length,
    uint32_t property_index, uint32_t property_count,
    uint32_t& corrected_length) {
  // Two shipping Boris obsolete stubs overstate the final eMNA data length by
  // exactly eight bytes, swallowing only the vendor/key of the following
  // public AE_Reserved_Info property. Recover solely when the entire remaining
  // structure is exact: padded Pascal match name, aeFL(value 8), then the
  // already-supported count-external aeRD(value 0). This is not a generic
  // Pascal-string or property-length repair.
  if (!bytes || size == 0 || property_index + 2 != property_count) return false;
  const uint32_t string_length = uint32_t(bytes[0]) + 1U;
  if (string_length > std::numeric_limits<uint32_t>::max() - 3U) return false;
  const uint32_t padded_length = (string_length + 3U) & ~3U;
  if (declared_length != padded_length + 8U ||
      size != std::size_t(padded_length) + 40U) return false;
  for (uint32_t index = string_length; index < padded_length; ++index)
    if (bytes[index] != 0) return false;
  const unsigned char* reserved_info = bytes + padded_length;
  if (!pipl_tag(reserved_info, "MIB8") ||
      !pipl_tag(reserved_info + 4, "LFea") ||
      read_pipl_u32(reserved_info + 8) != 0 ||
      read_pipl_u32(reserved_info + 12) != 4 ||
      read_pipl_u32(reserved_info + 16) != 8 ||
      !terminal_zero_ae_reserved_property(reserved_info + 20, 20))
    return false;
  corrected_length = padded_length;
  return true;
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
    uint32_t length = read_pipl_u32(property + 12);
    const bool adobe_vendor = pipl_tag(property, "MIB8");
    offset += 16;
    if (adobe_vendor && pipl_tag(property + 4, "ANMe")) {
      uint32_t corrected_length{};
      if (terminal_match_name_reserved_info_length_drift(
              bytes + offset, size - offset, length, index, count,
              corrected_length))
        length = corrected_length;
    }
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
  if (offset != size) {
    if (!terminal_zero_ae_reserved_property(bytes + offset, size - offset)) {
      result.kind = PiplPluginKind::Invalid;
      return result;
    }
    offset = size;
  }
  if (!saw_kind) {
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
  if (names.empty()) return {PiplPluginKind::Missing, {}};
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

// Adobe's PluginData ABI is intentionally reproduced as a small clean-room
// boundary here instead of including the redistributable SDK header.  The
// worker only needs the opaque pointer, callback layouts, and the documented
// entrypoint names to discover an Effect when no PiPL resource is present.
using PluginDataOpaque = void;
using PluginDataCallback2 = int32_t(__cdecl*)(
    PluginDataOpaque*, const unsigned char*, const unsigned char*,
    const unsigned char*, const unsigned char*, int32_t, int32_t, int32_t,
    int32_t, const unsigned char*);
using PluginDataCallback1 = int32_t(__cdecl*)(
    PluginDataOpaque*, const unsigned char*, const unsigned char*,
    const unsigned char*, const unsigned char*, int32_t, int32_t, int32_t,
    int32_t);
using PluginDataEntry2 = int32_t(__cdecl*)(
    PluginDataOpaque*, PluginDataCallback2, void*, const char*, const char*);
using PluginDataEntry1 = int32_t(__cdecl*)(
    PluginDataOpaque*, PluginDataCallback1, void*, const char*, const char*);

constexpr std::size_t kPluginDataNameBytes = 256;
constexpr std::size_t kPluginDataCategoryBytes = 256;
constexpr std::size_t kPluginDataEntryBytes = 128;
constexpr std::size_t kPluginDataSupportUrlBytes = 1024;
constexpr int32_t kPluginDataRejected = 3;  // A_Err_PARAMETER
constexpr int32_t kPluginDataException = 512;
constexpr int32_t kPluginDataReservedInfo = 8;
constexpr int32_t kPluginDataApiMajor = 13;
// Bundled AE 2025 effects register api 13.29 (observed across the whole
// bundled corpus via the #326 probe): Adobe ships them built against a newer
// internal SDK than the public 25.2 headers (13.28).
constexpr int32_t kPluginDataApiMinor = 29;

template <std::size_t Capacity>
struct BoundedPluginDataText {
  std::array<char, Capacity + 1> text{};
  std::size_t length{};
  bool readable{};
  bool terminated{};
  bool safe_bytes{};
};

template <std::size_t Capacity>
BoundedPluginDataText<Capacity> copy_bounded_plugin_data_text(
    const unsigned char* source) noexcept {
  BoundedPluginDataText<Capacity> copy;
  copy.safe_bytes = true;
  if (!source) return copy;
  __try {
    for (; copy.length < Capacity; ++copy.length) {
      const unsigned char value = source[copy.length];
      if (value == '\0') {
        copy.terminated = true;
        break;
      }
      // Metadata is an opaque byte string in the PluginData ABI.  Preserve
      // bounded/NUL-terminated copying and reject C0 controls and DEL, but do
      // not reject valid non-ASCII localized names, categories, or URLs.
      if (value < 0x20 || value == 0x7f) copy.safe_bytes = false;
      copy.text[copy.length] = static_cast<char>(value);
    }
    copy.readable = true;
  } __except (EXCEPTION_EXECUTE_HANDLER) {
    copy.readable = false;
  }
  copy.text[copy.length] = '\0';
  return copy;
}

struct PluginDataRegistration {
  std::array<char, kPluginDataNameBytes + 1> name{};
  std::array<char, kPluginDataNameBytes + 1> match_name{};
  std::array<char, kPluginDataCategoryBytes + 1> category{};
  std::array<char, kPluginDataEntryBytes + 1> entrypoint{};
  std::array<char, kPluginDataSupportUrlBytes + 1> support_url{};
  std::size_t name_length{};
  std::size_t match_name_length{};
  std::size_t category_length{};
  std::size_t entrypoint_length{};
  std::size_t support_url_length{};
  int32_t kind{};
  int32_t api_major{};
  int32_t api_minor{};
  int32_t reserved_info{};
  bool support_url_present{};
  bool valid{};
};

struct PluginDataContext {
  uint32_t callback_count{};
  uint32_t exception_code{};
  bool invalid{};
  PluginDataRegistration registration{};
};

bool valid_plugin_data_export_name(const BoundedPluginDataText<kPluginDataEntryBytes>& text) {
  if (!text.readable || !text.terminated || !text.safe_bytes || text.length == 0 ||
      text.length > 127) return false;
  if (!(text.text[0] == '_' || (text.text[0] >= 'A' && text.text[0] <= 'Z') ||
        (text.text[0] >= 'a' && text.text[0] <= 'z')))
    return false;
  for (std::size_t index = 1; index < text.length; ++index) {
    const char value = text.text[index];
    if (!(value == '_' || (value >= 'A' && value <= 'Z') ||
          (value >= 'a' && value <= 'z') || (value >= '0' && value <= '9')))
      return false;
  }
  return true;
}

bool plugin_data_effect_kind(int32_t kind) {
  // Use the same compiler literal as the SDK's PF_REGISTER_EFFECT_EXT2 macro.
  return kind == static_cast<int32_t>('eFKT');
}

template <std::size_t Capacity>
bool valid_plugin_data_text(const BoundedPluginDataText<Capacity>& text,
                            bool required) {
  return text.readable && text.terminated && text.safe_bytes &&
      (!required || text.length != 0);
}

int32_t record_plugin_data_registration(
    PluginDataContext* context, const unsigned char* name,
    const unsigned char* match_name, const unsigned char* category,
    const unsigned char* entrypoint, int32_t kind, int32_t api_major,
    int32_t api_minor, int32_t reserved_info,
    const unsigned char* support_url) noexcept {
  if (!context) return kPluginDataRejected;
  // Multi-effect bundles legitimately register more than once (issue #326):
  // keep the first registration and accept the rest.
  if (context->callback_count != 0) {
    ++context->callback_count;
    return 0;
  }
  ++context->callback_count;
  const auto name_copy = copy_bounded_plugin_data_text<kPluginDataNameBytes>(name);
  const auto match_copy =
      copy_bounded_plugin_data_text<kPluginDataNameBytes>(match_name);
  const auto category_copy =
      copy_bounded_plugin_data_text<kPluginDataCategoryBytes>(category);
  const auto entry_copy =
      copy_bounded_plugin_data_text<kPluginDataEntryBytes>(entrypoint);
  const auto support_copy =
      copy_bounded_plugin_data_text<kPluginDataSupportUrlBytes>(support_url);
  const bool support_valid = !support_url ||
      valid_plugin_data_text(support_copy, false);
  if (!valid_plugin_data_text(name_copy, true) ||
      !valid_plugin_data_text(match_copy, true) ||
      !valid_plugin_data_text(category_copy, true) ||
      !valid_plugin_data_export_name(entry_copy) || !support_valid ||
      !plugin_data_effect_kind(kind) || api_major <= 0 ||
      api_major > kPluginDataApiMajor || api_minor < 0 ||
      (api_major == kPluginDataApiMajor && api_minor > kPluginDataApiMinor)) {
    // reserved_info is deliberately not validated: in the wild it is a
    // plugin-defined opaque value (0/1/8/9 observed across the bundled
    // corpus), not the SDK sample's AE_RESERVED_INFO constant (#326).
    context->invalid = true;
    return kPluginDataRejected;
  }
  auto& registration = context->registration;
  std::copy_n(name_copy.text.data(), name_copy.length + 1,
              registration.name.data());
  std::copy_n(match_copy.text.data(), match_copy.length + 1,
              registration.match_name.data());
  std::copy_n(category_copy.text.data(), category_copy.length + 1,
              registration.category.data());
  std::copy_n(entry_copy.text.data(), entry_copy.length + 1,
              registration.entrypoint.data());
  if (support_url) {
    std::copy_n(support_copy.text.data(), support_copy.length + 1,
                registration.support_url.data());
    registration.support_url_present = true;
  }
  registration.name_length = name_copy.length;
  registration.match_name_length = match_copy.length;
  registration.category_length = category_copy.length;
  registration.entrypoint_length = entry_copy.length;
  registration.support_url_length = support_copy.length;
  registration.kind = kind;
  registration.api_major = api_major;
  registration.api_minor = api_minor;
  registration.reserved_info = reserved_info;
  registration.valid = true;
  return 0;
}

int32_t __cdecl plugin_data_callback2(
    PluginDataOpaque* in_ptr, const unsigned char* name,
    const unsigned char* match_name, const unsigned char* category,
    const unsigned char* entrypoint, int32_t kind, int32_t api_major,
    int32_t api_minor, int32_t reserved_info,
    const unsigned char* support_url) noexcept {
  return record_plugin_data_registration(
      static_cast<PluginDataContext*>(in_ptr), name, match_name, category,
      entrypoint, kind, api_major, api_minor, reserved_info, support_url);
}

int32_t __cdecl plugin_data_callback1(
    PluginDataOpaque* in_ptr, const unsigned char* name,
    const unsigned char* match_name, const unsigned char* category,
    const unsigned char* entrypoint, int32_t kind, int32_t api_major,
    int32_t api_minor, int32_t reserved_info) noexcept {
  return record_plugin_data_registration(
      static_cast<PluginDataContext*>(in_ptr), name, match_name, category,
      entrypoint, kind, api_major, api_minor, reserved_info, nullptr);
}

int plugin_data_exception_filter(EXCEPTION_POINTERS* information,
                                 uint32_t* exception_code) {
  if (exception_code && information && information->ExceptionRecord)
    *exception_code = information->ExceptionRecord->ExceptionCode;
  return EXCEPTION_EXECUTE_HANDLER;
}

int32_t invoke_plugin_data_entry2_seh(PluginDataEntry2 entry,
                                      PluginDataContext* context) {
  if (!entry || !context) return kPluginDataRejected;
  int32_t result = kPluginDataRejected;
  __try {
    result = entry(context, &plugin_data_callback2, &g_basic_suite,
                   "AEXCompat", "2025");
  } __except(plugin_data_exception_filter(GetExceptionInformation(),
                                           &context->exception_code)) {
    result = kPluginDataException;
  }
  return result;
}

int32_t invoke_plugin_data_entry1_seh(PluginDataEntry1 entry,
                                      PluginDataContext* context) {
  if (!entry || !context) return kPluginDataRejected;
  int32_t result = kPluginDataRejected;
  __try {
    result = entry(context, &plugin_data_callback1, &g_basic_suite,
                   "AEXCompat", "2025");
  } __except(plugin_data_exception_filter(GetExceptionInformation(),
                                           &context->exception_code)) {
    result = kPluginDataException;
  }
  return result;
}

PiplEntrypoint resolve_plugin_data_entrypoints(PluginDataEntry2 entry2,
                                               PluginDataEntry1 entry1) {
  if (!entry2 && !entry1) return {PiplPluginKind::Unknown, {}};
  PluginDataContext context;
  const int32_t result = entry2
      ? invoke_plugin_data_entry2_seh(entry2, &context)
      : invoke_plugin_data_entry1_seh(entry1, &context);
  if (result != 0 || context.exception_code != 0 || context.invalid ||
      context.callback_count == 0 || !context.registration.valid)
    return {PiplPluginKind::Unknown, {}};
  return {PiplPluginKind::Effect,
          std::string(context.registration.entrypoint.data(),
                      context.registration.entrypoint_length)};
}

PiplEntrypoint discover_plugin_data_entrypoint(HMODULE module) {
  if (!module) return {PiplPluginKind::Unknown, {}};
  // PluginDataEntryFunction3 is the entry the AE 2026 bundle ships (VR*,
  // Fast_Blur, Sharpen; issue #326). It is absent from the public 25.2 header,
  // but a live probe (tools/aex_plugindata_probe.py) confirmed it takes the
  // same (opaque, callback, SPBasic, host, version) entry signature and drives
  // the v2 registration callback with identical argument layout - name,
  // match_name, category, entrypoint, kind, api, reserved, support_url all
  // arrive intact - so it resolves through the same v2 path. Preferred over
  // v2 over v1. That callback layout is confirmed by the live probe across the
  // whole AE2026 bundle; were some other build's v3 to drive a wider or
  // narrower callback, record_plugin_data_registration bounds-checks every
  // field and returns Unknown, so the worst case stays fail-closed (never a
  // false Effect verdict, never a crash past the v2 SEH guard below).
  const auto entry3 = reinterpret_cast<PluginDataEntry2>(
      GetProcAddress(module, "PluginDataEntryFunction3"));
  const auto entry2 = entry3 ? entry3 : reinterpret_cast<PluginDataEntry2>(
      GetProcAddress(module, "PluginDataEntryFunction2"));
  const auto entry1 = reinterpret_cast<PluginDataEntry1>(
      GetProcAddress(module, "PluginDataEntryFunction"));
  return resolve_plugin_data_entrypoints(entry2, entry1);
}

int32_t __cdecl synthetic_plugin_data_entry2(
    PluginDataOpaque* in_ptr, PluginDataCallback2 callback, void* basic_suite,
    const char* host, const char* version) {
  if (!callback || !basic_suite || !host || !version ||
      std::strcmp(host, "AEXCompat") != 0 || std::strcmp(version, "2025") != 0)
    return kPluginDataRejected;
  return callback(in_ptr,
      reinterpret_cast<const unsigned char*>("Synthetic Effect"),
      reinterpret_cast<const unsigned char*>("AEXCompat Synthetic"),
      reinterpret_cast<const unsigned char*>("AEXCompat Tests"),
      reinterpret_cast<const unsigned char*>("entryPointFunc"),
      static_cast<int32_t>('eFKT'), kPluginDataApiMajor, kPluginDataApiMinor,
      kPluginDataReservedInfo,
      reinterpret_cast<const unsigned char*>("https://example.invalid"));
}

// A multi-effect v2/v3 bundle (the Fast_Blur shape, issue #326): two
// registrations in one entry, the first the effect the module is discovered
// as. This pins the shared resolution path that a v3 bundle flows through
// (multi-registration → first entrypoint); the v3 export lookup itself is a
// GetProcAddress name a unit test cannot reach, so it is covered by the
// render sweep, not here.
int32_t __cdecl synthetic_plugin_data_entry_multi(
    PluginDataOpaque* in_ptr, PluginDataCallback2 callback, void* basic_suite,
    const char* host, const char* version) {
  if (!callback || !basic_suite || !host || !version ||
      std::strcmp(host, "AEXCompat") != 0 || std::strcmp(version, "2025") != 0)
    return kPluginDataRejected;
  const int32_t first = callback(in_ptr,
      reinterpret_cast<const unsigned char*>("Primary Effect"),
      reinterpret_cast<const unsigned char*>("AEXCompat Primary"),
      reinterpret_cast<const unsigned char*>("AEXCompat Tests"),
      reinterpret_cast<const unsigned char*>("EffectMainExtra"),
      static_cast<int32_t>('eFKT'), kPluginDataApiMajor, kPluginDataApiMinor,
      kPluginDataReservedInfo,
      reinterpret_cast<const unsigned char*>("https://example.invalid"));
  if (first != 0) return first;
  return callback(in_ptr,
      reinterpret_cast<const unsigned char*>("Secondary Effect"),
      reinterpret_cast<const unsigned char*>("AEXCompat Secondary"),
      reinterpret_cast<const unsigned char*>("AEXCompat Tests"),
      reinterpret_cast<const unsigned char*>("EffectMainExtra2"),
      static_cast<int32_t>('eFKT'), kPluginDataApiMajor, kPluginDataApiMinor,
      kPluginDataReservedInfo,
      reinterpret_cast<const unsigned char*>("https://example.invalid"));
}

int32_t __cdecl synthetic_plugin_data_entry1(
    PluginDataOpaque* in_ptr, PluginDataCallback1 callback, void* basic_suite,
    const char* host, const char* version) {
  if (!callback || !basic_suite || !host || !version ||
      std::strcmp(host, "AEXCompat") != 0 || std::strcmp(version, "2025") != 0)
    return kPluginDataRejected;
  return callback(in_ptr,
      reinterpret_cast<const unsigned char*>("Synthetic v1 Effect"),
      reinterpret_cast<const unsigned char*>("AEXCompat Synthetic v1"),
      reinterpret_cast<const unsigned char*>("AEXCompat Tests"),
      reinterpret_cast<const unsigned char*>("EffectMain"),
      static_cast<int32_t>('eFKT'), kPluginDataApiMajor, kPluginDataApiMinor,
      kPluginDataReservedInfo);
}

bool verify_plugin_data_entrypoint() {
  const auto v2 = resolve_plugin_data_entrypoints(&synthetic_plugin_data_entry2,
                                                  &synthetic_plugin_data_entry1);
  if (v2.kind != PiplPluginKind::Effect || v2.symbol != "entryPointFunc")
    return false;
  const auto v1 = resolve_plugin_data_entrypoints(nullptr,
                                                  &synthetic_plugin_data_entry1);
  if (v1.kind != PiplPluginKind::Effect || v1.symbol != "EffectMain")
    return false;
  // A multi-effect bundle (Fast_Blur shape) resolves to its first registered
  // entrypoint; PluginDataEntryFunction3 flows through this same v2 path.
  const auto multi = resolve_plugin_data_entrypoints(
      &synthetic_plugin_data_entry_multi, nullptr);
  if (multi.kind != PiplPluginKind::Effect || multi.symbol != "EffectMainExtra")
    return false;
  PluginDataContext duplicate;
  // Duplicate registrations are accepted and ignored: both calls must succeed
  // and the first registration must win (issue #326).
  if (plugin_data_callback1(
          &duplicate, reinterpret_cast<const unsigned char*>("Name"),
          reinterpret_cast<const unsigned char*>("Match"),
          reinterpret_cast<const unsigned char*>("Category"),
          reinterpret_cast<const unsigned char*>("EffectMain"),
          static_cast<int32_t>('eFKT'), kPluginDataApiMajor,
          kPluginDataApiMinor, kPluginDataReservedInfo) != 0 ||
      plugin_data_callback1(
          &duplicate, reinterpret_cast<const unsigned char*>("Name2"),
          reinterpret_cast<const unsigned char*>("Match2"),
          reinterpret_cast<const unsigned char*>("Category2"),
          reinterpret_cast<const unsigned char*>("EffectMain2"),
          static_cast<int32_t>('eFKT'), kPluginDataApiMajor,
          kPluginDataApiMinor, kPluginDataReservedInfo) != 0 ||
      duplicate.callback_count != 2 ||
      std::string(duplicate.registration.name.data(),
                  duplicate.registration.name_length) != "Name")
    return false;
  PluginDataContext localized;
  const unsigned char localized_name[] = {0xe3, 0x83, 0x86, 0x00};
  const unsigned char localized_match[] = {0xe3, 0x82, 0xb9, 0x00};
  const unsigned char localized_category[] = {0xe3, 0x83, 0x88, 0x00};
  const unsigned char localized_url[] = {0x68, 0x74, 0x74, 0x70, 0x73,
                                          0x3a, 0x2f, 0x2f, 0xe3, 0x00};
  if (plugin_data_callback2(
          &localized, localized_name, localized_match, localized_category,
          reinterpret_cast<const unsigned char*>("EffectMain"),
          static_cast<int32_t>('eFKT'), kPluginDataApiMajor,
          kPluginDataApiMinor, kPluginDataReservedInfo, localized_url) != 0 ||
      !localized.registration.valid || !localized.registration.support_url_present)
    return false;
  PluginDataContext invalid_pointer;
  return plugin_data_callback1(
      &invalid_pointer, reinterpret_cast<const unsigned char*>(1),
      reinterpret_cast<const unsigned char*>("Match"),
      reinterpret_cast<const unsigned char*>("Category"),
      reinterpret_cast<const unsigned char*>("EffectMain"),
      static_cast<int32_t>('eFKT'), kPluginDataApiMajor,
      kPluginDataApiMinor, kPluginDataReservedInfo) != 0;
}

// Extended inter slots beyond the public SDK's PF_InteractCallbacks (issue
// #382). The real AE host's callback table is larger than the public headers
// show; bundled effects call in_data+0x60 first thing in GLOBAL_SETUP
// (allocating 0xFA0 bytes) and release it through in_data+0x70, while
// PARAMS_SETUP resolves parameter-name strings through in_data+0x68.
// Diagnostics (worker_extended_diag.hpp): defined near
// make_bootstrap_abi_hooks; dumps host callback addresses once so external
// debugger breakpoints can trace a plug-in's call path.
void diag_dump_callback_addresses();

int32_t __cdecl host_extended_alloc(void** out, std::size_t size) {
  if (extended_diag_enabled()) {
    diag_dump_callback_addresses();
    std::cerr << "extended_diag:alloc out=" << static_cast<const void*>(out)
              << " size=0x" << std::hex << size << std::dec << "\n"
              << std::flush;
  }
  if (!out || size == 0 || size > (size_t{1} << 24)) {
    record_host_callback_invocation(
        "inter.extended_alloc", 4,
        HostCallbackClassification::implemented);
    return 4;
  }
  void* buffer = std::calloc(1, size);
  if (!buffer) {
    record_host_callback_invocation(
        "inter.extended_alloc", 4,
        HostCallbackClassification::implemented);
    return 4;
  }
  *out = buffer;
  observe_extended_allocation(buffer);
  record_host_callback_invocation(
      "inter.extended_alloc", 0,
      HostCallbackClassification::implemented);
  return 0;
}
int32_t __cdecl host_extended_free(void** ptr) {
  // The plug-in passes the address of its buffer pointer (lea rcx,[local]),
  // not the buffer itself.
  if (ptr) {
    observe_extended_free(*ptr);
    std::free(*ptr);
  }
  record_host_callback_invocation(
      "inter.extended_free", 0,
      HostCallbackClassification::implemented);
  return 0;
}

constexpr std::uintmax_t kMaxAexStringTableFileBytes = 256u * 1024u * 1024u;

bool load_aex_string_table(
    HMODULE module, aexcompat::aex_strings::StringTable& table) {
  table = {};
  wchar_t module_path[32768]{};
  const DWORD length = GetModuleFileNameW(
      module, module_path, static_cast<DWORD>(std::size(module_path)));
  if (length == 0 || length >= std::size(module_path)) {
    table.status = aexcompat::aex_strings::ParseStatus::Invalid;
    return false;
  }
  std::ifstream input(std::filesystem::path(module_path), std::ios::binary);
  if (!input) {
    table.status = aexcompat::aex_strings::ParseStatus::Invalid;
    return false;
  }
  input.seekg(0, std::ios::end);
  const std::streamoff end = input.tellg();
  if (end <= 0 || static_cast<std::uintmax_t>(end) >
                       kMaxAexStringTableFileBytes) {
    table.status = aexcompat::aex_strings::ParseStatus::Invalid;
    return false;
  }
  input.seekg(0, std::ios::beg);
  std::vector<unsigned char> bytes(static_cast<std::size_t>(end));
  input.read(reinterpret_cast<char*>(bytes.data()),
             static_cast<std::streamsize>(bytes.size()));
  if (!input) {
    table.status = aexcompat::aex_strings::ParseStatus::Invalid;
    return false;
  }
  table = aexcompat::aex_strings::parse_readonly_pe_strings(
      bytes.data(), bytes.size());
  return table.status != aexcompat::aex_strings::ParseStatus::Invalid;
}

ExtendedLookupStringTableState extended_lookup_table_state(
    aexcompat::aex_strings::ParseStatus status) noexcept {
  return status == aexcompat::aex_strings::ParseStatus::Valid
             ? ExtendedLookupStringTableState::valid
         : status == aexcompat::aex_strings::ParseStatus::NoEntries
             ? ExtendedLookupStringTableState::none
             : ExtendedLookupStringTableState::invalid;
}

ExtendedLookupOpaqueTableClassification classify_other_lookup_module(
    void* module) noexcept {
  switch (aexcompat::worker_runtime::classify_loaded_module_provenance(
      module)) {
    case aexcompat::worker_runtime::LoadedModuleProvenance::sealed:
      return ExtendedLookupOpaqueTableClassification::
          other_loaded_sealed_module;
    case aexcompat::worker_runtime::LoadedModuleProvenance::system:
      return ExtendedLookupOpaqueTableClassification::
          other_loaded_system_module;
    case aexcompat::worker_runtime::LoadedModuleProvenance::unrecognized:
      return ExtendedLookupOpaqueTableClassification::unrecognized;
  }
  return ExtendedLookupOpaqueTableClassification::unrecognized;
}

const char* __cdecl host_extended_lookup(void* table, int32_t id, void* arg2,
                                         void* arg3) {
  // `table` is opaque and is never dereferenced. The diagnostics use only
  // VirtualQuery/GetModuleHandleEx containment plus authenticated module
  // provenance; resource ownership and lookup behavior remain unchanged.
  const ExtendedLookupOpaqueTableClassification table_classification =
      aexcompat::worker_runtime::classify_extended_lookup_table(
          table, aexcompat::worker_runtime::active_plugin::effect_module, nullptr,
          &classify_other_lookup_module);
  const auto* active_string_table =
      aexcompat::worker_runtime::active_plugin::string_table;
  const char* result = active_string_table
      ? active_string_table->lookup(id)
      : nullptr;
  // LoadString semantics for a valid table (issue #362): bundled effects
  // probe optional string ids and use the result unchecked (Three-Way Color
  // Corrector strlens the answer for the absent id 610), so a missing id in
  // a valid table yields an empty string, never null. An absent/invalid
  // table stays fail-closed and keeps returning null.
  if (!result && active_string_table &&
      active_string_table->status ==
          aexcompat::aex_strings::ParseStatus::Valid) {
    static constexpr char kEmptyString[] = "";
    result = kEmptyString;
  }
  if (extended_diag_enabled()) {
    std::cerr << "extended_diag:lookup id=" << id;
    diag_probe_arg("a0", table);
    diag_probe_arg("a2", arg2);
    diag_probe_arg("a3", arg3);
    std::cerr << " -> " << (result ? result : "(null)") << "\n" << std::flush;
  }
  const ExtendedLookupStringTableState raw_private_table_state =
      extended_lookup_table_state(
          active_string_table
              ? active_string_table->status
              : aexcompat::aex_strings::ParseStatus::NoEntries);
  const ExtendedLookupStringTableState windows_resource_source_state =
      ExtendedLookupStringTableState::none;
  const int32_t return_code = result ? 0 : 4;
  const ExtendedLookupOutcome outcome =
      result ? ExtendedLookupOutcome::found
             : raw_private_table_state ==
                       ExtendedLookupStringTableState::invalid
                   ? ExtendedLookupOutcome::invalid
                   : ExtendedLookupOutcome::missing;
  record_extended_lookup_diagnostic(
      table_classification, raw_private_table_state,
      windows_resource_source_state, id, outcome, return_code);
  record_host_callback_invocation(
      "inter.extended_lookup", return_code,
      HostCallbackClassification::fallback);
  return result;
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
  if (std::string(unresolved_entrypoint_plugin_kind(PiplPluginKind::Aegp)) !=
          "aegp_candidate" ||
      std::string(unresolved_entrypoint_plugin_kind(PiplPluginKind::Invalid)) !=
          "invalid_pipl" ||
      std::string(unresolved_entrypoint_plugin_kind(PiplPluginKind::Effect)) !=
          "unknown_no_effect_entrypoint" ||
      unresolved_entrypoint_report_json(PiplPluginKind::Aegp) !=
          "{\"plugin_kind\":\"aegp_candidate\"}" ||
      unresolved_entrypoint_report_json(PiplPluginKind::Unknown) !=
          "{\"plugin_kind\":\"unknown_no_effect_entrypoint\"}")
    return false;
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
  auto terminal_reserved = effect;
  append_pipl_property(terminal_reserved, "DRea", {0, 0, 0, 0});
  const auto reserved_parsed =
      parse_pipl_entrypoint(terminal_reserved.data(), terminal_reserved.size());
  if (reserved_parsed.kind != PiplPluginKind::Effect ||
      reserved_parsed.symbol != "entryPointFunc") return false;
  auto nonzero_terminal_reserved = terminal_reserved;
  nonzero_terminal_reserved.back() = 1;
  if (parse_pipl_entrypoint(nonzero_terminal_reserved.data(),
                            nonzero_terminal_reserved.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto duplicate_terminal_reserved = terminal_reserved;
  append_pipl_property(duplicate_terminal_reserved, "DRea", {0, 0, 0, 0});
  if (parse_pipl_entrypoint(duplicate_terminal_reserved.data(),
                            duplicate_terminal_reserved.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto unrelated_terminal_property = effect;
  append_pipl_property(unrelated_terminal_property, "eman", {0, 0, 0, 0});
  if (parse_pipl_entrypoint(unrelated_terminal_property.data(),
                            unrelated_terminal_property.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto match_name_length_drift = effect;
  match_name_length_drift[6] = 4;
  const std::size_t match_name_offset = match_name_length_drift.size();
  append_pipl_property(
      match_name_length_drift, "ANMe",
      {16, 'B', 'C', 'C', '3', 'O', 'p', 't', 'i', 'c', 'a', 'l', ' ',
       'F', 'l', 'o', 'w', 0, 0, 0});
  std::fill(match_name_length_drift.begin() + match_name_offset + 12,
            match_name_length_drift.begin() + match_name_offset + 16, 0);
  match_name_length_drift[match_name_offset + 12] = 28;
  append_pipl_property(match_name_length_drift, "LFea", {8, 0, 0, 0});
  append_pipl_property(match_name_length_drift, "DRea", {0, 0, 0, 0});
  const auto drift_parsed = parse_pipl_entrypoint(
      match_name_length_drift.data(), match_name_length_drift.size());
  if (drift_parsed.kind != PiplPluginKind::Effect ||
      drift_parsed.symbol != "entryPointFunc") return false;
  auto wrong_reserved_info = match_name_length_drift;
  wrong_reserved_info[wrong_reserved_info.size() - 24] = 9;
  if (parse_pipl_entrypoint(wrong_reserved_info.data(),
                            wrong_reserved_info.size()).kind !=
      PiplPluginKind::Invalid) return false;
  auto wrong_length_drift = match_name_length_drift;
  wrong_length_drift[match_name_offset + 12] = 24;
  if (parse_pipl_entrypoint(wrong_length_drift.data(),
                            wrong_length_drift.size()).kind !=
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

// Legacy-support-library process initialization (issue #362 selector
// families): PIN-era bundled effects (Curves, Reshape, FILE.dll-based
// classics) allocate through U.dll's process-wide allocator, which real AE
// initializes at process start by calling U_Birth. Nothing in the plug-in
// closure calls it (verified: no staged DLL imports U_Birth), so the host
// must, once per U.dll mapping. Failures leave the plug-in no worse off than
// before (its U_AllocateHandle calls keep failing as they already did).
//
// Called before every plug-in's inspect/bootstrap, one-shot and session alike.
// The latch (worker_legacy_support_init.hpp) keys on the U.dll mapping, not on
// "already tried": a session whose first member has no U.dll must still birth
// the allocator when a later member maps it (issue #1063; the former
// once-per-process `attempted` flag left Reshape_New failing PARAMS_SETUP with
// 4 (PF_Err_OUT_OF_MEMORY) whenever it followed a U.dll-free plug-in in the
// same discovery session).
int u_birth_seh_filter(EXCEPTION_POINTERS*) {
  return EXCEPTION_EXECUTE_HANDLER;
}

void initialize_u_dll_allocator() {
  namespace legacy = aexcompat::worker_runtime::legacy_support;
  static legacy::State state;
  // The absent/no-export markers are logged once per process: the decision is
  // re-asked for every member, but a 300-member session must not fill the
  // bounded stderr tail the broker keeps with the same line.
  static bool absent_logged = false;
  static bool no_birth_logged = false;
  const HMODULE u_module = GetModuleHandleW(L"U.dll");
  using UBirth = int(__cdecl*)(void*, uint8_t);
  const auto u_birth = u_module
      ? reinterpret_cast<UBirth>(GetProcAddress(u_module, "U_Birth"))
      : nullptr;
  switch (legacy::decide(state, u_module, u_birth != nullptr)) {
    case legacy::Decision::Absent:
      if (!absent_logged) {
        absent_logged = true;
        std::cerr << "stage:legacy_support_init status=absent\n" << std::flush;
      }
      return;
    case legacy::Decision::AlreadyInitialized:
      return;
    case legacy::Decision::NoBirthExport:
      if (!no_birth_logged) {
        no_birth_logged = true;
        std::cerr << "stage:legacy_support_init status=no_u_birth\n"
                  << std::flush;
      }
      return;
    case legacy::Decision::Call:
      break;
  }
  legacy::note_called(state, u_module);
  int result = -1;
  __try {
    // The first argument is an optional host context block; a zeroed buffer
    // keeps the late-field reads in U_Birth from faulting (null crashes
    // after the allocator init, which is the part the plug-ins need).
    static std::byte host_context[64]{};
    result = u_birth(host_context, 0);
  } __except (u_birth_seh_filter(GetExceptionInformation())) {
    result = -1;
  }
  std::cerr << "stage:legacy_support_init status=called result=" << result
            << "\n" << std::flush;
}

// The rest of the process-wide support-library births (issue #1279). U.dll is
// not the only library real AE births at process start, and a plug-in closure
// imports only their working entry points, never the birth. Two of the
// discovery failures in that issue are exactly this gap:
//
//   * Particle_Playground, once past U's Sweet Pea gate, faults during
//     GLOBAL_SETUP on a null read inside VAL.dll (observed:
//     `stage:selector_seh selector=GLOBAL_SETUP code=0xc0000005
//     site=other_module module=VAL.dll access=read fault=null`), and it
//     stops faulting once VAL_Birth runs. That VAL's global value-type
//     list is what was missing is the reading of VAL_Birth's
//     disassembly, not something the trace shows directly.
//   * PSL_Adjustments returns 21249 from GLOBAL_SETUP. That is COR.dll's
//     0x5301, returned by `COR_GetBIBAddressProc` ("BIB is uninitialized.")
//     when COR's cached BIB resolver is null - and that global is set by
//     `COR_Conception`, which nothing but the host's birth sequence calls.
//
// The entries below are the part of AE's own startup sequence (read out of
// `aelib.dll`, transcribed in that document) that this host runs, in AE's
// order and with AE's arguments:
// BEZ_Birth() / FILE_Birth() / M_Birth(3, 0) / RND_Birth() / VAL_Birth() /
// COR_Conception(quiet_bib_errors), where `quiet_bib_errors` is AE's
// "flags & 1" and is false for a host that does not set that flag, so this is
// what AE's own code produces rather than a guessed shape. Signatures come
// from each library's disassembly: `void VAL_Birth(void)`,
// `int FILE_Birth(void)`, `int BEZ_Birth(void)`, `int RND_Birth(void)`,
// `int M_Birth(unsigned, void*)`, `int COR_Conception(bool)`.
//
// It is a subset, not the whole sequence. What AE also does and this does not,
// and why:
//
//   * `FILE_AddExtensionMap(...)` x9 before FILE_Birth: registers AE's own
//     document extensions, which no effect plug-in needs to inspect or render.
//   * `PREF_Birth(...)` (between RND and VAL) and `MC_Birth(...)` (between VAL
//     and COR_Conception): AE's preferences file and media core. Neither is
//     needed by anything measured here, and both take AE-mode arguments and
//     touch user state, so they stay out until something demonstrably needs
//     them.
//   * `COR_Birth(a, quiet_bib_errors, b)`, two steps after COR_Conception:
//     it blocks indefinitely in this worker (observed: the l2 worker never
//     returns from it, while COR_Conception returns 0), and COR_Conception is
//     the step that sets the BIB resolver the 21249 comes from. Whatever
//     COR_Birth waits for is recorded as its own follow-up rather than worked
//     around.
//   * PLUG_Birth, TDB_Birth, TXT_Birth, BEE_Birth and the rest of the tail:
//     not reached by any failure measured for issue #1279. `PF_Birth`, which
//     sits in that tail, is run by initialize_pf_dll_host_layer below
//     (issue #1212 / PR #1284) - after this step, matching AE's order.
//
// VAL_Birth allocates through U (U_CopyString) and LIST, so U has to be
// birthed first, which is why this runs after initialize_u_dll_allocator.
// Same latch shape as U_Birth: keyed on the mapping, so a session whose first
// member maps none of these still births them for a later member that does,
// and a failed attempt is not retried on the same mapping.
void initialize_process_support_libraries() {
  namespace legacy = aexcompat::worker_runtime::legacy_support;
  enum class BirthShape {
    // int (*)()
    no_arguments,
    // void (*)(), reported as result 0 when it returns: VAL_Birth has no
    // return value, so reading one would report whatever is left in eax.
    no_arguments_void,
    // int (*)(unsigned, void*), called as AE calls M_Birth: (3, nullptr)
    m_birth,
    // int (*)(bool), called with AE's non-quiet value
    cor_conception,
  };
  struct SupportLibrary {
    const wchar_t* module;
    // `name` is the narrow spelling of `module` for the trace (std::cerr
    // takes no wide strings); `birth` is the exported symbol, which for
    // COR is a C++ mangled name.
    const char* name;
    const char* birth;
    BirthShape shape;
  };
  static constexpr SupportLibrary kLibraries[] = {
      {L"BEZ.dll", "BEZ.dll", "BEZ_Birth",
       BirthShape::no_arguments},
      {L"FILE.dll", "FILE.dll", "FILE_Birth",
       BirthShape::no_arguments},
      {L"U.dll", "U.dll", "M_Birth",
       BirthShape::m_birth},
      {L"U.dll", "U.dll", "RND_Birth",
       BirthShape::no_arguments},
      {L"VAL.dll", "VAL.dll", "VAL_Birth",
       BirthShape::no_arguments_void},
      {L"COR.dll", "COR.dll", "?COR_Conception@@YAH_N@Z",
       BirthShape::cor_conception},
  };
  static legacy::State states[std::size(kLibraries)];
  // A mapped library whose birth export is missing is reported once per
  // library: it means this AE build moved or renamed the entry, which must
  // not look the same as "the library is not in this closure".
  static bool no_export_logged[std::size(kLibraries)]{};
  for (std::size_t index = 0; index < std::size(kLibraries); ++index) {
    const SupportLibrary& library = kLibraries[index];
    const HMODULE module = GetModuleHandleW(library.module);
    const FARPROC birth =
        module ? GetProcAddress(module, library.birth) : nullptr;
    const legacy::Decision decision =
        legacy::decide(states[index], module, birth != nullptr);
    if (decision == legacy::Decision::NoBirthExport &&
        !no_export_logged[index]) {
      no_export_logged[index] = true;
      std::cerr << "stage:legacy_support_init library=" << library.name
                << " entry=" << library.birth << " status=no_export\n"
                << std::flush;
    }
    if (decision != legacy::Decision::Call) continue;
    legacy::note_called(states[index], module);
    int result = -1;
    __try {
      switch (library.shape) {
        case BirthShape::no_arguments:
          result = reinterpret_cast<int(__cdecl*)()>(birth)();
          break;
        case BirthShape::no_arguments_void:
          reinterpret_cast<void(__cdecl*)()>(birth)();
          result = 0;
          break;
        case BirthShape::m_birth:
          result = reinterpret_cast<int(__cdecl*)(uint32_t, void*)>(birth)(
              3, nullptr);
          break;
        case BirthShape::cor_conception:
          result = reinterpret_cast<int(__cdecl*)(bool)>(birth)(false);
          break;
      }
    } __except (u_birth_seh_filter(GetExceptionInformation())) {
      // COR_Conception reports failures by throwing, which arrives here as
      // SEH. The sequence continues after a fault rather than stopping:
      // each entry initializes its own library, a faulted one leaves that
      // library exactly as unusable as it was before this step existed,
      // and stopping would deny the later libraries an initialization they
      // do not depend on the faulted one for. The one real dependency in
      // this table (VAL needs U) is satisfied by initialize_u_dll_allocator
      // running first, not by an earlier entry here. The result is not
      // reported for a faulted entry: the trace says `status=faulted`, and a
      // number there would look like the library answered.
      std::cerr << "stage:legacy_support_init library=" << library.name
                << " entry=" << library.birth << " status=faulted\n"
                << std::flush;
      continue;
    }
    // A void birth has no result: printing one would be
    // indistinguishable from a library that answered with that value.
    std::cerr << "stage:legacy_support_init library=" << library.name
              << " entry=" << library.birth << " status=called";
    if (library.shape != BirthShape::no_arguments_void)
      std::cerr << " result=" << result;
    std::cerr << "\n" << std::flush;
  }
}

// PF.dll (AE's PF host layer) keeps its own dispatch table (`PFp_G_OverFuncs`,
// PF.dll+0x44d9b0..) that its exported entry points jump through:
// `PF_TransferRect` is `jmp [PFp_G_OverFuncs+0xe8]`, `PF_TransformWorld` calls
// through +0xf0, `PF_Blend` reads the block, and so on. Real AE fills the table
// at startup by calling the exported `PF_Birth` (which reads U.dll's CPU
// feature block, so U_Birth has to have run first; PF.dll imports U.dll, so it
// is mapped whenever PF.dll is). Adobe-bundled effects import those exports
// directly (Echo: PF_TransferRect, Transform: PF_TransformWorld, Spill2:
// PF_Blend, CannedWarp: PFp_G_fcd_table) and nothing in a plug-in closure
// calls PF_Birth, so without this step every such call jumps to address 0
// (observed as `stage:selector_seh ... access=execute fault=null` with the
// return address in the plug-in or PF.dll; issue #1212's Echo / Transform /
// Spill2 rows). Same latch as the U.dll step: once per PF.dll mapping.
void initialize_pf_dll_host_layer() {
  namespace legacy = aexcompat::worker_runtime::legacy_support;
  static legacy::State state;
  static bool no_birth_logged = false;
  const HMODULE pf_module = GetModuleHandleW(L"PF.dll");
  using PfBirth = uint32_t(__cdecl*)();
  const auto pf_birth = pf_module
      ? reinterpret_cast<PfBirth>(GetProcAddress(pf_module, "PF_Birth"))
      : nullptr;
  switch (legacy::decide(state, pf_module, pf_birth != nullptr)) {
    case legacy::Decision::Absent:
    case legacy::Decision::AlreadyInitialized:
      return;
    case legacy::Decision::NoBirthExport:
      if (!no_birth_logged) {
        no_birth_logged = true;
        std::cerr << "stage:pf_host_layer_init status=no_pf_birth\n"
                  << std::flush;
      }
      return;
    case legacy::Decision::Call:
      break;
  }
  legacy::note_called(state, pf_module);
  uint32_t result = 0xffffffffu;
  bool faulted = false;
  __try {
    result = pf_birth();
  } __except (u_birth_seh_filter(GetExceptionInformation())) {
    faulted = true;
  }
  std::cerr << "stage:pf_host_layer_init status=called result=" << result
            << (faulted ? " faulted=1" : "") << "\n" << std::flush;
}

void initialize_legacy_support_libraries() {
  initialize_u_dll_allocator();
  initialize_process_support_libraries();
  initialize_pf_dll_host_layer();
}

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
// The effect ref is a PF_ProgressInfo-shaped object (issue #1212): its
// identity is still this address, but a plug-in (or PF.dll) that reads it as
// AE's `PF_ProgressInfo` finds the refcon / abort / progress slots instead of a
// 4-byte tag. The slots are published by publish_effect_ref_object() before
// every plug-in bootstrap (worker_pf_progress_info).
aexcompat::worker_runtime::pf_progress_info::EffectRefObject g_effect{};
// Called before every plug-in bootstrap (one-shot, cluster member, swap):
// republishes the effect-ref layout in case the previous member left a slot
// edited.
void publish_effect_ref_object() {
  aexcompat::worker_runtime::pf_progress_info::publish(g_effect);
}
// The effect layer handle is a BEE.dll-compatible object (issue #1210): its
// identity is still this address, but a plug-in that reads it as a
// `BEE_AVLayer*` finds the observed layout instead of a 4-byte tag. The
// vtable and object graph behind it are installed at hand-out time by
// AEGP_GetEffectLayer (worker_bee_scene_facade).
aexcompat::worker_runtime::bee_facade::LayerObject g_layer{};

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

std::string suite_call_slot_probe_report_json() {
  return aexcompat::worker_runtime::suite_call_slot_probe::report_json();
}

std::string compute_cache_timeline_report_json() {
  return aexcompat::worker_runtime::compute_cache::telemetry_report_json();
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
  if (plugin_id < 0 || !value) return false;
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
         (key.second == 11 || key.second == 13 || key.second == 14 ||
          key.second == 15)) ||
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

// The same accessors on a world storage (the LayerDef part).
template <typename T>
T read(const aexcompat::world_safety::EffectWorldStorage& world, std::size_t offset) {
  return read<T>(world.layer_def, offset);
}
template <typename T>
void write(aexcompat::world_safety::EffectWorldStorage& world, std::size_t offset, T value) {
  write(world.layer_def, offset, value);
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


// Legacy application-specific callback at utils+0xC8 (issue #362): PIN-era
// effects call app(effect_ref, selector, arg) during GLOBAL_SETUP — traced
// Drop_Shadow passing selector 3 with an effect-owned callback table, which
// real AE accepts. Selectors are accepted and logged; nothing in the
// headless worker consumes the registered tables.
int32_t __cdecl host_app_callback(void* effect_ref, int32_t selector,
                                  void* arg) {
  if (extended_diag_enabled())
    std::cerr << "extended_diag:app selector=" << selector << " arg=" << arg
              << "\n" << std::flush;
  if (!effect_ref) return 4;
  return 0;
}

int32_t __cdecl get_platform_data(void* effect_ref, int32_t which, void* data) {
  constexpr int32_t kExeFilePathDeprecated = 1;
  constexpr int32_t kResourceFilePathDeprecated = 2;
  constexpr int32_t kExeFilePathWide = 7;
  constexpr int32_t kResourceFilePathWide = 8;
  constexpr std::size_t kMaxPath = 260;
  if (!effect_ref || !data ||
      (which != kExeFilePathDeprecated &&
       which != kResourceFilePathDeprecated &&
       which != kExeFilePathWide && which != kResourceFilePathWide) ||
      g_plugin_file_path.empty() || g_plugin_file_path.size() >= kMaxPath ||
      !std::filesystem::path(g_plugin_file_path).is_absolute()) return 4;
  if (which == kExeFilePathDeprecated ||
      which == kResourceFilePathDeprecated) {
    BOOL used_default_character = FALSE;
    const int written = WideCharToMultiByte(
        CP_ACP, WC_NO_BEST_FIT_CHARS, g_plugin_file_path.c_str(), -1,
        static_cast<char*>(data), static_cast<int>(kMaxPath), nullptr,
        &used_default_character);
    return written > 0 && !used_default_character ? 0 : 4;
  }
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
  // UPDATE_PARAMS_UI is not dispatched here (issue #1156). A headless render has
  // no Effect Controls panel, and an AE oracle capture confirmed AE's own
  // aerender never issues UPDATE_PARAMS_UI even to an effect that advertises
  // SEND_UPDATE_PARAMS_UI (selector-timeline probe, AE 26.3: GLOBAL_SETUP ..
  // SEQUENCE_SETUP .. FRAME_SETUP .. RENDER .. with zero UPDATE_PARAMS_UI).
  // Issuing it here drove UI-only suite lifecycles the render never pairs -
  // Cycore effects released PF Path Data Suite they only acquire while a real
  // UI is up, which the fail-closed suite-lease check then rejected at close
  // (session_close_failed), and DeepGlow2 answered it with error 4 and lost the
  // frame (#883) though it renders in AE. Matching AE (not issuing it) is the
  // fix. `g_update_params_ui_error` stays -1 (not dispatched) for the report.
  if (g_query_dynamic_flags_advertised) {
    g_query_dynamic_flags_error = entry(kQueryDynamicFlags, input.data(), output.data(), params,
                                        nullptr, nullptr);
    g_conditional_ui_selectors_dispatched = true;
  }
  // Only QUERY_DYNAMIC_FLAGS decides whether the frame can proceed.
  //
  // The two selectors are not the same kind of precondition. QUERY_DYNAMIC_FLAGS
  // rewrites out_flags/out_flags2, which the render path then reads for the
  // wide-time and shutter rules and for NOP_RENDER, so a plug-in that fails it
  // has left the host without the flags the rest of the frame is built on.
  // UPDATE_PARAMS_UI only refreshes what the Effect Controls panel shows. The
  // observation this rests on is that DeepGlow2 answers it with error 4 and
  // renders in AE anyway, while here that error ended the frame before
  // SMART_PRE_RENDER (issue #883); AE's own dispatch conditions are not
  // measured, so "AE does not require it to render" is inference from that one
  // plug-in, not something this host has checked.
  //
  // The error is not swallowed - it stays in `update_params_ui_error` for the
  // report and in the line below - it just no longer decides the frame. It
  // does still decide whether the definitions the selector rewrote are
  // adopted; see `conditional_ui_definitions_usable`.
  if (extended_diag_enabled())
    std::cerr << "extended_diag:conditional_ui update_advertised="
              << g_update_params_ui_advertised
              << " update_error=" << g_update_params_ui_error
              << " query_advertised=" << g_query_dynamic_flags_advertised
              << " query_error=" << g_query_dynamic_flags_error << "\n"
              << std::flush;
  return !g_query_dynamic_flags_advertised || g_query_dynamic_flags_error == 0;
}

// Whether the parameter definitions the conditional-UI selectors left behind
// may replace the host's own. A selector that reported an error stopped
// somewhere it chose not to say, so what it wrote is partial by definition;
// letting the frame proceed without it is the #883 fix, adopting its writes
// anyway is not part of that. UPDATE_PARAMS_UI is no longer dispatched (#1156),
// so it rewrites nothing and does not gate this; only QUERY_DYNAMIC_FLAGS does.
bool conditional_ui_definitions_usable() {
  return !g_query_dynamic_flags_advertised || g_query_dynamic_flags_error == 0;
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


// Builds the one-shot L2 report JSON. `include_module_audit` is false only
// for the discovery session's inspect_done payload, whose report mirrors the
// one-shot params-only JSON minus module_audit (closure-session design §4.2);
// the session's own final report carries the audit with the swap epochs.
std::string build_l2_report_json(
    const char* status, int32_t global_error, int32_t params_error,
    int32_t setdown_error, const std::array<std::byte, kOutSize>& output,
    const std::string& about_message, const std::array<int32_t, 5>& lifecycle_errors,
    bool lifecycle_data_null, bool include_module_audit) {
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
  c.missing_suites_json = missing_suites_report_json();
  c.suite_timeline_json = suite_timeline_report_json();
  const auto* message = reinterpret_cast<const char*>(output.data() + kOutMessage);
  c.return_message.assign(message, strnlen_s(message, 256)); c.about_message = about_message;
  c.about_selector_dispatched = !g_skip_about; c.last_seh_selector = g_last_seh_selector; c.last_seh_error = g_last_seh_error;
  c.last_seh_exception_code = g_last_seh_exception_code;
  c.lifecycle_errors = lifecycle_errors; c.lifecycle_data_null = lifecycle_data_null;
  c.unsupported_suite_calls_json = unsupported_suite_calls_report_json();
  c.suite_call_slot_probe_json = suite_call_slot_probe_report_json();
  c.compute_cache_timeline_json = compute_cache_timeline_report_json();
  c.selector_invocations_json = selector_invocations_report_json();
  if (!module_audit_passed())
    c.module_audit_failure_json = module_audit_failure_json();
  if (include_module_audit) c.module_audit_json = module_audit_json();
  c.parameters.reserve(g_params.size());
  for (const auto& p : g_params) {
    const auto* name = reinterpret_cast<const char*>(p.raw.data() + kParamName);
    c.parameters.push_back({p.index, p.disk_id, p.type, read<uint32_t>(p.raw, kParamUiFlags), read<int16_t>(p.raw, 8), read<int16_t>(p.raw, 10), read<uint32_t>(p.raw, kParamFlags), std::string(name, strnlen_s(name, kParamNameSize)), p.has_numeric, p.valid_min, p.valid_max, p.slider_min, p.slider_max, p.default_value, p.has_current, p.current_value, p.has_color, p.default_color, p.current_color, p.component_count, p.default_components, p.current_components, p.precision, p.choices, p.label, p.arbitrary_summary, p.layer_default});
  }
  return aexcompat::worker_report::serialize_l2_report(c);
}

void report(const char* status, int32_t global_error, int32_t params_error,
            int32_t setdown_error, const std::array<std::byte, kOutSize>& output,
            const std::string& about_message, const std::array<int32_t, 5>& lifecycle_errors,
            bool lifecycle_data_null) {
  restore_native_stdout();
  std::cout << build_l2_report_json(status, global_error, params_error,
                                    setdown_error, output, about_message,
                                    lifecycle_errors, lifecycle_data_null,
                                    /*include_module_audit=*/true);
}

// The early-mode bridge (EarlyModeBridge and its l2mode hook adapters)
// moved to worker_early_mode_bridge.cpp (issue #165); worker_main keeps
// resolving it through worker_early_mode_bridge.hpp.


// Diagnostics (worker_extended_diag.hpp): one-shot dump of host callback
// entry points for external debugger tracing (issue #362).
void diag_dump_callback_addresses() {
  static bool dumped = false;
  if (dumped) return;
  dumped = true;
  std::cerr << std::hex << std::showbase;
  std::cerr << "extended_diag:addr add_param=" << reinterpret_cast<void*>(&add_param)
            << " checkout_param=" << reinterpret_cast<void*>(&checkout_param)
            << " checkin_param=" << reinterpret_cast<void*>(&checkin_param) << "\n";
  std::cerr << "extended_diag:addr abort_render=" << reinterpret_cast<void*>(&abort_render)
            << " report_progress=" << reinterpret_cast<void*>(&report_progress)
            << " register_custom_ui=" << reinterpret_cast<void*>(&register_custom_ui) << "\n";
  std::cerr << "extended_diag:addr ext_alloc=" << reinterpret_cast<void*>(&host_extended_alloc)
            << " ext_lookup=" << reinterpret_cast<void*>(&host_extended_lookup)
            << " ext_free=" << reinterpret_cast<void*>(&host_extended_free) << "\n";
  std::cerr << "extended_diag:addr acquire_suite=" << reinterpret_cast<void*>(&acquire_suite)
            << " ansi_strcpy=" << reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy)
            << " ansi_strcpy_bounded="
            << reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_strcpy_bounded)
            << " ansi_sprintf=" << reinterpret_cast<void*>(&aexcompat::pf_ansi::ansi_sprintf)
            << "\n";
  std::cerr << "extended_diag:addr new_handle=" << reinterpret_cast<void*>(&new_handle)
            << " lock_handle=" << reinterpret_cast<void*>(&lock_handle)
            << " unlock_handle=" << reinterpret_cast<void*>(&unlock_handle)
            << " dispose_handle=" << reinterpret_cast<void*>(&dispose_handle)
            << " handle_size=" << reinterpret_cast<void*>(&handle_size)
            << " resize_handle=" << reinterpret_cast<void*>(&resize_handle) << "\n";
  std::cerr << "extended_diag:addr get_platform_data="
            << reinterpret_cast<void*>(&get_platform_data) << "\n";
  std::cerr << std::dec << std::noshowbase << std::flush;
}

}  // namespace aexcompat::l2_detail
