#include <windows.h>

#include "render_lifecycle.hpp"
#include "render_pixel_buffer.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "runtime_module_audit.hpp"
#include "worker_aegp_layer_render_runtime.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_classic_execution.hpp"
#include "worker_classic_runtime.hpp"
#include "worker_l2_render_abi.hpp"
#include "worker_param_checkout_runtime.hpp"
#include "worker_parameter_execution.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_request_parser.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_smart_execution.hpp"
#include "worker_smart_render_runtime.hpp"
#include "worker_smart_runtime.hpp"
#include "worker_smart_setup.hpp"
#include "worker_ui_event_execution.hpp"
#include "worker_world_registry.hpp"
#include "worker_suite_abi.hpp"
#include "worker_world_safety.hpp"

#include <array>
#include <iostream>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <new>
#include <string>
#include <vector>

// Classic and smart render runtimes moved from worker_main (issue #170).
// Worker-entry owned callbacks, snapshot wrappers, and probes stay in their
// owners and are reached cross-TU; every direct EffectMain selector call
// keeps the audited guarded_effect_call boundary through the macro below.
namespace aexcompat::l2_detail {

using namespace aexcompat::worker_runtime::parameter_execution;
using aexcompat::render_lifecycle::RenderLifecycle;
using aexcompat::render_safety::InputPixelBuffer;
using aexcompat::render_safety::OutputPixelBuffer;
using aexcompat::pf_ae_channel::activate_external_aux;
using aexcompat::pf_ae_channel::deactivate_external_aux;
using aexcompat::pf_ae_channel::clear_native_aux_provider;
using aexcompat::pf_ae_channel::publish_alpha_coverage_provider;
using aexcompat::render_pixel_transport::rgba8_to_argb;
using aexcompat::worker_runtime::guarded_effect_call;
using aexcompat::worker_runtime::invoke_entry_seh;
using aexcompat::worker_runtime::capture_module_audit;
using aexcompat::worker_runtime::module_audit_report;
using aexcompat::worker_runtime::parameters::ParamRecord;
using RequestedAssignments = aexcompat::worker_runtime::parameters::RequestedAssignments;
using aexcompat::world_registry::kPixelFormatArgb128;
using aexcompat::world_registry::kPixelFormatArgb32;
using aexcompat::world_registry::kPixelFormatArgb64;
using aexcompat::world_safety::DispatchWorldFormatScope;
using AegpTime = aexcompat::suite_abi::AegpTime;
using aexcompat::world_safety::kEffectWorldSize;
using ExternalLayerInput = aexcompat::worker_runtime::request_parser::LayerInput;
using LayerRenderContext = aexcompat::aegp_layer_render_runtime::Context;
using SmartRuntimeSession = aexcompat::worker_runtime::smart::Session;

// Cross-TU declarations into worker-entry owned helpers and callbacks; every
// definition stays with its owner.
int32_t invoke_sequence_selector(EffectEntry entry, int32_t selector, void* input,
                                 void* output, uint32_t* exception_code = nullptr);
bool dispatch_render_click(EffectEntry entry, std::array<std::byte, 408>& input,
                           std::array<std::byte, 408>& output,
                           std::vector<std::array<std::byte, 176>>& definitions);
bool dispatch_render_draw(EffectEntry entry, std::array<std::byte, 408>& input,
                          std::array<std::byte, 408>& output,
                          std::vector<std::array<std::byte, 176>>& definitions);
bool close_render_ui_context(EffectEntry entry, std::array<std::byte, 408>& input,
                             std::array<std::byte, 408>& output,
                             std::vector<std::array<std::byte, 176>>& definitions);
bool apply_parameter_animation(
    std::vector<std::array<std::byte, 176>>& definitions, int32_t time, uint32_t scale);
bool same_rational_time(int32_t left, uint32_t left_scale,
                        int32_t right, uint32_t right_scale);
void dump_world_snapshot(const std::string& stage, const unsigned char* packed_argb,
                         int32_t width, int32_t height, int32_t pixel_bytes);
std::string sha256_bytes(const unsigned char* data, std::size_t size);
void* aegp_comp_item_handle();
void reset_smart_host_telemetry();
int32_t __cdecl guid_mix_in_ptr(void* effect_ref, uint32_t size, const void* bytes);

// Private protocol constants mirrored from worker_main's frozen buffer layout.
namespace {
constexpr std::size_t kInSize = 408;
constexpr std::size_t kInEffectRef = 184;
constexpr std::size_t kOutSize = 408;
constexpr std::size_t kParamSize = 176;
constexpr std::size_t kInQuality = 192;
constexpr std::size_t kInCurrentTime = 224;
constexpr std::size_t kInTimeScale = 240;
constexpr std::size_t kInSequenceData = 320;
constexpr std::size_t kInFrameData = 328;
constexpr std::size_t kOutSequenceData = 56;
constexpr std::size_t kOutFrameData = 72;
constexpr std::size_t kOutWidth = 80;
constexpr std::size_t kOutHeight = 84;
constexpr std::size_t kOutOrigin = 88;
constexpr std::size_t kOutFlags = 96;
constexpr std::size_t kOutFlags2 = 400;
constexpr uint32_t kOutFlagWideTimeInput = 1u << 1;
constexpr uint32_t kOutFlagNopRender = 1u << 18;
constexpr uint32_t kOutFlagIWriteInputBuffer = 1u << 11;
constexpr uint32_t kOutFlagIUseShutterAngle = 1u << 19;
constexpr uint32_t kOutFlag2AutomaticWideTimeInput = 1u << 17;
constexpr uint32_t kOutFlag2SupportsSmartRender = 1u << 10;
constexpr int32_t kSequenceSetup = 5;
constexpr int32_t kSequenceSetdown = 8;
constexpr int32_t kFrameSetup = 10;
constexpr int32_t kFrameSetdown = 12;
constexpr int32_t kRender = 11;

auto& g_module_audit = module_audit_report();
auto& g_render_context_state = aexcompat::render::render_context_state();
auto& g_full_resolution_width = g_render_context_state.full_resolution_width;
auto& g_full_resolution_height = g_render_context_state.full_resolution_height;
auto& g_pixel_aspect_ratio = g_render_context_state.pixel_aspect_ratio;
auto& g_parameter_runtime = aexcompat::worker_runtime::parameters::state();
auto& g_params = g_parameter_runtime.records;
auto& g_custom_ui_telemetry =
    aexcompat::worker_runtime::ui_event_execution::custom_ui_telemetry();
auto& g_render_ui_context_active = g_custom_ui_telemetry.render_ui_context_active;
auto& g_host_callback_telemetry =
    aexcompat::worker_runtime::classic::host_callback_telemetry();
auto& g_secondary_layer_slot = g_host_callback_telemetry.secondary_layer_slot;

auto& smart_state() { return aexcompat::worker_runtime::smart::state(); }

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}
}  // namespace

