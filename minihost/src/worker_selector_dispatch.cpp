#include "worker_selector_dispatch.hpp"

#include <windows.h>
#include <bcrypt.h>
#include <delayimp.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cstring>
#include <iomanip>
#include <sstream>

#include "worker_minidump_runtime.hpp"
#include "worker_aegp_compute_cache.hpp"
#include "worker_suite_registry.hpp"
namespace aexcompat::worker_runtime {
namespace {

constexpr int32_t kAuditFailure = 512;
AuditCapture g_capture_audit{};
AuditPassed g_audit_passed{};
SelectorDispatchTrace g_selector_trace{};
SelectorDispatchTelemetry g_telemetry;
thread_local HMODULE g_active_entry_module{};
thread_local const char* g_current_fault_module_class{};
thread_local uint64_t g_current_plugin_rva{};
thread_local bool g_current_has_plugin_rva{};
thread_local bool g_has_previous_global_data_output{};
thread_local uintptr_t g_previous_global_data_output{};
thread_local bool g_has_global_setup_effect_ref_entry{};
thread_local uintptr_t g_global_setup_effect_ref_entry{};
thread_local bool g_has_global_setup_application_id_entry{};
thread_local uint32_t g_global_setup_application_id_entry{};
thread_local bool g_has_global_setup_spec_version_entry{};
thread_local uint32_t g_global_setup_spec_version_entry{};
thread_local const char* g_host_callback_selector = "HOST";
HostCallbackTimelineTelemetry g_host_callback_timeline;
ExtendedLookupTimelineTelemetry g_extended_lookup_timeline;

struct TrackedExtendedAllocation {
  uintptr_t pointer{};
  std::string token;
  std::string owner_selector;
  bool live{};
};

struct ExtendedAllocationSnapshot {
  std::string token;
  std::string owner_selector;
  bool live{};
};

struct ExtendedAllocationTimelineRecord {
  uint32_t sequence{};
  std::string selector;
  bool entry{};
  uint32_t live_allocation_count{};
  uint32_t new_allocations{};
  uint32_t frees{};
  uint32_t invalid_frees{};
  uint32_t double_frees{};
  uint32_t global_setup_live_allocation_count{};
  std::vector<ExtendedAllocationSnapshot> allocations;
};

struct ExtendedAllocationDiagnostics {
  uint32_t next_sequence{};
  uint32_t phase_new_allocations{};
  uint32_t phase_frees{};
  uint32_t phase_invalid_frees{};
  uint32_t phase_double_frees{};
  std::vector<TrackedExtendedAllocation> tracked;
  std::vector<ExtendedAllocationTimelineRecord> records;
  bool truncated{};
};

ExtendedAllocationDiagnostics g_extended_allocations;

constexpr uint32_t kDelayLoadModuleNotFound = 0xC06D007Eu;
constexpr std::size_t kMaxDependencyName = 260;
constexpr uintptr_t kLowAddressLimit = 0x10000;
constexpr std::size_t kInEffectRefOffset = 184;
constexpr std::size_t kInSpecVersionOffset = 196;
constexpr std::size_t kInApplicationIdOffset = 204;
constexpr std::size_t kInGlobalDataOffset = 312;
constexpr std::size_t kOutGlobalDataOffset = 40;
// PF_OutData::return_msg and the out flag a plug-in raises alongside it.
constexpr std::size_t kOutFlagsOffset = 96;
constexpr std::size_t kOutReturnMsgOffset = 100;
constexpr std::size_t kOutReturnMsgSize = 256;
constexpr uint32_t kOutFlagDisplayErrorMessage = 1u << 8;
constexpr std::array<const char*, 5> kRegisterNames{
    "rcx", "rdx", "r8", "r9", "rsp"};

struct RawAccessViolationContext {
  bool access_violation{};
  uint8_t access_type{};
  bool has_fault_address{};
  uintptr_t fault_address{};
  bool has_registers{};
  std::array<uintptr_t, kRegisterNames.size()> registers{};
  std::array<uintptr_t, kMaxSelectorStackValues> stack_values{};
  std::array<bool, kMaxSelectorStackValues> stack_readable{};
};

thread_local RawAccessViolationContext g_current_access_violation{};

HMODULE module_from_address(const void* address) noexcept {
  HMODULE module{};
  if (address && GetModuleHandleExW(
          GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
              GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
          reinterpret_cast<LPCWSTR>(address), &module))
    return module;
  return nullptr;
}

std::string module_basename(HMODULE module) {
  if (!module) return {};
  std::array<wchar_t, MAX_PATH> path{};
  const DWORD length = GetModuleFileNameW(
      module, path.data(), static_cast<DWORD>(path.size()));
  if (length == 0 || length >= path.size()) return {};
  const wchar_t* start = path.data();
  for (const wchar_t* cursor = path.data(); *cursor; ++cursor)
    if (*cursor == L'\\' || *cursor == L'/') start = cursor + 1;
  std::string basename;
  for (const wchar_t* cursor = start; *cursor && basename.size() < 260; ++cursor)
    basename.push_back(*cursor >= 0x20 && *cursor <= 0x7e
                           ? static_cast<char>(*cursor)
                           : '?');
  return basename;
}

uint64_t pointer_token_secret() noexcept {
  static const uint64_t secret = []() noexcept {
    uint64_t value{};
    if (BCryptGenRandom(nullptr, reinterpret_cast<PUCHAR>(&value),
                        sizeof(value), BCRYPT_USE_SYSTEM_PREFERRED_RNG) < 0) {
      LARGE_INTEGER counter{};
      QueryPerformanceCounter(&counter);
      value = static_cast<uint64_t>(counter.QuadPart) ^
          (static_cast<uint64_t>(GetCurrentProcessId()) << 32) ^
          static_cast<uint64_t>(GetTickCount64());
    }
    return value ? value : 0x6a09e667f3bcc909ULL;
  }();
  return secret;
}

uint64_t mix_pointer_token(uintptr_t pointer) noexcept {
  uint64_t value = static_cast<uint64_t>(pointer) ^ pointer_token_secret();
  value += 0x9e3779b97f4a7c15ULL;
  value = (value ^ (value >> 30)) * 0xbf58476d1ce4e5b9ULL;
  value = (value ^ (value >> 27)) * 0x94d049bb133111ebULL;
  return value ^ (value >> 31);
}

std::string pointer_token(uintptr_t pointer) {
  std::ostringstream output;
  output << "ptr-" << std::hex << std::setw(16) << std::setfill('0')
         << mix_pointer_token(pointer);
  return output.str();
}

PointerClassificationDiagnostic classify_pointer(uintptr_t pointer) {
  PointerClassificationDiagnostic result;
  if (pointer == 0) {
    result.classification = "null";
    return result;
  }
  if (pointer < kLowAddressLimit) {
    result.classification = "low";
    return result;
  }
  HMODULE module =
      module_from_address(reinterpret_cast<const void*>(pointer));
  if (module) {
    result.classification =
        module == g_active_entry_module ? "plugin" : "module";
    result.module = module_basename(module);
    const uintptr_t base = reinterpret_cast<uintptr_t>(module);
    if (pointer >= base) {
      result.has_relative_offset = true;
      result.relative_offset = static_cast<uint64_t>(pointer - base);
    }
    return result;
  }
  result.classification = "heap_or_unknown";
  result.token = pointer_token(pointer);
  return result;
}

struct CapturedGlobalDataState {
  GlobalDataStateDiagnostic diagnostic;
  uintptr_t pointer{};
};

CapturedGlobalDataState capture_global_data_state(
    const void* buffer, std::size_t offset) {
  CapturedGlobalDataState captured;
  const uintptr_t base = reinterpret_cast<uintptr_t>(buffer);
  if (!base || base > UINTPTR_MAX - offset) return captured;
  SIZE_T bytes_read{};
  if (!ReadProcessMemory(
          GetCurrentProcess(), reinterpret_cast<const void*>(base + offset),
          &captured.pointer, sizeof(captured.pointer), &bytes_read) ||
      bytes_read != sizeof(captured.pointer))
    return captured;
  captured.diagnostic.captured = true;
  captured.diagnostic.is_null = captured.pointer == 0;
  captured.diagnostic.classification =
      classify_pointer(captured.pointer).classification;
  if (captured.pointer)
    captured.diagnostic.process_local_token =
        pointer_token(captured.pointer);
  return captured;
}

struct CapturedApplicationId {
  bool captured{};
  uint32_t value{};
  std::string printable_code;
};

CapturedApplicationId capture_application_id(const void* buffer) {
  CapturedApplicationId captured;
  const uintptr_t base = reinterpret_cast<uintptr_t>(buffer);
  if (!base || base > UINTPTR_MAX - kInApplicationIdOffset) return captured;
  SIZE_T bytes_read{};
  if (!ReadProcessMemory(
          GetCurrentProcess(),
          reinterpret_cast<const void*>(base + kInApplicationIdOffset),
          &captured.value, sizeof(captured.value), &bytes_read) ||
      bytes_read != sizeof(captured.value))
    return captured;
  captured.captured = true;
  std::ostringstream code;
  code << std::hex << std::setfill('0');
  for (int shift : {24, 16, 8, 0}) {
    const uint8_t byte =
        static_cast<uint8_t>((captured.value >> shift) & 0xffu);
    if (byte >= 0x20 && byte <= 0x7e)
      code << static_cast<char>(byte);
    else
      code << "\\x" << std::setw(2) << static_cast<uint32_t>(byte);
  }
  captured.printable_code = code.str();
  return captured;
}

struct CapturedSpecVersion {
  bool captured{};
  uint32_t raw_packed_value{};
  int16_t major{};
  int16_t minor{};
};

CapturedSpecVersion capture_spec_version(const void* buffer) {
  CapturedSpecVersion captured;
  const uintptr_t base = reinterpret_cast<uintptr_t>(buffer);
  if (!base || base > UINTPTR_MAX - kInSpecVersionOffset) return captured;
  SIZE_T bytes_read{};
  if (!ReadProcessMemory(
          GetCurrentProcess(),
          reinterpret_cast<const void*>(base + kInSpecVersionOffset),
          &captured.raw_packed_value, sizeof(captured.raw_packed_value),
          &bytes_read) ||
      bytes_read != sizeof(captured.raw_packed_value))
    return captured;
  captured.captured = true;
  captured.major =
      static_cast<int16_t>(captured.raw_packed_value & 0xffffu);
  captured.minor = static_cast<int16_t>(
      (captured.raw_packed_value >> 16) & 0xffffu);
  return captured;
}

void capture_access_violation_context(
    EXCEPTION_POINTERS* information) noexcept {
  g_current_access_violation = {};
  if (!information || !information->ExceptionRecord)
    return;
  const EXCEPTION_RECORD* record = information->ExceptionRecord;
  if (record->ExceptionCode != EXCEPTION_ACCESS_VIOLATION &&
      record->ExceptionCode != EXCEPTION_IN_PAGE_ERROR)
    return;
  g_current_access_violation.access_violation = true;
  if (record->NumberParameters >= 1) {
    const ULONG_PTR operation = record->ExceptionInformation[0];
    g_current_access_violation.access_type =
        operation == 0 ? 1 : operation == 1 ? 2 : operation == 8 ? 3 : 4;
  } else {
    g_current_access_violation.access_type = 4;
  }
  if (record->NumberParameters >= 2) {
    g_current_access_violation.has_fault_address = true;
    g_current_access_violation.fault_address =
        static_cast<uintptr_t>(record->ExceptionInformation[1]);
  }
#if defined(_M_X64)
  if (!information->ContextRecord) return;
  const CONTEXT& context = *information->ContextRecord;
  g_current_access_violation.has_registers = true;
  g_current_access_violation.registers = {
      static_cast<uintptr_t>(context.Rcx),
      static_cast<uintptr_t>(context.Rdx),
      static_cast<uintptr_t>(context.R8),
      static_cast<uintptr_t>(context.R9),
      static_cast<uintptr_t>(context.Rsp)};
  for (std::size_t index = 0; index < kMaxSelectorStackValues; ++index) {
    const uintptr_t offset = index * sizeof(uintptr_t);
    if (context.Rsp > UINTPTR_MAX - offset) break;
    SIZE_T bytes_read{};
    uintptr_t value{};
    const void* address =
        reinterpret_cast<const void*>(static_cast<uintptr_t>(context.Rsp) +
                                      offset);
    if (ReadProcessMemory(GetCurrentProcess(), address, &value, sizeof(value),
                          &bytes_read) &&
        bytes_read == sizeof(value)) {
      g_current_access_violation.stack_values[index] = value;
      g_current_access_violation.stack_readable[index] = true;
    }
  }
#endif
}

const char* access_type_name(uint8_t access_type) noexcept {
  switch (access_type) {
    case 1: return "read";
    case 2: return "write";
    case 3: return "execute";
    default: return "unknown";
  }
}

void classify_fault_site(EXCEPTION_POINTERS* information) noexcept {
  g_current_fault_module_class = "unknown";
  g_current_plugin_rva = 0;
  g_current_has_plugin_rva = false;
  const void* address = information && information->ExceptionRecord
      ? information->ExceptionRecord->ExceptionAddress : nullptr;
  if (!address) return;
  HMODULE fault_module = module_from_address(address);
  if (!fault_module) {
    g_current_fault_module_class = "unmapped";
    return;
  }
  if (g_active_entry_module && fault_module == g_active_entry_module) {
    g_current_fault_module_class = "plugin";
    const uintptr_t fault = reinterpret_cast<uintptr_t>(address);
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_active_entry_module);
    if (fault >= base) {
      g_current_plugin_rva = static_cast<uint64_t>(fault - base);
      g_current_has_plugin_rva = true;
    }
    return;
  }
  g_current_fault_module_class =
      fault_module == GetModuleHandleW(nullptr) ? "worker" : "other_module";
}

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
  ++g_telemetry.seh_sequence;
  minidump::classify_seh_exception(
      information, {g_telemetry.seh_code, g_telemetry.seh_address,
                    g_telemetry.seh_module});
  classify_fault_site(information);
  capture_access_violation_context(information);
  char dependency[kMaxDependencyName + 1]{};
  if (capture_delay_load_basename(information, dependency, sizeof(dependency)))
    g_telemetry.missing_dependency = dependency;
  return EXCEPTION_EXECUTE_HANDLER;
}

