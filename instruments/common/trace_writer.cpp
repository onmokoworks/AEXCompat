#include "trace_writer.hpp"

#include <cstdlib>
#include <cwchar>
#include <sstream>
#include <stdexcept>
#include <windows.h>

namespace aexcompat {
namespace {

constexpr std::size_t kMaxEvents = 4096;
constexpr std::size_t kMaxStringBytes = 256;
constexpr std::size_t kMaxPayloadBytes = 1024;

HANDLE trace_handle() {
  wchar_t buffer[32]{};
  const DWORD length = GetEnvironmentVariableW(L"AEX_INSTRUMENT_TRACE_HANDLE", buffer,
                                                static_cast<DWORD>(std::size(buffer)));
  if (length == 0 || length >= std::size(buffer)) return nullptr;
  wchar_t* end = nullptr;
  const unsigned long long value = std::wcstoull(buffer, &end, 10);
  if (!end || *end != L'\0' || value == 0) return nullptr;
  const HANDLE handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(value));
  DWORD flags{};
  if (!GetHandleInformation(handle, &flags) || GetFileType(handle) != FILE_TYPE_DISK) return nullptr;
  return handle;
}

}  // namespace

TraceWriter::TraceWriter(std::string host_kind, std::string host_version_label,
                         std::string plugin_label)
    : host_kind_(std::move(host_kind)),
      host_version_label_(std::move(host_version_label)),
      plugin_label_(std::move(plugin_label)) {
  requested_ = GetEnvironmentVariableW(L"AEX_INSTRUMENT_TRACE_HANDLE", nullptr, 0) != 0;
  if (!safe_string(host_kind_) || !safe_string(host_version_label_) ||
      !safe_string(plugin_label_) || plugin_label_.find_first_of("\\/:") != std::string::npos) {
    return;
  }
  handle_ = trace_handle();
}

TraceWriter::~TraceWriter() {
  if (session_started_ && !session_ended_) session_end();
  std::lock_guard<std::mutex> lock(mutex_);
  if (handle_) {
    CloseHandle(static_cast<HANDLE>(handle_));
    handle_ = nullptr;
  }
}

bool TraceWriter::enabled() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return handle_ != nullptr;
}
bool TraceWriter::requested() const { return requested_; }

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
  if (value.size() > kMaxStringBytes || !strict_utf8(value)) return false;
  for (const unsigned char ch : value) {
    if (ch < 0x20 || ch == 0x7f) return false;
  }
  for (std::size_t i = 0; i < value.size(); ++i) {
    if (value[i] == '/') return false;
    if (i + 1 < value.size() && value[i] == '\\' && value[i + 1] == '\\') return false;
  }
  for (std::size_t i = 0; i + 2 < value.size(); ++i) {
    if (((value[i] >= 'A' && value[i] <= 'Z') || (value[i] >= 'a' && value[i] <= 'z')) &&
        value[i + 1] == ':' && (value[i + 2] == '\\' || value[i + 2] == '/')) return false;
  }
  return true;
}

bool TraceWriter::strict_utf8(const std::string& value) {
  int remaining = 0;
  unsigned char lead = 0;
  for (const unsigned char ch : value) {
    if (remaining == 0) {
      if (ch <= 0x7f) continue;
      lead = ch;
      if (ch >= 0xc2 && ch <= 0xdf) remaining = 1;
      else if (ch >= 0xe0 && ch <= 0xef) remaining = 2;
      else if (ch >= 0xf0 && ch <= 0xf4) remaining = 3;
      else return false;
    } else {
      if (ch < 0x80 || ch > 0xbf) return false;
      if (remaining == 2 && lead == 0xe0 && ch < 0xa0) return false;
      if (remaining == 2 && lead == 0xed && ch > 0x9f) return false;
      if (remaining == 3 && lead == 0xf0 && ch < 0x90) return false;
      if (remaining == 3 && lead == 0xf4 && ch > 0x8f) return false;
      --remaining;
    }
  }
  return remaining == 0;
}

void TraceWriter::write_base(const std::string& event_kind, const std::string& payload) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (!handle_ || event_index_ >= kMaxEvents || event_kind.size() > kMaxStringBytes ||
      payload.size() > kMaxPayloadBytes || !safe_string(event_kind) || !safe_string(payload))
    return;
  std::ostringstream line;
  line << "{\"schema_version\":1,\"event_index\":" << event_index_++
       << ",\"event_kind\":\"" << escape(event_kind) << "\",\"host_kind\":\""
       << escape(host_kind_) << "\",\"host_version_label\":\""
       << escape(host_version_label_) << "\",\"plugin_label\":\""
       << escape(plugin_label_) << "\"" << payload << "}\n";
  const std::string bytes = line.str();
  DWORD written{};
  if (!WriteFile(static_cast<HANDLE>(handle_), bytes.data(), static_cast<DWORD>(bytes.size()),
                 &written, nullptr) || written != bytes.size() ||
      !FlushFileBuffers(static_cast<HANDLE>(handle_))) {
    CloseHandle(static_cast<HANDLE>(handle_));
    handle_ = nullptr;
  }
}

void TraceWriter::session_start() {
  if (session_started_) return;
  session_started_ = true;
  write_base("session_start");
}
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
void TraceWriter::session_end() {
  if (!session_started_ || session_ended_) return;
  session_ended_ = true;
  write_base("session_end");
}

}  // namespace aexcompat
