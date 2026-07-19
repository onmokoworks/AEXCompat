#include "worker_pf_pixel_data_suite.hpp"

#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"

#include <array>
#include <cstdlib>
#include <cstring>
#include <limits>

namespace aexcompat::l2_detail {

using world_registry::kPixelFormatArgb32;
using world_registry::kPixelFormatArgb64;
using world_registry::kPixelFormatArgb128;
using world_registry::kPixelFormatGpuBgra128;
using world_safety::DispatchWorldFormat;
using world_safety::kEffectWorldSize;

namespace {

int32_t get_typed_pixel_data(void* world, void* pixels0, void** output,
                             int32_t required_format, int32_t pixel_bytes) {
  if (!output) return 4;
  *output = nullptr;
  if (!world) return 4;
  DispatchWorldFormat resolved{};
  if (!world_registry::resolve_dispatch_world_format(world, resolved)) return 4;
  const int32_t format = resolved.pixel_format;
  const int32_t rowbytes = resolved.rowbytes;
  const int32_t width = resolved.width;
  const int32_t height = resolved.height;
  if (format != required_format) return 0;
  if (pixel_bytes <= 0 || width <= 0 || height <= 0 ||
      width > 4096 || height > 4096 ||
      rowbytes == (std::numeric_limits<int32_t>::min)() ||
      std::abs(rowbytes) < width * pixel_bytes) return 4;
  void* data = pixels0 ? pixels0 : resolved.data;
  if (!data) return 4;
  *output = data;
  return 0;
}

}  // namespace

int32_t __cdecl get_pixel_data8(void* world, void* pixels0, void** output) {
  return get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb32, 4);
}

int32_t __cdecl get_pixel_data16(void* world, void* pixels0, void** output) {
  return get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb64, 8);
}

int32_t __cdecl get_pixel_data_float(void* world, void* pixels0, void** output) {
  return get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb128, 16);
}

int32_t __cdecl get_pixel_data_float_gpu(void* world, void** output) {
  return get_typed_pixel_data(world, nullptr, output, kPixelFormatGpuBgra128, 16);
}

PixelDataSuite1 g_pixel_data_suite1{
    &get_pixel_data8, &get_pixel_data16, &get_pixel_data_float};
PixelDataSuite2 g_pixel_data_suite2{
    &get_pixel_data8, &get_pixel_data16, &get_pixel_data_float,
    &get_pixel_data_float_gpu};

bool verify_pixel_data_suites() {
  const std::array<int32_t, 4> formats{
      kPixelFormatArgb32, kPixelFormatArgb64, kPixelFormatArgb128,
      kPixelFormatGpuBgra128};
  std::array<std::array<std::byte, kEffectWorldSize>, 4> worlds{};
  std::array<void*, 4> world_pixels{};
  bool valid = true;
  std::size_t created = 0;
  for (; created < formats.size(); ++created) {
    if (world_registry::new_world(nullptr, 3, 2, 1, formats[created],
                                  worlds[created].data()) != 0) {
      valid = false;
      break;
    }
    std::memcpy(&world_pixels[created], worlds[created].data() + 24,
                sizeof(world_pixels[created]));
  }
  if (created == formats.size()) {
    void* output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data8(
                worlds[0].data(), nullptr, &output) == 0 &&
            output == world_pixels[0] && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data16(
                worlds[1].data(), nullptr, &output) == 0 &&
            output == world_pixels[1] && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data_float(
                worlds[2].data(), nullptr, &output) == 0 &&
            output == world_pixels[2] && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite2.get_pixel_data_float_gpu(
                worlds[3].data(), &output) == 0 &&
            output == world_pixels[3] && valid;

    std::array<std::byte, 16> alternate_pixels{};
    output = nullptr;
    valid = g_pixel_data_suite2.get_pixel_data8(
                worlds[0].data(), alternate_pixels.data(), &output) == 0 &&
            output == alternate_pixels.data() && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite2.get_pixel_data_float(
                worlds[3].data(), nullptr, &output) == 0 &&
            output == nullptr && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite2.get_pixel_data_float_gpu(
                worlds[2].data(), &output) == 0 &&
            output == nullptr && valid;

    std::array<std::byte, kEffectWorldSize> unregistered = worlds[0];
    void* unknown_pixels = alternate_pixels.data();
    std::memcpy(unregistered.data() + 24, &unknown_pixels, sizeof(unknown_pixels));
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data8(
                unregistered.data(), nullptr, &output) != 0 &&
            output == nullptr && valid;
    output = reinterpret_cast<void*>(1);
    valid = g_pixel_data_suite1.get_pixel_data8(nullptr, nullptr, &output) != 0 &&
            output == nullptr && valid;
  }
  while (created > 0) {
    --created;
    valid = world_registry::dispose_world(nullptr, worlds[created].data()) == 0 && valid;
  }
  return valid && world_registry::lifetimes_balanced();
}

}  // namespace aexcompat::l2_detail
