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
