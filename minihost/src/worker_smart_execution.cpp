#include "worker_smart_execution.hpp"

#include "render_subsystem.h"

namespace aexcompat::worker_runtime::smart_execution {
namespace {
Hooks g_hooks{};

struct Request {
  EffectEntry entry;
  Input& input;
  Output& output;
  const std::string& case_id;
  const RequestedAssignments* requested;
  const std::vector<unsigned char>* external_rgba;
  const std::filesystem::path* external_output;
  int32_t external_width;
  int32_t external_height;
  const std::vector<ExternalLayerInput>* external_layers;
  int32_t external_current_time;
  int32_t external_time_step;
  int32_t external_total_time;
  uint32_t external_time_scale;
  int32_t external_pixel_bytes;
  Result result;
};

bool dependencies_ready(void* opaque) {
  const auto& request = *static_cast<Request*>(opaque);
  return request.entry && request.external_time_scale != 0 &&
      request.external_time_step > 0 &&
      request.external_total_time >= request.external_current_time;
}

int execute(void* opaque) {
  auto& request = *static_cast<Request*>(opaque);
  request.result = g_hooks.execute(request.entry, request.input, request.output,
      request.case_id, request.requested, request.external_rgba,
      request.external_output, request.external_width, request.external_height,
      request.external_layers, request.external_current_time,
      request.external_time_step, request.external_total_time,
      request.external_time_scale, request.external_pixel_bytes);
  if (request.result.gpu_setup_error != 0) return request.result.gpu_setup_error;
  if (request.result.pre_error != 0) return request.result.pre_error;
  return request.result.render_error;
}

int cleanup(void*) { return 0; }
}  // namespace

bool configure(const Hooks& hooks) noexcept {
  if (!hooks.execute || !hooks.module_audit_required) return false;
  g_hooks = hooks;
  return true;
}

Result render_once(EffectEntry entry, Input& input, Output& output,
                   const std::string& case_id,
                   const RequestedAssignments* requested,
                   const std::vector<unsigned char>* external_rgba,
                   const std::filesystem::path* external_output,
                   int32_t external_width, int32_t external_height,
                   const std::vector<ExternalLayerInput>* external_layers,
                   int32_t external_current_time, int32_t external_time_step,
                   int32_t external_total_time, uint32_t external_time_scale,
                   int32_t external_pixel_bytes) {
  Request request{entry, input, output, case_id, requested, external_rgba,
      external_output, external_width, external_height, external_layers,
      external_current_time, external_time_step, external_total_time,
      external_time_scale, external_pixel_bytes, {}};
  render::RenderContext context{
      render::RenderKind::SmartPreRenderAndRender, &request,
      {&execute, &cleanup, &dependencies_ready},
      g_hooks.module_audit_required()};
  const int dispatch_error = render::dispatch(context);
  if (!context.selector_started && dispatch_error != 0)
    request.result.render_error = dispatch_error;
  return request.result;
}

}  // namespace aexcompat::worker_runtime::smart_execution
