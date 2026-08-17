#include "worker_dynamic_suite_fixture_abi.hpp"

#include <cstdint>

namespace {
int32_t g_death_calls{};
const FixtureBasicSuite* g_basic{};
int32_t __cdecl add_seven(int32_t* value) {
  if (!value) return 4;
  *value += 7;
  return 0;
}
void* g_table[]{reinterpret_cast<void*>(&add_seven)};
}  // namespace

extern "C" __declspec(dllexport) int32_t __cdecl FixtureAegpInitialize(
    const FixtureSPSuitesSuite2* suites) {
  if (!suites || !suites->add) return 4;
  return suites->add(nullptr, reinterpret_cast<void*>(1), kFixtureSuiteName,
                     1, 0, g_table, nullptr);
}

extern "C" __declspec(dllexport) int32_t __cdecl EntryPointFunc(
    void* basic_suite, int32_t, int32_t, int32_t, void** global_refcon) {
  if (global_refcon) *global_refcon = reinterpret_cast<void*>(1);
  const auto* basic = static_cast<const FixtureBasicSuite*>(basic_suite);
  if (!basic || !basic->acquire || !basic->release) return 4;
  g_basic = basic;
  const void* procedures{};
  const int32_t acquired = basic->acquire("SP Suites Suite", 2, &procedures);
  if (acquired != 0 || !procedures) return acquired == 0 ? 4 : acquired;
  const int32_t added = FixtureAegpInitialize(
      static_cast<const FixtureSPSuitesSuite2*>(procedures));
  const int32_t released = basic->release("SP Suites Suite", 2);
  return added != 0 ? added : released;
}

extern "C" __declspec(dllexport) int32_t __cdecl FixtureAegpDeath() {
  if (++g_death_calls != 1) return 4;
  if (!g_basic) return 0;
  const void* gated{};
  const int32_t acquired =
      g_basic->acquire(kFixtureGatedAegpSuiteName, 1, &gated);
  if (acquired != 0 || !gated) return acquired == 0 ? 4 : acquired;
  return g_basic->release(kFixtureGatedAegpSuiteName, 1);
}
