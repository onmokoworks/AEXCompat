#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <map>
#include <unordered_map>
#include <vector>

namespace aexcompat::worker_runtime::classic {

constexpr std::size_t kParameterDefinitionSize = 176;
using ParameterDefinition = std::array<std::byte, kParameterDefinitionSize>;

struct TimedLayerDefinition {
  int32_t slot{};
  int32_t time{};
  uint32_t time_scale{};
  ParameterDefinition definition{};
};

struct Diagnostics {
  bool wide_time_allowed{};
  bool shutter_dependency_advertised{};
  uint32_t rejected_temporal_checkouts{};
  uint32_t checkout_calls{};
  uint32_t checkin_calls{};
  uint32_t automatic_checkins{};
  uint32_t invalid_checkins{};
  int32_t last_index{-1};
  int32_t last_time{};
  int32_t last_time_step{};
  uint32_t last_time_scale{};
  bool balanced{true};
};

class Context final {
 public:
  Context() noexcept;
  ~Context();

  Context(const Context&) = delete;
  Context& operator=(const Context&) = delete;

  bool add_timed_layer(TimedLayerDefinition layer);
  bool copy_timed_layer(int32_t slot, int32_t time, uint32_t time_scale,
                        void* destination, std::size_t destination_size) const;
  bool has_timed_slot(int32_t slot) const;
  void set_definition(int32_t slot, const ParameterDefinition& definition);
  bool copy_definition(int32_t slot, void* destination,
                       std::size_t destination_size) const;
  // True when a definition table has been published (slots 0..N) and `slot`
  // lies past its last entry. Negative slots and gaps inside the table are
  // not "beyond": those stay refusals in checkout_param.
  bool beyond_definition_table(int32_t slot) const;
  void set_fallback_definition(int32_t slot,
                               const ParameterDefinition& definition);
  bool copy_fallback_definition(int32_t slot, void* destination,
                                std::size_t destination_size) const;
  void configure_checkout_time(int32_t current_time, uint32_t time_scale,
                               bool wide_time_allowed,
                               bool shutter_dependency_advertised) noexcept;
  bool checkout_time_allowed(int32_t time, uint32_t time_scale) noexcept;
  bool shutter_dependency_advertised() const noexcept {
    return diagnostics_.shutter_dependency_advertised;
  }
  void record_checkout(void* definition, int32_t index, int32_t time,
                       int32_t time_step, uint32_t time_scale);
  int32_t checkin(void* definition);
  bool checkouts_balanced() const noexcept;
  void automatic_checkin();
  void mark_selector_dispatched() noexcept;
  bool selector_dispatched() const noexcept { return selector_dispatched_; }

 private:
  Context* previous_{};
  std::vector<TimedLayerDefinition> timed_layers_;
  std::map<int32_t, ParameterDefinition> definitions_;
  std::map<int32_t, ParameterDefinition> fallback_definitions_;
  std::unordered_map<void*, uint32_t> live_checkouts_;
  Diagnostics diagnostics_{};
  int32_t current_time_{};
  uint32_t current_time_scale_{1};
  bool wide_time_allowed_{};
  bool selector_dispatched_{};
};

Context* active_context() noexcept;
bool last_selector_dispatched() noexcept;
void reset_selector_diagnostic() noexcept;

// Classic host-callback telemetry (issue #126 Phase D): counters recorded by
// worker_main's PF utility and world-transform callbacks (quack,
// transform_world, abort, progress) plus the classic fallback layer slot.
// Those callbacks and the world-transform pointer bundle write it; the
// classic completion report reads it back. Lifetime: process-lifetime, never
// torn down.
struct HostCallbackTelemetry {
  uint32_t duck_quacks{};
  uint32_t transform_world_calls{};
  int32_t last_transform_x{};
  int32_t last_transform_y{};
  uint8_t last_transform_opacity{};
  uint32_t abort_calls{};
  uint32_t progress_calls{};
  int32_t last_progress_current{};
  int32_t last_progress_total{};
  int32_t secondary_layer_slot{6};
};
HostCallbackTelemetry& host_callback_telemetry();
bool dispatch_active() noexcept;
Diagnostics diagnostics() noexcept;

struct Hooks {
  int (*render)(void*){};
  int (*cleanup)(void*){};
  bool (*dependencies_ready)(void*){};
};

struct Request {
  void* opaque{};
  Hooks hooks{};
  bool module_audit_required{};
};

// Owns the complete classic-render dispatch lifetime. The active context is
// thread-local so PF checkout callbacks see only the current render's timed
// layers and nested renders restore the outer context on unwind.
int dispatch(const Request& request);

}  // namespace aexcompat::worker_runtime::classic
