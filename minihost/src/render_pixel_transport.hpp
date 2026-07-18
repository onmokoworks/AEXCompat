#pragma once

#include <cstdint>

namespace aexcompat::render_pixel_transport {

#if defined(AEXCOMPAT_RENDER_WORKER) || defined(AEXCOMPAT_SMART_WORKER)
void rgba8_to_argb(void* destination, const unsigned char* rgba,
                   int32_t pixel_bytes);
void argb_to_rgba8(unsigned char* rgba, const void* source,
                   int32_t pixel_bytes);
void argb_to_rgba_native(void* rgba, const void* source, int32_t pixel_bytes);
#endif

}  // namespace aexcompat::render_pixel_transport
