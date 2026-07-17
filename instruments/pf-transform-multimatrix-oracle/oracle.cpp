#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"

#include <array>
#include <cstring>
#include <limits>

namespace {

PF_Err checked_world_bytes(const PF_EffectWorld& world, size_t* bytes) {
  if (!bytes || world.rowbytes < 0 || world.height < 0) return PF_Err_BAD_CALLBACK_PARAM;
  const size_t rowbytes = static_cast<size_t>(world.rowbytes);
  const size_t height = static_cast<size_t>(world.height);
  if (height && rowbytes > std::numeric_limits<size_t>::max() / height) {
    return PF_Err_BAD_CALLBACK_PARAM;
  }
  *bytes = rowbytes * height;
  return PF_Err_NONE;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in, PF_OutData* out,
    PF_ParamDef*[], PF_LayerDef* output, void*) {
  if (cmd == PF_Cmd_GLOBAL_SETUP) {
    if (!out) return PF_Err_BAD_CALLBACK_PARAM;
    out->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
    out->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_DEEP_COLOR_AWARE;
    return PF_Err_NONE;
  }
  if (cmd == PF_Cmd_PARAMS_SETUP) {
    if (!out) return PF_Err_BAD_CALLBACK_PARAM;
    out->num_params = 1;
    return PF_Err_NONE;
  }
  if (cmd != PF_Cmd_RENDER) return PF_Err_NONE;
  if (!in || !in->pica_basicP || !output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  size_t output_bytes = 0;
  PF_Err err = checked_world_bytes(*output, &output_bytes);
  if (err) return err;
  if (in->current_time == 0) {
    std::memset(output->data, 0, output_bytes);
    return PF_Err_NONE;
  }
  const PF_WorldTransformSuite1* transforms = nullptr;
  const PF_WorldSuite2* worlds = nullptr;
  err = static_cast<PF_Err>(in->pica_basicP->AcquireSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1,
      reinterpret_cast<const void**>(&transforms)));
  if (!err && !transforms) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) err = static_cast<PF_Err>(in->pica_basicP->AcquireSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2, reinterpret_cast<const void**>(&worlds)));
  if (!err && !worlds) err = PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld source{};
  bool source_live = false;
  if (!err) {
    err = worlds->PF_NewWorld(in->effect_ref, 3, 1, TRUE, PF_PixelFormat_ARGB32, &source);
    source_live = !err;
  }
  if (!err) {
    std::memset(source.data, 0, static_cast<size_t>(source.rowbytes));
    *reinterpret_cast<PF_Pixel8*>(source.data) = PF_Pixel8{255, 255, 255, 255};
    std::memset(output->data, 0, output_bytes);
    PF_CompositeMode mode{};
    mode.xfer = PF_Xfer_COPY; mode.opacity = PF_MAX_CHAN8;
    mode.opacitySu = PF_MAX_CHAN16; mode.rgb_only = FALSE;
    const std::array<PF_FloatMatrix, 2> matrices{{
        {{{1,0,0},{0,1,0},{0,0,1}}},
         {{{1,0,0},{0,1,0},{2,0,1}}}}};
    PF_Rect bounds{0, 0, output->width, output->height};
    err = transforms->transform_world(in->effect_ref, PF_Quality_LO, PF_MF_Alpha_STRAIGHT,
        PF_Field_FRAME, &source, &mode, nullptr, matrices.data(), 2, TRUE, &bounds, output);
  }
  if (source_live) {
    const PF_Err dispose = worlds->PF_DisposeWorld(in->effect_ref, &source);
    if (!err) err = dispose;
  }
  if (worlds) in->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2);
  if (transforms) in->pica_basicP->ReleaseSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1);
  return err;
}
