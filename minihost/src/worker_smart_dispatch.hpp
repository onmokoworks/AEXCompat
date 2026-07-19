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

bool dispatch(const Request&, const Hooks&, smart_execution::Result&, State&);

}  // namespace aexcompat::worker_runtime::smart_dispatch
