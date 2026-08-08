#include "worker_aegp_utility_suite.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_suite_registry.hpp"

#include <cstring>
#include <string>

#include <windows.h>

namespace aexcompat::l2_detail {

int32_t acquire_suite(const char*, int32_t, const void**);
int32_t release_suite(const char*, int32_t);
uint32_t suite_acquire_count();
uint32_t suite_release_count();
uint32_t live_suite_reference_count();
bool suite_leases_balanced();
std::string unsupported_suite_calls_report_json();

namespace {
uint32_t g_main_hwnd_queries{};

template <aexcompat::worker_runtime::UnsupportedSuiteId Suite, std::size_t N>
void populate_unsupported_slots(void* destination) {
  const auto& slots = aexcompat::worker_runtime::unsupported_suite_slots<Suite, N>();
  std::memcpy(destination, slots.data(), sizeof(void*) * N);
}

UtilitySuite make_utility_suite13() {
  UtilitySuite suite{};
  populate_unsupported_slots<aexcompat::worker_runtime::UnsupportedSuiteId::aegp_utility_13,
                             33>(&suite);
  suite.register_with_aegp = &register_with_aegp;
  suite.get_main_hwnd = &get_main_hwnd;
  return suite;
}

UtilitySuite3 make_utility_suite7() {
  UtilitySuite3 suite{};
  populate_unsupported_slots<aexcompat::worker_runtime::UnsupportedSuiteId::aegp_utility_7,
                             25>(&suite);
  suite.register_with_aegp = &register_with_aegp;
  suite.get_main_hwnd = &get_main_hwnd;
  return suite;
}

UtilitySuite2 make_utility_suite5() {
  UtilitySuite2 suite{};
  populate_unsupported_slots<aexcompat::worker_runtime::UnsupportedSuiteId::aegp_utility_5,
                             19>(&suite);
  suite.register_with_aegp = &register_with_aegp;
  suite.get_main_hwnd = &get_main_hwnd;
  return suite;
}
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

UtilitySuite g_utility_suite = make_utility_suite13();
UtilitySuite3 g_utility_suite3 = make_utility_suite7();
UtilitySuite2 g_utility_suite2 = make_utility_suite5();
UtilitySuite1 g_utility_suite1{{}, &register_with_aegp, &get_main_hwnd};
UtilitySuite5 g_utility_suite5{{}, &register_with_aegp, &get_main_hwnd, {}};

UtilitySuiteSelftestResult verify_suite_entry_guards_and_utility13() {
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
  const void* utility7 = nullptr;
  const void* utility5 = nullptr;
  const void* rejected12 = reinterpret_cast<const void*>(1);
  const void* rejected14 = reinterpret_cast<const void*>(1);
  ok = acquire_suite("AEGP Utility Suite", 13, &utility13) == 0 &&
      acquire_suite("AEGP Utility Suite", 7, &utility7) == 0 &&
      acquire_suite("AEGP Utility Suite", 5, &utility5) == 0 &&
      acquire_suite("AEGP Utility Suite", 12, &rejected12) != 0 && rejected12 == nullptr &&
      acquire_suite("AEGP Utility Suite", 14, &rejected14) != 0 && rejected14 == nullptr && ok;
  aexcompat::mask_runtime::set_model_enabled(saved_mask_model_enabled);
  const auto* utility = static_cast<const UtilitySuite*>(utility13);
  const auto* utility3 = static_cast<const UtilitySuite3*>(utility7);
  const auto* utility2 = static_cast<const UtilitySuite2*>(utility5);
  HWND main_window = reinterpret_cast<HWND>(static_cast<uintptr_t>(0xCDCDCDCD));
  ok = ok && utility13 == &g_utility_suite && utility13 != &g_utility_suite3 && utility &&
      utility3 == &g_utility_suite3 &&
      utility2 == &g_utility_suite2 &&
      utility->unsupported[0] != nullptr && utility->unsupported_tail[0] != nullptr &&
      utility3->unsupported[0] != nullptr && utility3->unsupported_tail[0] != nullptr &&
      utility2->unsupported[0] != nullptr && utility2->unsupported_tail[0] != nullptr &&
      reinterpret_cast<int32_t(__cdecl*)()>(utility->unsupported[0])() == 4 &&
      reinterpret_cast<int32_t(__cdecl*)()>(utility3->unsupported[0])() == 4 &&
      reinterpret_cast<int32_t(__cdecl*)()>(utility->unsupported_tail[0])() == 4 &&
      reinterpret_cast<int32_t(__cdecl*)()>(utility3->unsupported_tail[0])() == 4 &&
      reinterpret_cast<int32_t(__cdecl*)()>(utility2->unsupported[0])() == 4 &&
      reinterpret_cast<int32_t(__cdecl*)()>(utility2->unsupported_tail[0])() == 4 &&
      utility->register_with_aegp == &register_with_aegp &&
      utility->get_main_hwnd == &get_main_hwnd &&
      utility2->register_with_aegp == &register_with_aegp &&
      utility2->get_main_hwnd == &get_main_hwnd &&
      utility->get_main_hwnd(nullptr) != 0 &&
      utility->get_main_hwnd(&main_window) == 0 &&
      main_window == GetDesktopWindow() &&
      release_suite("AEGP Utility Suite", 13) == 0 &&
      release_suite("AEGP Utility Suite", 7) == 0 &&
      release_suite("AEGP Utility Suite", 5) == 0;
  const std::string unsupported_report = unsupported_suite_calls_report_json();
  const bool unsupported_slots_diagnosed =
      unsupported_report.find("{\"name\":\"AEGP Utility Suite\",\"version\":13,\"slot\":0,\"call_count\":1}") != std::string::npos &&
      unsupported_report.find("{\"name\":\"AEGP Utility Suite\",\"version\":13,\"slot\":11,\"call_count\":1}") != std::string::npos &&
      unsupported_report.find("{\"name\":\"AEGP Utility Suite\",\"version\":7,\"slot\":0,\"call_count\":1}") != std::string::npos &&
      unsupported_report.find("{\"name\":\"AEGP Utility Suite\",\"version\":7,\"slot\":9,\"call_count\":1}") != std::string::npos;
  const bool utility5_slots_diagnosed =
      unsupported_report.find("{\"name\":\"AEGP Utility Suite\",\"version\":5,\"slot\":0,\"call_count\":1}") != std::string::npos &&
      unsupported_report.find("{\"name\":\"AEGP Utility Suite\",\"version\":5,\"slot\":9,\"call_count\":1}") != std::string::npos;
  const bool utility_v7_acquired = utility3 == &g_utility_suite3;
  const bool utility_v5_acquired = utility2 == &g_utility_suite2;
  const bool passed = ok && utility_v7_acquired && utility_v5_acquired &&
      unsupported_slots_diagnosed && utility5_slots_diagnosed &&
      suite_acquire_count() == acquires_before + 3 &&
      suite_release_count() == releases_before + 3 && suite_leases_balanced();
  return {passed, utility_v7_acquired, utility_v5_acquired,
          unsupported_slots_diagnosed && utility5_slots_diagnosed};
}

}  // namespace aexcompat::l2_detail
