#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::l2_detail {

// Production callbacks stay in l2_main with the render/receipt and suite
// registry state they guard; this header freezes their published slot
// layouts with decltype so the ABI cannot drift from the implementations.
int32_t __cdecl checkout_item_frame_async(void*, uint32_t, void*, void**);
int32_t __cdecl checkout_layer_frame_async(void*, uint32_t, void*, void**);

struct AegpRenderAsyncManagerSuite1 {
  decltype(&checkout_item_frame_async) checkout_item_frame;
  decltype(&checkout_layer_frame_async) checkout_layer_frame;
};
static_assert(sizeof(AegpRenderAsyncManagerSuite1) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderAsyncManagerSuite1, checkout_item_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderAsyncManagerSuite1, checkout_layer_frame) == 1 * sizeof(void*));

using AegpRenderCancelV1 = int32_t(__cdecl*)(void*, uint8_t*);
using AegpAsyncFrameReadyCallback =
    int32_t(__cdecl*)(uint64_t, uint8_t, int32_t, void*, void*);
int32_t __cdecl render_checkout_frame_reject(void*, AegpRenderCancelV1, void*, void**);
int32_t __cdecl render_checkout_layer_reject(void*, uint8_t, void*, void*, void**);
int32_t __cdecl render_checkout_layer_v5(void*, AegpRenderCancelV1, void*, void**);
int32_t __cdecl render_checkout_layer_async_reject(
    void*, AegpAsyncFrameReadyCallback, void*, uint64_t*);
int32_t __cdecl render_cancel_async_reject(uint64_t);
int32_t __cdecl checkin_frame(void*);
int32_t __cdecl get_receipt_world(void*, void***);
int32_t __cdecl render_get_region_reject(void*, void*);
int32_t __cdecl render_sufficient_reject(void*, void*, uint8_t*);
int32_t __cdecl render_sound_reject(void*, const void*, const void*, const void*, void*, void*, void**);
int32_t __cdecl render_timestamp_reject(void*);
int32_t __cdecl render_changed_reject(void*, const void*, const void*, const void*, uint8_t*);
int32_t __cdecl render_worthwhile_reject(void*, const void*, uint8_t*);
int32_t __cdecl render_checkin_rendered(void*, const void*, uint32_t, void*);
int32_t __cdecl render_guid_reject(void*, void**);

struct AegpRenderSuite4 {
  decltype(&render_checkout_frame_reject) render_frame;
  decltype(&render_checkout_layer_reject) render_layer;
  decltype(&checkin_frame) checkin;
  decltype(&get_receipt_world) get_world;
  decltype(&render_get_region_reject) get_region;
  decltype(&render_sufficient_reject) sufficient;
  decltype(&render_sound_reject) render_sound;
  decltype(&render_timestamp_reject) timestamp;
  decltype(&render_changed_reject) changed;
  decltype(&render_worthwhile_reject) worthwhile;
  decltype(&render_checkin_rendered) checkin_rendered;
  decltype(&render_guid_reject) guid;
};
static_assert(sizeof(AegpRenderSuite4) == 12 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, render_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, render_layer) == 1 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, checkin) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, get_world) == 3 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, get_region) == 4 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, sufficient) == 5 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, render_sound) == 6 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, timestamp) == 7 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, changed) == 8 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, worthwhile) == 9 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, checkin_rendered) == 10 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite4, guid) == 11 * sizeof(void*));

struct AegpRenderSuite5 {
  decltype(&render_checkout_frame_reject) render_frame;
  decltype(&render_checkout_layer_v5) render_layer;
  decltype(&render_checkout_layer_async_reject) render_layer_async;
  decltype(&render_cancel_async_reject) cancel_async;
  decltype(&checkin_frame) checkin;
  decltype(&get_receipt_world) get_world;
  decltype(&render_get_region_reject) get_region;
  decltype(&render_sufficient_reject) sufficient;
  decltype(&render_sound_reject) render_sound;
  decltype(&render_timestamp_reject) timestamp;
  decltype(&render_changed_reject) changed;
  decltype(&render_worthwhile_reject) worthwhile;
  decltype(&render_checkin_rendered) checkin_rendered;
  decltype(&render_guid_reject) guid;
};
static_assert(sizeof(AegpRenderSuite5) == 14 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, render_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, render_layer) == 1 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, render_layer_async) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, cancel_async) == 3 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, checkin) == 4 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, get_world) == 5 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite5, guid) == 13 * sizeof(void*));

struct AegpRenderSuite2 {
  decltype(&render_checkout_frame_reject) render_frame;
  decltype(&checkin_frame) checkin;
  decltype(&get_receipt_world) get_world;
  decltype(&render_get_region_reject) get_region;
  decltype(&render_sufficient_reject) sufficient;
  decltype(&render_sound_reject) render_sound;
  decltype(&render_timestamp_reject) timestamp;
  decltype(&render_changed_reject) changed;
  decltype(&render_worthwhile_reject) worthwhile;
  decltype(&render_checkin_rendered) checkin_rendered;
};
static_assert(sizeof(AegpRenderSuite2) == 10 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, render_frame) == 0 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, checkin) == 1 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, get_world) == 2 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, get_region) == 3 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, sufficient) == 4 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, render_sound) == 5 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, timestamp) == 6 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, changed) == 7 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, worthwhile) == 8 * sizeof(void*));
static_assert(offsetof(AegpRenderSuite2, checkin_rendered) == 9 * sizeof(void*));

int32_t __cdecl adv_item_move_time_step(void* in_data, void* world,
                                        int32_t direction, int32_t steps);
int32_t __cdecl adv_item_move_time_step_active(int32_t direction, int32_t steps);
int32_t __cdecl adv_item_touch_active();
int32_t __cdecl adv_item_force_rerender(void* in_data, void* world);
int32_t __cdecl adv_item_effect_is_active(void* context_handle, uint8_t* enabled);

struct PfAdvItemSuite1 {
  decltype(&adv_item_move_time_step) move_time_step;
  decltype(&adv_item_move_time_step_active) move_time_step_active_item;
  decltype(&adv_item_touch_active) touch_active_item;
  decltype(&adv_item_force_rerender) force_rerender;
  decltype(&adv_item_effect_is_active) effect_is_active_or_enabled;
};
static_assert(sizeof(PfAdvItemSuite1) == 5 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, move_time_step) == 0 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, move_time_step_active_item) == 1 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, touch_active_item) == 2 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, force_rerender) == 3 * sizeof(void*));
static_assert(offsetof(PfAdvItemSuite1, effect_is_active_or_enabled) == 4 * sizeof(void*));
extern PfAdvItemSuite1 g_adv_item_suite1;

int32_t acquire_suite(const char*, int32_t, const void**);
int32_t release_suite(const char*, int32_t);

struct BasicSuite {
  decltype(&acquire_suite) acquire;
  decltype(&release_suite) release;
  void* unsupported[5]{};
};
extern BasicSuite g_basic_suite;

}  // namespace aexcompat::l2_detail
