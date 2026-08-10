#include "worker_classic_runtime.hpp"

#include "render_subsystem.h"
#include "worker_extended_diag.hpp"

#include <algorithm>
#include <atomic>
#include <cstring>
#include <mutex>
#include <utility>

namespace aexcompat::worker_runtime::classic {
namespace {

thread_local Context* g_active_context{};
std::atomic_bool g_last_selector_dispatched{};
std::atomic_uint32_t g_dispatch_count{};
std::mutex g_diagnostics_mutex;
Diagnostics g_diagnostics;

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
  const int cleanup_error =
      request.hooks.cleanup ? request.hooks.cleanup(request.opaque) : 0;
  if (auto* context = active_context()) context->automatic_checkin();
  return cleanup_error;
}

bool dependencies_ready(void* opaque) {
  const auto& request = *static_cast<const Request*>(opaque);
  return request.hooks.dependencies_ready &&
      request.hooks.dependencies_ready(request.opaque);
}

}  // namespace

Context::Context() noexcept : previous_(g_active_context) {
  g_dispatch_count.fetch_add(1, std::memory_order_acq_rel);
  g_active_context = this;
}

Context::~Context() {
  {
    std::lock_guard<std::mutex> lock(g_diagnostics_mutex);
    g_diagnostics.wide_time_allowed =
        g_diagnostics.wide_time_allowed || diagnostics_.wide_time_allowed;
    g_diagnostics.shutter_dependency_advertised =
        g_diagnostics.shutter_dependency_advertised ||
        diagnostics_.shutter_dependency_advertised;
    g_diagnostics.rejected_temporal_checkouts += diagnostics_.rejected_temporal_checkouts;
    g_diagnostics.checkout_calls += diagnostics_.checkout_calls;
    g_diagnostics.checkin_calls += diagnostics_.checkin_calls;
    g_diagnostics.automatic_checkins += diagnostics_.automatic_checkins;
    g_diagnostics.invalid_checkins += diagnostics_.invalid_checkins;
    if (diagnostics_.last_index >= 0) {
      g_diagnostics.last_index = diagnostics_.last_index;
      g_diagnostics.last_time = diagnostics_.last_time;
      g_diagnostics.last_time_step = diagnostics_.last_time_step;
      g_diagnostics.last_time_scale = diagnostics_.last_time_scale;
    }
    g_diagnostics.balanced = g_diagnostics.balanced && checkouts_balanced();
  }
  if (g_active_context == this) g_active_context = previous_;
  g_dispatch_count.fetch_sub(1, std::memory_order_acq_rel);
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

void Context::set_definition(int32_t slot,
                             const ParameterDefinition& definition) {
  definitions_[slot] = definition;
}

bool Context::copy_definition(int32_t slot, void* destination,
                              std::size_t destination_size) const {
  if (!destination || destination_size < kParameterDefinitionSize) return false;
  const auto found = definitions_.find(slot);
  if (found == definitions_.end()) return false;
  std::memcpy(destination, found->second.data(), found->second.size());
  return true;
}

void Context::set_fallback_definition(
    int32_t slot, const ParameterDefinition& definition) {
  fallback_definitions_[slot] = definition;
}

bool Context::copy_fallback_definition(
    int32_t slot, void* destination, std::size_t destination_size) const {
  if (!destination || destination_size < kParameterDefinitionSize) return false;
  const auto found = fallback_definitions_.find(slot);
  if (found == fallback_definitions_.end()) return false;
  std::memcpy(destination, found->second.data(), found->second.size());
  return true;
}

void Context::configure_checkout_time(int32_t current_time, uint32_t time_scale,
                                      bool wide_time_allowed,
                                      bool shutter_dependency_advertised) noexcept {
  current_time_ = current_time;
  current_time_scale_ = time_scale;
  wide_time_allowed_ = wide_time_allowed;
  diagnostics_.wide_time_allowed = wide_time_allowed;
  diagnostics_.shutter_dependency_advertised =
      shutter_dependency_advertised;
}

bool Context::checkout_time_allowed(int32_t time, uint32_t time_scale) noexcept {
  (void)time;
  // WIDE_TIME_INPUT is a cache-dependency declaration, not permission for a
  // plug-in to request a parameter value at another valid time.
  return time_scale != 0 && current_time_scale_ != 0;
}

void Context::record_checkout(void* definition, int32_t index, int32_t time,
                              int32_t time_step, uint32_t time_scale) {
  ++live_checkouts_[definition];
  ++diagnostics_.checkout_calls;
  diagnostics_.last_index = index;
  diagnostics_.last_time = time;
  diagnostics_.last_time_step = time_step;
  diagnostics_.last_time_scale = time_scale;
}

int32_t Context::checkin(void* definition) {
  if (!definition || live_checkouts_.empty()) {
    ++diagnostics_.invalid_checkins;
    return 4;
  }
  auto found = live_checkouts_.find(definition);
  if (found == live_checkouts_.end()) found = live_checkouts_.begin();
  if (--found->second == 0) live_checkouts_.erase(found);
  ++diagnostics_.checkin_calls;
  return 0;
}

bool Context::checkouts_balanced() const noexcept {
  return live_checkouts_.empty() &&
      diagnostics_.checkout_calls == diagnostics_.checkin_calls &&
      diagnostics_.invalid_checkins == 0;
}

void Context::automatic_checkin() {
  uint32_t count{};
  for (const auto& checkout : live_checkouts_) count += checkout.second;
  diagnostics_.automatic_checkins += count;
  diagnostics_.checkin_calls += count;
  live_checkouts_.clear();
}

void Context::mark_selector_dispatched() noexcept {
  selector_dispatched_ = true;
  g_last_selector_dispatched.store(true, std::memory_order_relaxed);
}

Context* active_context() noexcept { return g_active_context; }
bool last_selector_dispatched() noexcept {
  return g_last_selector_dispatched.load(std::memory_order_relaxed);
}
HostCallbackTelemetry& host_callback_telemetry() {
  static HostCallbackTelemetry telemetry;
  return telemetry;
}

void reset_selector_diagnostic() noexcept {
  g_last_selector_dispatched.store(false, std::memory_order_relaxed);
  std::lock_guard<std::mutex> lock(g_diagnostics_mutex);
  g_diagnostics = {};
  g_diagnostics.last_index = -1;
  g_diagnostics.balanced = true;
}
bool dispatch_active() noexcept {
  return g_dispatch_count.load(std::memory_order_acquire) != 0;
}
Diagnostics diagnostics() noexcept {
  std::lock_guard<std::mutex> lock(g_diagnostics_mutex);
  return g_diagnostics;
}

int dispatch(const Request& request) {
  if (!request.hooks.render || !request.hooks.dependencies_ready) return -1;
  Context context;
  aexcompat::render::RenderContext render_context{
      aexcompat::render::RenderKind::Classic,
      const_cast<Request*>(&request),
      {&invoke_render, &invoke_cleanup, &dependencies_ready},
      request.module_audit_required};
  return aexcompat::render::dispatch(render_context);
}

}  // namespace aexcompat::worker_runtime::classic

