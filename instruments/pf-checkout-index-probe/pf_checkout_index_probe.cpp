// checkout_param index probe (issue #1251).
//
// Adobe's RollingShutter.aex bundle (Timewarp / Pixel Motion Blur / Rolling
// Shutter Repair share one Kronos render core) checks out param indices 29 and
// 31 - Timewarp's Matte Layer / Warp Layer slots - from Pixel Motion Blur's
// RENDER, whose PARAMS_SETUP registered only 4 params (num_params = 5). It
// zero-fills the PF_ParamDef first, treats `u.ld.data == NULL` as "no layer",
// and gates its main loop on the second checkout's return code, so the way AE
// answers an out-of-range checkout decides whether the effect renders at all.
//
// This probe registers the same 5-slot table (one popup, three sliders) and
// asks the host's checkout_param the same questions, encoding every answer as
// solid colour bands in the output so an AE reference capture (8 bpc PNG,
// tools/capture-ae-probe-oracle.ps1) turns into numbers. Per queried index the
// probe writes three bands (RGB, alpha 255):
//
//   band 3k+0: R = checkout err & 0xff, G = (err >> 8) & 0xff, B = checkin err & 0xff
//              for a checkout into a ZERO-filled def followed by checkin
//              (exactly the RollingShutter shape);
//   band 3k+1: R = data-non-null flag (255/0) after that zeroed checkout,
//              G = param_type & 0xff, B = u.ld.width & 0xff;
//   band 3k+2: R = "def touched" flag (255/0): a second checkout into a def
//              seeded with 0xAB in every byte, reporting whether AE wrote any
//              byte at all, G = err & 0xff of that seeded call, B = 0xAB
//              (band marker). The seeded def is checked in only when the host
//              returned 0 and wrote into it (an untouched 0xAB def is not a
//              checkout the host knows about).
//
// Indices, in order: 29, 31 (the RollingShutter pair), 5 (first slot past the
// table), 0 (the input layer, control: err 0, data non-null, type LAYER).
// Bands beyond the 12 used are filled with 0x40 grey. Never touches AE state
// beyond those callbacks; 8-bit only; no SmartFX.

#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <cstring>

namespace {

constexpr A_long kQueriedIndices[] = {29, 31, 5, 0};
constexpr A_long kQueryCount = 4;
constexpr A_long kBandsPerQuery = 3;
constexpr A_long kBandCount = kQueryCount * kBandsPerQuery;

struct Answer {
  PF_Err zeroed_err{};
  PF_Err zeroed_checkin_err{};
  bool zeroed_data_non_null{};
  A_long zeroed_param_type{};
  A_long zeroed_width{};
  PF_Err seeded_err{};
  bool seeded_touched{};
};

Answer query(PF_InData* in_data, A_long index) {
  Answer answer{};
  PF_ParamDef zeroed;
  std::memset(&zeroed, 0, sizeof(zeroed));
  answer.zeroed_err = PF_CHECKOUT_PARAM(in_data, index, in_data->current_time,
                                        in_data->time_step, in_data->time_scale,
                                        &zeroed);
  answer.zeroed_data_non_null = zeroed.u.ld.data != nullptr;
  answer.zeroed_param_type = zeroed.param_type;
  answer.zeroed_width = zeroed.u.ld.width;
  // Checked in unconditionally, error or not: the RollingShutter host trace
  // shows the render core checking both zeroed defs in after a refused
  // checkout as well as after an accepted one (docs/
  // CHECKOUT_PARAM_INDEX_OBSERVATION_2026-08-17.md section 2), and the
  // checkin answer to that shape is part of what is being asked. The seeded
  // call below is guarded on purpose; the asymmetry is deliberate.
  answer.zeroed_checkin_err = PF_CHECKIN_PARAM(in_data, &zeroed);

  PF_ParamDef seeded;
  std::memset(&seeded, 0xAB, sizeof(seeded));
  answer.seeded_err = PF_CHECKOUT_PARAM(in_data, index, in_data->current_time,
                                        in_data->time_step, in_data->time_scale,
                                        &seeded);
  const auto* bytes = reinterpret_cast<const unsigned char*>(&seeded);
  for (std::size_t i = 0; i < sizeof(seeded); ++i) {
    if (bytes[i] != 0xAB) {
      answer.seeded_touched = true;
      break;
    }
  }
  // Only a def the host actually wrote is safe to hand back; a 0xAB-filled def
  // returned untouched with err 0 stays out of checkin on purpose.
  if (answer.seeded_err == PF_Err_NONE && answer.seeded_touched)
    PF_CHECKIN_PARAM(in_data, &seeded);
  return answer;
}

void fill_band(PF_LayerDef* output, A_long band, A_long band_width,
               unsigned char r, unsigned char g, unsigned char b) {
  const A_long left = band * band_width;
  A_long right = left + band_width;
  if (band == kBandCount - 1 || right > output->width) right = output->width;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel*>(
        reinterpret_cast<char*>(output->data) + y * output->rowbytes);
    for (A_long x = left; x < right && x < output->width; ++x) {
      row[x].alpha = 255;
      row[x].red = r;
      row[x].green = g;
      row[x].blue = b;
    }
  }
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !output || !output->data || output->width < kBandCount ||
      output->height < 1)
    return PF_Err_BAD_CALLBACK_PARAM;
  const A_long band_width = output->width / kBandCount;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel*>(
        reinterpret_cast<char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      row[x].alpha = 255;
      row[x].red = row[x].green = row[x].blue = 0x40;
    }
  }
  for (A_long k = 0; k < kQueryCount; ++k) {
    const Answer a = query(in_data, kQueriedIndices[k]);
    fill_band(output, 3 * k + 0, band_width,
              static_cast<unsigned char>(a.zeroed_err & 0xff),
              static_cast<unsigned char>((a.zeroed_err >> 8) & 0xff),
              static_cast<unsigned char>(a.zeroed_checkin_err & 0xff));
    fill_band(output, 3 * k + 1, band_width, a.zeroed_data_non_null ? 255 : 0,
              static_cast<unsigned char>(a.zeroed_param_type & 0xff),
              static_cast<unsigned char>(a.zeroed_width & 0xff));
    fill_band(output, 3 * k + 2, band_width, a.seeded_touched ? 255 : 0,
              static_cast<unsigned char>(a.seeded_err & 0xff), 0xAB);
  }
  return PF_Err_NONE;
}

PF_Err params_setup(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef def;
  // Same slot shape as Pixel Motion Blur: popup, three sliders.
  AEFX_CLR_STRUCT(def);
  PF_ADD_POPUP("Shutter Control", 2, 1, "Manual|Automatic", 1);
  AEFX_CLR_STRUCT(def);
  PF_ADD_FLOAT_SLIDERX("Shutter Angle", 0, 720, 0, 720, 180, PF_Precision_INTEGER,
                       0, 0, 2);
  AEFX_CLR_STRUCT(def);
  PF_ADD_FLOAT_SLIDERX("Shutter Samples", 1, 64, 1, 64, 4, PF_Precision_INTEGER,
                       0, 0, 3);
  AEFX_CLR_STRUCT(def);
  PF_ADD_FLOAT_SLIDERX("Vector Detail", 1, 100, 1, 100, 20, PF_Precision_INTEGER,
                       0, 0, 4);
  out_data->num_params = 5;
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data,
                                        PF_ParamDef*[], PF_LayerDef* output,
                                        void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return params_setup(in_data, out_data);
    case PF_Cmd_RENDER:
      return render(in_data, output);
    default:
      return PF_Err_NONE;
  }
}
