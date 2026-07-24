#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime {

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using AuditCapture = void(*)();
using AuditPassed = bool(*)();
using SelectorDispatchTrace = void(*)(const char* selector);

constexpr std::size_t kMaxSelectorInvocationDiagnostics = 64;
constexpr std::size_t kMaxSelectorStackValues = 6;
constexpr std::size_t kMaxHostCallbackTimelineRecords = 128;
constexpr std::size_t kMaxExtendedAllocationTimelineRecords = 128;

enum class HostCallbackClassification : uint8_t {
  implemented,
  unsupported,
  fallback,
};

struct HostCallbackTimelineRecord {
  uint32_t sequence{};
  std::string callback;
  std::string selector;
  uint32_t call_count{};
  bool success{};
  int32_t return_code{};
  HostCallbackClassification classification{
      HostCallbackClassification::implemented};
};

struct HostCallbackTimelineTelemetry {
  uint32_t next_sequence{};
  std::vector<HostCallbackTimelineRecord> records;
  bool truncated{};
};

struct PointerClassificationDiagnostic {
  std::string classification;
  std::string module;
  bool has_relative_offset{};
  uint64_t relative_offset{};
  std::string token;
};

struct RegisterClassificationDiagnostic {
  std::string name;
  PointerClassificationDiagnostic value;
};

struct StackValueClassificationDiagnostic {
  uint32_t offset_bytes{};
  bool readable{};
  PointerClassificationDiagnostic value;
};

struct GlobalDataStateDiagnostic {
  bool captured{};
  bool is_null{};
  std::string classification;
  std::string process_local_token;
};

struct GlobalDataHandoffDiagnostic {
  GlobalDataStateDiagnostic input_at_entry;
  GlobalDataStateDiagnostic output_after_return;
  bool has_previous_output{};
  bool same_identity_as_previous_output{};
};

struct EffectRefEntryDiagnostic {
  GlobalDataStateDiagnostic state;
  bool has_global_setup_entry{};
  bool same_identity_as_global_setup_entry{};
};

struct ApplicationIdEntryDiagnostic {
  bool captured{};
  uint32_t value{};
  std::string printable_code;
  bool has_global_setup_entry{};
  bool same_value_as_global_setup_entry{};
};

struct SpecVersionEntryDiagnostic {
  bool captured{};
  uint32_t raw_packed_value{};
  int16_t major{};
  int16_t minor{};
  bool has_global_setup_entry{};
  bool same_value_as_global_setup_entry{};
};

struct SelectorInvocationDiagnostic {
  std::string selector;
  bool invocation_completed_normally{};
  bool has_raw_return_code{};
  int32_t raw_return_code{};
  int32_t host_result_code{};
  bool seh_caught{};
  uint32_t seh_code{};
  std::string fault_module_class;
  std::string fault_module;
  bool has_plugin_rva{};
  uint64_t plugin_rva{};
  std::string access_type;
  bool has_fault_address{};
  PointerClassificationDiagnostic fault_address;
  std::vector<RegisterClassificationDiagnostic> registers;
  std::array<StackValueClassificationDiagnostic,
             kMaxSelectorStackValues> stack_values{};
  bool has_register_snapshot{};
  GlobalDataHandoffDiagnostic global_data_handoff;
  EffectRefEntryDiagnostic effect_ref_at_entry;
  ApplicationIdEntryDiagnostic appl_id_at_entry;
  SpecVersionEntryDiagnostic version_at_entry;
};

struct SelectorDispatchTelemetry {
  uint32_t seh_code{};
  uint64_t seh_address{};
  std::string seh_module;
  std::string selector;
  std::string missing_dependency;
  int32_t error{};
  std::vector<SelectorInvocationDiagnostic> invocations;
  bool invocations_truncated{};
};

void configure_selector_dispatch_audit(AuditCapture capture,
                                       AuditPassed passed) noexcept;
void configure_selector_dispatch_trace(SelectorDispatchTrace trace) noexcept;
SelectorDispatchTelemetry& selector_dispatch_telemetry() noexcept;
HostCallbackTimelineTelemetry& host_callback_timeline_telemetry() noexcept;
void reset_host_callback_timeline() noexcept;
const char* set_host_callback_timeline_selector(
    const char* selector) noexcept;
void record_host_callback_invocation(
    const char* callback, int32_t return_code,
    HostCallbackClassification classification) noexcept;
void reset_extended_allocation_diagnostics() noexcept;
void observe_extended_allocation(void* allocation) noexcept;
void observe_extended_free(void* allocation) noexcept;
void record_extended_allocation_selector_entry(
    const char* selector) noexcept;
void record_extended_allocation_selector_exit(
    const char* selector) noexcept;
std::string selector_invocations_report_json();
const char* effect_selector_name(int32_t command) noexcept;
int32_t invoke_entry_seh(EffectEntry entry, int32_t command, void* input,
                         void* output, void** params, void* world, void* extra,
                         uint32_t* out_exception_code);
int32_t guarded_effect_call(EffectEntry entry, int32_t command, void* input,
                            void* output, void** params, void* world, void* extra);
int32_t invoke_smart_pre_render_cleanup_seh(void(__cdecl* cleanup)(void*),
                                            void* data);

}  // namespace aexcompat::worker_runtime
