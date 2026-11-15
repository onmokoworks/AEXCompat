// PF_UtilCallbacks.new_world takes a PF_NewWorldFlags, and a plug-in may put
// something else there. F's OpticalDiffusion (public source and binaries at
// github.com/bryful/F-s-PluginsProjects) passes a PF_PixelFormat instead, so
// the call arrives with 0x62677261 ('bgra', PF_PixelFormat_ARGB32) in the flags
// parameter. This host used to fail such a call closed with
// PF_Err_OUT_OF_MEMORY; the plug-in does not read the result, so it went on to
// use a PF_EffectWorld left untouched and took an access violation inside its
// own render (SEH 512) on a 256x144 argb8 frame, in classic and SmartFX alike.
//
// AE does not refuse it: that plug-in renders there, and the low two bits of
// what it passes are CLEAR_PIXELS set and DEEP_PIXELS clear - the cleared
// 8-bit world its 8-bit render then writes. So the contract checked here is
// that legacy_new_world reads the bits the SDK defines and ignores the rest,
// while still failing closed on everything allocation is actually allowed to
// refuse.
//
// This drives the registry directly, so it needs no plug-in, no AEX and no
// worker process.

#include "worker_world_registry.hpp"
#include "worker_world_safety.hpp"
#include "trace_writer.hpp"
#include "gpu_memory_world_transport.hpp"

#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>

namespace aexcompat::l2_detail { aexcompat::TraceWriter* g_trace_writer{}; }
// This test drives the CPU registry alone and never bootstraps GPU transport.
// Fail loudly if this unused linkage boundary is reached; it must not become
// fake GPU evidence.
namespace aexcompat::gpu_runtime::memory_world_transport {
void configure_host_world_fallback(HostNewWorld, HostDisposeWorld, HostOwnsWorld,
                                   HostRecognizesSmartWorld) { std::abort(); }
}

using aexcompat::world_registry::dispose_world;
using aexcompat::world_registry::kPixelFormatArgb32;
using aexcompat::world_registry::kPixelFormatArgb64;
using aexcompat::world_registry::legacy_new_world;
using aexcompat::world_safety::kEffectWorldSize;

namespace {

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

// AE_EffectCB.h's PF_NewWorldFlags: the two bits that select anything, then
// the two the host reserves for itself.
constexpr int32_t kClearPixels = 1 << 0;
constexpr int32_t kDeepPixels = 1 << 1;
constexpr int32_t kReserved0 = 1 << 2;
constexpr int32_t kReserved1 = 1 << 3;

// What F's OpticalDiffusion puts in the flags parameter: a PF_PixelFormat,
// which is MAKE_PIXEL_FORMAT_FOURCC('a','r','g','b') for ARGB32. Its low two
// bits are CLEAR_PIXELS set, DEEP_PIXELS clear.
constexpr int32_t kPixelFormatInFlagsSlot = 0x62677261;

struct World {
  alignas(8) std::array<std::byte, kEffectWorldSize> bytes{};

  void* data() { return bytes.data(); }

  void* pixels() const {
    void* value{};
    std::memcpy(&value, bytes.data() + 24, sizeof(value));
    return value;
  }
  int32_t rowbytes() const { return read<int32_t>(32); }
  int32_t width() const { return read<int32_t>(36); }
  int32_t height() const { return read<int32_t>(40); }

