#pragma once

#include "worker_world_safety.hpp"
#include <array>
#include <atomic>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace aexcompat::worker_runtime::smart {

struct HostedLayer {
  int32_t slot{};
  int32_t time{};
  uint32_t time_scale{};
  bool timed{};
  int32_t width{};
  int32_t height{};
  int32_t checkout_id{-1};
  void* world{};
  void* view_world{};
  std::array<int32_t, 4> checkout_rect{-1, -1, -1, -1};
};

struct PixelCheckout {
  int32_t id{};
  void* world{};
  void* view_world{};
  std::array<int32_t, 4> rect{-1, -1, -1, -1};
  bool checked_out{};
  // PreRender answered this one as an empty layer parameter, so it has no
  // pixels by construction and `checkout_pixels` hands back the host's own
  // empty world rather than treating the absent world as a fault (issue #898).
  bool empty_layer_param{};
  // Distinguishes an unused PreRender registration from a pixel lease that
  // was already checked back in. The former may be retired once without ever
  // asking for pixels; the latter must still reject a duplicate checkin.
  bool ever_checked_out{};
  // A failed pixel request is not an unused registration. Its later checkin
  // remains a failure instead of laundering the earlier missing/empty-world
  // refusal into a successful cleanup.
  bool checkout_attempted{};
};

struct State {
  void* input_world{};
  void* output_world{};
  void* map_world{};
  void* input_checkout_view_world{};
  void* map_checkout_view_world{};
  std::vector<HostedLayer> hosted_layers;
  std::vector<PixelCheckout> pixel_checkouts;
  int32_t width{16};
  int32_t height{12};
  int32_t map_width{};
  int32_t map_height{};
  int32_t rowbytes{64};
  std::string pixel_format{"argb8"};
  bool wide_time_checkout_allowed{};
  bool shutter_dependency_advertised{};
  int32_t current_time{};
  uint32_t current_time_scale{1};
  uint32_t rejected_temporal_checkouts{};
  int32_t secondary_layer_slot{6};
  // How many parameters the plug-in declared, which bounds what
  // `checkout_layer` may name: the SDK defines its index as "0 = input, 1..n =
  // param". A parameter inside that range that this host has no world for is
  // answered with an empty rect (issue #898); anything outside it is still an
  // unknown layer.
  int32_t param_count{};
  uint32_t empty_layer_param_checkouts{};
  // The layer handed back for one of those: the session's own geometry with
  // nothing in it. A layer parameter with no layer is transparent, not absent -
  // that is what an unset matte or an unconnected second view composites as -
  // so the answer is a real PF_EffectWorld the plug-in can measure, sample and
  // copy, whose every pixel is zero.
  //
  // Allocated by the dispatch through the host's own new-world path, so the
  // world registry owns it and every host callback resolves it. The two shapes
  // tried before this each broke a real plug-in: a null pointer behind
  // PF_Err_NONE, which 3DGlasses answers PF_Err_BAD_CALLBACK_PARAM to, and a
  // 120-byte zeroed world describing a 0x0 layer, which DeepGlow2 answers
  // PF_Err_INTERNAL_STRUCT_DAMAGED to. A hand-built full-size world fails too,
  // for a host reason rather than a plug-in one: `PF_COPY` resolves its
  // arguments through the registry and refuses a world the registry does not
  // own (issues #958, #962).
  aexcompat::world_safety::EffectWorldStorage empty_layer_world{};
  bool empty_layer_world_live{};
  /// Allocates `empty_layer_world` through the host's own new-world path and
  /// returns whether it did. Installed by the dispatch, which is the layer that
  /// may reach the world registry; called on the first checkout that needs the
  /// layer and not before, so a frame whose plug-in never asks for one pays
  /// neither the allocation nor its share of the registry's budget.
  bool (*allocate_empty_layer)(void* world_storage){};
  int32_t full_resolution_width{};
  int32_t full_resolution_height{};
  int32_t pixel_aspect_numerator{1};
  uint32_t pixel_aspect_denominator{1};
  int32_t secondary_checkout_id{-1};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  std::array<int32_t, 4> input_checkout_request{-1, -1, -1, -1};
  std::array<int32_t, 4> map_checkout_request{-1, -1, -1, -1};
  std::array<int32_t, 4> input_checkout_result_rect{-1, -1, -1, -1};
  std::array<int32_t, 4> map_checkout_result_rect{-1, -1, -1, -1};
  uint32_t malformed_checkout_requests{};
  uint32_t empty_checkout_pixel_denials{};
  // Kept apart from the denials above: that counter means "the plug-in asked
  // for pixels it was told did not exist and was refused", and folding the
  // empty-layer-parameter checkouts into it would make a report reader unable
  // to tell a refusal from the answer this host now gives.
  uint32_t empty_layer_param_pixel_checkouts{};
  bool gpu_render_dispatched{};

