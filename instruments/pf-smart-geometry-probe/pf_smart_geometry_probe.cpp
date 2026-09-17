#include "AEConfig.h"
#include "entry.h"
#include "AE_Effect.h"

// Prophylactic, not load-bearing: nothing here trips the function-like max/min
// macros today (this file uses neither) and nothing here uses anything
// WIN32_LEAN_AND_MEAN excludes. They keep the next use from failing to compile
// with an error that points at a standard header rather than at this include.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <array>
#include <cstdint>
#include <cstring>

namespace {

// Depth-advertisement variants, selected by a marker in this module's own file
// name so the same fixture, the same entry point and the same render answer
// every case - only the advertisement differs, which is the variable under
// test. Every other probe in the tree advertises DEEP and FLOAT
// unconditionally, so without these a session's dispatch depth always equals
// its own and the host's narrowing path never runs under test.
//
//   ...-shallow.aex   advertise neither depth at GLOBAL_SETUP.
//   ...-rewrite.aex   advertise both at GLOBAL_SETUP and then ASSIGN them away
//                     in PARAMS_SETUP, the way a plug-in that assigns rather
//                     than ORs does. A host that decides its dispatch depth
//                     from the live out_data instead of from the GLOBAL_SETUP
//                     snapshot follows the rewrite and renders shallower.
//
// The file name and not an environment variable: a test copies the probe to a
// marked name and points the worker at the copy, so the variant travels with
// the plug-in the run actually loaded. An environment variable is ambient -
// every other test that spawns this probe inherits it, and after this change a
// session report describes the slot rather than the plug-in's world, so those
// tests stay green while silently measuring a narrowed run.
const char* ModuleFileName() {
  // One function-local static with an initializer, so the compiler emits the
  // thread-safe guard: a `static bool resolved` tested and set by hand is a
  // data race even when the racing writes are identical.
  //
  // Not MAX_PATH: GetModuleFileNameA truncates silently, and the marker is the
  // last thing before the extension - a deep enough checkout would make it
  // invisible and the variant would quietly become the plain probe.
  static const std::array<char, 4096> path = [] {
    std::array<char, 4096> resolved{};
    HMODULE self = nullptr;
    if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                               GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           reinterpret_cast<LPCSTR>(&ModuleFileName), &self)) {
      GetModuleFileNameA(self, resolved.data(),
                         static_cast<DWORD>(resolved.size()));
    }
    return resolved;
  }();
  return path.data();
}

bool ModuleNameHas(const char* marker) {
  return std::strstr(ModuleFileName(), marker) != nullptr;
}

bool AdvertisesDeepColor() { return !ModuleNameHas("-shallow"); }

bool RewritesFlagsInParamsSetup() { return ModuleNameHas("-rewrite"); }

bool RectEmpty(const PF_LRect& rect) {
  return rect.left >= rect.right || rect.top >= rect.bottom;
}

bool SameRect(const PF_LRect& rect, A_long left, A_long top, A_long right, A_long bottom) {
  return rect.left == left && rect.top == top && rect.right == right && rect.bottom == bottom;
}

PF_Err CheckoutInput(PF_InData* in_data, PF_PreRenderExtra* extra,
                     const PF_LRect& request_rect, PF_CheckoutResult* result) {
  PF_RenderRequest request = extra->input->output_request;
  request.rect = request_rect;
  return extra->cb->checkout_layer(in_data->effect_ref, 0, 0, &request,
                                   in_data->current_time, in_data->time_step,
                                   in_data->time_scale, result);
}

// Probe modes are selected through the generic render time the host already
// supplies (current_time modulo 5), so no host-side probe-specific branch is
// needed to reach any scenario.
enum class Mode : A_long {
  VerifyIntersection = 0,
  ExtraPixels = 1,
  FlaglessOverrun = 2,
  EmptyResult = 3,
  LargeEnvelope = 4,
};

Mode ProbeMode(const PF_InData* in_data) {
  return static_cast<Mode>(((in_data->current_time % 5) + 5) % 5);
}

