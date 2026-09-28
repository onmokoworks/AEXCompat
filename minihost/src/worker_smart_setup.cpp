#include "worker_smart_setup.hpp"

#include "generated/aex_abi_contract.hpp"

#include "gpu_device_info_registry.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_output_coverage.hpp"
#include "render_pixel_transport.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>

namespace aexcompat::l2_detail {
bool apply_parameter_animation(
    worker_runtime::parameter_execution::Definitions&, int32_t, uint32_t);
}

namespace aexcompat::worker_runtime::smart_setup {

namespace aexcompat_l2 = ::aexcompat::l2_detail;
namespace {
template <typename T>
T read(const parameter_execution::BufferOut& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
thread_local bool g_force_gpu_retry = false;
thread_local bool g_force_pr_gpu_retry = false;
thread_local int32_t g_pr_gpu_retry_cause = 0;
thread_local bool g_pr_gpu_pf_first = false;
}  // namespace

void set_force_gpu_retry(bool value) { g_force_gpu_retry = value; }
bool force_gpu_retry_requested() { return g_force_gpu_retry; }
void set_force_pr_gpu_retry(bool value, int32_t cause) {
  g_force_pr_gpu_retry = value;
  g_pr_gpu_retry_cause = value ? cause : 0;
}
bool force_pr_gpu_retry_requested() { return g_force_pr_gpu_retry; }
int32_t pr_gpu_retry_cause() { return g_pr_gpu_retry_cause; }
void set_pr_gpu_pf_first(bool value) { g_pr_gpu_pf_first = value; }
bool pr_gpu_pf_first_requested() { return g_pr_gpu_pf_first; }

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
      (request.advertised_out_flags2 & (1u << 25)) != 0;
  plan.gpu_negotiation = plan.fixture_gpu_negotiation ||
      plan.opencl_gpu_negotiation || plan.directx_gpu_negotiation ||
      plan.explicit_gpu_device ||
      (request.has_external_rgba && request.external_pixel_bytes == 16 &&
       advertised_gpu_support && !plan.force_cpu_image);
  // GPU-required fallback (issue #1072): the frame loop sets force_gpu_retry
  // after a CPU smart render returned PF_Err 14 from an effect advertising GPU
  // F32 support. Route the retry through the GPU transport.
  if (advertised_gpu_support && !plan.force_cpu_image && force_gpu_retry_requested())
    plan.gpu_negotiation = true;
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
  const auto& diagnostic = render::diagnostic_world_layout();
  plan.input_pixel_bytes = diagnostic.enabled && diagnostic.input_pixel_bytes != 0
      ? diagnostic.input_pixel_bytes : plan.pixel_bytes;
  plan.rowbytes = case_id == "padded_stride" ? 64 : plan.width * plan.input_pixel_bytes;
  plan.output_rowbytes = plan.width * plan.pixel_bytes;
  if (diagnostic.enabled) {
    if (plan.gpu_negotiation ||
        diagnostic.input_row_padding % plan.input_pixel_bytes != 0 ||
        diagnostic.output_row_padding % plan.pixel_bytes != 0 ||
        (diagnostic.has_request_rect &&
         (diagnostic.request_rect[2] > plan.width ||
          diagnostic.request_rect[3] > plan.height)) ||
        (diagnostic.has_extent_hint &&
         (diagnostic.extent_hint[2] > plan.width ||
          diagnostic.extent_hint[3] > plan.height))) return plan;
    plan.rowbytes += diagnostic.input_row_padding;
    plan.output_rowbytes = plan.width * plan.pixel_bytes +
        diagnostic.output_row_padding;
  }
  // `request_cpu` is a session case like `request`, and is admitted the same
  // way. It used to be admitted only incidentally, through `deep16`/`float32`
  // being true whenever the session was deep; dispatching a plug-in that
  // advertises neither deep depth at 8 bits makes those false for a deep
  // session and would turn the whole plan invalid. Named rather than widened to "any
  // external image", which would have admitted an unrecognised case_id too
  // wherever the classic route's `prepare_image_request` still answers -2 for
  // one. (The terms below can still admit an unrecognised case_id on their
  // own, at a depth the plug-in advertises; sessions never carry one.)
  if (!render::is_fixed_image_case(case_id) && case_id != "request" &&
      !plan.force_cpu_image && !plan.deep16 &&
      !plan.float32 && !plan.missing_input && !plan.crash_null_output &&
      !plan.temporal_context && !plan.partial_output_request &&
      !plan.connected_map) return plan;
  plan.valid = true;
  return plan;
}

bool verify_fixed_image_case_admission() {
  parameter_execution::BufferOut output{};
  const std::string case_id = "seed_max";
  const auto plan = prepare({}, {&output, &case_id, false, 0, 0, 0, 1, 4});
  return plan.valid && plan.width == 16 && plan.height == 12 &&
      plan.pixel_bytes == 4 && plan.rowbytes == 64;
}

bool verify_gpu_advertisement_snapshot() {
  constexpr uint32_t kGpuSupport = 1u << 25;
  parameter_execution::BufferOut output{};
  const std::string case_id = "request";
  Request request{&output, &case_id, true, 16, 12, 0, 1, 16, kGpuSupport};
  const auto advertised = prepare({}, request);
  request.advertised_out_flags2 = 0;
  std::memcpy(output.data() + 400, &kGpuSupport, sizeof(kGpuSupport));
  const auto not_advertised = prepare({}, request);
  return advertised.valid && advertised.gpu_negotiation &&
      not_advertised.valid && !not_advertised.gpu_negotiation;
}

// A world handed to a plug-in inside a PF_ParamDef. The copy keeps the
// `reserved_long4` it inherits, which points at the *live* world's PF_World
// object (the storage prefix): that is AE's own shape - a ParamDef's world
// names AE's PF_World for that layer - and it is the only facade such a world
// can have, because `world - 8` inside a ParamDef is the ParamDef's own bytes.
// A plug-in that writes through it (the issue #1090 origin shape) therefore
// writes the live world's fields, exactly as it would in AE; the host reads its
// own geometry from the render plan, not from those fields.
template <typename ParamDef, typename World>
void copy_world_into_param_def(ParamDef& definition, const World& world) {
  std::memcpy(definition.data() + 56, world.data(), world.size());
}

bool prepare_world_buffers(const Plan& plan, const std::string& case_id,
                           const std::vector<unsigned char>* external_rgba,
                           bool input_write_advertised,
                           WorldBuffers buffers) {
  if (!plan.valid || !buffers.source || !buffers.output || !buffers.destination ||
      !buffers.input_world || !buffers.output_world || !buffers.formats ||
      !buffers.input_checkout_view || !buffers.map_checkout_view ||
      !buffers.map || !*buffers.source || !*buffers.output) return false;
  auto& source = *buffers.source;
  std::memset(source.data(), render::diagnostic_world_layout().input_padding_byte,
              static_cast<std::size_t>(plan.rowbytes) * plan.height);
  if (external_rgba && external_rgba->size() !=
      static_cast<std::size_t>(plan.width) * plan.height * 4) return false;
  for (int32_t y = 0; y < plan.height; ++y) {
    for (int32_t x = 0; x < plan.width; ++x) {
      auto* pixel = &source[static_cast<std::size_t>(y) * plan.rowbytes +
                            static_cast<std::size_t>(x) * plan.input_pixel_bytes];
      if (external_rgba) {
        const auto* rgba = &(*external_rgba)[
            (static_cast<std::size_t>(y) * plan.width + x) * 4];
        render_pixel_transport::rgba8_to_argb(pixel, rgba, plan.input_pixel_bytes);
      } else if (plan.float32) {
        const float values[4] = {1.0f,
            static_cast<float>(x) / static_cast<float>(plan.width - 1),
            static_cast<float>(y) / static_cast<float>(plan.height - 1),
            static_cast<float>(x + y) /
                static_cast<float>(plan.width + plan.height - 2)};
        std::memcpy(pixel, values, sizeof(values));
      } else if (plan.deep16) {
        const uint16_t values[4] = {32768,
            static_cast<uint16_t>(x * 32768 / (plan.width - 1)),
            static_cast<uint16_t>(y * 32768 / (plan.height - 1)),
            static_cast<uint16_t>((x + y) * 32768 /
                                  (plan.width + plan.height - 2))};
        std::memcpy(pixel, values, sizeof(values));
      } else {
        pixel[0] = 255;
        pixel[1] = static_cast<unsigned char>(x * 255 / (plan.width - 1));
        pixel[2] = static_cast<unsigned char>(y * 255 / (plan.height - 1));
        pixel[3] = static_cast<unsigned char>((x + y) * 255 /
                                              (plan.width + plan.height - 2));
      }
    }
  }
  if (!source.set_plugin_writable(input_write_advertised)) return false;
  *buffers.destination = buffers.output->data();
  if (!output_coverage::seed(*buffers.destination, buffers.output->size(),
          plan.width, plan.height, plan.output_rowbytes, plan.pixel_bytes)) return false;
  const render::WorldLayout input_layout{plan.input_pixel_bytes == 4 ? 0 : 1,
      plan.input_pixel_bytes, plan.width, plan.height, plan.rowbytes};
  const render::WorldLayout output_layout{(plan.deep16 || plan.float32) ? 1 : 0,
      plan.pixel_bytes, plan.width, plan.height, plan.output_rowbytes};
  if (!render::prepare_world_layout(*buffers.input_world, input_layout, source.data()) ||
      !render::prepare_world_layout(*buffers.output_world, output_layout,
                                    *buffers.destination)) return false;
  const auto& diagnostic = render::diagnostic_world_layout();
  if (diagnostic.enabled) {
    std::memcpy(buffers.input_world->data() + 104,
                &diagnostic.input_origin_x, sizeof(diagnostic.input_origin_x));
    std::memcpy(buffers.input_world->data() + 108,
                &diagnostic.input_origin_y, sizeof(diagnostic.input_origin_y));
    if (diagnostic.has_extent_hint)
      std::memcpy(buffers.input_world->data() + 44,
                  diagnostic.extent_hint.data(), sizeof(diagnostic.extent_hint));
  }
  if (!render::capture_requested_world(
          "smart-input", *buffers.input_world, source.data(),
          static_cast<std::size_t>(plan.rowbytes) * plan.height,
          plan.input_pixel_bytes)) return false;
  const int32_t pixel_format = plan.float32 ? world_registry::kPixelFormatArgb128 :
      (plan.deep16 ? world_registry::kPixelFormatArgb64 :
                     world_registry::kPixelFormatArgb32);
  const int32_t input_pixel_format = plan.input_pixel_bytes == 16
      ? world_registry::kPixelFormatArgb128 : (plan.input_pixel_bytes == 8
          ? world_registry::kPixelFormatArgb64 : world_registry::kPixelFormatArgb32);
  if (!buffers.formats->register_world(buffers.input_world->data(), input_pixel_format) ||
      !buffers.formats->register_world(buffers.output_world->data(), pixel_format))
    return false;
  *buffers.input_checkout_view = *buffers.input_world;
  auto& smart_state = smart::state();
  smart_state.input_checkout_view_world = buffers.input_checkout_view->data();
  smart_state.map_checkout_view_world = nullptr;
  if (plan.connected_map) {
    if (!render::prepare_connected_map_world(case_id, plan.width, plan.height,
                                             *buffers.map) ||
        !buffers.formats->register_world(
            buffers.map->world.data(), world_registry::kPixelFormatArgb32))
      return false;
    auto& state = smart::state();
    state.map_width = buffers.map->width;
    state.map_height = buffers.map->height;
    state.map_world = buffers.map->world.data();
    *buffers.map_checkout_view = buffers.map->world;
    state.map_checkout_view_world = buffers.map_checkout_view->data();
  }
  return true;
}

ParameterState::ParameterState(std::size_t definition_count,
                               std::size_t external_layer_count)
    : definitions(definition_count), hosted_pixels(external_layer_count),
      hosted_worlds(external_layer_count), hosted_view_worlds(external_layer_count) {}

ParameterState::~ParameterState() {
  parameters::state().checkout.definitions.clear();
}

void publish_frame_times(const ParameterRequest& request) {
  const auto& plan = *request.plan;
  const int32_t current_time = plan.temporal_context ? 42 :
      request.external_current_time;
  const int32_t time_step = plan.temporal_context ? 2 : request.external_time_step;
  const uint32_t time_scale = plan.temporal_context ? 24 : request.external_time_scale;
  const int32_t total_time = plan.temporal_context ? 240 : request.external_total_time;
  auto write_input = [&](std::size_t offset, const auto& value) {
    std::memcpy(request.input->data() + offset, &value, sizeof(value));
  };
  write_input(224, current_time); write_input(228, time_step);
  write_input(232, total_time); write_input(236, time_step); write_input(240, time_scale);
}

bool prepare_parameters(const ParameterRequest& request, ParameterState& prepared,
                        const ParameterHooks& hooks) {
  if (!request.entry || !request.input || !request.output || !request.case_id ||
      !request.plan || !request.input_world || !request.formats || !request.source ||
      !hooks.apply_animation || !hooks.dump_world) return false;
  const auto& plan = *request.plan;
  auto& runtime = parameters::state();
  auto& definitions = prepared.definitions;
  if (definitions.size() != runtime.records.size() + 1) return false;
  if (request.requested && !parameter_execution::apply_arbitrary_text_assignments(
          request.entry, *request.input, *request.output, definitions,
          *request.requested)) return false;
  parameter_execution::probe_arbitrary_scan(
      request.entry, *request.input, *request.output, definitions);
  for (std::size_t slot = 1; slot < definitions.size(); ++slot) {
    if (runtime.records[slot - 1].type == 0 &&
        runtime.records[slot - 1].layer_default == -1)
      copy_world_into_param_def(definitions[slot], *request.input_world);
  }
  auto& smart_state = smart::state();
  smart_state.hosted_layers.clear();
  if (request.external_layers) {
    for (std::size_t layer_index = 0;
         layer_index < request.external_layers->size(); ++layer_index) {
      const auto& layer = (*request.external_layers)[layer_index];
      const bool historical_input = layer.slot == 0 && layer.timed &&
          layer.time_scale == request.external_time_scale &&
          layer.time >= 0 && layer.time <= request.external_total_time &&
          layer.width == plan.width && layer.height == plan.height;
      if (!historical_input &&
          (layer.slot <= 0 ||
           static_cast<std::size_t>(layer.slot) >= definitions.size() ||
           runtime.records[layer.slot - 1].type != 0)) return false;
      if (layer.width <= 0 || layer.height <= 0 || layer.rgba.size() !=
          static_cast<std::size_t>(layer.width) * layer.height * 4) return false;
      const int32_t layer_pixel_bytes = layer.pixel_bytes != 0
          ? layer.pixel_bytes : plan.pixel_bytes;
      if (layer.has_world_layout && layer.row_padding % layer_pixel_bytes != 0)
        return false;
      auto& pixels = prepared.hosted_pixels[layer_index];
      pixels.resize(static_cast<std::size_t>(layer.width) * layer.height *
                    layer_pixel_bytes);
      for (std::size_t offset = 0; offset < layer.rgba.size(); offset += 4)
        render_pixel_transport::rgba8_to_argb(
            pixels.data() + (offset / 4) * layer_pixel_bytes,
            layer.rgba.data() + offset, layer_pixel_bytes);
      hooks.dump_world("smart-layer-slot" + std::to_string(layer.slot),
                       pixels.data(), layer.width, layer.height, layer_pixel_bytes);
      const int32_t packed_rowbytes = layer.width * layer_pixel_bytes;
      const int32_t layer_rowbytes = packed_rowbytes +
          (layer.has_world_layout ? layer.row_padding : 0);
      if (layer.has_world_layout && layer.row_padding != 0) {
        std::vector<unsigned char> strided(
            static_cast<std::size_t>(layer_rowbytes) * layer.height,
            layer.padding_byte);
        for (int32_t y = 0; y < layer.height; ++y)
          std::memcpy(strided.data() + static_cast<std::size_t>(y) * layer_rowbytes,
                      pixels.data() + static_cast<std::size_t>(y) * packed_rowbytes,
                      packed_rowbytes);
        pixels.swap(strided);
      }
      auto& world = prepared.hosted_worlds[layer_index];
      if (!render::prepare_world_layout(
              world, {layer_pixel_bytes == 4 ? 0 : 1, layer_pixel_bytes,
                       layer.width, layer.height, layer_rowbytes},
               pixels.data()) ||
           !request.formats->register_world(world.data(),
               layer_pixel_bytes == 4 ? world_registry::kPixelFormatArgb32 :
               (layer_pixel_bytes == 8 ? world_registry::kPixelFormatArgb64 :
                                         world_registry::kPixelFormatArgb128))) return false;
      if (layer.has_world_layout) {
        std::memcpy(world.data() + 104, &layer.origin_x, sizeof(layer.origin_x));
        std::memcpy(world.data() + 108, &layer.origin_y, sizeof(layer.origin_y));
        std::memcpy(world.data() + 44, layer.extent.data(), sizeof(layer.extent));
      }
      if (!render::capture_requested_world(
              "smart-layer-slot" + std::to_string(layer.slot), world,
              pixels.data(), pixels.size(), layer_pixel_bytes)) return false;
      auto& view_world = prepared.hosted_view_worlds[layer_index];
      view_world = world;
      const int32_t requested_time = plan.temporal_context ? 42 :
          request.external_current_time;
      const uint32_t requested_scale = plan.temporal_context ? 24 :
          request.external_time_scale;
      const bool same_time = static_cast<int64_t>(layer.time) * requested_scale ==
          static_cast<int64_t>(requested_time) * layer.time_scale;
      if (!historical_input && (!layer.timed || same_time))
        copy_world_into_param_def(definitions[layer.slot], world);
      smart_state.hosted_layers.push_back({layer.slot, layer.time, layer.time_scale,
          layer.timed, layer.width, layer.height, -1, world.data(),
          view_world.data(), {-1, -1, -1, -1}});
    }
  }
  if (request.requested) {
    // POINT/POINT_3D overrides are percentages of the layer size (issue
    // #1061 chain); the input world's extent turns them into pixels.
    int32_t layer_width = 0, layer_height = 0;
    std::memcpy(&layer_width, request.input_world->data() +
                aexcompat::abi::x86_64_windows::LAYER_WIDTH_OFFSET, sizeof(layer_width));
    std::memcpy(&layer_height, request.input_world->data() +
                aexcompat::abi::x86_64_windows::LAYER_HEIGHT_OFFSET, sizeof(layer_height));
    if (!parameter_execution::apply_requested_assignments(
            definitions, *request.requested, layer_width, layer_height)) return false;
  } else if (definitions.size() > 7) {
    const auto profile = render::prepare_parameter_profile(*request.case_id);
    auto write_i32 = [&](std::size_t slot, int32_t value) {
      std::memcpy(definitions[slot].data() + 56, &value, sizeof(value));
    };
    write_i32(1, profile.amount); write_i32(2, profile.direction);
    write_i32(3, profile.seed); write_i32(4, profile.repeat);
    std::memcpy(definitions[5].data() + 56, &profile.mix, sizeof(profile.mix));
    if (profile.inverted_map) write_i32(7, 1);
  }
  const int32_t animation_time = plan.temporal_context ? 42 :
      request.external_current_time;
  const uint32_t animation_scale = plan.temporal_context ? 24 :
      request.external_time_scale;
  parameters::set_animation_layer_extent(plan.width, plan.height);
  if (!hooks.apply_animation(definitions, animation_time, animation_scale) ||
      !parameter_execution::apply_arbitrary_parameter_animation(
          request.entry, *request.input, *request.output, definitions,
          animation_time, animation_scale)) return false;
  auto& checkout = runtime.checkout;
  checkout.definitions.clear();
  for (std::size_t slot = 0; slot < definitions.size(); ++slot)
    checkout.definitions.emplace(static_cast<int32_t>(slot), definitions[slot]);
  {
    std::lock_guard<std::mutex> lock(checkout.mutex);
    checkout.live.clear();
    checkout.checkout_calls = checkout.checkin_calls = checkout.invalid_checkins = 0;
    checkout.automatic_checkins = 0;
    checkout.last_index = -1;
    checkout.last_time = checkout.last_time_step = 0;
    checkout.last_time_scale = 0;
  }
  prepared.params.resize(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i)
    prepared.params[i] = definitions[i].data();
  publish_frame_times(request);
  auto write_input = [&](std::size_t offset, const auto& value) {
    std::memcpy(request.input->data() + offset, &value, sizeof(value));
  };
  const int32_t full_width = request.full_resolution_width > 0 ?
      request.full_resolution_width : plan.width;
  const int32_t full_height = request.full_resolution_height > 0 ?
      request.full_resolution_height : plan.height;
  write_input(252, full_width); write_input(256, full_height);
  const int32_t full_extent[4] = {0, 0, plan.width, plan.height};
  std::memcpy(request.input->data() + 260, full_extent, sizeof(full_extent));
  if (plan.partial_output_request) {
    const int32_t extent[4] = {3, 2, 11, 8};
    std::memcpy(request.input->data() + 260, extent, sizeof(extent));
  }
  const auto& diagnostic = render::diagnostic_world_layout();
  if (diagnostic.enabled && diagnostic.has_extent_hint)
    std::memcpy(request.input->data() + 260,
                diagnostic.extent_hint.data(), sizeof(diagnostic.extent_hint));
  smart_state.checkout_time = smart_state.checkout_time_step = 0;
  smart_state.checkout_time_scale = 0;
  prepared.pre_render_source.resize(
      static_cast<std::size_t>(plan.width) * plan.height * plan.input_pixel_bytes);
  for (int32_t row = 0; row < plan.height; ++row)
    std::memcpy(prepared.pre_render_source.data() +
                    static_cast<std::size_t>(row) * plan.width * plan.input_pixel_bytes,
                request.source->data() + static_cast<std::size_t>(row) * plan.rowbytes,
                static_cast<std::size_t>(plan.width) * plan.input_pixel_bytes);
  hooks.dump_world("smart-input", prepared.pre_render_source.data(), plan.width,
                   plan.height, plan.input_pixel_bytes);
  return true;
}

bool verify_animation_extent_wiring_for_test() {
  auto& runtime = parameters::state();
  if (runtime.records.size() < 3) return false;
  Plan plan{};
  plan.valid = true;
  plan.width = 256;
  plan.height = 144;
  plan.pixel_bytes = 4;
  plan.rowbytes = plan.width * plan.pixel_bytes;
  parameter_execution::BufferIn input{};
  parameter_execution::BufferOut output{};
  aexcompat::world_safety::EffectWorldStorage input_world{};
  render_safety::InputPixelBuffer source(
      static_cast<std::size_t>(plan.rowbytes) * plan.height);
  if (!source || !render::prepare_world_layout(
                     input_world, {0, plan.pixel_bytes, plan.width, plan.height,
                                   plan.rowbytes},
                     source.data()))
    return false;
  world_safety::DispatchWorldFormatScope formats;
  if (!formats.register_world(input_world.data(),
                              world_registry::kPixelFormatArgb32))
    return false;
  ParameterState prepared(runtime.records.size() + 1, 0);
  copy_world_into_param_def(prepared.definitions[0], input_world);
  parameter_execution::initialize_parameter_definitions(
      prepared.definitions, plan.width, plan.height);
  const std::string case_id = "request";
  ParameterRequest request{
      +[](int32_t, void*, void*, void**, void*, void*) { return int32_t{0}; },
      &input,
      &output,
      &case_id,
      &plan,
      nullptr,
      nullptr,
      0,
      1,
      1,
      24,
      plan.width,
      plan.height,
      0,
      &input_world,
      &formats,
      &source};
  const ParameterHooks hooks{
      &aexcompat_l2::apply_parameter_animation,
      +[](const std::string&, const unsigned char*, int32_t, int32_t, int32_t) {}};
  if (!prepare_parameters(request, prepared, hooks)) return false;
  const auto& point = prepared.definitions[2];
  const auto& point3d = prepared.definitions[3];
  auto read_definition = [](const auto& definition, std::size_t offset,
                            auto* value) {
    std::memcpy(value, definition.data() + offset, sizeof(*value));
  };
  int32_t point_x = 0, point_y = 0;
  double point3d_x = 0, point3d_y = 0, point3d_z = 0;
  read_definition(point, 56, &point_x);
  read_definition(point, 60, &point_y);
  read_definition(point3d, 56, &point3d_x);
  read_definition(point3d, 64, &point3d_y);
  read_definition(point3d, 72, &point3d_z);
  if (point_x != 128 * 65536 || point_y != 36 * 65536 ||
      std::abs(point3d_x - 64.0) >= 1e-12 ||
      std::abs(point3d_y - 72.0) >= 1e-12 ||
      std::abs(point3d_z - 108.0) >= 1e-12) return false;
  // The session's total-time endpoint is inclusive. Exercise the real world
  // preparation path, then prove a frame beyond that endpoint remains refused.
  request_parser::LayerInput endpoint{};
  endpoint.slot = 0;
  endpoint.time = request.external_total_time;
  endpoint.time_scale = request.external_time_scale;
  endpoint.timed = true;
  endpoint.width = plan.width;
  endpoint.height = plan.height;
  endpoint.rgba.assign(static_cast<std::size_t>(plan.width) * plan.height * 4, 255);
  std::vector<request_parser::LayerInput> layers{endpoint};
  request.external_layers = &layers;
  ParameterState endpoint_prepared(runtime.records.size() + 1, layers.size());
  copy_world_into_param_def(endpoint_prepared.definitions[0], input_world);
  parameter_execution::initialize_parameter_definitions(
      endpoint_prepared.definitions, plan.width, plan.height);
  const bool endpoint_ready = prepare_parameters(request, endpoint_prepared, hooks) &&
      smart::state().hosted_layers.size() == 1 &&
      smart::state().hosted_layers.front().world != nullptr;
  if (!endpoint_ready) {
    smart::state().hosted_layers.clear();
    return false;
  }
  layers.front().time = request.external_total_time + 1;
  ParameterState outside_prepared(runtime.records.size() + 1, layers.size());
  copy_world_into_param_def(outside_prepared.definitions[0], input_world);
  parameter_execution::initialize_parameter_definitions(
      outside_prepared.definitions, plan.width, plan.height);
  const bool outside_refused = !prepare_parameters(request, outside_prepared, hooks);
  smart::state().hosted_layers.clear();
  return outside_refused;
}

}  // namespace aexcompat::worker_runtime::smart_setup
