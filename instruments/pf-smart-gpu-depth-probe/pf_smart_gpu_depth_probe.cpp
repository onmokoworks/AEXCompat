// GPU-only float admission fixture. CUDA transfers exercise real GPU worlds;
// the known spatial pattern is computed on the host, not a CUDA shader test.
#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include "AE_EffectGPUSuites.h"
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <vector>

namespace {
bool mode(const char* name) {
  const char* value = std::getenv("AEXCOMPAT_GPU_DEPTH_PROBE");
  return value && std::strcmp(value, name) == 0;
}
bool dual8_mode() {
  return mode("dual8-capable") || mode("dual8-no-cap") ||
      mode("dual8-pre-cpu") || mode("dual8-setup-fail") ||
      mode("dual8-cleanup-error") || mode("dual8-pre-error") ||
      mode("dual8-invalid-rect") || mode("dual8-gpu-render-error") ||
      mode("dual8-gpu-setdown-error") || mode("dual8-frame-setdown-error") ||
      mode("dual8-pre-unbalanced") || mode("dual8-dynamic-toggle") ||
      mode("dual8-cpu14") || mode("dual8-frame-setup14");
}

unsigned cpu_calls{}, gpu_calls{}, gpu_setup_calls{}, pre_cleanup_calls{},
    frame_setup_calls{};
int pre_render_token{};

void cleanup_pre_render_data(void*) {
  ++pre_cleanup_calls;
  if (mode("dual8-cleanup-error"))
    RaiseException(0xE0424242u, EXCEPTION_NONCONTINUABLE, 0, nullptr);
}

PF_Err dual8_cpu_render(PF_InData* in, PF_SmartRenderExtra* extra) {
  if (extra->input->bitdepth != 8) return PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld* output = nullptr;
  const auto err = extra->cb->checkout_output(in->effect_ref, &output);
  if (err || !output || output->width <= 0 || output->height <= 0 ||
      output->width > 4096 || output->height > 4096 || !output->data ||
      output->rowbytes < output->width * 4)
    return err ? err : PF_Err_BAD_CALLBACK_PARAM;
  // Red proves that an explicit/no-capability CPU route never entered GPU
  // setup. Blue proves that Auto first attempted GPU setup and then cleanly
  // restarted at the caller's requested 8-bit CPU depth.
  const bool completed_gpu_attempt = gpu_setup_calls != 0 && pre_cleanup_calls != 0;
  if (mode("dual8-pre-cpu") && !completed_gpu_attempt)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_Pixel8 marker = completed_gpu_attempt ? PF_Pixel8{255, 19, 43, 227}
                                                  : PF_Pixel8{255, 231, 17, 29};
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel8*>(
        reinterpret_cast<unsigned char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) row[x] = marker;
  }
  return PF_Err_NONE;
}

PF_Err cpu_render(PF_InData* in, PF_SmartRenderExtra* extra) {
  const int depth = extra->input->bitdepth;
  if ((depth != 8 || !mode("cpu8")) && (depth != 16 || !mode("cpu16")) &&
      (depth != 32 || !mode("cpu32"))) return PF_Err_BAD_CALLBACK_PARAM;
  PF_EffectWorld* output = nullptr;
  const auto err = extra->cb->checkout_output(in->effect_ref, &output);
  const int bytes = depth == 32 ? 16 : (depth == 16 ? 8 : 4);
  if (err || !output || output->width <= 0 || output->height <= 0 ||
      output->width > 4096 || output->height > 4096 || !output->data ||
      output->rowbytes < output->width * bytes)
    return err ? err : PF_Err_BAD_CALLBACK_PARAM;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<unsigned char*>(output->data) + y * output->rowbytes;
    for (A_long x = 0; x < output->width; ++x) {
      const float r = static_cast<float>(x) / output->width;
      const float g = static_cast<float>(y) / output->height;
      if (depth == 32) {
        reinterpret_cast<PF_PixelFloat*>(row)[x] = {1.f, r, g, .25f};
      } else if (depth == 16) {
        reinterpret_cast<PF_Pixel16*>(row)[x] = {32768,
          static_cast<A_u_short>(r * 32768), static_cast<A_u_short>(g * 32768), 8192};
      } else {
        reinterpret_cast<PF_Pixel8*>(row)[x] = {255,
          static_cast<A_u_char>(r * 255), static_cast<A_u_char>(g * 255), 63};
      }
    }
  }
  return PF_Err_NONE;
}