// Keep every direct EffectMain selector call on the same audited boundary.
#define entry(...) guarded_effect_call(entry, __VA_ARGS__)

struct LifecycleContext { EffectEntry effect_entry; };

constexpr aexcompat::render_lifecycle::Layout kRenderLifecycleLayout{
    kInSequenceData, kOutSequenceData, kInFrameData, kOutFrameData,
    kSequenceSetup, kSequenceSetdown, kFrameSetup, kFrameSetdown};

int32_t lifecycle_invoke_frame(void* opaque, int32_t selector, void* input,
                               void* output, void** params, void* world) {
  const EffectEntry effect_entry = static_cast<LifecycleContext*>(opaque)->effect_entry;
  return guarded_effect_call(effect_entry, selector, input, output, params, world, nullptr);
}

int32_t lifecycle_invoke_sequence(void* opaque, int32_t selector, void* input,
                                  void* output) {
  return invoke_sequence_selector(
      static_cast<LifecycleContext*>(opaque)->effect_entry, selector, input, output);
}

void lifecycle_activate_aux(void*) {
  activate_external_aux();
}

void lifecycle_cleanup_aux(void*) {
  // Aux channel chunks are host-owned and cannot outlive a render lifecycle.
  aexcompat::pf_ae_channel::reclaim_layer_channels();
  deactivate_external_aux();
  clear_native_aux_provider();
}

aexcompat::render_lifecycle::Hooks lifecycle_hooks(LifecycleContext& context) {
  return {&context, &lifecycle_invoke_frame, &lifecycle_invoke_sequence,
          &lifecycle_activate_aux, &lifecycle_cleanup_aux};
}

RenderLifecycle begin_frame_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::begin_frame(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world);
}

int32_t end_frame_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::end_frame(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world, lifecycle, primary_error);
}

RenderLifecycle begin_render_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::begin_render(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world);
}

int32_t end_render_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::end_render(
      lifecycle_hooks(context), kRenderLifecycleLayout, input.data(), output.data(),
      params, world, lifecycle, primary_error);
}

bool dispatch_conditional_ui_selectors(EffectEntry entry,
                                       std::array<std::byte, 408>& input,
                                       std::array<std::byte, 408>& output,
                                       void** params);

// Owns the references that must remain alive from lifecycle begin through end.
// The opaque hook ABI never outlives this stack owner.
struct ClassicLifecycleOwner {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  std::vector<std::array<std::byte, kParamSize>>& definitions;
  std::vector<void*>& params;
  std::array<std::byte, kEffectWorldSize>& world;
  bool manage_sequence;

  aexcompat::worker_runtime::classic_execution::LifecycleResult begin() {
    return aexcompat::worker_runtime::classic_execution::begin_lifecycle(this, hooks());
  }
  int32_t finish(aexcompat::worker_runtime::classic_execution::LifecycleResult& state,
                 bool draw = false) {
    return aexcompat::worker_runtime::classic_execution::finish_lifecycle(
        this, state, hooks(), draw);
  }

 private:
  static const aexcompat::worker_runtime::classic_execution::LifecycleHooks& hooks() {
    static const aexcompat::worker_runtime::classic_execution::LifecycleHooks value{
        +[](void* opaque) -> void* {
          auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          const auto lifecycle = h.manage_sequence
              ? begin_render_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data())
              : begin_frame_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data());
          return new (std::nothrow) RenderLifecycle(lifecycle);
        },
        +[](void* lifecycle) {
          return static_cast<RenderLifecycle*>(lifecycle)->setup_error; },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return dispatch_render_click(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return interpolate_arbitrary_values(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return roundtrip_arbitrary_values(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return dispatch_conditional_ui_selectors(h.entry, h.input, h.output, h.params.data()); },
        +[](void* opaque) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return dispatch_render_draw(h.entry, h.input, h.output, h.definitions); },
        +[](void* opaque, void* lifecycle, int32_t error) { auto& h = *static_cast<ClassicLifecycleOwner*>(opaque);
          return h.manage_sequence
              ? end_render_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data(),
                    *static_cast<RenderLifecycle*>(lifecycle), error)
              : end_frame_lifecycle(h.entry, h.input, h.output, h.params.data(), h.world.data(),
                    *static_cast<RenderLifecycle*>(lifecycle), error); },
        +[](void* lifecycle) { delete static_cast<RenderLifecycle*>(lifecycle); }};
    return value;
  }
};

