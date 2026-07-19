#pragma once

#include "worker_parameter_execution.hpp"

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

}  // namespace aexcompat::worker_runtime::smart_setup
