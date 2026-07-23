#include "worker_selector_dispatch.hpp"

#include <windows.h>
#include <delayimp.h>

#include "worker_minidump_runtime.hpp"
#include "worker_suite_registry.hpp"
namespace aexcompat::worker_runtime {
namespace {

constexpr int32_t kAuditFailure = 512;
AuditCapture g_capture_audit{};
AuditPassed g_audit_passed{};
SelectorDispatchTrace g_selector_trace{};
SelectorDispatchTelemetry g_telemetry;

constexpr uint32_t kDelayLoadModuleNotFound = 0xC06D007Eu;
constexpr std::size_t kMaxDependencyName = 260;

bool capture_delay_load_basename(EXCEPTION_POINTERS* information,
                                 char* output,
                                 std::size_t capacity) noexcept {
  if (!information || !information->ExceptionRecord || !output || capacity < 2)
    return false;
  const EXCEPTION_RECORD* record = information->ExceptionRecord;
  if (record->ExceptionCode != kDelayLoadModuleNotFound ||
      record->NumberParameters < 1 || record->ExceptionInformation[0] == 0)
    return false;
  __try {
    const auto* delay = reinterpret_cast<const DelayLoadInfo*>(
        record->ExceptionInformation[0]);
    const char* name = delay->szDll;
    if (!name) return false;
    std::size_t length = 0;
    for (; length + 1 < capacity && length < kMaxDependencyName; ++length) {
      const unsigned char byte = static_cast<unsigned char>(name[length]);
      if (byte == 0) break;
      if (!(isalnum(byte) || byte == '.' || byte == '_' || byte == '-'))
        return false;
      output[length] = static_cast<char>(byte);
    }
    if (length == 0 || length >= kMaxDependencyName || name[length] != '\0')
      return false;
    if (length < 4 || _stricmp(output + length - 4, ".dll") != 0)
      return false;
    output[length] = '\0';
    return true;
  } __except(EXCEPTION_EXECUTE_HANDLER) {
    return false;
  }
}

int capture_seh_exception(EXCEPTION_POINTERS* information) {
  minidump::classify_seh_exception(
      information, {g_telemetry.seh_code, g_telemetry.seh_address,
                    g_telemetry.seh_module});
  char dependency[kMaxDependencyName + 1]{};
  if (capture_delay_load_basename(information, dependency, sizeof(dependency)))
    g_telemetry.missing_dependency = dependency;
  return EXCEPTION_EXECUTE_HANDLER;
}

int32_t audited_effect_call(EffectEntry entry, int32_t command, void* input,
                            void* output, void** params, void* world, void* extra) {
  if (!entry || !g_capture_audit || !g_audit_passed) return kAuditFailure;
  if (g_selector_trace) g_selector_trace(effect_selector_name(command));
  const int32_t error = entry(command, input, output, params, world, extra);
  g_capture_audit();
  return g_audit_passed() ? error : kAuditFailure;
}

}  // namespace

void configure_selector_dispatch_audit(AuditCapture capture,
                                       AuditPassed passed) noexcept {
  g_capture_audit = capture;
  g_audit_passed = passed;
}

void configure_selector_dispatch_trace(SelectorDispatchTrace trace) noexcept {
  g_selector_trace = trace;
}

SelectorDispatchTelemetry& selector_dispatch_telemetry() noexcept {
  return g_telemetry;
}

const char* effect_selector_name(int32_t command) noexcept {
  switch (command) {
    case 0: return "ABOUT";
    case 1: return "GLOBAL_SETUP";
    case 3: return "GLOBAL_SETDOWN";
    case 4: return "PARAMS_SETUP";
    case 5: return "SEQUENCE_SETUP";
    case 6: return "SEQUENCE_RESETUP";
    case 7: return "SEQUENCE_FLATTEN";
    case 8: return "SEQUENCE_SETDOWN";
    case 9: return "DO_DIALOG";
    case 10: return "FRAME_SETUP";
    case 11: return "RENDER";
    case 12: return "FRAME_SETDOWN";
    case 13: return "USER_CHANGED_PARAM";
    case 14: return "UPDATE_PARAMS_UI";
    case 15: return "EVENT";
    case 16: return "GET_EXTERNAL_DEPENDENCIES";
    case 18: return "QUERY_DYNAMIC_FLAGS";
    case 19: return "AUDIO_RENDER";
    case 20: return "AUDIO_SETUP";
    case 21: return "AUDIO_SETDOWN";
    case 22: return "ARBITRARY_CALLBACK";
    case 23: return "SMART_PRE_RENDER";
    case 24: return "SMART_RENDER";
    case 28: return "GET_FLATTENED_SEQUENCE_DATA";
    case 31: return "SMART_RENDER_GPU";
    case 32: return "GPU_DEVICE_SETUP";
    case 33: return "GPU_DEVICE_SETDOWN";
    default: return "UNKNOWN";
  }
}

int32_t invoke_entry_seh(EffectEntry entry, int32_t command, void* input,
                         void* output, void** params, void* world, void* extra,
                         uint32_t* out_exception_code) {
  if (!out_exception_code) return kAuditFailure;
  *out_exception_code = 0;
  g_telemetry.missing_dependency.clear();
  const char* previous_suite_selector =
      set_suite_timeline_selector(effect_selector_name(command));
  int32_t result = 0;
  __try {
    result = audited_effect_call(entry, command, input, output, params, world, extra);
  } __except(capture_seh_exception(GetExceptionInformation())) {
    *out_exception_code = GetExceptionCode();
    g_telemetry.selector = effect_selector_name(command);
    g_telemetry.error = kAuditFailure;
    if (g_capture_audit) g_capture_audit();
    result = kAuditFailure;
  }
  set_suite_timeline_selector(previous_suite_selector);
  return result;
}

int32_t guarded_effect_call(EffectEntry entry, int32_t command, void* input,
                            void* output, void** params, void* world, void* extra) {
  uint32_t exception_code{};
  return invoke_entry_seh(entry, command, input, output, params, world, extra,
                          &exception_code);
}

int32_t invoke_smart_pre_render_cleanup_seh(void(__cdecl* cleanup)(void*),
                                            void* data) {
  if (!cleanup) return 0;
  __try {
    cleanup(data);
    return 0;
  } __except(capture_seh_exception(GetExceptionInformation())) {
    g_telemetry.selector = "SMART_PRE_RENDER_CLEANUP";
    g_telemetry.error = kAuditFailure;
    if (g_capture_audit) g_capture_audit();
    return kAuditFailure;
  }
}

}  // namespace aexcompat::worker_runtime
