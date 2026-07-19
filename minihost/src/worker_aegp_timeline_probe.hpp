#pragma once

#include <windows.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <thread>
#include <vector>

namespace aexcompat::worker_runtime::aegp_timeline {

#pragma pack(push, 1)
struct TimelinePacketHeader {
  uint32_t magic{0x52414558u};
  uint16_t version{2};
  uint16_t type{13};
  uint64_t sequence{1};
};
struct TimelineKeyframeRequest {
  TimelinePacketHeader header{};
  uint64_t comp_id{1001};
  uint64_t layer_id{2001};
};
struct TimelineKeyframesSnapshotHeader {
  TimelinePacketHeader header{};
  uint64_t comp_id{};
  uint64_t layer_id{};
  uint32_t total_data_bytes{};
  uint32_t prop_count{};
};
struct TimelineKeyframedPropHeader {
  char name[64]{};
  char effect_match[32]{};
  uint32_t keyframe_count{};
  uint8_t reserved[4]{};
};
struct TimelineKeyframeEntry {
  int32_t frame{};
  uint8_t interpolation{};
  uint8_t reserved[3]{};
  float value[4]{};
};
struct TimelineHostSeekRequest {
  TimelinePacketHeader header{0x52414558u, 2, 5, 7};
  int64_t frame{75};
  double time_seconds{2.5};
  double fps{30.0};
  uint32_t flags{};
  uint32_t reserved{};
};
struct TimelineHostSeekAck {
  TimelinePacketHeader header{};
  int32_t status{};
  int64_t accepted_frame{};
  double accepted_time_seconds{};
  double accepted_fps{};
};
struct TimelineHostTrimRequest {
  TimelinePacketHeader header{0x52414558u, 2, 15, 9};
  uint64_t comp_id{1001};
  uint64_t layer_id{2001};
  int32_t in_frame{30};
  int32_t out_frame{240};
  uint64_t reserved{};
};
struct TimelineHostTrimAck {
  TimelinePacketHeader header{};
  uint32_t status{};
  uint32_t reserved{};
  uint64_t layer_id{};
  int32_t in_frame{};
  int32_t out_frame{};
};
struct TimelineHostSwitchRequest {
  TimelinePacketHeader header{0x52414558u, 2, 22, 11};
  uint64_t comp_id{1001};
  uint64_t layer_id{2001};
  uint32_t flags_to_toggle{0x00000036u};
  uint32_t reserved{};
};
struct TimelineHostSwitchAck {
  TimelinePacketHeader header{};
  uint32_t status{};
  uint32_t applied_flags{};
  uint64_t layer_id{};
  uint32_t reserved{};
};
#pragma pack(pop)

static_assert(sizeof(TimelineKeyframeRequest) == 32);
static_assert(sizeof(TimelineKeyframesSnapshotHeader) == 40);
static_assert(sizeof(TimelineKeyframedPropHeader) == 104);
static_assert(sizeof(TimelineKeyframeEntry) == 24);
static_assert(sizeof(TimelineHostSeekRequest) == 48);
static_assert(sizeof(TimelineHostSeekAck) == 44);
static_assert(sizeof(TimelineHostTrimRequest) == 48);
static_assert(sizeof(TimelineHostTrimAck) == 40);
static_assert(sizeof(TimelineHostSwitchRequest) == 40);
static_assert(sizeof(TimelineHostSwitchAck) == 36);

struct KeyframePipeProbe {
  HANDLE pipe{INVALID_HANDLE_VALUE};
  std::thread reader;
  std::atomic_bool connected{false};
  std::atomic_bool request_sent{false};
  std::atomic_bool response_received{false};
  std::atomic_bool response_valid{false};
  std::atomic_uint32_t response_bytes{0};

  bool start();
  void stop();
  ~KeyframePipeProbe();
};

}  // namespace aexcompat::worker_runtime::aegp_timeline
