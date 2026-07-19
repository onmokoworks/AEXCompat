#include "worker_render_session.hpp"

#include <windows.h>

#include <cstring>
#include <cwchar>

namespace aexcompat::worker_render_session {
namespace {

constexpr const wchar_t* kRequestVariable = L"AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE";
constexpr const wchar_t* kResponseVariable = L"AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE";
constexpr const wchar_t* kSectionVariable = L"AEXCOMPAT_RENDER_SESSION_SECTION_HANDLE";

std::size_t align_slot(std::size_t bytes) {
  return (bytes + kSlotAlignment - 1) / kSlotAlignment * kSlotAlignment;
}

bool variable_present(const wchar_t* name) {
  wchar_t buffer[32];
  return GetEnvironmentVariableW(name, buffer, 32) != 0 ||
         GetLastError() != ERROR_ENVVAR_NOT_FOUND;
}

// Same shape as the AEX_INSTRUMENT_TRACE_HANDLE precedent in the shared
// trace writer: decimal handle number, strict tail, zero rejected, then a
// type check on the resulting handle.
HANDLE handle_from_variable(const wchar_t* name) {
  wchar_t buffer[32];
  const DWORD length = GetEnvironmentVariableW(name, buffer, 32);
  if (length == 0 || length >= 32) return nullptr;
  wchar_t* tail = nullptr;
  const unsigned long long value = std::wcstoull(buffer, &tail, 10);
  if (!tail || *tail != L'\0' || value == 0) return nullptr;
  const HANDLE handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(value));
  DWORD flags = 0;
  if (!GetHandleInformation(handle, &flags)) return nullptr;
  return handle;
}

HANDLE pipe_from_variable(const wchar_t* name) {
  const HANDLE handle = handle_from_variable(name);
  if (!handle || GetFileType(handle) != FILE_TYPE_PIPE) return nullptr;
  return handle;
}

// Returns the byte count actually collected; a stop before `bytes` means the
// pipe hit EOF or an error mid-read.
std::size_t read_up_to(HANDLE pipe, unsigned char* destination, std::size_t bytes) {
  std::size_t collected = 0;
  while (collected < bytes) {
    DWORD read = 0;
    const DWORD request = static_cast<DWORD>(bytes - collected);
    if (!ReadFile(pipe, destination + collected, request, &read, nullptr) ||
        read == 0)
      break;
    collected += read;
  }
  return collected;
}

bool write_exact(HANDLE pipe, const unsigned char* source, std::size_t bytes) {
  std::size_t sent = 0;
  while (sent < bytes) {
    DWORD written = 0;
    const DWORD request = static_cast<DWORD>(bytes - sent);
    if (!WriteFile(pipe, source + sent, request, &written, nullptr) ||
        written == 0)
      return false;
    sent += written;
  }
  return true;
}

}  // namespace

std::size_t input_slot_bytes(const SessionGeometry& geometry) {
  return static_cast<std::size_t>(geometry.max_width) * geometry.max_height * 4;
}

std::size_t output_slot_bytes(const SessionGeometry& geometry) {
  return static_cast<std::size_t>(geometry.max_width) * geometry.max_height *
         geometry.output_pixel_bytes;
}

std::size_t input_slot_offset() { return kHeaderBytes; }

std::size_t output_slot_offset(const SessionGeometry& geometry) {
  return kHeaderBytes + align_slot(input_slot_bytes(geometry));
}

std::size_t expected_section_bytes(const SessionGeometry& geometry) {
  return output_slot_offset(geometry) + align_slot(output_slot_bytes(geometry)) +
         static_cast<std::size_t>(geometry.layer_slot_count) *
             align_slot(input_slot_bytes(geometry));
}

bool session_environment_requested() {
  return variable_present(kRequestVariable) ||
         variable_present(kResponseVariable) ||
         variable_present(kSectionVariable);
}

SessionChannels::~SessionChannels() {
  if (view_) UnmapViewOfFile(view_);
  if (section_) CloseHandle(section_);
  if (request_pipe_) CloseHandle(request_pipe_);
  if (response_pipe_) CloseHandle(response_pipe_);
}

