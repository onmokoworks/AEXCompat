#pragma once

#include "render_lifecycle.hpp"
#include "worker_smart_dispatch.hpp"
#include "worker_smart_finalize.hpp"

namespace aexcompat::worker_runtime::smart_render_runtime {

struct Request {
  parameter_execution::EffectEntry entry{};
  parameter_execution::BufferIn* input{};
  parameter_execution::BufferOut* output{};
  smart_setup::Plan const* plan{};
  smart_setup::ParameterState* parameters{};
  std::array<std::byte, 120>* input_world{};
  std::array<std::byte, 120>* output_world{};
  world_safety::DispatchWorldFormatScope* formats{};
  render_safety::InputPixelBuffer* source{};
  render_safety::OutputPixelBuffer* guarded{};
  unsigned char** destination{};
  const render_lifecycle::RenderLifecycle* lifecycle{};
  int32_t dispatch_pixel_format{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  int32_t pixel_bytes{};
  smart_execution::SessionFrame* session{};
};

struct Hooks {
  bool (*dispatch_render_draw)(parameter_execution::EffectEntry,
      parameter_execution::BufferIn&, parameter_execution::BufferOut&,
      parameter_execution::Definitions&){};
  smart_dispatch::Hooks dispatch;
  smart_finalize::Hooks finalize;
};

bool execute(const Request&, const Hooks&, smart_execution::Result&);

}  // namespace aexcompat::worker_runtime::smart_render_runtime
