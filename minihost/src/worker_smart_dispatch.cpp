#include "worker_smart_dispatch.hpp"

#include "render_subsystem.h"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <cstring>
#include <iostream>

namespace aexcompat::worker_runtime::smart_dispatch {
namespace {

// What the empty-layer allocation needs, set by the dispatch and read by the
// hook it installs. Thread-local for the same reason the smart runtime's own
// state is: one worker can render on more than one thread.
struct EmptyLayerGeometry {
  int32_t width{};
  int32_t height{};
  int32_t pixel_format{};
};
thread_local EmptyLayerGeometry g_empty_layer;

// `clear_pixels`, because an empty layer is transparent rather than absent.
// Failure leaves the caller without a world, and `checkout_pixels` fails closed
// on that rather than handing back an uninitialized struct.
bool allocate_empty_layer_world(void* world_storage) {
  return g_empty_layer.width > 0 && g_empty_layer.height > 0 &&
      world_registry::new_world(nullptr, g_empty_layer.width, g_empty_layer.height,
                                /*clear_pixels=*/1, g_empty_layer.pixel_format,
                                world_storage) == 0;
}

constexpr int32_t kSmartPreRender = 23;
constexpr int32_t kSmartRender = 24;
constexpr int32_t kSmartRenderGpu = 31;
constexpr int32_t kGpuDeviceSetup = 32;
constexpr int32_t kGpuDeviceSetdown = 33;
// `PF_RenderRequest` (AE_Effect.h) leads both selector inputs and is 44 bytes:
// rect at 0, `PF_Field` at 16, `PF_ChannelMask` at 20, then
// `preserve_rgb_of_zero_alpha`, padding, and reserved words. `bitdepth`
// follows it, and `PF_SmartRenderInput` continues with `pre_render_data`.
constexpr std::size_t kRenderRequestBytes = 44;
constexpr std::size_t kRenderRequestField = 16;
constexpr std::size_t kRenderRequestChannelMask = 20;
constexpr std::size_t kInputBitdepth = 44;
constexpr std::size_t kSmartInputPreRenderData = 48;
constexpr int32_t kFieldFrame = 0;
// Hypothesis, not an observation: AE is assumed to pass PF_ChannelMask_ARGB
// for an ordinary frame render, so 0xF is written. No probe has recorded what
// AE actually passes (the selector-timeline probe captures output_request.rect
// only). What is certain is that 0 reads as "no channel requested", and that
// PreRender and SmartRender must see the same value.
constexpr int32_t kChannelMaskArgb = 0xF;

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

void write_render_request(std::byte* destination,
                          const std::array<int32_t, 4>& rect) {
  std::memset(destination, 0, kRenderRequestBytes);
  std::memcpy(destination, rect.data(), sizeof(rect));
  const int32_t field = kFieldFrame;
  const int32_t channel_mask = kChannelMaskArgb;
  std::memcpy(destination + kRenderRequestField, &field, sizeof(field));
  std::memcpy(destination + kRenderRequestChannelMask, &channel_mask,
              sizeof(channel_mask));
}

}  // namespace

SelectorInputs build_selector_inputs(const std::array<int32_t, 4>& request_rect,
                                     int16_t bitdepth, void* pre_render_data) {
  SelectorInputs inputs{};
  write_render_request(inputs.pre_render.data(), request_rect);
  write<int16_t>(inputs.pre_render, kInputBitdepth, bitdepth);
  write_render_request(inputs.smart_render.data(), request_rect);
  write<int16_t>(inputs.smart_render, kInputBitdepth, bitdepth);
  write<void*>(inputs.smart_render, kSmartInputPreRenderData, pre_render_data);
  return inputs;
}

SelectorInputLayout selector_input_layout() {
  return {static_cast<int32_t>(kRenderRequestBytes),
          static_cast<int32_t>(kRenderRequestField),
          static_cast<int32_t>(kRenderRequestChannelMask),
          static_cast<int32_t>(kInputBitdepth),
          static_cast<int32_t>(kSmartInputPreRenderData)};
}

bool verify_selector_inputs() {
  const std::array<int32_t, 4> rect{3, 2, 11, 8};
  int32_t pre_render_marker = 0;
  for (const int16_t bitdepth : {int16_t{8}, int16_t{16}, int16_t{32}}) {
    const SelectorInputs inputs =
        build_selector_inputs(rect, bitdepth, &pre_render_marker);
    // The regression this guards is asymmetry: SmartRender used to get a zeroed
    // prefix while PreRender got the real one. Compare the two byte for byte
    // over the shared prefix rather than re-reading each field with the same
    // constants that wrote it.
    if (!std::equal(inputs.pre_render.begin(),
                    inputs.pre_render.begin() + kInputBitdepth + sizeof(bitdepth),
                    inputs.smart_render.begin()))
      return false;
    std::array<int32_t, 4> observed{};
    std::memcpy(observed.data(), inputs.smart_render.data(), sizeof(observed));
    if (observed != rect) return false;
    if (read<int32_t>(inputs.smart_render, kRenderRequestField) != kFieldFrame)
      return false;
    if (read<int32_t>(inputs.smart_render, kRenderRequestChannelMask) !=
        kChannelMaskArgb)
      return false;
    if (read<int16_t>(inputs.smart_render, kInputBitdepth) != bitdepth) return false;
    // The request prefix must stop before `pre_render_data`: PreRender's pointer
    // has to reach SmartRender intact.
    if (read<void*>(inputs.smart_render, kSmartInputPreRenderData) !=
        static_cast<void*>(&pre_render_marker))
      return false;
    // Nothing past the pointer belongs to this builder.
    if (!std::all_of(inputs.smart_render.begin() + kSmartInputPreRenderData +
                         sizeof(void*),
                     inputs.smart_render.end(),
                     [](std::byte value) { return value == std::byte{}; }))
      return false;
  }
  return true;
}

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
  // CPU renders must not touch any GPU backend: an unconditional context
  // start loads the CUDA runtime (nvcuda.dll plus NVIDIA driver-store
  // DLLs) that the module audit cannot classify, failing every sealed
  // smart dispatch on NVIDIA machines even for pure CPU plans (issue #185).
  const bool gpu_context_started = plan.gpu_negotiation &&
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

