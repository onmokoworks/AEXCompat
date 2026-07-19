#include "worker_smart_dispatch.hpp"

#include "render_subsystem.h"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_world_registry.hpp"

#include <cstring>
#include <iostream>

namespace aexcompat::worker_runtime::smart_dispatch {
namespace {

constexpr int32_t kSmartPreRender = 23;
constexpr int32_t kSmartRender = 24;
constexpr int32_t kSmartRenderGpu = 31;
constexpr int32_t kGpuDeviceSetup = 32;
constexpr int32_t kGpuDeviceSetdown = 33;

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& buffer, std::size_t offset, T value) {
  std::memcpy(buffer.data() + offset, &value, sizeof(value));
}

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& buffer, std::size_t offset) {
  T value{};
  std::memcpy(&value, buffer.data() + offset, sizeof(value));
  return value;
}

}  // namespace

bool dispatch(const Request& request, const Hooks& hooks,
              smart_execution::Result& result, State& dispatch_state) {
  if (!request.entry || !request.input || !request.output || !request.plan ||
      !request.parameters || !request.input_world || !request.output_world ||
      !request.formats || !request.guarded || !request.destination ||
      !hooks.guarded_call || !hooks.capture_module_audit ||
      !hooks.guid_mix_in_callback ||
      !hooks.automatic_checkin)
    return false;

  const auto& plan = *request.plan;
  auto& runtime = smart::state();
  auto& params = request.parameters->params;
  namespace transport = gpu_runtime::memory_world_transport;

  std::array<std::byte, 8> gpu_setup_input{}, gpu_setup_output{};
  std::array<std::byte, 16> gpu_setup_extra{};
  const int32_t gpu_framework =
      (plan.fixture_gpu_negotiation || plan.directx_gpu_negotiation)
          ? 4
          : (plan.opencl_gpu_negotiation ? 1 : 3);
  const bool use_transport = plan.gpu_negotiation &&
      (gpu_framework == 1 || gpu_framework == 3 || gpu_framework == 4);
  if (plan.gpu_negotiation) hooks.capture_module_audit();
  const bool gpu_context_started =
      transport::begin_backend_context(gpu_framework, plan.gpu_device_index);
  if (plan.gpu_negotiation) {
    write<int32_t>(gpu_setup_input, 0, gpu_framework);
    write<uint32_t>(gpu_setup_input, 4, plan.gpu_device_index);
    write<void*>(gpu_setup_extra, 0, gpu_setup_input.data());
    write<void*>(gpu_setup_extra, 8, gpu_setup_output.data());
    std::cerr << "stage:gpu_device_setup_begin\n" << std::flush;
    result.gpu_setup_error = gpu_context_started
        ? hooks.guarded_call(request.entry, kGpuDeviceSetup, request.input->data(),
                             request.output->data(), params.data(), nullptr,
                             gpu_setup_extra.data())
        : -6;
    std::cerr << "stage:gpu_device_setup_end error=" << result.gpu_setup_error
              << "\n" << std::flush;
  }

  std::array<std::byte, 64> pre_input{};
  std::array<std::byte, 16> pre_callbacks{};
  std::array<std::byte, 24> pre_extra{};
  const std::array<int32_t, 4> expected_request = plan.partial_output_request
      ? std::array<int32_t, 4>{3, 2, 11, 8}
      : std::array<int32_t, 4>{0, 0, plan.width, plan.height};
  std::memcpy(pre_input.data(), expected_request.data(), sizeof(expected_request));
  write<int16_t>(pre_input, 44, plan.float32 ? 32 : (plan.deep16 ? 16 : 8));
  if (plan.gpu_negotiation) {
    write<void*>(pre_input, 48, read<void*>(gpu_setup_output, 0));
    write<int32_t>(pre_input, 56, gpu_framework);
    write<uint32_t>(pre_input, 60, plan.gpu_device_index);
  }
  write<void*>(pre_callbacks, 0, reinterpret_cast<void*>(&smart::pre_checkout_layer));
  write<void*>(pre_callbacks, 8, hooks.guid_mix_in_callback);
  write<void*>(pre_extra, 0, pre_input.data());
  write<void*>(pre_extra, 8, dispatch_state.pre_output.data());
  write<void*>(pre_extra, 16, pre_callbacks.data());
  runtime.input_checkout_request.fill(-1);
  runtime.map_checkout_request.fill(-1);
  runtime.secondary_checkout_id = -1;
  runtime.width = plan.width;
  runtime.height = plan.height;
  runtime.rowbytes = plan.rowbytes;
  runtime.pixel_format = plan.float32 ? "argb32f" : (plan.deep16 ? "argb16" : "argb8");
  std::cerr << "stage:smart_pre_render_begin\n" << std::flush;
  result.pre_error = (!plan.gpu_negotiation || result.gpu_setup_error == 0)
      ? hooks.guarded_call(request.entry, kSmartPreRender, request.input->data(),
                           request.output->data(), params.data(), nullptr,
                           pre_extra.data())
      : -1;
  std::cerr << "stage:smart_pre_render_end error=" << result.pre_error << "\n"
            << std::flush;
  hooks.automatic_checkin();

  const render::SmartOutputBounds smart_bounds = render::prepare_smart_output_bounds(
      dispatch_state.pre_output.data(), dispatch_state.pre_output.size(), plan.pixel_bytes);
  result.result_rect = smart_bounds.result_rect;
  result.max_result_rect = smart_bounds.max_result_rect;
  result.rects_valid = result.pre_error == 0 && smart_bounds.valid;
  if (result.rects_valid) {
    if (!request.guarded->reset(
            static_cast<std::size_t>(smart_bounds.rowbytes) * smart_bounds.height)) {
      result.rects_valid = false;
      result.pre_error = -3;
    }
    *request.destination = request.guarded->data();
    if (!render::prepare_world_layout(
            *request.output_world,
            {(plan.deep16 || plan.float32) ? 1 : 0, plan.pixel_bytes,
             smart_bounds.width, smart_bounds.height, smart_bounds.rowbytes},
            *request.destination) ||
        !request.formats->register_world(request.output_world->data(),
                                         request.dispatch_pixel_format))
      result.rects_valid = false;
    write<int32_t>(*request.input, 276, -smart_bounds.max_result_rect[0]);
    write<int32_t>(*request.input, 280, -smart_bounds.max_result_rect[1]);
    result.output_width = smart_bounds.width;
    result.output_height = smart_bounds.height;
    result.output_rowbytes = smart_bounds.rowbytes;
  }
  result.roi_contract_valid = !plan.partial_output_request ||
      (runtime.input_checkout_request == expected_request &&
       runtime.map_checkout_request == expected_request &&
       result.result_rect == expected_request &&
       result.max_result_rect == expected_request);
  result.gpu_render_possible =
      (read<uint16_t>(dispatch_state.pre_output, 34) & 0x2u) != 0;
  result.checkout_time = runtime.checkout_time;
  result.checkout_time_step = runtime.checkout_time_step;
  result.checkout_time_scale = runtime.checkout_time_scale;

  std::array<std::byte, 72> smart_input{};
  std::array<std::byte, 24> callbacks{};
  std::array<std::byte, 16> smart_extra{};
  write<void*>(smart_input, 48, read<void*>(dispatch_state.pre_output, 40));
  if (plan.gpu_negotiation) {
    write<void*>(smart_input, 56, read<void*>(gpu_setup_output, 0));
    write<int32_t>(smart_input, 64, gpu_framework);
    write<uint32_t>(smart_input, 68, plan.gpu_device_index);
  }
  write<void*>(callbacks, 0, reinterpret_cast<void*>(&smart::checkout_pixels));
  write<void*>(callbacks, 8, reinterpret_cast<void*>(&smart::checkin_pixels));
  write<void*>(callbacks, 16, reinterpret_cast<void*>(&smart::checkout_output));
  write<void*>(smart_extra, 0, smart_input.data());
  write<void*>(smart_extra, 8, callbacks.data());
  runtime.input_world = plan.missing_input ? nullptr : request.input_world->data();
  runtime.output_world = request.output_world->data();
  if (plan.gpu_negotiation &&
      ((!plan.missing_input && !request.formats->register_world(
          request.input_world->data(), world_registry::kPixelFormatGpuBgra128)) ||
       !request.formats->register_world(request.output_world->data(),
                                        world_registry::kPixelFormatGpuBgra128)))
    result.pre_error = 4;

  const int32_t render_selector = plan.gpu_negotiation && result.gpu_render_possible
      ? kSmartRenderGpu : kSmartRender;
  result.gpu_render_dispatched = render_selector == kSmartRenderGpu;
  transport::RenderTransport render_transport;
  const bool transport_ready = !result.gpu_render_dispatched || !use_transport ||
      transport::prepare_render_transport(runtime.input_world, runtime.output_world,
                                          render_transport);
  std::cerr << "stage:"
            << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_begin\n" << std::flush;
  if (result.pre_error == 0 && transport_ready) {
    if (plan.gpu_negotiation) hooks.capture_module_audit();
    result.selector_error = hooks.guarded_call(request.entry, render_selector,
        request.input->data(), request.output->data(), params.data(), nullptr,
        smart_extra.data());
    result.render_error = result.selector_error;
  } else {
    result.render_error = result.pre_error == 0 ? -6 : -1;
  }
  if (result.gpu_render_dispatched && use_transport && transport_ready &&
      !transport::finish_render_transport(render_transport) && result.render_error == 0)
    result.render_error = -6;
  std::cerr << "stage:"
            << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_end error=" << result.render_error << "\n" << std::flush;

  if (plan.gpu_negotiation && result.gpu_setup_error == 0) {
    std::array<std::byte, 16> setdown_input{};
    std::array<std::byte, 8> setdown_extra{};
    write<void*>(setdown_input, 0, read<void*>(gpu_setup_output, 0));
    write<int32_t>(setdown_input, 8, gpu_framework);
    write<uint32_t>(setdown_input, 12, plan.gpu_device_index);
    write<void*>(setdown_extra, 0, setdown_input.data());
    hooks.capture_module_audit();
    std::cerr << "stage:gpu_device_setdown_begin\n" << std::flush;
    result.gpu_setdown_error = invoke_entry_seh(request.entry, kGpuDeviceSetdown,
        request.input->data(), request.output->data(), params.data(), nullptr,
        setdown_extra.data(), &result.gpu_setdown_exception_code);
    std::cerr << "stage:gpu_device_setdown_end error=" << result.gpu_setdown_error
              << "\n" << std::flush;
  }
  if (plan.gpu_negotiation) hooks.capture_module_audit();
  if (plan.gpu_negotiation && gpu_context_started &&
      !transport::end_backend_context(gpu_framework) && result.gpu_setdown_error == 0)
    result.gpu_setdown_error = -6;
  return true;
}

}  // namespace aexcompat::worker_runtime::smart_dispatch