// Owns render-dispatch state. Buffer resize mutates all related world references
// atomically; LayerRenderContext is scoped strictly to the kRender callback.
struct ClassicRenderDispatchOwner {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  std::array<std::byte, kEffectWorldSize>& world;
  OutputPixelBuffer& guarded;
  DispatchWorldFormatScope& worlds;
  std::vector<std::array<std::byte, kParamSize>>& definitions;
  std::vector<void*>& params;
  int32_t& width; int32_t& height; int32_t& rowbytes;
  unsigned char*& destination;
  int32_t pixel_bytes; int32_t pixel_format;
  int32_t current_time; int32_t time_step; int32_t total_time; uint32_t time_scale;
  const std::string& case_id; const RequestedAssignments* requested;
  const std::vector<unsigned char>* external_rgba;
  const std::vector<ExternalLayerInput>* external_layers;
  int32_t external_width; int32_t external_height;
  aexcompat::worker_runtime::classic::Context& classic_context;
  std::vector<unsigned char>& logical_source;
  // When set, records that the host itself rejected or failed the plug-in's
  // requested output resize, so callers can tell host-side output validation
  // failures apart from selector errors sharing the same numeric codes.
  bool* output_validation_failed{};

  int32_t run(int32_t error) {
    return aexcompat::worker_runtime::classic_execution::dispatch_render(this, error, hooks());
  }