int32_t audited_effect_call(EffectEntry entry, int32_t command, void* input,
                            void* output, void** params, void* world, void* extra,
                            bool* invocation_completed_normally,
                            int32_t* raw_return_code) {
  if (!entry || !g_capture_audit || !g_audit_passed) return kAuditFailure;
  if (g_selector_trace) g_selector_trace(effect_selector_name(command));
  const int32_t error = entry(command, input, output, params, world, extra);
  if (invocation_completed_normally) *invocation_completed_normally = true;
  if (raw_return_code) *raw_return_code = error;
  g_capture_audit();
  return g_audit_passed() ? error : kAuditFailure;
}

// The buffer is a fixed-size C string the plug-in owns; it is read as bytes,
// bounded by its declared size, and kept only when every byte is printable
// ASCII. That rejects the shapes that would break the hand-built JSON around
// it (newlines, control bytes, a non-terminated blob) - it does not make the
// text trustworthy, and the broker re-validates it at the transport boundary.
std::string captured_return_message(void* output) {
  if (!output) return {};
  const auto* bytes = static_cast<const unsigned char*>(output) + kOutReturnMsgOffset;
  std::size_t length = 0;
  while (length < kOutReturnMsgSize - 1 && bytes[length] != 0) ++length;
  std::string text(reinterpret_cast<const char*>(bytes), length);
  const bool printable = std::all_of(text.begin(), text.end(), [](unsigned char value) {
    return value >= 0x20 && value < 0x7F;
  });
  return printable ? text : std::string{};
}

