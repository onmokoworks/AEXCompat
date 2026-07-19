#pragma once

#include "worker_aegp_init_execution.hpp"

#include <cstdint>
#include <vector>

namespace aexcompat::worker_runtime::aegp_init {

using EntryPoint = int32_t(__cdecl*)(void*, int32_t, int32_t, int32_t, void**);

struct OrchestrationModes {
  bool update_menu{};
  bool idle{};
  bool command_roundtrip{};
  RoundtripModes roundtrip;
};

struct OrchestrationRequest {
  EntryPoint entry{};
  void* basic_suite{};
  const std::vector<int32_t>* inserted_commands{};
  int32_t* scene_frame{};
  aegp_timeline::KeyframePipeProbe* keyframe_probe{};
  aegp_timeline::SeekPipeProbe* seek_probe{};
  aegp_timeline::TrimPipeProbe* trim_probe{};
  aegp_timeline::SwitchPipeProbe* switch_probe{};
  OrchestrationModes modes;
};

struct OrchestrationResult {
  void* global_refcon{};
  int32_t init_error{};
  int32_t event_error{};
  int32_t death_error{};
  uint32_t hooks_invoked{};
  uint32_t menu_hooks_invoked{};
  uint32_t death_hooks_invoked{};
  uint32_t command_hooks_invoked{};
  uint32_t command_handled_count{};
  int32_t idle_max_sleep{-1};
};

OrchestrationResult run_orchestration(
    const OrchestrationRequest& request,
    const RoundtripValidationHooks& validation);

}  // namespace aexcompat::worker_runtime::aegp_init
