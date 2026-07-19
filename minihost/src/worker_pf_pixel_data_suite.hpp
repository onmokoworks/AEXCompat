#pragma once

#include <cstddef>
#include <cstdint>

namespace aexcompat::l2_detail {

int32_t __cdecl get_pixel_data8(void* world, void* pixels0, void** output);
int32_t __cdecl get_pixel_data16(void* world, void* pixels0, void** output);
int32_t __cdecl get_pixel_data_float(void* world, void* pixels0, void** output);
int32_t __cdecl get_pixel_data_float_gpu(void* world, void** output);

struct PixelDataSuite1 {
  decltype(&get_pixel_data8) get_pixel_data8;
  decltype(&get_pixel_data16) get_pixel_data16;
  decltype(&get_pixel_data_float) get_pixel_data_float;
};

struct PixelDataSuite2 {
  decltype(&get_pixel_data8) get_pixel_data8;
  decltype(&get_pixel_data16) get_pixel_data16;
  decltype(&get_pixel_data_float) get_pixel_data_float;
  decltype(&get_pixel_data_float_gpu) get_pixel_data_float_gpu;
};

static_assert(sizeof(PixelDataSuite1) == 3 * sizeof(void*));
static_assert(offsetof(PixelDataSuite1, get_pixel_data8) == 0 * sizeof(void*));
static_assert(offsetof(PixelDataSuite1, get_pixel_data16) == 1 * sizeof(void*));
static_assert(offsetof(PixelDataSuite1, get_pixel_data_float) == 2 * sizeof(void*));
static_assert(sizeof(PixelDataSuite2) == 4 * sizeof(void*));
static_assert(offsetof(PixelDataSuite2, get_pixel_data_float_gpu) == 3 * sizeof(void*));

extern PixelDataSuite1 g_pixel_data_suite1;
extern PixelDataSuite2 g_pixel_data_suite2;

bool verify_pixel_data_suites();

}  // namespace aexcompat::l2_detail
