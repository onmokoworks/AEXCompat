#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"
#include "AE_EffectCBSuites.h"
#include <array>
#include <cstddef>
#include <cstring>
#include <type_traits>

namespace {
static_assert(std::is_standard_layout_v<PF_WorldTransformSuite1>);
static_assert(sizeof(PF_WorldTransformSuite1) == 7 * sizeof(void*));
static_assert(offsetof(PF_WorldTransformSuite1, convolve) == 2 * sizeof(void*));

template <typename T> void keep_first(PF_Err& first, T candidate) {
  if (!first && candidate) first = static_cast<PF_Err>(candidate);
}
constexpr std::array<A_u_char, 9> kSource{{10,20,30,40,50,60,70,80,90}};
constexpr std::array<A_u_char, 9> kBlur{{13,23,18,30,50,37,27,43,31}};
constexpr std::array<A_u_char, 9> kSharpen{{0,10,70,70,50,130,230,190,255}};

void seed(PF_EffectWorld& world) {
  for (A_long y = 0; y < 3; ++y) {
    auto* row = reinterpret_cast<PF_Pixel8*>(reinterpret_cast<A_u_char*>(world.data) +
                                             static_cast<size_t>(y) * world.rowbytes);
    for (A_long x = 0; x < 3; ++x) {
      const auto value = kSource[static_cast<size_t>(y * 3 + x)];
      row[x] = {value, value, value, value};
    }
  }
}
bool matches(const PF_EffectWorld& world, const std::array<A_u_char, 9>& expected) {
  for (A_long y = 0; y < 3; ++y) {
    const auto* row = reinterpret_cast<const PF_Pixel8*>(
        reinterpret_cast<const A_u_char*>(world.data) + static_cast<size_t>(y) * world.rowbytes);
    for (A_long x = 0; x < 3; ++x) {
      const auto value = expected[static_cast<size_t>(y * 3 + x)];
      const PF_Pixel8 pixel{value, value, value, value};
      if (std::memcmp(&row[x], &pixel, sizeof(pixel))) return false;
    }
  }
  return true;
}
PF_Err apply(const PF_WorldTransformSuite1* suite, PF_ProgPtr effect_ref,
             PF_EffectWorld* source, PF_EffectWorld* destination,
             std::array<A_long, 9>& kernel, PF_KernelFlags flags) {
  return suite->convolve(effect_ref, source, nullptr, flags, 3, kernel.data(), kernel.data(),
                         kernel.data(), kernel.data(), destination);
}

PF_Err render(PF_InData* in_data, PF_LayerDef* output) {
  if (!in_data || !in_data->pica_basicP || !in_data->effect_ref || !output ||
      !output->data || output->width <= 0 || output->height <= 0 || output->rowbytes <= 0)
    return PF_Err_BAD_CALLBACK_PARAM;
  const PF_WorldTransformSuite1* transforms = nullptr;
  const PF_WorldSuite2* worlds = nullptr;
  PF_Err err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1,
      reinterpret_cast<const void**>(&transforms)));
  if (!err) err = static_cast<PF_Err>(in_data->pica_basicP->AcquireSuite(
      kPFWorldSuite, kPFWorldSuiteVersion2, reinterpret_cast<const void**>(&worlds)));
  if (!err && (!transforms || !transforms->convolve || !worlds || !worlds->PF_NewWorld ||
      !worlds->PF_DisposeWorld || !worlds->PF_GetPixelFormat)) err = PF_Err_INVALID_CALLBACK;
  PF_PixelFormat format = PF_PixelFormat_INVALID;
  if (!err) err = worlds->PF_GetPixelFormat(output, &format);
  if (!err && format != PF_PixelFormat_ARGB32) err = PF_Err_BAD_CALLBACK_PARAM;

  PF_EffectWorld source{}, destination{};
  bool source_created = false, destination_created = false;
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 3, TRUE,
      PF_PixelFormat_ARGB32, &source); source_created = !err; }
  if (!err) { err = worlds->PF_NewWorld(in_data->effect_ref, 3, 3, TRUE,
      PF_PixelFormat_ARGB32, &destination); destination_created = !err; }

  std::array<A_long, 9> identity{}; identity[4] = 2295;
  constexpr PF_KernelFlags unnormalized = PF_KernelFlag_2D | PF_KernelFlag_CLAMP |
      PF_KernelFlag_USE_LONG | PF_KernelFlag_TRANSPARENT_BORDERS |
      PF_KernelFlag_STRAIGHT_CONVOLVE;
  constexpr PF_KernelFlags normalized = unnormalized | PF_KernelFlag_NORMALIZED;
  if (!err) { seed(source); err = apply(transforms, in_data->effect_ref, &source,
                                        &destination, identity, unnormalized); }
  if (!err && !matches(destination, kSource)) err = PF_Err_BAD_CALLBACK_PARAM;
  std::array<A_long, 9> sharpen{{0,-255,0,-255,1275,-255,0,-255,0}};
  if (!err) { seed(source); err = apply(transforms, in_data->effect_ref, &source,
                                        &destination, sharpen, normalized); }
  if (!err && !matches(destination, kSharpen)) err = PF_Err_BAD_CALLBACK_PARAM;
  std::array<A_long, 9> blur{}; blur.fill(255);
  if (!err) { seed(source); err = apply(transforms, in_data->effect_ref, &source,
                                        &destination, blur, normalized); }
  if (!err && !matches(destination, kBlur)) err = PF_Err_BAD_CALLBACK_PARAM;
  if (!err) { seed(source); err = apply(transforms, in_data->effect_ref, &source,
                                        &source, blur, normalized); }
  if (!err && !matches(source, kBlur)) err = PF_Err_BAD_CALLBACK_PARAM;

  if (destination_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &destination));
  if (source_created) keep_first(err, worlds->PF_DisposeWorld(in_data->effect_ref, &source));
  if (worlds) keep_first(err, in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2));
  if (transforms) keep_first(err, in_data->pica_basicP->ReleaseSuite(kPFWorldTransformSuite, kPFWorldTransformSuiteVersion1));
  if (!err) std::memset(output->data, 0, static_cast<size_t>(output->rowbytes) * output->height);
  return err;
}
}
extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
    PF_OutData* out_data, PF_ParamDef*[], PF_LayerDef* output, void*) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP: out_data->my_version = PF_VERSION(1,0,0,PF_Stage_DEVELOP,0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT; return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP: out_data->num_params = 1; return PF_Err_NONE;
    case PF_Cmd_RENDER: return render(in_data, output);
    default: return PF_Err_NONE;
  }
}