 private:
  static const aexcompat::worker_runtime::classic_execution::RenderHooks& hooks() {
    static const aexcompat::worker_runtime::classic_execution::RenderHooks value{
        +[](void* opaque) { auto& h = *static_cast<ClassicRenderDispatchOwner*>(opaque);
          return dispatch_render_draw(h.entry, h.input, h.output, h.definitions); },
        // The stage markers sit inside the hooks, not around
        // classic_execution::dispatch_render, so each one brackets only what it
        // actually names. dispatch_render short-circuits on a non-zero incoming
        // error and runs prepare_output before the selector, so a bracket around
        // the whole call would file a frame that never reached RENDER - or one
        // the host itself refused - under the plug-in's selector (issue #722).
        //
        // prepare_output is entirely host-side: it validates the plug-in's
        // requested output resize and re-lays the output world. It never calls
        // the plug-in, so its refusals get their own name rather than the
        // selector's. It emits no `_begin` because there is no foreign code
        // inside it for a crash to be attributed to.
        //
        // Deliberately not the broker's "output_validation": that override
        // (image_render/session.rs) keys on `output_pixels_valid`, which only
        // the smart report emits (worker_smart_report.cpp) and which means the
        // output pixels came back empty, still at the 0xCC fill, or non-finite.
        // A different condition, so a different name.
        +[](void* opaque) {
          const int32_t error = static_cast<ClassicRenderDispatchOwner*>(opaque)->prepare_output();
          if (error != 0)
            std::cerr << "stage:classic_output_resize_end error=" << error << "\n" << std::flush;
          return error; },
        // The plug-in's RENDER. The unbalanced `_begin` left by a crash or hang
        // in here is what lets active_stage name this frame's selector.
        +[](void* opaque) {
          std::cerr << "stage:classic_render_begin\n" << std::flush;
          const int32_t error = static_cast<ClassicRenderDispatchOwner*>(opaque)->dispatch_selector();
          std::cerr << "stage:classic_render_end error=" << error << "\n" << std::flush;
          return error; },
        +[](void* opaque) { auto& h = *static_cast<ClassicRenderDispatchOwner*>(opaque);
          return !g_render_ui_context_active || close_render_ui_context(h.entry, h.input, h.output, h.definitions); }};
    return value;
  }
  int32_t prepare_output() {
    const auto fail = [this](int32_t error) {
      if (output_validation_failed) *output_validation_failed = true;
      return error;
    };
    const int32_t next_width = read<int32_t>(output, kOutWidth);
    const int32_t next_height = read<int32_t>(output, kOutHeight);
    if (!aexcompat::render::validate_output_extent(width, height, next_width, next_height,
            read<uint32_t>(output, kOutFlags))) return fail(4);
    if (next_width <= 0 || next_height <= 0) return 0;
    width = next_width; height = next_height; rowbytes = width * pixel_bytes;
    if (!guarded.reset(static_cast<std::size_t>(rowbytes) * height)) return fail(-3);
    destination = guarded.data();
    if (!aexcompat::render::prepare_world_layout(world,
            {pixel_bytes == 4 ? 0 : 1, pixel_bytes, width, height, rowbytes}, destination)) return fail(-3);
    if (!worlds.register_world(world.data(), pixel_format)) return fail(4);
    write<int32_t>(input, 276, read<int32_t>(output, kOutOrigin));
    write<int32_t>(input, 280, read<int32_t>(output, kOutOrigin + 4));
    return 0;
  }
  int32_t dispatch_selector() {
    struct LayerContextScope {
      LayerRenderContext previous;
      explicit LayerContextScope(LayerRenderContext next)
          : previous(aexcompat::aegp_layer_render_runtime::replace_context(std::move(next))) {}
      ~LayerContextScope() {
        aexcompat::aegp_layer_render_runtime::replace_context(std::move(previous));
      }
    };
    LayerRenderContext next{
        entry, &input, &output, current_time, static_cast<int32_t>(time_scale), case_id,
        requested, external_rgba, external_layers, external_width, external_height,
        time_step, total_time, pixel_bytes, &logical_source, width, height};
    void* render_ref = nullptr;
    std::memcpy(&render_ref, input.data() + kInEffectRef, sizeof(render_ref));
    next.active_effect_instance =
        staged_effect_identity_for_render_ref(render_ref);
    next.project_generation =
        aexcompat::aegp_external_render_runtime::project_generation();
    LayerContextScope scope(std::move(next));
    classic_context.mark_selector_dispatched();
    return entry(kRender, input.data(), output.data(), params.data(), world.data(), nullptr);
  }
};
int32_t classic_render_runtime(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& command_output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes,
                    std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4,
                    bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr,
                    bool* output_validation_failed = nullptr) {
  auto* classic_context = aexcompat::worker_runtime::classic::active_context();
  if (!classic_context) return -1;
  aexcompat::render::ImageRequest image_request;
  const int request_error = aexcompat::render::prepare_image_request(
      case_id, external_rgba != nullptr, external_width, external_height,
      external_pixel_bytes, image_request);
  if (request_error != 0) return request_error;
  const bool connected_map = image_request.connected_map;
  const bool partial_extent_hint = image_request.partial_extent_hint;
  width = image_request.width;
  height = image_request.height;
  const int32_t pixel_bytes = image_request.pixel_bytes;
  smart_state().pixel_format = pixel_bytes == 16 ? "argb32f" :
      (pixel_bytes == 8 ? "argb16" : "argb8");
  rowbytes = image_request.rowbytes;
  const aexcompat::render::ParameterProfile parameter_profile =
      aexcompat::render::prepare_parameter_profile(case_id);
  std::vector<unsigned char> logical_source(width * height * pixel_bytes);
  InputPixelBuffer source(static_cast<std::size_t>(rowbytes) * height);
  if (!source) return -3;
  std::memset(source.data(), 0x5A, static_cast<std::size_t>(rowbytes) * height);
  if (!aexcompat::render::build_argb_input(image_request, external_rgba,
                                            logical_source, source.data())) return -3;
  dump_world_snapshot("classic-input", logical_source.data(), width, height, pixel_bytes);
  const bool input_write_advertised =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagIWriteInputBuffer) != 0;
  if (!source.set_plugin_writable(input_write_advertised)) return -3;
  OutputPixelBuffer guarded(static_cast<std::size_t>(rowbytes) * height);
  if (!guarded) return -3;
  unsigned char* destination = guarded.data();
  guards_intact = true;

  std::array<std::byte, 120> input_world{}, output_world{};
  const aexcompat::render::WorldLayout primary_world{
      pixel_bytes == 4 ? 0 : 1, pixel_bytes, width, height, rowbytes};
  if (!aexcompat::render::prepare_world_layout(input_world, primary_world, source.data()) ||
      !aexcompat::render::prepare_world_layout(output_world, primary_world, destination)) return -3;
  const int32_t dispatch_pixel_format = pixel_bytes == 4 ? kPixelFormatArgb32 :
      (pixel_bytes == 8 ? kPixelFormatArgb64 : kPixelFormatArgb128);
  DispatchWorldFormatScope dispatch_worlds;
  if (!dispatch_worlds.register_world(input_world.data(), dispatch_pixel_format) ||
      !dispatch_worlds.register_world(output_world.data(), dispatch_pixel_format)) return -3;

  aexcompat::render::MapWorld map_world;
  if (connected_map) {
    if (!aexcompat::render::prepare_connected_map_world(case_id, width, height, map_world) ||
        !dispatch_worlds.register_world(map_world.world.data(), kPixelFormatArgb32)) return -3;
    aexcompat::worker_runtime::classic::ParameterDefinition checkout_definition{};
    write<int32_t>(checkout_definition, 12, 0);
    std::memcpy(checkout_definition.data() + 56, map_world.world.data(), map_world.world.size());
    classic_context->set_fallback_definition(g_secondary_layer_slot,
                                             checkout_definition);
  }

  std::vector<std::array<std::byte, kParamSize>> definitions(g_params.size() + 1);
  std::vector<std::vector<unsigned char>> hosted_pixels;
  std::vector<std::array<std::byte, 120>> hosted_worlds;
  if (external_layers) {
    hosted_pixels.resize(external_layers->size());
    hosted_worlds.resize(external_layers->size());
  }
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (!initialize_arbitrary_values(entry, input, command_output, definitions)) return -5;
  ArbitraryValuesScope arbitrary_scope{entry, &input, &command_output, &definitions};
  if (requested && !apply_arbitrary_text_assignments(entry, input, command_output, definitions, *requested)) return -5;
  probe_arbitrary_scan(entry, input, command_output, definitions);
  for (std::size_t slot = 1; slot < definitions.size(); ++slot)
    if (g_params[slot - 1].type == 0 && g_params[slot - 1].layer_default == -1)
      std::memcpy(definitions[slot].data() + 56, input_world.data(), input_world.size());
  if (external_layers) for (std::size_t layer_index = 0; layer_index < external_layers->size(); ++layer_index) {
    const auto& layer = (*external_layers)[layer_index];
    if (layer.slot <= 0 || static_cast<std::size_t>(layer.slot) >= definitions.size() ||
        g_params[layer.slot - 1].type != 0 || layer.rgba.size() !=
            static_cast<std::size_t>(layer.width) * layer.height * 4) return -3;
    auto& pixels = hosted_pixels[layer_index];
      pixels.resize(static_cast<std::size_t>(layer.width) * layer.height * pixel_bytes);
      for (std::size_t offset = 0; offset < layer.rgba.size(); offset += 4) {
      rgba8_to_argb(pixels.data() + (offset / 4) * pixel_bytes,
                    layer.rgba.data() + offset, pixel_bytes);
    }
    dump_world_snapshot("classic-layer-slot" + std::to_string(layer.slot),
                        pixels.data(), layer.width, layer.height, pixel_bytes);
    auto& world = hosted_worlds[layer_index];
    if (!aexcompat::render::prepare_world_layout(
            world, {pixel_bytes == 4 ? 0 : 1, pixel_bytes, layer.width, layer.height,
                    layer.width * pixel_bytes}, pixels.data()) ||
        !dispatch_worlds.register_world(world.data(), dispatch_pixel_format)) return -3;
    if (!layer.timed || same_rational_time(layer.time, layer.time_scale,
            external_current_time, external_time_scale))
      std::memcpy(definitions[layer.slot].data() + 56, world.data(), world.size());
    std::array<std::byte, kParamSize> checkout{};
    write<int32_t>(checkout, 12, 0);
    std::memcpy(checkout.data() + 56, world.data(), world.size());
    if (layer.timed) {
      if (!classic_context->add_timed_layer(
              {layer.slot, layer.time, layer.time_scale, checkout})) return -3;
    }
  }
  if (requested) {
    if (!apply_requested_assignments(definitions, *requested)) return -3;
  } else if (definitions.size() > 7) {
    write<int32_t>(definitions[1], 56, parameter_profile.amount);
    write<int32_t>(definitions[2], 56, parameter_profile.direction);
    write<int32_t>(definitions[3], 56, parameter_profile.seed);
    write<int32_t>(definitions[4], 56, parameter_profile.repeat);
    write<double>(definitions[5], 56, parameter_profile.mix);
    if (parameter_profile.inverted_map) write<int32_t>(definitions[7], 56, 1);
  }
  if (!apply_parameter_animation(definitions, external_current_time, external_time_scale)) return -3;
  if (!apply_arbitrary_parameter_animation(entry, input, command_output, definitions,
                                            external_current_time, external_time_scale)) return -3;
  for (std::size_t slot = 0; slot < definitions.size(); ++slot)
    classic_context->set_definition(static_cast<int32_t>(slot), definitions[slot]);
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i) params[i] = definitions[i].data();
  write<int32_t>(input, 224, external_current_time);
  write<int32_t>(input, 228, external_time_step);
  write<int32_t>(input, 232, external_total_time);
  write<int32_t>(input, 236, external_time_step);
  write<uint32_t>(input, 240, external_time_scale);
  write<int32_t>(input, 252, g_full_resolution_width > 0 ? g_full_resolution_width : width);
  write<int32_t>(input, 256, g_full_resolution_height > 0 ? g_full_resolution_height : height);
  const int32_t full_extent[4] = {0, 0, width, height};
  std::memcpy(input.data() + 260, full_extent, sizeof(full_extent));
  if (partial_extent_hint) {
    const int32_t extent[4] = {3, 2, 11, 8};
    std::memcpy(input.data() + 260, extent, sizeof(extent));
  }
  struct RenderUiContextScope {
    EffectEntry entry;
    std::array<std::byte, kInSize>& input;
    std::array<std::byte, kOutSize>& output;
    std::vector<std::array<std::byte, kParamSize>>& definitions;
    ~RenderUiContextScope() {
      if (g_render_ui_context_active)
        close_render_ui_context(entry, input, output, definitions);
    }
  } render_ui_context_scope{entry, input, command_output, definitions};
  publish_alpha_coverage_provider(logical_source, width, height, pixel_bytes,
                                  external_current_time, external_time_scale);
  ClassicLifecycleOwner lifecycle_owner{entry, input, command_output, definitions, params,
                                        output_world, manage_sequence};
  auto lifecycle = lifecycle_owner.begin();
  int32_t error = lifecycle.error;
  const uint32_t effective_out_flags = read<uint32_t>(command_output, kOutFlags);
  const uint32_t effective_out_flags2 = read<uint32_t>(command_output, kOutFlags2);
  const bool classic_wide_time_allowed =
      (effective_out_flags & kOutFlagWideTimeInput) != 0 ||
      ((effective_out_flags2 & kOutFlag2AutomaticWideTimeInput) != 0 &&
       (effective_out_flags2 & kOutFlag2SupportsSmartRender) == 0);
  const bool classic_shutter_dependency_advertised =
      (effective_out_flags & kOutFlagIUseShutterAngle) != 0;
  classic_context->configure_checkout_time(
      read<int32_t>(input, kInCurrentTime), read<uint32_t>(input, kInTimeScale),
      classic_wide_time_allowed, classic_shutter_dependency_advertised);
  input_hash = sha256_bytes(logical_source.data(), logical_source.size());
  const bool nop_render =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagNopRender) != 0;
  if (nop_render) {
    if (error == 0) {
      for (int32_t y = 0; y < height; ++y)
        std::memcpy(destination + y * rowbytes,
                    logical_source.data() + y * width * pixel_bytes,
                    width * pixel_bytes);
    }
    error = lifecycle_owner.finish(lifecycle);
  } else {
    ClassicRenderDispatchOwner dispatch_owner{entry, input, command_output, output_world,
        guarded, dispatch_worlds, definitions, params, width, height, rowbytes, destination,
        pixel_bytes, dispatch_pixel_format, external_current_time, external_time_step,
        external_total_time, external_time_scale, case_id, requested, external_rgba,
        external_layers, external_width, external_height, *classic_context, logical_source,
        output_validation_failed};
    // The `stage:classic_render_*` and `stage:classic_output_resize_end` markers are
    // emitted per frame from inside RenderHooks (see ClassicRenderDispatchOwner)
    // so each brackets only the step it names. The session-wide `stage:render_*`
    // pair in worker_invocation_orchestration.cpp is emitted once, so before
    // this a classic session's frame errors carried no stage at all and every
    // one of them came back with `first_failure_stage: null` (issue #722).
    error = dispatch_owner.run(error);
    lifecycle.error = error;
    error = lifecycle_owner.finish(lifecycle);
  }
  // Everything the plug-in itself got a say in (RENDER plus the setdown half of
  // the frame lifecycle), before the host's own finalize can add to it. Kept so
  // the two can be told apart: a frame that fails only past this point failed in
  // the host, not in the plug-in (issue #722).
  const int32_t selector_error = error;
  aexcompat::worker_runtime::classic_execution::Context final_context{
      destination, rowbytes, width, height, pixel_bytes, error,
      external_current_time, external_time_step, external_time_scale,
      read<int32_t>(input, kInQuality), dispatch_pixel_format, &output_hash,
      &guards_intact, captured_argb, guarded.sentinels_intact()};
  error = aexcompat::worker_runtime::classic_execution::finalize(final_context, {
      +[](const unsigned char* data, int32_t rowbytes, int32_t width, int32_t height,
          int32_t bytes, std::vector<unsigned char>& output) {
        return aexcompat::render::copy_packed_world(data, rowbytes, width, height, bytes, output);
      }, &sha256_bytes,
      +[](AegpTime time, AegpTime step, int8_t quality, int32_t format,
          int32_t width, int32_t height, const void* pixels) {
        return aexcompat::aegp_staged_item_runtime::publish_world(aegp_comp_item_handle(),
            time, step, quality, 0, format, width, height,
            width * (format == kPixelFormatArgb32 ? 4 :
                (format == kPixelFormatArgb64 ? 8 : 16)), pixels);
      },
      +[](const void* pixels, int32_t width, int32_t height, int32_t bytes) {
        dump_world_snapshot("classic-output",
            static_cast<const unsigned char*>(pixels), width, height, bytes);
      },
      +[](const char* format) { smart_state().pixel_format = format; }});
  if (error != selector_error)
    std::cerr << "stage:classic_finalize_end error=" << error << "\n" << std::flush;
  // The render dispatch hook (ClassicRenderDispatchOwner, RenderHooks) already
  // closes the UI context when it is active, so only close here if it is still
  // open. Without the g_render_ui_context_active guard this re-closes an
  // already-closed context: close_render_ui_context early-returns false for a
  // click/draw request whose context is inactive, spuriously turning a clean
  // custom-UI render into render_error -5 (issue #259). The smart path guards
  // this the same way.
  if (g_render_ui_context_active &&
      !close_render_ui_context(entry, input, command_output, definitions) &&
      error == 0)
    error = -5;
  return error;
}

