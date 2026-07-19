#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
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
  void mark_selector_dispatched() noexcept { selector_dispatched_ = true; }
  bool selector_dispatched() const noexcept { return selector_dispatched_; }

 private:
  Context* previous_{};
  std::vector<TimedLayerDefinition> timed_layers_;
  bool selector_dispatched_{};
};

Context* active_context() noexcept;
bool last_selector_dispatched() noexcept;

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