void record_selector_invocation(const char* selector,
                                bool invocation_completed_normally,
                                int32_t raw_return_code,
                                int32_t host_result_code,
                                bool seh_caught,
                                uint32_t seh_code,
                                const GlobalDataHandoffDiagnostic& handoff,
                                const EffectRefEntryDiagnostic& effect_ref,
                                const ApplicationIdEntryDiagnostic& appl_id,
                                const SpecVersionEntryDiagnostic& version) {
  if (g_telemetry.invocations.size() >=
      kMaxSelectorInvocationDiagnostics) {
    g_telemetry.invocations_truncated = true;
    return;
  }
  SelectorInvocationDiagnostic diagnostic;
  diagnostic.selector = selector ? selector : "UNKNOWN";
  diagnostic.invocation_completed_normally = invocation_completed_normally;
  diagnostic.has_raw_return_code = invocation_completed_normally;
  diagnostic.raw_return_code = raw_return_code;
  diagnostic.host_result_code = host_result_code;
  diagnostic.seh_caught = seh_caught;
  diagnostic.seh_code = seh_code;
  diagnostic.global_data_handoff = handoff;
  diagnostic.effect_ref_at_entry = effect_ref;
  diagnostic.appl_id_at_entry = appl_id;
  diagnostic.version_at_entry = version;
  if (seh_caught) {
    if (g_current_fault_module_class)
      diagnostic.fault_module_class = g_current_fault_module_class;
    diagnostic.fault_module = g_telemetry.seh_module;
    diagnostic.has_plugin_rva = g_current_has_plugin_rva;
    diagnostic.plugin_rva = g_current_plugin_rva;
    if (g_current_access_violation.access_violation) {
      diagnostic.access_type =
          access_type_name(g_current_access_violation.access_type);
      diagnostic.has_fault_address =
          g_current_access_violation.has_fault_address;
      if (diagnostic.has_fault_address)
        diagnostic.fault_address =
            classify_pointer(g_current_access_violation.fault_address);
      diagnostic.has_register_snapshot =
          g_current_access_violation.has_registers;
      if (diagnostic.has_register_snapshot) {
        diagnostic.registers.reserve(kRegisterNames.size());
        for (std::size_t index = 0; index < kRegisterNames.size(); ++index) {
          diagnostic.registers.push_back({
              kRegisterNames[index],
              classify_pointer(g_current_access_violation.registers[index])});
        }
        for (std::size_t index = 0;
             index < kMaxSelectorStackValues; ++index) {
          diagnostic.stack_values[index].offset_bytes =
              static_cast<uint32_t>(index * sizeof(uintptr_t));
          diagnostic.stack_values[index].readable =
              g_current_access_violation.stack_readable[index];
          if (diagnostic.stack_values[index].readable)
            diagnostic.stack_values[index].value = classify_pointer(
                g_current_access_violation.stack_values[index]);
        }
      }
    }
  }
  g_telemetry.invocations.push_back(std::move(diagnostic));
}

