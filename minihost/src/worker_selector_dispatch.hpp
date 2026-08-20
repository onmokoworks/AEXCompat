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
// Frames kept from the faulting context's unwind (innermost first, frame 0 is
// the fault site). Bounded because it is captured inside an SEH filter on a
// stack a malformed plug-in may have corrupted.
constexpr std::size_t kMaxSelectorUnwindFrames = 12;
constexpr std::size_t kMaxHostCallbackTimelineRecords = 128;
constexpr std::size_t kMaxExtendedLookupTimelineRecords = 128;
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

enum class ExtendedLookupStringTableState : uint8_t {
  valid,
  none,
  invalid,
};

enum class ExtendedLookupOpaqueTableClassification : uint8_t {
  null,
  active_effect_module,
  active_resource_module,
  other_loaded_sealed_module,
  other_loaded_system_module,
  unrecognized,
};

using ExtendedLookupOtherModuleClassifier =
    ExtendedLookupOpaqueTableClassification(*)(void* module) noexcept;

enum class ExtendedLookupOutcome : uint8_t {
  found,
  missing,
  invalid,
};

struct ExtendedLookupTimelineRecord {
  uint32_t sequence{};
  std::string selector;
  uint32_t call_count{};
  ExtendedLookupOpaqueTableClassification opaque_table_classification{
      ExtendedLookupOpaqueTableClassification::unrecognized};
  ExtendedLookupStringTableState raw_private_table_state{
      ExtendedLookupStringTableState::none};
  ExtendedLookupStringTableState windows_resource_source_state{
      ExtendedLookupStringTableState::none};
  int32_t lookup_id{};
  ExtendedLookupOutcome outcome{ExtendedLookupOutcome::missing};
  int32_t return_code{};
};

struct ExtendedLookupTimelineTelemetry {
  uint32_t next_sequence{};
  std::vector<ExtendedLookupTimelineRecord> records;
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

/// Why the faulting context's unwind stopped. A capped or aborted walk must not
/// read like a complete chain: the cap is the ordinary outcome (a fault deep in
/// a plug-in reaches it every time), so the outermost frame printed is normally
/// not the top of the stack.
enum class SelectorUnwindStop : uint8_t {
  /// The walk was not attempted.
  not_attempted,
  /// Reached the outermost frame: the unwind produced a null instruction
  /// pointer, which is where a Windows thread's frame chain ends.
  end_of_chain,
  /// The walk stopped making table-backed progress, so the chain was lost
  /// part-way: either the unwind stopped moving toward the stack base, or the
  /// fault site's return slot held null. A malformed plug-in's unwind data and
  /// a smashed return slot both land here, and the outermost frame printed is
  /// NOT the top of the stack - which is why this does not share a name with
  /// end_of_chain.
  chain_lost,
  /// Stopped at kMaxSelectorUnwindFrames with frames left above.
  frame_cap,
  /// A frame behind the fault site had no unwind entry, so the chain was lost.
  no_unwind_entry,
  /// The fault site had no unwind entry and the return address it pushed could
  /// not be read back at all (a null that reads back fine is chain_lost).
  return_slot_unreadable,
  /// Reading the stack faulted; the frames recorded before that still stand.
  walk_faulted,
  /// Skipped: the faulting frame sat too close to the thread's stack limit for
  /// the walk to run without risking a second, uncontainable fault.
  low_stack,
};

const char* selector_unwind_stop_name(SelectorUnwindStop stop) noexcept;

/// One frame of the faulting context's unwind. `from_return_slot` marks the
/// single frame that is not table-derived: when the fault site itself has no
/// unwind entry (a call through a null or garbage slot), the frame behind it is
/// the return address read off RSP, which is a caller only if the fault really
/// was a call.
struct UnwindFrameDiagnostic {
  PointerClassificationDiagnostic site;
  bool from_return_slot{};
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
  std::array<UnwindFrameDiagnostic,
             kMaxSelectorUnwindFrames> unwind_frames{};
  std::size_t unwind_frame_count{};
  SelectorUnwindStop unwind_stop{SelectorUnwindStop::not_attempted};
  bool has_register_snapshot{};
  GlobalDataHandoffDiagnostic global_data_handoff;
  EffectRefEntryDiagnostic effect_ref_at_entry;
  ApplicationIdEntryDiagnostic appl_id_at_entry;
  SpecVersionEntryDiagnostic version_at_entry;
};

/// What a selector left in `PF_OutData::return_msg`. The SDK's own
/// `AEFX_AcquireSuite` writes there when a suite cannot be acquired, and
/// plug-ins write there to say why they failed, so the text is often the whole
/// diagnosis (issue #707). The buffer is reused across selectors, so the
/// selector that wrote it has to be recorded with it.
struct SelectorReturnMessage {
  std::string selector;
  std::string text;
  int32_t error{};
  bool display_requested{};

