#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectSuites.h"

#include <cmath>
#include <cstring>

namespace {
PF_Err setup_parameters(PF_InData* in_data, PF_OutData* out_data) {
  PF_ParamDef definition{};
  definition.param_type = PF_Param_PATH;
  definition.uu.id = 1;
  std::strcpy(definition.name, "Path");
  definition.u.path_d.dephault = 0;
  const PF_Err error = PF_ADD_PARAM(in_data, -1, &definition);
  if (!error) out_data->num_params = 2;
  return error;
}

PF_Err exercise_path_data(PF_InData* in_data, PF_ParamDef* params[]) {
  const PF_PathQuerySuite1* query = nullptr;
  const PF_PathDataSuite1* data = nullptr;
  PF_Err error = in_data->pica_basicP->AcquireSuite(
      kPFPathQuerySuite, kPFPathQuerySuiteVersion1,
      reinterpret_cast<const void**>(&query));
  if (!error) error = in_data->pica_basicP->AcquireSuite(
      kPFPathDataSuite, kPFPathDataSuiteVersion1,
      reinterpret_cast<const void**>(&data));
  if (error || !query || !data) return error ? error : PF_Err_BAD_CALLBACK_PARAM;

  const PF_PathID id = params[1]->u.path_d.path_id;
  A_long path_count = 0;
  PF_PathID observed_id = 0;
  PF_PathOutlinePtr path = nullptr;
  PF_PathSegPrepPtr prep = nullptr;
  PF_Boolean open = TRUE;
  A_long segments = 0;
  PF_PathVertex first{}, second{};
  PF_FpLong length = 0.0, start_x = 0.0, start_y = 0.0;
  PF_FpLong middle_x = 0.0, middle_y = 0.0, end_x = 0.0, end_y = 0.0;
  PF_FpLong deriv_x = 0.0, deriv_y = 0.0;
  PF_Boolean inverted = TRUE;
  PF_MaskMode mode = PF_MaskMode_NONE;
  char name[PF_MAX_PATH_NAME_LEN + 1]{};

  if (!error) error = query->PF_NumPaths(in_data->effect_ref, &path_count);
  if (!error && path_count != 1) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = query->PF_PathInfo(in_data->effect_ref, 0, &observed_id);
  if (!error && (id == 0 || observed_id != id)) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = query->PF_CheckoutPath(in_data->effect_ref, id,
      in_data->current_time, in_data->time_step, in_data->time_scale, &path);
  if (!error && !path) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = data->PF_PathIsOpen(in_data->effect_ref, path, &open);
  if (!error && open) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = data->PF_PathNumSegments(in_data->effect_ref, path, &segments);
  if (!error && segments != 4) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = data->PF_PathVertexInfo(in_data->effect_ref, path, 0, &first);
  if (!error) error = data->PF_PathVertexInfo(in_data->effect_ref, path, 1, &second);
  if (!error) error = data->PF_PathPrepareSegLength(in_data->effect_ref, path, 0, 64, &prep);
  if (!error) error = data->PF_PathGetSegLength(in_data->effect_ref, path, 0, &prep, &length);
  if (!error && (!(length > 0.0) || !std::isfinite(length))) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = data->PF_PathEvalSegLength(
      in_data->effect_ref, path, &prep, 0, 0.0, &start_x, &start_y);
  if (!error) error = data->PF_PathEvalSegLength(
      in_data->effect_ref, path, &prep, 0, length * 0.5, &middle_x, &middle_y);
  if (!error) error = data->PF_PathEvalSegLengthDeriv1(
      in_data->effect_ref, path, &prep, 0, length, &end_x, &end_y, &deriv_x, &deriv_y);
  const auto near = [](double a, double b) { return std::abs(a - b) < 0.02; };
  if (!error && (!near(start_x, first.x) || !near(start_y, first.y) ||
                 !near(end_x, second.x) || !near(end_y, second.y) ||
                 !std::isfinite(middle_x) || !std::isfinite(middle_y) ||
                 !near(std::hypot(deriv_x, deriv_y), 1.0))) {
    error = PF_Err_BAD_CALLBACK_PARAM;
  }
  if (!error) error = data->PF_PathIsInverted(in_data->effect_ref, id, &inverted);
  if (!error && inverted) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = data->PF_PathGetMaskMode(in_data->effect_ref, id, &mode);
  if (!error && mode != PF_MaskMode_ADD) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = data->PF_PathGetName(in_data->effect_ref, id, name);
  if (!error && std::strcmp(name, "Mask 1") != 0) error = PF_Err_BAD_CALLBACK_PARAM;

  if (prep) {
    const PF_Err cleanup_error = data->PF_PathCleanupSegLength(
        in_data->effect_ref, path, 0, &prep);
    if (!error) error = cleanup_error;
  }
  if (path) {
    const PF_Err checkin_error = query->PF_CheckinPath(in_data->effect_ref, id, FALSE, path);
    if (!error) error = checkin_error;
  }
  const PF_Err data_release = in_data->pica_basicP->ReleaseSuite(
      kPFPathDataSuite, kPFPathDataSuiteVersion1);
  const PF_Err query_release = in_data->pica_basicP->ReleaseSuite(
      kPFPathQuerySuite, kPFPathQuerySuiteVersion1);
  if (!error) error = data_release ? data_release : query_release;
  return error;
}

PF_Err copy_input(PF_ParamDef* params[], PF_LayerDef* output) {
  const PF_LayerDef* input = &params[0]->u.ld;
  const A_long rows = input->height < output->height ? input->height : output->height;
  const A_long bytes = input->rowbytes < output->rowbytes ? input->rowbytes : output->rowbytes;
  for (A_long y = 0; y < rows; ++y) {
    std::memcpy(reinterpret_cast<char*>(output->data) + y * output->rowbytes,
                reinterpret_cast<const char*>(input->data) + y * input->rowbytes, bytes);
  }
  return PF_Err_NONE;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef* params[],
                                        PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      return setup_parameters(in_data, out_data);
    case PF_Cmd_RENDER: {
      const PF_Err error = exercise_path_data(in_data, params);
      if (error) return error;
      return copy_input(params, output);
    }
    default:
      return PF_Err_NONE;
  }
}