  void clear_transient();
};

struct Snapshot {
  int32_t width{};
  int32_t height{};
  int32_t rowbytes{};
  std::string pixel_format;
  bool wide_time_checkout_allowed{};
  bool shutter_dependency_advertised{};
  uint32_t rejected_temporal_checkouts{};
  int32_t checkout_time{};
  int32_t checkout_time_step{};
  uint32_t checkout_time_scale{};
  std::array<int32_t, 4> input_checkout_request{-1, -1, -1, -1};
  std::array<int32_t, 4> map_checkout_request{-1, -1, -1, -1};
  std::array<int32_t, 4> input_checkout_result_rect{-1, -1, -1, -1};
  std::array<int32_t, 4> map_checkout_result_rect{-1, -1, -1, -1};
  uint32_t malformed_checkout_requests{};
  uint32_t empty_checkout_pixel_denials{};
  uint32_t empty_layer_param_checkouts{};
  uint32_t empty_layer_param_pixel_checkouts{};
  bool pixel_checkouts_balanced{true};
};

State& state();

// Smart host telemetry (issue #126 Phase D): comp-bg-color and GUID mix-in
// counters recorded by worker_main's AEGP callbacks during smart pre-render
// and read back by the smart completion report. Atomics because the GUID
// mix-in callback may run on plug-in render threads. Lifetime:
// process-lifetime storage; worker_main resets it once per smart render via
// reset_smart_host_telemetry() before smart_pre_render.
struct HostTelemetry {
  std::atomic<uint32_t> comp_bg_color_successes{};
  std::atomic<uint32_t> comp_bg_color_rejections{};
  std::atomic<uint32_t> guid_mix_in_calls{};
  std::atomic<uint32_t> guid_mix_in_successes{};
  std::atomic<uint32_t> guid_mix_in_rejections{};
  std::atomic<uint32_t> guid_mix_in_last_size{};
  std::atomic<uint32_t> guid_mix_in_max_size{};
  std::atomic<int32_t> guid_mix_in_last_result{};
};
HostTelemetry& host_telemetry();
int32_t __cdecl width();
int32_t __cdecl height();

class Session {
 public:
  Session();
  ~Session();
  Session(const Session&) = delete;
  Session& operator=(const Session&) = delete;
  std::shared_ptr<const Snapshot> snapshot() const { return snapshot_; }

 private:
  State state_{};
  State* previous_{};
  std::shared_ptr<Snapshot> snapshot_{std::make_shared<Snapshot>()};
};

int32_t __cdecl pre_checkout_layer(void*, int32_t index, int32_t checkout_id,
                                   const void* request, int32_t what_time,
                                   int32_t time_step, uint32_t time_scale,
                                   void* result);
int32_t __cdecl checkout_pixels(void*, int32_t checkout_id, void** world);
int32_t __cdecl checkin_pixels(void*, int32_t checkout_id);
int32_t __cdecl checkout_output(void*, void** world);
bool pixel_checkouts_balanced();
bool concurrency_self_test();
bool checkout_intersection_self_test();

}  // namespace aexcompat::worker_runtime::smart
