#pragma once

#include <cstdint>
#include <ostream>

namespace aexcompat::worker_render_report {

struct TemporalCheckoutCounters {
  uint64_t layer{};
  uint64_t parameter{};
};

struct LayerTemporalRefusals {
  uint64_t count{};
};
struct ParameterTemporalRefusals {
  uint64_t count{};
};

constexpr TemporalCheckoutCounters make_temporal_checkout_counters(
    LayerTemporalRefusals layer, ParameterTemporalRefusals parameter) {
  return {layer.count, parameter.count};
}

// Keep rejected_temporal_param_checkouts byte-for-byte compatible with frozen
// reports: despite its historical name, it has always counted layer checkout
// refusals. The two explicit fields let new consumers distinguish the ledgers
// without silently changing the legacy field's meaning (issue #844).
inline void append_temporal_checkout_counters(
    std::ostream& output, const TemporalCheckoutCounters& counters) {
  output << ",\"rejected_temporal_param_checkouts\":" << counters.layer
         << ",\"rejected_temporal_layer_checkouts\":" << counters.layer
         << ",\"rejected_temporal_parameter_checkouts\":" << counters.parameter;
}

}  // namespace aexcompat::worker_render_report
