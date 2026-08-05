#include "worker_render_report.hpp"

#include <iostream>
#include <sstream>
#include <string>

int main() {
  aexcompat::worker_render_report::SmartReport::Head head{};
  head.temporal_checkout_counters =
      aexcompat::worker_render_report::make_temporal_checkout_counters(
          aexcompat::worker_render_report::LayerTemporalRefusals{17},
          aexcompat::worker_render_report::ParameterTemporalRefusals{23});
  std::ostringstream output;
  aexcompat::worker_render_report::append_temporal_checkout_counters(
      output, head.temporal_checkout_counters);
  const std::string json = "{" + output.str().substr(1) + "}";
  const bool passed =
      json == "{\"rejected_temporal_param_checkouts\":17,"
              "\"rejected_temporal_layer_checkouts\":17,"
              "\"rejected_temporal_parameter_checkouts\":23}";
  std::cout << json << '\n';
  return passed ? 0 : 1;
}
