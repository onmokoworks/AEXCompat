#include "worker_dynamic_suite_fixture_abi.hpp"

#include <cstdint>

extern "C" __declspec(dllexport) int32_t __cdecl FixturePfConsume(
    const FixtureBasicSuite* basic, int32_t* value) {
  if (!basic || !basic->acquire || !basic->release || !value) return 4;
  const void* suite{};
  const int32_t acquire = basic->acquire(kFixtureSuiteName, 1, &suite);
  if (acquire != 0 || !suite) return acquire == 0 ? 4 : acquire;
  const auto callback = reinterpret_cast<int32_t(__cdecl*)(int32_t*)>(
      static_cast<void* const*>(const_cast<void*>(suite))[0]);
  const int32_t callback_result = callback ? callback(value) : 4;
  const int32_t release = basic->release(kFixtureSuiteName, 1);
  return callback_result != 0 ? callback_result : release;
}
