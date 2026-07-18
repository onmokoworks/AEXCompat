#pragma once

#include <cstdint>

namespace aexcompat::render_pixel_transport {

void rgba8_to_argb(void* destination, const unsigned char* rgba,
                   int32_t pixel_bytes);
void argb_to_rgba8(unsigned char* rgba, const void* source,
                   int32_t pixel_bytes);
void argb_to_rgba_native(void* rgba, const void* source, int32_t pixel_bytes);

}  // namespace aexcompat::render_pixel_transport
