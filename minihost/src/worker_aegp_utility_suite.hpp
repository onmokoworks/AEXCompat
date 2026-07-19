#pragma once

#include <cstdint>
#include <cstddef>

namespace aexcompat::l2_detail {

int32_t __cdecl register_with_aegp(void*, const char*, int32_t* plugin_id);
int32_t __cdecl get_main_hwnd(void* main_hwnd);

struct UtilitySuite {
  // Function pointer positions mirror the reviewed Adobe suite versions.
  // AEGP_UtilitySuite6 (acquisition version 13) publishes 33 slots; only
  // RegisterWithAEGP (slot 9) and GetMainHWND (slot 10) are supported, the
  // rest stay null so out-of-range reads fail closed instead of leaving the
  // table shorter than the ABI the effect compiled against.
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
  // RegisterWithAEGP at slot 7 and GetMainHWND at slot 8.
  void* unsupported[7]{};
  decltype(&register_with_aegp) register_with_aegp;
  decltype(&get_main_hwnd) get_main_hwnd;
  void* unsupported_tail[16]{};
};
static_assert(sizeof(UtilitySuite3) == 25 * sizeof(void*));
static_assert(offsetof(UtilitySuite3, register_with_aegp) == 7 * sizeof(void*));
static_assert(offsetof(UtilitySuite3, get_main_hwnd) == 8 * sizeof(void*));

extern UtilitySuite g_utility_suite;
extern UtilitySuite3 g_utility_suite3;

bool verify_suite_entry_guards_and_utility13();

}  // namespace aexcompat::l2_detail