void json_string(std::ostringstream& output, const std::string& value,
                 std::size_t maximum_bytes) {
  output << '"';
  std::size_t emitted{};
  for (unsigned char ch : value) {
    if (emitted == maximum_bytes) break;
    if (ch == '"' || ch == '\\') output << '\\';
    if (ch >= 0x20 && ch < 0x7f) {
      output << static_cast<char>(ch);
      ++emitted;
    }
  }
  output << '"';
}

void relative_offset(std::ostringstream& output, bool present,
                     uint64_t value) {
  if (!present) {
    output << "null";
    return;
  }
  output << "\"0x" << std::hex << std::setw(16) << std::setfill('0')
         << value << std::dec << '"';
}

void pointer_classification_json(
    std::ostringstream& output,
    const PointerClassificationDiagnostic& value) {
  output << "{\"classification\":";
  json_string(output, value.classification, 32);
  output << ",\"module\":";
  if (value.module.empty())
    output << "null";
  else
    json_string(output, value.module, 260);
  output << ",\"relative_offset\":";
  relative_offset(output, value.has_relative_offset, value.relative_offset);
  output << ",\"token\":";
  if (value.token.empty())
    output << "null";
  else
    json_string(output, value.token, 20);
  output << '}';
}

void global_data_state_json(std::ostringstream& output,
                            const GlobalDataStateDiagnostic& value) {
  if (!value.captured) {
    output << "null";
    return;
  }
  output << "{\"state\":\"" << (value.is_null ? "null" : "non_null")
         << "\",\"classification\":";
  json_string(output, value.classification, 32);
  output << ",\"process_local_token\":";
  if (value.process_local_token.empty())
    output << "null";
  else
    json_string(output, value.process_local_token, 20);
  output << '}';
}

bool callback_timeline_selector(const char* selector) noexcept {
  return selector &&
      (std::strcmp(selector, "GLOBAL_SETUP") == 0 ||
       std::strcmp(selector, "PARAMS_SETUP") == 0);
}

bool safe_callback_id(const char* callback, std::size_t& length) noexcept {
  length = 0;
  if (!callback) return false;
  for (; callback[length] && length <= 64; ++length) {
    const unsigned char ch = static_cast<unsigned char>(callback[length]);
    if (!(std::isalnum(ch) || ch == '.' || ch == '_' || ch == '-'))
      return false;
  }
  return length > 0 && length <= 64 && callback[length] == '\0';
}

const char* callback_classification_name(
    HostCallbackClassification classification) noexcept {
  switch (classification) {
    case HostCallbackClassification::implemented: return "implemented";
    case HostCallbackClassification::unsupported: return "unsupported";
    case HostCallbackClassification::fallback: return "fallback";
  }
  return "unsupported";
}

const char* extended_lookup_string_table_state_name(
    ExtendedLookupStringTableState state) noexcept {
  switch (state) {
    case ExtendedLookupStringTableState::valid: return "valid";
    case ExtendedLookupStringTableState::none: return "none";
    case ExtendedLookupStringTableState::invalid: return "invalid";
  }
  return "invalid";
}

const char* extended_lookup_opaque_table_classification_name(
    ExtendedLookupOpaqueTableClassification classification) noexcept {
  switch (classification) {
    case ExtendedLookupOpaqueTableClassification::null: return "null";
    case ExtendedLookupOpaqueTableClassification::active_effect_module:
      return "active_effect_module";
    case ExtendedLookupOpaqueTableClassification::active_resource_module:
      return "active_resource_module";
    case ExtendedLookupOpaqueTableClassification::other_loaded_sealed_module:
      return "other_loaded_sealed_module";
    case ExtendedLookupOpaqueTableClassification::other_loaded_system_module:
      return "other_loaded_system_module";
    case ExtendedLookupOpaqueTableClassification::unrecognized:
      return "unrecognized";
  }
  return "unrecognized";
}

const char* extended_lookup_outcome_name(
    ExtendedLookupOutcome outcome) noexcept {
  switch (outcome) {
    case ExtendedLookupOutcome::found: return "found";
    case ExtendedLookupOutcome::missing: return "missing";
    case ExtendedLookupOutcome::invalid: return "invalid";
  }
  return "invalid";
}

