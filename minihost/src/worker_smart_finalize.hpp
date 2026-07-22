#pragma once
#include "render_lifecycle.hpp"
#include "render_pixel_buffer.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_smart_execution.hpp"
#include "worker_smart_setup.hpp"
#include <array>

namespace aexcompat::worker_runtime::smart_finalize {
struct Hooks {
  bool (*close_ui)(parameter_execution::EffectEntry, parameter_execution::BufferIn&,
                   parameter_execution::BufferOut&,
                   parameter_execution::Definitions&){};
  int32_t (*end_lifecycle)(parameter_execution::EffectEntry,
      parameter_execution::BufferIn&, parameter_execution::BufferOut&, void**,
      void*, const render_lifecycle::RenderLifecycle&, int32_t){};
  void (*dump_world)(const std::string&, const unsigned char*, int32_t, int32_t,
                     int32_t){};
  std::string (*sha256)(const unsigned char*, std::size_t){};
  bool (*ui_active)(){};
};
struct Request {
  parameter_execution::EffectEntry entry{};
  parameter_execution::BufferIn* input{};
  parameter_execution::BufferOut* output{};
  smart_setup::ParameterState* parameters{};
  std::array<std::byte, 120>* output_world{};
  const render_lifecycle::RenderLifecycle* lifecycle{};
  render_safety::InputPixelBuffer* source{};
  render_safety::OutputPixelBuffer* guarded{};
  unsigned char* destination{};
  int32_t width{}, height{}, rowbytes{}, pixel_bytes{};
  std::array<std::byte, 56>* pre_output{};
  smart_execution::SessionFrame* session{};
};
bool finalize(const Request&, const Hooks&, smart_execution::Result&);
}
