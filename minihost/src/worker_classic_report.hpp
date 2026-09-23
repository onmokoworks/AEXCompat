#pragma once

#include "worker_parameter_runtime.hpp"

#include <array>
#include <cstdint>
#include <string>

namespace aexcompat::l2_detail {

// Completion-report inputs captured from worker_main_impl's classic render
// locals. Host-global custom-UI/callback/context state is read by the owner
// TU through cross-TU declarations; buffer reads that depend on l2_main's
// private protocol offsets arrive pre-extracted.
struct ClassicCompletionInputs {
  int32_t render_error{};
  int32_t global_error{};
  int32_t params_error{};
  int32_t setdown_error{};
  bool parameter_count_contract_valid{};
  bool guards_intact{};
  bool arbitrary_defaults_disposed{};
  bool depth_supported{};
  /// Advertised fact and the depth actually dispatched, recorded beside
  /// `depth_supported` (which says whether the run served the caller's
  /// depth at all).
  bool advertised_depth_supported{};
  int32_t dispatch_pixel_bytes{4};
  /// Pixel depth of the session's output slot, or 0 outside a session. The
  /// frame block below describes what the caller receives, and in a session
  /// that is the slot: the plug-in may have been dispatched at another depth
  /// and the frame converted into it, so its own world depth would describe a
  /// buffer the caller never sees - and a row length taken from one with a
  /// pixel size taken from the other reports written bytes as undefined
  /// padding.
  int32_t session_pixel_bytes{};
  uint32_t advertised_out_flags{};
  uint32_t advertised_out_flags2{};
  bool image_render_supported{};
  bool nop_render_advertised{};
  bool input_write_advertised{};
  bool expand_buffer_advertised{};
  bool shrink_buffer_advertised{};
  std::string case_id;
  std::string input_hash;
  std::string output_hash;
  int32_t render_width{};
  int32_t render_height{};
  int32_t render_rowbytes{};
  std::array<int32_t, 2> thread_errors{};
  std::array<std::string, 2> thread_hashes{};
  std::array<bool, 2> thread_guards{};
  bool concurrent_render{};
  bool persistent_sequence{};
  int32_t persistent_sequence_setup_error{};
  int32_t persistent_sequence_setdown_error{};
  std::array<int32_t, 2> persistent_frame_errors{};
  std::array<std::string, 2> persistent_frame_hashes{};
  bool flattened_sequence{};
  int32_t sequence_flatten_error{};
  int32_t sequence_resetup_error{};
  bool flattened_handle_replaced{};
  bool resetup_handle_replaced{};
  bool flattened_handle_host_disposed{};
  bool copied_flattened_sequence{};
  int32_t get_flattened_sequence_data_error{};
  bool original_sequence_preserved{};
  std::string frame_return_message;
  bool request_mode{};
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

void emit_classic_completion_report(const ClassicCompletionInputs& inputs);

}  // namespace aexcompat::l2_detail
