#include "worker_report.hpp"
#include "native_stdout_guard.hpp"

#include <cmath>
#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <fcntl.h>
#include <iostream>
#include <io.h>
#include <limits>
#include <string>

namespace {

struct ProtocolCapture {
  bool emitted{};
  bool restored{};
  std::string bytes;
};

void __cdecl ignore_invalid_parameter(const wchar_t*, const wchar_t*,
                                      const wchar_t*, unsigned int,
                                      std::uintptr_t) {}

ProtocolCapture capture_protocol_stdout(const std::string& document) {
  ProtocolCapture result;
  const int stdout_fd = _fileno(stdout);
  if (stdout_fd < 0 || std::fflush(stdout) != 0) return result;
  const int saved_stdout = _dup(stdout_fd);
  int pipe_fds[2]{-1, -1};
  if (saved_stdout < 0 || _pipe(pipe_fds, 32 * 1024, _O_BINARY) != 0) {
    if (saved_stdout >= 0) _close(saved_stdout);
    return result;
  }
  if (_dup2(pipe_fds[1], stdout_fd) != 0) {
    _close(saved_stdout);
    _close(pipe_fds[0]);
    _close(pipe_fds[1]);
    return result;
  }
  _close(pipe_fds[1]);

  result.emitted =
      aexcompat::worker_runtime::emit_protocol_stdout(document);
  result.restored = _dup2(saved_stdout, stdout_fd) == 0;
  _close(saved_stdout);

  char buffer[4096];
  int count{};
  while ((count = _read(pipe_fds[0], buffer, sizeof(buffer))) > 0) {
    result.bytes.append(buffer, static_cast<std::size_t>(count));
  }
  _close(pipe_fds[0]);
  return result;
}

bool rejects_closed_protocol_stdout(const std::string& document) {
  const int stdout_fd = _fileno(stdout);
  if (stdout_fd < 0 || std::fflush(stdout) != 0) return false;
  const int saved_stdout = _dup(stdout_fd);
  if (saved_stdout < 0 || _close(stdout_fd) != 0) {
    if (saved_stdout >= 0) _close(saved_stdout);
    return false;
  }
  const auto previous_handler =
      _set_invalid_parameter_handler(&ignore_invalid_parameter);
  const bool rejected =
      !aexcompat::worker_runtime::emit_protocol_stdout(document);
  _set_invalid_parameter_handler(previous_handler);
  const bool restored = _dup2(saved_stdout, stdout_fd) == 0;
  _close(saved_stdout);
  std::cout.clear();
  return rejected && restored;
}

}  // namespace

int main() {
  aexcompat::worker_report::L2ReportContext report;
  report.status = "parameters_inspected";
  aexcompat::worker_report::ParameterSnapshot parameter;
  parameter.index = 1;
  parameter.has_numeric = true;
  parameter.valid_min = 0.0;
  parameter.valid_max = std::numeric_limits<double>::infinity();
  parameter.slider_min = -std::numeric_limits<double>::infinity();
  parameter.slider_max = 100.0;
  parameter.default_value = std::numeric_limits<double>::quiet_NaN();
  parameter.has_current = true;
  parameter.current_value = 25.0;
  parameter.component_count = 3;
  parameter.default_components = {0.0, std::numeric_limits<double>::infinity(), 1.0};
  parameter.current_components = {0.0, 0.5, std::numeric_limits<double>::quiet_NaN()};
  report.parameters.push_back(parameter);
  report.utility_undo_group_starts = 1;
  report.utility_undo_group_ends = 1;
  report.utility_undo_groups_balanced = true;
  report.utility_undo_group_operations_valid = true;
  report.mask_scene_observed = true;
  report.mask_scene_changed = true;
  report.mask_scene_id = "mask\"scene";
  report.mask_scene_fingerprint_before = 0x12;
  report.mask_scene_fingerprint_after = 0xfedcba9876543210ULL;
  report.mask_scene_active_masks = 1;
  report.mask_scene_mask_mutations = 2;
  aexcompat::worker_report::MaskCurveSnapshot curve;
  curve.id = 7;
  curve.vertices.push_back({1.25, 2.5, 0.0, 0.0, 3.75, 4.5});
  report.mask_scene_curves.push_back(curve);

  const std::string json = aexcompat::worker_report::serialize_l2_report(report);
  const std::string protocol_document =
      "{\"stage\":\"discovery_session\",\"payload\":\"" +
      std::string(16 * 1024, 'x') + "\"}\n";
  const ProtocolCapture protocol = capture_protocol_stdout(protocol_document);
  const bool closed_stdout_rejected =
      rejects_closed_protocol_stdout(protocol_document);

  const bool passed =
      json.find("\"valid_max\":null") != std::string::npos &&
      json.find("\"slider_min\":null") != std::string::npos &&
      json.find("\"default\":null") != std::string::npos &&
      json.find("\"default_components\":[0,null,1]") != std::string::npos &&
      json.find("\"current_components\":[0,0.5,null]") != std::string::npos &&
      json.find("\"utility_undo_groups\":{\"starts\":1,\"ends\":1,\"invalid_operations\":0,\"depth\":0,\"balanced\":true,\"operations_valid\":true}") !=
          std::string::npos &&
      json.find("\"mask_scene\":{\"observed\":true,\"id\":\"mask\\\"scene\",\"fingerprint_before\":\"0000000000000012\",\"fingerprint_after\":\"fedcba9876543210\",\"changed\":true") !=
          std::string::npos &&
      json.find("\"statistics\":{\"active_masks\":1,\"mask_mutations\":2") !=
          std::string::npos &&
      json.find("\"curves\":[{\"id\":7,\"open\":false,\"vertices\":[{\"x\":1.25,\"y\":2.5") !=
          std::string::npos &&
      json.find(":inf") == std::string::npos &&
      json.find(":-inf") == std::string::npos &&
      json.find(":nan") == std::string::npos &&
      protocol.emitted && protocol.restored &&
      protocol.bytes == protocol_document && closed_stdout_rejected;
  std::cout << "{\"worker_report_json\":\""
            << (passed ? "passed" : "failed")
            << "\",\"nonfinite_values\":\"null\",\"protocol_bytes\":"
            << protocol.bytes.size()
            << ",\"protocol_fail_closed\":"
            << (closed_stdout_rejected ? "true" : "false")
            << "}\n";
  return passed ? 0 : 1;
}
