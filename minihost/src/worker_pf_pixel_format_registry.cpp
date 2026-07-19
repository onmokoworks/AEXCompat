#include "worker_pf_pixel_format_registry.hpp"

#include "worker_mask_runtime_internal.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <mutex>

namespace aexcompat::l2_detail {

using world_registry::kPixelFormatArgb32;
using world_registry::kPixelFormatArgb64;
using world_registry::kPixelFormatArgb128;

extern OpaqueHostObject g_effect;

namespace {
std::mutex g_pixel_format_mutex;
}

std::vector<int32_t> g_supported_pixel_formats;
uint32_t g_pixel_format_add_calls{};
uint32_t g_pixel_format_clear_calls{};
uint32_t g_invalid_pixel_format_operations{};
std::atomic_bool g_global_setup_active{false};

bool supported_cpu_pixel_format(int32_t pixel_format) {
  return pixel_format == kPixelFormatArgb32 || pixel_format == kPixelFormatArgb64 ||
      pixel_format == kPixelFormatArgb128;
}

int32_t __cdecl add_supported_pixel_format(void*, int32_t pixel_format) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active || !supported_cpu_pixel_format(pixel_format)) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  ++g_pixel_format_add_calls;
  if (std::find(g_supported_pixel_formats.begin(), g_supported_pixel_formats.end(),
                pixel_format) == g_supported_pixel_formats.end()) {
    g_supported_pixel_formats.push_back(pixel_format);
  }
  return 0;
}

int32_t __cdecl clear_supported_pixel_formats(void*) {
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  if (!g_global_setup_active) {
    ++g_invalid_pixel_format_operations;
    return 4;
  }
  g_supported_pixel_formats.clear();
  ++g_pixel_format_clear_calls;
  return 0;
}

PixelFormatSuite g_pixel_format_suite{&add_supported_pixel_format,
                                      &clear_supported_pixel_formats};

bool verify_pixel_format_registry_rejection() {
  const uint32_t invalid_before = g_invalid_pixel_format_operations;
  const uint32_t add_before = g_pixel_format_add_calls;
  const uint32_t clear_before = g_pixel_format_clear_calls;
  const bool phase_rejected = clear_supported_pixel_formats(&g_effect) != 0;
  g_global_setup_active = true;
  if (clear_supported_pixel_formats(&g_effect) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb128) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb64) != 0 ||
      add_supported_pixel_format(&g_effect, kPixelFormatArgb128) != 0) {
    g_global_setup_active = false;
    return false;
  }
  bool order_valid = false;
  {
    std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
    order_valid = g_supported_pixel_formats ==
        std::vector<int32_t>{kPixelFormatArgb128, kPixelFormatArgb64};
  }
  const bool rejected = add_supported_pixel_format(&g_effect, 1717854562) != 0;
  const bool cleared = clear_supported_pixel_formats(&g_effect) == 0;
  g_global_setup_active = false;
  std::lock_guard<std::mutex> lock(g_pixel_format_mutex);
  return phase_rejected && order_valid && rejected && cleared &&
      g_supported_pixel_formats.empty() &&
      g_invalid_pixel_format_operations == invalid_before + 2 &&
      g_pixel_format_add_calls == add_before + 3 &&
      g_pixel_format_clear_calls == clear_before + 2;
}

}  // namespace aexcompat::l2_detail
