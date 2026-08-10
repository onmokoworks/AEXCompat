#include <windows.h>

#include "generated/aex_abi_contract.hpp"
#include "render_lifecycle.hpp"
#include "render_pixel_buffer.hpp"
#include "render_pixel_transport.hpp"
#include "render_subsystem.h"
#include "host_audio_runtime.hpp"
#include "runtime_module_audit.hpp"
#include "worker_aegp_layer_render_runtime.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_classic_execution.hpp"
#include "worker_classic_render_entry.hpp"
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

#include <algorithm>
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
// PF_InData::output_origin_x/y from the generated contract, so a regeneration
// that moves the field moves this use with it. The smart route writes the same
// field through the same contract constants (worker_smart_dispatch.cpp).
constexpr std::size_t kInOutputOriginX =
    aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_X_OFFSET;
constexpr std::size_t kInOutputOriginY =
    aexcompat::abi::x86_64_windows::IN_OUTPUT_ORIGIN_Y_OFFSET;
// The PF_OutData offsets this file writes through are in the contract too, and
// `offer_output_extent` memcpys into three of them before FRAME_SETUP. A
// regeneration that moved PF_OutData::width would otherwise leave the offer
// writing an extent over whatever field took its place - a sequence handle, say
// - and the plug-in dereferencing it, with nothing having failed to build.
static_assert(kOutWidth == aexcompat::abi::x86_64_windows::OUT_WIDTH_OFFSET);
static_assert(kOutHeight == aexcompat::abi::x86_64_windows::OUT_HEIGHT_OFFSET);
static_assert(kOutOrigin == aexcompat::abi::x86_64_windows::OUT_ORIGIN_OFFSET);
static_assert(kOutFlags == aexcompat::abi::x86_64_windows::OUT_OUT_FLAGS_OFFSET);
// `offer_output_extent` clears both halves of the in_data origin with one
// 8-byte store from `Layout::in_origin`, and `PF_OutData::origin` is read as a
// pair the same way, so the two fields being adjacent is load-bearing in the
// binary that performs the writes - not only in the self-test. A regeneration
// that inserted a reserved slot between them would otherwise have the clear
// zeroing whatever now follows output_origin_x before every FRAME_SETUP.
static_assert(kInOutputOriginY == kInOutputOriginX + 4);
constexpr std::size_t kInExtentHint =
    aexcompat::abi::x86_64_windows::IN_EXTENT_HINT_OFFSET;
static_assert(aexcompat::abi::x86_64_windows::IN_EXTENT_HINT_SIZE == 4 * sizeof(int32_t));
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

// The classic layout offers FRAME_SETUP the output world's extent to revise
// (issue #984); `prepare_output` below is what reads the answer back.
//
// Filled by name rather than positionally: Layout is a 14-member aggregate of
// which 13 are std::size_t, so a field inserted into it compiles cleanly with
// positional init and shifts every later offset one role along - the host would
// memcpy an extent into whatever PF_OutData field the shifted offset lands on.
constexpr aexcompat::render_lifecycle::Layout make_render_lifecycle_layout() {
  aexcompat::render_lifecycle::Layout layout{};
  layout.in_sequence_data = kInSequenceData;
  layout.out_sequence_data = kOutSequenceData;
  layout.in_frame_data = kInFrameData;
  layout.out_frame_data = kOutFrameData;
  layout.sequence_setup = kSequenceSetup;
  layout.sequence_setdown = kSequenceSetdown;
  layout.frame_setup = kFrameSetup;
  layout.frame_setdown = kFrameSetdown;
  layout.out_width = kOutWidth;
  layout.out_height = kOutHeight;
  layout.out_origin = kOutOrigin;
  layout.in_origin = kInOutputOriginX;
  layout.world_width = aexcompat::abi::x86_64_windows::LAYER_WIDTH_OFFSET;
  layout.world_height = aexcompat::abi::x86_64_windows::LAYER_HEIGHT_OFFSET;
  return layout;
}
constexpr aexcompat::render_lifecycle::Layout kRenderLifecycleLayout =
    make_render_lifecycle_layout();

