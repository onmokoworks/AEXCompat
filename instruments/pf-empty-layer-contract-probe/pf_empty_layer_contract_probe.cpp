// Issue #1679: turn the optional SmartFX layer callback contract into pixels.
// Each output pixel is one bit, MSB first, of the 32-bit words documented in
// tests/test_pf_empty_layer_contract_probe.py. Black is 0 and white is 1.
// The image is the evidence; a successful worker status alone is not an oracle.
#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectSuites.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <array>
#include <cstdint>
#include <new>

namespace {

constexpr A_long kMapSlot = 1;
constexpr A_long kInputId = 0;
constexpr A_long kMapId = 1;
constexpr uint32_t kMissing = 0xffffffffu;
constexpr size_t kWordCount = 32;
using Words = std::array<uint32_t, kWordCount>;

struct Snapshot {
  PF_Err map_pre_error = PF_Err_NONE;
  PF_CheckoutResult map_pre{};
};

void DeleteSnapshot(void* pointer) { delete static_cast<Snapshot*>(pointer); }

uint32_t Bits(A_long value) { return static_cast<uint32_t>(value); }

PF_Err QueryFormat(PF_InData* in_data, PF_EffectWorld* world,
                   PF_PixelFormat* format) {
  if (!in_data->pica_basicP || !world) return PF_Err_BAD_CALLBACK_PARAM;
  const void* acquired = nullptr;
  PF_Err err = in_data->pica_basicP->AcquireSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2, &acquired);
  if (err || !acquired) return err ? err : PF_Err_BAD_CALLBACK_PARAM;
  err = static_cast<const PF_WorldSuite2*>(acquired)->PF_GetPixelFormat(
      world, format);
  const PF_Err release = in_data->pica_basicP->ReleaseSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2);
  return err ? err : release;
}

uint32_t To8(float value) {
  if (!(value >= 0.0f)) return 0;
  if (value >= 1.0f) return 255;
  return static_cast<uint32_t>(value * 255.0f + 0.5f);
}

void Sample(PF_EffectWorld* world, PF_PixelFormat format, Words* words) {
  if (!world || !world->data || world->width <= 0 || world->height <= 0)
    return;
  if (format == PF_PixelFormat_ARGB32) {
    const auto* pixel = static_cast<const PF_Pixel8*>(world->data);
    (*words)[23] = pixel->alpha;
    (*words)[24] = pixel->red;
    (*words)[25] = pixel->green;
    (*words)[26] = pixel->blue;
  } else if (format == PF_PixelFormat_ARGB64) {
    const auto* pixel = reinterpret_cast<const PF_Pixel16*>(world->data);
    (*words)[23] = (static_cast<uint32_t>(pixel->alpha) * 255 + 16384) / 32768;
    (*words)[24] = (static_cast<uint32_t>(pixel->red) * 255 + 16384) / 32768;
    (*words)[25] = (static_cast<uint32_t>(pixel->green) * 255 + 16384) / 32768;
    (*words)[26] = (static_cast<uint32_t>(pixel->blue) * 255 + 16384) / 32768;
  } else if (format == PF_PixelFormat_ARGB128) {
    const auto* pixel = reinterpret_cast<const PF_PixelFloat*>(world->data);
    (*words)[23] = To8(pixel->alpha);
    (*words)[24] = To8(pixel->red);
    (*words)[25] = To8(pixel->green);
    (*words)[26] = To8(pixel->blue);
  }
}

template <typename Pixel, typename Channel>
void Paint(PF_EffectWorld* output, const Words& words, Channel white) {
  constexpr size_t kBits = kWordCount * 32;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      const size_t bit = static_cast<size_t>(y) * output->width + x;
      const Channel color = bit < kBits &&
                                    (words[bit / 32] & (uint32_t{1} << (31 - bit % 32)))
                                ? white
                                : Channel{};
      row[x].alpha = white;
      row[x].red = color;
      row[x].green = color;
      row[x].blue = color;
    }
  }
}

PF_Err SmartPreRender(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  auto* snapshot = new (std::nothrow) Snapshot;
  if (!snapshot) return PF_Err_OUT_OF_MEMORY;
  PF_CheckoutResult primary{};
  PF_RenderRequest request = extra->input->output_request;
  const PF_Err err = extra->cb->checkout_layer(
      in_data->effect_ref, 0, kInputId, &request, in_data->current_time,
      in_data->time_step, in_data->time_scale, &primary);
  if (err) {
    delete snapshot;
    return err;
  }
  snapshot->map_pre_error = extra->cb->checkout_layer(
      in_data->effect_ref, kMapSlot, kMapId, &request, in_data->current_time,
      in_data->time_step, in_data->time_scale, &snapshot->map_pre);
  // Even a failed optional-layer checkout is data to encode, not a reason to
  // suppress SmartRender. Primary geometry keeps the output non-empty.
  extra->output->result_rect = primary.result_rect;
  extra->output->max_result_rect = primary.max_result_rect;
  extra->output->pre_render_data = snapshot;
  extra->output->delete_pre_render_data_func = DeleteSnapshot;
  return PF_Err_NONE;
}

