#include "worker_l2_render_abi.hpp"

#include <windows.h>

#include "worker_aegp_async_layer_runtime.hpp"
#include "worker_aegp_external_render_runtime.hpp"
#include "worker_aegp_item_render_runtime.hpp"
#include "worker_aegp_layer_render_runtime.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_staged_item_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_pf_adv_time_suite.hpp"
#include "worker_render_receipts.hpp"
#include "worker_world_safety.hpp"
#include "worker_world_registry.hpp"

#include <cstdint>
#include <cstring>

namespace aexcompat::l2_detail {

// Worker-entry owned mode predicates and render-options snapshot wrappers
// stay in l2_main with the dispatch and registry state they guard; the
// callbacks here read them cross-TU.
using AegpRenderOptionsValue = aexcompat::render_options::ItemValue;
using aexcompat::suite_abi::AegpRect;
bool is_render_worker();
void* aegp_comp_item_handle();
bool snapshot_render_options(void* handle, AegpRenderOptionsValue& value);
bool snapshot_layer_render_options(void* handle, AegpLayerRenderOptionsValue& value);
void bump_render_project_timestamp();
using aexcompat::world_safety::DispatchWorldFormat;
using aexcompat::world_safety::resolve_registered_dispatch_world;

namespace {
using aexcompat::render_receipts::ReceiptSnapshot;
using aexcompat::worker_runtime::handles::free_aegp_mem_handle;
using aexcompat::worker_runtime::handles::lock_aegp_mem_handle;
using aexcompat::worker_runtime::handles::new_aegp_mem_handle;
using aexcompat::worker_runtime::handles::unlock_aegp_mem_handle;
using aexcompat::world_registry::kPixelFormatArgb128;
using aexcompat::world_registry::kPixelFormatArgb32;
using aexcompat::world_registry::kPixelFormatArgb64;
constexpr int32_t kSyntheticCompWidth = 17;
constexpr int32_t kSyntheticCompHeight = 9;
// Receipt test-mode storage stays with its owner,
// aexcompat::render_receipts::receipt_test_state() (issue #126 Phase D).
auto& g_async_manager =
    aexcompat::render_receipts::receipt_test_state().async_manager;
auto& g_synthetic_receipt_test_mode =
    aexcompat::render_receipts::receipt_test_state().synthetic_test_mode;
}  // namespace

uint32_t staged_item_project_generation() {
  return aexcompat::aegp_external_render_runtime::project_generation();
}
bool staged_item_synthetic_receipts_enabled() {
  return g_synthetic_receipt_test_mode;
}
const bool g_staged_item_runtime_configured = [] {
  aexcompat::aegp_staged_item_runtime::configure({
      &staged_item_project_generation, &staged_item_synthetic_receipts_enabled,
      &aexcompat::aegp_item_render_runtime::publish_synthetic});
  return true;
}();
const bool g_external_render_runtime_configured = [] {
  aexcompat::aegp_external_render_runtime::configure({
      &aexcompat::aegp_staged_item_runtime::clear,
      +[](void* item) { return item == aegp_comp_item_handle(); }});
  return true;
}();
const bool g_item_render_runtime_configured = [] {
  aexcompat::aegp_item_render_runtime::configure({
      &snapshot_render_options,
      &aexcompat::aegp_external_render_runtime::publish_cached_receipt,
      &aexcompat::aegp_staged_item_runtime::publish_receipt});
  return true;
}();
const bool g_layer_render_runtime_configured = [] {
  aexcompat::aegp_layer_render_runtime::configure({
      &is_render_worker, &layer_effect_boundary_is_live});
  return true;
}();
int32_t publish_loaded_layer_receipt(
    const AegpLayerRenderOptionsValue& options, void** receipt) {
  return aexcompat::aegp_layer_render_runtime::publish(options, receipt);
}
int32_t __cdecl checkout_item_frame_async(
    void* manager, uint32_t purpose, void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  if (manager != &g_async_manager || purpose == 0) return 4;
  return aexcompat::aegp_item_render_runtime::publish_receipt(options, receipt);
}
int32_t __cdecl checkout_layer_frame_async(
    void* manager, uint32_t purpose, void* options, void** receipt) {
  if (receipt) *receipt = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (manager != &g_async_manager || purpose == 0 || !receipt ||
      !snapshot_layer_render_options(options, snapshot)) return 4;
  if (is_render_worker() && aexcompat::aegp_layer_render_runtime::active())
    return publish_loaded_layer_receipt(snapshot, receipt);
  const int32_t pixel_format = snapshot.world_type == 1 ? kPixelFormatArgb32 :
      (snapshot.world_type == 2 ? kPixelFormatArgb64 : kPixelFormatArgb128);
  return aexcompat::aegp_item_render_runtime::publish_synthetic(
      pixel_format, receipt);
}
int32_t __cdecl get_receipt_world(void* receipt, void*** world) {
  return aexcompat::render_receipts::get_world(receipt, world);
}
int32_t __cdecl checkin_frame(void* receipt) {
  return aexcompat::render_receipts::checkin(receipt);
}

bool checkin_frame_if_live(void* receipt) {
  return aexcompat::render_receipts::checkin_if_live(receipt);
}

int32_t __cdecl render_checkout_frame_reject(
    void* options, AegpRenderCancelV1 check_cancel, void* cancel_refcon, void** out) {
  return aexcompat::aegp_item_render_runtime::checkout(
      options, check_cancel, cancel_refcon, out);
}
int32_t __cdecl render_checkout_layer_reject(
    void* options, uint8_t, void* check_cancel_raw, void* cancel_refcon, void** out) {
  if (out) *out = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (!out || !snapshot_layer_render_options(options, snapshot)) return 4;
  const auto check_cancel = reinterpret_cast<AegpRenderCancelV1>(check_cancel_raw);
  if (check_cancel) {
    uint8_t cancelled = 0;
    const int32_t cancel_error = check_cancel(cancel_refcon, &cancelled);
    if (cancel_error != 0) return cancel_error;
    if (cancelled != 0) return 4;
  }
  return publish_loaded_layer_receipt(snapshot, out);
}
int32_t __cdecl render_checkout_layer_v5(
    void* options, AegpRenderCancelV1 check_cancel, void* cancel_refcon, void** out) {
  if (out) *out = nullptr;
  AegpLayerRenderOptionsValue snapshot{};
  if (!out || !snapshot_layer_render_options(options, snapshot)) return 4;
  if (check_cancel) {
    uint8_t cancelled = 0;
    const int32_t cancel_error = check_cancel(cancel_refcon, &cancelled);
    if (cancel_error != 0) return cancel_error;
    if (cancelled != 0) return 4;
  }
  return publish_loaded_layer_receipt(snapshot, out);
}
int async_layer_exception_filter(EXCEPTION_POINTERS* information,
                                 uint32_t* exception_code) {
  if (exception_code && information && information->ExceptionRecord)
    *exception_code = information->ExceptionRecord->ExceptionCode;
  return EXCEPTION_EXECUTE_HANDLER;
}
int32_t invoke_async_layer_callback_seh(AegpAsyncFrameReadyCallback callback,
    uint64_t request_id, uint8_t canceled, int32_t error, void* receipt,
    void* refcon, int32_t* callback_error, uint32_t* exception_code) {
  if (!callback || !callback_error || !exception_code) return 4;
  *callback_error = 4; *exception_code = 0;
  __try { *callback_error = callback(request_id, canceled, error, receipt, refcon); return 0; }
  __except(async_layer_exception_filter(GetExceptionInformation(), exception_code)) {
    return 4;
  }
}
int32_t __cdecl render_checkout_layer_async_reject(
    void* options, AegpAsyncFrameReadyCallback callback, void* refcon, uint64_t* id) {
  return aexcompat::aegp_async_layer::checkout(options, callback, refcon, id);
}
int32_t __cdecl render_cancel_async_reject(uint64_t id) {
  return aexcompat::aegp_async_layer::cancel(id);
}
void drain_async_layer_requests() { aexcompat::aegp_async_layer::drain(); }
bool async_layer_requests_balanced() { return aexcompat::aegp_async_layer::balanced(); }
const bool g_async_layer_runtime_configured = [] {
  aexcompat::aegp_async_layer::configure({&is_render_worker,
      &snapshot_layer_render_options,
      &aexcompat::aegp_layer_render_runtime::capture_async_source,
      &aexcompat::aegp_layer_render_runtime::publish_async_source,
      &checkin_frame_if_live,
      &invoke_async_layer_callback_seh});
  return true;
}();
int32_t __cdecl render_get_region_reject(void* receipt, void* region) {
  if (!receipt || !region) return 4;
  ReceiptSnapshot snapshot{};
  if (!aexcompat::render_receipts::snapshot(receipt, snapshot)) return 4;
  std::memcpy(region, &snapshot.rendered_region, sizeof(AegpRect));
  return 0;
}
int32_t __cdecl render_sufficient_reject(void* rendered, void* proposed, uint8_t* out) {
  if (out) *out = 0;
  AegpRenderOptionsValue first{}, second{};
  if (!out || !snapshot_render_options(rendered, first) ||
      !snapshot_render_options(proposed, second)) return 4;
  const auto roi = [](const AegpRenderOptionsValue& options) {
    return options.roi.left == 0 && options.roi.top == 0 &&
        options.roi.right == 0 && options.roi.bottom == 0
        ? AegpRect{0, 0, kSyntheticCompWidth, kSyntheticCompHeight} : options.roi;
  };
  const AegpRect rendered_roi = roi(first), proposed_roi = roi(second);
  const auto same_rational = [](const AegpTime& left, const AegpTime& right) {
    return static_cast<int64_t>(left.value) * right.scale ==
        static_cast<int64_t>(right.value) * left.scale;
  };
  const bool same_time = same_rational(first.time, second.time);
  const bool same_step = same_rational(first.time_step, second.time_step);
  const bool contains = rendered_roi.left <= proposed_roi.left &&
      rendered_roi.top <= proposed_roi.top && rendered_roi.right >= proposed_roi.right &&
      rendered_roi.bottom >= proposed_roi.bottom;
  *out = first.item == second.item && same_time && same_step &&
      first.field == second.field && first.world_type == second.world_type &&
      first.downsample_x == second.downsample_x &&
      first.downsample_y == second.downsample_y && first.matte == second.matte &&
      first.channel_order == second.channel_order &&
      first.render_guide_layers == second.render_guide_layers &&
      first.render_quality == second.render_quality && contains;
  return 0;
}
int32_t __cdecl render_sound_reject(void*, const void*, const void*, const void*, void*, void*, void** out) {
  if (out) *out = nullptr; return 4;
}
int32_t __cdecl render_timestamp_reject(void* output) {
  return aexcompat::aegp_external_render_runtime::timestamp(output);
}
int32_t __cdecl render_changed_reject(void* item, const void* start,
    const void* duration, const void* timestamp, uint8_t* out) {
  return aexcompat::aegp_external_render_runtime::changed(
      item, start, duration, timestamp, out);
}
int32_t __cdecl render_worthwhile_reject(
    void* options, const void* timestamp, uint8_t* out) {
  return aexcompat::aegp_external_render_runtime::worthwhile(options, timestamp, out);
}
int32_t __cdecl render_checkin_rendered(
    void* options, const void* timestamp, uint32_t ticks, void* image) {
  return aexcompat::aegp_external_render_runtime::checkin_rendered(
      options, timestamp, ticks, image);
}
int32_t __cdecl render_guid_reject(void* receipt, void** out) {
  if (out) *out = nullptr;
  if (!receipt || !out) return 4;
  ReceiptSnapshot snapshot{};
  if (!aexcompat::render_receipts::snapshot(receipt, snapshot)) return 4;
  const auto& guid = snapshot.guid;
  if (new_aegp_mem_handle(1, "render receipt guid", static_cast<uint32_t>(guid.size()),
                          1, out) != 0) return 4;
  void* bytes = nullptr;
  if (lock_aegp_mem_handle(*out, &bytes) != 0 || !bytes) {
    free_aegp_mem_handle(*out); *out = nullptr; return 4;
  }
  std::memcpy(bytes, guid.data(), guid.size());
  return unlock_aegp_mem_handle(*out);
}

auto& g_pf_adv_item_touches =
    aexcompat::worker_runtime::pf_adv_time::item_telemetry().touches;
auto& g_pf_adv_item_rerenders =
    aexcompat::worker_runtime::pf_adv_time::item_telemetry().rerenders;

int32_t checked_adv_item_move(int32_t direction, int32_t steps, int32_t step,
                              int32_t& time) {
  if ((direction != 0 && direction != 1) || steps < 0 || step <= 0) return 4;
  const int64_t distance = static_cast<int64_t>(steps) * step;
  const int64_t moved = static_cast<int64_t>(time) + (direction == 0 ? distance : -distance);
  if (moved < INT32_MIN || moved > INT32_MAX) return 4;
  time = static_cast<int32_t>(moved);
  return 0;
}

bool active_adv_item_context(void* in_data) {
  const auto& context = aexcompat::aegp_layer_render_runtime::context();
  return context.entry && context.input && in_data == context.input;
}

bool active_adv_item_world(const void* world, DispatchWorldFormat& result) {
  return resolve_registered_dispatch_world(world, result);
}

int32_t __cdecl adv_item_move_time_step(void* in_data, void* world,
                                        int32_t direction, int32_t steps) {
  auto& context = aexcompat::aegp_layer_render_runtime::context();
  DispatchWorldFormat effect_world{};
  if (!active_adv_item_context(in_data) || !active_adv_item_world(world, effect_world) ||
      !effect_world.data || effect_world.width <= 0 || effect_world.height <= 0 ||
      effect_world.rowbytes <= 0 || context.pixel_bytes <= 0 ||
      effect_world.width > INT32_MAX / context.pixel_bytes ||
      effect_world.rowbytes < effect_world.width * context.pixel_bytes) return 4;
  int32_t moved = context.current_time;
  if (checked_adv_item_move(direction, steps, context.time_step, moved) != 0) return 4;
  context.active_item_time = moved;
  context.active_item_time_valid = true;
  return 0;
}

int32_t __cdecl adv_item_move_time_step_active(int32_t direction, int32_t steps) {
  auto& context = aexcompat::aegp_layer_render_runtime::context();
  if (!context.entry || context.time_step <= 0) return 4;
  int32_t moved = context.active_item_time_valid ? context.active_item_time : context.current_time;
  if (checked_adv_item_move(direction, steps, context.time_step, moved) != 0) return 4;
  context.active_item_time = moved;
  context.active_item_time_valid = true;
  return 0;
}

int32_t __cdecl adv_item_touch_active() {
  if (!aexcompat::aegp_layer_render_runtime::context().entry) return 4;
  ++g_pf_adv_item_touches;
  bump_render_project_timestamp();
  return 0;
}

int32_t __cdecl adv_item_force_rerender(void* in_data, void* world) {
  DispatchWorldFormat effect_world{};
  if (!active_adv_item_context(in_data) || !active_adv_item_world(world, effect_world) ||
      !effect_world.data || effect_world.width <= 0 || effect_world.height <= 0 ||
      effect_world.rowbytes <= 0) return 4;
  ++g_pf_adv_item_rerenders;
  bump_render_project_timestamp();
  return 0;
}

int32_t __cdecl adv_item_effect_is_active(void* context_handle, uint8_t* enabled) {
  if (enabled) *enabled = 0;
  if (!context_handle || !enabled || !aexcompat::aegp_layer_render_runtime::context().entry) return 4;
  // UI context handles are opaque. A live render owns no UI context, so headless mode
  // can only report disabled without dereferencing an untrusted or stale handle.
  return 0;
}

int32_t __cdecl get_context_async_manager(void* input, void* extra, void** manager) {
  if (!input || !extra || !manager) return 4;
  *manager = &g_async_manager;
  return 0;
}

PfAdvItemSuite1 g_adv_item_suite1{&adv_item_move_time_step,
    &adv_item_move_time_step_active, &adv_item_touch_active,
    &adv_item_force_rerender, &adv_item_effect_is_active};

BasicSuite g_basic_suite{&acquire_suite, &release_suite};

}  // namespace aexcompat::l2_detail