PF_Err SmartPreRender(PF_InData* in_data, PF_PreRenderExtra* extra) {
  if (!in_data || !extra || !extra->input || !extra->output || !extra->cb)
    return PF_Err_BAD_CALLBACK_PARAM;
  const A_long width = in_data->width;
  const A_long height = in_data->height;
  if (width < 8 || height < 4) return PF_Err_BAD_CALLBACK_PARAM;
  const PF_LRect full{0, 0, width, height};
  PF_CheckoutResult checkout{};
  switch (ProbeMode(in_data)) {
    case Mode::VerifyIntersection: {
      // A full-frame request answers full availability.
      PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      if (!SameRect(checkout.result_rect, 0, 0, width, height) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // A sub-rect request is answered with exactly the intersection while
      // max_result_rect stays the full layer extent.
      const PF_LRect sub{2, 1, width - 2, height - 1};
      err = CheckoutInput(in_data, extra, sub, &checkout);
      if (err) return err;
      if (!SameRect(checkout.result_rect, sub.left, sub.top, sub.right, sub.bottom) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // An oversized request clamps to the layer extent; max_result_rect must
      // not vary with the request.
      const PF_LRect oversized{-10, -10, width + 10, height + 10};
      err = CheckoutInput(in_data, extra, oversized, &checkout);
      if (err) return err;
      if (!SameRect(checkout.result_rect, 0, 0, width, height) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // A disjoint request is answered with a legally empty rect while
      // max_result_rect still reports the full layer extent.
      const PF_LRect disjoint{width + 5, height + 5, width + 6, height + 6};
      err = CheckoutInput(in_data, extra, disjoint, &checkout);
      if (err) return err;
      if (!RectEmpty(checkout.result_rect) ||
          !SameRect(checkout.max_result_rect, 0, 0, width, height))
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
      // The final full checkout is the one that backs the render.
      err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      extra->output->result_rect = full;
      extra->output->max_result_rect = full;
      return PF_Err_NONE;
    }
    case Mode::EmptyResult: {
      const PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      extra->output->result_rect = PF_LRect{0, 0, 0, 0};
      extra->output->max_result_rect = full;
      return PF_Err_NONE;
    }
    case Mode::LargeEnvelope: {
      const PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      extra->output->result_rect = full;
      extra->output->max_result_rect = PF_LRect{-5000, -5000, 5000, 5000};
      return PF_Err_NONE;
    }
    case Mode::ExtraPixels:
    case Mode::FlaglessOverrun: {
      const PF_Err err = CheckoutInput(in_data, extra, full, &checkout);
      if (err) return err;
      const PF_LRect expanded{-2, -2, width + 2, height + 2};
      extra->output->result_rect = expanded;
      extra->output->max_result_rect = expanded;
      if (ProbeMode(in_data) == Mode::ExtraPixels)
        extra->output->flags = PF_RenderOutputFlag_RETURNS_EXTRA_PIXELS;
      return PF_Err_NONE;
    }
  }
  return PF_Err_BAD_CALLBACK_PARAM;
}

template <typename Pixel, typename Channel>
void FillWorld(PF_EffectWorld* world, Channel opaque) {
  for (A_long y = 0; y < world->height; ++y) {
    auto* row = reinterpret_cast<Pixel*>(
        reinterpret_cast<A_u_char*>(world->data) + y * world->rowbytes);
    for (A_long x = 0; x < world->width; ++x) {
      auto* channels = reinterpret_cast<Channel*>(&row[x]);
      channels[0] = opaque;
      channels[1] = opaque;
      channels[2] = Channel(0);
      channels[3] = opaque;
    }
  }
}

PF_Err SmartRender(PF_InData* in_data, PF_SmartRenderExtra* extra) {
  if (!in_data || !extra || !extra->cb) return PF_Err_BAD_CALLBACK_PARAM;
  // The empty-result pre-render promised nothing, so the host must never
  // invoke the render selector for it.
  if (ProbeMode(in_data) == Mode::EmptyResult) return PF_Err_INTERNAL_STRUCT_DAMAGED;
  PF_EffectWorld* input = nullptr;
  PF_EffectWorld* output = nullptr;
  PF_Err err = extra->cb->checkout_layer_pixels(in_data->effect_ref, 0, &input);
  if (!err) err = extra->cb->checkout_output(in_data->effect_ref, &output);
  if (err) return err;
  if (!input || !input->data || !output || !output->data) return PF_Err_BAD_CALLBACK_PARAM;
  if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_PixelFloat)))
    FillWorld<PF_PixelFloat, PF_FpShort>(output, 1.0f);
  else if (output->rowbytes >= output->width * static_cast<A_long>(sizeof(PF_Pixel16)))
    FillWorld<PF_Pixel16, A_u_short>(output, PF_MAX_CHAN16);
  else
    FillWorld<PF_Pixel8, A_u_char>(output, PF_MAX_CHAN8);
  return extra->cb->checkin_layer_pixels(in_data->effect_ref, 0);
}

}  // namespace

extern "C" DllExport PF_Err EffectMain(PF_Cmd cmd, PF_InData* in_data,
                                        PF_OutData* out_data, PF_ParamDef*[],
                                        PF_LayerDef*, void* extra) {
  switch (cmd) {
    case PF_Cmd_GLOBAL_SETUP:
      out_data->my_version = PF_VERSION(1, 0, 0, PF_Stage_DEVELOP, 0);
      out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT |
          (AdvertisesDeepColor() ? PF_OutFlag_DEEP_COLOR_AWARE : 0);
      out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER |
          (AdvertisesDeepColor() ? PF_OutFlag2_FLOAT_COLOR_AWARE : 0);
      return PF_Err_NONE;
    case PF_Cmd_PARAMS_SETUP:
      out_data->num_params = 1;
      if (RewritesFlagsInParamsSetup()) {
        out_data->out_flags = PF_OutFlag_PIX_INDEPENDENT;
        out_data->out_flags2 = PF_OutFlag2_SUPPORTS_SMART_RENDER;
      }
      return PF_Err_NONE;
    case PF_Cmd_SMART_PRE_RENDER:
      return SmartPreRender(in_data, static_cast<PF_PreRenderExtra*>(extra));
    case PF_Cmd_SMART_RENDER:
      return SmartRender(in_data, static_cast<PF_SmartRenderExtra*>(extra));
    default:
      return PF_Err_NONE;
  }
}
