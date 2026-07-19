#pragma once

#include "worker_parameter_execution.hpp"
#include "worker_request_parser.hpp"
#include "worker_smart_runtime.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::smart_execution {

using EffectEntry = parameter_execution::EffectEntry;
using Input = parameter_execution::BufferIn;
using Output = parameter_execution::BufferOut;
using RequestedAssignments = parameters::RequestedAssignments;
using ExternalLayerInput = request_parser::LayerInput;

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
  bool gpu_render_possible{};
  bool gpu_render_dispatched{};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  bool roi_contract_valid{};
  std::array<int32_t, 4> result_rect{};
  std::array<int32_t, 4> max_result_rect{};
  int32_t output_width{};
  int32_t output_height{};
  int32_t output_rowbytes{};
};

using Execute = Result (*)(EffectEntry, Input&, Output&, const std::string&,
    const RequestedAssignments*, const std::vector<unsigned char>*,
    const std::filesystem::path*, int32_t, int32_t,
    const std::vector<ExternalLayerInput>*, int32_t, int32_t, int32_t,
    uint32_t, int32_t);

struct Hooks {
  Execute execute{};
  bool (*module_audit_required)(){};
};

bool configure(const Hooks&) noexcept;
Result render_once(EffectEntry, Input&, Output&, const std::string&,
                   const RequestedAssignments* = nullptr,
                   const std::vector<unsigned char>* = nullptr,
                   const std::filesystem::path* = nullptr,
                   int32_t = 0, int32_t = 0,
                   const std::vector<ExternalLayerInput>* = nullptr,
                   int32_t = 0, int32_t = 1, int32_t = 1,
                   uint32_t = 1, int32_t = 4);

}  // namespace aexcompat::worker_runtime::smart_execution