// SmartFX states its geometry through SMART_PRE_RENDER's result and max_result
// rects, and nothing on that route reads `out_data->width/height` back or has a
// resize step. It names no extent offsets, so FRAME_SETUP is not invited to
// revise an extent whose answer would be discarded.
constexpr aexcompat::render_lifecycle::Layout kSmartLifecycleLayout =
    aexcompat::render_lifecycle::without_extent_negotiation(kRenderLifecycleLayout);

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

// The four lifecycle entries, each taking the layout its caller renders under.
// One definition per lifecycle step rather than one per (step, layout): a begin
// and an end that disagreed about the layout is a class of bug the smart route
// walked into the first time these were duplicated.
RenderLifecycle begin_frame_lifecycle(
    const aexcompat::render_lifecycle::Layout& layout, EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::begin_frame(
      lifecycle_hooks(context), layout, input.data(), output.data(), params, world);
}

int32_t end_frame_lifecycle(
    const aexcompat::render_lifecycle::Layout& layout, EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::end_frame(
      lifecycle_hooks(context), layout, input.data(), output.data(), params, world,
      lifecycle, primary_error);
}

RenderLifecycle begin_render_lifecycle(
    const aexcompat::render_lifecycle::Layout& layout, EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::begin_render(
      lifecycle_hooks(context), layout, input.data(), output.data(), params, world);
}

int32_t end_render_lifecycle(
    const aexcompat::render_lifecycle::Layout& layout, EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  LifecycleContext context{effect_entry};
  return aexcompat::render_lifecycle::end_render(
      lifecycle_hooks(context), layout, input.data(), output.data(), params, world,
      lifecycle, primary_error);
}

// The smart route under the layout that names no extent offsets. One-line
// forwarders rather than copies, and both halves named here so a begin and an
// end cannot drift onto different layouts.
RenderLifecycle begin_smart_frame_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  return begin_frame_lifecycle(kSmartLifecycleLayout, effect_entry, input, output,
                               params, world);
}

RenderLifecycle begin_smart_render_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world) {
  return begin_render_lifecycle(kSmartLifecycleLayout, effect_entry, input, output,
                                params, world);
}

int32_t end_smart_frame_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  return end_frame_lifecycle(kSmartLifecycleLayout, effect_entry, input, output, params,
                             world, lifecycle, primary_error);
}

