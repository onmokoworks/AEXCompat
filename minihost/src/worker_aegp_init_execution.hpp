#pragma once

#include "worker_aegp_init_runtime.hpp"
#include "worker_aegp_timeline_probe.hpp"

#include <atomic>
#include <cstdint>
#include <vector>

namespace aexcompat::worker_runtime::aegp_init {

struct RoundtripModes {
  bool active_idle{};
  bool comp_idle{};
  bool keyframe{};
  bool seek{};
  bool trim{};
  bool switch_flags{};
};

struct RoundtripRequest {
  void* global_refcon{};
  const std::vector<int32_t>* inserted_commands{};
  int32_t* scene_frame{};
  aegp_timeline::KeyframePipeProbe* keyframe_probe{};
  aegp_timeline::SeekPipeProbe* seek_probe{};
  aegp_timeline::TrimPipeProbe* trim_probe{};
  aegp_timeline::SwitchPipeProbe* switch_probe{};
  RoundtripModes modes;
};

struct RoundtripValidationHooks {
  void* context{};
  bool (*keyframe)(void*){};
  bool (*seek)(void*){};
  bool (*trim)(void*){};
  bool (*switch_flags)(void*){};
};

struct RoundtripResult {
  int32_t error{};
  uint32_t hooks_invoked{};
  uint32_t menu_hooks_invoked{};
  uint32_t command_hooks_invoked{};
  uint32_t command_handled_count{};
  int32_t idle_max_sleep{-1};
};

RoundtripResult run_roundtrips(const RoundtripRequest& request,
                               const RoundtripValidationHooks& validation);

}  // namespace aexcompat::worker_runtime::aegp_init
