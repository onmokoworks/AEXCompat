#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <string>
#include <vector>

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
  // Sized from the contract rather than a literal 12 so the array and the
  // offsets `install_callback_tables` writes cannot drift apart. It does not
  // make a short brace list a compile error - aggregate initialization
  // value-initializes the tail - so the case where the contract grows past the
  // initializer in `make_bootstrap_abi_hooks` is caught at runtime, by
  // `unwired_installed_offsets` below.
  std::array<void*, abi::x86_64_windows::INPUT_CALLBACK_OFFSETS.size()>
      input_callbacks{};
  std::array<void*, abi::x86_64_windows::UTILITY_CALLBACK_OFFSETS.size()>
      utility_callbacks{};
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
  // Static timeline presented from GLOBAL_SETUP onward. Resident sessions
  // already carry these values in their authenticated launch request; keeping
  // bootstrap on the historical 0/1/0/1 defaults until the first frame makes
  // PARAMS_SETUP observe a different composition contract from
  // SEQUENCE_SETUP and RENDER.
  int32_t current_time{};
  int32_t time_step{1};
  int32_t total_time{};
  uint32_t time_scale{1};
  // GPU F32 admission is not permission to execute CPU float selectors.
  bool gpu_float_negotiation_allowed{};
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
  // PF_OutFlag_AUDIO_EFFECT_ONLY (bit 31): the plug-in processes audio and
  // never renders video. AE leaves such an effect's video untouched, which is
  // what the render session's passthrough mode reproduces (issue #1048).
  bool audio_effect_only{};
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

/// One slot `install_callback_tables` left null, named by the block it belongs
/// to. The offset alone would be ambiguous: ten of the twelve inter offsets are
/// also valid utility offsets, so 112 is both `inter.reserved_2` and
/// `utils.new_world` and an operator handed the bare number would audit the
/// wrong assignment list.
struct UnwiredSlot {
  /// Which installed block, in the generated contract's own naming: "in",
  /// "inter", "utils", or "utils.color_callbacks".
  const char* block;
  /// Byte offset within that block.
  std::size_t offset;
};

/// Every generated callback offset a plug-in would dereference that
/// `install_callback_tables` left null, read back out of the installed bytes.
///
/// The compile-time half of the invariant is `bindings_cover_contract_once`:
/// every generated utility offset has exactly one named source. Nothing proves
/// the caller assigned that source, nothing makes a short `input_callbacks`
/// brace list a compile error, and nothing catches an install loop that wrote
/// the wrong stride. Any of those leaves a null pointer no host code ever
/// reads, so the defect surfaces only when a plug-in calls through it and jumps
/// to address 0 - #777 as a 16-bit sampling crash, #981 as three FRAME_SETUP
/// crashes the SEH guard reported as error 512.
///
/// Reads the buffers rather than the `AbiHooks` it was built from, so a
/// regression in the install loop itself is in scope too. Covers everything
/// `install_callback_tables` writes: the inter and utility tables, the color
/// block pointer by pointer, and the `utils` / `pica_basicP` / `effect_ref`
/// links in in_data. It reports the `utils` link as null but does not check
/// that it points at this `State`'s own block, which is the caller's to decide
/// - `verify_production_utility_callback_table` does. A `PF_UtilCallbacks`
/// member the generated contract never names is outside both the install and
/// this answer (#991).
std::vector<UnwiredSlot> unwired_installed_offsets(const State& state);

Result run(State& state, EffectEntry entry, const AbiHooks& abi,
           const Request& request, const RuntimeHooks& hooks);

}  // namespace aexcompat::worker_runtime::effect_bootstrap
