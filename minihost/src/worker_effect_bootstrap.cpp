#include "worker_effect_bootstrap.hpp"

#include <algorithm>
#include <cstring>
#include <iostream>

namespace aexcompat::worker_runtime::effect_bootstrap {
namespace {

namespace contract = aexcompat::abi::x86_64_windows;

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
    write(state.input, contract::INPUT_CALLBACK_OFFSETS[i], abi.input_callbacks[i]);
  for (std::size_t i = 0; i < abi.utility_callbacks.size(); ++i)
    write(state.utils, contract::UTILITY_CALLBACK_OFFSETS[i], abi.utility_callbacks[i]);
  if (abi.color_callbacks && abi.color_callbacks_size == 64)
    std::memcpy(state.utils.data() + contract::UTILS_COLOR_CALLBACKS_OFFSET,
                abi.color_callbacks, abi.color_callbacks_size);
  write<void*>(state.input, contract::IN_UTILS_OFFSET, state.utils.data());
  write(state.input, contract::IN_PICA_BASICP_OFFSET, abi.basic_suite);
  write(state.input, contract::IN_EFFECT_REF_OFFSET, abi.effect_ref);
}

std::vector<UnwiredSlot> unwired_installed_offsets(const State& state) {
  std::vector<UnwiredSlot> slots;
  const auto collect = [&slots](const char* block, const std::byte* bytes,
                                std::size_t offset) {
    void* callback{};
    std::memcpy(&callback, bytes + offset, sizeof(callback));
    if (!callback) slots.push_back({block, offset});
  };
  // The three links `install_callback_tables` writes into in_data beside the
  // inter table. A plug-in reaches `pica_basicP` from GLOBAL_SETUP to acquire
  // every suite it uses, so a null there is the earliest and widest form of
  // this defect; `effect_ref` is the last member of `AbiHooks` and therefore
  // the one a short or reordered initializer drops first.
  for (const std::size_t offset : {contract::IN_UTILS_OFFSET,
                                   contract::IN_PICA_BASICP_OFFSET,
                                   contract::IN_EFFECT_REF_OFFSET})
    collect("in", state.input.data(), offset);
  for (const std::size_t offset : contract::INPUT_CALLBACK_OFFSETS)
    collect("inter", state.input.data(), offset);
  for (const std::size_t offset : contract::UTILITY_CALLBACK_OFFSETS)
    collect("utils", state.utils.data(), offset);
  // The color block is copied whole, so a size the installer refuses leaves
  // every entry zero; walking it pointer by pointer reports that case and also
  // a single null inside a block that was copied.
  for (std::size_t offset = 0; offset + sizeof(void*) <= contract::UTILS_COLOR_CALLBACKS_SIZE;
       offset += sizeof(void*))
    collect("utils.color_callbacks",
            state.utils.data() + contract::UTILS_COLOR_CALLBACKS_OFFSET, offset);
  return slots;
}

Result run(State& state, EffectEntry entry, const AbiHooks& abi,
           const Request& request, const RuntimeHooks& hooks) {
  Result result;
  install_callback_tables(state, abi);
  write(state.input, contract::IN_QUALITY_OFFSET, request.quality);
  write<int16_t>(state.input, contract::IN_VERSION_OFFSET, 13);
  // Present the version bundled effects themselves register (13.29, #326
  // probe), matching the AE 2025 host they ship with.
  write<int16_t>(state.input, contract::IN_VERSION_OFFSET + sizeof(int16_t), 29);
  write<uint32_t>(state.input, contract::IN_APPL_ID_OFFSET, 0x46585443u);
  write<int32_t>(state.input, contract::IN_NUM_PARAMS_OFFSET, 1);
  write<int32_t>(state.input, contract::IN_CURRENT_TIME_OFFSET, 0);
  write<int32_t>(state.input, contract::IN_TIME_STEP_OFFSET, 1);
  write<int32_t>(state.input, contract::IN_LOCAL_TIME_STEP_OFFSET, 1);
  write<uint32_t>(state.input, contract::IN_TIME_SCALE_OFFSET, 1);
  write(state.input, contract::IN_FIELD_OFFSET, request.field);
  write(state.input, contract::IN_SHUTTER_ANGLE_OFFSET, request.shutter_angle);
  write(state.input, contract::IN_PRE_EFFECT_SOURCE_ORIGIN_X_OFFSET,
        request.pre_effect_origin[0]);
  write(state.input, contract::IN_PRE_EFFECT_SOURCE_ORIGIN_Y_OFFSET,
        request.pre_effect_origin[1]);
  write(state.input, contract::IN_SHUTTER_PHASE_OFFSET, request.shutter_phase);
  write(state.input, contract::IN_DOWNSAMPLE_X_OFFSET, request.downsample_x[0]);
  write<uint32_t>(state.input,
                  contract::IN_DOWNSAMPLE_X_OFFSET + sizeof(int32_t),
                  request.downsample_x[1]);
  write(state.input, contract::IN_DOWNSAMPLE_Y_OFFSET, request.downsample_y[0]);
  write<uint32_t>(state.input,
                  contract::IN_DOWNSAMPLE_Y_OFFSET + sizeof(int32_t),
                  request.downsample_y[1]);
  write(state.input, contract::IN_PIXEL_ASPECT_RATIO_OFFSET,
        request.pixel_aspect_ratio[0]);
  write<uint32_t>(state.input,
                  contract::IN_PIXEL_ASPECT_RATIO_OFFSET + sizeof(int32_t),
                  request.pixel_aspect_ratio[1]);

  std::cerr << "stage:global_setup_begin\n" << std::flush;
  hooks.reset_effect_lifetime(true);
  hooks.set_global_setup_active(true);
  result.global_error = hooks.invoke(entry, 1, state.input.data(), state.output.data(),
                                     nullptr, nullptr, nullptr,
                                     &result.exception_codes[0]);
  hooks.set_global_setup_active(false);
  std::cerr << "stage:global_setup_end error=" << result.global_error << "\n" << std::flush;
  result.advertised_out_flags =
      read<uint32_t>(state.output, contract::OUT_OUT_FLAGS_OFFSET);
  result.advertised_out_flags2 =
      read<uint32_t>(state.output, contract::OUT_OUT_FLAGS2_OFFSET);
  if (request.render_worker)
    hooks.configure_audio_admission(request.audio_invocation,
                                    (result.advertised_out_flags & (1u << 20)) != 0);
  // Complementary reads of the same bit (PF_OutFlag_AUDIO_EFFECT_ONLY): edit
  // them together or the passthrough gates drift (issue #1048).
  result.image_render_supported = (result.advertised_out_flags & (1u << 31)) == 0;
  result.audio_effect_only = (result.advertised_out_flags & (1u << 31)) != 0;
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
  write(state.input, contract::IN_GLOBAL_DATA_OFFSET,
        read<void*>(state.output, contract::OUT_GLOBAL_DATA_OFFSET));
  if (!request.rendering_worker) {
    result.about_error = request.skip_about ? 0 :
        (result.global_error == 0
             ? hooks.invoke(entry, 0, state.input.data(), state.about_output.data(),
                            nullptr, nullptr, nullptr, &result.exception_codes[1])
             : -1);
    const char* text = reinterpret_cast<const char*>(
        state.about_output.data() + contract::OUT_RETURN_MSG_OFFSET);
    result.about_message.assign(
        text, strnlen_s(text, contract::OUT_RETURN_MSG_SIZE));
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
      read<int32_t>(state.output, contract::OUT_NUM_PARAMS_OFFSET) ==
          expected_num_params;
  if (result.parameter_count_contract_valid)
    write(state.input, contract::IN_NUM_PARAMS_OFFSET, expected_num_params);
  return result;
}

}  // namespace aexcompat::worker_runtime::effect_bootstrap
