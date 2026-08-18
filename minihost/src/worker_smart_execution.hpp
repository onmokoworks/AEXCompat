#pragma once

#include "worker_parameter_execution.hpp"
#include "worker_request_parser.hpp"
#include "worker_smart_runtime.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::smart_execution {

using EffectEntry = parameter_execution::EffectEntry;
using Input = parameter_execution::BufferIn;
using Output = parameter_execution::BufferOut;
using RequestedAssignments = parameters::RequestedAssignments;
using ExternalLayerInput = request_parser::LayerInput;

// One resident-session frame (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md
// v1.1). Non-null selects the frame-only lifecycle (SEQUENCE_SETUP/SETDOWN are
// hoisted by the session loop) and captures the packed ARGB output for the
// shared-memory slot instead of a file. guards_intact is only meaningful when
// output_buffer_allocated is true: early refusals never build the guarded
// buffer, so there is nothing to corrupt.
struct SessionFrame {
  std::vector<unsigned char>* captured_argb{};
  bool output_buffer_allocated{};
  bool guards_intact{true};
};

struct Result {
  std::shared_ptr<const smart::Snapshot> runtime{
      std::make_shared<smart::Snapshot>()};
  int32_t gpu_setup_error{};
  int32_t pre_error{-1};
  int32_t selector_error{-1};
  int32_t render_error{-1};
  int32_t gpu_setdown_error{};
  uint32_t gpu_setdown_exception_code{};
  std::string input_hash;
  std::string output_hash;
  bool rects_valid{};
  bool guards_intact{};
  bool output_pixels_valid{};
  // The guarded output still held its initialization sentinel after a
  // successful selector. Kept separate from non-finite/otherwise-invalid
  // output so only this exact no-output case can authorize route fallback.
  bool output_untouched{};
  bool gpu_render_possible{};
  bool gpu_render_dispatched{};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  bool roi_contract_valid{};
  std::array<int32_t, 4> result_rect{};
  std::array<int32_t, 4> max_result_rect{};
  std::array<int32_t, 4> input_checkout_result_rect{-1, -1, -1, -1};
  std::array<int32_t, 4> map_checkout_result_rect{-1, -1, -1, -1};
  uint32_t malformed_checkout_requests{};
  uint32_t empty_checkout_pixel_denials{};
  // A layer parameter this host has no world for, answered with an empty rect
  // at PreRender and with the empty world at checkout (issue #898). Separate
  // from the denials above so a report reader can tell the two apart.
  uint32_t empty_layer_param_checkouts{};
  uint32_t empty_layer_param_pixel_checkouts{};
  int32_t output_width{};
  int32_t output_height{};
  int32_t output_rowbytes{};
  // Top-left of the emitted buffer in layer coordinates. Equals the plug-in's
  // `result_rect` top-left for a rendered result; for the empty-result
  // passthrough the plug-in's rect is empty and this is the request rect, so a
  // reader of the emitted frame has somewhere to put it (issue #1285).
  int32_t output_origin_x{};
  int32_t output_origin_y{};
  // PF_RenderOutputFlag_RETURNS_EXTRA_PIXELS (pre_output flags bit 0x1): the
  // SDK admits result_rect > output_request.rect only when this is set. An
  // overrun without the flag is surfaced as a diagnostic, not a failure.
  bool returns_extra_pixels{};
  bool result_within_request{true};
  bool extra_pixels_contract_violation{};
  // A legally empty result_rect skips the render selector instead of
  // dispatching into a zero-sized world.
  bool empty_result_rect{};
  // ... and the frame that reached the output was the effect's input, copied
  // by the host because the effect promised no pixels. Always false unless
  // `empty_result_rect` is true. It is on the record so a nonempty frame from
  // an effect that rendered nothing can never read as one the effect produced
  // (issue #1285; the AE captures behind it are cited at the assignment site
  // in worker_smart_dispatch.cpp).
  bool empty_result_passthrough{};
  // True only when the Smart Render selector was actually invoked; a NOP
  // passthrough, an empty-result skip, and an invalid-geometry refusal all
  // leave it false so the report reflects the real dispatch decision.
  bool selector_dispatched{};
  // The Premiere GPU-filter route (xGPUFilterEntry) was entered on this
  // dispatch, whatever it answered. The session frame loop reads it so a CPU
  // SMART_RENDER 512 is retried through that route only when the route was not
  // already offered (issue #1271).
  bool pr_gpu_route_attempted{};
  // `selector_error` is the host's substitute (a caught fault, an escaped C++
  // exception, a failed module audit), not what the plug-in returned. Measured
  // around the Smart Render selector call alone, so a fault in another selector
  // of the same frame does not colour this one (issue #1271).
  bool selector_failure_substituted{};
  // The Premiere GPU-filter route saw a non-finite value in the float32 frame
  // the plug-in produced before narrowing it to an 8/16bpc session's depth.
  // finalize folds it into the same `output_pixels_valid` / -6 verdict a
  // float32 session gets from its own finite check, so the depth a session
  // renders at does not decide whether a broken output is a diagnostic.
  bool output_non_finite{};
  std::array<int32_t, 4> output_extent_hint{};
};

using Execute = Result (*)(EffectEntry, Input&, Output&, const std::string&,
    const RequestedAssignments*, const std::vector<unsigned char>*,
    int32_t, int32_t,
    const std::vector<ExternalLayerInput>*, int32_t, int32_t, int32_t,
    uint32_t, int32_t, SessionFrame*);

struct Hooks {
  Execute execute{};
  bool (*module_audit_required)(){};
};

bool configure(const Hooks&) noexcept;
Result render_once(EffectEntry, Input&, Output&, const std::string&,
                   const RequestedAssignments* = nullptr,
                   const std::vector<unsigned char>* = nullptr,
                   int32_t = 0, int32_t = 0,
                   const std::vector<ExternalLayerInput>* = nullptr,
                   int32_t = 0, int32_t = 1, int32_t = 1,
                   uint32_t = 1, int32_t = 4, SessionFrame* = nullptr);

}  // namespace aexcompat::worker_runtime::smart_execution
