#pragma once

#include <atomic>
#include <cstdint>
#include <vector>

namespace aexcompat::l2_detail {

bool supported_cpu_pixel_format(int32_t pixel_format);
int32_t __cdecl add_supported_pixel_format(void*, int32_t pixel_format);
int32_t __cdecl clear_supported_pixel_formats(void*);

struct PixelFormatSuite {
  decltype(&add_supported_pixel_format) add_supported_pixel_format;
  decltype(&clear_supported_pixel_formats) clear_supported_pixel_formats;
};
extern PixelFormatSuite g_pixel_format_suite;

// GLOBAL_SETUP scoping and diagnostics counters stay visible to the worker
// entry so dispatch can gate registration phases and reports can read the
// accounting without owning the registry.
extern std::atomic_bool g_global_setup_active;
extern std::vector<int32_t> g_supported_pixel_formats;
extern uint32_t g_pixel_format_add_calls;
extern uint32_t g_pixel_format_clear_calls;
extern uint32_t g_invalid_pixel_format_operations;

bool verify_pixel_format_registry_rejection();

}  // namespace aexcompat::l2_detail
