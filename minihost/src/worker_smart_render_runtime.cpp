#include "worker_smart_render_runtime.hpp"

namespace aexcompat::worker_runtime::smart_render_runtime {
namespace {
constexpr int32_t kFrameSetup = 10;
}

bool execute(const Request& request, const Hooks& hooks,
             smart_execution::Result& result) {
  if (!request.entry || !request.input || !request.output || !request.plan ||
      !request.parameters || !request.input_world || !request.output_world ||
      !request.formats || !request.source || !request.guarded ||
      !request.destination || !request.lifecycle ||
      !hooks.dispatch_render_draw || !hooks.finalize.close_ui ||
      !hooks.finalize.end_lifecycle || !hooks.finalize.ui_active)
    return false;

  auto& definitions = request.parameters->definitions;
  auto& params = request.parameters->params;
  if (request.plan->crash_null_output)
    request.entry(kFrameSetup, request.input->data(), request.output->data(),
                  params.data(), nullptr, nullptr);

  if (!hooks.dispatch_render_draw(request.entry, *request.input, *request.output,
                                  definitions)) {
    result.pre_error = -5;
    if (hooks.finalize.ui_active())
      hooks.finalize.close_ui(request.entry, *request.input, *request.output,
                              definitions);
    result.render_error = hooks.finalize.end_lifecycle(
        request.entry, *request.input, *request.output, params.data(),
        request.output_world->data(), *request.lifecycle, -5);
    return true;
  }

  smart_dispatch::State dispatch_state;
  if (!smart_dispatch::dispatch(
          {request.entry, request.input, request.output, request.plan,
           request.parameters, request.input_world, request.output_world,
           request.formats, request.guarded, request.destination,
           request.dispatch_pixel_format},
          hooks.dispatch, result, dispatch_state))
    return false;

  return smart_finalize::finalize(
      {request.entry, request.input, request.output, request.parameters,
       request.output_world, request.lifecycle, request.source, request.guarded,
       *request.destination, request.external_output, request.width,
       request.height, request.rowbytes, request.pixel_bytes,
       &dispatch_state.pre_output},
      hooks.finalize, result);
}

}  // namespace aexcompat::worker_runtime::smart_render_runtime
