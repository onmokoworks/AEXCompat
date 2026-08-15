#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace {

#if AEXCOMPAT_AUDIO_LAYER_PROBE
PF_Err setup_params(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  PF_ADD_LAYER("Layer", PF_LayerDefault_MYSELF, 1);
  out_data->num_params = 2;
  return PF_Err_NONE;
}

PF_Pixel sample_layer(const PF_LayerDef* layer, A_long x, A_long y) {
  const A_long cx = std::min(x, layer->width - 1);
  const A_long cy = std::min(y, layer->height - 1);
  const PF_Pixel* row = reinterpret_cast<const PF_Pixel*>(
      reinterpret_cast<const char*>(layer->data) + cy * layer->rowbytes);
  return row[cx];
}

A_u_char audio_channel(float value) {
  const float bounded = std::clamp(value, 0.0f, 1.0f);
  return static_cast<A_u_char>(std::lround(bounded * 255.0f));
}
#endif

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
#if AEXCOMPAT_AUDIO_EFFECT_ONLY
      out_data->out_flags |= PF_OutFlag_AUDIO_EFFECT_ONLY;
#endif
#if AEXCOMPAT_ADVERTISE_AUDIO
      out_data->out_flags |= PF_OutFlag_I_USE_AUDIO;
#endif
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
#if AEXCOMPAT_AUDIO_LAYER_PROBE
      return setup_params(in_data, out_data);
#else
      out_data->num_params = 1;
      return PF_Err_NONE;