// Classic host progress/abort/quack callbacks moved from worker_main
// (issue #170); their telemetry already lives in host_callback_telemetry().
// These callbacks keep the C linkage worker_l2_suite_abi.hpp froze for the
// legacy callback slots.
namespace aexcompat::l2_detail {
extern "C" {
int32_t __cdecl duck_quack(uint16_t);
int32_t __cdecl abort_render(void*);
int32_t __cdecl report_progress(void*, int32_t, int32_t);
}
namespace {
auto& g_host_callback_telemetry =
    aexcompat::worker_runtime::classic::host_callback_telemetry();
auto& g_duck_quacks = g_host_callback_telemetry.duck_quacks;
auto& g_abort_calls = g_host_callback_telemetry.abort_calls;
auto& g_progress_calls = g_host_callback_telemetry.progress_calls;
auto& g_last_progress_current = g_host_callback_telemetry.last_progress_current;
auto& g_last_progress_total = g_host_callback_telemetry.last_progress_total;
}  // namespace

int32_t __cdecl duck_quack(uint16_t times) {
  if (times > 64) return 4;
  g_duck_quacks += times;
  return 0;
}

int32_t __cdecl abort_render(void* effect_ref) {
  if (extended_diag_enabled())
    std::cerr << "extended_diag:abort_render ref=" << effect_ref << "\n"
              << std::flush;
  if (!effect_ref) return 4;
  ++g_abort_calls;
  return 0;
}

int32_t __cdecl report_progress(void* effect_ref, int32_t current, int32_t total) {
  if (!effect_ref || total <= 0 || current < 0 || current > total) return 4;
  ++g_progress_calls;
  g_last_progress_current = current;
  g_last_progress_total = total;
  return 0;
}

}  // namespace aexcompat::l2_detail
