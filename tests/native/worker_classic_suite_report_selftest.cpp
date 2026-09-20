#include "worker_aegp_utility_suite.hpp"
#include "worker_render_report.hpp"
#include "worker_suite_registry.hpp"

#include <cstddef>
#include <cstdint>
#include <iostream>
#include <sstream>
#include <string>

namespace {

std::string serialize_suite_state(
    aexcompat::worker_render_report::ClassicSubsystemDiagnostics diagnostics) {
  diagnostics.suite_balanced = false;
  diagnostics.suite_counts = {8, 7, 1, 1};
  diagnostics.live_suite_leases = "PF Handle Suite@2=1";
  diagnostics.handle_balanced = true;
  diagnostics.path_balanced = true;
  diagnostics.world_balanced = true;
  diagnostics.receipt_balanced = true;
  diagnostics.async_balanced = true;

  std::ostringstream output;
  aexcompat::worker_render_report::ReportSnapshot report(output);
  report.stream() << '{';
  aexcompat::worker_render_report::append_classic_subsystems(report, diagnostics);
  report.stream() << '}';
  aexcompat::worker_render_report::emit(report, output);
  return output.str();
}

std::string serialize_utility_state() {
  std::ostringstream output;
  aexcompat::worker_render_report::ReportSnapshot report(output);
  report.stream() << "{\"probe\":true";
  aexcompat::worker_render_report::append_utility_undo_groups(
      report,
      aexcompat::worker_render_report::capture_utility_undo_groups());
  report.stream() << '}';
  aexcompat::worker_render_report::emit(report, output);
  return output.str();
}

std::size_t occurrences(const std::string& value, const std::string& needle) {
  std::size_t count = 0;
  for (std::size_t position = 0;
       (position = value.find(needle, position)) != std::string::npos;
       position += needle.size()) {
    ++count;
  }
  return count;
}

}  // namespace

int main() {
  // #1182: a suite release with no matching acquire is contained by the registry
  // as a no-op (it returns rejection and touches no host state), so it is a
  // benign warning, not a session-failing fault. This pins that contract: the
  // rejected release is COUNTED as a diagnostic but never sets
  // suite_fault_observed, before or after.
  const uint32_t rejected_before =
      aexcompat::worker_runtime::suite_registry().rejected_release_count();
  auto clean_state =
      aexcompat::worker_render_report::capture_classic_subsystems();
  const int32_t rejected_release =
      aexcompat::worker_runtime::suite_registry().release(
          "AEGP Layer Mask Suite", 999, nullptr);
  const uint32_t rejected_after =
      aexcompat::worker_runtime::suite_registry().rejected_release_count();
  auto after_state =
      aexcompat::worker_render_report::capture_classic_subsystems();
  const std::string clean_warning = serialize_suite_state(clean_state);
  const std::string after_reject = serialize_suite_state(after_state);

  // The same guard is used by one-shot classic/smart completion, resident
  // terminal close, and the existing cluster-swap boundary. Exercise actual
  // Utility Suite state rather than a source-level assertion so both a leaked
  // StartUndoGroup and an EndUndoGroup underflow are fixed regressions.
  aexcompat::l2_detail::reset_utility_undo_group_statistics();
  const std::string clean_undo = serialize_utility_state();
  const bool clean_exit_contract =
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(0, 21) == 0 &&
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(0, 22) == 0;
  const int32_t open_result =
      aexcompat::l2_detail::start_undo_group("unclosed render group");
  const std::string open_undo = serialize_utility_state();
  const bool open_exit_contract = open_result == 0 &&
      !aexcompat::l2_detail::utility_undo_group_state_clean() &&
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(0, 21) == 21 &&
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(0, 22) == 22 &&
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(25, 21) == 25;
  aexcompat::l2_detail::reset_utility_undo_group_statistics();
  const int32_t underflow_result = aexcompat::l2_detail::end_undo_group();
  const std::string underflow_undo = serialize_utility_state();
  const bool underflow_exit_contract = underflow_result != 0 &&
      !aexcompat::l2_detail::utility_undo_group_state_clean() &&
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(0, 21) == 21 &&
      aexcompat::l2_detail::utility_undo_group_guarded_exit_code(0, 22) == 22;
  aexcompat::l2_detail::reset_utility_undo_group_statistics();
  const std::string key = "\"suite_fault_observed\":";
  const bool passed =
      // The registry rejected the unacquired release (the protection: a no-op).
      rejected_release != 0 &&
      // The rejection is still counted as a reproducible diagnostic.
      rejected_after == rejected_before + 1 &&
      // It is benign: suite_fault_observed stays false, before and after.
      !clean_state.suite_fault && !after_state.suite_fault &&
      clean_warning.find(key + "false") != std::string::npos &&
      after_reject.find(key + "false") != std::string::npos &&
      occurrences(clean_warning, key) == 1 && occurrences(after_reject, key) == 1 &&
      clean_warning.find("\"suite_acquires\":8") != std::string::npos &&
      clean_warning.find("\"suite_releases\":7") != std::string::npos &&
      clean_warning.find("\"live_suite_lease_count\":1") != std::string::npos &&
      clean_warning.find("\"live_suite_reference_count\":1") != std::string::npos &&
      clean_warning.find("\"live_suite_leases\":\"PF Handle Suite@2=1\"") !=
          std::string::npos &&
      clean_exit_contract && open_exit_contract && underflow_exit_contract &&
      clean_undo.find("\"utility_undo_groups\":{\"starts\":0,\"ends\":0,\"invalid_operations\":0,\"depth\":0,\"balanced\":true,\"operations_valid\":true}") !=
          std::string::npos &&
      open_undo.find("\"utility_undo_groups\":{\"starts\":1,\"ends\":0,\"invalid_operations\":0,\"depth\":1,\"balanced\":false,\"operations_valid\":true}") !=
          std::string::npos &&
      underflow_undo.find("\"utility_undo_groups\":{\"starts\":0,\"ends\":0,\"invalid_operations\":1,\"depth\":0,\"balanced\":true,\"operations_valid\":false}") !=
          std::string::npos;

  if (!passed) {
    std::cerr << "clean_warning=" << clean_warning << '\n'
              << "after_reject=" << after_reject << '\n'
              << "rejected_release=" << rejected_release
              << " rejected_before=" << rejected_before
              << " rejected_after=" << rejected_after << '\n'
              << "clean_undo=" << clean_undo << '\n'
              << "open_undo=" << open_undo << '\n'
              << "underflow_undo=" << underflow_undo << '\n';
  }

  std::cout << "{\"classic_suite_fault_report\":\""
            << (passed ? "passed" : "failed")
            << "\",\"rejected_release_is_benign_warning\":"
            << (passed ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}