void record_extended_allocation_boundary(
    const char* selector, bool entry) noexcept {
  std::size_t selector_length{};
  if (!safe_callback_id(selector, selector_length)) {
    g_extended_allocations.truncated = true;
    return;
  }
  if (g_extended_allocations.records.size() >=
      kMaxExtendedAllocationTimelineRecords) {
    g_extended_allocations.truncated = true;
    return;
  }
  try {
    ExtendedAllocationTimelineRecord record;
    record.sequence = g_extended_allocations.next_sequence++;
    record.selector.assign(selector, selector_length);
    record.entry = entry;
    record.new_allocations = g_extended_allocations.phase_new_allocations;
    record.frees = g_extended_allocations.phase_frees;
    record.invalid_frees = g_extended_allocations.phase_invalid_frees;
    record.double_frees = g_extended_allocations.phase_double_frees;
    record.allocations.reserve(g_extended_allocations.tracked.size());
    for (const auto& allocation : g_extended_allocations.tracked) {
      if (allocation.live) {
        ++record.live_allocation_count;
        if (allocation.owner_selector == "GLOBAL_SETUP")
          ++record.global_setup_live_allocation_count;
      }
      record.allocations.push_back({
          allocation.token, allocation.owner_selector, allocation.live});
    }
    g_extended_allocations.records.push_back(std::move(record));
  } catch (...) {
    g_extended_allocations.truncated = true;
  }
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

void reset_selector_return_message() noexcept { g_telemetry.return_message = {}; }

SelectorDispatchTelemetry& selector_dispatch_telemetry() noexcept {
  return g_telemetry;
}

void* active_selector_module() noexcept {
  return g_active_entry_module;
}

HostCallbackTimelineTelemetry& host_callback_timeline_telemetry() noexcept {
  return g_host_callback_timeline;
}

void reset_host_callback_timeline() noexcept {
  g_host_callback_timeline.records.clear();
  g_host_callback_timeline.next_sequence = 0;
  g_host_callback_timeline.truncated = false;
}

const char* set_host_callback_timeline_selector(
    const char* selector) noexcept {
  const char* previous = g_host_callback_selector;
  g_host_callback_selector = selector;
  return previous;
}

void record_host_callback_invocation(
    const char* callback, int32_t return_code,
    HostCallbackClassification classification) noexcept {
  if (!callback_timeline_selector(g_host_callback_selector)) return;
  std::size_t callback_length{};
  if (!safe_callback_id(callback, callback_length)) {
    g_host_callback_timeline.truncated = true;
    return;
  }
  const uint32_t sequence = g_host_callback_timeline.next_sequence++;
  const bool success = return_code == 0;
  if (!g_host_callback_timeline.records.empty()) {
    auto& previous = g_host_callback_timeline.records.back();
    if (previous.callback.size() == callback_length &&
        std::memcmp(previous.callback.data(), callback, callback_length) == 0 &&
        previous.selector == g_host_callback_selector &&
        previous.success == success &&
        previous.return_code == return_code &&
        previous.classification == classification) {
      if (previous.call_count != UINT32_MAX)
        ++previous.call_count;
      else
        g_host_callback_timeline.truncated = true;
      return;
    }
  }
  if (g_host_callback_timeline.records.size() >=
      kMaxHostCallbackTimelineRecords) {
    g_host_callback_timeline.truncated = true;
    return;
  }
  try {
    g_host_callback_timeline.records.push_back({
        sequence, std::string(callback, callback_length),
        g_host_callback_selector, 1, success, return_code, classification});
  } catch (...) {
    g_host_callback_timeline.truncated = true;
  }
}

ExtendedLookupTimelineTelemetry&
extended_lookup_timeline_telemetry() noexcept {
  return g_extended_lookup_timeline;
}

void reset_extended_lookup_diagnostics() noexcept {
  g_extended_lookup_timeline = {};
}

ExtendedLookupOpaqueTableClassification classify_extended_lookup_table(
    const void* table, void* active_effect_module,
    void* active_resource_module,
    ExtendedLookupOtherModuleClassifier classify_other_module) noexcept {
  if (!table) return ExtendedLookupOpaqueTableClassification::null;
  MEMORY_BASIC_INFORMATION memory{};
  if (VirtualQuery(table, &memory, sizeof(memory)) != sizeof(memory) ||
      memory.State != MEM_COMMIT || memory.Type != MEM_IMAGE ||
      !memory.AllocationBase)
    return ExtendedLookupOpaqueTableClassification::unrecognized;
  HMODULE module{};
  if (!GetModuleHandleExW(
          GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
              GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
          reinterpret_cast<LPCWSTR>(table), &module) ||
      module != static_cast<HMODULE>(memory.AllocationBase))
    return ExtendedLookupOpaqueTableClassification::unrecognized;
  if (module == static_cast<HMODULE>(active_effect_module))
    return ExtendedLookupOpaqueTableClassification::active_effect_module;
  if (module == static_cast<HMODULE>(active_resource_module))
    return ExtendedLookupOpaqueTableClassification::active_resource_module;
  if (!classify_other_module)
    return ExtendedLookupOpaqueTableClassification::unrecognized;
  const ExtendedLookupOpaqueTableClassification classification =
      classify_other_module(module);
  return classification ==
                 ExtendedLookupOpaqueTableClassification::
                     other_loaded_sealed_module ||
             classification ==
                 ExtendedLookupOpaqueTableClassification::
                     other_loaded_system_module
         ? classification
         : ExtendedLookupOpaqueTableClassification::unrecognized;
}

void record_extended_lookup_diagnostic(
    ExtendedLookupOpaqueTableClassification opaque_table_classification,
    ExtendedLookupStringTableState raw_private_table_state,
    ExtendedLookupStringTableState windows_resource_source_state,
    int32_t lookup_id,
    ExtendedLookupOutcome outcome, int32_t return_code) noexcept {
  if (!callback_timeline_selector(g_host_callback_selector)) return;
  const uint32_t sequence = g_extended_lookup_timeline.next_sequence++;
  if (!g_extended_lookup_timeline.records.empty()) {
    auto& previous = g_extended_lookup_timeline.records.back();
    if (previous.selector == g_host_callback_selector &&
        previous.opaque_table_classification ==
            opaque_table_classification &&
        previous.raw_private_table_state == raw_private_table_state &&
        previous.windows_resource_source_state ==
            windows_resource_source_state &&
        previous.lookup_id == lookup_id && previous.outcome == outcome &&
        previous.return_code == return_code) {
      if (previous.call_count != UINT32_MAX)
        ++previous.call_count;
      else
        g_extended_lookup_timeline.truncated = true;
      return;
    }
  }
  if (g_extended_lookup_timeline.records.size() >=
      kMaxExtendedLookupTimelineRecords) {
    g_extended_lookup_timeline.truncated = true;
    return;
  }
  try {
    g_extended_lookup_timeline.records.push_back(
        {sequence, g_host_callback_selector, 1, opaque_table_classification,
         raw_private_table_state, windows_resource_source_state, lookup_id,
         outcome, return_code});
  } catch (...) {
    g_extended_lookup_timeline.truncated = true;
  }
}

void reset_extended_allocation_diagnostics() noexcept {
  g_extended_allocations = {};
}

void observe_extended_allocation(void* allocation) noexcept {
  if (!allocation) return;
  const uintptr_t pointer = reinterpret_cast<uintptr_t>(allocation);
  auto existing = std::find_if(
      g_extended_allocations.tracked.begin(),
      g_extended_allocations.tracked.end(),
      [pointer](const auto& value) { return value.pointer == pointer; });
  if (existing != g_extended_allocations.tracked.end()) {
    if (existing->live) {
      g_extended_allocations.truncated = true;
      return;
    }
    existing->owner_selector =
        g_host_callback_selector ? g_host_callback_selector : "HOST";
    existing->live = true;
    ++g_extended_allocations.phase_new_allocations;
    return;
  }
  if (g_extended_allocations.tracked.size() >=
      kMaxExtendedAllocationTimelineRecords) {
    g_extended_allocations.truncated = true;
    return;
  }
  try {
    g_extended_allocations.tracked.push_back({
        pointer, pointer_token(pointer),
        g_host_callback_selector ? g_host_callback_selector : "HOST", true});
    ++g_extended_allocations.phase_new_allocations;
  } catch (...) {
    g_extended_allocations.truncated = true;
  }
}

void observe_extended_free(void* allocation) noexcept {
  if (!allocation) return;
  const uintptr_t pointer = reinterpret_cast<uintptr_t>(allocation);
  const auto existing = std::find_if(
      g_extended_allocations.tracked.begin(),
      g_extended_allocations.tracked.end(),
      [pointer](const auto& value) { return value.pointer == pointer; });
  if (existing == g_extended_allocations.tracked.end()) {
    ++g_extended_allocations.phase_invalid_frees;
    return;
  }
  if (!existing->live) {
    ++g_extended_allocations.phase_double_frees;
    return;
  }
  existing->live = false;
  ++g_extended_allocations.phase_frees;
}

void record_extended_allocation_selector_entry(
    const char* selector) noexcept {
  g_extended_allocations.phase_new_allocations = 0;
  g_extended_allocations.phase_frees = 0;
  g_extended_allocations.phase_invalid_frees = 0;
  g_extended_allocations.phase_double_frees = 0;
  record_extended_allocation_boundary(selector, true);
}

void record_extended_allocation_selector_exit(
    const char* selector) noexcept {
  record_extended_allocation_boundary(selector, false);
}

std::string selector_invocations_report_json() {
  std::ostringstream output;
  output << ",\"selector_invocations\":{\"maximum_records\":"
         << kMaxSelectorInvocationDiagnostics << ",\"records\":[";
  for (std::size_t index = 0; index < g_telemetry.invocations.size(); ++index) {
    if (index) output << ',';
    const SelectorInvocationDiagnostic& diagnostic =
        g_telemetry.invocations[index];
    output << "{\"selector\":";
    json_string(output, diagnostic.selector, 64);
    output << ",\"invocation_completed_normally\":"
           << (diagnostic.invocation_completed_normally ? "true" : "false")
           << ",\"raw_return_code\":";
    if (diagnostic.has_raw_return_code)
      output << diagnostic.raw_return_code;
    else
      output << "null";
    output << ",\"host_result_code\":" << diagnostic.host_result_code
           << ",\"seh_caught\":"
           << (diagnostic.seh_caught ? "true" : "false")
           << ",\"seh_code\":";
    if (diagnostic.seh_caught)
      output << diagnostic.seh_code;
    else
      output << "null";
    output << ",\"fault_module_class\":";
    if (diagnostic.fault_module_class.empty())
      output << "null";
    else
      json_string(output, diagnostic.fault_module_class, 32);
    output << ",\"fault_module\":";
    if (diagnostic.fault_module.empty())
      output << "null";
    else
      json_string(output, diagnostic.fault_module, 260);
    output << ",\"plugin_rva\":";
    if (diagnostic.has_plugin_rva) {
      output << "\"0x" << std::hex << std::setw(16) << std::setfill('0')
             << diagnostic.plugin_rva << std::dec << '"';
    } else {
      output << "null";
    }
    output << ",\"access_type\":";
    if (diagnostic.access_type.empty())
      output << "null";
    else
      json_string(output, diagnostic.access_type, 8);
    output << ",\"fault_address\":";
    if (diagnostic.has_fault_address)
      pointer_classification_json(output, diagnostic.fault_address);
    else
      output << "null";
    output << ",\"registers\":";
    if (!diagnostic.has_register_snapshot) {
      output << "null";
    } else {
      output << '{';
      for (std::size_t register_index = 0;
           register_index < diagnostic.registers.size(); ++register_index) {
        if (register_index) output << ',';
        json_string(output, diagnostic.registers[register_index].name, 8);
        output << ':';
        pointer_classification_json(
            output, diagnostic.registers[register_index].value);
      }
      output << '}';
    }
    output << ",\"stack_pointer_values\":";
    if (!diagnostic.has_register_snapshot) {
      output << "null";
    } else {
      output << '[';
      for (std::size_t stack_index = 0;
           stack_index < diagnostic.stack_values.size(); ++stack_index) {
        if (stack_index) output << ',';
        const StackValueClassificationDiagnostic& stack_value =
            diagnostic.stack_values[stack_index];
        output << "{\"offset_bytes\":" << stack_value.offset_bytes
               << ",\"value\":";
        if (stack_value.readable)
          pointer_classification_json(output, stack_value.value);
        else
          output << "null";
        output << '}';
      }
      output << ']';
    }
    output << ",\"global_data_handoff\":{\"input_at_entry\":";
    global_data_state_json(
        output, diagnostic.global_data_handoff.input_at_entry);
    output << ",\"output_after_return\":";
    global_data_state_json(
        output, diagnostic.global_data_handoff.output_after_return);
    output << ",\"same_identity_as_previous_output\":";
    if (diagnostic.global_data_handoff.has_previous_output)
      output << (diagnostic.global_data_handoff
                         .same_identity_as_previous_output
                     ? "true"
                     : "false");
    else
      output << "null";
    output << '}';
    output << ",\"effect_ref_at_entry\":";
    if (!diagnostic.effect_ref_at_entry.state.captured) {
      output << "null";
    } else {
      output << "{\"state\":\""
             << (diagnostic.effect_ref_at_entry.state.is_null
                     ? "null"
                     : "non_null")
             << "\",\"classification\":";
      json_string(
          output, diagnostic.effect_ref_at_entry.state.classification, 32);
      output << ",\"process_local_token\":";
      if (diagnostic.effect_ref_at_entry.state.process_local_token.empty())
        output << "null";
      else
        json_string(
            output,
            diagnostic.effect_ref_at_entry.state.process_local_token, 20);
      output << ",\"same_identity_as_global_setup_entry\":";
      if (diagnostic.effect_ref_at_entry.has_global_setup_entry)
        output << (diagnostic.effect_ref_at_entry
                           .same_identity_as_global_setup_entry
                       ? "true"
                       : "false");
      else
        output << "null";
      output << '}';
    }
    output << ",\"appl_id_at_entry\":";
    if (!diagnostic.appl_id_at_entry.captured) {
      output << "null";
    } else {
      output << "{\"printable_code\":";
      json_string(
          output, diagnostic.appl_id_at_entry.printable_code, 16);
      output << ",\"hex_u32\":\"0x" << std::hex << std::setw(8)
             << std::setfill('0') << diagnostic.appl_id_at_entry.value
             << std::dec
             << "\",\"same_value_as_global_setup_entry\":";
      if (diagnostic.appl_id_at_entry.has_global_setup_entry)
        output << (diagnostic.appl_id_at_entry
                           .same_value_as_global_setup_entry
                       ? "true"
                       : "false");
      else
        output << "null";
      output
          << ",\"host_setting_source\":\"worker_effect_bootstrap\"}";
    }
    output << ",\"version_at_entry\":";
    if (!diagnostic.version_at_entry.captured) {
      output << "null";
    } else {
      output << "{\"raw_packed_u32\":\"0x" << std::hex << std::setw(8)
             << std::setfill('0')
             << diagnostic.version_at_entry.raw_packed_value << std::dec
             << "\",\"major\":" << diagnostic.version_at_entry.major
             << ",\"minor\":" << diagnostic.version_at_entry.minor
             << ",\"same_value_as_global_setup_entry\":";
      if (diagnostic.version_at_entry.has_global_setup_entry)
        output << (diagnostic.version_at_entry
                           .same_value_as_global_setup_entry
                       ? "true"
                       : "false");
      else
        output << "null";
      output
          << ",\"host_setting_source\":\"worker_effect_bootstrap\"}";
    }
    output << '}';
  }
  output << "],\"truncated\":"
         << (g_telemetry.invocations_truncated ? "true" : "false") << '}';
  output << ",\"host_callback_timeline\":{\"maximum_records\":"
         << kMaxHostCallbackTimelineRecords << ",\"records\":[";
  for (std::size_t index = 0;
       index < g_host_callback_timeline.records.size(); ++index) {
    if (index) output << ',';
    const auto& record = g_host_callback_timeline.records[index];
    output << "{\"sequence\":" << record.sequence << ",\"callback\":";
    json_string(output, record.callback, 64);
    output << ",\"selector\":";
    json_string(output, record.selector, 32);
    output << ",\"call_count\":" << record.call_count
           << ",\"status\":\"" << (record.success ? "success" : "failure")
           << "\",\"return_code\":" << record.return_code
           << ",\"classification\":\""
           << callback_classification_name(record.classification) << "\"}";
  }
  output << "],\"truncated\":"
         << (g_host_callback_timeline.truncated ? "true" : "false") << '}';
  output << ",\"extended_lookup_timeline\":{\"maximum_records\":"
         << kMaxExtendedLookupTimelineRecords << ",\"records\":[";
  for (std::size_t index = 0;
       index < g_extended_lookup_timeline.records.size(); ++index) {
    if (index) output << ',';
    const auto& record = g_extended_lookup_timeline.records[index];
    output << "{\"sequence\":" << record.sequence << ",\"selector\":";
    json_string(output, record.selector, 32);
    output << ",\"call_count\":" << record.call_count
           << ",\"opaque_table_classification\":\""
           << extended_lookup_opaque_table_classification_name(
                  record.opaque_table_classification)
           << "\",\"raw_private_table_state\":\""
           << extended_lookup_string_table_state_name(
                  record.raw_private_table_state)
           << "\",\"windows_resource_source_state\":\""
           << extended_lookup_string_table_state_name(
                  record.windows_resource_source_state)
           << "\",\"lookup_id\":" << record.lookup_id
           << ",\"outcome\":\""
           << extended_lookup_outcome_name(record.outcome)
           << "\",\"return_code\":" << record.return_code << '}';
  }
  output << "],\"truncated\":"
         << (g_extended_lookup_timeline.truncated ? "true" : "false")
         << '}';
  output << ",\"extended_allocation_timeline\":{\"maximum_records\":"
         << kMaxExtendedAllocationTimelineRecords << ",\"records\":[";
  for (std::size_t index = 0;
       index < g_extended_allocations.records.size(); ++index) {
    if (index) output << ',';
    const auto& record = g_extended_allocations.records[index];
    output << "{\"sequence\":" << record.sequence << ",\"selector\":";
    json_string(output, record.selector, 32);
    output << ",\"boundary\":\"" << (record.entry ? "entry" : "exit")
           << "\",\"live_allocation_count\":"
           << record.live_allocation_count
           << ",\"new_allocations\":" << record.new_allocations
           << ",\"frees\":" << record.frees
           << ",\"invalid_frees\":" << record.invalid_frees
           << ",\"double_frees\":" << record.double_frees
           << ",\"global_setup_live_allocation_count\":"
           << record.global_setup_live_allocation_count
           << ",\"allocations\":[";
    for (std::size_t allocation_index = 0;
         allocation_index < record.allocations.size(); ++allocation_index) {
      if (allocation_index) output << ',';
      const auto& allocation = record.allocations[allocation_index];
      output << "{\"allocation_token\":";
      json_string(output, allocation.token, 20);
      output << ",\"state\":\""
             << (allocation.live ? "live" : "non_live")
             << "\",\"owner_selector\":";
      json_string(output, allocation.owner_selector, 32);
      output << '}';
    }
    output << "]}";
  }
  output << "],\"truncated\":"
         << (g_extended_allocations.truncated ? "true" : "false") << '}';
  return output.str();
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

int32_t invoke_audited_effect_call_seh(
    EffectEntry entry, int32_t command, void* input, void* output,
    void** params, void* world, void* extra,
    bool* invocation_completed_normally, int32_t* raw_return_code,
    uint32_t* out_exception_code, const char* selector) {
  int32_t result = 0;
  __try {
    result = audited_effect_call(
        entry, command, input, output, params, world, extra,
        invocation_completed_normally, raw_return_code);
  } __except(capture_seh_exception(GetExceptionInformation())) {
    *out_exception_code = GetExceptionCode();
    g_telemetry.selector = selector;
    g_telemetry.error = kAuditFailure;
    if (g_capture_audit) g_capture_audit();
    result = kAuditFailure;
  }
  return result;
}

int32_t invoke_entry_seh(EffectEntry entry, int32_t command, void* input,
                         void* output, void** params, void* world, void* extra,
                         uint32_t* out_exception_code) {
  if (!out_exception_code) return kAuditFailure;
  *out_exception_code = 0;
  g_telemetry.missing_dependency.clear();
  // The buffer is cleared before the selector runs. The `PF_OutData` lives for
  // the whole session and is reused across frames and, through the cluster
  // swap, across plug-ins, so text left by an earlier selector would otherwise
  // be re-read and attributed to this one (issue #707). The telemetry is not
  // cleared here - a plug-in often writes its reason from an earlier selector
  // than the one that finally fails - it is cleared per frame by
  // `reset_selector_return_message`.
  if (output)
    std::memset(static_cast<unsigned char*>(output) + kOutReturnMsgOffset, 0,
                kOutReturnMsgSize);
  const char* selector = effect_selector_name(command);
  const char* previous_suite_selector = set_suite_timeline_selector(selector);
  if (command == 1) {
    reset_host_callback_timeline();
    reset_extended_lookup_diagnostics();
    reset_extended_allocation_diagnostics();
    compute_cache::reset_telemetry();
  }
  const char* previous_callback_selector =
      set_host_callback_timeline_selector(selector);
  record_extended_allocation_selector_entry(selector);
  const uintptr_t entry_address = reinterpret_cast<uintptr_t>(entry);
  g_active_entry_module =
      module_from_address(reinterpret_cast<const void*>(entry_address));
  if (command == 1) {
    g_has_previous_global_data_output = false;
    g_previous_global_data_output = 0;
    g_has_global_setup_effect_ref_entry = false;
    g_global_setup_effect_ref_entry = 0;
    g_has_global_setup_application_id_entry = false;
    g_global_setup_application_id_entry = 0;
    g_has_global_setup_spec_version_entry = false;
    g_global_setup_spec_version_entry = 0;
  }
  const CapturedGlobalDataState effect_ref_at_entry =
      capture_global_data_state(input, kInEffectRefOffset);
  EffectRefEntryDiagnostic effect_ref;
  effect_ref.state = effect_ref_at_entry.diagnostic;
  effect_ref.has_global_setup_entry =
      command != 1 && g_has_global_setup_effect_ref_entry &&
      effect_ref_at_entry.diagnostic.captured;
  effect_ref.same_identity_as_global_setup_entry =
      effect_ref.has_global_setup_entry &&
      effect_ref_at_entry.pointer == g_global_setup_effect_ref_entry;
  if (command == 1 && effect_ref_at_entry.diagnostic.captured) {
    g_has_global_setup_effect_ref_entry = true;
    g_global_setup_effect_ref_entry = effect_ref_at_entry.pointer;
  }
  const CapturedApplicationId captured_appl_id =
      capture_application_id(input);
  ApplicationIdEntryDiagnostic appl_id;
  appl_id.captured = captured_appl_id.captured;
  appl_id.value = captured_appl_id.value;
  appl_id.printable_code = captured_appl_id.printable_code;
  appl_id.has_global_setup_entry =
      command != 1 && g_has_global_setup_application_id_entry &&
      captured_appl_id.captured;
  appl_id.same_value_as_global_setup_entry =
      appl_id.has_global_setup_entry &&
      captured_appl_id.value == g_global_setup_application_id_entry;
  if (command == 1 && captured_appl_id.captured) {
    g_has_global_setup_application_id_entry = true;
    g_global_setup_application_id_entry = captured_appl_id.value;
  }
  const CapturedSpecVersion captured_version =
      capture_spec_version(input);
  SpecVersionEntryDiagnostic version;
  version.captured = captured_version.captured;
  version.raw_packed_value = captured_version.raw_packed_value;
  version.major = captured_version.major;
  version.minor = captured_version.minor;
  version.has_global_setup_entry =
      command != 1 && g_has_global_setup_spec_version_entry &&
      captured_version.captured;
  version.same_value_as_global_setup_entry =
      version.has_global_setup_entry &&
      captured_version.raw_packed_value ==
          g_global_setup_spec_version_entry;
  if (command == 1 && captured_version.captured) {
    g_has_global_setup_spec_version_entry = true;
    g_global_setup_spec_version_entry =
        captured_version.raw_packed_value;
  }
  const CapturedGlobalDataState input_global_data =
      capture_global_data_state(input, kInGlobalDataOffset);
  GlobalDataHandoffDiagnostic handoff;
  handoff.input_at_entry = input_global_data.diagnostic;
  handoff.has_previous_output =
      g_has_previous_global_data_output &&
      input_global_data.diagnostic.captured;
  handoff.same_identity_as_previous_output =
      handoff.has_previous_output &&
      input_global_data.pointer == g_previous_global_data_output;
  g_current_fault_module_class = nullptr;
  g_current_plugin_rva = 0;
  g_current_has_plugin_rva = false;
  g_current_access_violation = {};
  bool invocation_completed_normally = false;
  int32_t raw_return_code = 0;
  const int32_t result = invoke_audited_effect_call_seh(
      entry, command, input, output, params, world, extra,
      &invocation_completed_normally, &raw_return_code, out_exception_code,
      selector);
  record_extended_allocation_selector_exit(selector);
  // The buffer was cleared before the call, so whatever is in it now was
  // written by this selector. It is recorded whatever this selector returned:
  // the plug-in that says "Couldn't load suite." often says it from an earlier
  // selector than the one that finally reports the frame error, and the pair
  // (selector, its own error) is what makes the message readable rather than
  // misattributed. The latest one wins, so it stays the most recent thing the
  // plug-in said.
  if (std::string message = captured_return_message(output); !message.empty()) {
    uint32_t out_flags = 0;
    std::memcpy(&out_flags, static_cast<const unsigned char*>(output) + kOutFlagsOffset,
                sizeof(out_flags));
    g_telemetry.return_message = {selector ? selector : "", std::move(message), result,
                                  (out_flags & kOutFlagDisplayErrorMessage) != 0};
  }
  const CapturedGlobalDataState output_global_data =
      capture_global_data_state(output, kOutGlobalDataOffset);
  handoff.output_after_return = output_global_data.diagnostic;
  record_selector_invocation(
      selector, invocation_completed_normally, raw_return_code, result,
      *out_exception_code != 0, *out_exception_code, handoff, effect_ref,
      appl_id, version);
  g_has_previous_global_data_output =
      output_global_data.diagnostic.captured;
  g_previous_global_data_output = output_global_data.pointer;
  g_active_entry_module = nullptr;
  set_host_callback_timeline_selector(previous_callback_selector);
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
