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

struct ClassicReport {
  struct Head {
    bool completed{};
    int32_t global_setup_error{};
    int32_t params_setup_error{};
    uint32_t advertised_out_flags{};
    uint32_t advertised_out_flags2{};
    bool image_render_supported{};
    bool nop_render_advertised{};
    bool input_write_advertised{};
    bool expand_buffer_advertised{};
    bool shrink_buffer_advertised{};
    bool wide_time_checkout_allowed{};
    uint32_t rejected_temporal_param_checkouts{};
    bool shutter_dependency_advertised{};
  } head;
  struct Sequence {
    bool persistent{};
    int32_t setup_error{};
    int32_t setdown_error{};
    std::array<int32_t, 2> frame_errors{};
    std::array<std::string, 2> frame_hashes{};
    bool flattened{};
    int32_t flatten_error{};
    int32_t resetup_error{};
    bool flattened_handle_replaced{};
    bool resetup_handle_replaced{};
    bool flattened_handle_host_disposed{};
    bool copied_flattened{};
    int32_t get_flattened_error{};
    bool original_preserved{};
  } sequence;
  struct Audio {
    bool usage_advertised{};
    bool checkout_allowed{};
    bool source_available{};
    uint64_t rejected_unadvertised_checkouts{};
    uint64_t rejected_format_requests{};
    uint64_t handle_exhaustions{};
    uint64_t peak_live_handles{};
    uint64_t checkout_calls{};
    uint64_t checkin_calls{};
    uint64_t get_data_calls{};
    uint64_t invalid_operations{};
    int64_t last_checkout_start_time{};
    int64_t last_checkout_duration{};
    uint32_t last_checkout_time_scale{};
    int64_t last_window_start_sample{};
    int64_t last_window_sample_count{};
    int64_t last_window_silence_samples{};
    uint32_t last_output_rate{};
    int32_t last_output_bytes_per_sample{};
    int32_t last_output_channels{};
    int32_t last_output_format{};
    int64_t last_returned_sample_frames{};
    bool lifetimes_balanced{};
  } audio;
  struct Frame {
    int32_t global_setdown_error{};
    std::string escaped_return_message;
    std::string case_id;
    std::string pixel_format;
    int32_t width{};
    int32_t height{};
    int32_t rowbytes{};
    int32_t bytes_written_per_row{};
    int32_t undefined_tail_bytes_per_row{};
    std::string input_sha256;
    std::string output_sha256;
    bool guard_bytes_intact{};
    std::string world_debug_json;
  } frame;
  struct Threads {
    bool concurrent{};
    std::array<int32_t, 2> errors{};
    std::array<std::string, 2> hashes{};
    std::array<bool, 2> guards_intact{};
  } threads;
};

void begin_classic(ReportSnapshot& report, const ClassicReport::Head& snapshot);
void append_classic_sequence(ReportSnapshot& report, const ClassicReport::Sequence& snapshot);
void append_classic_audio(ReportSnapshot& report, const ClassicReport::Audio& snapshot);
void append_classic_frame(ReportSnapshot& report, const ClassicReport::Frame& snapshot);
void append_classic_threads(ReportSnapshot& report, const ClassicReport::Threads& snapshot);

void emit(const ReportSnapshot& snapshot, std::ostream& output);

}  // namespace aexcompat::worker_render_report
