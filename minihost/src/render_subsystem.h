#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

// Render dispatch is deliberately opaque to the command-line worker.  The
// runtime owns PF world, parameter, suite, and module-audit state; this
// boundary only owns selector admission, error priority, and cleanup order.
// Keeping it in a normal header/cpp pair prevents render code from being
// textually included into l2_main.cpp.
namespace aexcompat::render {

enum class RenderKind { Classic, SmartPreRenderAndRender };

struct HostHooks {
  // Calls EffectMain through the host's SEH/module-audit guarded path.
  int (*guarded_effect_main)(void* request){};
  // Restores world registrations, suite leases, parameter checkouts and
  // pre-render data.  It must be safe after a partially completed dispatch.
  int (*cleanup)(void* request){};
  // Verifies the runtime prepared all dependencies before selector admission.
  bool (*dependencies_ready)(void* request){};
};

struct RenderContext {
  RenderKind kind{};
  void* request{};
  HostHooks hooks{};
  bool module_audit_required{};
  bool selector_started{};
  bool cleanup_started{};
  int primary_error{};
  int cleanup_error{};
};

// Dispatches a fully prepared request exactly once.  A cleanup failure only
// wins when the selector path succeeded, preserving the native failure
// priority used by Classic and SmartFX GPU/CPU lifecycles.
int dispatch(RenderContext& context);

// The request-shaping portion of the render runtime is independent of PF
// suite state.  Keep the bounds and the named self-test profiles here so the
// Classic and Smart entry paths cannot silently drift apart.
struct ImageRequest {
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  int32_t pixel_bytes{};
  bool connected_map{};
  bool partial_extent_hint{};
};

// PF_World is a host ABI blob, but the bounded layout we expose to an effect
// is common to Classic and SmartFX.  Keeping the raw-byte preparation here
// makes its dimensions, row stride, and extent checks independent of either
// worker's selector plumbing.
struct WorldLayout {
  int32_t world_flags{};
  int32_t pixel_bytes{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
};

bool prepare_world_layout(std::array<std::byte, 120>& world,
                          const WorldLayout& layout, void* pixels);

struct MapWorld {
  int32_t width{};
  int32_t height{};
  std::vector<unsigned char> pixels;
  std::array<std::byte, 120> world{};
};

// Construct the bounded ARGB8 map world used by the connected-map Classic
// and SmartFX requests.  The worker retains registration and callback
// ownership while this subsystem owns the shared pixel/world preparation.
bool prepare_connected_map_world(const std::string& case_id, int32_t input_width,
                                 int32_t input_height, MapWorld& map);

struct ParameterProfile {
  int32_t amount{5};
  int32_t direction{3};
  int32_t seed{};
  int32_t repeat{1};
  double mix{100.0};
  bool inverted_map{};
};

// Named worker cases select parameter defaults before PF parameter records
// are allocated.  Both render paths consume this immutable profile.
ParameterProfile prepare_parameter_profile(const std::string& case_id);

// Validates effect-requested Classic output resizing before the host swaps a
// guarded output world.  The selector may only grow/shrink with the matching
// advertised output flag, and all extents remain bounded.
bool validate_output_extent(int32_t current_width, int32_t current_height,
                            int32_t requested_width, int32_t requested_height,
                            uint32_t output_flags);

// The two ways an effect declines to resize: leaving the extent at zero, and
// restating the extent it was offered before FRAME_SETUP (issue #984). Both
// mean the host keeps the output world it already laid out - re-laying it would
// repack a padded stride and swap the guarded buffer out from under a pointer
// the plug-in may have taken during FRAME_SETUP.
//
// Deliberately exact. Anything else, including a negative extent or one axis
// zeroed, is a malformed answer that `validate_output_extent` must still refuse
// as a reproducible diagnostic rather than being absorbed as "no resize".
bool output_extent_unchanged(int32_t current_width, int32_t current_height,
                             int32_t requested_width, int32_t requested_height);

// Plausibility of PF_OutData::origin, which AE_Effect.h defines (on
// PF_InData::output_origin_x/y, the field it is copied into) as "the position
// of the top left corner of the input buffer in the output buffer". So it is
// positive when the effect expanded - the input is inset inside a bigger buffer
// - and negative when it cropped, because the output is then a window taken out
// of the input and the input's corner sits above/left of it (issue #984).
//
// Deliberately weak, and not a host-protection bound. The host never indexes
// with this value; it writes it into in_data for the plug-in that stated it,
// and the plug-in's own writes are contained by the guarded buffer's sentinels
// and guard page. Two tighter rules were tried and both were wrong: requiring
// the whole source to fit is unsatisfiable for any declared shrink, and
// requiring a non-negative origin refuses the canonical crop answer. What
// remains is what a stated origin cannot be: absurd in magnitude, or placing
// the input rectangle entirely off the output so that nothing it describes is
// in the buffer.
bool validate_output_origin(int32_t origin_x, int32_t origin_y,
                            int32_t source_width, int32_t source_height,
                            int32_t output_width, int32_t output_height);

// What the host learned about a Classic frame's output while dispatching it.
//
// One bundle rather than parallel out-parameters: the classic entry point's
// signature is hand-mirrored across three translation units, so every new
// out-parameter is three edits plus a fourth (threading it to the code that
// fills it) that nothing forces. Issue #984 first added the origin as two
// out-params and the fourth edit was missed - the parameters linked, and were
// dropped on the floor inside the entry point. Growing this struct cannot
// repeat that, because the pointer is already threaded.
struct ClassicFrameOutput {
  // The host refused or failed the plug-in's requested output resize. Lets a
  // caller tell host-side output validation apart from selector errors that
  // share the same numeric codes.
  bool validation_failed{};
  // PF_OutData::origin as the effect stated it, in the plug-in's own
  // convention: the position of the input buffer's top-left corner in the
  // output buffer, so positive when the effect expanded and negative when it
  // cropped. Deliberately NOT the layer-relative origin the frame report
  // carries (SessionFrameOutput::origin_x, which is negative when the output
  // grew) - the conversion is the reporting side's, and keeping the plug-in's
  // own convention here is what makes the sign visible at it.
  //
  // Filled only when the host accepted a resize, and only after it accepted
  // one: AE_Effect.h says this is "non-zero only when effect changes buffer
  // size", so an origin stated without a resize is not the frame's geometry and
  // a refused resize leaves none behind. Zero on every other path.
  int32_t input_origin_x{};
  int32_t input_origin_y{};
};

// `PF_InData::extent_hint` brought inside an output buffer that just shrank.
//
// The hint is written from the source extent before the lifecycle runs, and an
// accepted shrink leaves it naming more rows and columns than the new buffer
// holds. The SDK invites an effect to iterate exactly this rect ("copying just
// this rectangle from the source image to the destination image is sufficient"),
// so a hint left too large is an invitation to write past the guarded buffer
// into its sentinel band (issue #984).
//
// Only ever shrinks, and never below an empty rect at the origin it already
// had: growing it would name rows the input never had, and moving its top-left
// would put it in a different coordinate frame than the one the effect was
// handed. Which frame AE states the hint in after a resize is issue #997.
std::array<int32_t, 4> extent_hint_within(const std::array<int32_t, 4>& hint,
                                          int32_t output_width, int32_t output_height);

// The frame report's origin from the one the plug-in stated. The two describe
// the same geometry from opposite ends: PF_OutData::origin is where the input
// buffer's top-left sits in the output buffer, while the report's origin is
// where the output buffer's top-left sits relative to the layer. An effect that
// grew its output by 4 and placed the input 3px inside it stated 3 and is
// reported at -3, which is where its buffer starts.
//
// A named function rather than a `-` at the call site because it is the one
// place the two conventions meet, the SmartFX path reaches the same field from
// the other direction (`result_rect`'s top-left, already layer-relative), and a
// sign error here places every resized classic frame on the wrong side of the
// layer origin without failing anything (issue #984).
constexpr int32_t layer_origin_from_input_origin(int32_t input_origin) {
  return -input_origin;
}

struct SmartOutputBounds {
  bool valid{};
  // SDK: PF_PreRenderOutput.result_rect "can be empty". An empty result rect
  // with well-formed geometry is a legal answer that renders nothing; the
  // caller skips the render selector instead of failing the run.
  bool empty_result{};
  std::array<int32_t, 4> result_rect{};
  std::array<int32_t, 4> max_result_rect{};
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  // Top-left of the output buffer in layer coordinates (result_rect's
  // top-left); becomes PF_LayerDef::origin_x/origin_y on the output world.
  int32_t origin_x{};
  int32_t origin_y{};
};

// Absolute-coordinate bound for plug-in supplied Smart geometry rects. Layer
// coordinates can legitimately be negative (buffer expansion), but a rect
// coordinate beyond this magnitude cannot come from any real composition and
// only feeds later size arithmetic, so it fails closed.
constexpr int32_t kMaxSmartRectMagnitude = 1 << 24;

// Well-formedness for plug-in supplied Smart geometry rects: non-inverted,
// coordinates within +/-kMaxSmartRectMagnitude, and edge/area caps.
bool smart_geometry_rect_valid(const std::array<int32_t, 4>& rect);

// True when inner is contained in outer; an empty inner rect is contained in
// anything.
bool smart_rect_contained(const std::array<int32_t, 4>& inner,
                          const std::array<int32_t, 4>& outer);

// Self-test for the rect validation and containment helpers.
bool smart_geometry_rect_self_test();

// Parses Smart Pre-Render rectangles and applies the same output bounds used
// to allocate the guarded Smart render world.
SmartOutputBounds prepare_smart_output_bounds(const void* pre_render_output,
                                              std::size_t output_size,
                                              int32_t pixel_bytes);

bool copy_packed_world(const unsigned char* strided_source, int32_t rowbytes,
                       int32_t width, int32_t height, int32_t pixel_bytes,
                       std::vector<unsigned char>& packed_destination);
bool finite_float_world(const std::vector<unsigned char>& packed);

int prepare_image_request(const std::string& case_id, bool has_external_input,
                          int32_t external_width, int32_t external_height,
                          int32_t external_pixel_bytes, ImageRequest& request);
bool build_argb_input(const ImageRequest& request,
                      const std::vector<unsigned char>* external_rgba,
                      std::vector<unsigned char>& logical_argb,
                      unsigned char* strided_destination);

// Debug evidence is host-owned, but its serialization and byte transport are
// shared by Classic and Smart rendering.  The hash and color conversion hooks
// preserve the worker's existing provenance implementation without exposing
// worker globals to this translation unit.
struct TelemetryHooks {
  void (*argb_to_rgba_native)(void*, const void*, int32_t){};
  std::string (*sha256_bytes)(const unsigned char*, std::size_t){};
};

// Spatial/quality render context parsed from the CLI spatial-context and
// render-environment payloads (issue #126 Phase D): downsample ratios, pixel
// aspect ratio, full-resolution override, pre-effect source origin, and the
// quality/field/shutter values written into the effect input block.
// worker_main's payload parsers write it and the effect bootstrap, dispatch,
// and completion reports read it (partly through pointer bundles). Lifetime:
// process-lifetime, defaults quality=1 / identity ratios, never torn down.
struct SpatialRatio { int32_t numerator{1}; uint32_t denominator{1}; };
struct RenderContextState {
  SpatialRatio downsample_x;
  SpatialRatio downsample_y;
  SpatialRatio pixel_aspect_ratio;
  int32_t full_resolution_width{};
  int32_t full_resolution_height{};
  int32_t pre_effect_source_origin_x{};
  int32_t pre_effect_source_origin_y{};
  int32_t render_quality{1};
  int32_t render_field{};
  int32_t shutter_angle{};
  int32_t shutter_phase{};
};
RenderContextState& render_context_state();

struct RenderTelemetry {
  const std::filesystem::path* dump_directory{};
  uint32_t* dumps_written{};
  uint32_t* dumps_skipped{};
  uint64_t* dump_bytes{};
  bool output_checksum_detail{};
  std::vector<uint32_t>* output_row_crc32{};
  std::array<std::string, 4>* output_channel_sha256{};
  TelemetryHooks hooks{};
};

// Owned storage behind the worker's RenderTelemetry bundles (issue #126
// Phase D): the world-dump destination and counters plus the opt-in output
// checksum detail. Writers are worker_main's dump/checksum-detail CLI
// callbacks and the render pipeline through the pointer bundle; the image
// report emitters read it back via world_debug_report_json. Lifetime:
// process-lifetime, empty/zero defaults, never torn down.
struct TelemetryState {
  std::filesystem::path dump_worlds_dir;
  uint32_t world_dumps_written{};
  uint32_t world_dumps_skipped{};
  uint64_t world_dump_bytes{};
  bool output_checksum_detail{};
  std::vector<uint32_t> output_row_crc32;
  std::array<std::string, 4> output_channel_sha256;
};
TelemetryState& telemetry_state();

void dump_world_snapshot(RenderTelemetry& telemetry, const std::string& stage,
                         const unsigned char* packed_argb, int32_t width,
                         int32_t height, int32_t pixel_bytes);
void record_output_checksum_detail(RenderTelemetry& telemetry,
                                   const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes);
std::string world_debug_report_json(const RenderTelemetry& telemetry);

}  // namespace aexcompat::render
