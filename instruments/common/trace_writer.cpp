#include "trace_writer.hpp"

#include <chrono>
#include <cstdlib>
#include <sstream>
#include <stdexcept>
#include <windows.h>

namespace aexcompat {
namespace {

std::filesystem::path trace_directory() {
  wchar_t buffer[32768]{};
  const DWORD length = GetEnvironmentVariableW(L"AEX_INSTRUMENT_TRACE_DIR", buffer,
                                                static_cast<DWORD>(std::size(buffer)));
  if (length == 0 || length >= std::size(buffer)) return {};
  return std::filesystem::path(buffer);
}

}  // namespace

TraceWriter::TraceWriter(std::string host_kind, std::string host_version_label,
                         std::string plugin_label)
    : host_kind_(std::move(host_kind)),
      host_version_label_(std::move(host_version_label)),
      plugin_label_(std::move(plugin_label)) {
  if (!safe_string(host_kind_) || !safe_string(host_version_label_) ||
      !safe_string(plugin_label_) || plugin_label_.find_first_of("\\/:") != std::string::npos) {
    return;
  }
  const auto directory = trace_directory();
  if (directory.empty() || !std::filesystem::is_directory(directory)) return;
  const auto ticks = std::chrono::high_resolution_clock::now().time_since_epoch().count();
  path_ = directory / ("host-trace-" + std::to_string(GetCurrentProcessId()) + "-" +
                       std::to_string(ticks) + ".jsonl");
  stream_.open(path_, std::ios::out | std::ios::binary);
  if (!stream_) path_.clear();
}

bool TraceWriter::enabled() const { return stream_.is_open(); }
const std::filesystem::path& TraceWriter::path() const { return path_; }

std::string TraceWriter::escape(const std::string& value) {
  std::ostringstream out;
  for (const unsigned char ch : value) {
    switch (ch) {
      case '"': out << "\\\""; break;
      case '\\': out << "\\\\"; break;
      case '\n': out << "\\n"; break;
      case '\r': out << "\\r"; break;
      case '\t': out << "\\t"; break;
      default:
        if (ch < 0x20) out << '?'; else out << ch;
    }
  }
  return out.str();
}

bool TraceWriter::safe_string(const std::string& value) {
  for (std::size_t i = 0; i + 2 < value.size(); ++i) {
    if (((value[i] >= 'A' && value[i] <= 'Z') || (value[i] >= 'a' && value[i] <= 'z')) &&
        value[i + 1] == ':' && (value[i + 2] == '\\' || value[i + 2] == '/')) return false;
  }
  return true;
}

void TraceWriter::write_base(const std::string& event_kind, const std::string& payload) {
  if (!enabled() || !safe_string(event_kind) || !safe_string(payload)) return;
  stream_ << "{\"schema_version\":1,\"event_index\":" << event_index_++
          << ",\"event_kind\":\"" << escape(event_kind) << "\",\"host_kind\":\""
          << escape(host_kind_) << "\",\"host_version_label\":\""
          << escape(host_version_label_) << "\",\"plugin_label\":\""
          << escape(plugin_label_) << "\"" << payload << "}\n";
  stream_.flush();
}

void TraceWriter::session_start() { write_base("session_start"); }
void TraceWriter::selector_dispatch(const std::string& selector) {
  if (safe_string(selector)) write_base("selector_dispatch", ",\"selector\":\"" + escape(selector) + "\"");
}
void TraceWriter::suite_acquire(const std::string& name, std::int64_t version, bool granted) {
  if (safe_string(name)) write_base("suite_acquire", ",\"suite\":{\"name\":\"" + escape(name) + "\",\"version\":" + std::to_string(version) + ",\"granted\":" + (granted ? "true" : "false") + "}");
}
void TraceWriter::suite_release(const std::string& name, std::int64_t version, bool granted) {
  if (safe_string(name)) write_base("suite_release", ",\"suite\":{\"name\":\"" + escape(name) + "\",\"version\":" + std::to_string(version) + ",\"granted\":" + (granted ? "true" : "false") + "}");
}
void TraceWriter::world_descriptor(std::int64_t width, std::int64_t height, std::int64_t rowbytes, const std::string& pixel_format) {
  if (width >= 0 && height >= 0 && rowbytes >= 0 && safe_string(pixel_format)) write_base("world_descriptor", ",\"world\":{\"width\":" + std::to_string(width) + ",\"height\":" + std::to_string(height) + ",\"rowbytes\":" + std::to_string(rowbytes) + ",\"pixel_format\":\"" + escape(pixel_format) + "\"}");
}
void TraceWriter::callback_invoke() { write_base("callback_invoke"); }
void TraceWriter::error(const std::string& code_label, const std::string& message, bool unimplemented) {
  if (safe_string(code_label) && safe_string(message)) write_base(unimplemented ? "unimplemented" : "error", ",\"error\":{\"code_label\":\"" + escape(code_label) + "\",\"message\":\"" + escape(message) + "\"}");
}
void TraceWriter::session_end() { write_base("session_end"); }

}  // namespace aexcompat
