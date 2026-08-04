#pragma once

#include "gpu_memory_world_transport.hpp"
#include "runtime_module_audit.hpp"
#include "worker_smart_execution.hpp"
#include "worker_smart_setup.hpp"

#include <array>
#include <cstddef>

namespace aexcompat::worker_runtime::smart_dispatch {

struct State {
  std::array<std::byte, 56> pre_output{};
};

struct Request {
  parameter_execution::EffectEntry entry{};
  parameter_execution::BufferIn* input{};
  parameter_execution::BufferOut* output{};
  smart_setup::Plan const* plan{};
  smart_setup::ParameterState* parameters{};
  std::array<std::byte, 120>* input_world{};
  std::array<std::byte, 120>* output_world{};
  world_safety::DispatchWorldFormatScope* formats{};
  render_safety::OutputPixelBuffer* guarded{};
  unsigned char** destination{};
  int32_t dispatch_pixel_format{};
};

struct Hooks {
  int32_t (*guarded_call)(parameter_execution::EffectEntry, int32_t, void*, void*,
                          void**, void*, void*){};
  ModuleAuditSnapshot (*capture_module_audit)(){};
  void* guid_mix_in_callback{};
  void (*automatic_checkin)(){};
};

/// The two selector inputs a SmartFX dispatch hands the plug-in:
/// `PF_PreRenderInput` (64 bytes) and `PF_SmartRenderInput` (72 bytes). Both
/// lead with the same `PF_RenderRequest` and `bitdepth`; only the tails differ
/// (GPU fields on one, `pre_render_data` plus GPU fields on the other).
struct SelectorInputs {
  std::array<std::byte, 64> pre_render{};
  std::array<std::byte, 72> smart_render{};
};

/// Builds both selector inputs from the values PreRender was negotiated with,
/// so the two cannot drift apart. SmartRender used to receive the whole shared
/// prefix zeroed - an empty request rect, an all-zero channel mask, and
/// bitdepth 0 - while PreRender received the real values (issue #699).
///
/// `pre_render_data` is the pointer PreRender left in `PF_PreRenderOutput`; the
/// GPU tails are the caller's to overlay afterwards.
SelectorInputs build_selector_inputs(const std::array<int32_t, 4>& request_rect,
                                     int16_t bitdepth, void* pre_render_data);

/// Offsets this translation unit uses inside the two selector inputs, reported
/// so the frozen SDK ABI evidence can be compared against them instead of the
/// numbers only agreeing with themselves (issue #699).
struct SelectorInputLayout {
  int32_t render_request_bytes{};
  int32_t field_offset{};
  int32_t channel_mask_offset{};
  int32_t bitdepth_offset{};
  int32_t pre_render_data_offset{};
};

SelectorInputLayout selector_input_layout();

/// Self-test hook: both selector inputs carry the same request rect, field,
/// channel mask, and bitdepth, and `pre_render_data` survives them intact.
bool verify_selector_inputs();

bool dispatch(const Request&, const Hooks&, smart_execution::Result&, State&);

}  // namespace aexcompat::worker_runtime::smart_dispatch
