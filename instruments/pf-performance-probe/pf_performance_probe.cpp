// Public bounded performance shapes. The mode slider changes the work per
// frame; no absolute wall-time expectation is built into the fixture.
#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_Macros.h"
#include "Param_Utils.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <algorithm>
#include <cmath>
#include <cstdint>

namespace {
constexpr int kModeSlot = 1;
constexpr std::size_t kAllocationBytes = 256 * 1024;
constexpr std::size_t kTemporaryBytes = 1024 * 1024;
void* g_retained[16]{};
std::size_t g_retained_count{};

void touch_pages(void* memory, std::size_t bytes) {
  auto* pages = static_cast<volatile unsigned char*>(memory);
  for (std::size_t offset = 0; offset < bytes; offset += 4096)
    pages[offset] = static_cast<unsigned char>(offset / 4096);
}

PF_Err render(PF_ParamDef* params[], PF_LayerDef* output) {
  if (!params || !params[kModeSlot] || !output || !output->data ||
      output->width <= 0 || output->height <= 0)
    return PF_Err_BAD_CALLBACK_PARAM;
  const int mode = static_cast<int>(std::lround(params[kModeSlot]->u.fs_d.value));
  if (mode < 0 || mode > 6) return PF_Err_BAD_CALLBACK_PARAM;
  const std::uint64_t pixels = static_cast<std::uint64_t>(output->width) * output->height;
  volatile std::uint64_t work = 0;
  if (mode == 1) {
    // Fixed work independent of the dimensions, followed by the same pixel
    // loop as mode 0. The sink contributes to a pixel so it cannot disappear.
    for (std::uint64_t i = 0; i < 2'000'000; ++i) work += i & 7;
  }
  if (mode >= 3 && mode <= 6) {
    // Modes 5/6 magnify retained versus recovered commit above allocator
    // noise, while bounding the retained/temporary allocation to 32 MiB.
    const std::size_t bytes = mode == 6 ? 32 * kTemporaryBytes
        : mode == 5 ? 2 * kTemporaryBytes
        : mode == 3 ? kTemporaryBytes : kAllocationBytes;
    void* memory = VirtualAlloc(nullptr, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    if (!memory) return PF_Err_BAD_CALLBACK_PARAM;
    touch_pages(memory, bytes);
    if (mode == 3 || mode == 6) {
      VirtualFree(memory, 0, MEM_RELEASE);
    } else if (g_retained_count < 16) {
      g_retained[g_retained_count++] = memory;
    } else {
      VirtualFree(memory, 0, MEM_RELEASE);
    }
  }
  const std::uint64_t quadratic_inner =
      mode == 2 ? std::max<std::uint64_t>(1, pixels / 5'000) : 0;
  for (A_long y = 0; y < output->height; ++y) {
    auto* row = reinterpret_cast<PF_Pixel*>(
        reinterpret_cast<unsigned char*>(output->data) + y * output->rowbytes);
    for (A_long x = 0; x < output->width; ++x) {
      if (mode == 2)
        for (std::uint64_t repeat = 0; repeat < quadratic_inner; ++repeat)
          work += (static_cast<std::uint64_t>(x) + y + repeat) & 3;
      PF_Pixel pixel{};
      pixel.alpha = 255;
      pixel.red = static_cast<A_u_char>((x + y + work) & 255);
      pixel.green = static_cast<A_u_char>((x ^ y) & 255);
      pixel.blue = static_cast<A_u_char>(work & 255);
      row[x] = pixel;
    }
  }
  return PF_Err_NONE;
}
}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                       PF_OutData* out_data, PF_ParamDef* params[],
                                       PF_LayerDef* output, void*) {
  (void)in_data;
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: {
      PF_ParamDef def{};
      PF_ADD_FLOAT_SLIDERX("Mode", 0, 6, 0, 6, 0, 1,
                           PF_ValueDisplayFlag_NONE, 0, kModeSlot);
      out_data->num_params = 2;
      return PF_Err_NONE;
    }
    case PF_Cmd_RENDER:
      return render(params, output);
    case PF_Cmd_GLOBAL_SETDOWN:
      for (std::size_t i = 0; i < g_retained_count; ++i)
        VirtualFree(g_retained[i], 0, MEM_RELEASE);
      g_retained_count = 0;
      return PF_Err_NONE;
    default:
      return PF_Err_NONE;
  }
}
