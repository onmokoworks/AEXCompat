#pragma once
#include <cstdint>
struct PfSamplingHostHooks {
  bool (*resolve_world)(void*, int32_t, unsigned char*&, int32_t&, int32_t&, int32_t&){};
  int32_t (*acquire_suite)(const char*, int32_t, const void**){};
  int32_t (*release_suite)(const char*, int32_t){};
  void* effect_ref{};
  void* batch_sampling_suite{};
  // AE's private get_callback_addr ids (issue #985): -5 is PF.dll's
  // PFp_GaussianValue, -2 the FLT.dll in-place blur in its straight-alpha
  // (request mode 1) and premultiplied (any other mode) form. Left null, both
  // ids stay refused with the always-on `callback_addr_denied` marker.
  double(__cdecl* private_gaussian_value)(double){};
  int32_t(__cdecl* private_blur_straight)(void*, void*, double, void*, int32_t, void*){};
  int32_t(__cdecl* private_blur_premultiplied)(void*, void*, double, void*, int32_t, void*){};
};
void configure_pf_sampling_runtime(const PfSamplingHostHooks&) noexcept;

