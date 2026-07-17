#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectGPUSuites.h"

#include <cstring>

namespace {
PF_Err exercise_gpu_device_suite(PF_InData* in_data) {
  const PF_GPUDeviceSuite1* gpu = nullptr;
  PF_Err error = in_data->pica_basicP->AcquireSuite(
      kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1,
      reinterpret_cast<const void**>(&gpu));
  if (error != PF_Err_NONE || !gpu) return error ? error : PF_Err_BAD_CALLBACK_PARAM;

  A_u_long count = 0;
  PF_GPUDeviceInfo info{};
  void* device_memory = nullptr;
  void* host_memory = nullptr;
  size_t purged = 1;
  error = gpu->GetDeviceCount(in_data->effect_ref, &count);
  if (!error && count != 1) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = gpu->GetDeviceInfo(in_data->effect_ref, 0, &info);
  if (!error) error = gpu->AcquireExclusiveDeviceAccess(in_data->effect_ref, 0);
  if (!error) error = gpu->AllocateDeviceMemory(in_data->effect_ref, 0, 64, &device_memory);
  if (!error) error = gpu->AllocateHostMemory(in_data->effect_ref, 0, 128, &host_memory);
  if (!error) error = gpu->PurgeDeviceMemory(in_data->effect_ref, 0, 64, &purged);
  if (!error && purged != 0) error = PF_Err_BAD_CALLBACK_PARAM;
  if (!error) error = gpu->PurgeHostMemory(in_data->effect_ref, 0, 128, &purged);
  if (!error && purged != 0) error = PF_Err_BAD_CALLBACK_PARAM;
  if (host_memory) {
    const PF_Err free_error = gpu->FreeHostMemory(in_data->effect_ref, 0, host_memory);
    if (!error) error = free_error;
  }
  if (device_memory) {
    const PF_Err free_error = gpu->FreeDeviceMemory(in_data->effect_ref, 0, device_memory);
    if (!error) error = free_error;
    if (!error && gpu->FreeDeviceMemory(in_data->effect_ref, 0, device_memory) == PF_Err_NONE) {
      error = PF_Err_BAD_CALLBACK_PARAM;
    }
  }
  const PF_Err release_error = gpu->ReleaseExclusiveDeviceAccess(in_data->effect_ref, 0);
  if (!error) error = release_error;
  const PF_Err suite_error = in_data->pica_basicP->ReleaseSuite(
      kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
  return error ? error : suite_error;
}

PF_Err render(PF_ParamDef* params[], PF_LayerDef* output) {
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
      return exercise_gpu_device_suite(in_data);
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_RENDER:
      return render(params, output);
    default:
      return PF_Err_NONE;
  }
}
