#include "worker_audio_session.hpp"

#include <windows.h>

#include <cstring>

namespace aexcompat::worker_audio_session {
namespace {

// The audio session reuses the image session's inherited-handle transport, so
// it reuses the same env variable names the broker's SessionChildHandles sets
// (windows_process.rs). The worker distinguishes an audio session from an image
// session by the CLI command word (--render-audio-session-v1), not the env name.
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

// Same shape as the image session / trace-writer precedent: decimal handle
// number, strict tail, zero rejected, then a type check on the resulting
// handle.
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

std::size_t slot_bytes(const AudioSessionGeometry& geometry) {
  return static_cast<std::size_t>(geometry.max_samples) * geometry.channels *
         sizeof(float);
}

std::size_t input_slot_offset() { return kHeaderBytes; }

std::size_t output_slot_offset(const AudioSessionGeometry& geometry) {
  return kHeaderBytes + align_slot(slot_bytes(geometry));
}

std::size_t expected_section_bytes(const AudioSessionGeometry& geometry) {
  return output_slot_offset(geometry) + align_slot(slot_bytes(geometry));
}

bool session_environment_requested() {
  return variable_present(kRequestVariable) ||
         variable_present(kResponseVariable) ||
         variable_present(kSectionVariable);
}

AudioSessionChannels::~AudioSessionChannels() {
  if (view_) UnmapViewOfFile(view_);
  if (section_) CloseHandle(section_);
  if (request_pipe_) CloseHandle(request_pipe_);
  if (response_pipe_) CloseHandle(response_pipe_);
}

bool AudioSessionChannels::open_from_environment(const AudioSessionGeometry& geometry) {
  if (opened()) return false;
  if (geometry.max_samples <= 0 || geometry.channels <= 0) return false;
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

AudioSessionChannels::ReadResult AudioSessionChannels::read_message(std::string& payload) {
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

bool AudioSessionChannels::write_message(const std::string& payload) {
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

uint32_t AudioSessionChannels::read_header_u32(std::size_t offset) const {
  uint32_t value = 0;
  if (view_ && offset + sizeof(value) <= kHeaderBytes)
    std::memcpy(&value, view_ + offset, sizeof(value));
  return value;
}

void AudioSessionChannels::write_header_u32(std::size_t offset, uint32_t value) {
  if (view_ && offset + sizeof(value) <= kHeaderBytes)
    std::memcpy(view_ + offset, &value, sizeof(value));
}

bool AudioSessionChannels::static_header_matches(const AudioSessionGeometry& geometry) const {
  return read_header_u32(kHeaderMagicOffset) == kHeaderMagic &&
         read_header_u32(kHeaderVersionOffset) == kProtocolVersion &&
         read_header_u32(kHeaderMaxSamplesOffset) ==
             static_cast<uint32_t>(geometry.max_samples) &&
         read_header_u32(kHeaderChannelsOffset) ==
             static_cast<uint32_t>(geometry.channels);
}

}  // namespace aexcompat::worker_audio_session
