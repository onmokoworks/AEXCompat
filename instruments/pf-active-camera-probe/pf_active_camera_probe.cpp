#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_GeneralPlug.h"

#include <cmath>
#include <array>
#include <cstddef>
#include <cstring>

namespace {

bool transform_point(const A_Matrix4& matrix,
    const std::array<double, 4>& point, std::array<double, 4>& result) {
  result = {};
  // Public SDK mathematical row-vector convention, independent of the host's
  // internal matrix implementation. Include homogeneous w in the oracle.
  for (std::size_t column = 0; column < 4; ++column)
    for (std::size_t row = 0; row < 4; ++row) {
      if (!std::isfinite(matrix.mat[row][column])) return false;
      result[column] += point[row] * matrix.mat[row][column];
    }
  for (double component : result)
    if (!std::isfinite(component)) return false;
  return std::abs(result[3] - 1.0) < 1e-9;
}

PF_Err render(PF_InData* in, PF_LayerDef* output) {
  if (!in || !in->pica_basicP || !output || !output->data ||
      output->width < 3 || output->height < 1 || in->time_scale == 0)
    return PF_Err_BAD_CALLBACK_PARAM;

  const AEGP_PFInterfaceSuite1* interface_suite = nullptr;
  PF_Err err = static_cast<PF_Err>(in->pica_basicP->AcquireSuite(
      kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1,
      reinterpret_cast<const void**>(&interface_suite)));
  if (!err && (!interface_suite || !interface_suite->AEGP_GetEffectCamera ||
      !interface_suite->AEGP_GetEffectCameraMatrix))
    err = PF_Err_INVALID_CALLBACK;

  A_Time time{in->current_time, in->time_scale};
  AEGP_LayerH camera = nullptr;
  if (!err) {
    err = static_cast<PF_Err>(interface_suite->AEGP_GetEffectCamera(
        in->effect_ref, &time, &camera));
  }

  AEGP_ObjectType type = AEGP_ObjectType_NONE;
  A_long id = 0;
  A_Matrix4 matrix{};
  A_FpLong zoom = 0.0;
  A_short width = 0, height = 0;
  const AEGP_LayerSuite9* layer_suite = nullptr;
  std::array<double, 4> camera_origin{}, camera_x{}, camera_y{};
  if (!err && camera) {
    err = static_cast<PF_Err>(in->pica_basicP->AcquireSuite(
        kAEGPLayerSuite, kAEGPLayerSuiteVersion9,
        reinterpret_cast<const void**>(&layer_suite)));
    if (!err && (!layer_suite || !layer_suite->AEGP_GetLayerObjectType ||
        !layer_suite->AEGP_GetLayerID)) err = PF_Err_INVALID_CALLBACK;
    if (!err) {
      err = static_cast<PF_Err>(
          layer_suite->AEGP_GetLayerObjectType(camera, &type));
    }
    if (!err) {
      err = static_cast<PF_Err>(layer_suite->AEGP_GetLayerID(camera, &id));
    }
    if (!err) {
      err = static_cast<PF_Err>(interface_suite->AEGP_GetEffectCameraMatrix(
          in->effect_ref, &time, &matrix, &zoom, &width, &height));
    }
    if (!err && (!std::isfinite(zoom) || zoom < 0.0 ||
        !transform_point(matrix, {{0,0,0,1}}, camera_origin) ||
        !transform_point(matrix, {{1,0,0,1}}, camera_x) ||
        !transform_point(matrix, {{0,1,0,1}}, camera_y)))
      err = PF_Err_BAD_CALLBACK_PARAM;
  }

  A_long layer_ids[3]{};
  AEGP_LayerFlags layer_flags = 0;
  A_Matrix4 layer_world{};
  std::array<double, 4> layer_origin{}, layer_x{}, layer_y{};
  if (!err && output->height >= 2) {
    if (!layer_suite) {
      err = static_cast<PF_Err>(in->pica_basicP->AcquireSuite(
          kAEGPLayerSuite, kAEGPLayerSuiteVersion9,
          reinterpret_cast<const void**>(&layer_suite)));
    }
    AEGP_LayerH layer = nullptr;
    if (!err && (!layer_suite || !interface_suite->AEGP_GetEffectLayer ||
        !layer_suite->AEGP_GetLayerParent || !layer_suite->AEGP_GetLayerID ||
        !layer_suite->AEGP_GetLayerToWorldXform || !layer_suite->AEGP_GetLayerFlags))
      err = PF_Err_INVALID_CALLBACK;
    if (!err) err = static_cast<PF_Err>(
        interface_suite->AEGP_GetEffectLayer(in->effect_ref, &layer));
    if (!err && !layer) err = PF_Err_BAD_CALLBACK_PARAM;
    if (!err) err = static_cast<PF_Err>(layer_suite->AEGP_GetLayerFlags(layer, &layer_flags));
    if (!err) err = static_cast<PF_Err>(
        layer_suite->AEGP_GetLayerToWorldXform(layer, &time, &layer_world));
    for (std::size_t depth = 0; !err && layer && depth < 3; ++depth) {
      err = static_cast<PF_Err>(layer_suite->AEGP_GetLayerID(layer, &layer_ids[depth]));
      AEGP_LayerH parent = nullptr;
      if (!err) err = static_cast<PF_Err>(layer_suite->AEGP_GetLayerParent(layer, &parent));
      layer = parent;
    }
    if (!err && (!transform_point(layer_world, {{0,0,0,1}}, layer_origin) ||
        !transform_point(layer_world, {{1,0,0,1}}, layer_x) ||
        !transform_point(layer_world, {{0,1,0,1}}, layer_y)))
      err = PF_Err_BAD_CALLBACK_PARAM;
  }

  if (!err) {
    std::memset(output->data, 0,
        static_cast<size_t>(output->rowbytes) * output->height);
    for (A_long y = 0; y < output->height; ++y) {
      auto* row = reinterpret_cast<PF_Pixel8*>(
          reinterpret_cast<A_u_char*>(output->data) + y * output->rowbytes);
      for (A_long x = 0; x < output->width; ++x) row[x].alpha = 255;
    }
    if (camera) {
      auto* row = static_cast<PF_Pixel8*>(output->data);
      row[0].red = static_cast<A_u_char>(type);
      row[0].green = static_cast<A_u_char>(id & 0xff);
      row[0].blue = static_cast<A_u_char>(std::lround(zoom / 10.0));
      row[1].red = static_cast<A_u_char>(std::lround(camera_origin[0]));
      row[1].green = static_cast<A_u_char>(std::lround(camera_origin[1]));
      row[1].blue = static_cast<A_u_char>(std::lround(camera_origin[2]));
      row[2].red = static_cast<A_u_char>(std::lround(std::abs(camera_x[0] - camera_origin[0]) * 100.0));
      row[2].green = static_cast<A_u_char>(std::lround(std::abs(camera_x[1] - camera_origin[1]) * 100.0));
      row[2].blue = static_cast<A_u_char>(std::lround(std::abs(camera_y[0] - camera_origin[0]) * 100.0));
    }
    if (output->height >= 2) {
      auto* row = reinterpret_cast<PF_Pixel8*>(
          reinterpret_cast<A_u_char*>(output->data) + output->rowbytes);
      row[0].red = static_cast<A_u_char>(layer_ids[0] & 0xff);
      row[0].green = static_cast<A_u_char>(layer_ids[1] & 0xff);
      row[0].blue = static_cast<A_u_char>(layer_ids[2] & 0xff);
      row[1].red = static_cast<A_u_char>(std::lround(layer_origin[0] + 128.0));
      row[1].green = static_cast<A_u_char>(std::lround(layer_origin[1] + 128.0));
      row[1].blue = static_cast<A_u_char>(std::lround(layer_origin[2] + 128.0));
      row[2].red = static_cast<A_u_char>(std::lround(std::abs(layer_x[0] - layer_origin[0]) * 10.0));
      row[2].green = static_cast<A_u_char>(std::lround(std::abs(layer_y[0] - layer_origin[0]) * 10.0));
      row[2].blue = static_cast<A_u_char>(std::lround(std::abs(layer_x[1] - layer_origin[1]) * 10.0));
      if (output->width >= 4)
        row[3].red = (layer_flags & AEGP_LayerFlag_LAYER_IS_3D) != 0 ? 1 : 0;
    }
  }
  if (layer_suite) {
    const PF_Err release = static_cast<PF_Err>(in->pica_basicP->ReleaseSuite(
        kAEGPLayerSuite, kAEGPLayerSuiteVersion9));
    if (!err) err = release;
  }
  if (interface_suite) {
    const PF_Err release = static_cast<PF_Err>(in->pica_basicP->ReleaseSuite(
        kAEGPPFInterfaceSuite, kAEGPPFInterfaceSuiteVersion1));
    if (!err) err = release;
  }
  return err;
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in,
    PF_OutData* out, PF_ParamDef*[], PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      if (!out) return PF_Err_BAD_CALLBACK_PARAM;
      out->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      if (!out) return PF_Err_BAD_CALLBACK_PARAM;
      out->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in, output);
    default: return PF_Err_NONE;
  }
}
