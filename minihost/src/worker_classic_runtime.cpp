#include "worker_classic_runtime.hpp"

#include "render_subsystem.h"

#include <algorithm>
#include <cstring>
#include <utility>

namespace aexcompat::worker_runtime::classic {
namespace {

thread_local Context* g_active_context{};
thread_local bool g_last_selector_dispatched{};

bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale) {
  return left_scale != 0 && right_scale != 0 &&
      static_cast<int64_t>(left) * right_scale ==
          static_cast<int64_t>(right) * left_scale;
}

int invoke_render(void* opaque) {
  const auto& request = *static_cast<const Request*>(opaque);
  return request.hooks.render(request.opaque);
}

int invoke_cleanup(void* opaque) {
  const auto& request = *static_cast<const Request*>(opaque);
  return request.hooks.cleanup ? request.hooks.cleanup(request.opaque) : 0;
}

bool dependencies_ready(void* opaque) {
  const auto& request = *static_cast<const Request*>(opaque);
  return request.hooks.dependencies_ready &&
      request.hooks.dependencies_ready(request.opaque);
}

}  // namespace

Context::Context() noexcept : previous_(g_active_context) {
  g_active_context = this;
}

Context::~Context() {
  if (g_active_context == this) g_active_context = previous_;
}

bool Context::add_timed_layer(TimedLayerDefinition layer) {
  if (layer.slot <= 0) return false;
  timed_layers_.push_back(std::move(layer));
  return true;
}

bool Context::copy_timed_layer(int32_t slot, int32_t time, uint32_t time_scale,
                               void* destination,
                               std::size_t destination_size) const {
  if (!destination || destination_size < kParameterDefinitionSize ||
      time_scale == 0) return false;
  const auto found = std::find_if(timed_layers_.begin(), timed_layers_.end(),
      [slot, time, time_scale](const TimedLayerDefinition& layer) {
        return layer.slot == slot &&
            same_rational_time(layer.time, layer.time_scale, time, time_scale);
      });
  if (found == timed_layers_.end()) return false;
  std::memcpy(destination, found->definition.data(), found->definition.size());
  return true;
}

bool Context::has_timed_slot(int32_t slot) const {
  return std::any_of(timed_layers_.begin(), timed_layers_.end(),
      [slot](const TimedLayerDefinition& layer) { return layer.slot == slot; });
}

Context* active_context() noexcept { return g_active_context; }
bool last_selector_dispatched() noexcept { return g_last_selector_dispatched; }

int dispatch(const Request& request) {
  if (!request.hooks.render || !request.hooks.dependencies_ready) return -1;
  Context context;
  g_last_selector_dispatched = false;
  aexcompat::render::RenderContext render_context{
      aexcompat::render::RenderKind::Classic,
      const_cast<Request*>(&request),
      {&invoke_render, &invoke_cleanup, &dependencies_ready},
      request.module_audit_required};
  const int result = aexcompat::render::dispatch(render_context);
  g_last_selector_dispatched = context.selector_dispatched();
  return result;
}

}  // namespace aexcompat::worker_runtime::classic
