#define _CRT_SECURE_NO_WARNINGS
#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

#include <cstring>

// An expanding effect that also states PF_OutData::origin, which the frame
// resize probes never do (issue #984).
//
// AE_Effect.h defines the field the host copies this into,
// PF_InData::output_origin_x/y, as "the position of the top left corner of the
// input buffer in the output buffer", while the host's own frame report carries
// the opposite convention - the output's top-left in layer coordinates, so
// negative when the output grew. The classic path therefore negates one into the
// other. With every existing resize fixture leaving the origin at zero, a sign
// error there placed the frame on the wrong side of the layer origin with the
// whole suite green, so this fixture exists to make the negation observable end
// to end.
//
// Deliberately its own instrument rather than a fifth pf-frame-resize-probe
// variant: those four share one translation unit whose compiled bytes are
// recorded as frozen provenance in
// analysis/PF_FRAME_RESIZE_FLAG_RESULT_2026-07-15.json, and adding to it would
// invalidate that record for a reason unrelated to what it documents.

#ifndef AEXCOMPAT_ORIGIN_DELTA
#define AEXCOMPAT_ORIGIN_DELTA 4
#endif
#ifndef AEXCOMPAT_ORIGIN_INSET
#define AEXCOMPAT_ORIGIN_INSET 3
#endif
#ifndef AEXCOMPAT_ORIGIN_FROM_OUT_DATA
#define AEXCOMPAT_ORIGIN_FROM_OUT_DATA 0
#endif

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT | PF_OutFlag_I_EXPAND_BUFFER;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_FRAME_SETUP:
      // Grow by the delta on each axis and place the input inset inside the
      // grown buffer. The inset is smaller than the growth, so the input still
      // fits: this is a well-formed expand, not a bounds probe.
#if AEXCOMPAT_ORIGIN_FROM_OUT_DATA
      // Derived from what the host left in out_data rather than from the layer
      // parameter, which is the shape issue #984 is about: AE's Basic_3D reads
      // out_data->width/height on entry, and this host used to leave them zero.
      // Nothing else in the fixture set reads them, so without this variant a
      // regression that stopped the offer reaching a real plug-in's out_data
      // would leave every AEX-backed test green.
      if (out_data->width <= 0 || out_data->height <= 0)
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      out_data->width += AEXCOMPAT_ORIGIN_DELTA;
      out_data->height += AEXCOMPAT_ORIGIN_DELTA;
#else
      out_data->width = params[0]->u.ld.width + AEXCOMPAT_ORIGIN_DELTA;
      out_data->height = params[0]->u.ld.height + AEXCOMPAT_ORIGIN_DELTA;
#endif
      out_data->origin.h = AEXCOMPAT_ORIGIN_INSET;
      out_data->origin.v = AEXCOMPAT_ORIGIN_INSET;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      if (!output || !output->data || output->width <= 0 || output->height <= 0 ||
          output->rowbytes < output->width * 4)
        return PF_Err_BAD_CALLBACK_PARAM;
      // The host must have relayed the origin back through in_data. Reporting a
      // mismatch as an error makes the relay observable from the frame result
      // rather than only from a diagnostic dump.
      if (!in_data || in_data->output_origin_x != AEXCOMPAT_ORIGIN_INSET ||
          in_data->output_origin_y != AEXCOMPAT_ORIGIN_INSET)
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      for (A_long y = 0; y < output->height; ++y)
        std::memset(reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes,
                    0x7f, static_cast<std::size_t>(output->width) * 4);
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