PF_Err gpu_render(PF_InData* in, PF_SmartRenderExtra* extra) {
  ++gpu_calls;
  PF_EffectWorld* output = nullptr;
  PF_Err err = extra->cb->checkout_output(in->effect_ref, &output);
  if (err || !output || output->width <= 0 || output->height <= 0 ||
      output->width > 4096 || output->height > 4096 ||
      output->rowbytes < output->width * 16)
    return err ? err : PF_Err_BAD_CALLBACK_PARAM;
  const PF_GPUDeviceSuite1* suite = nullptr;
  err = in->pica_basicP->AcquireSuite(kPFGPUDeviceSuite,
      kPFGPUDeviceSuiteVersion1, reinterpret_cast<const void**>(&suite));
  if (err || !suite) return err ? err : PF_Err_BAD_CALLBACK_PARAM;
  void* device = nullptr;
  err = suite->GetGPUWorldData(in->effect_ref, output, &device);
  using Upload = int (WINAPI*)(uint64_t, const void*, size_t);
  auto upload = reinterpret_cast<Upload>(GetProcAddress(
      GetModuleHandleW(L"nvcuda.dll"), "cuMemcpyHtoD_v2"));
  if (!err && (!upload || !device)) err = PF_Err_BAD_CALLBACK_PARAM;
  // BGRA128 GPU world: B=.25, G=y/height, R=x/width, A=1.
  std::vector<float> row(static_cast<size_t>(output->width) * 4);
  for (A_long y = 0; !err && y < output->height; ++y) {
    for (A_long x = 0; x < output->width; ++x) {
      row[x * 4] = .25f;
      row[x * 4 + 1] = static_cast<float>(y) / output->height;
      row[x * 4 + 2] = static_cast<float>(x) / output->width;
      row[x * 4 + 3] = 1.f;
    }
    if (upload(reinterpret_cast<uint64_t>(device) +
                   static_cast<uint64_t>(y) * output->rowbytes,
               row.data(), row.size() * sizeof(float)) != 0)
      err = PF_Err_BAD_CALLBACK_PARAM;
  }
  const auto release = in->pica_basicP->ReleaseSuite(
      kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
  return err ? err : release;
}
}

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in,
    PF_OutData* out, PF_ParamDef*[], PF_LayerDef*, void* opaque) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      cpu_calls = gpu_calls = gpu_setup_calls = pre_cleanup_calls =
          frame_setup_calls = 0;
      out->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      out->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
                       PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;
      if (mode("dual8-dynamic-toggle"))
        out->out_flags2 |= PF_OutFlag2_SUPPORTS_QUERY_DYNAMIC_FLAGS;
      if (mode("cpu8") || mode("cpu16") || mode("cpu32") ||
          mode("dual8-no-cap")) {
        out->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER;
        if (mode("cpu16")) out->out_flags |= PF_OutFlag_DEEP_COLOR_AWARE;
        if (mode("cpu32")) out->out_flags2 |= PF_OutFlag2_FLOAT_COLOR_AWARE;
      }
      return PF_Err_NONE;
    case PF_Cmd_QUERY_DYNAMIC_FLAGS:
      if (mode("dual8-dynamic-toggle"))
        out->out_flags2 &= ~PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out->num_params = 1;
      return PF_Err_NONE;
    case PF_Cmd_FRAME_SETUP:
      if (mode("dual8-frame-setup14")) {
        ++frame_setup_calls;
        std::snprintf(out->return_msg, sizeof(out->return_msg),
                      "frame_setup_calls=%u", frame_setup_calls);
        return 14;
      }
      return PF_Err_NONE;
    case PF_Cmd_GPU_DEVICE_SETUP: {
      ++gpu_setup_calls;
      auto* extra = static_cast<PF_GPUDeviceSetupExtra*>(opaque);
      if (!extra || !extra->input || mode("setup-fail") ||
          mode("dual8-setup-fail") ||
          extra->input->what_gpu != PF_GPU_Framework_CUDA)
        return PF_Err_BAD_CALLBACK_PARAM;
      if (mode("setup-drop-gpu"))
        out->out_flags2 &= ~PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;
      return PF_Err_NONE;
    }
    case PF_Cmd_SMART_PRE_RENDER: {
      auto* extra = static_cast<PF_PreRenderExtra*>(opaque);
      extra->output->result_rect = extra->input->output_request.rect;
      extra->output->max_result_rect = extra->input->output_request.rect;
      if (dual8_mode()) {
        extra->output->pre_render_data = &pre_render_token;
        extra->output->delete_pre_render_data_func = cleanup_pre_render_data;
      }
      if (mode("dual8-pre-error")) return PF_Err_BAD_CALLBACK_PARAM;
      if (mode("dual8-invalid-rect")) ++extra->output->result_rect.right;
      if (mode("dual8-pre-unbalanced")) {
        PF_ParamDef foreign{};
        in->inter.checkin_param(in->effect_ref, &foreign);
      }
      if (!mode("pre-cpu") && !mode("dual8-pre-cpu") &&
          !mode("dual8-pre-unbalanced") &&
          !mode("setup-drop-gpu") && !mode("dual8-no-cap") &&
          !mode("cpu8") && !mode("cpu16") && !mode("cpu32"))
        extra->output->flags |= PF_RenderOutputFlag_GPU_RENDER_POSSIBLE;
      return PF_Err_NONE;
    }
    case PF_Cmd_SMART_RENDER_GPU:
      if (mode("dual8-gpu-render-error")) {
        ++gpu_calls;
        return PF_Err_BAD_CALLBACK_PARAM;
      }
      return gpu_render(in, static_cast<PF_SmartRenderExtra*>(opaque));
    case PF_Cmd_SMART_RENDER:
      if (mode("dual8-cpu14")) {
        ++cpu_calls;
        return 14;
      }
      if (dual8_mode()) {
        ++cpu_calls;
        return dual8_cpu_render(in, static_cast<PF_SmartRenderExtra*>(opaque));
      }
      if (mode("cpu8") || mode("cpu16") || mode("cpu32")) {
        ++cpu_calls;
        return cpu_render(in, static_cast<PF_SmartRenderExtra*>(opaque));
      }
      [[fallthrough]];
    case PF_Cmd_RENDER:
      ++cpu_calls;
      std::strcpy(out->return_msg, "GPU-only probe unexpectedly reached CPU");
      return PF_Err_BAD_CALLBACK_PARAM;
    case PF_Cmd_GPU_DEVICE_SETDOWN:
      return mode("dual8-gpu-setdown-error") ? PF_Err_BAD_CALLBACK_PARAM
                                              : PF_Err_NONE;
    case PF_Cmd_FRAME_SETDOWN:
      return mode("dual8-frame-setdown-error") ? PF_Err_BAD_CALLBACK_PARAM
                                                : PF_Err_NONE;
    case PF_Cmd_GLOBAL_SETDOWN:
      std::fprintf(stderr,
                   "gpu_depth_probe cpu_calls=%u gpu_calls=%u gpu_setup_calls=%u "
                   "pre_cleanup_calls=%u frame_setup_calls=%u\n",
                   cpu_calls, gpu_calls, gpu_setup_calls, pre_cleanup_calls,
                   frame_setup_calls);
      std::fflush(stderr);
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
