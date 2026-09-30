#pragma once

#include "worker_aegp_entry_guard.hpp"

#include <cstdint>

namespace aexcompat::l2_detail {

// Completion-report inputs captured from worker_main_impl's aegp_init locals.
// Value-only boundary: orchestration results and timeline-probe outcomes
// arrive as plain values; host-global AEGP counters are read by the owner TU
// through their runtime-state owners and cross-TU declarations.
struct AegpInitKeyframeProbeSnapshot {
  bool connected{};
  bool request_sent{};
  bool response_received{};
  bool response_valid{};
  uint32_t response_bytes{};
};

struct AegpInitAckProbeSnapshot {
  bool connected{};
  bool request_sent{};
  bool ack_received{};
  bool ack_valid{};
};

struct AegpInitCompletionInputs {
  int32_t init_error{};
  worker_runtime::aegp_entry_guard::FaultKind entry_fault{
      worker_runtime::aegp_entry_guard::FaultKind::none};
  uint32_t entry_exception_code{};
  bool entry_invoked{};
  uint32_t forced_suite_releases{};
  bool boundary_regression_mode{};
  int32_t event_error{};
  int32_t death_error{};
  bool global_refcon_nonnull{};
  uint32_t hooks_invoked{};
  uint32_t menu_hooks_invoked{};
  uint32_t death_hooks_invoked{};
  uint32_t command_hooks_invoked{};
  uint32_t command_handled_count{};
  int32_t idle_max_sleep{};
  bool module_audit_ok{};
  AegpInitKeyframeProbeSnapshot keyframe_pipe{};
  AegpInitAckProbeSnapshot seek_pipe{};
  AegpInitAckProbeSnapshot trim_pipe{};
  AegpInitAckProbeSnapshot switch_pipe{};
};

// Emits the aegp_init completion JSON and returns the pass verdict that
// worker_main_impl maps onto the process exit code.
bool emit_aegp_init_completion_report(const AegpInitCompletionInputs& inputs);
bool emit_aegp_borrowed_handle_report_selftest();

}  // namespace aexcompat::l2_detail
