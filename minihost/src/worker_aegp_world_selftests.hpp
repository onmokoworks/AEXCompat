#pragma once

#include <cstdint>

namespace aexcompat::aegp_world_selftests {

struct Hooks {
  bool (*world_lifetimes_balanced)();
  void* (*comp_item_handle)();
  int32_t (*new_item_options)(void*, void**);
  int32_t (*timestamp)(void*);
  int32_t (*checkin_rendered)(void*, const void*, uint32_t, void*);
  int32_t (*worthwhile)(void*, const void*, uint8_t*);
  int32_t (*checkout_frame)(void*, void**);
  int32_t (*get_receipt_world)(void*, void***);
  int32_t (*checkin_frame)(void*);
  void (*bump_project_timestamp)();
  int32_t (*dispose_item_options)(void*);
  bool (*external_render_cache_empty)();

  void (*set_synthetic_receipt_mode)(bool);
  bool (*receipt_lifetimes_balanced)();
  int32_t (*publish_receipt)(int32_t, void**);
  int32_t (*insert_default_layer_options)(void**);
  int32_t (*checkout_layer_frame)(void*, void**);
  int32_t (*dispose_layer_options)(void*);

  // Optional, side-effect-free test diagnostics. Verification semantics remain boolean.
  void (*diagnostic)(const char*){};
};

bool verify_world_suite3(const Hooks& hooks);
bool verify_world_mfr_safety(const Hooks& hooks);
bool verify_async_receipts(const Hooks& hooks);

}  // namespace aexcompat::aegp_world_selftests