// The request keeps render-local state out of wmain.  The shared subsystem
// controls admission and failure priority; this hook retains the audited host
// implementation that prepares PF worlds, params, suites, and lifecycle data.
struct ClassicRenderRequest {
  EffectEntry entry;
  std::array<std::byte, kInSize>& input;
  std::array<std::byte, kOutSize>& output;
  const std::string& case_id;
  int32_t& width;
  int32_t& height;
  int32_t& rowbytes;
  std::string& input_hash;
  std::string& output_hash;
  bool& guards_intact;
  const RequestedAssignments* requested;
  const std::vector<unsigned char>* external_rgba;
  int32_t external_width;
  int32_t external_height;
  const std::vector<ExternalLayerInput>* external_layers;
  int32_t external_current_time;
  int32_t external_time_step;
  int32_t external_total_time;
  uint32_t external_time_scale;
  int32_t external_pixel_bytes;
  bool manage_sequence;
  std::vector<unsigned char>* captured_argb;
  bool* output_validation_failed;
};

bool classic_render_dependencies_ready(void* opaque) {
  const auto& request = *static_cast<ClassicRenderRequest*>(opaque);
  return request.entry && request.width >= 0 && request.height >= 0 &&
      request.external_time_scale != 0;
}

int classic_render_guarded_effect_main(void* opaque) {
  auto& request = *static_cast<ClassicRenderRequest*>(opaque);
  return classic_render_runtime(request.entry, request.input, request.output, request.case_id,
      request.width, request.height, request.rowbytes, request.input_hash, request.output_hash,
      request.guards_intact, request.requested, request.external_rgba,
      request.external_width, request.external_height, request.external_layers,
      request.external_current_time, request.external_time_step, request.external_total_time,
      request.external_time_scale, request.external_pixel_bytes, request.manage_sequence,
      request.captured_argb, request.output_validation_failed);
}

