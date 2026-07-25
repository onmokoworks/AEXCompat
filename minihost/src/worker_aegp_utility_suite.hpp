#pragma once

#include <cstdint>
#include <cstddef>

namespace aexcompat::l2_detail {

int32_t __cdecl register_with_aegp(void*, const char*, int32_t* plugin_id);
int32_t __cdecl get_main_hwnd(void* main_hwnd);

struct UtilitySuite {
  // Function pointer positions mirror the reviewed Adobe suite versions.
  // AEGP_UtilitySuite6 (acquisition version 13) publishes 33 slots; only
  // RegisterWithAEGP (slot 9) and GetMainHWND (slot 10) are supported. The
  // other ABI slots receive fail-closed diagnostic callbacks, so a plugin
  // that reaches one cannot jump through a null pointer without leaving a
  // version/slot record in the worker report.
  void* unsupported[9]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[22]{};
};
static_assert(sizeof(UtilitySuite) == 33 * sizeof(void*));
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
extern UtilitySuite1 g_utility_suite1;
extern UtilitySuite5 g_utility_suite5;

bool verify_suite_entry_guards_and_utility13();

}  // namespace aexcompat::l2_detail