int32_t end_smart_render_lifecycle(EffectEntry effect_entry,
    std::array<std::byte, kInSize>& input,
    std::array<std::byte, kOutSize>& output, void** params, void* world,
    const RenderLifecycle& lifecycle, int32_t primary_error) {
  return end_render_lifecycle(kSmartLifecycleLayout, effect_entry, input, output, params,
                              world, lifecycle, primary_error);
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
              ? begin_render_lifecycle(kRenderLifecycleLayout, h.entry, h.input,
                                       h.output, h.params.data(), h.world.data())
              : begin_frame_lifecycle(kRenderLifecycleLayout, h.entry, h.input,
                                      h.output, h.params.data(), h.world.data());
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
              ? end_render_lifecycle(kRenderLifecycleLayout, h.entry, h.input, h.output,
                                     h.params.data(), h.world.data(),
                    *static_cast<RenderLifecycle*>(lifecycle), error)
              : end_frame_lifecycle(kRenderLifecycleLayout, h.entry, h.input, h.output,
                                    h.params.data(), h.world.data(),
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
  // The extent `logical_source` was filled at. `width`/`height` are references
  // that `prepare_output` moves to the *output* extent when an effect expands,
  // and the AEGP layer-render context describes `logical_source` with whatever
  // it is handed - a pair that disagrees makes every source checkout fail
  // closed with error 4 (worker_aegp_layer_render_runtime's size check). The
  // resize path only became reachable once FRAME_SETUP started being offered a
  // real extent (issue #984), so these carry the source extent unchanged.
  int32_t source_width; int32_t source_height;
  // What the caller learns about this frame's output. Optional: the one-shot
  // routes that do not report frame geometry pass nullptr.
  aexcompat::render::ClassicFrameOutput* frame_output{};
  // Why `prepare_output` returned what it did, for the stage marker its caller
  // emits. Static strings only; null means it had nothing to add.
  const char* resize_reason{};

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
          auto& owner = *static_cast<ClassicRenderDispatchOwner*>(opaque);
          const int32_t error = owner.prepare_output();
          // One line, and the reason rides it. Several refusals in there report
          // 4, so the code alone cannot say which; the broker parses `reason=`
          // off this same marker into the stage event (issue #984). A separate
          // line would be dropped by that parser, and a second `_end` would
          // double-count the stage for every consumer that reads the events.
          if (error != 0 || owner.resize_reason) {
            std::cerr << "stage:classic_output_resize_end error=" << error;
            if (owner.resize_reason) std::cerr << " reason=" << owner.resize_reason;
            std::cerr << "\n" << std::flush;
          }
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
  // Bring `in_data->extent_hint` inside the buffer that now backs the output.
  void shrink_extent_hint(int32_t output_width, int32_t output_height) {
    std::array<int32_t, 4> hint{};
    std::memcpy(hint.data(), input.data() + kInExtentHint, sizeof(hint));
    hint = aexcompat::render::extent_hint_within(hint, output_width, output_height);
    std::memcpy(input.data() + kInExtentHint, hint.data(), sizeof(hint));
  }
  int32_t prepare_output() {
    const auto fail = [this](int32_t error) {
      if (frame_output) frame_output->validation_failed = true;
      return error;
    };
    const int32_t next_width = read<int32_t>(output, kOutWidth);
    const int32_t next_height = read<int32_t>(output, kOutHeight);
    // Declining the extent comes first, and is not validated: a zero or
    // half-zero pair is not an extent, and the pair `begin_frame` offered is the
    // extent the host already laid the world out at. Validating it would turn
    // "I want nothing" written as a lone zero into a refused frame.
    // The origin travels with the resize or not at all. Both halves of it - what
    // in_data tells the plug-in, and what `ClassicFrameOutput` tells the frame
    // report - have to say the same thing: relaying an inset to the plug-in
    // while reporting the frame at the layer's top-left makes RENDER draw as if
    // its buffer were displaced and the host place it as if it were not, which
    // is a shifted picture with no error anywhere. AE_Effect.h's "non-zero only
    // when effect changes buffer size" is what picks which of the two agreeing
    // answers is right when nothing resized.
    //
    // The offer makes "restated the extent it was handed" indistinguishable from
    // "wrote nothing", so an origin stated alongside a restated extent declines
    // with it. Recorded, because that is a real difference from AE if AE honours
    // it, and a diagnostic is what makes the difference findable.
    if (aexcompat::render::output_extent_unchanged(width, height, next_width, next_height)) {
      if (read<int32_t>(output, kOutOrigin) != 0 || read<int32_t>(output, kOutOrigin + 4) != 0)
        resize_reason = "origin_without_resize";
      return 0;
    }
    if (!aexcompat::render::validate_output_extent(width, height, next_width, next_height,
            read<uint32_t>(output, kOutFlags))) {
      resize_reason = "extent_not_allowed";
      // What the plug-in asked for against what it was, and which resize flags
      // it declared: the collapsed `extent_not_allowed` could not say whether
      // an expand/shrink lacked its flag or the extent was out of range
      // (issue #984 family). Always on, integers only.
      std::cerr << "stage:classic_output_resize_denied from=" << width << "x" << height
                << " to=" << next_width << "x" << next_height
                << " out_flags=" << read<uint32_t>(output, kOutFlags) << "\n" << std::flush;
      return fail(4);
    }
    const int32_t origin_x = read<int32_t>(output, kOutOrigin);
    const int32_t origin_y = read<int32_t>(output, kOutOrigin + 4);
    // Frame-local, not `fail`: this runs before `guarded.reset` and before the
    // world is re-laid or re-registered, so nothing the session owns has moved
    // and the next frame can run. `validation_failed` is the session's
    // output-bounds invariant and ends the session on the frame that sets it,
    // which would turn one implausible origin into a dead session and a
    // respawned worker per frame.
    if (!aexcompat::render::validate_output_origin(origin_x, origin_y, source_width,
                                                   source_height, next_width, next_height)) {
      resize_reason = "origin_not_plausible";
      return 4;
    }
    // The buffer about to be released was handed to FRAME_SETUP, so an overrun
    // committed there is only observable now - `guarded.reset` frees it and
    // `finalize` would only ever see the replacement's sentinels. Nothing is
    // recorded here: `finalize` reads `guarded.sentinels_intact()` off this same
    // un-reset buffer (the `fail` below returns before `reset`) and is what
    // writes the flag, so a second writer would only be another thing to keep
    // in agreement.
    if (!guarded.sentinels_intact()) {
      resize_reason = "sentinels_broken";
      return fail(4);
    }
    // The extent moves only once the buffer that backs it exists. `width`,
    // `height`, `rowbytes` and `destination` are references the caller's
    // `finalize` reads whatever this returns, so committing the new extent
    // before a `reset` that can fail would leave it scanning the enlarged
    // extent across the old, smaller allocation and straight into the guard
    // page that follows it.
    const int32_t next_rowbytes = next_width * pixel_bytes;
    // Through `fail`, which stops the session. The label is wrong - this is host
    // resource exhaustion, not the plug-in asking for something invalid - but
    // stopping is the property that matters: the host cannot give this frame an
    // output buffer, so it cannot give the next one either, and a session that
    // keeps going returns a whole range of frame-local -3s and closes reporting
    // no error at all. The marker below is what separates the two causes until
    // there is a channel that stops the session without claiming validation.
    if (!guarded.reset(static_cast<std::size_t>(next_rowbytes) * next_height)) {
      resize_reason = "output_allocation_failed";
      return fail(-3);
    }
    width = next_width; height = next_height; rowbytes = next_rowbytes;
    destination = guarded.data();
    if (!aexcompat::render::prepare_world_layout(world,
            {pixel_bytes == 4 ? 0 : 1, pixel_bytes, width, height, rowbytes}, destination))
      return fail(-3);
    if (!worlds.register_world(world.data(), pixel_format)) {
      resize_reason = "world_registration_failed";
      return fail(4);
    }
    // The rect the SDK invites the effect to iterate ("copying just this
    // rectangle ... is sufficient", AE_Effect.h on extent_hint) was written from
    // the source extent before the lifecycle, and a shrink leaves it naming more
    // rows than the new buffer holds - an effect that honours it writes past the
    // guarded buffer into the sentinel band. Shrinking it back is the whole of
    // what this does.
    //
    // Deliberately not translated by the accepted origin, which was tried and
    // withdrawn. This host's own PF_Iterate refuses (rather than clamps) an area
    // wider than the *input* world - `bound_width = min(source, destination)` in
    // worker_pf_suites.cpp - so a hint rewritten into output coordinates makes
    // the canonical `iterate(..., &in_data->extent_hint, ...)` call fail for
    // every expanding effect, where leaving it in input coordinates works. Which
    // coordinate frame AE states the hint in after a resize is issue #997's
    // question, and it is not answerable from this host's behaviour alone.
    shrink_extent_hint(width, height);
    // Only now: this is the frame's geometry, and only an accepted resize has
    // one. A refused resize leaves nothing here for the frame report, and
    // nothing in in_data either - the two have to agree.
    write<int32_t>(input, kInOutputOriginX, origin_x);
    write<int32_t>(input, kInOutputOriginY, origin_y);
    if (frame_output) {
      frame_output->input_origin_x = origin_x;
      frame_output->input_origin_y = origin_y;
    }
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
        time_step, total_time, pixel_bytes, &logical_source, source_width, source_height};
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
                    aexcompat::render::ClassicFrameOutput* frame_output = nullptr) {
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
  // POINT/POINT_3D defaults are percentages of the layer size (SDK
  // PF_PointDef); the input world's extent is what turns them into pixels.
  initialize_parameter_definitions(
      definitions,
      read<int32_t>(input_world, aexcompat::abi::x86_64_windows::LAYER_WIDTH_OFFSET),
      read<int32_t>(input_world, aexcompat::abi::x86_64_windows::LAYER_HEIGHT_OFFSET));
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
    if (!apply_requested_assignments(
            definitions, *requested,
            read<int32_t>(input_world, aexcompat::abi::x86_64_windows::LAYER_WIDTH_OFFSET),
            read<int32_t>(input_world, aexcompat::abi::x86_64_windows::LAYER_HEIGHT_OFFSET)))
      return -3;
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
  std::memcpy(input.data() + kInExtentHint, full_extent, sizeof(full_extent));
  if (partial_extent_hint) {
    const int32_t extent[4] = {3, 2, 11, 8};
    std::memcpy(input.data() + kInExtentHint, extent, sizeof(extent));
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
  // FRAME_SETUP is already plug-in code and may checkout parameters. Publish
  // this frame's time before the first lifecycle selector, using the static
  // capability flags available at entry. QUERY_DYNAMIC_FLAGS runs inside
  // lifecycle_owner.begin(); its answer is applied again below for the rest of
  // the frame (issue #839).
  const uint32_t static_out_flags = read<uint32_t>(command_output, kOutFlags);
  const uint32_t static_out_flags2 = read<uint32_t>(command_output, kOutFlags2);
  const bool static_wide_time_allowed =
      (static_out_flags & kOutFlagWideTimeInput) != 0 ||
      ((static_out_flags2 & kOutFlag2AutomaticWideTimeInput) != 0 &&
       (static_out_flags2 & kOutFlag2SupportsSmartRender) == 0);
  classic_context->configure_checkout_time(
      read<int32_t>(input, kInCurrentTime), read<uint32_t>(input, kInTimeScale),
      static_wide_time_allowed,
      (static_out_flags & kOutFlagIUseShutterAngle) != 0);
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
    // PF_OutFlag_NOP_RENDER means the host copies the input through instead of
    // dispatching RENDER, so there is no `prepare_output` on this branch and no
    // buffer to re-lay. FRAME_SETUP is still offered the extent (the offer is
    // made before the branch is known), so an effect that advertised NOP_RENDER
    // alongside a resize flag can now answer with a revision nothing here can
    // honour. Copying the input through at the old extent and reporting success
    // would ship a frame that is silently missing whatever the revision asked
    // for, so this is a refusal with a name instead (issue #984).
    // A frame-local diagnostic, not `frame_output->validation_failed`. That flag
    // is the session's output-bounds invariant (worker_render_session.cpp) and
    // tears the whole session down on the frame that sets it; nothing here
    // touched the guarded buffer or the world registration, so the host's state
    // is intact and the next frame can run. Escalating would turn one effect's
    // unanswerable request into a dead session and a respawned worker per frame.
    const int32_t revised_width = read<int32_t>(command_output, kOutWidth);
    const int32_t revised_height = read<int32_t>(command_output, kOutHeight);
    if (error == 0 && !aexcompat::render::output_extent_unchanged(
                          width, height, revised_width, revised_height)) {
      // Emitted here rather than through RenderHooks: this branch never reaches
      // `prepare_output`, so nothing else files the stage. Without it the
      // broker's stage parser sees only the plug-in's own `render_end` and
      // attributes the host's refusal to the plug-in (the #722 shape).
      std::cerr << "stage:classic_output_resize_end error=4 reason=nop_render_resize\n"
                << std::flush;
      error = 4;
    }
    if (error == 0) {
      for (int32_t y = 0; y < height; ++y)
        std::memcpy(destination + y * rowbytes,
                    logical_source.data() + y * width * pixel_bytes,
                    width * pixel_bytes);
    }
    // A failing setdown still wins. Main reported `finish` unconditionally
    // here, so keeping the refusal above only when setdown was clean is what
    // stops a plug-in that corrupts its sequence handle at FRAME_SETDOWN from
    // being reported as an output-resize refusal instead.
    const int32_t finish_error = lifecycle_owner.finish(lifecycle);
    if (finish_error != 0) error = finish_error;
  } else {
    ClassicRenderDispatchOwner dispatch_owner{entry, input, command_output, output_world,
        guarded, dispatch_worlds, definitions, params, width, height, rowbytes, destination,
        pixel_bytes, dispatch_pixel_format, external_current_time, external_time_step,
        external_total_time, external_time_scale, case_id, requested, external_rgba,
        external_layers, external_width, external_height, *classic_context, logical_source,
        width, height, frame_output};
    // The `stage:classic_render_*` and `stage:classic_output_resize_end` markers are
    // emitted per frame from inside RenderHooks (see ClassicRenderDispatchOwner)
    // so each brackets only the step it names. The session-wide `stage:render_*`
    // pair in worker_invocation_orchestration.cpp is emitted once, so before
    // this a classic session's frame errors carried no stage at all and every
    // one of them came back with `first_failure_stage: null` (issue #722).
    // `dispatch_owner` fills `frame_output` in place: where an expand put the
    // source inside the enlarged output, and whether the host refused the
    // resize. Without the origin a resized buffer is composited at the layer's
    // top-left and the picture is shifted by the inset (issue #984).
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
  aexcompat::render::ClassicFrameOutput* frame_output;
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
      request.captured_argb, request.frame_output);
}

// Declared in worker_classic_render_entry.hpp; the defaults live there.
int32_t render_once(EffectEntry entry, std::array<std::byte, kInSize>& input,
                    std::array<std::byte, kOutSize>& output,
                    const std::string& case_id, int32_t& width, int32_t& height,
                    int32_t& rowbytes, std::string& input_hash, std::string& output_hash,
                    bool& guards_intact, const RequestedAssignments* requested,
                    const std::vector<unsigned char>* external_rgba,
                    int32_t external_width, int32_t external_height,
                    const std::vector<ExternalLayerInput>* external_layers,
                    int32_t external_current_time, int32_t external_time_step,
                    int32_t external_total_time, uint32_t external_time_scale,
                    int32_t external_pixel_bytes, bool manage_sequence,
                    std::vector<unsigned char>* captured_argb,
                    aexcompat::render::ClassicFrameOutput* frame_output) {
  ClassicRenderRequest request{entry, input, output, case_id, width, height, rowbytes,
      input_hash, output_hash, guards_intact, requested, external_rgba,
      external_width, external_height, external_layers, external_current_time,
      external_time_step, external_total_time, external_time_scale, external_pixel_bytes,
      manage_sequence, captured_argb, frame_output};
  aexcompat::worker_runtime::classic::Request context{
      &request,
      {&classic_render_guarded_effect_main,
       &aexcompat::host_audio::cleanup_after_render,
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
  // The smart path serves parameter checkouts from the hosted ledger, not from a
  // classic dispatch context, and that ledger kept its default current_time 0 /
  // time_scale 1 because nothing ever set it. `checkout_param` refuses any other
  // time, so every SmartFX frame past t=0 had its first checkout answered with
  // PF_Err_OUT_OF_MEMORY and the plug-in gave up - AviUtl2 renders at the cursor,
  // so no smart effect worked anywhere but frame 0 (issue #828).
  //
  // This runs before anything enters the plug-in. PF_Cmd_QUERY_DYNAMIC_FLAGS
  // and PF_Cmd_FRAME_SETUP both reach the plug-in ahead of SMART_PRE_RENDER,
  // and the SDK documents the first as a place to check parameters out
  // (AE_Effect.h: "the effect may examine the values of its parameters at the
  // current time (except layer parameters) by checking them out"), so
  // configuring only next to the dynamic out-flags below would leave both
  // answering against the previous frame's time.
  //
  // The wide-time rule used here is whatever `prepare` last read out of the
  // out-flags in `command_output`. On the first frame that is the GLOBAL_SETUP
  // advertisement; on later frames of a session the buffer still holds the
  // previous frame's QUERY_DYNAMIC_FLAGS result, because nothing restores it
  // between frames (issue #843). Either way it is only in force until the call
  // below re-applies the rule from this frame's own dynamic flags.
  aexcompat::l2_detail::configure_hosted_checkout_time(
      external_current_time, external_time_scale,
      smart_state().wide_time_checkout_allowed);
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
  initialize_parameter_definitions(
      definitions,
      read<int32_t>(input_world, aexcompat::abi::x86_64_windows::LAYER_WIDTH_OFFSET),
      read<int32_t>(input_world, aexcompat::abi::x86_64_windows::LAYER_HEIGHT_OFFSET));
  if (!initialize_arbitrary_values(entry, input, command_output, definitions)) return result;
  ArbitraryValuesScope arbitrary_scope{entry, &input, &command_output, &definitions};
  const aexcompat::worker_runtime::smart_setup::ParameterRequest parameter_request{
      entry, &input, &command_output, &case_id, &plan, requested,
      external_layers, external_current_time, external_time_step,
      external_total_time, external_time_scale, g_full_resolution_width,
      g_full_resolution_height, dispatch_pixel_format, &input_world,
      &dispatch_worlds, &source};
  // Interpolate and roundtrip the arbitrary values BEFORE prepare_parameters
  // snapshots the definitions into `params`. In the smart path the plug-in
  // renders from that `params` snapshot, so churning definitions[u+16] after
  // the snapshot (each of these disposes the old value handle and swaps in a
  // freshly allocated one) strands the handle `params` still points at; the
  // plug-in's render-time lock of it then fails closed with
  // PF_Err_INTERNAL_STRUCT_DAMAGED (issue #993). ARB callbacks do not require
  // an active SEQUENCE here: initialize_arbitrary_values just above already
  // exercises ARB_COPY before begin_lifecycle. The classic path keeps these
  // after render_click because its dispatch_render_draw reads `definitions`
  // directly, so there is no earlier snapshot to go stale there.
  // interpolate reads the frame's current/total time from the input buffer,
  // which prepare_parameters no longer populates first, so publish the times
  // here (prepare_parameters re-publishes the same values, idempotently).
  aexcompat::worker_runtime::smart_setup::publish_frame_times(parameter_request);
  if (!interpolate_arbitrary_values(entry, input, command_output, definitions) ||
      !roundtrip_arbitrary_values(entry, input, command_output, definitions)) {
    // Same failure code as before the move; the lifecycle has not begun yet,
    // so unlike the old call site there is nothing to tear down.
    result.pre_error = -5;
    return result;
  }
  if (!aexcompat::worker_runtime::smart_setup::prepare_parameters(
          parameter_request, parameter_state,
          {&apply_parameter_animation, &dump_world_snapshot}))
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
  // Both halves under the smart layout, which names no extent offsets.
  const auto begin_lifecycle =
      session ? &begin_smart_frame_lifecycle : &begin_smart_render_lifecycle;
  const auto end_lifecycle =
      session ? &end_smart_frame_lifecycle : &end_smart_render_lifecycle;
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
  // Re-apply the frame time now that QUERY_DYNAMIC_FLAGS has had its say: only
  // the wide-time rule can have changed, but it decides whether a checkout at
  // another time is admitted for the rest of the frame (issue #828). The time
  // itself was already published before the first selector, above.
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
