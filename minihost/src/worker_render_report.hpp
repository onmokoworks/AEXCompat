#pragma once

#include <iosfwd>
#include <sstream>
#include <array>
#include <cstdint>
#include <string>

namespace aexcompat::worker_render_report {

// Owns one complete ordered report before it becomes externally observable.
// Callers append fields in schema order; emit publishes the JSON atomically.
class ReportSnapshot {
 public:
  explicit ReportSnapshot(const std::ios& formatting_source);

  std::ostream& stream() noexcept { return stream_; }
  const std::string json() const { return stream_.str(); }

 private:
  std::ostringstream stream_;
};

struct CustomUiSnapshot {
  bool click_dispatched{};
  int32_t click_error{};
  int32_t click_out_flags{};
  bool click_changed_value{};
  bool draw_dispatched{};
  int32_t draw_error{};
  int32_t draw_out_flags{};
  std::array<int32_t, 4> lifecycle_errors{};
  bool context_closed{};
  uint64_t color_picker_calls{};
  uint64_t invalidate_rect_calls{};
  std::array<float, 4> picker_color{};
};

void append_custom_ui(ReportSnapshot& report, const CustomUiSnapshot& snapshot);

struct RequestedParametersSnapshot {
  std::string parameters_json;
  int32_t amount{};
  int32_t direction{};
  int32_t seed{};
  double mix{};
  int32_t invert_map{};
  bool render_performed{};
  std::string module_audit_json;
};

void finish_requested_parameters(
    ReportSnapshot& report, const RequestedParametersSnapshot& snapshot);

void emit(const ReportSnapshot& snapshot, std::ostream& output);

}  // namespace aexcompat::worker_render_report
