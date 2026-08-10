#pragma once

#include <atomic>
#include <cstdint>

namespace aexcompat::l2_detail {

bool supported_cpu_pixel_format(int32_t pixel_format);
int32_t __cdecl add_supported_pixel_format(void*, int32_t pixel_format);
int32_t __cdecl clear_supported_pixel_formats(void*);
int32_t __cdecl new_world_of_pixel_format(void*, uint32_t width,
                                           uint32_t height, int32_t flags,
                                           int32_t pixel_format, void* world);
int32_t __cdecl dispose_pixel_format_world(void*, void* world);
int32_t __cdecl get_pixel_format(const void* world, int32_t* pixel_format);
int32_t __cdecl get_black_for_pixel_format(int32_t pixel_format, void* pixel);
int32_t __cdecl get_white_for_pixel_format(int32_t pixel_format, void* pixel);
int32_t __cdecl convert_color_to_pixel_formatted_data(
    int32_t pixel_format, float alpha, float red, float green, float blue,
    void* pixel);

struct PixelFormatSuite1 {
  decltype(&add_supported_pixel_format) add_supported_pixel_format;
  decltype(&clear_supported_pixel_formats) clear_supported_pixel_formats;
  decltype(&new_world_of_pixel_format) new_world_of_pixel_format;
  decltype(&dispose_pixel_format_world) dispose_world;
  decltype(&get_pixel_format) get_pixel_format;
  decltype(&get_black_for_pixel_format) get_black_for_pixel_format;
  decltype(&get_white_for_pixel_format) get_white_for_pixel_format;
  decltype(&convert_color_to_pixel_formatted_data)
      convert_color_to_pixel_formatted_data;
};

struct PixelFormatSuite2 {
  decltype(&add_supported_pixel_format) add_supported_pixel_format;
  decltype(&clear_supported_pixel_formats) clear_supported_pixel_formats;
};

extern PixelFormatSuite1 g_pixel_format_suite1;
extern PixelFormatSuite2 g_pixel_format_suite2;

struct PixelFormatTelemetry {
  uint32_t add_calls{};
  uint32_t clear_calls{};
  uint32_t supported_count{};
  uint32_t invalid_operations{};
};

PixelFormatTelemetry pixel_format_telemetry();

// GLOBAL_SETUP scoping and diagnostics counters stay visible to the worker
// entry so dispatch can gate registration phases and reports can read the
// accounting without owning the registry.
extern std::atomic_bool g_global_setup_active;
extern std::atomic_uint32_t g_invalid_pixel_format_operations;

bool verify_pixel_format_registry_rejection();

}  // namespace aexcompat::l2_detail
