#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::compute_cache {

// AEGP Compute Cache Suite v1 ("AEGP Compute Cache", frozen in AE 18.2).
// Bundled histogram-class effects (Auto Color / Auto Contrast / Auto Levels /
// Levels / Shadow-Highlight, issue #362 selector families) register a
// compute class from GLOBAL_SETUP and abort with "Not able to acquire AEFX
// Suite." when the suite is missing.

struct AegpGuid {
  int32_t bytes[4];
};

struct AegpComputeCacheCallbacks {
  int32_t(__cdecl* generate_key)(void* options, AegpGuid* out_key);
  int32_t(__cdecl* compute)(void* options, void** out_value);
  size_t(__cdecl* approx_size_value)(void* value);
  void(__cdecl* delete_compute_value)(void* value);
};

struct AegpComputeCacheSuite1 {
  int32_t(__cdecl* class_register)(const char* compute_class,
                                   const AegpComputeCacheCallbacks* callbacks);
  int32_t(__cdecl* class_unregister)(const char* compute_class);
  int32_t(__cdecl* compute_if_needed_and_checkout)(const char* compute_class,
                                                   void* opaque_options,
                                                   uint8_t wait_for_other_thread,
                                                   void** compute_receipt);
  int32_t(__cdecl* checkout_cached)(const char* compute_class,
                                    void* opaque_options,
                                    void** compute_receipt);
  int32_t(__cdecl* get_receipt_compute_value)(const void* compute_receipt,
                                              void** compute_value);
  int32_t(__cdecl* checkin_compute_receipt)(void* compute_receipt);
};

static_assert(sizeof(AegpComputeCacheSuite1) == 6 * sizeof(void*));

AegpComputeCacheSuite1& suite_table();

// Cluster-session reset (issue #405 parity with reset_cluster_effect_state):
// the registered classes and cached values belong to the previous plug-in's
// code, so the whole registry is purged through the plug-in's delete
// callbacks before the next plug-in is bootstrapped.
void purge_registry();

// Self-test hook: exercises register/checkout/checkin/purge against a fake
// compute class without any plug-in loaded.
bool selftest();

}  // namespace aexcompat::compute_cache
