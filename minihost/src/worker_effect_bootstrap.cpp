#include "worker_effect_bootstrap.hpp"

#include <cstring>
#include <iostream>

namespace aexcompat::worker_runtime::effect_bootstrap {
namespace {

constexpr std::array<std::size_t, 9> kInputCallbackOffsets{
    0, 8, 16, 24, 32, 40, 48, 56, 64};
constexpr std::array<std::size_t, 31> kUtilityCallbackOffsets{
    0, 8, 16, 32, 48, 56, 64, 72, 96, 104, 488, 496,
    88, 112, 120, 152, 224, 248, 296, 304, 328, 336, 432, 528, 536,
    // Handle callbacks (issue #220). Offsets are PF_UtilCallbacks member
    // offsets verified against the SDK header by abi-layout-probe static_asserts:
    // host_new_handle=160, host_lock_handle=168, host_unlock_handle=176,
    // host_dispose_handle=184, host_get_handle_size=440, host_resize_handle=464.
    160, 168, 176, 184, 440, 464};
constexpr std::size_t kUtilityColorCallbacksOffset = 368;
constexpr std::size_t kUtilityPlatformDataOffset = 432;
static_assert(kUtilityColorCallbacksOffset + 64 == kUtilityPlatformDataOffset);

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

}  // namespace

void install_callback_tables(State& state, const AbiHooks& abi) {
  for (std::size_t i = 0; i < abi.input_callbacks.size(); ++i)
    write(state.input, kInputCallbackOffsets[i], abi.input_callbacks[i]);
  for (std::size_t i = 0; i < abi.utility_callbacks.size(); ++i)
    write(state.utils, kUtilityCallbackOffsets[i], abi.utility_callbacks[i]);
  if (abi.color_callbacks && abi.color_callbacks_size == 64)
    std::memcpy(state.utils.data() + kUtilityColorCallbacksOffset,
                abi.color_callbacks, abi.color_callbacks_size);
  write<void*>(state.input, 176, state.utils.data());
  write(state.input, 384, abi.basic_suite);
  write(state.input, 184, abi.effect_ref);
}

Result run(State& state, EffectEntry entry, const AbiHooks& abi,
           const Request& request, const RuntimeHooks& hooks) {
  Result result;
  install_callback_tables(state, abi);
  write(state.input, 192, request.quality);
  write<int16_t>(state.input, 196, 13);
  write<int16_t>(state.input, 198, 28);
  write<uint32_t>(state.input, 204, 0x46585443u);
  write<int32_t>(state.input, 208, 1);
  write<int32_t>(state.input, 224, 0);
  write<int32_t>(state.input, 228, 1);
  write<int32_t>(state.input, 236, 1);
  write<uint32_t>(state.input, 240, 1);
  write(state.input, 244, request.field);
  write(state.input, 248, request.shutter_angle);
  write(state.input, 392, request.pre_effect_origin[0]);
  write(state.input, 396, request.pre_effect_origin[1]);
  write(state.input, 400, request.shutter_phase);
  write(state.input, 284, request.downsample_x[0]);
  write<uint32_t>(state.input, 288, request.downsample_x[1]);
  write(state.input, 292, request.downsample_y[0]);
  write<uint32_t>(state.input, 296, request.downsample_y[1]);
  write(state.input, 300, request.pixel_aspect_ratio[0]);
  write<uint32_t>(state.input, 304, request.pixel_aspect_ratio[1]);

  std::cerr << "stage:global_setup_begin\n" << std::flush;
  hooks.reset_effect_lifetime(true);
  hooks.set_global_setup_active(true);
  result.global_error = hooks.invoke(entry, 1, state.input.data(), state.output.data(),
                                     nullptr, nullptr, nullptr,
                                     &result.exception_codes[0]);
  hooks.set_global_setup_active(false);
  std::cerr << "stage:global_setup_end error=" << result.global_error << "\n" << std::flush;
  result.advertised_out_flags = read<uint32_t>(state.output, 96);
  result.advertised_out_flags2 = read<uint32_t>(state.output, 400);
  if (request.render_worker)
    hooks.configure_audio_admission(request.audio_invocation,
                                    (result.advertised_out_flags & (1u << 20)) != 0);
  result.image_render_supported = (result.advertised_out_flags & (1u << 31)) == 0;
  result.nop_render_advertised = (result.advertised_out_flags & (1u << 18)) != 0;
  result.input_write_advertised = (result.advertised_out_flags & (1u << 11)) != 0;
  result.expand_buffer_advertised = (result.advertised_out_flags & (1u << 9)) != 0;
  result.shrink_buffer_advertised = (result.advertised_out_flags & (1u << 12)) != 0;
  result.depth_supported = request.external_pixel_bytes == 4 ||
      (request.external_pixel_bytes == 8 &&
       (result.advertised_out_flags & (1u << 25)) != 0) ||
      (request.external_pixel_bytes == 16 &&
       (result.advertised_out_flags2 & (1u << 12)) != 0);
  result.smart_render_supported = (result.advertised_out_flags2 & (1u << 10)) != 0;
  result.update_params_ui_advertised = (result.advertised_out_flags & (1u << 26)) != 0;
  result.query_dynamic_flags_advertised = (result.advertised_out_flags2 & 1u) != 0;
  write(state.input, 312, read<void*>(state.output, 40));
  if (!request.rendering_worker) {
    result.about_error = request.skip_about ? 0 :
        (result.global_error == 0
             ? hooks.invoke(entry, 0, state.input.data(), state.about_output.data(),
                            nullptr, nullptr, nullptr, &result.exception_codes[1])
             : -1);
    const char* text = reinterpret_cast<const char*>(state.about_output.data() + 100);
    result.about_message.assign(text, strnlen_s(text, 256));
  }
  std::cerr << "stage:params_setup_begin\n" << std::flush;
  result.params_error = result.global_error == 0
      ? hooks.invoke(entry, 4, state.input.data(), state.output.data(), nullptr,
                     nullptr, nullptr, &result.exception_codes[2]) : -1;
  std::cerr << "stage:params_setup_end error=" << result.params_error << "\n" << std::flush;
  if (result.params_error == 0) hooks.observe_arbitrary_defaults(entry, state);
  // expected_num_params = static_cast<int32_t>(g_params.size() + 1), read
  // through the hook only after PARAMS_SETUP returned: add_param discovery
  // grows the host's records during the selector, so a launch-time snapshot
  // would reject every effect that declares parameters (issue #177).
  const int32_t expected_num_params =
      hooks.discovered_parameter_count ? hooks.discovered_parameter_count() + 1 : 1;
  result.parameter_count_contract_valid = result.params_error == 0 &&
      read<int32_t>(state.output, 48) == expected_num_params;
  if (result.parameter_count_contract_valid)
    write(state.input, 208, expected_num_params);
  return result;
}

}  // namespace aexcompat::worker_runtime::effect_bootstrap
