#include "worker_report.hpp"
#include "native_stdout_guard.hpp"

#include <cmath>
#include <iostream>
#include <limits>
#include <streambuf>
#include <string>

namespace {

class ProtocolBuffer final : public std::streambuf {
 public:
  std::string bytes;
  int flushes{};

 protected:
  std::streamsize xsputn(const char* text, std::streamsize count) override {
    bytes.append(text, static_cast<std::size_t>(count));
    return count;
  }
  int overflow(int value) override {
    if (value != traits_type::eof()) bytes.push_back(static_cast<char>(value));
    return value;
  }
  int sync() override {
    ++flushes;
    return 0;
  }
};

class RejectingProtocolBuffer final : public std::streambuf {
 public:
  explicit RejectingProtocolBuffer(bool reject_flush)
      : reject_flush_(reject_flush) {}

 protected:
  std::streamsize xsputn(const char*, std::streamsize count) override {
    return reject_flush_ ? count : count - 1;
  }
  int sync() override { return reject_flush_ ? -1 : 0; }

 private:
  bool reject_flush_{};
};

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

  const std::string json = aexcompat::worker_report::serialize_l2_report(report);
  ProtocolBuffer protocol_buffer;
  std::streambuf* original = std::cout.rdbuf(&protocol_buffer);
  const std::string protocol_document =
      "{\"stage\":\"discovery_session\",\"payload\":\"" +
      std::string(16 * 1024, 'x') + "\"}\n";
  const bool protocol_written =
      aexcompat::worker_runtime::emit_protocol_stdout(protocol_document);
  std::cout.rdbuf(original);

  RejectingProtocolBuffer short_write(false);
  std::cout.rdbuf(&short_write);
  const bool short_write_rejected =
      !aexcompat::worker_runtime::emit_protocol_stdout(protocol_document);
  std::cout.rdbuf(original);
  std::cout.clear();

  RejectingProtocolBuffer failed_flush(true);
  std::cout.rdbuf(&failed_flush);
  const bool failed_flush_rejected =
      !aexcompat::worker_runtime::emit_protocol_stdout(protocol_document);
  std::cout.rdbuf(original);
  std::cout.clear();

  const bool passed =
      json.find("\"valid_max\":null") != std::string::npos &&
      json.find("\"slider_min\":null") != std::string::npos &&
      json.find("\"default\":null") != std::string::npos &&
      json.find("\"default_components\":[0,null,1]") != std::string::npos &&
      json.find("\"current_components\":[0,0.5,null]") != std::string::npos &&
      json.find(":inf") == std::string::npos &&
      json.find(":-inf") == std::string::npos &&
      json.find(":nan") == std::string::npos &&
      protocol_written &&
      protocol_buffer.bytes == protocol_document &&
      protocol_buffer.flushes == 1 && short_write_rejected &&
      failed_flush_rejected;
  std::cout << "{\"worker_report_json\":\""
            << (passed ? "passed" : "failed")
            << "\",\"nonfinite_values\":\"null\",\"protocol_bytes\":"
            << protocol_buffer.bytes.size()
            << ",\"protocol_flushes\":" << protocol_buffer.flushes
            << ",\"protocol_fail_closed\":"
            << (short_write_rejected && failed_flush_rejected ? "true" : "false")
            << "}\n";
  return passed ? 0 : 1;
}
