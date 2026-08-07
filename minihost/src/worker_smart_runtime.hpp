#pragma once

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
  // The world handed back for one of those. A PF_EffectWorld is 120 bytes and
  // all-zero is exactly an empty layer: no flags, null data, zero rowbytes,
  // zero width and height, and an empty extent. Handing back null with
  // PF_Err_NONE instead would invent a contract AE does not have - a plug-in
  // that reads `world->width` after a successful checkout would fault, and
  // that pattern is in the SDK samples. Owned by the state so its lifetime is
  // the session's.
  std::array<std::byte, 120> empty_layer_world{};
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
