#include "worker_aegp_utility_suite.hpp"
#include "worker_mask_runtime.hpp"

#include <algorithm>
#include <cstring>
#include <iterator>

#include <windows.h>

namespace aexcompat::l2_detail {

int32_t acquire_suite(const char*, int32_t, const void**);
int32_t release_suite(const char*, int32_t);
uint32_t suite_acquire_count();
uint32_t suite_release_count();
uint32_t live_suite_reference_count();
bool suite_leases_balanced();

namespace {
uint32_t g_main_hwnd_queries{};
}

int32_t __cdecl register_with_aegp(void*, const char*, int32_t* plugin_id) {
  if (!plugin_id) return 4;
  *plugin_id = 1;
  return 0;
}

// AE always fills the caller-owned HWND storage. The headless worker has no
// application window, so the desktop window is published as the deterministic
// dialog parent instead of leaving the caller's buffer uninitialized.
int32_t __cdecl get_main_hwnd(void* main_hwnd) {
  if (!main_hwnd) return 4;
  ++g_main_hwnd_queries;
  const HWND desktop = GetDesktopWindow();
  std::memcpy(main_hwnd, &desktop, sizeof(desktop));
  return 0;
}

UtilitySuite g_utility_suite{{}, &register_with_aegp, &get_main_hwnd, {}};
UtilitySuite3 g_utility_suite3{{}, &register_with_aegp, &get_main_hwnd, {}};
UtilitySuite1 g_utility_suite1{{}, &register_with_aegp, &get_main_hwnd};
UtilitySuite5 g_utility_suite5{{}, &register_with_aegp, &get_main_hwnd, {}};

bool verify_suite_entry_guards_and_utility13() {
  const uint32_t acquires_before = suite_acquire_count();
  const uint32_t releases_before = suite_release_count();
  const uint32_t live_before = live_suite_reference_count();
  const void* acquired = reinterpret_cast<const void*>(1);
  bool ok = acquire_suite(nullptr, 13, &acquired) != 0 && acquired == nullptr &&
      acquire_suite("AEGP Utility Suite", 13, nullptr) != 0 &&
      release_suite(nullptr, 13) != 0 && suite_acquire_count() == acquires_before &&
      suite_release_count() == releases_before && live_suite_reference_count() == live_before;
  const bool saved_mask_model_enabled = aexcompat::mask_runtime::model_enabled();
  aexcompat::mask_runtime::set_model_enabled(false);
  const void* utility13 = nullptr;
  const void* rejected12 = reinterpret_cast<const void*>(1);
  const void* rejected14 = reinterpret_cast<const void*>(1);
  ok = acquire_suite("AEGP Utility Suite", 13, &utility13) == 0 &&
      acquire_suite("AEGP Utility Suite", 12, &rejected12) != 0 && rejected12 == nullptr &&
      acquire_suite("AEGP Utility Suite", 14, &rejected14) != 0 && rejected14 == nullptr && ok;
  aexcompat::mask_runtime::set_model_enabled(saved_mask_model_enabled);
  const auto* utility = static_cast<const UtilitySuite*>(utility13);
  HWND main_window = reinterpret_cast<HWND>(static_cast<uintptr_t>(0xCDCDCDCD));
  ok = ok && utility13 == &g_utility_suite && utility13 != &g_utility_suite3 && utility &&
      std::all_of(std::begin(utility->unsupported), std::end(utility->unsupported),
                  [](void* callback) { return callback == nullptr; }) &&
      std::all_of(std::begin(utility->unsupported_tail), std::end(utility->unsupported_tail),
                  [](void* callback) { return callback == nullptr; }) &&
      utility->register_with_aegp == &register_with_aegp &&
      utility->get_main_hwnd == &get_main_hwnd &&
      utility->get_main_hwnd(nullptr) != 0 &&
      utility->get_main_hwnd(&main_window) == 0 &&
      main_window == GetDesktopWindow() &&
      release_suite("AEGP Utility Suite", 13) == 0;
  return ok && suite_acquire_count() == acquires_before + 1 &&
      suite_release_count() == releases_before + 1 && suite_leases_balanced();
}

}  // namespace aexcompat::l2_detail
