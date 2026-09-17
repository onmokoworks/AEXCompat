#pragma once

#include "worker_parameter_runtime.hpp"
#include "worker_smart_execution.hpp"

#include <array>
#include <cstdint>
#include <string>

namespace aexcompat::l2_detail {

// Completion-report inputs captured from worker_main_impl's smart render
// locals. Host-global custom-UI/telemetry state is read by the owner TU
// through cross-TU declarations; buffer reads that depend on l2_main's
// private protocol offsets arrive pre-extracted.
struct SmartCompletionInputs {
  const worker_runtime::smart_execution::Result* smart{};
  bool lifetime_fault_observed{};
  bool suite_fault_observed{};
  bool handle_fault_observed{};
  bool world_fault_observed{};
  bool pixel_format_fault_observed{};
  bool outline_fault_observed{};
  bool mask_attribute_fault_observed{};
  bool stream_metadata_fault_observed{};
  bool keyframe_fault_observed{};
  bool dynamic_stream_fault_observed{};
  bool aegp_memory_fault_observed{};
  int32_t global_error{};
  int32_t params_error{};
  int32_t setdown_error{};
  uint32_t advertised_out_flags{};
  uint32_t advertised_out_flags2{};
  bool parameter_count_contract_valid{};
  bool arbitrary_defaults_disposed{};
  bool depth_supported{};
  /// Advertised fact and the depth actually dispatched, recorded beside
  /// `depth_supported` (which says whether the run served the caller's
  /// depth at all).
  bool advertised_depth_supported{};
  int32_t dispatch_pixel_bytes{4};
  /// Pixel depth of the session's output slot, or 0 outside a session. The
  /// frame block below describes what the caller receives, and in a session
  /// that is the slot: the plug-in may have been dispatched shallower and
  /// the frame widened into it, so its own world depth would describe a
  /// buffer the caller never sees - and a row length taken from one with a
  /// pixel size taken from the other reports written bytes as undefined
  /// padding.
  int32_t session_pixel_bytes{};
  bool image_render_supported{};
  bool smart_render_supported{};
  bool nop_render_advertised{};
  bool input_write_advertised{};
  bool request_mode{};
  // Resident smart session summary (protocol v1.1); emitted and folded into
  // the completion verdict only when session_mode is true.
  bool session_mode{};
  int32_t session_frames_attempted{};
  int32_t session_sequence_setup_error{-1};
  int32_t session_sequence_setdown_error{-1};
  int32_t session_render_error{-1};
  bool session_protocol_violation{};
  bool session_invariant_failure{};
  std::string case_id;
  std::array<int32_t, 2> external_size{};
  const worker_runtime::parameters::RequestedAssignments* requested_parameters{};
  std::array<int32_t, 2> downsample_x{};
  std::array<int32_t, 2> downsample_y{};
  std::array<int32_t, 2> pixel_aspect_ratio{};
  std::array<int32_t, 2> resolution{};
  std::array<int32_t, 6> context_head{};
  std::array<int32_t, 2> context_zoom{};
  std::array<int32_t, 2> context_origin{};
  std::array<int32_t, 2> context_extent{};
};

void emit_smart_completion_report(const SmartCompletionInputs& inputs);

}  // namespace aexcompat::l2_detail