int classic_render_cleanup(void*) {
  // classic_render_runtime performs sequence/frame/UI/world cleanup before it
  // returns.  This explicit hook documents the completed cleanup boundary.
  return 0;
}

int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes, std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested = nullptr,
                    const std::vector<unsigned char>* external_rgba = nullptr,
                    int32_t external_width = 0, int32_t external_height = 0,
                    const std::vector<ExternalLayerInput>* external_layers = nullptr,
                    int32_t external_current_time = 0, int32_t external_time_step = 1,
                    int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                    int32_t external_pixel_bytes = 4, bool manage_sequence = true,
                    std::vector<unsigned char>* captured_argb = nullptr,
                    bool* output_validation_failed = nullptr) {
  ClassicRenderRequest request{entry, input, output, case_id, width, height, rowbytes,
      input_hash, output_hash, guards_intact, requested, external_rgba,
      external_width, external_height, external_layers, external_current_time,
      external_time_step, external_total_time, external_time_scale, external_pixel_bytes,
      manage_sequence, captured_argb, output_validation_failed};
  aexcompat::worker_runtime::classic::Request context{
      &request,
      {&classic_render_guarded_effect_main, &classic_render_cleanup,
       &classic_render_dependencies_ready},
      g_module_audit.required};
  return aexcompat::worker_runtime::classic::dispatch(context);
}

using SmartResult = aexcompat::worker_runtime::smart_execution::Result;

