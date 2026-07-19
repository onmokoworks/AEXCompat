#include "worker_smart_setup.hpp"

#include "gpu_device_info_registry.hpp"
#include "worker_smart_runtime.hpp"

#include <algorithm>
#include <cstring>

namespace aexcompat::worker_runtime::smart_setup {
namespace {
template <typename T>
T read(const parameter_execution::BufferOut& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
}  // namespace

Plan prepare(const Context& context, const Request& request) {
  Plan plan;
  if (!request.command_output || !request.case_id || request.external_time_scale == 0)
    return plan;
  const auto& output = *request.command_output;
  const auto& case_id = *request.case_id;
  auto& state = smart::state();
  constexpr uint32_t kWideTimeInput = 1u << 1;
  constexpr uint32_t kAutomaticWideTimeInput = 1u << 17;
  state.wide_time_checkout_allowed =
      (read<uint32_t>(output, 96) & kWideTimeInput) != 0 ||
      (read<uint32_t>(output, 400) & kAutomaticWideTimeInput) != 0;
  state.current_time = request.external_current_time;
  state.current_time_scale = request.external_time_scale;
  state.rejected_temporal_checkouts = 0;
  state.secondary_layer_slot = context.secondary_layer_slot;
  state.full_resolution_width = context.full_resolution_width;
  state.full_resolution_height = context.full_resolution_height;
  state.pixel_aspect_numerator = context.pixel_aspect_numerator;
  state.pixel_aspect_denominator = context.pixel_aspect_denominator;

  plan.deep16 = case_id == "deep16_default" ||
      (request.has_external_rgba && request.external_pixel_bytes == 8);
  plan.fixture_gpu_negotiation = case_id == "gpu_fallback_float32";
  plan.opencl_gpu_negotiation = case_id == "gpu_opencl_float32";
  plan.directx_gpu_negotiation = case_id == "gpu_directx_float32";
  constexpr const char* kGpuPrefix = "gpu_device_";
  plan.explicit_gpu_device = case_id.rfind(kGpuPrefix, 0) == 0;
  if (plan.explicit_gpu_device) {
    const std::string ordinal = case_id.substr(std::strlen(kGpuPrefix));
    if (ordinal.empty() || ordinal.size() > 2 ||
        !std::all_of(ordinal.begin(), ordinal.end(), [](unsigned char ch) {
          return ch >= '0' && ch <= '9';
        })) return plan;
    plan.gpu_device_index = static_cast<uint32_t>(std::stoul(ordinal));
    if (plan.gpu_device_index >= gpu_runtime::kMaxGpuDevices) return plan;
  }
  plan.force_cpu_image = case_id == "request_cpu";
  const bool advertised_gpu_support =
      (read<uint32_t>(output, 400) & (1u << 25)) != 0;
  plan.gpu_negotiation = plan.fixture_gpu_negotiation ||
      plan.opencl_gpu_negotiation || plan.directx_gpu_negotiation ||
      plan.explicit_gpu_device ||
      (request.has_external_rgba && request.external_pixel_bytes == 16 &&
       advertised_gpu_support && !plan.force_cpu_image);
  plan.missing_input = case_id == "error_missing_input";
  plan.crash_null_output = case_id == "crash_null_output_world";
  plan.temporal_context = case_id == "temporal_context";
  plan.partial_output_request = case_id == "partial_output_request";
  plan.float32 = case_id == "float32_default" || plan.gpu_negotiation ||
      (request.has_external_rgba && request.external_pixel_bytes == 16);
  plan.connected_map = case_id == "connected_map" || case_id == "inverted_map";
  plan.width = request.has_external_rgba ? request.external_width :
      (plan.connected_map ? 11 :
       ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 13 : 16));
  plan.height = request.has_external_rgba ? request.external_height :
      (plan.connected_map ? 7 :
       ((case_id == "odd_dimensions" || case_id == "padded_stride") ? 9 : 12));
  if (plan.width <= 0 || plan.height <= 0 || plan.width > 4096 ||
      plan.height > 4096) return plan;
  plan.pixel_bytes = plan.float32 ? 16 : (plan.deep16 ? 8 : 4);
  plan.rowbytes = case_id == "padded_stride" ? 64 : plan.width * plan.pixel_bytes;
  if (case_id != "default" && case_id != "request" && !plan.deep16 &&
      !plan.float32 && !plan.missing_input && !plan.crash_null_output &&
      !plan.temporal_context && !plan.partial_output_request &&
      !plan.connected_map) return plan;
  plan.valid = true;
  return plan;
}

}  // namespace aexcompat::worker_runtime::smart_setup