 private:
  template <typename T>
  T read(std::size_t offset) const {
    T value{};
    std::memcpy(&value, bytes.data() + offset, sizeof(value));
    return value;
  }
};

// A world the call is expected to have filled in at the requested geometry and
// pixel size, cleared or not as asked.
void check_allocated(World& world, const char* what, int32_t expected_width,
                     int32_t expected_height, int32_t expected_pixel_bytes,
                     bool expect_cleared) {
  char label[192];
  const auto describe = [&](const char* detail) -> const char* {
    std::snprintf(label, sizeof(label), "%s: %s", what, detail);
    return label;
  };
  if (!world.pixels()) {
    check(false, describe("no pixels"));
    return;
  }
  check(world.width() == expected_width, describe("width"));
  check(world.height() == expected_height, describe("height"));
  const int64_t tight =
      static_cast<int64_t>(expected_width) * expected_pixel_bytes;
  check(world.rowbytes() >= tight, describe("rowbytes covers the row"));
  if (expect_cleared) {
    const auto* first = static_cast<const unsigned char*>(world.pixels());
    bool cleared = true;
    for (int64_t index = 0; index < tight; ++index)
      if (first[index] != 0) cleared = false;
    check(cleared, describe("CLEAR_PIXELS cleared the first row"));
  }
  check(dispose_world(nullptr, world.data()) == 0, describe("disposes"));
}

// The case that regressed: a PF_PixelFormat in the flags parameter still
// allocates, and reads as CLEAR_PIXELS set and DEEP_PIXELS clear, so the world
// is the cleared 8-bit one an 8-bit render goes on to write.
void pixel_format_in_the_flags_slot_allocates_a_cleared_8_bit_world() {
  World world;
  check(legacy_new_world(nullptr, 36, 8, kPixelFormatInFlagsSlot,
                         world.data()) == 0,
        "a PF_PixelFormat in the flags slot is not refused");
  check_allocated(world, "pixel format in flags slot", 36, 8, 4, true);
}

// The same request at the other geometry the plug-in asks for, so the case is
// not pinned to one shape.
void the_second_geometry_the_plug_in_asks_for_allocates_too() {
  World world;
  check(legacy_new_world(nullptr, 128, 4, kPixelFormatInFlagsSlot,
                         world.data()) == 0,
        "128x4 with a pixel format in the flags slot is not refused");
  check_allocated(world, "second geometry", 128, 4, 4, true);
}

// Ignoring the undefined bits may not change what the defined ones select.
void the_defined_bits_still_select_depth_and_clearing() {
  World shallow;
  check(legacy_new_world(nullptr, 4, 3, kClearPixels, shallow.data()) == 0,
        "CLEAR_PIXELS alone allocates");
  check_allocated(shallow, "CLEAR_PIXELS", 4, 3, 4, true);

  World deep;
  check(legacy_new_world(nullptr, 4, 3, kClearPixels | kDeepPixels,
                         deep.data()) == 0,
        "CLEAR_PIXELS|DEEP_PIXELS allocates");
  check_allocated(deep, "DEEP_PIXELS", 4, 3, 8, true);

  World none;
  check(legacy_new_world(nullptr, 4, 3, 0, none.data()) == 0,
        "PF_NewWorldFlag_NONE allocates");
  check_allocated(none, "NONE", 4, 3, 4, false);

  // An undefined bit riding along with DEEP_PIXELS must not cost the depth.
  World deep_with_noise;
  check(legacy_new_world(nullptr, 4, 3,
                         kDeepPixels | kReserved0 | kReserved1 | (1 << 30),
                         deep_with_noise.data()) == 0,
        "DEEP_PIXELS survives undefined bits");
  check_allocated(deep_with_noise, "DEEP_PIXELS with undefined bits", 4, 3, 8,
                  false);
}

// What PF_Err_OUT_OF_MEMORY from here still means. Relaxing the flags may not
// relax any of this.
void allocation_still_fails_closed() {
  World world;
  check(legacy_new_world(nullptr, 0, 8, kClearPixels, world.data()) != 0,
        "zero width is refused");
  check(legacy_new_world(nullptr, 36, 0, kClearPixels, world.data()) != 0,
        "zero height is refused");
  check(legacy_new_world(nullptr, -36, 8, kClearPixels, world.data()) != 0,
        "negative width is refused");
  check(legacy_new_world(nullptr, 36, -8, kClearPixels, world.data()) != 0,
        "negative height is refused");
  check(legacy_new_world(nullptr, 36, 8, kClearPixels, nullptr) != 0,
        "a null world struct is refused");

  // Past the registry's byte ceiling, and a row too wide to measure.
  check(legacy_new_world(nullptr, 65536, 65536, kClearPixels, world.data()) != 0,
        "an allocation past the byte ceiling is refused");
  check(legacy_new_world(nullptr, (std::numeric_limits<int32_t>::max)(), 2,
                         kDeepPixels, world.data()) != 0,
        "a row wider than int32 is refused");
}

}  // namespace

int main() {
  pixel_format_in_the_flags_slot_allocates_a_cleared_8_bit_world();
  the_second_geometry_the_plug_in_asks_for_allocates_too();
  the_defined_bits_still_select_depth_and_clearing();
  allocation_still_fails_closed();

  if (failures != 0) {
    std::fprintf(stderr, "%d check(s) failed\n", failures);
    return 1;
  }
  std::fprintf(stderr, "worker_legacy_new_world_flags_selftest: ok\n");
  return 0;
}
