#include "worker_render_report.hpp"
#include "worker_suite_registry.hpp"

#include <cstddef>
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
  auto clean_state =
      aexcompat::worker_render_report::capture_classic_subsystems();
  const int32_t rejected_release =
      aexcompat::worker_runtime::suite_registry().release(
          "AEGP Layer Mask Suite", 999, nullptr);
  auto fault_state =
      aexcompat::worker_render_report::capture_classic_subsystems();
  const std::string clean_warning = serialize_suite_state(clean_state);
  const std::string actual_fault = serialize_suite_state(fault_state);
  const std::string key = "\"suite_fault_observed\":";
  const bool passed =
      rejected_release != 0 && !clean_state.suite_fault && fault_state.suite_fault &&
      clean_warning.find(key + "false") != std::string::npos &&
      actual_fault.find(key + "true") != std::string::npos &&
      occurrences(clean_warning, key) == 1 && occurrences(actual_fault, key) == 1 &&
      clean_warning.find("\"suite_acquires\":8") != std::string::npos &&
      clean_warning.find("\"suite_releases\":7") != std::string::npos &&
      clean_warning.find("\"live_suite_lease_count\":1") != std::string::npos &&
      clean_warning.find("\"live_suite_reference_count\":1") != std::string::npos &&
      clean_warning.find("\"live_suite_leases\":\"PF Handle Suite@2=1\"") !=
          std::string::npos;

  if (!passed) {
    std::cerr << "clean_warning=" << clean_warning << '\n'
              << "actual_fault=" << actual_fault << '\n';
  }

  std::cout << "{\"classic_suite_fault_report\":\""
            << (passed ? "passed" : "failed")
            << "\",\"false_and_true_mutations\":"
            << (passed ? "true" : "false") << "}\n";
  return passed ? 0 : 1;
}