SmartResult smart_render_runtime(EffectEntry entry, std::array<std::byte, kInSize>& input,
                              std::array<std::byte, kOutSize>& command_output,
                              const std::string& case_id,
                              const RequestedAssignments* requested = nullptr,
                              const std::vector<unsigned char>* external_rgba = nullptr,
                              int32_t external_width = 0, int32_t external_height = 0,
                              const std::vector<ExternalLayerInput>* external_layers = nullptr,
                              int32_t external_current_time = 0, int32_t external_time_step = 1,
                              int32_t external_total_time = 1, uint32_t external_time_scale = 1,
                              int32_t external_pixel_bytes = 4,
                              aexcompat::worker_runtime::smart_execution::SessionFrame* session =
                                  nullptr) {
  SmartRuntimeSession smart_session;
  reset_smart_host_telemetry();
  SmartResult result;
  result.runtime = smart_session.snapshot();
  const auto plan = aexcompat::worker_runtime::smart_setup::prepare(
      {g_secondary_layer_slot, g_full_resolution_width, g_full_resolution_height,
       g_pixel_aspect_ratio.numerator, g_pixel_aspect_ratio.denominator},
      {&command_output, &case_id, external_rgba != nullptr, external_width,
       external_height, external_current_time, external_time_scale,
       external_pixel_bytes});
  if (!plan.valid) return result;
  const bool deep16 = plan.deep16;
  const bool fixture_gpu_negotiation = plan.fixture_gpu_negotiation;
  const bool opencl_gpu_negotiation = plan.opencl_gpu_negotiation;
  const bool directx_gpu_negotiation = plan.directx_gpu_negotiation;
  const bool explicit_gpu_device = plan.explicit_gpu_device;
  const uint32_t gpu_device_index = plan.gpu_device_index;
  const bool gpu_negotiation = plan.gpu_negotiation;
  const bool missing_input = plan.missing_input;
  const bool temporal_context = plan.temporal_context;
  const bool partial_output_request = plan.partial_output_request;
  const bool float32 = plan.float32;
  const bool connected_map = plan.connected_map;
  const int32_t width = plan.width;
  const int32_t height = plan.height;
  const int32_t pixel_bytes = plan.pixel_bytes;
  const int32_t rowbytes = plan.rowbytes;
  InputPixelBuffer source(static_cast<std::size_t>(rowbytes) * height);
  OutputPixelBuffer guarded(static_cast<std::size_t>(rowbytes) * height);
  auto* destination = guarded.data();
  result.guards_intact = true;
  // A session frame needs the sentinel verdict on every exit path, including
  // early refusals after this point; the one-shot report keeps its existing
  // finalize-only semantics, so this probe writes to the session record, not
  // to result.guards_intact.
  struct SessionGuardProbe {
    aexcompat::worker_runtime::smart_execution::SessionFrame* session;
    OutputPixelBuffer& guarded;
    ~SessionGuardProbe() {
      if (session) session->guards_intact = guarded.sentinels_intact();
    }
  } session_guard_probe{session, guarded};
  if (session) session->output_buffer_allocated = true;
  std::array<std::byte, 120> input_world{}, output_world{};
  std::array<std::byte, 120> input_checkout_view{}, map_checkout_view{};
  DispatchWorldFormatScope dispatch_worlds;
  aexcompat::render::MapWorld map_world;
  const bool input_write_advertised =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagIWriteInputBuffer) != 0;
  if (!aexcompat::worker_runtime::smart_setup::prepare_world_buffers(
          plan, case_id, external_rgba, input_write_advertised,
          {&source, &guarded, &destination, &input_world, &output_world,
           &input_checkout_view, &map_checkout_view, &dispatch_worlds,
           &map_world})) return result;
  const int32_t dispatch_pixel_format = float32 ? kPixelFormatArgb128 :
      (deep16 ? kPixelFormatArgb64 : kPixelFormatArgb32);
  aexcompat::worker_runtime::smart_setup::ParameterState parameter_state(
      g_params.size() + 1, external_layers ? external_layers->size() : 0);
  auto& definitions = parameter_state.definitions;
  std::memcpy(definitions[0].data() + 56, input_world.data(), input_world.size());
  initialize_parameter_definitions(definitions);
  if (!initialize_arbitrary_values(entry, input, command_output, definitions)) return result;
  ArbitraryValuesScope arbitrary_scope{entry, &input, &command_output, &definitions};
  if (!aexcompat::worker_runtime::smart_setup::prepare_parameters(
          {entry, &input, &command_output, &case_id, &plan, requested,
           external_layers, external_current_time, external_time_step,
           external_total_time, external_time_scale, g_full_resolution_width,
           g_full_resolution_height, dispatch_pixel_format, &input_world,
           &dispatch_worlds, &source},
          parameter_state, {&apply_parameter_animation, &dump_world_snapshot}))
    return result;
  auto& params = parameter_state.params;
  auto& pre_render_source = parameter_state.pre_render_source;
  struct SmartRenderUiContextScope {
    EffectEntry entry;
    std::array<std::byte, kInSize>& input;
    std::array<std::byte, kOutSize>& output;
    std::vector<std::array<std::byte, kParamSize>>& definitions;
    ~SmartRenderUiContextScope() {
      if (g_render_ui_context_active)
        close_render_ui_context(entry, input, output, definitions);
    }
  } render_ui_context_scope{entry, input, command_output, definitions};
  // This immutable provider is published before Smart Pre-Render and remains pinned
  // through Smart Render; output pixels are never used to infer auxiliary planes.
  publish_alpha_coverage_provider(pre_render_source, width, height, pixel_bytes,
                                  external_current_time, external_time_scale);
  // Session frames run under a hoisted SEQUENCE owned by the session loop
  // (protocol v1.1): only the FRAME pair is managed here, mirroring the
  // classic render_once(manage_sequence=false) boundary.
  const auto begin_lifecycle = session ? &begin_frame_lifecycle : &begin_render_lifecycle;
  const auto end_lifecycle = session ? &end_frame_lifecycle : &end_render_lifecycle;
  const RenderLifecycle lifecycle = begin_lifecycle(
      entry, input, command_output, params.data(), output_world.data());
  if (lifecycle.setup_error != 0) {
    result.pre_error = lifecycle.setup_error;
    result.render_error = end_lifecycle(entry, input, command_output, params.data(),
                                        output_world.data(), lifecycle,
                                        lifecycle.setup_error);
    return result;
  }
  if (!dispatch_render_click(entry, input, command_output, definitions)) {
    result.pre_error = -5;
    result.render_error = end_lifecycle(entry, input, command_output, params.data(),
                                        output_world.data(), lifecycle, -5);
    return result;
  }
  if (!interpolate_arbitrary_values(entry, input, command_output, definitions) ||
      !roundtrip_arbitrary_values(entry, input, command_output, definitions)) {
    result.pre_error = -5;
    result.render_error = end_lifecycle(entry, input, command_output, params.data(),
                                        output_world.data(), lifecycle, -5);
    return result;
  }
  if (!dispatch_conditional_ui_selectors(entry, input, command_output, params.data())) {
    result.pre_error = -5;
    result.render_error = end_lifecycle(entry, input, command_output, params.data(),
                                        output_world.data(), lifecycle, -5);
    return result;
  }
  const uint32_t dynamic_out_flags = read<uint32_t>(command_output, kOutFlags);
  const uint32_t dynamic_out_flags2 = read<uint32_t>(command_output, kOutFlags2);
  smart_state().wide_time_checkout_allowed =
      (dynamic_out_flags & kOutFlagWideTimeInput) != 0 ||
      (dynamic_out_flags2 & kOutFlag2AutomaticWideTimeInput) != 0;
  smart_state().shutter_dependency_advertised =
      (dynamic_out_flags & kOutFlagIUseShutterAngle) != 0;
  // The smart path serves parameter checkouts from the hosted ledger, not from a
  // classic dispatch context, and that ledger kept its default current_time 0 /
  // time_scale 1 because nothing ever set it. `checkout_param` refuses any other
  // time, so every SmartFX frame past t=0 had its first checkout answered with
  // PF_Err_OUT_OF_MEMORY and the plug-in gave up - AviUtl2 renders at the cursor,
  // so no smart effect worked anywhere but frame 0 (issue #828). The classic path
  // has always configured the equivalent state on its context.
  aexcompat::l2_detail::configure_hosted_checkout_time(
      external_current_time, external_time_scale,
      smart_state().wide_time_checkout_allowed);
  const bool nop_render =
      (read<uint32_t>(command_output, kOutFlags) & kOutFlagNopRender) != 0;
  if (nop_render) {
    for (int32_t y = 0; y < height; ++y)
      std::memcpy(destination + y * rowbytes, source.data() + y * rowbytes,
                  width * pixel_bytes);
    result.pre_error = 0;
    result.render_error = end_lifecycle(entry, input, command_output, params.data(),
                                        output_world.data(), lifecycle, 0);
    result.rects_valid = true;
    result.roi_contract_valid = true;
    result.output_width = width;
    result.output_height = height;
    result.output_rowbytes = rowbytes;
    result.result_rect = {0, 0, width, height};
    result.max_result_rect = result.result_rect;
    result.output_extent_hint = result.result_rect;
    std::vector<unsigned char> logical_input(width * height * pixel_bytes);
    std::vector<unsigned char> logical_output(width * height * pixel_bytes);
    for (int32_t y = 0; y < height; ++y) {
      std::memcpy(logical_input.data() + y * width * pixel_bytes,
                  source.data() + y * rowbytes, width * pixel_bytes);
      std::memcpy(logical_output.data() + y * width * pixel_bytes,
                  destination + y * rowbytes, width * pixel_bytes);
    }
    result.input_hash = sha256_bytes(logical_input.data(), logical_input.size());
    result.output_hash = sha256_bytes(logical_output.data(), logical_output.size());
    dump_world_snapshot("smart-output", logical_output.data(), width, height, pixel_bytes);
    if (session && session->captured_argb) *session->captured_argb = logical_output;
    result.guards_intact = guarded.sentinels_intact();
    if (g_render_ui_context_active &&
        !close_render_ui_context(entry, input, command_output, definitions) &&
        result.render_error == 0)
      result.render_error = -5;
    return result;
  }
  if (!aexcompat::worker_runtime::smart_render_runtime::execute(
          {entry, &input, &command_output, &plan, &parameter_state, &input_world,
           &output_world, &dispatch_worlds, &source, &guarded, &destination,
           &lifecycle, dispatch_pixel_format, width, height,
           rowbytes, pixel_bytes, session},
          {&dispatch_render_draw,
           {&guarded_effect_call, &capture_module_audit,
            reinterpret_cast<void*>(&guid_mix_in_ptr),
            &automatic_checkin_pre_render_params},
           {&close_render_ui_context, end_lifecycle, &dump_world_snapshot,
            &sha256_bytes,
            +[] { return g_render_ui_context_active; }}}, result))
    return result;
  return result;
}

const bool g_smart_execution_configured =
    aexcompat::worker_runtime::smart_execution::configure({
        &smart_render_runtime, +[] { return g_module_audit.required; }});

SmartResult smart_render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                              std::array<std::byte, kOutSize>& output,
                              const std::string& case_id,
                              const RequestedAssignments* requested = nullptr,
                              const std::vector<unsigned char>* external_rgba = nullptr,
                              int32_t external_width = 0, int32_t external_height = 0,
                              const std::vector<ExternalLayerInput>* external_layers = nullptr,
                              int32_t external_current_time = 0, int32_t external_time_step = 1,
                              int32_t external_total_time = 1,
                              uint32_t external_time_scale = 1,
                              int32_t external_pixel_bytes = 4,
                              aexcompat::worker_runtime::smart_execution::SessionFrame* session =
                                  nullptr) {
  return aexcompat::worker_runtime::smart_execution::render_once(
      entry, input, output, case_id, requested, external_rgba,
      external_width, external_height, external_layers, external_current_time,
      external_time_step, external_total_time, external_time_scale,
      external_pixel_bytes, session);
}

#undef entry

}  // namespace aexcompat::l2_detail
