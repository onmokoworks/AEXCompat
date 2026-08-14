#pragma once

#include "worker_parameter_execution.hpp"
#include "worker_request_parser.hpp"
#include "render_pixel_buffer.hpp"
#include "render_subsystem.h"
#include "worker_world_safety.hpp"

#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::smart_setup {

struct Context {
  int32_t secondary_layer_slot{};
  int32_t full_resolution_width{};
  int32_t full_resolution_height{};
  int32_t pixel_aspect_numerator{1};
  uint32_t pixel_aspect_denominator{1};
};

struct Request {
  parameter_execution::BufferOut* command_output{};
  const std::string* case_id{};
  bool has_external_rgba{};
  int32_t external_width{};
  int32_t external_height{};
  int32_t external_current_time{};
  uint32_t external_time_scale{1};
  int32_t external_pixel_bytes{4};
};

struct Plan {
  bool valid{};
  bool deep16{};
  bool float32{};
  bool gpu_negotiation{};
  bool fixture_gpu_negotiation{};
  bool opencl_gpu_negotiation{};
  bool directx_gpu_negotiation{};
  bool explicit_gpu_device{};
  bool force_cpu_image{};
  bool missing_input{};
  bool crash_null_output{};
  bool temporal_context{};
  bool partial_output_request{};
  bool connected_map{};
  uint32_t gpu_device_index{};
  int32_t width{};
  int32_t height{};
  int32_t pixel_bytes{};
  int32_t rowbytes{};
};

Plan prepare(const Context&, const Request&);

// GPU-required fallback (issue #1072): the color family advertises CPU smart
// render (out_flags2 bit10) but returns PF_Err 14 at the start of SMART_RENDER
// because it only implements the GPU path. out_flags2 does not distinguish those
// effects from ones that render on CPU, so the only signal is the runtime 14.
// When the frame loop sees it (with GPU F32 advertised) it sets this flag and
// re-runs the frame, which routes the retry through the GPU transport. Thread-
// local: the render thread that sets it is the one prepare() reads it on.
void set_force_gpu_retry(bool);
bool force_gpu_retry_requested();

// RAII holder for the flag above: sets it for the duration of a scope so that an
// early return or a throw out of the retry dispatch cannot leave it stuck true
// (which would force every later frame's first attempt onto the GPU and disable
// the retry block via the force_gpu_retry_requested() guard).
class ForceGpuRetryScope {
 public:
  ForceGpuRetryScope() { set_force_gpu_retry(true); }
  ~ForceGpuRetryScope() { set_force_gpu_retry(false); }
  ForceGpuRetryScope(const ForceGpuRetryScope&) = delete;
  ForceGpuRetryScope& operator=(const ForceGpuRetryScope&) = delete;
};

struct WorldBuffers {
  render_safety::InputPixelBuffer* source{};
  render_safety::OutputPixelBuffer* output{};
  unsigned char** destination{};
  std::array<std::byte, 120>* input_world{};
  std::array<std::byte, 120>* output_world{};
  std::array<std::byte, 120>* input_checkout_view{};
  std::array<std::byte, 120>* map_checkout_view{};
  world_safety::DispatchWorldFormatScope* formats{};
  render::MapWorld* map{};
};

bool prepare_world_buffers(const Plan&, const std::string&,
                           const std::vector<unsigned char>*, bool,
                           WorldBuffers);

struct ParameterHooks {
  bool (*apply_animation)(parameter_execution::Definitions&, int32_t, uint32_t){};
  void (*dump_world)(const std::string&, const unsigned char*, int32_t, int32_t,
                     int32_t){};
};

struct ParameterState {
  explicit ParameterState(std::size_t definition_count,
                          std::size_t external_layer_count);
  ~ParameterState();
  parameter_execution::Definitions definitions;
  std::vector<std::vector<unsigned char>> hosted_pixels;
  std::vector<std::array<std::byte, 120>> hosted_worlds;
  std::vector<std::array<std::byte, 120>> hosted_view_worlds;
  std::vector<void*> params;
  std::vector<unsigned char> pre_render_source;
};

struct ParameterRequest {
  parameter_execution::EffectEntry entry{};
  parameter_execution::BufferIn* input{};
  parameter_execution::BufferOut* output{};
  const std::string* case_id{};
  const Plan* plan{};
  const parameters::RequestedAssignments* requested{};
  const std::vector<request_parser::LayerInput>* external_layers{};
  int32_t external_current_time{};
  int32_t external_time_step{1};
  int32_t external_total_time{1};
  uint32_t external_time_scale{1};
  int32_t full_resolution_width{};
  int32_t full_resolution_height{};
  int32_t dispatch_pixel_format{};
  std::array<std::byte, 120>* input_world{};
  world_safety::DispatchWorldFormatScope* formats{};
  render_safety::InputPixelBuffer* source{};
};

// Publishes the frame's time fields (current time, step, total, scale) into
// the command input buffer. prepare_parameters calls this itself; the smart
// render path additionally calls it before interpolate_arbitrary_values runs,
// because that runs ahead of prepare_parameters (issue #993) and reads the
// current/total time from these offsets.
void publish_frame_times(const ParameterRequest&);

bool prepare_parameters(const ParameterRequest&, ParameterState&,
                        const ParameterHooks&);

// Exercises production Smart parameter preparation through the point
// animation write and exposes only the resulting value assertion.
bool verify_animation_extent_wiring_for_test();

}  // namespace aexcompat::worker_runtime::smart_setup
