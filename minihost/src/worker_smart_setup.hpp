#pragma once

#include "worker_parameter_execution.hpp"
#include "worker_request_parser.hpp"
#include "render_pixel_buffer.hpp"
#include "render_subsystem.h"
#include "worker_world_safety.hpp"

#include <cstdint>
#include <string>
#include <utility>

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

// Runs the production plan preparation used before Smart PreRender for the
// fixed seed-max image case shared with Classic.
bool verify_fixed_image_case_admission();

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

// Premiere GPU-filter fallback (issue #1271): a plug-in that exports
// xGPUFilterEntry and answers PF_Err 512 from CPU SMART_RENDER in an 8/16bpc
// session only implements the GPU path (the VR family; the CPU path just draws
// a "requires GPU acceleration" notice). The export alone does not separate
// those from effects whose PF CPU path works (Levels2, Box_Blur, Lumetri and
// others export it too), so the runtime 512 is the signal. When the frame loop
// sees it, it sets this flag and re-runs the frame, which lets the smart
// dispatch take the Premiere GPU-filter route at the session depth. Thread-
// local like the GPU retry flag above, with the same RAII holder shape.
void set_force_pr_gpu_retry(bool, int32_t cause);
bool force_pr_gpu_retry_requested();
// The PF CPU path's own refusal that sent this frame to the GPU route, or 0
// when the route was offered for another reason (a float32 plan, a
// gpu_*_float32 case). It rides on the route's `stage:pr_gpu_route_begin`
// line so a `rendered` verdict still names the refusal it replaced: the
// route's own `_end reason=committed` says the frame came from the GPU, and
// nothing else in a default report would say the CPU path had been turned
// away (issue #1283).
int32_t pr_gpu_retry_cause();

class ForcePrGpuRetryScope {
 public:
  explicit ForcePrGpuRetryScope(int32_t cause) {
    set_force_pr_gpu_retry(true, cause);
  }
  ~ForcePrGpuRetryScope() { set_force_pr_gpu_retry(false, 0); }
  ForcePrGpuRetryScope(const ForcePrGpuRetryScope&) = delete;
  ForcePrGpuRetryScope& operator=(const ForcePrGpuRetryScope&) = delete;
};

// Resident smart sessions can observe the PF selector result and retry, so
// they defer an otherwise export-first float32 Premiere route. One-shot
// callers have no frame-loop retry and retain the existing float32 behavior.
void set_pr_gpu_pf_first(bool);
bool pr_gpu_pf_first_requested();

class PrGpuPfFirstScope {
 public:
  PrGpuPfFirstScope() { set_pr_gpu_pf_first(true); }
  ~PrGpuPfFirstScope() { set_pr_gpu_pf_first(false); }
  PrGpuPfFirstScope(const PrGpuPfFirstScope&) = delete;
  PrGpuPfFirstScope& operator=(const PrGpuPfFirstScope&) = delete;
};

// Production resident-frame coordinator seam. Every SmartFX attempt made by
// the session loop goes through this wrapper, including bounded retries.
template <typename Attempt>
decltype(auto) run_pr_gpu_pf_first_session_attempt(Attempt&& attempt) {
  const PrGpuPfFirstScope pf_first;
  return std::forward<Attempt>(attempt)();
}

struct WorldBuffers {
  render_safety::InputPixelBuffer* source{};
  render_safety::OutputPixelBuffer* output{};
  unsigned char** destination{};
  aexcompat::world_safety::EffectWorldStorage* input_world{};
  aexcompat::world_safety::EffectWorldStorage* output_world{};
  aexcompat::world_safety::EffectWorldStorage* input_checkout_view{};
  aexcompat::world_safety::EffectWorldStorage* map_checkout_view{};
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
  std::vector<aexcompat::world_safety::EffectWorldStorage> hosted_worlds;
  std::vector<aexcompat::world_safety::EffectWorldStorage> hosted_view_worlds;
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
  aexcompat::world_safety::EffectWorldStorage* input_world{};
  world_safety::DispatchWorldFormatScope* formats{};
  render_safety::InputPixelBuffer* source{};
};

// Publishes the frame's time fields (current time, step, total, scale) into
// the command input buffer. prepare_parameters calls this before any explicit
// arbitrary-text or keyed-animation callback so those value-producing
// callbacks observe this frame rather than the previous one.
void publish_frame_times(const ParameterRequest&);

bool prepare_parameters(const ParameterRequest&, ParameterState&,
                        const ParameterHooks&);

// Exercises production Smart parameter preparation through the point
// animation write and exposes only the resulting value assertion.
bool verify_animation_extent_wiring_for_test();

}  // namespace aexcompat::worker_runtime::smart_setup