  bool empty() const noexcept { return text.empty(); }
};

struct SelectorDispatchTelemetry {
  // Selector return codes this host substituted for the plug-in's own since
  // the worker started, all selectors together: a fault the SEH boundary
  // caught, a C++ exception that escaped the entry point, a module audit the
  // call did not pass, and the guards that refuse to dispatch at all. Every
  // one of them surfaces as `kAuditFailure` (512), which is indistinguishable
  // from a plug-in that returned 512 itself. Monotonic
  // and never reset, so a caller snapshots it around a single selector call
  // and compares afterwards to know which of the two it got (the invocation
  // list below is capped and cannot answer that once the cap is reached).
  // Not atomic, and neither are this struct's other fields: smart selector
  // dispatch is driven serially from one frame loop, so the snapshot and the
  // increments it is compared against are the same thread's. A second thread
  // incrementing inside somebody's snapshot window can only make that window
  // look substituted when it was not, which suppresses a fallback route - it
  // can never present a faulted selector's 512 as the plug-in's own.
  uint64_t substituted_selector_failures{};
  uint32_t seh_code{};
  uint64_t seh_address{};
  std::string seh_module;
  std::string selector;
  std::string missing_dependency;
  SelectorReturnMessage return_message;
  int32_t error{};
  std::vector<SelectorInvocationDiagnostic> invocations;
  bool invocations_truncated{};
};

/// What the unwind self-test observed.
struct SelectorFaultUnwindProbe {
  bool passed{};
  /// Frames the capture recorded for the injected fault.
  std::size_t frame_count{};
  /// Frame 0 was the null fault site.
  bool fault_site_is_null{};
  /// Frame 1 resolved to the function that made the null call, and frame 2 to
  /// its caller - identified by unwind-table entry, not merely by module.
  bool call_site_frame_identified{};
  bool caller_frame_identified{};
  /// Both reference functions resolved to an unwind-table entry. False means
  /// the comparison could not be made (an incremental-link thunk, say), not
  /// that the walk was wrong.
  bool reference_identities_resolved{};
};

/// Behavioural check that the faulting-context unwind recovers a caller chain
/// through a call to a null slot - the shape a plug-in reaching an
/// uninitialised Adobe-library dispatch table produces (issue #1264). Raises
/// the fault behind the same SEH capture the selector dispatch uses.
SelectorFaultUnwindProbe verify_selector_fault_unwind() noexcept;

void configure_selector_dispatch_audit(AuditCapture capture,
                                       AuditPassed passed) noexcept;
void configure_selector_dispatch_trace(SelectorDispatchTrace trace) noexcept;
SelectorDispatchTelemetry& selector_dispatch_telemetry() noexcept;
/// Forgets the last `PF_OutData::return_msg` a selector wrote. Called once per
/// frame and on a cluster plug-in swap, so a frame never reports what a
/// previous frame - or a previous plug-in - said (issue #707).
void reset_selector_return_message() noexcept;
void* active_selector_module() noexcept;
HostCallbackTimelineTelemetry& host_callback_timeline_telemetry() noexcept;
void reset_host_callback_timeline() noexcept;
const char* set_host_callback_timeline_selector(
    const char* selector) noexcept;
void record_host_callback_invocation(
    const char* callback, int32_t return_code,
    HostCallbackClassification classification) noexcept;
ExtendedLookupTimelineTelemetry&
extended_lookup_timeline_telemetry() noexcept;
void reset_extended_lookup_diagnostics() noexcept;
ExtendedLookupOpaqueTableClassification classify_extended_lookup_table(
    const void* table, void* active_effect_module,
    void* active_resource_module,
    ExtendedLookupOtherModuleClassifier classify_other_module) noexcept;
void record_extended_lookup_diagnostic(
    ExtendedLookupOpaqueTableClassification opaque_table_classification,
    ExtendedLookupStringTableState raw_private_table_state,
    ExtendedLookupStringTableState windows_resource_source_state,
    int32_t lookup_id,
    ExtendedLookupOutcome outcome, int32_t return_code) noexcept;
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
