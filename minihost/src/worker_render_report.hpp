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
  struct Callbacks {
    bool param_checkouts_balanced{};
    std::array<int64_t, 8> param{};
    std::string escaped_options_button_name;
    std::array<int64_t, 11> host{};
  } callbacks;
  struct Context {
    bool request_mode{};
    std::array<int32_t, 2> downsample_x{};
    std::array<int32_t, 2> downsample_y{};
    std::array<int32_t, 2> pixel_aspect_ratio{};
    std::array<int32_t, 2> full_resolution_dimensions{};
    std::array<int32_t, 6> scalar_metadata{};
    std::array<int32_t, 2> input_dimensions{};
    std::array<int32_t, 2> pre_effect_source_origin{};
    std::array<int32_t, 2> output_origin{};
  } context;
};

void begin_classic(ReportSnapshot& report, const ClassicReport::Head& snapshot);
void append_classic_sequence(ReportSnapshot& report, const ClassicReport::Sequence& snapshot);
void append_classic_audio(ReportSnapshot& report, const ClassicReport::Audio& snapshot);
void append_classic_frame(ReportSnapshot& report, const ClassicReport::Frame& snapshot);
void append_classic_threads(ReportSnapshot& report, const ClassicReport::Threads& snapshot);
void append_classic_callbacks(ReportSnapshot& report, const ClassicReport::Callbacks& snapshot);
void append_classic_context(ReportSnapshot& report, const ClassicReport::Context& snapshot);

struct SmartReport {
  struct Head {
    bool completed{};
    std::array<int64_t, 4> setup_flags{};
    std::array<bool, 4> advertised{};
    std::array<bool, 5> runtime_flags{};
    uint64_t rejected_temporal_checkouts{};
    std::array<int64_t, 9> host_context{};
    bool depth_supported{};
    std::array<int64_t, 6> selector_errors{};
    std::array<bool, 2> gpu_flags{};
    std::array<int64_t, 3> checkout_time{};
    bool roi_contract_valid{};
    std::array<int32_t, 4> input_checkout{};
    std::array<int32_t, 4> map_checkout{};
    int32_t global_setdown_error{};
    std::string case_id;
    std::string pixel_format;
    std::array<int32_t, 3> dimensions{};
    std::string input_sha256;
    std::string output_sha256;
    bool result_rects_valid{};
    std::string world_debug_json;
  } head;
  struct Context {
    std::array<int32_t, 4> result_rect{};
    std::array<int32_t, 4> max_result_rect{};
    std::array<bool, 3> validity{};
    std::array<int64_t, 4> parameter_checkouts{};
    bool request_mode{};
    std::array<int32_t, 2> downsample_x{};
    std::array<int32_t, 2> downsample_y{};
    std::array<int32_t, 2> pixel_aspect_ratio{};
    std::array<int32_t, 2> full_resolution_dimensions{};
    std::array<int32_t, 6> scalar_metadata{};
    std::array<int32_t, 2> input_dimensions{};
    std::array<int32_t, 2> pre_effect_source_origin{};
    std::array<int32_t, 2> output_origin{};
  } context;
  struct Lifetimes {
    std::string mask_scene_id;
    std::array<int64_t, 3> mask_geometry{};
    bool mask_balanced{};
    std::array<int64_t, 6> mask_handles{};
    bool lifetime_fault{};
    bool suites_balanced{};
    std::array<int64_t, 4> suites{};
    std::string missing_suites_json;
    std::string live_suite_leases;
    bool suite_fault{};
    bool handles_balanced{};
    std::array<int64_t, 2> handles{};
    std::array<int64_t, 13> arbitrary{};
    double arbitrary_interpolation_amount{};
    std::array<int64_t, 7> handle_details{};
    bool handle_fault{};
    bool world_fault{};
    bool worlds_balanced{};
    std::array<int64_t, 5> worlds{};
    bool gpu_balanced{};
    std::array<int64_t, 6> gpu_allocations{};
  } lifetimes;
  struct Faults {
    std::array<int64_t, 5> cuda{};
    std::array<int64_t, 5> opencl{};
    std::array<int64_t, 5> directx{};
    bool directx_context_used{};
    bool pixel_format_fault{};
    std::array<int64_t, 4> pixel_format{};
    std::array<bool, 7> faults{};
    std::array<int64_t, 15> operations{};
    std::array<int64_t, 5> aegp_memory{};
  } faults;
};

void begin_smart(ReportSnapshot& report, const SmartReport::Head& snapshot);
void append_smart_context(ReportSnapshot& report, const SmartReport::Context& snapshot);
void append_smart_lifetimes(ReportSnapshot& report, const SmartReport::Lifetimes& snapshot);
void append_smart_faults(ReportSnapshot& report, const SmartReport::Faults& snapshot);

struct GpuDiagnosticsSnapshot {
  bool memory_lifetimes_balanced{};
  bool cuda_context_used{};
  std::array<int64_t, 5> cuda{};  // upload, download, failures, count, index
  bool opencl_context_used{};
  std::array<int64_t, 5> opencl{};
  bool directx_context_used{};
  std::array<int64_t, 5> directx{};  // count, index, upload, download, failures
  std::array<int64_t, 6> allocations{};  // created, freed, live count/bytes, depth, invalid
};

void append_gpu_diagnostics(ReportSnapshot& report, const GpuDiagnosticsSnapshot& snapshot);

struct SehDiagnosticsSnapshot {
  uint32_t code{};
  uint64_t address{};
  std::string escaped_module;
  std::string escaped_selector;
  int32_t error{};
};

void append_seh_diagnostics(ReportSnapshot& report, const SehDiagnosticsSnapshot& snapshot);

struct ClassicSubsystemDiagnostics {
  bool suite_balanced{};
  std::array<int64_t, 4> suite_counts{};  // acquires, releases, live leases, references
  std::string missing_suites_json;
  std::string live_suite_leases;
  bool handle_balanced{};
  bool path_balanced{};
  std::array<int64_t, 8> path_counts{};
  std::array<double, 2> path_feather{};
  double path_opacity{};
  int64_t path_quality{};
  std::array<int64_t, 4> path_bounds{};
  std::array<int64_t, 2> handles{};
  std::array<int64_t, 15> arbitrary{};
  double arbitrary_interpolation_amount{};
  bool world_balanced{};
  std::array<int64_t, 2> worlds{};
  bool receipt_balanced{};
  std::array<int64_t, 5> receipts{};
  bool async_balanced{};
  std::array<int64_t, 7> async{};
};

struct ClassicEmission {
  ClassicReport report;
  CustomUiSnapshot custom_ui;
  ClassicSubsystemDiagnostics subsystems;
  GpuDiagnosticsSnapshot gpu;
  SehDiagnosticsSnapshot seh;
  RequestedParametersSnapshot requested;
  bool selector_dispatched{};
  bool depth_supported{};
  int32_t render_error{};
};

void emit_classic_complete(ReportSnapshot& output, const ClassicEmission& emission);

void append_classic_subsystems(
    ReportSnapshot& report, const ClassicSubsystemDiagnostics& snapshot);

void emit(const ReportSnapshot& snapshot, std::ostream& output);

}  // namespace aexcompat::worker_render_report
