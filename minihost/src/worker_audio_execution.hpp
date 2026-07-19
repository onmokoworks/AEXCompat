#pragma once

#include "worker_parameter_execution.hpp"

#include <cstdint>
#include <filesystem>
#include <vector>

namespace aexcompat::l2_detail {

// Audio-mode execution request captured from worker_main_impl's locals. The
// effect entry and parameter buffers stay shared with the caller; invocation
// values arrive as plain values (issue #168).
struct AudioModeRequest {
  worker_runtime::parameter_execution::EffectEntry entry{};
  worker_runtime::parameter_execution::BufferIn* input{};
  worker_runtime::parameter_execution::BufferOut* output{};
  int32_t global_error{};
  int32_t params_error{};
  int32_t external_audio_samples{};
  std::vector<float>* external_audio{};
  const std::filesystem::path* external_audio_output{};
  const worker_runtime::parameters::RequestedAssignments* requested_parameters{};
};

// Locals the completion report echoes back after the guarded selector run.
struct AudioModeOutcome {
  bool assignments_applied{};
  int32_t audio_setup_error{};
  int32_t audio_render_error{};
  int32_t audio_setdown_error{};
  int32_t audio_global_setdown_error{};
  int32_t output_start{};
  int32_t output_samples{};
  bool setup_range_valid{};
  bool guards_intact{};
  bool samples_finite{};
  bool audio_lifetimes_balanced{};
  bool arbitrary_defaults_disposed{};
  bool passed{};
  bool output_created{};
};

// Runs the guarded audio setup/render/setdown selectors, disposes arbitrary
// defaults, runs global setdown, and writes the output file on success.
// stderr stage traces keep their exact order.
AudioModeOutcome run_audio_mode(const AudioModeRequest& request);

// Emits the audio_render completion JSON.
void emit_audio_render_report(const AudioModeRequest& request,
                              const AudioModeOutcome& outcome);

}  // namespace aexcompat::l2_detail
