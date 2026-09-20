#include "worker_aegp_utility_suite.hpp"
#include "worker_mask_runtime.hpp"
#include "worker_suite_registry.hpp"

#include <cstring>
#include <mutex>
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
std::mutex g_undo_group_mutex;
UtilityUndoGroupStats g_undo_group_stats;
constexpr uint32_t kMaxUndoGroupDepth = 32;

template <aexcompat::worker_runtime::UnsupportedSuiteId Suite, std::size_t N>
void populate_unsupported_slots(void* destination) {
  const auto& slots = aexcompat::worker_runtime::unsupported_suite_slots<Suite, N>();
  std::memcpy(destination, slots.data(), sizeof(void*) * N);
}

UtilitySuite make_utility_suite13() {
  UtilitySuite suite{};
  populate_unsupported_slots<aexcompat::worker_runtime::UnsupportedSuiteId::aegp_utility_13,
                             33>(&suite);
  suite.start_undo_group = &start_undo_group;
  suite.end_undo_group = &end_undo_group;
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

int32_t __cdecl start_undo_group(const char* name) {
  const std::size_t name_length = name ? strnlen_s(name, 256) : 0;
  if (!name || name_length == 0 || name_length == 256) {
    std::lock_guard<std::mutex> lock(g_undo_group_mutex);
    ++g_undo_group_stats.invalid_operations;
    return 4;
  }
  std::lock_guard<std::mutex> lock(g_undo_group_mutex);
  if (g_undo_group_stats.depth >= kMaxUndoGroupDepth) {
    ++g_undo_group_stats.invalid_operations;
    return 4;
  }
  ++g_undo_group_stats.starts;
  ++g_undo_group_stats.depth;
  return 0;
}

int32_t __cdecl end_undo_group() {
  std::lock_guard<std::mutex> lock(g_undo_group_mutex);
  if (g_undo_group_stats.depth == 0) {
    ++g_undo_group_stats.invalid_operations;
    return 4;
  }
  ++g_undo_group_stats.ends;
  --g_undo_group_stats.depth;
  return 0;
}

UtilityUndoGroupStats utility_undo_group_stats() {
  std::lock_guard<std::mutex> lock(g_undo_group_mutex);
  return g_undo_group_stats;
}

bool utility_undo_groups_balanced() {
  const auto stats = utility_undo_group_stats();
  return stats.depth == 0 && stats.starts == stats.ends;
}

bool utility_undo_group_operations_valid() {
  return utility_undo_group_stats().invalid_operations == 0;
}

bool utility_undo_group_state_clean() {
  const auto stats = utility_undo_group_stats();
  return stats.depth == 0 && stats.starts == stats.ends &&
      stats.invalid_operations == 0;
}

int32_t utility_undo_group_guarded_exit_code(int32_t candidate,
                                             int32_t failure_exit) {
  return candidate == 0 && !utility_undo_group_state_clean()
      ? failure_exit : candidate;
}

void reset_utility_undo_group_statistics() {
  std::lock_guard<std::mutex> lock(g_undo_group_mutex);
  g_undo_group_stats = {};
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
  reset_utility_undo_group_statistics();
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
  const std::string overlong_name(256, 'x');
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
      utility->start_undo_group == &start_undo_group &&
      utility->end_undo_group == &end_undo_group &&
      utility->get_main_hwnd == &get_main_hwnd &&
      utility2->register_with_aegp == &register_with_aegp &&
      utility2->get_main_hwnd == &get_main_hwnd &&
      utility->get_main_hwnd(nullptr) != 0 &&
      utility->start_undo_group(nullptr) != 0 &&
      utility->start_undo_group("") != 0 &&
      utility->start_undo_group(overlong_name.c_str()) != 0 &&
      utility_undo_group_stats().invalid_operations == 3 &&
      utility_undo_group_stats().depth == 0 &&
      utility->start_undo_group("utility outer") == 0 &&
      utility->start_undo_group("utility inner") == 0 &&
      utility_undo_group_stats().depth == 2 &&
      !utility_undo_groups_balanced() &&
      utility->end_undo_group() == 0 &&
      utility->end_undo_group() == 0 && utility_undo_groups_balanced() &&
      utility->end_undo_group() != 0 &&
      utility_undo_group_stats().invalid_operations == 4 &&
      utility_undo_group_stats().depth == 0 &&
      utility->get_main_hwnd(&main_window) == 0 &&
      main_window == GetDesktopWindow() &&
      release_suite("AEGP Utility Suite", 13) == 0 &&
      release_suite("AEGP Utility Suite", 7) == 0 &&
      release_suite("AEGP Utility Suite", 5) == 0;
  const auto ordinary_undo_stats = utility_undo_group_stats();
  const bool ordinary_undo_contract =
      ordinary_undo_stats.starts == 2 && ordinary_undo_stats.ends == 2 &&
      ordinary_undo_stats.invalid_operations == 4 &&
      ordinary_undo_stats.depth == 0;

  reset_utility_undo_group_statistics();
  bool maximum_depth_entered = true;
  for (uint32_t depth = 0; depth < kMaxUndoGroupDepth; ++depth) {
    maximum_depth_entered =
        start_undo_group("utility depth guard") == 0 && maximum_depth_entered;
  }
  const bool excessive_depth_rejected =
      start_undo_group("utility excessive depth") != 0;
  const auto maximum_depth_stats = utility_undo_group_stats();
  bool maximum_depth_left = true;
  for (uint32_t depth = 0; depth < kMaxUndoGroupDepth; ++depth) {
    maximum_depth_left = end_undo_group() == 0 && maximum_depth_left;
  }
  const auto final_depth_stats = utility_undo_group_stats();
  const bool maximum_depth_contract =
      maximum_depth_entered && excessive_depth_rejected &&
      maximum_depth_stats.starts == kMaxUndoGroupDepth &&
      maximum_depth_stats.ends == 0 &&
      maximum_depth_stats.invalid_operations == 1 &&
      maximum_depth_stats.depth == kMaxUndoGroupDepth &&
      maximum_depth_left && final_depth_stats.starts == kMaxUndoGroupDepth &&
      final_depth_stats.ends == kMaxUndoGroupDepth &&
      final_depth_stats.invalid_operations == 1 && final_depth_stats.depth == 0 &&
      utility_undo_groups_balanced();
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
      ordinary_undo_contract && maximum_depth_contract &&
      suite_acquire_count() == acquires_before + 3 &&
      suite_release_count() == releases_before + 3 && suite_leases_balanced();
  reset_utility_undo_group_statistics();
  return {passed, utility_v7_acquired, utility_v5_acquired,
          unsupported_slots_diagnosed && utility5_slots_diagnosed};
}

}  // namespace aexcompat::l2_detail
