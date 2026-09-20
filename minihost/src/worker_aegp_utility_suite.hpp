#pragma once

#include <cstdint>
#include <cstddef>

namespace aexcompat::l2_detail {

int32_t __cdecl register_with_aegp(void*, const char*, int32_t* plugin_id);
int32_t __cdecl get_main_hwnd(void* main_hwnd);
int32_t __cdecl start_undo_group(const char* name);
int32_t __cdecl end_undo_group();

struct UtilityUndoGroupStats {
  uint32_t starts{};
  uint32_t ends{};
  uint32_t invalid_operations{};
  uint32_t depth{};
};
UtilityUndoGroupStats utility_undo_group_stats();
bool utility_undo_groups_balanced();
bool utility_undo_group_operations_valid();
bool utility_undo_group_state_clean();
int32_t utility_undo_group_guarded_exit_code(int32_t candidate,
                                             int32_t failure_exit);
void reset_utility_undo_group_statistics();

struct UtilitySuite {
  // Function pointer positions mirror the reviewed Adobe suite versions.
  // AEGP_UtilitySuite6 (acquisition version 13) publishes 33 slots; only
  // StartUndoGroup/EndUndoGroup (slots 7/8), RegisterWithAEGP (slot 9), and
  // GetMainHWND (slot 10) are supported. The other ABI slots receive
  // fail-closed diagnostic callbacks, so a plugin
  // that reaches one cannot jump through a null pointer without leaving a
  // version/slot record in the worker report.
  void* unsupported[7]{};
  decltype(&start_undo_group) start_undo_group;
  decltype(&end_undo_group) end_undo_group;
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[22]{};
};
static_assert(sizeof(UtilitySuite) == 33 * sizeof(void*));
static_assert(offsetof(UtilitySuite, start_undo_group) == 7 * sizeof(void*));
static_assert(offsetof(UtilitySuite, end_undo_group) == 8 * sizeof(void*));
static_assert(offsetof(UtilitySuite, register_with_aegp) == 9 * sizeof(void*));
static_assert(offsetof(UtilitySuite, get_main_hwnd) == 10 * sizeof(void*));
struct UtilitySuite3 {
  // AEGP_UtilitySuite3 (acquisition version 7) publishes 25 slots with
  // RegisterWithAEGP at slot 7 and GetMainHWND at slot 8. The remaining
  // slots use the same fail-closed diagnostic boundary as version 13.
  void* unsupported[7]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[16]{};
};
static_assert(sizeof(UtilitySuite3) == 25 * sizeof(void*));
static_assert(offsetof(UtilitySuite3, register_with_aegp) == 7 * sizeof(void*));
static_assert(offsetof(UtilitySuite3, get_main_hwnd) == 8 * sizeof(void*));
struct UtilitySuite2 {
  // AEGP_UtilitySuite2 (acquisition version 5, frozen in AE 6.0) publishes
  // 19 slots. Its two host callbacks retain the Suite1 positions; the ten
  // added palette/floater functions remain fail-closed and diagnostic.
  void* unsupported[7]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[10]{};
};
static_assert(sizeof(UtilitySuite2) == 19 * sizeof(void*));
static_assert(offsetof(UtilitySuite2, register_with_aegp) == 7 * sizeof(void*));
static_assert(offsetof(UtilitySuite2, get_main_hwnd) == 8 * sizeof(void*));
struct UtilitySuite1 {
  // AEGP_UtilitySuite1 (acquisition version 3, frozen in AE 5.0) publishes 9
  // slots with RegisterWithAEGP at slot 7 and GetMainHWND at slot 8 (issue
  // #362: the Liquify family acquires exactly version 3).
  void* unsupported[7]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
};
static_assert(sizeof(UtilitySuite1) == 9 * sizeof(void*));
static_assert(offsetof(UtilitySuite1, register_with_aegp) == 7 * sizeof(void*));
static_assert(offsetof(UtilitySuite1, get_main_hwnd) == 8 * sizeof(void*));
struct UtilitySuite5 {
  // AEGP_UtilitySuite5 (acquisition version 11, frozen in AE 8.0) publishes
  // 31 slots with RegisterWithAEGP at slot 8 and GetMainHWND at slot 9
  // (issue #362: Cryptomatte acquires exactly version 11).
  void* unsupported[8]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[21]{};
};
static_assert(sizeof(UtilitySuite5) == 31 * sizeof(void*));
static_assert(offsetof(UtilitySuite5, register_with_aegp) == 8 * sizeof(void*));
static_assert(offsetof(UtilitySuite5, get_main_hwnd) == 9 * sizeof(void*));

extern UtilitySuite g_utility_suite;
extern UtilitySuite3 g_utility_suite3;
extern UtilitySuite2 g_utility_suite2;
extern UtilitySuite1 g_utility_suite1;
extern UtilitySuite5 g_utility_suite5;

struct UtilitySuiteSelftestResult {
  bool passed{};
  bool utility_v7_acquired{};
  bool utility_v5_acquired{};
  bool unsupported_slots_diagnosed{};
};

UtilitySuiteSelftestResult verify_suite_entry_guards_and_utility13();

}  // namespace aexcompat::l2_detail
