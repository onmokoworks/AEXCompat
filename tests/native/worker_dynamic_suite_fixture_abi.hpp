#pragma once

#include <cstdint>

struct FixtureBasicSuite {
  int32_t(__cdecl* acquire)(const char*, int32_t, const void**);
  int32_t(__cdecl* release)(const char*, int32_t);
};

struct FixtureSPSuitesSuite2 {
  void* allocate_list;
  void* free_list;
  int32_t(__cdecl* add)(void*, void*, const char*, int32_t, int32_t,
                        const void*, void**);
  int32_t(__cdecl* acquire)(void*, const char*, int32_t, int32_t,
                            const void**);
  int32_t(__cdecl* release)(void*, const char*, int32_t, int32_t);
};

inline constexpr char kFixtureSuiteName[] = "Fixture Companion Suite";
inline constexpr char kFixtureGatedAegpSuiteName[] =
    "Fixture Gated AEGP Host Suite";
