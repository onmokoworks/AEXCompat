#pragma once
#include <cstdint>
struct PfSamplingHostHooks {
  bool (*resolve_world)(void*, int32_t, unsigned char*&, int32_t&, int32_t&, int32_t&){};
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  void* effect_ref{};
  void* batch_sampling_suite{};
};
void configure_pf_sampling_runtime(const PfSamplingHostHooks&) noexcept;

