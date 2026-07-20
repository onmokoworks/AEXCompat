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

// One audio span's guarded selector run, shared by the one-shot audio mode and
// the resident audio session (issue #239). Applies the requested parameters,
// seeds the input audio metadata, runs the host-audio source, drives
// AUDIO_SETUP -> AUDIO_RENDER -> AUDIO_SETDOWN under the sentinel guard, checks
// the output range/guards/finiteness, and (on a valid range) copies the
// rendered samples into `captured_output`. It does NOT dispose arbitrary
// defaults, run GLOBAL_SETDOWN, or write any file: the one-shot caller adds
// those once, and the session caller loops this per request. stderr stage
// traces keep their exact order.
struct AudioSpanOutcome {
  bool assignments_applied{};
  int32_t audio_setup_error{};
  int32_t audio_render_error{};
  int32_t audio_setdown_error{};
  int32_t output_start{};
  int32_t output_samples{};
  bool setup_range_valid{};
  bool guards_intact{};
  bool samples_finite{};
  bool audio_lifetimes_balanced{};
};

AudioSpanOutcome run_audio_span(
    worker_runtime::parameter_execution::EffectEntry entry,
    worker_runtime::parameter_execution::BufferIn& input,
    worker_runtime::parameter_execution::BufferOut& output,
    std::vector<float>* external_audio, int32_t external_audio_samples,
    const worker_runtime::parameters::RequestedAssignments& requested_parameters,
    std::vector<float>* captured_output);

// Runs one audio span (run_audio_span), disposes arbitrary defaults, runs
// global setdown, and writes the output file on success.
AudioModeOutcome run_audio_mode(const AudioModeRequest& request);

// Emits the audio_render completion JSON.
void emit_audio_render_report(const AudioModeRequest& request,
                              const AudioModeOutcome& outcome);

}  // namespace aexcompat::l2_detail