PF_Err SmartRender(PF_InData* in_data, PF_SmartRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  const auto* snapshot = static_cast<const Snapshot*>(extra->input->pre_render_data);
  if (!snapshot) return PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld* primary = nullptr;
  PF_EffectWorld* map = nullptr;
  PF_EffectWorld* output = nullptr;
  const PF_Err primary_err = extra->cb->checkout_layer_pixels(
      in_data->effect_ref, kInputId, &primary);
  if (primary_err) return primary_err;
  PF_Err map_err = PF_Err_NONE;
  const bool map_pixels_attempted = snapshot->map_pre_error == PF_Err_NONE;
  if (map_pixels_attempted)
    map_err = extra->cb->checkout_layer_pixels(in_data->effect_ref, kMapId, &map);
  const PF_Err output_err = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (output_err || !output || !output->data) {
    extra->cb->checkin_layer_pixels(in_data->effect_ref, kInputId);
    if (map_pixels_attempted && !map_err)
      extra->cb->checkin_layer_pixels(in_data->effect_ref, kMapId);
    return output_err ? output_err : PF_Err_BAD_CALLBACK_PARAM;
  }

  PF_PixelFormat map_format = static_cast<PF_PixelFormat>(0);
  const PF_Err map_format_err = map ? QueryFormat(in_data, map, &map_format)
                                    : PF_Err_BAD_CALLBACK_PARAM;
  PF_PixelFormat output_format = static_cast<PF_PixelFormat>(0);
  const PF_Err output_format_err = QueryFormat(in_data, output, &output_format);
  Words words{};
  words.fill(kMissing);
  words[0] = 0x554c5031u;  // "ULP1"
  words[1] = 1;            // schema version
  words[2] = static_cast<uint32_t>(kWordCount);
  words[3] = Bits(snapshot->map_pre_error);
  words[4] = Bits(snapshot->map_pre.result_rect.left);
  words[5] = Bits(snapshot->map_pre.result_rect.top);
  words[6] = Bits(snapshot->map_pre.result_rect.right);
  words[7] = Bits(snapshot->map_pre.result_rect.bottom);
  words[8] = Bits(snapshot->map_pre.max_result_rect.left);
  words[9] = Bits(snapshot->map_pre.max_result_rect.top);
  words[10] = Bits(snapshot->map_pre.max_result_rect.right);
  words[11] = Bits(snapshot->map_pre.max_result_rect.bottom);
  words[12] = map_pixels_attempted ? Bits(map_err) : kMissing;
  words[13] = map ? 1 : 0;
  words[14] = map && map->data ? 1 : 0;
  words[15] = map ? Bits(map->width) : kMissing;
  words[16] = map ? Bits(map->height) : kMissing;
  words[17] = map ? Bits(map->rowbytes) : kMissing;
  words[18] = map ? Bits(map->extent_hint.left) : kMissing;
  words[19] = map ? Bits(map->extent_hint.top) : kMissing;
  words[20] = map ? Bits(map->extent_hint.right) : kMissing;
  words[21] = map ? Bits(map->extent_hint.bottom) : kMissing;
  words[22] = Bits(map_format_err);
  Sample(map, map_format, &words);
  words[27] = static_cast<uint32_t>(map_format);
  words[28] = Bits(output_format_err);
  words[29] = static_cast<uint32_t>(output_format);
  words[30] = primary && primary->data ? 1 : 0;
  words[31] = output->data ? 1 : 0;

  if (output_format_err || output->width <= 0 || output->height <= 0 ||
      static_cast<size_t>(output->width) * output->height < kWordCount * 32) {
    extra->cb->checkin_layer_pixels(in_data->effect_ref, kInputId);
    if (map_pixels_attempted && !map_err)
      extra->cb->checkin_layer_pixels(in_data->effect_ref, kMapId);
    return output_format_err ? output_format_err : PF_Err_BAD_CALLBACK_PARAM;
  }
  if (output_format == PF_PixelFormat_ARGB32)
    Paint<PF_Pixel8, A_u_char>(output, words, 255);
  else if (output_format == PF_PixelFormat_ARGB64)
    Paint<PF_Pixel16, A_u_short>(output, words, 32768);
  else if (output_format == PF_PixelFormat_ARGB128)
    Paint<PF_PixelFloat, float>(output, words, 1.0f);
  else {
    extra->cb->checkin_layer_pixels(in_data->effect_ref, kInputId);
    if (map_pixels_attempted && !map_err)
      extra->cb->checkin_layer_pixels(in_data->effect_ref, kMapId);
    return PF_Err_BAD_CALLBACK_PARAM;
  }

  const PF_Err input_checkin = extra->cb->checkin_layer_pixels(in_data->effect_ref, kInputId);
  const PF_Err map_checkin = !map_pixels_attempted || map_err ? PF_Err_NONE
      : extra->cb->checkin_layer_pixels(in_data->effect_ref, kMapId);
  return input_checkin ? input_checkin : map_checkin;
}

PF_Err ParamsSetup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def{};
  AEFX_CLR_STRUCT(def);
  PF_ADD_LAYER("Optional Map", PF_LayerDefault_NONE, kMapSlot);
  out_data->num_params = kMapSlot + 1;
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef*[],
                                       PF_LayerDef*, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
                             PF_OutFlag2_FLOAT_COLOR_AWARE;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return ParamsSetup(in_data, out_data);
    case PF_Cmd_SMART_PRE_RENDER:
      return SmartPreRender(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER:
      return SmartRender(in_data, static_cast<PF_SmartRenderExtra*>(extra));
    default:
      return PF_Err_NONE;
  }
}
