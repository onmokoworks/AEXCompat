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

void dump_world_snapshot(RenderTelemetry& telemetry, const std::string& stage,
                         const unsigned char* packed_argb, int32_t width,
                         int32_t height, int32_t pixel_bytes);
void record_output_checksum_detail(RenderTelemetry& telemetry,
                                   const unsigned char* rgba, int32_t width,
                                   int32_t height, int32_t pixel_bytes);
std::string world_debug_report_json(const RenderTelemetry& telemetry);

}  // namespace aexcompat::render