  std::array<std::byte, 16> pre_callbacks{};
  std::array<std::byte, 24> pre_extra{};
  const std::array<int32_t, 4> expected_request = plan.partial_output_request
      ? std::array<int32_t, 4>{3, 2, 11, 8}
      : std::array<int32_t, 4>{0, 0, plan.width, plan.height};
  const int16_t render_bitdepth = plan.float32 ? 32 : (plan.deep16 ? 16 : 8);
  // Both selector inputs come from one builder, so SmartRender cannot be handed
  // a different request or bitdepth than PreRender was (issue #699).
  // `pre_render_data` is only known after PreRender ran; it is written into the
  // SmartRender copy below.
  SelectorInputs selector_inputs =
      build_selector_inputs(expected_request, render_bitdepth, nullptr);
  std::array<std::byte, 64>& pre_input = selector_inputs.pre_render;
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
  // `params` holds the input plus one entry per declared parameter, matching
  // the SDK's "0 = input, 1..n = param" indexing for checkout_layer.
  runtime.param_count = params.empty()
      ? 0 : static_cast<int32_t>(params.size() - 1);
  runtime.pixel_format = plan.float32 ? "argb32f" : (plan.deep16 ? "argb16" : "argb8");
  // Before the selector, not after it: PreRender is where a SmartFX plug-in
  // checks its input out, and the registration that checkout leaves behind is
  // what SmartRender later hands pixels from. Assigning this only in the render
  // preamble below registered a null world for the whole negotiation (issue
  // #675). The secondary and hosted-layer worlds are already set up before
  // dispatch, so this brings the primary input in line with them.
  //
  // `output_world` deliberately stays below: nothing in PreRender reads it, and
  // publishing it early would also widen the GPU world registry's view of it
  // before the transport that backs it is prepared.
  runtime.input_world = plan.missing_input ? nullptr : request.input_world->data();
  // The layer a plug-in gets when it checks out a layer parameter this host has
  // no layer for, as a hook the runtime calls on the first checkout that needs
  // one. Through the host's own new-world path, so the world registry owns it
  // and the host's own callbacks resolve it: copying or sampling an empty layer
  // is an ordinary thing for a plug-in to do, and `PF_COPY` resolves its
  // arguments through that registry (issue #962).
  //
  // On first need rather than here, because that registry is bounded (256 MB,
  // 64 worlds) and shared with the plug-in's own PF_NEW_WORLD: a full frame
  // taken on every dispatch is a full frame taken from an effect building a
  // scratch pyramid, on frames where nothing asks for an empty layer at all.
  g_empty_layer.width = plan.width;
  g_empty_layer.height = plan.height;
  g_empty_layer.pixel_format = plan.float32 ? world_registry::kPixelFormatArgb128
      : (plan.deep16 ? world_registry::kPixelFormatArgb64
                     : world_registry::kPixelFormatArgb32);
  runtime.allocate_empty_layer = &allocate_empty_layer_world;
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
  result.empty_result_rect = result.rects_valid && smart_bounds.empty_result;
  result.returns_extra_pixels =
      (read<uint16_t>(dispatch_state.pre_output, 34) & 0x1u) != 0;
  // Without RETURNS_EXTRA_PIXELS the SDK does not admit result > request. The
  // overrun is surfaced as an explicit diagnostic rather than a render
  // failure: AE silently clips, and blocking here would turn an observable
  // compatibility gap into a dead end for real-AEX observation.
  result.result_within_request =
      render::smart_rect_contained(smart_bounds.result_rect, expected_request);
  result.extra_pixels_contract_violation = result.rects_valid &&
      !result.returns_extra_pixels && !result.result_within_request;
  if (result.rects_valid && !result.empty_result_rect) {
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
    // AE 25.3 observation (issue #102): the output world carries the
    // result_rect top-left as PF_LayerDef::origin_x/origin_y (offset 104/108),
    // and in_data.output_origin (276/280) is the position of the layer origin
    // inside that buffer, i.e. the negated result_rect top-left.
    write<int32_t>(*request.output_world, 104, smart_bounds.origin_x);
    write<int32_t>(*request.output_world, 108, smart_bounds.origin_y);
    write<int32_t>(*request.input, 276, -smart_bounds.result_rect[0]);
    write<int32_t>(*request.input, 280, -smart_bounds.result_rect[1]);
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

  std::array<std::byte, 24> callbacks{};
  std::array<std::byte, 16> smart_extra{};
  std::array<std::byte, 72>& smart_input = selector_inputs.smart_render;
  write<void*>(smart_input, kSmartInputPreRenderData,
               read<void*>(dispatch_state.pre_output, 40));
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
  runtime.output_world = request.output_world->data();
  if (plan.gpu_negotiation &&
      ((!plan.missing_input && !request.formats->register_world(
          request.input_world->data(), world_registry::kPixelFormatGpuBgra128)) ||
       !request.formats->register_world(request.output_world->data(),
                                        world_registry::kPixelFormatGpuBgra128)))
    result.pre_error = 4;

  const int32_t render_selector = plan.gpu_negotiation && result.gpu_render_possible
      ? kSmartRenderGpu : kSmartRender;
  // One predicate drives the selector call, the GPU transport, and the
  // dispatch reporting, so a skipped render (empty result or rejected
  // geometry) never prepares device transport or claims a GPU dispatch.
  const bool will_dispatch = result.pre_error == 0 && result.rects_valid &&
      !result.empty_result_rect;
  result.gpu_render_dispatched = render_selector == kSmartRenderGpu && will_dispatch;
  transport::RenderTransport render_transport;
  bool transport_ready = !result.gpu_render_dispatched || !use_transport;
  bool transport_prepared = false;
  if (result.gpu_render_dispatched && use_transport) {
    transport_ready = transport::prepare_render_transport(
        runtime.input_world, runtime.output_world, render_transport);
    transport_prepared = transport_ready;
    // prepare_render_transport swaps each world's +24 pixel pointer to the GPU
    // device buffer (so PF_GPUDeviceSuite1::GetGPUWorldData returns it). The
    // pre-transport register_world above captured the host pointer, so re-register
    // with the device-pointer layout the plug-in actually observes during
    // dispatch; otherwise PF_GetPixelFormat's dispatch-format resolve rejects the
    // layout mismatch and the SmartRenderGPU selector fails with
    // PF_Err_OUT_OF_MEMORY (issue #305).
    if (transport_ready &&
        ((!plan.missing_input && !request.formats->register_world(
              runtime.input_world, world_registry::kPixelFormatGpuBgra128)) ||
         !request.formats->register_world(runtime.output_world,
                                          world_registry::kPixelFormatGpuBgra128)))
      transport_ready = false;
  }
  std::cerr << "stage:"
            << (result.gpu_render_dispatched ? "smart_render_gpu" : "smart_render_cpu")
            << "_begin\n" << std::flush;
  if (result.empty_result_rect && result.pre_error == 0) {
    // A legally empty result_rect renders nothing; the selector is skipped.
    result.render_error = 0;
  } else if (will_dispatch && transport_ready) {
    if (plan.gpu_negotiation) hooks.capture_module_audit();
    runtime.gpu_render_dispatched = result.gpu_render_dispatched;
    result.selector_dispatched = true;
    result.selector_error = hooks.guarded_call(request.entry, render_selector,
        request.input->data(), request.output->data(), params.data(), nullptr,
        smart_extra.data());
    runtime.gpu_render_dispatched = false;
    result.render_error = result.selector_error;
  } else {
    // Invalid geometry (rects_valid false with a successful pre-render) lands
    // here too: dispatching into the stale full-frame output world would turn
    // the rejected rects into a silent render, so the run fails explicitly.
    result.render_error = result.pre_error == 0 ? -6 : -1;
  }
  if (transport_prepared) {
    // Free the device allocations and restore each world's +24 host pointer
    // whenever the transport was prepared, even if the device-pointer re-register
    // above failed and suppressed the selector, so a prepared transport never
    // leaks its allocations (#305 review).
    if (!transport::finish_render_transport(render_transport) && result.render_error == 0)
      result.render_error = -6;
    // finish_render_transport restored +24 to the host pointer; re-register the
    // host layout so a post-dispatch PF_GetPixelFormat (e.g. a plug-in that
    // queries a render world during GPU device setdown) resolves the current
    // layout instead of the now-stale device pointer (mirror of #305).
    if (!plan.missing_input)
      request.formats->register_world(runtime.input_world,
                                      world_registry::kPixelFormatGpuBgra128);
    request.formats->register_world(runtime.output_world,
                                    world_registry::kPixelFormatGpuBgra128);
  }
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
  result.input_checkout_result_rect = runtime.input_checkout_result_rect;
  result.map_checkout_result_rect = runtime.map_checkout_result_rect;
  result.malformed_checkout_requests = runtime.malformed_checkout_requests;
  result.empty_checkout_pixel_denials = runtime.empty_checkout_pixel_denials;
  result.empty_layer_param_checkouts = runtime.empty_layer_param_checkouts;
  result.empty_layer_param_pixel_checkouts =
      runtime.empty_layer_param_pixel_checkouts;
  return true;
}

}  // namespace aexcompat::worker_runtime::smart_dispatch
