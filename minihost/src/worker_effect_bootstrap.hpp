#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>

#include "generated/aex_abi_contract.hpp"

namespace aexcompat::worker_runtime::effect_bootstrap {

using EffectEntry = int32_t(__cdecl*)(int32_t, void*, void*, void**, void*, void*);
using InvokeEntry = int32_t(*)(EffectEntry, int32_t, void*, void*, void**, void*,
                               void*, uint32_t*);

struct State {
  alignas(8) std::array<std::byte, abi::x86_64_windows::PF_IN_DATA_SIZE> input{};
  alignas(8) std::array<std::byte, abi::x86_64_windows::PF_OUT_DATA_SIZE> output{};
  alignas(8) std::array<std::byte, abi::x86_64_windows::PF_UTIL_CALLBACKS_SIZE> utils{};
  std::array<std::byte, abi::x86_64_windows::PF_OUT_DATA_SIZE> about_output{};
};

struct AbiHooks {
  std::array<void*, 12> input_callbacks{};
  std::array<void*, 31> utility_callbacks{};
  const void* color_callbacks{};
  std::size_t color_callbacks_size{};
  void* basic_suite{};
  void* effect_ref{};
};

struct Request {
  int32_t quality{};
  int32_t field{};
  int32_t shutter_angle{};
  int32_t shutter_phase{};
  std::array<int32_t, 2> pre_effect_origin{};
  std::array<int32_t, 2> downsample_x{};
  std::array<int32_t, 2> downsample_y{};
  std::array<int32_t, 2> pixel_aspect_ratio{};
  int32_t external_pixel_bytes{};
  bool render_worker{};
  bool rendering_worker{};
  /// This invocation renders audio, so the host-audio runtime admits a
  /// checkout even when the plug-in did not advertise PF_OutFlag_I_USE_AUDIO.
  /// Fed by the audio session since #365 deleted the one-shot --render-audio
  /// mode that used to set it (the field was named audio_mode for that).
  bool audio_invocation{};
  bool skip_about{};
};

struct RuntimeHooks {
  InvokeEntry invoke{};
  void (*reset_effect_lifetime)(bool){};
  void (*set_global_setup_active)(bool){};
  void (*configure_audio_admission)(bool, bool){};
  void (*observe_arbitrary_defaults)(EffectEntry, State&){};
  // Number of parameter records the host has discovered so far. The count
  // contract must read this AFTER PARAMS_SETUP, because add_param discovery
  // grows the records during that selector (issue #177); a launch-time
  // snapshot rejects every effect that declares parameters dynamically.
  int32_t (*discovered_parameter_count)(){};
};

struct Result {
  int32_t global_error{};
  int32_t about_error{-1};
  int32_t params_error{-1};
  std::array<uint32_t, 3> exception_codes{};
  uint32_t advertised_out_flags{};
  uint32_t advertised_out_flags2{};
  bool image_render_supported{};
  bool nop_render_advertised{};
  bool input_write_advertised{};
  bool expand_buffer_advertised{};
  bool shrink_buffer_advertised{};
  bool depth_supported{};
  bool smart_render_supported{};
  bool update_params_ui_advertised{};
  bool query_dynamic_flags_advertised{};
  bool parameter_count_contract_valid{};
  std::string about_message;
};

// Installs the input/utility/color callback tables into the ABI buffers and
// links in_data->utils to the utility block. Extracted from run() so behavioral
// self-tests can exercise the exact wiring a plug-in observes through
// in_data->utils without dispatching a selector (issue #220).
void install_callback_tables(State& state, const AbiHooks& abi);

Result run(State& state, EffectEntry entry, const AbiHooks& abi,
           const Request& request, const RuntimeHooks& hooks);

}  // namespace aexcompat::worker_runtime::effect_bootstrap