bool SessionChannels::open_from_environment(const SessionGeometry& geometry) {
  if (opened()) return false;
  if (geometry.max_width <= 0 || geometry.max_height <= 0 ||
      geometry.layer_slot_count < 0)
    return false;
  const HANDLE request = pipe_from_variable(kRequestVariable);
  const HANDLE response = pipe_from_variable(kResponseVariable);
  const HANDLE section = handle_from_variable(kSectionVariable);
  if (!request || !response || !section) return false;
  void* view = MapViewOfFile(section, FILE_MAP_ALL_ACCESS, 0, 0, 0);
  if (!view) return false;
  MEMORY_BASIC_INFORMATION region{};
  if (VirtualQuery(view, &region, sizeof(region)) != sizeof(region) ||
      region.RegionSize < expected_section_bytes(geometry)) {
    UnmapViewOfFile(view);
    return false;
  }
  request_pipe_ = request;
  response_pipe_ = response;
  section_ = section;
  view_ = static_cast<unsigned char*>(view);
  view_bytes_ = region.RegionSize;
  return true;
}

SessionChannels::ReadResult SessionChannels::read_message(std::string& payload) {
  if (!request_pipe_) return ReadResult::Violation;
  unsigned char prefix[4];
  const std::size_t prefix_read = read_up_to(request_pipe_, prefix, sizeof(prefix));
  if (prefix_read == 0) return ReadResult::Eof;
  if (prefix_read != sizeof(prefix)) return ReadResult::Violation;
  const uint32_t length = static_cast<uint32_t>(prefix[0]) |
                          (static_cast<uint32_t>(prefix[1]) << 8) |
                          (static_cast<uint32_t>(prefix[2]) << 16) |
                          (static_cast<uint32_t>(prefix[3]) << 24);
  if (length == 0 || length > kMaxMessageBytes) return ReadResult::Violation;
  payload.resize(length);
  return read_up_to(request_pipe_,
                    reinterpret_cast<unsigned char*>(payload.data()),
                    length) == length
             ? ReadResult::Message
             : ReadResult::Violation;
}

bool SessionChannels::write_message(const std::string& payload) {
  if (!response_pipe_ || payload.empty() || payload.size() > kMaxMessageBytes)
    return false;
  const uint32_t length = static_cast<uint32_t>(payload.size());
  const unsigned char prefix[4] = {
      static_cast<unsigned char>(length & 0xFF),
      static_cast<unsigned char>((length >> 8) & 0xFF),
      static_cast<unsigned char>((length >> 16) & 0xFF),
      static_cast<unsigned char>((length >> 24) & 0xFF)};
  return write_exact(response_pipe_, prefix, sizeof(prefix)) &&
         write_exact(response_pipe_,
                     reinterpret_cast<const unsigned char*>(payload.data()),
                     payload.size());
}

uint32_t SessionChannels::read_header_u32(std::size_t offset) const {
  uint32_t value = 0;
  if (view_ && offset + sizeof(value) <= kHeaderBytes)
    std::memcpy(&value, view_ + offset, sizeof(value));
  return value;
}

void SessionChannels::write_header_u32(std::size_t offset, uint32_t value) {
  if (view_ && offset + sizeof(value) <= kHeaderBytes)
    std::memcpy(view_ + offset, &value, sizeof(value));
}

bool SessionChannels::static_header_matches(const SessionGeometry& geometry) const {
  const uint32_t depth_code = geometry.output_pixel_bytes == 16
                                  ? 32
                                  : (geometry.output_pixel_bytes == 8 ? 16 : 8);
  return opened() && read_header_u32(kHeaderMagicOffset) == kHeaderMagic &&
         read_header_u32(kHeaderVersionOffset) == kProtocolVersion &&
         read_header_u32(kHeaderDepthCodeOffset) == depth_code &&
         read_header_u32(kHeaderMaxWidthOffset) ==
             static_cast<uint32_t>(geometry.max_width) &&
         read_header_u32(kHeaderMaxHeightOffset) ==
             static_cast<uint32_t>(geometry.max_height) &&
         read_header_u32(kHeaderLayerSlotCountOffset) ==
             static_cast<uint32_t>(geometry.layer_slot_count);
}

}  // namespace aexcompat::worker_render_session
