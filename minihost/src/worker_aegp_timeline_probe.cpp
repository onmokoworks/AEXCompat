#include "worker_aegp_timeline_probe.hpp"

#include <cstring>

namespace aexcompat::worker_runtime::aegp_timeline {

bool KeyframePipeProbe::start() {
  pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
      PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
      1, 1024 * 1024, 1024 * 1024, 0, nullptr);
  if (pipe == INVALID_HANDLE_VALUE) return false;
  reader = std::thread([this]() {
    const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
        GetLastError() == ERROR_PIPE_CONNECTED;
    if (!connected_now) return;
    connected = true;
    const TimelineKeyframeRequest request{};
    DWORD written = 0;
    if (WriteFile(pipe, &request, sizeof(request), &written, nullptr) &&
        written == sizeof(request)) request_sent = true;
    else return;

    std::vector<uint8_t> inbound;
    inbound.reserve(4096);
    std::array<uint8_t, 4096> chunk{};
    while (inbound.size() < 1024 * 1024) {
      DWORD bytes_read = 0;
      if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()),
                    &bytes_read, nullptr) || bytes_read == 0) return;
      inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
      for (std::size_t offset = 0;
           offset + sizeof(TimelineKeyframesSnapshotHeader) <= inbound.size(); ++offset) {
        TimelineKeyframesSnapshotHeader header{};
        std::memcpy(&header, inbound.data() + offset, sizeof(header));
        if (header.header.magic != 0x52414558u || header.header.version != 2 ||
            header.header.type != 14) continue;
        if (header.total_data_bytes > 1024 * 1024 - sizeof(header) ||
            offset + sizeof(header) + header.total_data_bytes > inbound.size()) continue;
        response_received = true;
        response_bytes = static_cast<uint32_t>(sizeof(header) + header.total_data_bytes);
        if (header.comp_id != 1001 || header.layer_id != 2001 || header.prop_count != 1 ||
            header.total_data_bytes != sizeof(TimelineKeyframedPropHeader) +
                2 * sizeof(TimelineKeyframeEntry)) return;
        TimelineKeyframedPropHeader property{};
        TimelineKeyframeEntry first{}, second{};
        const auto* payload = inbound.data() + offset + sizeof(header);
        std::memcpy(&property, payload, sizeof(property));
        std::memcpy(&first, payload + sizeof(property), sizeof(first));
        std::memcpy(&second, payload + sizeof(property) + sizeof(first), sizeof(second));
        const bool strings_valid = std::strncmp(property.name, "Amount", sizeof(property.name)) == 0 &&
            std::strncmp(property.effect_match, "AEXCompat.Probe", sizeof(property.effect_match)) == 0;
        const bool keys_valid = property.keyframe_count == 2 &&
            first.frame == 0 && first.interpolation == 0 && first.value[0] == 10.0f &&
            second.frame == 60 && second.interpolation == 2 && second.value[0] == 90.0f;
        response_valid = strings_valid && keys_valid;
        return;
      }
    }
  });
  return true;
}

void KeyframePipeProbe::stop() {
  if (reader.joinable())
    CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
  if (reader.joinable()) reader.join();
  if (pipe != INVALID_HANDLE_VALUE) {
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    pipe = INVALID_HANDLE_VALUE;
  }
}

KeyframePipeProbe::~KeyframePipeProbe() { stop(); }

bool SeekPipeProbe::start() {
  pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
      PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
      1, 1024 * 1024, 1024 * 1024, 0, nullptr);
  if (pipe == INVALID_HANDLE_VALUE) return false;
  reader = std::thread([this]() {
    const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
        GetLastError() == ERROR_PIPE_CONNECTED;
    if (!connected_now) return;
    connected = true;
    const TimelineHostSeekRequest request{};
    DWORD written = 0;
    if (!WriteFile(pipe, &request, sizeof(request), &written, nullptr) ||
        written != sizeof(request)) return;
    request_sent = true;
    std::vector<uint8_t> inbound;
    inbound.reserve(4096);
    std::array<uint8_t, 4096> chunk{};
    while (inbound.size() < 1024 * 1024) {
      DWORD bytes_read = 0;
      if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()),
                    &bytes_read, nullptr) || bytes_read == 0) return;
      inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
      for (std::size_t offset = 0; offset + sizeof(TimelineHostSeekAck) <= inbound.size(); ++offset) {
        TimelineHostSeekAck ack{};
        std::memcpy(&ack, inbound.data() + offset, sizeof(ack));
        if (ack.header.magic != 0x52414558u || ack.header.version != 2 ||
            ack.header.type != 6) continue;
        ack_received = true;
        ack_valid = ack.header.sequence == 7 && ack.status == 0 &&
            ack.accepted_frame == 75 && ack.accepted_time_seconds == 2.5 &&
            ack.accepted_fps == 30.0;
        return;
      }
    }
  });
  return true;
}

