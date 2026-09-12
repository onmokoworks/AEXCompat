#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "Param_Utils.h"

#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>

namespace {
#ifndef AEX_TIMED_PROBE_SLOT
#define AEX_TIMED_PROBE_SLOT 1
#endif
constexpr A_long kLayerSlot = AEX_TIMED_PROBE_SLOT;
constexpr std::array<A_long, 3> kCheckoutIds{101, 202, 303};
constexpr std::array<A_long, 3> kTimes{6, 1, 5};
constexpr std::array<A_u_long, 3> kScales{8, 3, 4};
constexpr std::array<int, 3> kWeights{1, 2, 3};
int g_successful_checkouts = 0;
int g_successful_checkins = 0;

template <typename Pixel, typename Channel, int Maximum>
PF_Err Composite(PF_EffectWorld* const worlds[3], PF_EffectWorld* output) {
  if (!output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  for (const auto* world : std::array<PF_EffectWorld*, 3>{worlds[0], worlds[1], worlds[2]}) {
    if (!world || !world->data || world->width != output->width ||
        world->height != output->height) return PF_Err_BAD_CALLBACK_PARAM;
  }
  for (A_long y = 0; y < output->height; ++y) {
    const auto* p0 = reinterpret_cast<const Pixel*>(
        reinterpret_cast<const A_u_char*>(worlds[0]->data) + y * worlds[0]->rowbytes);
    const auto* p1 = reinterpret_cast<const Pixel*>(
        reinterpret_cast<const A_u_char*>(worlds[1]->data) + y * worlds[1]->rowbytes);
    const auto* p2 = reinterpret_cast<const Pixel*>(
        reinterpret_cast<const A_u_char*>(worlds[2]->data) + y * worlds[2]->rowbytes);
    auto* dst = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      const Channel* sources[3] = {
          reinterpret_cast<const Channel*>(&p0[x]),
          reinterpret_cast<const Channel*>(&p1[x]),
          reinterpret_cast<const Channel*>(&p2[x])};
      Channel* target = reinterpret_cast<Channel*>(&dst[x]);
      for (int channel = 0; channel < 4; ++channel) {
        if constexpr (Maximum == 1) {
          target[channel] = static_cast<Channel>((sources[0][channel] +
              2.0f * sources[1][channel] + 3.0f * sources[2][channel]) / 6.0f);
        } else {
          const std::int64_t sum = static_cast<std::int64_t>(sources[0][channel]) +
              2 * static_cast<std::int64_t>(sources[1][channel]) +
              3 * static_cast<std::int64_t>(sources[2][channel]);
          target[channel] = static_cast<Channel>((sum + 3) / 6);
        }
      }
    }
  }
  return PF_Err_NONE;
}

PF_Err SmartPreRender(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  g_successful_checkouts = 0;
  g_successful_checkins = 0;
  PF_Rect result{};
  PF_Rect maximum{};
  for (std::size_t i = 0; i < kCheckoutIds.size(); ++i) {
    PF_RenderRequest request = extra->input->output_request;
    PF_CheckoutResult checkout{};
    const PF_Err err = extra->cb->checkout_layer(
        in_data->effect_ref, kLayerSlot, kCheckoutIds[i], &request,
        kTimes[i], 1, kScales[i], &checkout);
    if (err) return err;
    ++g_successful_checkouts;
    if (i == 0) {
      result = checkout.result_rect;
      maximum = checkout.max_result_rect;
    } else {
      result.left = std::min(result.left, checkout.result_rect.left);
      result.top = std::min(result.top, checkout.result_rect.top);
      result.right = std::max(result.right, checkout.result_rect.right);
      result.bottom = std::max(result.bottom, checkout.result_rect.bottom);
      maximum.left = std::min(maximum.left, checkout.max_result_rect.left);
      maximum.top = std::min(maximum.top, checkout.max_result_rect.top);
      maximum.right = std::max(maximum.right, checkout.max_result_rect.right);
      maximum.bottom = std::max(maximum.bottom, checkout.max_result_rect.bottom);
    }
  }
  extra->output->result_rect = result;
  extra->output->max_result_rect = maximum;
  return g_successful_checkouts == 3 ? PF_Err_NONE : PF_Err_INTERNAL_STRUCT_DAMAGED;
}

PF_Err SmartRender(PF_InData* in_data, PF_SmartRenderExtra* extra) {
  if (!in_data || !extra || !extra->cb || g_successful_checkouts != 3)
    return PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld* worlds[3]{};
  PF_EffectWorld* output = nullptr;
  PF_Err err = PF_Err_NONE;
  for (std::size_t i = 0; i < kCheckoutIds.size() && !err; ++i)
    err = extra->cb->checkout_layer_pixels(in_data->effect_ref, kCheckoutIds[i], &worlds[i]);
  // Both legal checkout APIs must resolve the same timed image. Keeping all
  // pixel leases alive also exposes accidental reuse of one mutable world.
  for (std::size_t i = 0; i < kCheckoutIds.size() && !err; ++i) {
    PF_ParamDef parameter{};
    err = in_data->inter.checkout_param(in_data->effect_ref, kLayerSlot,
        kTimes[i], 1, kScales[i], &parameter);
    if (err) break;
    const auto& layer = parameter.u.ld;
    if (!layer.data || layer.width != worlds[i]->width ||
        layer.height != worlds[i]->height || layer.rowbytes != worlds[i]->rowbytes) {
      err = PF_Err_INTERNAL_STRUCT_DAMAGED;
    } else {
      for (A_long y = 0; y < layer.height && !err; ++y) {
        if (std::memcmp(reinterpret_cast<const char*>(layer.data) + y * layer.rowbytes,
                reinterpret_cast<const char*>(worlds[i]->data) + y * worlds[i]->rowbytes,
                static_cast<std::size_t>(layer.rowbytes)) != 0)
          err = PF_Err_INTERNAL_STRUCT_DAMAGED;
      }
    }
    const PF_Err checked_in = in_data->inter.checkin_param(in_data->effect_ref, &parameter);
    if (!err) err = checked_in;
  }
  if (!err) err = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (!err) {
    if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_PixelFloat)))
      err = Composite<PF_PixelFloat, PF_FpShort, 1>(worlds, output);
    else if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_Pixel16)))
      err = Composite<PF_Pixel16, A_u_short, PF_MAX_CHAN16>(worlds, output);
    else
      err = Composite<PF_Pixel8, A_u_char, PF_MAX_CHAN8>(worlds, output);
  }
  for (std::size_t i = 0; i < kCheckoutIds.size(); ++i) {
    if (!worlds[i]) continue;
    const PF_Err checkin = extra->cb->checkin_layer_pixels(in_data->effect_ref, kCheckoutIds[i]);
    if (!checkin) ++g_successful_checkins;
    else if (!err) err = checkin;
  }
  if (!err && g_successful_checkins != g_successful_checkouts)
    err = PF_Err_INTERNAL_STRUCT_DAMAGED;
  return err;
}
PF_Err ClassicRender(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !output) return PF_Err_BAD_CALLBACK_PARAM;
  PF_ParamDef parameters[3]{};
  PF_EffectWorld* worlds[3]{};
  PF_Err err = PF_Err_NONE;
  std::size_t acquired = 0;
  for (std::size_t i = 0; i < kTimes.size(); ++i) {
    err = in_data->inter.checkout_param(in_data->effect_ref, kLayerSlot,
        kTimes[i], 1, kScales[i], &parameters[i]);
    if (err) break;
    ++acquired;
    worlds[i] = &parameters[i].u.ld;
  }
  if (!err) {
    if (PF_WORLD_IS_DEEP(output))
      err = Composite<PF_Pixel16, A_u_short, PF_MAX_CHAN16>(worlds, output);
    else
      err = Composite<PF_Pixel8, A_u_char, PF_MAX_CHAN8>(worlds, output);
  }
  for (std::size_t i = 0; i < acquired; ++i) {
    const auto result = in_data->inter.checkin_param(in_data->effect_ref, &parameters[i]);
    if (!err) err = result;
  }
  return err;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef* output, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE |
                            (kLayerSlot == 0 ? 0 : PF_OutFlag_WIDE_TIME_INPUT);
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
                             PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: {
      PF_ParamDef def{};
      if constexpr (kLayerSlot != 0) {
        PF_ADD_LAYER("Timed Layer", PF_LayerDefault_MYSELF, 1);
      }
      out_data->num_params = kLayerSlot == 0 ? 1 : 2;
      return PF_Err_NONE;
    }
    case PF_Cmd_SMART_PRE_RENDER:
      return SmartPreRender(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER:
      return SmartRender(in_data, static_cast<PF_SmartRenderExtra*>(extra));
    case PF_Cmd_RENDER:
      return ClassicRender(in_data, output);
    default:
      return PF_Err_NONE;
  }
}
