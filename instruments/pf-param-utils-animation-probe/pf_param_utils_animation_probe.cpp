#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectSuites.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#include <cstdio>
#include <cstring>

namespace {
constexpr PF_ParamIndex kAnimated = 1;
constexpr A_u_long kScale = 24;

bool is_time(A_long time, A_u_long scale, A_long expected) {
  return scale != 0 && static_cast<long long>(time) * kScale ==
                           static_cast<long long>(expected) * scale;
}

void fill(PF_LayerDef* output, A_u_char red, A_u_char green, A_u_char blue) {
  if (!output || !output->data) return;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel8*>(
        reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) row[x] = {255, red, green, blue};
  }
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: {
      PF_ParamDef def{};
      PF_ADD_FLOAT_SLIDERX("Animated", -100, 100, -100, 100, 10, 1,
                           PF_ValueDisplayFlag_NONE, 0, kAnimated);
      out_data->num_params = 2;
      return PF_Err_NONE;
    }
    case PF_Cmd_RENDER: {
      unsigned failures = 0;
      const PF_ParamUtilsSuite3* suite = nullptr;
      const SPErr acquired = in_data->pica_basicP->AcquireSuite(
          kPFParamUtilsSuite, kPFParamUtilsSuiteVersion3,
          reinterpret_cast<const void**>(&suite));
      if (acquired || !suite) {
        failures |= 1u;
      } else {
        PF_KeyIndex count = 0;
        if (suite->PF_GetKeyframeCount(in_data->effect_ref, kAnimated, &count) || count != 3)
          failures |= 2u;

        const PF_TimeDir dirs[] = {PF_TimeDir_GREATER_THAN, PF_TimeDir_GREATER_THAN_OR_EQUAL,
                                   PF_TimeDir_LESS_THAN, PF_TimeDir_LESS_THAN_OR_EQUAL};
        const PF_KeyIndex indices[] = {2, 1, 0, 1};
        const A_long times[] = {24, 12, 0, 12};
        for (int i = 0; i < 4; ++i) {
          PF_Boolean found = FALSE; PF_KeyIndex index = PF_KeyIndex_NONE;
          A_long time = -1; A_u_long scale = 0;
          if (suite->PF_FindKeyframeTime(in_data->effect_ref, kAnimated, 12, kScale,
                                        dirs[i], &found, &index, &time, &scale) ||
              !found || index != indices[i] || !is_time(time, scale, times[i])) failures |= 4u;
        }

        for (PF_KeyIndex i = 0; i < 3; ++i) {
          A_long time = -1; A_u_long scale = 0;
          if (suite->PF_KeyIndexToTime(in_data->effect_ref, kAnimated, i, &time, &scale) ||
              !is_time(time, scale, i * 12)) failures |= 8u;
        }

        A_long time = -1; A_u_long scale = 0;
        if (suite->PF_CheckoutKeyframe(in_data->effect_ref, kAnimated, 0, &time, &scale, nullptr) ||
            !is_time(time, scale, 0)) failures |= 16u;

        PF_ParamDef value_only{};
        if (suite->PF_CheckoutKeyframe(in_data->effect_ref, kAnimated, 1, nullptr, nullptr,
                                      &value_only) || value_only.u.fs_d.value != 20.0 ||
            suite->PF_CheckinKeyframe(in_data->effect_ref, &value_only)) failures |= 32u;

        PF_ParamDef both{}; time = -1; scale = 0;
        if (suite->PF_CheckoutKeyframe(in_data->effect_ref, kAnimated, 2, &time, &scale, &both) ||
            !is_time(time, scale, 24) || both.u.fs_d.value != 30.0 ||
            suite->PF_CheckinKeyframe(in_data->effect_ref, &both)) failures |= 64u;
        if (!suite->PF_CheckinKeyframe(in_data->effect_ref, &both)) failures |= 128u;
        PF_ParamDef foreign{};
        if (!suite->PF_CheckinKeyframe(in_data->effect_ref, &foreign)) failures |= 256u;

        PF_Boolean identical = FALSE;
        if (suite->PF_IsIdenticalCheckout(in_data->effect_ref, kAnimated, 12, 1, 24,
                                          1, 1, 2, &identical) || !identical) failures |= 512u;
        identical = TRUE;
        if (suite->PF_IsIdenticalCheckout(in_data->effect_ref, kAnimated, 0, 1, 24,
                                          24, 1, 24, &identical) || identical) failures |= 1024u;
      }
      SPErr released = 0;
      if (!acquired && suite)
        released = in_data->pica_basicP->ReleaseSuite(kPFParamUtilsSuite,
                                                       kPFParamUtilsSuiteVersion3);
      if (released) failures |= 2048u;
      std::snprintf(out_data->return_msg, sizeof(out_data->return_msg),
                    "PFPUAP:v1 status=%s mask=%u count=3 find=4 index=3 checkout=T,V,B checkin=ok reject=double,foreign identical=T,F lease=released",
                    failures ? "fail" : "pass", failures);
      fill(output, static_cast<A_u_char>(failures & 255u), failures ? 0 : 211,
           static_cast<A_u_char>((failures >> 8) & 255u));
      return PF_Err_NONE;
    }
    default:
      return PF_Err_NONE;
  }
}