void SeekPipeProbe::stop() {
  if (reader.joinable()) CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
  if (reader.joinable()) reader.join();
  if (pipe != INVALID_HANDLE_VALUE) {
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    pipe = INVALID_HANDLE_VALUE;
  }
}

SeekPipeProbe::~SeekPipeProbe() { stop(); }

bool TrimPipeProbe::start() {
  pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
      PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
      1, 1024 * 1024, 1024 * 1024, 0, nullptr);
  if (pipe == INVALID_HANDLE_VALUE) return false;
  reader = std::thread([this]() {
    const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
        GetLastError() == ERROR_PIPE_CONNECTED;
    if (!connected_now) return;
    connected = true;
    const TimelineHostTrimRequest request{};
    DWORD written = 0;
    if (!WriteFile(pipe, &request, sizeof(request), &written, nullptr) ||
        written != sizeof(request)) return;
    request_sent = true;
    std::vector<uint8_t> inbound;
    inbound.reserve(4096);
    std::array<uint8_t, 4096> chunk{};
    while (inbound.size() < 1024 * 1024) {
      DWORD bytes_read = 0;
      if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()),
                    &bytes_read, nullptr) || bytes_read == 0) return;
      inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
      for (std::size_t offset = 0; offset + sizeof(TimelineHostTrimAck) <= inbound.size(); ++offset) {
        TimelineHostTrimAck ack{};
        std::memcpy(&ack, inbound.data() + offset, sizeof(ack));
        if (ack.header.magic != 0x52414558u || ack.header.version != 2 ||
            ack.header.type != 16) continue;
        ack_received = true;
        ack_valid = ack.header.sequence == 9 && ack.status == 0 &&
            ack.layer_id == 2001 && ack.in_frame == 30 && ack.out_frame == 240;
        return;
      }
    }
  });
  return true;
}

void TrimPipeProbe::stop() {
  if (reader.joinable()) CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
  if (reader.joinable()) reader.join();
  if (pipe != INVALID_HANDLE_VALUE) {
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    pipe = INVALID_HANDLE_VALUE;
  }
}

TrimPipeProbe::~TrimPipeProbe() { stop(); }

bool SwitchPipeProbe::start() {
  pipe = CreateNamedPipeW(L"\\\\.\\pipe\\ae-timeline-sync",
      PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
      1, 1024 * 1024, 1024 * 1024, 0, nullptr);
  if (pipe == INVALID_HANDLE_VALUE) return false;
  reader = std::thread([this]() {
    const BOOL connected_now = ConnectNamedPipe(pipe, nullptr) ||
        GetLastError() == ERROR_PIPE_CONNECTED;
    if (!connected_now) return;
    connected = true;
    const TimelineHostSwitchRequest request{};
    DWORD written = 0;
    if (!WriteFile(pipe, &request, sizeof(request), &written, nullptr) ||
        written != sizeof(request)) return;
    request_sent = true;
    std::vector<uint8_t> inbound;
    inbound.reserve(4096);
    std::array<uint8_t, 4096> chunk{};
    while (inbound.size() < 1024 * 1024) {
      DWORD bytes_read = 0;
      if (!ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()),
                    &bytes_read, nullptr) || bytes_read == 0) return;
      inbound.insert(inbound.end(), chunk.begin(), chunk.begin() + bytes_read);
      for (std::size_t offset = 0; offset + sizeof(TimelineHostSwitchAck) <= inbound.size(); ++offset) {
        TimelineHostSwitchAck ack{};
        std::memcpy(&ack, inbound.data() + offset, sizeof(ack));
        if (ack.header.magic != 0x52414558u || ack.header.version != 2 ||
            ack.header.type != 23) continue;
        ack_received = true;
        ack_valid = ack.header.sequence == 11 && ack.status == 0 &&
            ack.applied_flags == 0x00000032u && ack.layer_id == 2001;
        return;
      }
    }
  });
  return true;
}

void SwitchPipeProbe::stop() {
  if (reader.joinable()) CancelSynchronousIo(static_cast<HANDLE>(reader.native_handle()));
  if (reader.joinable()) reader.join();
  if (pipe != INVALID_HANDLE_VALUE) {
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    pipe = INVALID_HANDLE_VALUE;
  }
}

SwitchPipeProbe::~SwitchPipeProbe() { stop(); }

}  // namespace aexcompat::worker_runtime::aegp_timeline