#endif
    case PF_Cmd_RENDER: {
#if AEXCOMPAT_AUDIO_EFFECT_ONLY
      // The worker must passthrough an AUDIO_EFFECT_ONLY plug-in without
      // dispatching its nonexistent video path. A render call makes the
      // production cluster regression observable as a frame error.
      return PF_Err_INTERNAL_STRUCT_DAMAGED;
#elif AEXCOMPAT_CLUSTER_VIDEO_PROBE
      // Dedicated cluster discriminator: no audio-suite observation is mixed
      // into this fixture. The output makes dispatch of the current video
      // member distinguishable from audio-only input passthrough.
      if (!output || !output->data || output->rowbytes < output->width * 4)
        return PF_Err_BAD_CALLBACK_PARAM;
      for (A_long y = 0; y < output->height; ++y)
        std::memset(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                    0x29, static_cast<std::size_t>(output->width) * 4);
      return PF_Err_NONE;
#endif
#if AEXCOMPAT_AUDIO_LAYER_PROBE
      float audio_window_start = 0.0f;
      float audio_window_end = 0.0f;
#endif
#if AEXCOMPAT_AUDIO_DOUBLE_CHECKIN_PROBE
      PF_LayerAudio checked_in = nullptr;
      PF_Err lifetime_error = PF_CHECKOUT_LAYER_AUDIO(in_data, 0, 0, 4, 44100,
          0xac440000u, 4, PF_Channels_MONO, PF_SIGNED_FLOAT, &checked_in);
      if (lifetime_error == PF_Err_NONE)
        lifetime_error = PF_CHECKIN_LAYER_AUDIO(in_data, checked_in);
      if (lifetime_error != PF_Err_NONE ||
          PF_CHECKIN_LAYER_AUDIO(in_data, checked_in) == PF_Err_NONE)
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
#elif AEXCOMPAT_AUDIO_MULTI_HANDLE_PROBE
      PF_LayerAudio first = nullptr, second = nullptr;
      PF_Err multi_error = PF_CHECKOUT_LAYER_AUDIO(in_data, 0, 0, 4, 44100,
          0xac440000u, 4, PF_Channels_MONO, PF_SIGNED_FLOAT, &first);
      if (multi_error == PF_Err_NONE)
        multi_error = PF_CHECKOUT_LAYER_AUDIO(in_data, 0, 8, 4, 44100,
            0xac440000u, 4, PF_Channels_MONO, PF_SIGNED_FLOAT, &second);
      PF_SndSamplePtr first_samples = nullptr, second_samples = nullptr;
      A_long first_count = 0, second_count = 0;
      if (multi_error == PF_Err_NONE && first != second)
        multi_error = PF_GET_AUDIO_DATA(in_data, second, &second_samples,
            &second_count, nullptr, nullptr, nullptr, nullptr);
      if (multi_error == PF_Err_NONE)
        multi_error = PF_GET_AUDIO_DATA(in_data, first, &first_samples,
            &first_count, nullptr, nullptr, nullptr, nullptr);
      const bool handles_valid = multi_error == PF_Err_NONE && first != second &&
          first_count == 5 && second_count == 5 && first_samples && second_samples &&
          reinterpret_cast<float*>(first_samples)[0] == 0.0f &&
          reinterpret_cast<float*>(first_samples)[3] == 0.1875f &&
          reinterpret_cast<float*>(second_samples)[0] == 0.5f &&
          reinterpret_cast<float*>(second_samples)[3] == 0.6875f &&
          reinterpret_cast<float*>(first_samples)[4] == 0.0f &&
          reinterpret_cast<float*>(second_samples)[4] == 0.0f;
      if (second && PF_CHECKIN_LAYER_AUDIO(in_data, second) != PF_Err_NONE)
        multi_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      if (first && PF_CHECKIN_LAYER_AUDIO(in_data, first) != PF_Err_NONE)
        multi_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      if (!handles_valid || multi_error != PF_Err_NONE)
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
#elif AEXCOMPAT_AUDIO_FORMAT_PROBE
      const auto checkout = [in_data](A_u_long rate, A_long bytes, A_long channels,
          A_long format, PF_LayerAudio* audio, PF_SndSamplePtr* samples,
          A_long* count) -> PF_Err {
        PF_Err err = PF_CHECKOUT_LAYER_AUDIO(in_data, 0, 0, 4, 22050, rate,
            bytes, channels, format, audio);
        A_u_long actual_rate = 0;
        A_long actual_bytes = 0, actual_channels = 0, actual_format = -1;
        if (err == PF_Err_NONE)
          err = PF_GET_AUDIO_DATA(in_data, *audio, samples, count, &actual_rate,
              &actual_bytes, &actual_channels, &actual_format);
        if (err == PF_Err_NONE && (actual_rate != rate || actual_bytes != bytes ||
            actual_channels != channels || actual_format != format))
          err = PF_Err_INTERNAL_STRUCT_DAMAGED;
        return err;
      };
      PF_LayerAudio audio = nullptr;
      PF_SndSamplePtr samples = nullptr;
      A_long count = 0;
      PF_Err format_error = checkout(0x56220000u, 4, 1, PF_SIGNED_FLOAT,
          &audio, &samples, &count);
      if (format_error == PF_Err_NONE) {
        const float* values = reinterpret_cast<float*>(samples);
        if (count != 5 || values[0] != 0.0f || values[1] != 0.125f ||
            values[2] != 0.25f || values[3] != 0.375f || values[4] != 0.0f)
          format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
      if (audio && PF_CHECKIN_LAYER_AUDIO(in_data, audio) != PF_Err_NONE)
        format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      audio = nullptr; samples = nullptr; count = 0;
      if (format_error == PF_Err_NONE)
        format_error = checkout(0x56220000u, 2, 2, PF_SIGNED_PCM,
            &audio, &samples, &count);
      if (format_error == PF_Err_NONE) {
        const A_short* values = reinterpret_cast<A_short*>(samples);
        if (count != 5 || values[0] != 0 || values[1] != 0 ||
            values[2] != 4096 || values[3] != 4096 || values[8] != 0 || values[9] != 0)
          format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
      if (audio && PF_CHECKIN_LAYER_AUDIO(in_data, audio) != PF_Err_NONE)
        format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      audio = nullptr; samples = nullptr; count = 0;
      if (format_error == PF_Err_NONE)
        format_error = checkout(0x56220000u, 1, 1, PF_UNSIGNED_PCM,
            &audio, &samples, &count);
      if (format_error == PF_Err_NONE) {
        const A_u_char* values = reinterpret_cast<A_u_char*>(samples);
        if (count != 5 || values[0] != 128 || values[1] != 143 ||
            values[2] != 159 || values[3] != 175 || values[4] != 128)
          format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
      if (audio && PF_CHECKIN_LAYER_AUDIO(in_data, audio) != PF_Err_NONE)
        format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      PF_LayerAudio rejected_audio = nullptr;
      if (format_error == PF_Err_NONE && PF_CHECKOUT_LAYER_AUDIO(in_data, 0, 0, 1,
          44100, 0xac440000u, 2, PF_Channels_MONO, PF_SIGNED_FLOAT,
          &rejected_audio) == PF_Err_NONE)
        format_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      if (format_error != PF_Err_NONE) return format_error;
#elif AEXCOMPAT_AUDIO_BOUNDARY_PROBE
      const auto verify_window = [in_data](A_long start, A_long duration,
          A_long expected_count, const float* expected) -> PF_Err {
        PF_LayerAudio audio = nullptr;
        PF_Err err = PF_CHECKOUT_LAYER_AUDIO(
            in_data, 0, start, duration, 44100, 0xac440000u, PF_SSS_4,
            PF_Channels_MONO, PF_SIGNED_FLOAT, &audio);
        if (err != PF_Err_NONE) return err;
        PF_SndSamplePtr samples = reinterpret_cast<PF_SndSamplePtr>(1);
        A_long sample_count = -1;
        err = PF_GET_AUDIO_DATA(
            in_data, audio, &samples, &sample_count, nullptr, nullptr, nullptr, nullptr);
        bool valid = err == PF_Err_NONE && sample_count == expected_count + 1 && samples;
        for (A_long index = 0; valid && index < expected_count; ++index)
          valid = reinterpret_cast<float*>(samples)[index] == expected[index];
        valid = valid && reinterpret_cast<float*>(samples)[expected_count] == 0.0f;
        const PF_Err checkin_error = PF_CHECKIN_LAYER_AUDIO(in_data, audio);
        return valid && checkin_error == PF_Err_NONE
            ? PF_Err_NONE : PF_Err_INTERNAL_STRUCT_DAMAGED;
      };
      const float negative_window[20] = {
          0.0f, 0.0f, 0.0f, 0.0625f, 0.125f, 0.1875f, 0.25f, 0.3125f,
          0.375f, 0.4375f, 0.5f, 0.5625f, 0.625f, 0.6875f, 0.75f,
          0.8125f, 0.875f, 0.9375f, 0.0f, 0.0f};
      const float tail_window[8] = {
          0.875f, 0.9375f, 0.0f, 0.0f, 0.0f, 0.0f, 0.0f, 0.0f};
      const float fractional_window[2] = {0.0625f, 0.125f};
      PF_Err boundary_error = verify_window(-2, 20, 20, negative_window);
      if (boundary_error == PF_Err_NONE)
        boundary_error = verify_window(14, 8, 8, tail_window);
      if (boundary_error == PF_Err_NONE)
        boundary_error = verify_window(3, 0, 0, nullptr);
      if (boundary_error == PF_Err_NONE) {
        PF_LayerAudio audio = nullptr;
        boundary_error = PF_CHECKOUT_LAYER_AUDIO(
            in_data, 0, 1, 1, 30000, 0xac440000u, PF_SSS_4,
            PF_Channels_MONO, PF_SIGNED_FLOAT, &audio);
        PF_SndSamplePtr samples = nullptr;
        A_long sample_count = 0;
        if (boundary_error == PF_Err_NONE)
          boundary_error = PF_GET_AUDIO_DATA(
              in_data, audio, &samples, &sample_count, nullptr, nullptr, nullptr, nullptr);
        const bool fractional_valid = boundary_error == PF_Err_NONE && samples &&
            sample_count == 3 && reinterpret_cast<float*>(samples)[0] == fractional_window[0] &&
            reinterpret_cast<float*>(samples)[1] == fractional_window[1] &&
            reinterpret_cast<float*>(samples)[2] == 0.0f;
        const PF_Err checkin_error = audio
            ? PF_CHECKIN_LAYER_AUDIO(in_data, audio) : PF_Err_INTERNAL_STRUCT_DAMAGED;
        if (!fractional_valid || checkin_error != PF_Err_NONE)
          boundary_error = PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
      if (boundary_error != PF_Err_NONE) return boundary_error;
#else
      PF_LayerAudio audio = nullptr;
      const PF_Err error = PF_CHECKOUT_LAYER_AUDIO(
          in_data, 0, 4, 6, 44100, 0xac440000u, PF_SSS_4,
          PF_Channels_MONO, PF_SIGNED_FLOAT, &audio);
#if AEXCOMPAT_REQUIRE_AUDIO_SIDECAR
      if (error != PF_Err_NONE) return error;
      PF_SndSamplePtr samples = nullptr;
      A_long sample_count = 0;
      PF_Err data_error = PF_GET_AUDIO_DATA(
          in_data, audio, &samples, &sample_count, nullptr, nullptr, nullptr, nullptr);
#if AEXCOMPAT_AUDIO_LAYER_PROBE
      const bool valid = data_error == PF_Err_NONE && samples && sample_count == 7 &&
          std::isfinite(reinterpret_cast<float*>(samples)[0]) &&
          std::isfinite(reinterpret_cast<float*>(samples)[5]);
      if (valid) {
        audio_window_start = reinterpret_cast<float*>(samples)[0];
        audio_window_end = reinterpret_cast<float*>(samples)[5];
      }
#else
      const bool valid = data_error == PF_Err_NONE && samples && sample_count == 7 &&
          reinterpret_cast<float*>(samples)[0] == 0.25f &&
          reinterpret_cast<float*>(samples)[5] == 0.5625f &&
          reinterpret_cast<float*>(samples)[6] == 0.0f;
#endif
      const PF_Err checkin_error = PF_CHECKIN_LAYER_AUDIO(in_data, audio);
      if (!valid || checkin_error != PF_Err_NONE) return PF_Err_INTERNAL_STRUCT_DAMAGED;
#else
      if (error == PF_Err_NONE) {
        PF_CHECKIN_LAYER_AUDIO(in_data, audio);
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
#endif
#endif
      if (!output || !output->data || output->rowbytes < output->width * 4)
        return PF_Err_BAD_CALLBACK_PARAM;
#if AEXCOMPAT_AUDIO_LAYER_PROBE
      if (!params || !params[1] || !params[1]->u.ld.data ||
          params[1]->u.ld.width <= 0 || params[1]->u.ld.height <= 0)
        return PF_Err_BAD_CALLBACK_PARAM;
      const PF_LayerDef* layer = &params[1]->u.ld;
      const A_u_char audio_start = audio_channel(audio_window_start);
      const A_u_char audio_end = audio_channel(audio_window_end);
      for (A_long y = 0; y < output->height; ++y) {
        PF_Pixel* dst = reinterpret_cast<PF_Pixel*>(
            reinterpret_cast<char*>(output->data) + y * output->rowbytes);
        for (A_long x = 0; x < output->width; ++x) {
          const PF_Pixel source = sample_layer(layer, x, y);
          dst[x].alpha = 255;
          dst[x].red = source.red;
          dst[x].green = static_cast<A_u_char>(source.green ^ audio_start);
          dst[x].blue = static_cast<A_u_char>(source.blue ^ audio_end);
        }
      }
#else
      for (A_long y = 0; y < output->height; ++y)
        std::memset(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                    0x29, static_cast<std::size_t>(output->width) * 4);
#endif
      return PF_Err_NONE;
    }
    default:
      return PF_Err_NONE;
  }
}
