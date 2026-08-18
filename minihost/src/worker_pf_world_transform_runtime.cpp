#include "worker_pf_world_transform_runtime.hpp"

#include "worker_extended_diag.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <iostream>
#include <string>
#include <thread>
#include <type_traits>
#include <vector>

namespace aexcompat::pf_world_transform {
namespace {

using world_safety::DispatchWorldFormat;
using world_safety::DispatchWorldFormatScope;
using world_safety::kEffectWorldSize;

constexpr int32_t kPfErrBadCallbackParam = 516;
constexpr int32_t kPixelFormatArgb32 = 1650946657;
using aexcompat::world_registry::kPixelFormatArgb64;
using aexcompat::world_registry::kPixelFormatArgb128;
constexpr uint64_t kMaxAsyncReceiptBytes = 64ULL * 1024 * 1024;
constexpr std::size_t kUtilsSize = 552;
constexpr std::size_t kUtilsFill = 72;
constexpr std::size_t kUtilsPremultiply = 96;
constexpr std::size_t kUtilsPremultiplyColor = 104;
constexpr std::size_t kUtilsFill16 = 488;
constexpr std::size_t kUtilsPremultiplyColor16 = 496;

Context g_context{};
bool g_configured{};
std::atomic<bool> g_fail_next_allocation_for_self_test{};

bool resolve_world(void* world, int32_t pixel_bytes, unsigned char*& pixels,
                   int32_t& rowbytes, int32_t& width, int32_t& height) {
  return g_configured && g_context.hooks.resolve_world &&
      g_context.hooks.resolve_world(world, pixel_bytes, pixels, rowbytes, width, height);
}

bool resolve_dispatch_world_format(const void* world, DispatchWorldFormat& result) {
  return g_configured && g_context.hooks.resolve_dispatch_world_format &&
      g_context.hooks.resolve_dispatch_world_format(world, result);
}

const char* pixel_format() {
  return g_configured && g_context.hooks.pixel_format ? g_context.hooks.pixel_format() : "";
}

bool set_pixel_format(const char* value) {
  return g_configured && g_context.hooks.set_pixel_format &&
      g_context.hooks.set_pixel_format(value);
}

bool bounded_argb8(void* world, unsigned char*& pixels, int32_t& rowbytes,
                   int32_t& width, int32_t& height) {
  return g_configured && g_context.hooks.bounded_argb8_world &&
      g_context.hooks.bounded_argb8_world(world, pixels, rowbytes, width, height);
}

// The two rectangles a copy actually runs over, clipped together so the
// correspondence between them survives: source (left+i, top+j) goes to
// destination (left+i, top+j), so whatever is trimmed off one leading edge is
// trimmed off the other, and the two extents end up equal.
//
// False only when a world has no pixels to copy between; a pair that shares no
// region clips to empty, which the caller answers as a copy of nothing. Both
// results are inside their worlds, which is what lets the memcpys below index
// with them.
bool copy_correspondence(const LegacyRect* source_rect,
                         const DispatchWorldFormat& source,
                         const LegacyRect* destination_rect,
                         const DispatchWorldFormat& destination,
                         LegacyRect& src, LegacyRect& dst) {
  if (source.width <= 0 || source.height <= 0 || destination.width <= 0 ||
      destination.height <= 0)
    return false;
  const LegacyRect wanted_src =
      source_rect ? *source_rect : LegacyRect{0, 0, source.width, source.height};
  const LegacyRect wanted_dst = destination_rect
      ? *destination_rect
      : LegacyRect{0, 0, destination.width, destination.height};
  // In 64-bit arithmetic: a caller's rectangle is not bounded, and the shifts
  // below add and subtract across it.
  const int64_t lead_x = std::max<int64_t>({0, -static_cast<int64_t>(wanted_src.left),
                                            -static_cast<int64_t>(wanted_dst.left)});
  const int64_t lead_y = std::max<int64_t>({0, -static_cast<int64_t>(wanted_src.top),
                                            -static_cast<int64_t>(wanted_dst.top)});
  const int64_t src_left = static_cast<int64_t>(wanted_src.left) + lead_x;
  const int64_t src_top = static_cast<int64_t>(wanted_src.top) + lead_y;
  const int64_t dst_left = static_cast<int64_t>(wanted_dst.left) + lead_x;
  const int64_t dst_top = static_cast<int64_t>(wanted_dst.top) + lead_y;
  const int64_t width = std::min<int64_t>(
      {static_cast<int64_t>(wanted_src.right) - src_left,
       static_cast<int64_t>(wanted_dst.right) - dst_left,
       static_cast<int64_t>(source.width) - src_left,
       static_cast<int64_t>(destination.width) - dst_left});
  const int64_t height = std::min<int64_t>(
      {static_cast<int64_t>(wanted_src.bottom) - src_top,
       static_cast<int64_t>(wanted_dst.bottom) - dst_top,
       static_cast<int64_t>(source.height) - src_top,
       static_cast<int64_t>(destination.height) - dst_top});
  // A leading edge past its world's far side leaves nothing to copy, and so
  // does a negative extent; both come back as an empty rectangle rather than as
  // an origin outside the world.
  if (width <= 0 || height <= 0 || src_left >= source.width ||
      src_top >= source.height || dst_left >= destination.width ||
      dst_top >= destination.height) {
    src = {};
    dst = {};
    return true;
  }
  src = {static_cast<int32_t>(src_left), static_cast<int32_t>(src_top),
         static_cast<int32_t>(src_left + width), static_cast<int32_t>(src_top + height)};
  dst = {static_cast<int32_t>(dst_left), static_cast<int32_t>(dst_top),
         static_cast<int32_t>(dst_left + width), static_cast<int32_t>(dst_top + height)};
  return true;
}

bool normalize_legacy_rect(const LegacyRect* requested, int32_t width, int32_t height,
                           LegacyRect& result) {
  if (width <= 0 || height <= 0) return false;
  result = requested ? *requested : LegacyRect{0, 0, width, height};
  return result.left >= 0 && result.top >= 0 && result.right >= result.left &&
      result.bottom >= result.top && result.right <= width && result.bottom <= height;
}

bool clip_legacy_rect(const LegacyRect* requested, int32_t width, int32_t height,
                      LegacyRect& result) {
  if (width <= 0 || height <= 0) return false;
  const LegacyRect candidate = requested ? *requested : LegacyRect{0, 0, width, height};
  result.left = std::clamp(candidate.left, 0, width);
  result.top = std::clamp(candidate.top, 0, height);
  result.right = std::clamp(candidate.right, result.left, width);
  result.bottom = std::clamp(candidate.bottom, result.top, height);
  return true;
}

template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  if (offset > N || sizeof(value) > N - offset) return value;
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  if (offset > N || sizeof(value) > N - offset) return;
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}

}  // namespace

void configure(const Context& context) {
  g_context = context;
  g_configured = context.hooks.resolve_world &&
      context.hooks.resolve_dispatch_world_format && context.hooks.pixel_format &&
      context.hooks.set_pixel_format && context.hooks.bounded_argb8_world &&
      context.telemetry.calls &&
      context.telemetry.last_x && context.telemetry.last_y &&
      context.telemetry.last_opacity;
}

bool configured() noexcept { return g_configured; }

namespace {
// PF Fill Matte Suite fill/premultiply answer 516 for a handful of distinct
// reasons, and a plug-in that passes that through as its frame error
// (SolidComposite, Unmultiply, issue #1086) was attributable only by rebuilding
// the worker with prints. Same always-on marker as transform_world's (issue
// #995/#1032); these callbacks are per-call, not per-pixel, so one line each.
int32_t fill_matte_denied(const char* callback, const char* reason) {
  std::cerr << "stage:callback_denied callback=" << callback << " reason=" << reason
            << "\n" << std::flush;
  return kPfErrBadCallbackParam;
}

// The world an effect hands fill/premultiply carries its own depth, which is
// independent of the callback variant: PF_FillMatteSuite2's fill / fill16 /
// fill_float and premultiply_color / _color16 / _color_float differ only in the
// precision of the *colour* argument (PF_Pixel / PF_Pixel16 / PF_PixelFloat) -
// all three take the same PF_EffectWorld, and AE fills it at the world's own
// depth. Reading the depth off the world (issue #1086: SolidComposite calls the
// float-colour fill and Unmultiply the float-colour premultiply on an 8-bit
// world) rather than assuming it matches the colour is what this recovers.
// world_flags cannot tell 16-bit from float apart (host-stamped deep worlds set
// the DEEP bit for both), so the registered dispatch format is the authority: it
// distinguishes all three depths and covers every world an effect is handed in
// render (checkout worlds and new_world scratch worlds are both registered). A
// world the scope has not seen - a bare PF_EffectWorld the caller filled in
// itself (the fill/matte self-test) or a sub-world struct laid over a registered
// buffer - reaches the fallback: the DEEP bit separates 8-bit (clear) from deep
// (set), and the deep case is disambiguated 16-vs-float by the session format,
// since an unregistered deep world almost always shares the render's depth.
// That last step is a heuristic, not a proof: a deep sub-world whose depth
// differs from the session (e.g. a float scratch strip in an 8-bit session) is
// still mis-depthed here. It is depth-correct for every registered world and for
// the depth-matching sessions that produce the observed callers.
int32_t world_pixel_bytes(void* world) {
  DispatchWorldFormat format{};
  if (world && resolve_dispatch_world_format(world, format)) {
    if (format.pixel_format == kPixelFormatArgb32) return 4;
    if (format.pixel_format == kPixelFormatArgb64) return 8;
    if (format.pixel_format == kPixelFormatArgb128) return 16;
  }
  if (world) {
    int32_t flags{};
    std::memcpy(&flags, static_cast<const std::byte*>(world) + 16, sizeof(flags));
    if ((flags & 1) == 0) return 4;
    return std::strcmp(pixel_format(), "argb32f") == 0 ? 16 : 8;
  }
  return 4;
}

// Rewrites one ARGB colour from the callback variant's precision (from_bytes)
// into the world's depth (to_bytes) through a normalised [0,1] intermediate, so
// the value written matches the world rather than the colour argument. AE's
// 16-bit channel maximum is 0x8000 = 32768, not 65535.
void convert_argb_color(int32_t from_bytes, int32_t to_bytes, const void* in, void* out) {
  for (int channel = 0; channel < 4; ++channel) {
    const double normalized =
        from_bytes == 4 ? static_cast<const uint8_t*>(in)[channel] / 255.0 :
        from_bytes == 8 ? static_cast<const uint16_t*>(in)[channel] / 32768.0 :
                          static_cast<double>(static_cast<const float*>(in)[channel]);
    // An integer destination cannot hold out-of-range or non-finite float
    // channels, so bound to [0,1] before scaling: this both matches what AE
    // stores into an 8/16-bit world and keeps lround's argument finite (a raw
    // 1e30 or +inf * 32768 overflows long). NaN compares false against both
    // ends, so std::clamp would pass it straight through to lround; map it to 0
    // explicitly for a deterministic channel. The float destination keeps the
    // value as given - a float world holds HDR.
    const double bounded =
        normalized == normalized ? std::clamp(normalized, 0.0, 1.0) : 0.0;
    if (to_bytes == 4)
      static_cast<uint8_t*>(out)[channel] =
          static_cast<uint8_t>(std::lround(bounded * 255.0));
    else if (to_bytes == 8)
      static_cast<uint16_t*>(out)[channel] =
          static_cast<uint16_t>(std::lround(bounded * 32768.0));
    else
      static_cast<float*>(out)[channel] = static_cast<float>(normalized);
  }
}
}  // namespace

int32_t fill_world_typed(int32_t pixel_bytes, const void* color,
                         const LegacyRect* requested, void* world) {
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!resolve_world(world, pixel_bytes, pixels, rowbytes, width, height))
    return fill_matte_denied("fill", "unresolved_world");
  const std::array<unsigned char, 16> transparent_black{};
  if (!color) color = transparent_black.data();
  LegacyRect bounds{};
  if (!normalize_legacy_rect(requested, width, height, bounds))
    return fill_matte_denied("fill", "invalid_area");
  for (int32_t y = bounds.top; y < bounds.bottom; ++y)
    for (int32_t x = bounds.left; x < bounds.right; ++x)
      std::memcpy(pixels + static_cast<std::size_t>(y) * rowbytes +
                      static_cast<std::size_t>(x) * pixel_bytes,
                  color, pixel_bytes);
  return 0;
}

// The three fill variants differ only in the colour argument's precision; each
// fills the world at the world's own depth, converting the colour into it.
int32_t fill_matte_fill(int32_t color_bytes, const void* color,
                        const LegacyRect* area, void* world) {
  const int32_t world_bytes = world_pixel_bytes(world);
  if (!color) return fill_world_typed(world_bytes, nullptr, area, world);
  std::array<unsigned char, 16> converted{};
  convert_argb_color(color_bytes, world_bytes, color, converted.data());
  return fill_world_typed(world_bytes, converted.data(), area, world);
}
int32_t __cdecl fill_world8(void*, const void* color, const LegacyRect* area, void* world) {
  return fill_matte_fill(4, color, area, world);
}
int32_t __cdecl fill_world16(void*, const void* color, const LegacyRect* area, void* world) {
  return fill_matte_fill(8, color, area, world);
}
int32_t __cdecl fill_world_float(void*, const void* color, const LegacyRect* area, void* world) {
  return fill_matte_fill(16, color, area, world);
}

int32_t premultiply_color_typed(int32_t pixel_bytes, void* source_world, const void* matte,
                                int32_t forward, void* destination_world) {
  if (!source_world || !destination_world || !matte)
    return fill_matte_denied("premultiply_color", "null_argument");
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination, destination_rowbytes,
                           destination_width, destination_height) ||
      source_width != destination_width || source_height != destination_height)
    return fill_matte_denied("premultiply_color", "unresolved_or_size_mismatch");
  const std::size_t packed_row = static_cast<std::size_t>(source_width) * pixel_bytes;
  if (packed_row > SIZE_MAX / static_cast<std::size_t>(source_height))
    return fill_matte_denied("premultiply_color", "row_overflow");
  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(packed_row * static_cast<std::size_t>(source_height));
  } catch (const std::bad_alloc&) {
    return 4;
  } catch (...) {
    return kPfErrBadCallbackParam;
  }
  for (int32_t y = 0; y < source_height; ++y)
    std::memcpy(snapshot.data() + static_cast<std::size_t>(y) * packed_row,
                source + static_cast<std::size_t>(y) * source_rowbytes, packed_row);
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const auto read_value = [&](const unsigned char* pixel, int channel) {
    return pixel_bytes == 4 ? static_cast<double>(pixel[channel]) :
        (pixel_bytes == 8 ? static_cast<double>(reinterpret_cast<const uint16_t*>(pixel)[channel]) :
                            static_cast<double>(reinterpret_cast<const float*>(pixel)[channel]));
  };
  const auto write_value = [&](unsigned char* pixel, int channel, double value) {
    if (pixel_bytes == 4)
      pixel[channel] = static_cast<uint8_t>(std::clamp(std::lround(value), 0l, 255l));
    else if (pixel_bytes == 8)
      reinterpret_cast<uint16_t*>(pixel)[channel] = static_cast<uint16_t>(
          std::clamp(std::lround(value), 0l, 32768l));
    else
      reinterpret_cast<float*>(pixel)[channel] = static_cast<float>(value);
  };
  const auto* matte_bytes = static_cast<const unsigned char*>(matte);
  for (int32_t y = 0; y < source_height; ++y) {
    for (int32_t x = 0; x < source_width; ++x) {
      const auto* input = snapshot.data() + static_cast<std::size_t>(y) * packed_row +
          static_cast<std::size_t>(x) * pixel_bytes;
      auto* output = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      const double alpha_value = read_value(input, 0);
      const double alpha = alpha_value / maximum;
      write_value(output, 0, alpha_value);
      for (int channel = 1; channel < 4; ++channel) {
        const double input_value = read_value(input, channel);
        const double matte_value = read_value(matte_bytes, channel);
        const double result = forward
            ? input_value * alpha + matte_value * (1.0 - alpha)
            : (alpha > 0.0
                ? (input_value - matte_value * (1.0 - alpha)) / alpha : 0.0);
        write_value(output, channel, result);
      }
    }
  }
  return 0;
}

int32_t __cdecl premultiply_world8(void*, int32_t forward, void* world) {
  // The matte is black; a 16-byte zero buffer reads as zero in every depth, so
  // premultiply_color_typed at the world's own depth needs no per-format array.
  const std::array<unsigned char, 16> black{};
  return premultiply_color_typed(world_pixel_bytes(world), world, black.data(), forward, world);
}
// premultiply_color / _color16 / _color_float name the matte colour's precision;
// the source and destination worlds carry their own depth (issue #1086).
int32_t premultiply_color_dispatch(int32_t matte_bytes, void* source, const void* matte,
                                   int32_t forward, void* destination) {
  if (!source || !destination || !matte)
    return fill_matte_denied("premultiply_color", "null_argument");
  const int32_t world_bytes = world_pixel_bytes(destination);
  std::array<unsigned char, 16> converted{};
  convert_argb_color(matte_bytes, world_bytes, matte, converted.data());
  return premultiply_color_typed(world_bytes, source, converted.data(), forward, destination);
}
int32_t __cdecl premultiply_color8(void*, void* source, const void* color,
                                   int32_t forward, void* destination) {
  return premultiply_color_dispatch(4, source, color, forward, destination);
}
int32_t __cdecl premultiply_color16(void*, void* source, const void* color,
                                    int32_t forward, void* destination) {
  return premultiply_color_dispatch(8, source, color, forward, destination);
}
int32_t __cdecl premultiply_color_float(void*, void* source, const void* color,
                                        int32_t forward, void* destination) {
  return premultiply_color_dispatch(16, source, color, forward, destination);
}

static_assert(kUtilsFill == 9 * sizeof(void*));
static_assert(kUtilsPremultiply == 12 * sizeof(void*));
static_assert(kUtilsPremultiplyColor == 13 * sizeof(void*));
static_assert(kUtilsFill16 == 61 * sizeof(void*));
static_assert(kUtilsPremultiplyColor16 == 62 * sizeof(void*));
static_assert(kUtilsSize == 69 * sizeof(void*));

void wire_legacy_fill_matte_callbacks(std::array<std::byte, kUtilsSize>& utils) {
  write(utils, kUtilsFill, &fill_world8);
  write(utils, kUtilsPremultiply, &premultiply_world8);
  write(utils, kUtilsPremultiplyColor, &premultiply_color8);
  write(utils, kUtilsFill16, &fill_world16);
  write(utils, kUtilsPremultiplyColor16, &premultiply_color16);
}

bool verify_legacy_fill_matte_callbacks() {
  std::array<std::byte, kUtilsSize> utils{};
  wire_legacy_fill_matte_callbacks(utils);
  using Fill8 = int32_t(__cdecl*)(void*, const void*, const LegacyRect*, void*);
  using Fill16 = int32_t(__cdecl*)(void*, const void*, const LegacyRect*, void*);
  using Premultiply = int32_t(__cdecl*)(void*, int32_t, void*);
  using PremultiplyColor = int32_t(__cdecl*)(void*, void*, const void*, int32_t, void*);
  const auto fill8 = read<Fill8>(utils, kUtilsFill);
  const auto fill16 = read<Fill16>(utils, kUtilsFill16);
  const auto premultiply = read<Premultiply>(utils, kUtilsPremultiply);
  const auto premultiply8 = read<PremultiplyColor>(utils, kUtilsPremultiplyColor);
  const auto premultiply16 = read<PremultiplyColor>(utils, kUtilsPremultiplyColor16);
  if (fill8 != &fill_world8 || fill16 != &fill_world16 ||
      premultiply != &premultiply_world8 || premultiply8 != &premultiply_color8 ||
      premultiply16 != &premultiply_color16) return false;

  const char* active_format = pixel_format();
  if (!active_format) return false;
  const std::string saved_format = active_format;
  if (!set_pixel_format("argb8")) return false;
  std::array<uint8_t, 24> guarded8{};
  guarded8.fill(0xa5);
  LocalEffectWorld world8{};
  world8.data = guarded8.data() + 4;
  world8.rowbytes = 8;
  world8.width = 2;
  world8.height = 2;
  const std::array<uint8_t, 4> color8{{255, 10, 20, 30}};
  const LegacyRect one_pixel{1, 0, 2, 1};
  bool ok = fill8(nullptr, color8.data(), &one_pixel, &world8) == 0 &&
      std::all_of(guarded8.begin(), guarded8.begin() + 4,
                  [](uint8_t value) { return value == 0xa5; }) &&
      std::all_of(guarded8.end() - 4, guarded8.end(),
                  [](uint8_t value) { return value == 0xa5; }) &&
      std::memcmp(guarded8.data() + 8, color8.data(), color8.size()) == 0;
  const auto before_error = guarded8;
  LegacyRect invalid_rect{0, 0, 3, 1};
  ok = ok && fill8(nullptr, color8.data(), &invalid_rect, &world8) ==
          kPfErrBadCallbackParam &&
      guarded8 == before_error &&
      fill8(nullptr, color8.data(), nullptr, nullptr) == kPfErrBadCallbackParam;

  std::array<uint8_t, 32> guarded16{};
  guarded16.fill(0x5a);
  LocalEffectWorld world16{};
  world16.world_flags = 1;
  world16.data = guarded16.data() + 8;
  world16.rowbytes = 16;
  world16.width = 2;
  world16.height = 1;
  const std::array<uint16_t, 4> color16{{32768, 1024, 2048, 4096}};
  ok = ok && fill16(nullptr, color16.data(), nullptr, &world16) == 0 &&
      std::memcmp(guarded16.data() + 8, color16.data(), sizeof(color16)) == 0 &&
      std::memcmp(guarded16.data() + 16, color16.data(), sizeof(color16)) == 0 &&
      std::all_of(guarded16.begin(), guarded16.begin() + 8,
                  [](uint8_t value) { return value == 0x5a; }) &&
      std::all_of(guarded16.end() - 8, guarded16.end(),
                  [](uint8_t value) { return value == 0x5a; });
  ok = ok && premultiply(nullptr, 1, &world8) == 0 &&
      premultiply8(nullptr, nullptr, color8.data(), 1, &world8) ==
          kPfErrBadCallbackParam &&
      premultiply16(nullptr, &world16, nullptr, 1, &world16) ==
          kPfErrBadCallbackParam;
  if (!set_pixel_format(saved_format.c_str())) return false;
  return ok;
}

int32_t __cdecl convolve_world(void*, void* source_world, const LegacyRect* requested,
                               uint32_t flags, int32_t kernel_size, void* alpha_kernel,
                               void* red_kernel, void* green_kernel, void* blue_kernel,
                               void* destination_world) {
  constexpr uint32_t kOneDimensional = 1u << 0;
  constexpr uint32_t kNormalized = 1u << 1;
  constexpr uint32_t kNoClamp = 1u << 2;
  constexpr uint32_t kUseChar = 1u << 3;
  constexpr uint32_t kUseFixed = 1u << 4;
  constexpr uint32_t kVertical = 1u << 5;
  constexpr uint32_t kReplicateBorders = 1u << 6;
  constexpr uint32_t kAlphaWeighted = 1u << 7;
  constexpr uint32_t kKnownFlags = (1u << 8) - 1;
  const char* active_format = pixel_format();
  if (!active_format) return kPfErrBadCallbackParam;
  const int32_t pixel_bytes = std::strcmp(active_format, "argb32f") == 0 ? 16 :
      (std::strcmp(active_format, "argb16") == 0 ? 8 : 4);
  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if ((flags & ~kKnownFlags) || (flags & kUseChar && flags & kUseFixed) ||
      kernel_size <= 0 || kernel_size > 15 ||
      (kernel_size & 1) == 0 || !alpha_kernel || !red_kernel || !green_kernel ||
      !blue_kernel ||
      !resolve_world(source_world, pixel_bytes, source, source_rowbytes,
                           source_width, source_height) ||
      !resolve_world(destination_world, pixel_bytes, destination,
                           destination_rowbytes, destination_width, destination_height) ||
      source_width != destination_width || source_height != destination_height)
    return kPfErrBadCallbackParam;
  LegacyRect bounds{};
  if (!clip_legacy_rect(requested, source_width, source_height, bounds))
    return kPfErrBadCallbackParam;
  const auto kernels = std::array<const void*, 4>{
      alpha_kernel, red_kernel, green_kernel, blue_kernel};
  const bool one_dimensional = (flags & kOneDimensional) != 0;
  const int32_t tap_count = one_dimensional ? kernel_size : kernel_size * kernel_size;
  const double coefficient_scale = flags & kUseFixed ? 65536.0 : 255.0;
  const auto coefficient = [&](int channel, int index) -> double {
    if (flags & kUseChar)
      return static_cast<const uint8_t*>(kernels[channel])[index];
    if (flags & kUseFixed)
      return static_cast<const int32_t*>(kernels[channel])[index];
    return static_cast<const int32_t*>(kernels[channel])[index];
  };
  std::array<double, 4> divisors{};
  for (int channel = 0; channel < 4; ++channel) {
    double sum = 0.0;
    for (int index = 0; index < tap_count; ++index) sum += coefficient(channel, index);
    divisors[channel] = flags & kNormalized ? sum : coefficient_scale * tap_count;
    if (std::abs(divisors[channel]) < 1e-12) return kPfErrBadCallbackParam;
  }
  if (bounds.right <= bounds.left || bounds.bottom <= bounds.top) return 0;
  const uint64_t packed_rowbytes = static_cast<uint64_t>(source_width) * pixel_bytes;
  const uint64_t source_bytes = packed_rowbytes * source_height;
  if (!source_bytes || source_bytes > kMaxAsyncReceiptBytes)
    return kPfErrBadCallbackParam;
  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(static_cast<std::size_t>(source_bytes));
  } catch (const std::bad_alloc&) {
    return 4;
  } catch (...) {
    return kPfErrBadCallbackParam;
  }
  for (int32_t y = 0; y < source_height; ++y)
    std::memcpy(snapshot.data() + static_cast<std::size_t>(y) * packed_rowbytes,
                source + static_cast<std::size_t>(y) * source_rowbytes,
                static_cast<std::size_t>(packed_rowbytes));
  const int32_t radius = kernel_size / 2;
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const auto sample = [&](int32_t x, int32_t y, int channel, bool& present) -> double {
    present = x >= 0 && x < source_width && y >= 0 && y < source_height;
    if (!present && !(flags & kReplicateBorders)) return 0.0;
    x = std::clamp(x, 0, source_width - 1);
    y = std::clamp(y, 0, source_height - 1);
    const auto* pixel = snapshot.data() + static_cast<std::size_t>(y) * packed_rowbytes +
        static_cast<std::size_t>(x) * pixel_bytes;
    if (pixel_bytes == 4) return pixel[channel];
    if (pixel_bytes == 8) return reinterpret_cast<const uint16_t*>(pixel)[channel];
    return reinterpret_cast<const float*>(pixel)[channel];
  };
  const auto store = [&](unsigned char* pixel, int channel, double value) {
    if (flags & kNoClamp) {
      if (pixel_bytes == 4) pixel[channel] = static_cast<uint8_t>(std::llround(value));
      else if (pixel_bytes == 8) reinterpret_cast<uint16_t*>(pixel)[channel] =
          static_cast<uint16_t>(std::llround(value));
      else reinterpret_cast<float*>(pixel)[channel] = static_cast<float>(value);
      return;
    }
    value = std::clamp(value, 0.0, maximum);
    if (pixel_bytes == 4) pixel[channel] = static_cast<uint8_t>(std::lround(value));
    else if (pixel_bytes == 8) reinterpret_cast<uint16_t*>(pixel)[channel] =
        static_cast<uint16_t>(std::lround(value));
    else reinterpret_cast<float*>(pixel)[channel] = static_cast<float>(value);
  };
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      auto* output_pixel = destination + static_cast<std::size_t>(y) * destination_rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      double output_alpha = 0.0;
      for (int32_t channel = 0; channel < 4; ++channel) {
        double sum = 0.0;
        for (int32_t index = 0; index < tap_count; ++index) {
          const int32_t kernel_y = one_dimensional
              ? ((flags & kVertical) ? index : radius) : index / kernel_size;
          const int32_t kernel_x = one_dimensional
              ? ((flags & kVertical) ? radius : index) : index % kernel_size;
          bool present = false;
          double value = sample(x + kernel_x - radius, y + kernel_y - radius,
                                channel, present);
          if ((flags & kAlphaWeighted) && channel > 0) {
            bool alpha_present = false;
            const double alpha = sample(x + kernel_x - radius, y + kernel_y - radius,
                                        0, alpha_present) / maximum;
            value *= alpha;
          }
          sum += value * coefficient(channel, index);
        }
        double value = sum / divisors[channel];
        if (channel == 0) output_alpha = value;
        else if ((flags & kAlphaWeighted) && output_alpha > 0.0)
          value /= output_alpha / maximum;
        else if ((flags & kAlphaWeighted) && output_alpha <= 0.0) value = 0.0;
        store(output_pixel, channel, value);
      }
    }
  }
  return 0;
}

int32_t __cdecl blend_world(void*, const void* source_world1, const void* source_world2,
                            int32_t ratio, void* destination_world) {
  if (ratio < 0 || ratio > 65536) return kPfErrBadCallbackParam;
  DispatchWorldFormat first_info{}, second_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world1, first_info) ||
      !resolve_dispatch_world_format(source_world2, second_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      first_info.pixel_format != second_info.pixel_format ||
      first_info.pixel_format != destination_info.pixel_format ||
      first_info.width != second_info.width || first_info.width != destination_info.width ||
      first_info.height != second_info.height || first_info.height != destination_info.height ||
      first_info.width > 4096 || first_info.height > 4096)
    return kPfErrBadCallbackParam;
  const int32_t pixel_bytes = first_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (first_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (first_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes || first_info.rowbytes < static_cast<int64_t>(first_info.width) * pixel_bytes ||
      second_info.rowbytes < static_cast<int64_t>(second_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return kPfErrBadCallbackParam;
  const std::size_t packed_row = static_cast<std::size_t>(first_info.width) * pixel_bytes;
  std::vector<unsigned char> first_copy, second_copy;
  try {
    first_copy.resize(packed_row * first_info.height);
    second_copy.resize(packed_row * first_info.height);
  } catch (const std::bad_alloc&) {
    return 4;
  } catch (...) {
    return kPfErrBadCallbackParam;
  }
  for (int32_t y = 0; y < first_info.height; ++y) {
    std::memcpy(first_copy.data() + static_cast<std::size_t>(y) * packed_row,
        static_cast<const unsigned char*>(first_info.data) +
            static_cast<std::size_t>(y) * first_info.rowbytes, packed_row);
    std::memcpy(second_copy.data() + static_cast<std::size_t>(y) * packed_row,
        static_cast<const unsigned char*>(second_info.data) +
            static_cast<std::size_t>(y) * second_info.rowbytes, packed_row);
  }
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  const double fraction = ratio / 65536.0;
  for (int32_t y = 0; y < first_info.height; ++y) {
    for (int32_t x = 0; x < first_info.width; ++x) {
      const auto* first = first_copy.data() + static_cast<std::size_t>(y) * packed_row +
          static_cast<std::size_t>(x) * pixel_bytes;
      const auto* second = second_copy.data() + static_cast<std::size_t>(y) * packed_row +
          static_cast<std::size_t>(x) * pixel_bytes;
      auto* output = destination + static_cast<std::size_t>(y) * destination_info.rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      if (pixel_bytes == 4) {
        for (int channel = 0; channel < 4; ++channel) {
          const int value = (static_cast<int>(first[channel]) * (65536 - ratio) +
                             static_cast<int>(second[channel]) * ratio + 32768) >> 16;
          output[channel] = static_cast<unsigned char>(std::clamp(value, 0, 255));
        }
      } else if (pixel_bytes == 8) {
        const auto* first16 = reinterpret_cast<const uint16_t*>(first);
        const auto* second16 = reinterpret_cast<const uint16_t*>(second);
        auto* output16 = reinterpret_cast<uint16_t*>(output);
        for (int channel = 0; channel < 4; ++channel) {
          const int64_t value = (static_cast<int64_t>(first16[channel]) * (65536 - ratio) +
                                 static_cast<int64_t>(second16[channel]) * ratio + 32768) >> 16;
          output16[channel] = static_cast<uint16_t>(std::clamp<int64_t>(value, 0, 32768));
        }
      } else {
        const auto* first32 = reinterpret_cast<const float*>(first);
        const auto* second32 = reinterpret_cast<const float*>(second);
        auto* output32 = reinterpret_cast<float*>(output);
        for (int channel = 0; channel < 4; ++channel)
          output32[channel] = static_cast<float>(first32[channel] * (1.0 - fraction) +
                                                 second32[channel] * fraction);
      }
    }
  }
  return 0;
}

bool verify_world_transform_convolve() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 12> source{{255, 10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0}};
  std::array<uint8_t, 12> destination{};
  LocalEffectWorld source_world{}, destination_world{};
  source_world.data = source.data();
  source_world.rowbytes = 12;
  source_world.width = 3;
  source_world.height = 1;
  destination_world.data = destination.data();
  destination_world.rowbytes = 12;
  destination_world.width = 3;
  destination_world.height = 1;
  if (!formats.register_world(&source_world, kPixelFormatArgb32) ||
      !formats.register_world(&destination_world, kPixelFormatArgb32))
    return false;
  std::array<uint8_t, 1> kernel{{255}};
  constexpr uint32_t flags = (1u << 1) | (1u << 3);  // normalized, byte kernel
  const auto run = [&](const LegacyRect& area) {
    return convolve_world(nullptr, &source_world, &area, flags, 1, kernel.data(),
                          kernel.data(), kernel.data(), kernel.data(),
                          &destination_world);
  };
  const LegacyRect partial{-4, 0, 2, 1};
  destination.fill(0x5a);
  if (run(partial) != 0 || destination[1] != 10 || destination[5] != 20 ||
      destination[9] != 0x5a)
    return false;
  const LegacyRect oversized{-4, -3, 9, 7};
  destination.fill(0);
  if (run(oversized) != 0 || destination != source) return false;
  const LegacyRect outside{4, 0, 8, 1};
  destination.fill(0x5a);
  const auto sentinel = destination;
  if (run(outside) != 0 || destination != sentinel) return false;
  const LegacyRect inverted{2, 1, 1, 0};
  if (run(inverted) != 0 || destination != sentinel) return false;
  std::array<uint8_t, 1> zero_kernel{};
  if (convolve_world(nullptr, &source_world, &outside, flags, 1, zero_kernel.data(),
                     zero_kernel.data(), zero_kernel.data(), zero_kernel.data(),
                     &destination_world) != kPfErrBadCallbackParam)
    return false;
  return convolve_world(nullptr, &source_world, &oversized, flags, 0, kernel.data(),
                        kernel.data(), kernel.data(), kernel.data(),
                        &destination_world) == kPfErrBadCallbackParam;
}

bool verify_world_transform_blend() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 8> first{{255,10,20,30, 128,40,50,60}};
  std::array<uint8_t, 8> second{{0,110,120,130, 64,140,150,160}};
  std::array<uint8_t, 8> destination{};
  LocalEffectWorld first_world{}, second_world{}, destination_world{};
  auto initialize = [](LocalEffectWorld& world, void* data) {
    world.data=data; world.rowbytes=8; world.width=2; world.height=1;
  };
  initialize(first_world, first.data()); initialize(second_world, second.data());
  initialize(destination_world, destination.data());
  if (!formats.register_world(&first_world, kPixelFormatArgb32) ||
      !formats.register_world(&second_world, kPixelFormatArgb32) ||
      !formats.register_world(&destination_world, kPixelFormatArgb32) ||
      blend_world(nullptr, &first_world, &second_world, 32768, &destination_world) != 0)
    return false;
  const std::array<uint8_t, 8> expected{{128,60,70,80, 96,90,100,110}};
  if (destination != expected) return false;
  first = {{255,10,20,30, 128,40,50,60}};
  if (blend_world(nullptr, &first_world, &second_world, 32768, &first_world) != 0 ||
      first != expected) return false;
  first = {{255,10,20,30, 128,40,50,60}};
  second = {{0,110,120,130, 64,140,150,160}};
  return blend_world(nullptr, &first_world, &second_world, 32768, &second_world) == 0 &&
      second == expected;
}

namespace {

// Answers a refused PF_COPY / PF_COPY_HQ naming the condition that refused it,
// like `transform_world_denied` below: the plug-in usually passes the 516
// through as its whole frame error and says something unrelated ("insufficient
// memory for Wave Warp."), so without the marker the refusing check is
// recoverable only by rebuilding the worker with prints (issue #1037 filed
// Wave Warp as "refused with no denial trace" for exactly this gap). Always
// on; the reason is a lower-case identifier, the shape the broker's
// `callback_denials` parser vouches for.
int32_t copy_denied(const char* reason) {
  std::cerr << "stage:callback_denied callback=copy reason=" << reason
            << "\n" << std::flush;
  return kPfErrBadCallbackParam;
}

// Deliberately narrower than `world_registry::bytes_per_pixel`: the GPU
// format (`kPixelFormatGpuBgra128`) maps to 0 here so a GPU-world anchor can
// never admit a foreign operand - GPU worlds have no CPU pixel base for the
// bounds check to cap. Do not "unify" the two helpers.
int32_t dispatch_pixel_format_bytes(int32_t pixel_format) {
  return pixel_format == kPixelFormatArgb32 ? 4 :
      (pixel_format == kPixelFormatArgb64 ? 8 :
       (pixel_format == kPixelFormatArgb128 ? 16 : 0));
}

// A copy operand the format registry has never seen, admitted the way the
// sampling callbacks admit their source worlds (worker_pf_sampling_runtime.cpp,
// issues #777/#813): by the declared-stride bounds check, not by ownership.
// AE's PF_COPY takes any PF_EffectWorld the caller can describe, not only
// worlds the host handed out - Wave Warp builds a scratch row buffer inside a
// locked handle, wraps it in a stack PF_EffectWorld, and copies out of it, and
// refusing that world failed its whole frame with 516 (issue #1037). The
// declared stride and height cap every access the copy below makes, so the
// host's own walk stays inside what the plug-in declared; a declaration that
// misdescribes the plug-in's own memory is the plug-in's fault, contained by
// the worker process exactly as it is for sampling.
//
// Three refusals survive, deliberately:
// - A world the dispatch-format registry already knows - a registered struct
//   whose fields no longer match registration, or a struct re-declaring a
//   registered world's pixel base under a different geometry - never takes
//   this path: reaching a failed resolve with one is the registry's
//   fail-closed mismatch refusal on a host-handed reference (the frame input
//   and output worlds above all), and it must stay a refusal instead of
//   degrading into foreign admission. The refusal is as wide as the process:
//   the registry keeps its scope stack per-thread but publishes every live
//   stack, so this check sees the dispatch worlds from a thread the plug-in
//   spawned itself as well as from the one holding the scope (issue #1299).
// - A world whose pixel base the host allocated (`PF_NEW_WORLD`, AEGP
//   platform/owned backings) never takes this path either:
//   `world_pixels_owned` keeps each allocation's own fail-closed geometry
//   check (issue #700) authoritative, so a struct retargeted at a smaller
//   host allocation stays refused rather than bounds-laundered here. A
//   pointer *into* one of those allocations is indistinguishable from foreign
//   memory and is admitted at its declared stride, the sampling latitude.
// - The unknown side borrows its pixel format from the resolved side (the
//   copy requires the two formats equal anyway, and the DEEP-flag check inside
//   `resolve_world` still has to agree). With neither side resolved there is
//   no format to anchor on and the copy stays refused.
bool resolve_foreign_copy_world(void* world, const DispatchWorldFormat& known,
                                DispatchWorldFormat& result) {
  if (!g_configured || !g_context.hooks.world_pixels_owned ||
      world_safety::dispatch_world_reference_known(world) ||
      g_context.hooks.world_pixels_owned(world))
    return false;
  const int32_t pixel_bytes = dispatch_pixel_format_bytes(known.pixel_format);
  unsigned char* pixels{};
  int32_t rowbytes{}, width{}, height{};
  if (!pixel_bytes ||
      !resolve_world(world, pixel_bytes, pixels, rowbytes, width, height))
    return false;
  result = {};
  result.world = world;
  result.data = pixels;
  result.width = width;
  result.height = height;
  result.rowbytes = rowbytes;
  result.pixel_format = known.pixel_format;
  return true;
}

bool resolve_copy_worlds(void* source_world, void* destination_world,
                         DispatchWorldFormat& source_info,
                         DispatchWorldFormat& destination_info) {
  const bool source_known =
      resolve_dispatch_world_format(source_world, source_info);
  const bool destination_known =
      resolve_dispatch_world_format(destination_world, destination_info);
  if (source_known && destination_known) return true;
  if (source_known)
    return resolve_foreign_copy_world(destination_world, source_info,
                                      destination_info);
  if (destination_known)
    return resolve_foreign_copy_world(source_world, destination_info,
                                      source_info);
  return false;
}

}  // namespace

int32_t __cdecl copy_world8(void*, void* source_world, void* destination_world,
                            const LegacyRect* source_rect, const LegacyRect* destination_rect) {
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_copy_worlds(source_world, destination_world, source_info,
                           destination_info))
    return copy_denied("unresolved_world");
  if (source_info.pixel_format != destination_info.pixel_format)
    return copy_denied("pixel_format_mismatch");
  const int32_t pixel_bytes = dispatch_pixel_format_bytes(source_info.pixel_format);
  if (!pixel_bytes || source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return copy_denied("world_bounds");
  auto* source = static_cast<unsigned char*>(source_info.data);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  // Clipped to both worlds at once rather than refused against either. AE's
  // PF_COPY takes rectangles that may run past their worlds and copies the part
  // that overlaps: an effect splitting a stereo pair asks for the right half of
  // a full-width source into a half-width destination, and refusing that
  // answered PF_Err_BAD_CALLBACK_PARAM for the whole frame (3DGlasses, issue
  // #962).
  //
  // At once, because the copy is a correspondence: source (left+i, top+j) goes
  // to destination (left+i, top+j), so trimming one rectangle's leading edge
  // has to trim the other's by the same amount or the pixels land at the wrong
  // offset - a shifted copy rather than an error, which is the worse of the two
  // (the sweep that produced this change measures buckets, not pixels, and
  // would not have caught it).
  LegacyRect src{}, dst{};
  if (!copy_correspondence(source_rect, source_info, destination_rect,
                           destination_info, src, dst))
    return copy_denied("no_correspondence");
  const int32_t copy_width = src.right - src.left;
  const int32_t copy_height = src.bottom - src.top;
  // Disjoint after clipping: the caller named a region the two worlds do not
  // share, which is a copy of nothing rather than a fault, and is what AE does
  // with the same rectangles.
  if (copy_width <= 0 || copy_height <= 0) return 0;
  const std::size_t row_size = static_cast<std::size_t>(copy_width) * pixel_bytes;
  std::vector<unsigned char> temporary;
  try {
    if (g_fail_next_allocation_for_self_test.exchange(false)) throw std::bad_alloc();
    temporary.resize(row_size * copy_height);
  } catch (const std::bad_alloc&) {
    return 4;
  }
  for (int32_t row = 0; row < copy_height; ++row)
    std::memcpy(temporary.data() + static_cast<std::size_t>(row) * row_size,
                source + static_cast<std::size_t>(src.top + row) * source_info.rowbytes +
                    static_cast<std::size_t>(src.left) * pixel_bytes,
                row_size);
  for (int32_t row = 0; row < copy_height; ++row)
    std::memcpy(destination + static_cast<std::size_t>(dst.top + row) * destination_info.rowbytes +
                    static_cast<std::size_t>(dst.left) * pixel_bytes,
                temporary.data() + static_cast<std::size_t>(row) * row_size, row_size);
  return 0;
}

bool verify_bad_callback_param_contract() {
  if (fill_world8(nullptr, nullptr, nullptr, nullptr) != kPfErrBadCallbackParam ||
      blend_world(nullptr, nullptr, nullptr, -1, nullptr) != kPfErrBadCallbackParam ||
      transform_world(nullptr, 0, 0, 0, nullptr, nullptr, nullptr, nullptr, 1, 0,
                      nullptr, nullptr) != kPfErrBadCallbackParam ||
      copy_world8(nullptr, nullptr, nullptr, nullptr, nullptr) != kPfErrBadCallbackParam)
    return false;

  DispatchWorldFormatScope formats;
  std::array<uint8_t, 4> source_pixels{255, 1, 2, 3};
  std::array<uint8_t, 4> destination_pixels{4, 5, 6, 7};
  const auto destination_before = destination_pixels;
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data();
  source.rowbytes = 4;
  source.width = 1;
  source.height = 1;
  destination.data = destination_pixels.data();
  destination.rowbytes = 4;
  destination.width = 1;
  destination.height = 1;
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32))
    return false;
  g_fail_next_allocation_for_self_test.store(true);
  const int32_t allocation_result =
      copy_world8(nullptr, &source, &destination, nullptr, nullptr);
  g_fail_next_allocation_for_self_test.store(false);
  return allocation_result == 4 && destination_pixels == destination_before;
}

// What PF_COPY does with a rectangle that does not fit its world. AE clips such
// a rectangle and copies the overlap; this host refused it, which ended a frame
// for an effect splitting a stereo pair (issue #962). Clipping has to keep the
// correspondence between the two rectangles, so the cases below check where the
// pixels land and not only that the call succeeded - clipping each rectangle on
// its own passes a "did it return 0" test while copying to the wrong column.
bool verify_copy_world_clipping() {
  DispatchWorldFormatScope formats;
  constexpr int32_t kWide = 4, kNarrow = 2, kRows = 1;
  // Distinct per column so a shifted copy is visible: 10, 20, 30, 40 in red.
  std::array<uint8_t, kWide * 4> source_pixels{};
  for (int32_t column = 0; column < kWide; ++column) {
    source_pixels[static_cast<std::size_t>(column) * 4] = 255;
    source_pixels[static_cast<std::size_t>(column) * 4 + 1] =
        static_cast<uint8_t>((column + 1) * 10);
  }
  std::array<uint8_t, kNarrow * 4> destination_pixels{};
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data();
  source.rowbytes = kWide * 4;
  source.width = kWide;
  source.height = kRows;
  destination.data = destination_pixels.data();
  destination.rowbytes = kNarrow * 4;
  destination.width = kNarrow;
  destination.height = kRows;
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32))
    return false;
  const auto red = [&](int32_t column) {
    return destination_pixels[static_cast<std::size_t>(column) * 4 + 1];
  };

  // The stereo case: the right half of a wide source into a narrow
  // destination. Refused before, and the two source columns have to land in
  // destination order.
  const LegacyRect right_half{kNarrow, 0, kWide, kRows};
  const LegacyRect whole_destination{0, 0, kNarrow, kRows};
  bool ok = copy_world8(nullptr, &source, &destination, &right_half,
                        &whole_destination) == 0 &&
      red(0) == 30 && red(1) == 40;

  // A leading edge outside its world. The pair shifts together: the copy maps
  // source (left + i) to destination (left + i), so trimming the destination's
  // out-of-world first column trims the source's first column with it, and
  // source column 1 lands at destination column 0. Clipping the two rectangles
  // on their own instead would put source column 0 there - a copy shifted by
  // one, returning success.
  destination_pixels.fill(0);
  const LegacyRect from_origin{0, 0, kWide, kRows};
  const LegacyRect one_left{-1, 0, kNarrow - 1, kRows};
  ok = copy_world8(nullptr, &source, &destination, &from_origin, &one_left) == 0 &&
      red(0) == 20 && red(1) == 0 && ok;

  // Wholly outside, and inverted: a copy of nothing, and nothing written.
  destination_pixels.fill(0);
  const LegacyRect past_the_end{kWide + 8, 0, kWide + 16, kRows};
  const LegacyRect inverted{2, 0, 1, kRows};
  ok = copy_world8(nullptr, &source, &destination, &past_the_end,
                   &whole_destination) == 0 &&
      copy_world8(nullptr, &source, &destination, &inverted,
                  &whole_destination) == 0 &&
      red(0) == 0 && red(1) == 0 && ok;

  // An extent large enough to overflow 32-bit arithmetic is clipped, not
  // wrapped, and PF_COPY_HQ admits everything PF_COPY does - a plug-in that
  // branches on quality must not get one answer at draft and a refusal at the
  // quality a final renders at.
  destination_pixels.fill(0);
  const LegacyRect enormous{-2147483647 - 1, -2147483647 - 1, 2147483647, 2147483647};
  ok = copy_world8(nullptr, &source, &destination, &enormous, &enormous) == 0 && ok;
  destination_pixels.fill(0);
  ok = copy_world_hq(nullptr, &source, &destination, &right_half,
                     &whole_destination) == 0 &&
      red(0) == 30 && red(1) == 40 && ok;

  // A source world the host never handed out: Wave Warp wraps a scratch row
  // buffer from a locked handle in a stack PF_EffectWorld and copies out of it
  // (issue #1037). Admitted by the declared-stride bounds check with the
  // resolved destination's pixel format, at both quality entry points.
  std::array<uint8_t, kWide * 4> scratch_pixels{};
  for (int32_t column = 0; column < kWide; ++column) {
    scratch_pixels[static_cast<std::size_t>(column) * 4] = 255;
    scratch_pixels[static_cast<std::size_t>(column) * 4 + 1] =
        static_cast<uint8_t>((column + 1) * 11);
  }
  LocalEffectWorld scratch{};
  scratch.data = scratch_pixels.data();
  scratch.rowbytes = kWide * 4;
  scratch.width = kWide;
  scratch.height = kRows;
  destination_pixels.fill(0);
  ok = copy_world8(nullptr, &scratch, &destination, &right_half,
                   &whole_destination) == 0 &&
      red(0) == 33 && red(1) == 44 && ok;
  destination_pixels.fill(0);
  ok = copy_world_hq(nullptr, &scratch, &destination, &right_half,
                     &whole_destination) == 0 &&
      red(0) == 33 && red(1) == 44 && ok;

  // The same latitude on the destination side: a copy into a foreign world
  // lands where the declared geometry says.
  scratch_pixels.fill(0);
  destination_pixels.fill(0);
  const LegacyRect whole_source{0, 0, kNarrow, kRows};
  const LegacyRect scratch_right_half{kNarrow, 0, kWide, kRows};
  ok = copy_world8(nullptr, &source, &destination, &right_half,
                   &whole_destination) == 0 &&
      copy_world8(nullptr, &destination, &scratch, &whole_source,
                  &scratch_right_half) == 0 &&
      scratch_pixels[kNarrow * 4 + 1] == 30 &&
      scratch_pixels[(kNarrow + 1) * 4 + 1] == 40 && ok;

  // With neither side resolved there is no pixel format to anchor on: refused,
  // not guessed.
  LocalEffectWorld second_scratch = scratch;
  std::array<uint8_t, kWide * 4> second_scratch_pixels{};
  second_scratch.data = second_scratch_pixels.data();
  ok = copy_world8(nullptr, &scratch, &second_scratch, nullptr, nullptr) ==
      kPfErrBadCallbackParam && ok;

  // A foreign world whose DEEP flag disagrees with the anchoring format is a
  // misdescribed operand, refused by the same depth check sampling applies.
  LocalEffectWorld deep_scratch = scratch;
  deep_scratch.world_flags = 1;
  ok = copy_world8(nullptr, &deep_scratch, &destination, &right_half,
                   &whole_destination) == kPfErrBadCallbackParam && ok;

  // A registered world whose fields were mutated after registration is the
  // dispatch registry's fail-closed mismatch refusal, and the foreign-operand
  // fallback must not resurrect it: the struct pointer is known, so the
  // mutation stays refused, not admitted at its new declared geometry. The
  // mutation grows `height`, which the declared-stride bounds check alone
  // would accept - this case is what pins the gate, not the bounds.
  const int32_t source_height_before = source.height;
  source.height = kRows + 1;
  ok = copy_world8(nullptr, &source, &destination, nullptr, nullptr) ==
      kPfErrBadCallbackParam && ok;
  source.height = source_height_before;

  // Same refusal when the plug-in re-declares a registered world's pixel base
  // through a fresh struct with a different geometry: the base pointer is
  // known to the registry, so the re-declaration is a mismatch, not a foreign
  // world.
  LocalEffectWorld alias = scratch;
  alias.data = source_pixels.data();
  alias.rowbytes = kWide * 4;
  alias.width = kWide;
  alias.height = kRows + 1;
  ok = copy_world8(nullptr, &alias, &destination, nullptr, nullptr) ==
      kPfErrBadCallbackParam && ok;
  return ok;
}

// The foreign-operand fallback's gate, run by the harness under a
// configuration whose `world_pixels_owned` answers true for everything (a
// stand-in for "this base pointer is a host allocation") and again under a
// null hook: both must refuse the very world the fallback otherwise admits,
// because the gate - not the bounds check - is what keeps host-issued
// allocations under their own registries' fail-closed geometry checks. The
// admitting side of the same world is covered by `verify_copy_world_clipping`.
bool verify_copy_foreign_world_gate() {
  DispatchWorldFormatScope formats;
  constexpr int32_t kWidth = 2;
  std::array<uint8_t, kWidth * 4> source_pixels{{255, 10, 0, 0, 255, 20, 0, 0}};
  std::array<uint8_t, kWidth * 4> destination_pixels{};
  LocalEffectWorld foreign{}, destination{};
  foreign.data = source_pixels.data();
  foreign.rowbytes = kWidth * 4;
  foreign.width = kWidth;
  foreign.height = 1;
  destination.data = destination_pixels.data();
  destination.rowbytes = kWidth * 4;
  destination.width = kWidth;
  destination.height = 1;
  if (!formats.register_world(&destination, kPixelFormatArgb32)) return false;
  return copy_world8(nullptr, &foreign, &destination, nullptr, nullptr) ==
      kPfErrBadCallbackParam &&
      destination_pixels == std::array<uint8_t, kWidth * 4>{};
}

namespace {

// Answers a refused TRANSFORM_WORLD naming the condition that refused it.
// The numeric 516 alone cannot: this function refuses for a dozen reasons, and
// a plug-in that passes the answer through as its frame error (Tile, issue
// #995) was attributable only by rebuilding the worker with prints. Always on,
// like `stage:callback_addr_denied`; every reason is a lower-case identifier,
// which is the shape the broker's parser vouches for.
int32_t transform_world_denied(const char* reason) {
  std::cerr << "stage:callback_denied callback=transform_world reason=" << reason
            << "\n" << std::flush;
  return kPfErrBadCallbackParam;
}

// For a check where one scalar is the whole story - which transfer mode, how
// many matrices - the marker carries the value the plug-in passed. The value
// is plug-in-authored, so it rides in a numeric-only field the broker's
// parser range-checks against the emitted C types, never in the identifier.
int32_t transform_world_denied(const char* reason, int64_t value) {
  std::cerr << "stage:callback_denied callback=transform_world reason=" << reason
            << " value=" << value << "\n" << std::flush;
  return kPfErrBadCallbackParam;
}

// TRANSFER_RECT's twin (issue #1041): Lightning's 516 wobble came out of this
// callback and the collapsed callback_error could not say which of its checks
// refused, or with what. Per call, like transform_world's: TRANSFER_RECT is
// strip-shaped (per bolt segment for Lightning), not per-pixel.
int32_t transfer_rect_denied(const char* reason) {
  std::cerr << "stage:callback_denied callback=transfer_rect reason=" << reason
            << "\n" << std::flush;
  return kPfErrBadCallbackParam;
}

int32_t transfer_rect_denied(const char* reason, int64_t value) {
  std::cerr << "stage:callback_denied callback=transfer_rect reason=" << reason
            << " value=" << value << "\n" << std::flush;
  return kPfErrBadCallbackParam;
}

// PF transfer-mode blend, factored out of transfer_rect_registered so
// transform_world composites through the exact same math (issue #1178). These
// were transfer_rect's local lambdas; lifting them changes nothing there (the
// all-mode golden in verify_world_transform_composite_rect pins byte identity).
std::array<double, 3> blend_clip_color(std::array<double, 3> color) {
  const double luminance = 0.30 * color[0] + 0.59 * color[1] + 0.11 * color[2];
  const double minimum = (std::min)({color[0], color[1], color[2]});
  const double maximum_value = (std::max)({color[0], color[1], color[2]});
  if (minimum < 0.0) for (double& component : color)
    component = luminance + (component - luminance) * luminance / (luminance - minimum);
  if (maximum_value > 1.0) for (double& component : color)
    component = luminance + (component - luminance) * (1.0 - luminance) /
        (maximum_value - luminance);
  return color;
}
std::array<double, 3> blend_set_luminance(std::array<double, 3> color, double luminance) {
  const double delta = luminance - (0.30 * color[0] + 0.59 * color[1] + 0.11 * color[2]);
  for (double& component : color) component += delta;
  return blend_clip_color(color);
}
double blend_saturation(const std::array<double, 3>& color) {
  return (std::max)({color[0], color[1], color[2]}) -
      (std::min)({color[0], color[1], color[2]});
}
std::array<double, 3> blend_set_saturation(std::array<double, 3> color, double target) {
  int minimum_index = 0, maximum_index = 0;
  for (int index = 1; index < 3; ++index) {
    if (color[index] < color[minimum_index]) minimum_index = index;
    if (color[index] > color[maximum_index]) maximum_index = index;
  }
  const int middle_index = 3 - minimum_index - maximum_index;
  if (color[maximum_index] > color[minimum_index]) {
    color[middle_index] = (color[middle_index] - color[minimum_index]) * target /
        (color[maximum_index] - color[minimum_index]);
    color[maximum_index] = target;
  } else {
    color[middle_index] = color[maximum_index] = 0.0;
  }
  color[minimum_index] = 0.0;
  return color;
}
double blend_component_value(int32_t mode, double source, double destination) {
  switch (mode) {
    case 4: case 29: return source + destination;
    case 5: return source * destination;
    case 6: return source + destination - source * destination;
    case 7: return destination <= 0.5 ? 2.0 * source * destination :
        1.0 - 2.0 * (1.0 - source) * (1.0 - destination);
    case 8: return source <= 0.5 ? destination - (1.0 - 2.0 * source) * destination *
        (1.0 - destination) : destination + (2.0 * source - 1.0) *
        ((destination <= 0.25 ? ((16.0 * destination - 12.0) * destination + 4.0) *
        destination : std::sqrt((std::max)(destination, 0.0))) - destination);
    case 9: return source <= 0.5 ? 2.0 * source * destination :
        1.0 - 2.0 * (1.0 - source) * (1.0 - destination);
    case 10: return (std::min)(source, destination);
    case 11: return (std::max)(source, destination);
    case 12: case 26: return std::abs(destination - source);
    case 23: case 27: return source >= 1.0 ? 1.0 :
        (std::min)(1.0, destination / (1.0 - source));
    case 24: case 28: return source <= 0.0 ? 0.0 :
        1.0 - (std::min)(1.0, (1.0 - destination) / source);
    case 25: return source + destination - 2.0 * source * destination;
    case 30: return source + destination - 1.0;
    case 31: return source <= 0.5 ? destination + 2.0 * source - 1.0 :
        destination + 2.0 * (source - 0.5);
    case 32: return source <= 0.5 ? (source <= 0.0 ? 0.0 :
        1.0 - (std::min)(1.0, (1.0 - destination) / (2.0 * source))) :
        (source >= 1.0 ? 1.0 : (std::min)(1.0, destination / (2.0 * (1.0 - source))));
    case 33: return source <= 0.5 ? (std::min)(destination, 2.0 * source) :
        (std::max)(destination, 2.0 * source - 1.0);
    case 34: {
      const double vivid = source <= 0.5 ? (source <= 0.0 ? 0.0 :
          1.0 - (std::min)(1.0, (1.0 - destination) / (2.0 * source))) :
          (source >= 1.0 ? 1.0 :
          (std::min)(1.0, destination / (2.0 * (1.0 - source))));
      return vivid < 0.5 ? 0.0 : 1.0;
    }
    case 37: return destination - source;
    case 38: return source <= 0.0 ? 1.0 : destination / source;
    default: return source;
  }
}
// Composites straight-colour `src` onto `dst` (raw channel values in
// [0, maximum], index 0 = alpha) for one pixel under `transfer_mode`. Mutates
// `dst`; channels a mode leaves untouched keep their value, so the caller may
// store all four back unconditionally. Returns false only when a dissolve pixel
// is dropped (dst then unchanged). `px`/`py` seed the dissolve hash.
bool apply_transfer_blend(int32_t transfer_mode, uint32_t mode_flags, bool rgb_only,
                          int32_t random_seed, int64_t px, int64_t py,
                          double effective_opacity, double maximum,
                          const std::array<double, 4>& src,
                          std::array<double, 4>& dst) {
  if (transfer_mode == 0) {
    for (int channel = rgb_only ? 1 : 0; channel < 4; ++channel)
      dst[channel] = src[channel] * effective_opacity +
          dst[channel] * (1.0 - effective_opacity);
    return true;
  }
  const double raw_source_alpha = src[0] / maximum;
  if (transfer_mode >= 17 && transfer_mode <= 20) {
    const double source_luminance = (0.30 * src[1] + 0.59 * src[2] + 0.11 * src[3]) / maximum;
    const double factor = transfer_mode == 17 ? raw_source_alpha :
        (transfer_mode == 18 ? source_luminance :
         (transfer_mode == 19 ? 1.0 - raw_source_alpha : 1.0 - source_luminance));
    dst[0] = dst[0] * (1.0 - effective_opacity + effective_opacity * factor);
    return true;
  }
  if (transfer_mode == 22) {
    if (!rgb_only) dst[0] = dst[0] + src[0] * effective_opacity;
    return true;
  }
  if (transfer_mode == 3) {
    uint32_t hash = static_cast<uint32_t>(random_seed) ^
        (static_cast<uint32_t>(px) * 0x9e3779b9u) ^
        (static_cast<uint32_t>(py) * 0x85ebca6bu);
    hash ^= hash >> 16; hash *= 0x7feb352du; hash ^= hash >> 15;
    if ((hash & 0x00ffffffu) >= static_cast<uint32_t>(
            std::clamp(effective_opacity, 0.0, 1.0) * 16777216.0)) return false;
  }
  const double source_alpha = raw_source_alpha *
      (transfer_mode == 3 ? 1.0 : effective_opacity);
  const double destination_alpha = dst[0] / maximum;
  const bool behind = transfer_mode == 1;
  if (transfer_mode >= 4 && transfer_mode != 21) {
    std::array<double, 3> source_color{}, destination_color{}, blended{};
    for (int index = 0; index < 3; ++index) {
      source_color[index] = src[index + 1] / maximum;
      destination_color[index] = dst[index + 1] / maximum;
    }
    if (transfer_mode >= 13 && transfer_mode <= 16) {
      if (transfer_mode == 13)
        blended = blend_set_luminance(blend_set_saturation(source_color,
            blend_saturation(destination_color)), 0.30 * destination_color[0] +
            0.59 * destination_color[1] + 0.11 * destination_color[2]);
      else if (transfer_mode == 14)
        blended = blend_set_luminance(blend_set_saturation(destination_color,
            blend_saturation(source_color)), 0.30 * destination_color[0] +
            0.59 * destination_color[1] + 0.11 * destination_color[2]);
      else if (transfer_mode == 15)
        blended = blend_set_luminance(source_color, 0.30 * destination_color[0] +
            0.59 * destination_color[1] + 0.11 * destination_color[2]);
      else
        blended = blend_set_luminance(destination_color, 0.30 * source_color[0] +
            0.59 * source_color[1] + 0.11 * source_color[2]);
    } else if (transfer_mode == 35 || transfer_mode == 36) {
      const double source_luminance = 0.30 * source_color[0] +
          0.59 * source_color[1] + 0.11 * source_color[2];
      const double destination_luminance = 0.30 * destination_color[0] +
          0.59 * destination_color[1] + 0.11 * destination_color[2];
      blended = (transfer_mode == 35 ? source_luminance > destination_luminance :
          source_luminance < destination_luminance) ? source_color : destination_color;
    } else {
      for (int index = 0; index < 3; ++index)
        blended[index] = blend_component_value(transfer_mode, source_color[index],
                                               destination_color[index]);
    }
    if (rgb_only) {
      for (int index = 0; index < 3; ++index)
        dst[index + 1] = (destination_color[index] *
            (1.0 - effective_opacity) + blended[index] * effective_opacity) * maximum;
      return true;
    }
    for (int index = 0; index < 3; ++index) {
      const double result = (1.0 - source_alpha) * destination_color[index] +
          source_alpha * ((1.0 - destination_alpha) * source_color[index] +
                          destination_alpha * blended[index]);
      dst[index + 1] = result * maximum;
    }
    dst[0] = (source_alpha + destination_alpha * (1.0 - source_alpha)) * maximum;
    return true;
  }
  const double output_alpha = behind
      ? destination_alpha + source_alpha * (1.0 - destination_alpha)
      : source_alpha + destination_alpha * (1.0 - source_alpha);
  for (int channel = 1; channel < 4; ++channel) {
    double value = 0.0;
    if (transfer_mode == 21) {
      value = dst[channel] + src[channel] * effective_opacity;
    } else if (mode_flags == 1) {
      if (output_alpha > 0.0) {
        value = behind
            ? (dst[channel] * destination_alpha + src[channel] * source_alpha *
                (1.0 - destination_alpha)) / output_alpha
            : (src[channel] * source_alpha + dst[channel] * destination_alpha *
                (1.0 - source_alpha)) / output_alpha;
      }
    } else {
      value = behind
          ? dst[channel] + src[channel] * effective_opacity *
              (1.0 - destination_alpha)
          : src[channel] * effective_opacity + dst[channel] * (1.0 - source_alpha);
    }
    dst[channel] = value;
  }
  if (!rgb_only) dst[0] = output_alpha * maximum;
  return true;
}

}  // namespace

int32_t __cdecl transform_world(void* effect_ref, int32_t quality, uint32_t mode_flags,
                                int32_t field,
                                const void* source_world, const void* composite_mode,
                                const void* mask_world, const void* matrices,
                                int32_t matrix_count, uint8_t source_to_destination,
                                const LegacyRect* destination_rect, void* destination_world) {
  if (!effect_ref || !source_world || !composite_mode || !matrices)
    return transform_world_denied("null_argument");
  if (matrix_count != 1) return transform_world_denied("matrix_count", matrix_count);
  if (source_to_destination > 1)
    return transform_world_denied("source_to_destination_flag");
  if (quality < 0 || quality > 1) return transform_world_denied("quality_range", quality);
  if (mode_flags > 1) return transform_world_denied("mode_flags_range", mode_flags);
  if (field < 0 || field > 2) return transform_world_denied("field_range", field);
  int32_t transfer_mode{};
  int32_t random_seed{};
  uint8_t opacity{}, rgb_only{};
  uint16_t opacity16{};
  std::memcpy(&transfer_mode, composite_mode, sizeof(transfer_mode));
  std::memcpy(&random_seed, static_cast<const std::byte*>(composite_mode) + 4,
              sizeof(random_seed));
  std::memcpy(&opacity, static_cast<const std::byte*>(composite_mode) + 8, sizeof(opacity));
  std::memcpy(&rgb_only, static_cast<const std::byte*>(composite_mode) + 9, sizeof(rgb_only));
  std::memcpy(&opacity16, static_cast<const std::byte*>(composite_mode) + 10, sizeof(opacity16));
  // The warped source is composited through the full PF transfer-mode set, the
  // same blend transfer_rect serves (issue #1178). Numbers passes mode 2 and
  // Tile passes mode 22; refusing every non-zero mode failed the whole frame
  // with 516.
  if (transfer_mode < 0 || transfer_mode > 38)
    return transform_world_denied("transfer_mode_range", transfer_mode);
  if (rgb_only > 1) return transform_world_denied("rgb_only_range");
  std::array<double, 9> matrix{};
  std::memcpy(matrix.data(), matrices, sizeof(matrix));
  if (!std::all_of(matrix.begin(), matrix.end(),
                   [](double value) { return std::isfinite(value); }))
    return transform_world_denied("matrix_not_finite");
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info))
    return transform_world_denied("source_world_unresolved");
  if (!resolve_dispatch_world_format(destination_world, destination_info))
    return transform_world_denied("destination_world_unresolved");
  if (source_info.pixel_format != destination_info.pixel_format)
    return transform_world_denied("pixel_format_mismatch");
  const int32_t pixel_bytes = source_info.pixel_format == kPixelFormatArgb32 ? 4 :
      (source_info.pixel_format == kPixelFormatArgb64 ? 8 :
       (source_info.pixel_format == kPixelFormatArgb128 ? 16 : 0));
  if (!pixel_bytes) return transform_world_denied("pixel_format_unknown");
  // ARGB8 consumes only opacity8. Some plug-ins leave the adjacent deep-color
  // field uninitialized, so validate opacity16 only on paths that read it.
  if (pixel_bytes != 4 && opacity16 > 32768)
    return transform_world_denied("opacity16_range", opacity16);
  if (source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096)
    return transform_world_denied("extent_over_4096");
  if (source_info.rowbytes < static_cast<int64_t>(source_info.width) * pixel_bytes ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * pixel_bytes)
    return transform_world_denied("rowbytes_underrun");
  const double determinant = matrix[0] * (matrix[4] * matrix[8] - matrix[5] * matrix[7]) -
      matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6]) +
      matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6]);
  if (source_to_destination && std::abs(determinant) < 1e-12)
    return transform_world_denied("matrix_singular");
  std::array<double, 9> sampling_matrix = matrix;
  if (source_to_destination) {
    sampling_matrix = {{
        (matrix[4]*matrix[8]-matrix[5]*matrix[7])/determinant,
        (matrix[2]*matrix[7]-matrix[1]*matrix[8])/determinant,
        (matrix[1]*matrix[5]-matrix[2]*matrix[4])/determinant,
        (matrix[5]*matrix[6]-matrix[3]*matrix[8])/determinant,
        (matrix[0]*matrix[8]-matrix[2]*matrix[6])/determinant,
        (matrix[2]*matrix[3]-matrix[0]*matrix[5])/determinant,
        (matrix[3]*matrix[7]-matrix[4]*matrix[6])/determinant,
        (matrix[1]*matrix[6]-matrix[0]*matrix[7])/determinant,
        (matrix[0]*matrix[4]-matrix[1]*matrix[3])/determinant}};
  }
  const auto destination_to_source = [&](double x, double y) {
    const double homogeneous = x * sampling_matrix[2] + y * sampling_matrix[5] +
        sampling_matrix[8];
    if (!std::isfinite(homogeneous) || std::abs(homogeneous) < 1e-12)
      return std::array<double, 2>{-1.0e9, -1.0e9};
    return std::array<double, 2>{
        (x * sampling_matrix[0] + y * sampling_matrix[3] + sampling_matrix[6]) /
            homogeneous,
        (x * sampling_matrix[1] + y * sampling_matrix[4] + sampling_matrix[7]) /
            homogeneous};
  };
  LegacyRect bounds{};
  if (!clip_legacy_rect(destination_rect, destination_info.width,
                        destination_info.height, bounds))
    return transform_world_denied("destination_rect_invalid");
  const std::size_t packed_row = static_cast<std::size_t>(source_info.width) * pixel_bytes;
  std::vector<unsigned char> source_copy, mask_copy;
  int32_t mask_rowbytes{}, mask_width{}, mask_height{}, mask_offset_x{}, mask_offset_y{};
  uint32_t mask_flags{};
  try {
    source_copy.resize(packed_row * source_info.height);
    if (mask_world) {
      const auto* mask_bytes = static_cast<const std::byte*>(mask_world);
      void* mask_data{};
      std::memcpy(&mask_data, mask_bytes + 24, sizeof(mask_data));
      std::memcpy(&mask_rowbytes, mask_bytes + 32, sizeof(mask_rowbytes));
      std::memcpy(&mask_width, mask_bytes + 36, sizeof(mask_width));
      std::memcpy(&mask_height, mask_bytes + 40, sizeof(mask_height));
      std::memcpy(&mask_offset_x, mask_bytes + kEffectWorldSize, sizeof(mask_offset_x));
      std::memcpy(&mask_offset_y, mask_bytes + kEffectWorldSize + 4, sizeof(mask_offset_y));
      std::memcpy(&mask_flags, mask_bytes + kEffectWorldSize + 8, sizeof(mask_flags));
      if (!mask_data || mask_width <= 0 || mask_height <= 0 || mask_width > 4096 ||
          mask_height > 4096 || mask_rowbytes < static_cast<int64_t>(mask_width) * pixel_bytes ||
          (mask_flags & ~3u) != 0) return transform_world_denied("mask_invalid");
      const std::size_t mask_packed_row = static_cast<std::size_t>(mask_width) * pixel_bytes;
      mask_copy.resize(mask_packed_row * mask_height);
      for (int32_t y = 0; y < mask_height; ++y)
        std::memcpy(mask_copy.data() + static_cast<std::size_t>(y) * mask_packed_row,
                    static_cast<const unsigned char*>(mask_data) +
                        static_cast<std::size_t>(y) * mask_rowbytes, mask_packed_row);
      mask_rowbytes = static_cast<int32_t>(mask_packed_row);
    }
  } catch (const std::bad_alloc&) { return 4; }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  for (int32_t y = 0; y < source_info.height; ++y)
    std::memcpy(source_copy.data() + static_cast<std::size_t>(y) * packed_row,
                source + static_cast<std::size_t>(y) * source_info.rowbytes, packed_row);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  const double maximum = pixel_bytes == 4 ? 255.0 : (pixel_bytes == 8 ? 32768.0 : 1.0);
  const double opacity_fraction = pixel_bytes == 4 ? opacity / 255.0 : opacity16 / 32768.0;
  const auto read = [&](int x, int y, int channel) -> double {
    if (x < 0 || y < 0 || x >= source_info.width || y >= source_info.height) return 0.0;
    const auto* value = source_copy.data() + static_cast<std::size_t>(y) * packed_row +
        static_cast<std::size_t>(x) * pixel_bytes;
    return pixel_bytes == 4 ? value[channel] :
        (pixel_bytes == 8 ? reinterpret_cast<const uint16_t*>(value)[channel] :
                            reinterpret_cast<const float*>(value)[channel]);
  };
  const auto read_mask = [&](int x, int y, int channel) -> double {
    if (!mask_world || x < 0 || y < 0 || x >= mask_width || y >= mask_height) return 0.0;
    const auto* value = mask_copy.data() + static_cast<std::size_t>(y) * mask_rowbytes +
        static_cast<std::size_t>(x) * pixel_bytes;
    return pixel_bytes == 4 ? value[channel] :
        (pixel_bytes == 8 ? reinterpret_cast<const uint16_t*>(value)[channel] :
                            reinterpret_cast<const float*>(value)[channel]);
  };
  const auto write = [&](unsigned char* output, int channel, double value) {
    if (pixel_bytes == 4) output[channel] = static_cast<uint8_t>(
        std::clamp(std::lround(value), 0l, 255l));
    else if (pixel_bytes == 8) reinterpret_cast<uint16_t*>(output)[channel] =
        static_cast<uint16_t>(std::clamp(std::lround(value), 0l, 32768l));
    else reinterpret_cast<float*>(output)[channel] = static_cast<float>(value);
  };
  const auto floor_to_sample_coord = [](double value, int* result) -> bool {
    if (!result || !std::isfinite(value)) return false;
    const double floored = std::floor(value);
    constexpr double minimum = static_cast<double>((std::numeric_limits<int>::min)()) + 1.0;
    constexpr double maximum = static_cast<double>((std::numeric_limits<int>::max)()) - 1.0;
    if (floored < minimum || floored > maximum) return false;
    *result = static_cast<int>(floored);
    return true;
  };
  for (int32_t y = bounds.top; y < bounds.bottom; ++y) {
    if ((field == 1 && (y & 1)) || (field == 2 && !(y & 1))) continue;
    for (int32_t x = bounds.left; x < bounds.right; ++x) {
      const auto mapped = destination_to_source(x + 0.5, y + 0.5);
      const double source_x = mapped[0] - 0.5, source_y = mapped[1] - 0.5;
      std::array<double, 4> sampled{};
      if (quality == 0) {
        int sx = 0, sy = 0;
        if (floor_to_sample_coord(source_x + 0.5, &sx) &&
            floor_to_sample_coord(source_y + 0.5, &sy)) {
          for (int channel = 0; channel < 4; ++channel)
            sampled[channel] = read(sx, sy, channel);
        }
      } else {
        int x0 = 0, y0 = 0;
        if (floor_to_sample_coord(source_x, &x0) && floor_to_sample_coord(source_y, &y0)) {
          const double fx = source_x - x0, fy = source_y - y0;
          const std::array<double, 4> weights{{(1-fx)*(1-fy), fx*(1-fy), (1-fx)*fy, fx*fy}};
          const std::array<int, 4> xs{{x0, x0+1, x0, x0+1}}, ys{{y0, y0, y0+1, y0+1}};
          for (int tap = 0; tap < 4; ++tap) sampled[0] += read(xs[tap], ys[tap], 0) * weights[tap];
          for (int channel = 1; channel < 4; ++channel) {
            for (int tap = 0; tap < 4; ++tap) {
              double value = read(xs[tap], ys[tap], channel);
              if (mode_flags == 1) value *= read(xs[tap], ys[tap], 0) / maximum;
              sampled[channel] += value * weights[tap];
            }
            if (mode_flags == 1 && sampled[0] > 0.0)
              sampled[channel] /= sampled[0] / maximum;
          }
        }
      }
      double coverage = 1.0;
      if (mask_world) {
        const double mask_x = source_x - mask_offset_x;
        const double mask_y = source_y - mask_offset_y;
        auto mask_value = [&](int mx, int my) {
          if (mask_flags & 2u)
            return (0.299 * read_mask(mx, my, 1) + 0.587 * read_mask(mx, my, 2) +
                    0.114 * read_mask(mx, my, 3)) / maximum;
          return read_mask(mx, my, 0) / maximum;
        };
        if (quality == 0) {
          int mx = 0, my = 0;
          coverage = floor_to_sample_coord(mask_x + 0.5, &mx) &&
                     floor_to_sample_coord(mask_y + 0.5, &my) ? mask_value(mx, my) : 0.0;
        } else {
          int mx0 = 0, my0 = 0;
          if (floor_to_sample_coord(mask_x, &mx0) && floor_to_sample_coord(mask_y, &my0)) {
            const double mfx = mask_x - mx0, mfy = mask_y - my0;
            coverage = mask_value(mx0, my0) * (1-mfx) * (1-mfy) +
                mask_value(mx0+1, my0) * mfx * (1-mfy) +
                mask_value(mx0, my0+1) * (1-mfx) * mfy +
                mask_value(mx0+1, my0+1) * mfx * mfy;
          } else {
            coverage = 0.0;
          }
        }
        coverage = std::clamp(coverage, 0.0, 1.0);
        if (mask_flags & 1u) coverage = 1.0 - coverage;
      }
      auto* output = destination + static_cast<std::size_t>(y) * destination_info.rowbytes +
          static_cast<std::size_t>(x) * pixel_bytes;
      const auto read_output = [&](int channel) -> double {
        return pixel_bytes == 4 ? output[channel] :
            (pixel_bytes == 8 ? reinterpret_cast<uint16_t*>(output)[channel] :
                                reinterpret_cast<float*>(output)[channel]);
      };
      std::array<double, 4> src_channels{sampled[0], sampled[1], sampled[2], sampled[3]};
      std::array<double, 4> dst_channels{read_output(0), read_output(1),
                                         read_output(2), read_output(3)};
      const double effective_opacity = opacity_fraction * coverage;
      // Same blend as transfer_rect, fed the sample in the world's native
      // premultiply state exactly as transfer_rect feeds its `input`, so the
      // composite handles mode_flags identically. The mode_flags==1 bilinear
      // premultiply above is a sampling-quality step that is divided back out,
      // so it does not double-apply; it does not make the sample straight for
      // mode_flags==0. Unchanged channels keep their dst value, so storing all
      // four back matches the mode's selective writes.
      if (apply_transfer_blend(transfer_mode, mode_flags, rgb_only != 0, random_seed,
                               static_cast<int64_t>(x), static_cast<int64_t>(y),
                               effective_opacity, maximum, src_channels, dst_channels)) {
        for (int channel = 0; channel < 4; ++channel)
          write(output, channel, dst_channels[channel]);
      }
    }
  }
  ++*g_context.telemetry.calls;
  *g_context.telemetry.last_x = static_cast<int32_t>(std::lround(matrix[6]));
  *g_context.telemetry.last_y = static_cast<int32_t>(std::lround(matrix[7]));
  *g_context.telemetry.last_opacity = opacity;
  return 0;
}

bool verify_world_transform_affine() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 3 * 2 * 4> source_pixels{};
  std::array<uint8_t, 4 * 3 * 4> destination_pixels{};
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data(); source.rowbytes = 3 * 4; source.width = 3; source.height = 2;
  destination.data = destination_pixels.data(); destination.rowbytes = 4 * 4;
  destination.width = 4; destination.height = 3;
  for (int index = 0; index < 6; ++index) {
    source_pixels[index * 4] = 255;
    source_pixels[index * 4 + 1] = static_cast<uint8_t>((index + 1) * 10);
  }
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32)) return false;
  std::array<std::byte, 12> composite{};
  const int32_t copy = 0; const uint8_t opacity = 255; const uint16_t opacity16 = 32768;
  std::memcpy(composite.data(), &copy, sizeof(copy));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  LegacyRect bounds{0, 0, 4, 3};
  const std::array<double, 9> source_to_destination{{1,0,0, 0,1,0, 1,1,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      source_to_destination.data(), 1, 1, &bounds, &destination) != 0)
    return false;
  const auto red = [&](int x, int y) { return destination_pixels[(y * 4 + x) * 4 + 1]; };
  if (red(1,1) != 10 || red(2,1) != 20 || red(3,1) != 30 ||
      red(1,2) != 40 || red(2,2) != 50 || red(3,2) != 60 || red(0,0) != 0) return false;
  // The destination rect is an absolute write area. Clip it to the world
  // without rebasing matrix coordinates, and treat an empty intersection as a
  // successful no-op.
  const std::array<double, 9> identity{{1,0,0, 0,1,0, 0,0,1}};
  LegacyRect partial{-5, 0, 2, 1};
  destination_pixels.fill(0x5a);
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      identity.data(), 1, 0, &partial, &destination) != 0 ||
      red(0, 0) != 10 || red(1, 0) != 20 || red(2, 0) != 0x5a ||
      red(0, 1) != 0x5a)
    return false;
  const LegacyRect outside{6, 0, 9, 1};
  const auto sentinel = destination_pixels;
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      identity.data(), 1, 0, &outside, &destination) != 0 ||
      destination_pixels != sentinel)
    return false;
  const LegacyRect inverted{3, 2, 1, 0};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      identity.data(), 1, 0, &inverted, &destination) != 0 ||
      destination_pixels != sentinel)
    return false;
  // ARGB8 does not consume opacity16, whereas both deep-color paths do.
  const uint16_t garbage_opacity16 = 59342;
  std::memcpy(composite.data() + 10, &garbage_opacity16, sizeof(garbage_opacity16));
  destination_pixels.fill(0);
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      identity.data(), 1, 0, &bounds, &destination) != 0 ||
      red(0,0) != 10) return false;
  std::array<uint16_t, 4> source64{{32768, 1000, 2000, 3000}};
  std::array<uint16_t, 4> destination64{{123, 456, 789, 1011}};
  const auto destination64_before = destination64;
  LocalEffectWorld world64_source{}, world64_destination{};
  world64_source.data = source64.data(); world64_source.rowbytes = 8;
  world64_source.width = 1; world64_source.height = 1;
  world64_destination.data = destination64.data(); world64_destination.rowbytes = 8;
  world64_destination.width = 1; world64_destination.height = 1;
  std::array<float, 4> source128{{1.0f, 0.25f, 0.5f, 0.75f}};
  std::array<float, 4> destination128{{0.1f, 0.2f, 0.3f, 0.4f}};
  const auto destination128_before = destination128;
  LocalEffectWorld world128_source{}, world128_destination{};
  world128_source.data = source128.data(); world128_source.rowbytes = 16;
  world128_source.width = 1; world128_source.height = 1;
  world128_destination.data = destination128.data(); world128_destination.rowbytes = 16;
  world128_destination.width = 1; world128_destination.height = 1;
  const LegacyRect one_pixel{0, 0, 1, 1};
  if (!formats.register_world(&world64_source, kPixelFormatArgb64) ||
      !formats.register_world(&world64_destination, kPixelFormatArgb64) ||
      !formats.register_world(&world128_source, kPixelFormatArgb128) ||
      !formats.register_world(&world128_destination, kPixelFormatArgb128) ||
      transform_world(&world64_source, 0, 1, 0, &world64_source, composite.data(), nullptr,
                      identity.data(), 1, 0, &one_pixel, &world64_destination) !=
          kPfErrBadCallbackParam || destination64 != destination64_before ||
      transform_world(&world128_source, 0, 1, 0, &world128_source, composite.data(), nullptr,
                      identity.data(), 1, 0, &one_pixel, &world128_destination) !=
          kPfErrBadCallbackParam || destination128 != destination128_before) return false;
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  destination_pixels.fill(0);
  const std::array<double, 9> destination_to_source{{1,0,0, 0,1,0, -1,-1,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      destination_to_source.data(), 1, 0, &bounds, &destination) != 0 ||
      red(1,1) != 10 || red(3,2) != 60) return false;
  destination_pixels.fill(0);
  const std::array<double, 9> scale{{2,0,0, 0,2,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      scale.data(), 1, 1, &bounds, &destination) != 0 ||
      red(0,0) != 10 || red(1,0) != 10 || red(2,0) != 20 || red(3,0) != 20 ||
      red(0,1) != 10 || red(1,1) != 10 || red(2,1) != 20 || red(3,1) != 20 ||
      red(0,2) != 40 || red(1,2) != 40 || red(2,2) != 50 || red(3,2) != 50) return false;
  destination_pixels.fill(0);
  std::array<uint8_t, 3 * 2 * 4> mask_pixels{};
  for (int y = 0; y < 2; ++y) {
    mask_pixels[(y * 3) * 4] = 0;
    mask_pixels[(y * 3 + 1) * 4] = 128;
    mask_pixels[(y * 3 + 2) * 4] = 255;
  }
  std::array<std::byte, kEffectWorldSize + 12> mask{};
  void* mask_data = mask_pixels.data();
  const int32_t mask_rowbytes = 12, mask_width = 3, mask_height = 2;
  std::memcpy(mask.data() + 24, &mask_data, sizeof(mask_data));
  std::memcpy(mask.data() + 32, &mask_rowbytes, sizeof(mask_rowbytes));
  std::memcpy(mask.data() + 36, &mask_width, sizeof(mask_width));
  std::memcpy(mask.data() + 40, &mask_height, sizeof(mask_height));
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), mask.data(),
                      identity.data(), 1, 1, &bounds, &destination) != 0 ||
      red(0,0) != 0 || red(1,0) != 10 || red(2,0) != 30 ||
      red(0,1) != 0 || red(1,1) != 25 || red(2,1) != 60) return false;
  const auto before_invalid_mask = destination_pixels;
  const uint32_t invalid_mask_flags = 4;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &invalid_mask_flags,
              sizeof(invalid_mask_flags));
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), mask.data(),
                      identity.data(), 1, 1, &bounds, &destination) !=
          kPfErrBadCallbackParam || destination_pixels != before_invalid_mask) return false;
  destination_pixels.fill(0);
  const std::array<double, 9> projective_destination_to_source{{
      1,0,0.25, 0,1,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      projective_destination_to_source.data(), 1, 0, &bounds,
                      &destination) != 0 || red(0,0) != 10 || red(1,0) != 20 ||
      red(2,0) != 20 || red(3,0) != 20) return false;
  const auto direct_projective_result = destination_pixels;
  destination_pixels.fill(0);
  const std::array<double, 9> projective_source_to_destination{{
      1,0,-0.25, 0,1,0, 0,0,1}};
  if (transform_world(&source, 0, 1, 0, &source, composite.data(), nullptr,
                      projective_source_to_destination.data(), 1, 1, &bounds,
                      &destination) != 0 || destination_pixels != direct_projective_result)
    return false;
  return true;
}

template <typename Channel, int32_t PixelFormat>
int32_t transfer_rect_registered(int32_t quality, uint32_t mode_flags, int32_t field,
                                 const LegacyRect* source_rect, const void* source_world,
                                 int32_t transfer_mode, int32_t random_seed,
                                 uint8_t opacity8, uint8_t rgb_only,
                                 uint16_t opacity16, const void* mask_world,
                                 int32_t destination_x,
                                 int32_t destination_y, void* destination_world) {
  constexpr double maximum = std::is_same_v<Channel, uint8_t> ? 255.0 :
      (std::is_same_v<Channel, uint16_t> ? 32768.0 : 1.0);
  DispatchWorldFormat source_info{}, destination_info{};
  if (quality < 0 || quality > 1) return transfer_rect_denied("quality_range", quality);
  if (mode_flags > 1) return transfer_rect_denied("mode_flags_range", mode_flags);
  if (field < 0 || field > 2) return transfer_rect_denied("field_range", field);
  // Validated here because this is where it is read: the 8-bit instantiation
  // consumes `opacity8` and must not refuse garbage in a field it never
  // touches (issue #1041, Lightning).
  if constexpr (!std::is_same_v<Channel, uint8_t>) {
    if (opacity16 > 32768) return transfer_rect_denied("opacity16_range", opacity16);
  }
  if (!resolve_dispatch_world_format(source_world, source_info))
    return transfer_rect_denied("source_world_unresolved");
  if (!resolve_dispatch_world_format(destination_world, destination_info))
    return transfer_rect_denied("destination_world_unresolved");
  if (source_info.pixel_format != PixelFormat || destination_info.pixel_format != PixelFormat)
    return transfer_rect_denied("pixel_format_mismatch");
  if (source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096)
    return transfer_rect_denied("extent_over_4096");
  if (source_info.rowbytes < static_cast<int64_t>(source_info.width) * sizeof(Channel) * 4 ||
      destination_info.rowbytes <
          static_cast<int64_t>(destination_info.width) * sizeof(Channel) * 4)
    return transfer_rect_denied("rowbytes_underrun");
  // A source rect outside the source world is clipped, not refused (issue
  // #1041): Lightning's randomly-placed bolt rect leaves the frame on some
  // runs, and the refusal turned exactly those runs into frame_error:516 -
  // the mechanism behind its #993 wobble. The destination side already
  // clipped below; the source world's bounds join the same intersection, and
  // the anchor stays the requested rect's top-left so a surviving source
  // pixel still lands where it corresponds (the #962 rule). An empty or
  // inverted intersection transfers nothing and succeeds, like PF_COPY.
  const LegacyRect bounds = source_rect
      ? *source_rect : LegacyRect{0, 0, source_info.width, source_info.height};
  const int64_t clipped_left = (std::max)({static_cast<int64_t>(bounds.left),
      static_cast<int64_t>(bounds.left) - destination_x, int64_t{0}});
  const int64_t clipped_top = (std::max)({static_cast<int64_t>(bounds.top),
      static_cast<int64_t>(bounds.top) - destination_y, int64_t{0}});
  const int64_t clipped_right = (std::min)({static_cast<int64_t>(bounds.right),
      static_cast<int64_t>(bounds.left) - destination_x + destination_info.width,
      static_cast<int64_t>(source_info.width)});
  const int64_t clipped_bottom = (std::min)({static_cast<int64_t>(bounds.bottom),
      static_cast<int64_t>(bounds.top) - destination_y + destination_info.height,
      static_cast<int64_t>(source_info.height)});
  if (clipped_right <= clipped_left || clipped_bottom <= clipped_top) return 0;
  if (aexcompat::l2_detail::extended_diag_enabled() &&
      (bounds.left < 0 || bounds.top < 0 || bounds.right > source_info.width ||
       bounds.bottom > source_info.height))
    std::cerr << "extended_diag:transfer_rect source clipped l=" << bounds.left
              << " t=" << bounds.top << " r=" << bounds.right << " b=" << bounds.bottom
              << " source=" << source_info.width << "x" << source_info.height << "\n"
              << std::flush;
  const std::size_t width = static_cast<std::size_t>(clipped_right - clipped_left);
  const std::size_t height = static_cast<std::size_t>(clipped_bottom - clipped_top);
  if (width > SIZE_MAX / height || width * height > 16'777'216)
    return transfer_rect_denied("area_over_limit");
  using Pixel = std::array<Channel, 4>;
  std::vector<Pixel> snapshot;
  std::vector<double> mask_coverage;
  try {
    snapshot.resize(width * height);
    if (mask_world) mask_coverage.resize(width * height);
  } catch (...) { return transfer_rect_denied("allocation_failed"); }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  for (std::size_t row = 0; row < height; ++row)
    std::memcpy(snapshot.data() + row * width,
        source + (static_cast<std::size_t>(clipped_top) + row) * source_info.rowbytes +
            static_cast<std::size_t>(clipped_left) * sizeof(Pixel), width * sizeof(Pixel));
  if (mask_world) {
    const auto* mask_bytes = static_cast<const std::byte*>(mask_world);
    void* mask_data{};
    int32_t mask_rowbytes{}, mask_width{}, mask_height{}, mask_offset_x{}, mask_offset_y{};
    uint32_t mask_flags{};
    std::memcpy(&mask_data, mask_bytes + 24, sizeof(mask_data));
    std::memcpy(&mask_rowbytes, mask_bytes + 32, sizeof(mask_rowbytes));
    std::memcpy(&mask_width, mask_bytes + 36, sizeof(mask_width));
    std::memcpy(&mask_height, mask_bytes + 40, sizeof(mask_height));
    std::memcpy(&mask_offset_x, mask_bytes + kEffectWorldSize, sizeof(mask_offset_x));
    std::memcpy(&mask_offset_y, mask_bytes + kEffectWorldSize + 4, sizeof(mask_offset_y));
    std::memcpy(&mask_flags, mask_bytes + kEffectWorldSize + 8, sizeof(mask_flags));
    if (!mask_data || mask_width <= 0 || mask_height <= 0 || mask_width > 4096 ||
        mask_height > 4096 || mask_rowbytes < static_cast<int64_t>(mask_width) * sizeof(Pixel) ||
        (mask_flags & ~3u) != 0) return transfer_rect_denied("mask_invalid");
    const auto* mask_pixels = static_cast<const unsigned char*>(mask_data);
    for (std::size_t row = 0; row < height; ++row) {
      const int64_t mask_y = clipped_top + static_cast<int64_t>(row) - mask_offset_y;
      for (std::size_t column = 0; column < width; ++column) {
        const int64_t mask_x = clipped_left + static_cast<int64_t>(column) - mask_offset_x;
        double coverage = 0.0;
        if (mask_x >= 0 && mask_y >= 0 && mask_x < mask_width && mask_y < mask_height) {
          const auto* pixel = reinterpret_cast<const Pixel*>(mask_pixels +
              static_cast<std::size_t>(mask_y) * mask_rowbytes +

              static_cast<std::size_t>(mask_x) * sizeof(Pixel));
          if (mask_flags & 2u) {
            coverage = (0.299 * (*pixel)[1] + 0.587 * (*pixel)[2] +
                        0.114 * (*pixel)[3]) / maximum;
          } else {
            coverage = static_cast<double>((*pixel)[0]) / maximum;
          }
        }
        coverage = std::clamp(coverage, 0.0, 1.0);
        if (mask_flags & 1u) coverage = 1.0 - coverage;
        mask_coverage[row * width + column] = coverage;
      }
    }
  }
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  const double opacity = std::is_same_v<Channel, uint8_t>
      ? opacity8 / 255.0 : opacity16 / 32768.0;
  const auto store = [](double value) -> Channel {
    if constexpr (std::is_same_v<Channel, float>) return static_cast<float>(value);
    else return static_cast<Channel>(std::clamp(std::lround(value), 0l,
        std::is_same_v<Channel, uint8_t> ? 255l : 32768l));
  };
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t source_y = clipped_top + static_cast<int64_t>(row);
    const int64_t output_y = destination_y + source_y - bounds.top;
    if ((field == 1 && (output_y & 1)) || (field == 2 && !(output_y & 1))) continue;
    for (std::size_t column = 0; column < width; ++column) {
      const int64_t source_x = clipped_left + static_cast<int64_t>(column);
      const int64_t output_x = destination_x + source_x - bounds.left;
      const Pixel& input = snapshot[row * width + column];
      auto* output = reinterpret_cast<Pixel*>(destination +
          static_cast<std::size_t>(output_y) * destination_info.rowbytes +
          static_cast<std::size_t>(output_x) * sizeof(Pixel));
      const double effective_opacity = opacity *
          (mask_world ? mask_coverage[row * width + column] : 1.0);
      std::array<double, 4> src_channels{
          static_cast<double>(input[0]), static_cast<double>(input[1]),
          static_cast<double>(input[2]), static_cast<double>(input[3])};
      std::array<double, 4> dst_channels{
          static_cast<double>((*output)[0]), static_cast<double>((*output)[1]),
          static_cast<double>((*output)[2]), static_cast<double>((*output)[3])};
      // The dissolve hash keys off the source pixel coordinate, so pass
      // source_x/source_y as it did inline; unchanged channels keep their dst
      // value, so storing all four back matches the mode's selective writes.
      if (apply_transfer_blend(transfer_mode, mode_flags, rgb_only != 0, random_seed,
                               source_x, source_y, effective_opacity, maximum,
                               src_channels, dst_channels)) {
        for (int channel = 0; channel < 4; ++channel)
          (*output)[channel] = store(dst_channels[channel]);
      }
    }
  }
  return 0;
}

int32_t __cdecl copy_world_hq(void* effect_ref, void* source_world, void* destination_world,
                              const LegacyRect* source_rect,
                              const LegacyRect* destination_rect) {
  DispatchWorldFormat source_info{}, destination_info{};
  // The same operand admission `copy_world8` grants, so the two entry points
  // admit the same worlds (the #962 lesson below, applied to operands): a
  // plug-in that branches on `in_data->quality` must not find its scratch
  // world accepted at draft and refused at the quality a final renders at.
  if (!resolve_copy_worlds(source_world, destination_world, source_info,
                           destination_info))
    return copy_denied("unresolved_world");
  // The same clipping PF_COPY does, so the two entry points admit the same
  // rectangles. Refusing here while `copy_world8` clipped would have left a
  // plug-in that branches on `in_data->quality` - the ordinary
  // `PF_Quality_HI ? PF_COPY_HQ : PF_COPY` shape - working at draft and failing
  // the whole frame at the quality a final renders at (issue #962).
  //
  // The equal-extent check that stood here is what `copy_correspondence`
  // guarantees, so it is not repeated: it comes back with the two rectangles
  // already the same size.
  LegacyRect source_bounds{}, destination_bounds{};
  if (!copy_correspondence(source_rect, source_info, destination_rect,
                           destination_info, source_bounds, destination_bounds))
    return copy_denied("no_correspondence");
  return copy_world8(effect_ref, source_world, destination_world,
                     &source_bounds, &destination_bounds);
}

int32_t __cdecl transfer_rect(void* effect_ref, int32_t quality, uint32_t mode_flags,
                              int32_t field,
                              const LegacyRect* source_rect, const void* source_world,
                              const void* composite_mode, const void* mask_world,
                              int32_t destination_x, int32_t destination_y,
                              void* destination_world) {
  if (!effect_ref || !source_world || !composite_mode)
    return transfer_rect_denied("null_argument");
  if (destination_x < -4096 || destination_x > 4096)
    return transfer_rect_denied("destination_x_range", destination_x);
  if (destination_y < -4096 || destination_y > 4096)
    return transfer_rect_denied("destination_y_range", destination_y);
  int32_t transfer_mode{};
  int32_t random_seed{};
  uint8_t opacity{}, rgb_only{};
  uint16_t opacity16{};
  std::memcpy(&transfer_mode, composite_mode, sizeof(transfer_mode));
  std::memcpy(&random_seed, static_cast<const std::byte*>(composite_mode) + 4,
              sizeof(random_seed));
  std::memcpy(&opacity, static_cast<const std::byte*>(composite_mode) + 8, sizeof(opacity));
  std::memcpy(&rgb_only, static_cast<const std::byte*>(composite_mode) + 9, sizeof(rgb_only));
  std::memcpy(&opacity16, static_cast<const std::byte*>(composite_mode) + 10, sizeof(opacity16));
  if (transfer_mode < 0 || transfer_mode > 38)
    return transfer_rect_denied("transfer_mode_range", transfer_mode);
  if (rgb_only > 1) return transfer_rect_denied("rgb_only_range", rgb_only);
  // `opacity16` is validated where it is read - the deep-colour paths in
  // transfer_rect_registered - not here. On an 8-bit session only `opacity8`
  // is consumed, and Lightning leaves random garbage in the 16-bit field
  // (measured 36950..59342 across runs); refusing it here turned exactly the
  // runs where the garbage exceeded 32768 into frame errors, which was the
  // whole mechanism of Lightning's #993 wobble (issue #1041). The same shape
  // as area_sample's unread `area` field (issue #1033).
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info))
    return transfer_rect_denied("source_world_unresolved");
  if (!resolve_dispatch_world_format(destination_world, destination_info))
    return transfer_rect_denied("destination_world_unresolved");
  if (source_info.pixel_format != destination_info.pixel_format)
    return transfer_rect_denied("pixel_format_mismatch");
  if (source_info.pixel_format == kPixelFormatArgb32)
    return transfer_rect_registered<uint8_t, kPixelFormatArgb32>(quality, mode_flags, field,
        source_rect, source_world, transfer_mode, random_seed, opacity, rgb_only, opacity16,
        mask_world,
        destination_x, destination_y, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb64)
    return transfer_rect_registered<uint16_t, kPixelFormatArgb64>(quality, mode_flags, field,
        source_rect, source_world, transfer_mode, random_seed, opacity, rgb_only, opacity16,
        mask_world,
        destination_x, destination_y, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb128)
    return transfer_rect_registered<float, kPixelFormatArgb128>(quality, mode_flags, field,
        source_rect, source_world, transfer_mode, random_seed, opacity, rgb_only, opacity16,
        mask_world,
        destination_x, destination_y, destination_world);
  return transfer_rect_denied("pixel_format_unknown", source_info.pixel_format);
}

bool verify_world_transform_transfer_mask() {
  DispatchWorldFormatScope formats;
  std::array<uint8_t, 12> source_pixels{255, 200, 0, 0, 255, 200, 0, 0,
                                         255, 200, 0, 0};
  std::array<uint8_t, 12> destination_pixels{};
  std::array<uint8_t, 12> mask_pixels{0, 0, 0, 0, 128, 128, 128, 128,
                                      255, 255, 255, 255};
  LocalEffectWorld source{}, destination{};
  source.data = source_pixels.data(); source.rowbytes = 12; source.width = 3; source.height = 1;
  destination.data = destination_pixels.data(); destination.rowbytes = 12;
  destination.width = 3; destination.height = 1;
  if (!formats.register_world(&source, kPixelFormatArgb32) ||
      !formats.register_world(&destination, kPixelFormatArgb32)) return false;
  std::array<std::byte, kEffectWorldSize + 12> mask{};
  void* mask_data = mask_pixels.data();
  const int32_t mask_rowbytes = 12, mask_width = 3, mask_height = 1;
  std::memcpy(mask.data() + 24, &mask_data, sizeof(mask_data));
  std::memcpy(mask.data() + 32, &mask_rowbytes, sizeof(mask_rowbytes));
  std::memcpy(mask.data() + 36, &mask_width, sizeof(mask_width));
  std::memcpy(mask.data() + 40, &mask_height, sizeof(mask_height));
  std::array<std::byte, 12> composite{};
  const int32_t in_front = 2;
  const uint8_t opacity = 255;
  const uint16_t opacity16 = 32768;
  std::memcpy(composite.data(), &in_front, sizeof(in_front));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  LegacyRect bounds{0, 0, 3, 1};
  auto run = [&] {
    return transfer_rect(&source, 0, 0, 0, &bounds, &source, composite.data(), mask.data(),
                         0, 0, &destination);
  };
  if (run() != 0 || destination_pixels[1] != 0 || destination_pixels[5] != 100 ||
      destination_pixels[9] != 200 || destination_pixels[0] != 0 ||
      destination_pixels[4] != 128 || destination_pixels[8] != 255) return false;
  destination_pixels.fill(0);
  uint32_t flags = 1;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &flags, sizeof(flags));
  if (run() != 0 || destination_pixels[1] != 200 || destination_pixels[5] != 100 ||
      destination_pixels[9] != 0) return false;
  destination_pixels.fill(0);
  flags = 2;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &flags, sizeof(flags));
  if (run() != 0 || destination_pixels[1] != 0 || destination_pixels[5] != 100 ||
      destination_pixels[9] != 200) return false;
  destination_pixels.fill(0);
  flags = 0;
  const int32_t offset_x = 1;
  std::memcpy(mask.data() + kEffectWorldSize, &offset_x, sizeof(offset_x));
  if (run() != 0 || destination_pixels[1] != 0 || destination_pixels[5] != 0 ||
      destination_pixels[9] != 100) return false;
  int32_t transfer_mode = 0;
  const uint8_t half_opacity = 128;
  const uint16_t half_opacity16 = 16384;
  std::memcpy(composite.data(), &transfer_mode, sizeof(transfer_mode));
  std::memcpy(composite.data() + 8, &half_opacity, sizeof(half_opacity));
  std::memcpy(composite.data() + 10, &half_opacity16, sizeof(half_opacity16));
  for (std::size_t pixel = 0; pixel < 3; ++pixel) {
    destination_pixels[pixel * 4] = 255;
    destination_pixels[pixel * 4 + 1] = 40;
  }
  if (transfer_rect(&source, 0, 0, 0, &bounds, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 || destination_pixels[1] != 120 ||
      destination_pixels[5] != 120 || destination_pixels[9] != 120) return false;
  transfer_mode = 1;
  std::memcpy(composite.data(), &transfer_mode, sizeof(transfer_mode));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
  for (std::size_t pixel = 0; pixel < 3; ++pixel) {
    destination_pixels[pixel * 4] = 128;
    destination_pixels[pixel * 4 + 1] = 20;
    destination_pixels[pixel * 4 + 2] = 0;
    destination_pixels[pixel * 4 + 3] = 0;
  }
  if (transfer_rect(&source, 0, 0, 0, &bounds, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 || destination_pixels[0] != 255 ||
      destination_pixels[1] != 120) return false;
  const LegacyRect one_pixel{0, 0, 1, 1};
  const auto verify_mode = [&](int32_t mode, const std::array<uint8_t, 4>& source_pixel,
                               const std::array<uint8_t, 4>& destination_pixel,
                               const std::array<uint8_t, 4>& expected) {
    std::copy(source_pixel.begin(), source_pixel.end(), source_pixels.begin());
    std::copy(destination_pixel.begin(), destination_pixel.end(), destination_pixels.begin());
    std::memcpy(composite.data(), &mode, sizeof(mode));
    std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
    std::memcpy(composite.data() + 10, &opacity16, sizeof(opacity16));
    return transfer_rect(&source, 0, 0, 0, &one_pixel, &source, composite.data(), nullptr,
                         0, 0, &destination) == 0 &&
        std::equal(expected.begin(), expected.end(), destination_pixels.begin());
  };
  if (!verify_mode(5, {255, 128, 64, 32}, {255, 64, 128, 192},
                   {255, 32, 32, 24}) ||
      !verify_mode(6, {255, 128, 64, 32}, {255, 64, 128, 192},
                   {255, 160, 160, 200}) ||
      !verify_mode(37, {255, 128, 64, 32}, {255, 64, 128, 192},
                   {255, 0, 64, 160}) ||
      !verify_mode(17, {128, 128, 64, 32}, {200, 64, 128, 192},
                   {100, 64, 128, 192})) return false;
  // Issue #1041, both halves of Lightning's frame-ender. First: garbage in
  // the unread 16-bit opacity field must not refuse an 8-bit transfer - the
  // plug-in leaves random values there (measured 36950..59342) and only the
  // runs where the garbage topped 32768 died, which was its #993 wobble.
  const int32_t copy_mode = 0;
  const uint16_t garbage_opacity16 = 59342;
  std::memcpy(composite.data(), &copy_mode, sizeof(copy_mode));
  std::memcpy(composite.data() + 8, &opacity, sizeof(opacity));
  std::memcpy(composite.data() + 10, &garbage_opacity16, sizeof(garbage_opacity16));
  source_pixels = {255, 11, 0, 0, 255, 22, 0, 0, 255, 33, 0, 0};
  destination_pixels.fill(0);
  if (transfer_rect(&source, 0, 0, 0, &one_pixel, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 ||
      destination_pixels[0] != 255 || destination_pixels[1] != 11) return false;
  // Second: a source rect outside the source world is clipped with its anchor
  // preserved, not refused. The anchor is the requested top-left, so the
  // surviving source pixels land where they correspond (#962): with a rect
  // one column left of the world, source column 0 lands at destination
  // column 1.
  destination_pixels.fill(0);
  const LegacyRect one_left_outside{-1, 0, 2, 1};
  if (transfer_rect(&source, 0, 0, 0, &one_left_outside, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 ||
      destination_pixels[0] != 0 || destination_pixels[5] != 11 ||
      destination_pixels[9] != 22) return false;
  // Wholly outside: a transfer of nothing, and nothing written.
  destination_pixels.fill(0);
  const LegacyRect past_the_end{5, 0, 9, 1};
  if (transfer_rect(&source, 0, 0, 0, &past_the_end, &source, composite.data(), nullptr,
                    0, 0, &destination) != 0 ||
      destination_pixels != std::array<uint8_t, 12>{}) return false;

  destination_pixels.fill(0x5a);
  flags = 4;
  std::memcpy(mask.data() + kEffectWorldSize + 8, &flags, sizeof(flags));
  const auto before = destination_pixels;
  return run() == kPfErrBadCallbackParam && destination_pixels == before;
}


uint8_t composite_divide_255(uint32_t numerator) {
  return static_cast<uint8_t>(std::min<uint32_t>(255, (numerator + 127) / 255));
}

uint8_t composite_divide_65025(uint64_t numerator) {
  return static_cast<uint8_t>(std::min<uint64_t>(255, (numerator + 32'512) / 65'025));
}

int32_t __cdecl composite_rect8_legacy(void* effect_ref, LegacyRect* source_rect,
                                int32_t source_opacity, void* source_world,
                                int32_t destination_x, int32_t destination_y,
                                int32_t field, int32_t transfer_mode,
                                void* destination_world) {
  constexpr int32_t kFieldFrame = 0;
  constexpr int32_t kFieldUpper = 1;
  constexpr int32_t kFieldLower = 2;
  constexpr int32_t kTransferCopy = 0;
  constexpr int32_t kTransferBehind = 1;
  constexpr int32_t kTransferInFront = 2;
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      (field != kFieldFrame && field != kFieldUpper && field != kFieldLower) ||
      (transfer_mode != kTransferCopy && transfer_mode != kTransferBehind &&
       transfer_mode != kTransferInFront)) {
    return kPfErrBadCallbackParam;
  }

  unsigned char *source{}, *destination{};
  int32_t source_rowbytes{}, source_width{}, source_height{};
  int32_t destination_rowbytes{}, destination_width{}, destination_height{};
  if (!bounded_argb8(source_world, source, source_rowbytes, source_width, source_height) ||
      !bounded_argb8(destination_world, destination, destination_rowbytes,
                           destination_width, destination_height)) {
    return kPfErrBadCallbackParam;
  }
  if (source_rect->right < source_rect->left || source_rect->bottom < source_rect->top) {
    return kPfErrBadCallbackParam;
  }

  // Map the requested source rectangle's upper-left to the destination, then clip in 64-bit.
  const int64_t source_left = std::max<int64_t>(source_rect->left, 0);
  const int64_t source_top = std::max<int64_t>(source_rect->top, 0);
  const int64_t source_right = std::min<int64_t>(source_rect->right, source_width);
  const int64_t source_bottom = std::min<int64_t>(source_rect->bottom, source_height);
  const int64_t destination_left = static_cast<int64_t>(destination_x) +
      source_left - source_rect->left;
  const int64_t destination_top = static_cast<int64_t>(destination_y) +
      source_top - source_rect->top;
  const int64_t clipped_destination_left = std::max<int64_t>(destination_left, 0);
  const int64_t clipped_destination_top = std::max<int64_t>(destination_top, 0);
  const int64_t clipped_destination_right = std::min<int64_t>(
      destination_left + (source_right - source_left), destination_width);
  const int64_t clipped_destination_bottom = std::min<int64_t>(
      destination_top + (source_bottom - source_top), destination_height);
  if (source_right <= source_left || source_bottom <= source_top ||
      clipped_destination_right <= clipped_destination_left ||
      clipped_destination_bottom <= clipped_destination_top || source_opacity == 0) {
    return 0;
  }

  const int64_t clipped_source_left = source_left + clipped_destination_left - destination_left;
  const int64_t clipped_source_top = source_top + clipped_destination_top - destination_top;
  const std::size_t width = static_cast<std::size_t>(
      clipped_destination_right - clipped_destination_left);
  const std::size_t height = static_cast<std::size_t>(
      clipped_destination_bottom - clipped_destination_top);
  if (width > 4096 || height > 4096 || width > SIZE_MAX / 4 ||
      height > SIZE_MAX / (width * 4)) {
    return kPfErrBadCallbackParam;
  }

  std::vector<unsigned char> snapshot;
  try {
    snapshot.resize(width * height * 4);
  } catch (const std::bad_alloc&) {
    return 4;
  }
  for (std::size_t row = 0; row < height; ++row) {
    std::memcpy(snapshot.data() + row * width * 4,
                source + static_cast<std::size_t>(clipped_source_top + row) * source_rowbytes +
                    static_cast<std::size_t>(clipped_source_left) * 4,
                width * 4);
  }

  const uint32_t opacity = static_cast<uint32_t>(source_opacity);
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t output_y = clipped_destination_top + static_cast<int64_t>(row);
    if ((field == kFieldUpper && (output_y & 1) != 0) ||
        (field == kFieldLower && (output_y & 1) == 0)) {
      continue;
    }
    for (std::size_t column = 0; column < width; ++column) {
      const auto* input = snapshot.data() + (row * width + column) * 4;
      auto* output = destination + static_cast<std::size_t>(output_y) * destination_rowbytes +
          static_cast<std::size_t>(clipped_destination_left + column) * 4;
      if (transfer_mode == kTransferCopy) {
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_255(
              static_cast<uint32_t>(input[channel]) * opacity +
              static_cast<uint32_t>(output[channel]) * (255 - opacity));
        }
      } else if (transfer_mode == kTransferInFront) {
        const uint32_t destination_weight = 65'025 - input[0] * opacity;
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_65025(
              static_cast<uint64_t>(input[channel]) * opacity * 255 +
              static_cast<uint64_t>(output[channel]) * destination_weight);
        }
      } else {
        const uint32_t source_weight = opacity * (255 - output[0]);
        for (int channel = 0; channel < 4; ++channel) {
          output[channel] = composite_divide_65025(
              static_cast<uint64_t>(output[channel]) * 65'025 +
              static_cast<uint64_t>(input[channel]) * source_weight);
        }
      }
    }
  }
  return 0;
}

template <typename Channel, uint32_t Maximum>
int32_t composite_rect_registered(void* effect_ref, LegacyRect* source_rect,
                                  int32_t source_opacity, void* source_world,
                                  int32_t destination_x, int32_t destination_y,
                                  int32_t field, int32_t transfer_mode,
                                  void* destination_world) {
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      field < 0 || field > 2 || transfer_mode < 0 || transfer_mode > 2)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format ||
      source_info.width > 4096 || source_info.height > 4096 ||
      destination_info.width > 4096 || destination_info.height > 4096 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * sizeof(Channel) * 4 ||
      destination_info.rowbytes <
          static_cast<int64_t>(destination_info.width) * sizeof(Channel) * 4)
    return kPfErrBadCallbackParam;

  const int64_t sl = std::max<int64_t>(source_rect->left, 0);
  const int64_t st = std::max<int64_t>(source_rect->top, 0);
  const int64_t sr = std::min<int64_t>(source_rect->right, source_info.width);
  const int64_t sb = std::min<int64_t>(source_rect->bottom, source_info.height);
  const int64_t dl = static_cast<int64_t>(destination_x) + sl - source_rect->left;
  const int64_t dt = static_cast<int64_t>(destination_y) + st - source_rect->top;
  const int64_t cdl = std::max<int64_t>(dl, 0);
  const int64_t cdt = std::max<int64_t>(dt, 0);
  const int64_t cdr = std::min<int64_t>(dl + sr - sl, destination_info.width);
  const int64_t cdb = std::min<int64_t>(dt + sb - st, destination_info.height);
  if (sr <= sl || sb <= st || cdr <= cdl || cdb <= cdt || source_opacity == 0) return 0;
  const int64_t csl = sl + cdl - dl;
  const int64_t cst = st + cdt - dt;
  const std::size_t width = static_cast<std::size_t>(cdr - cdl);
  const std::size_t height = static_cast<std::size_t>(cdb - cdt);
  if (width > SIZE_MAX / height || width * height > 16'777'216) return kPfErrBadCallbackParam;

  using Pixel = std::array<Channel, 4>;
  std::vector<Pixel> snapshot;
  try { snapshot.resize(width * height); } catch (const std::bad_alloc&) { return 4; }
  const auto* source = static_cast<const unsigned char*>(source_info.data);
  auto* destination = static_cast<unsigned char*>(destination_info.data);
  for (std::size_t row = 0; row < height; ++row)
    std::memcpy(snapshot.data() + row * width,
                source + static_cast<std::size_t>(cst + row) * source_info.rowbytes +
                    static_cast<std::size_t>(csl) * sizeof(Pixel),
                width * sizeof(Pixel));

  const uint64_t opacity = static_cast<uint64_t>(source_opacity);
  const uint64_t denominator = static_cast<uint64_t>(Maximum) * 255;
  auto rounded = [](uint64_t numerator, uint64_t divisor) -> Channel {
    return static_cast<Channel>(std::min<uint64_t>(Maximum, (numerator + divisor / 2) / divisor));
  };
  for (std::size_t row = 0; row < height; ++row) {
    const int64_t output_y = cdt + static_cast<int64_t>(row);
    if ((field == 1 && (output_y & 1)) || (field == 2 && !(output_y & 1))) continue;
    for (std::size_t column = 0; column < width; ++column) {
      const Pixel& input = snapshot[row * width + column];
      auto* output = reinterpret_cast<Pixel*>(destination +
          static_cast<std::size_t>(output_y) * destination_info.rowbytes +
          static_cast<std::size_t>(cdl + column) * sizeof(Pixel));
      const uint64_t destination_alpha = (*output)[0];
      for (std::size_t channel = 0; channel < 4; ++channel) {
        uint64_t numerator{};
        uint64_t divisor{};
        if (transfer_mode == 0) {
          numerator = static_cast<uint64_t>(input[channel]) * opacity +
              static_cast<uint64_t>((*output)[channel]) * (255 - opacity);
          divisor = 255;
        } else if (transfer_mode == 2) {
          const uint64_t destination_weight = denominator -
              static_cast<uint64_t>(input[0]) * opacity;
          numerator = static_cast<uint64_t>(input[channel]) * opacity * Maximum +
              static_cast<uint64_t>((*output)[channel]) * destination_weight;
          divisor = denominator;
        } else {
          const uint64_t source_weight = opacity * (Maximum - destination_alpha);
          numerator = static_cast<uint64_t>((*output)[channel]) * denominator +
              static_cast<uint64_t>(input[channel]) * source_weight;
          divisor = denominator;
        }
        (*output)[channel] = rounded(numerator, divisor);
      }
    }
  }
  return 0;
}
int32_t composite_rect_float(void* effect_ref, LegacyRect* source_rect,
                             int32_t source_opacity, void* source_world,
                             int32_t destination_x, int32_t destination_y,
                             int32_t field, int32_t transfer_mode,
                             void* destination_world) {
  if (!effect_ref || !source_rect || source_opacity < 0 || source_opacity > 255 ||
      field < 0 || field > 2 || transfer_mode < 0 || transfer_mode > 2)
    return kPfErrBadCallbackParam;
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != kPixelFormatArgb128 ||
      destination_info.pixel_format != kPixelFormatArgb128 ||
      source_info.rowbytes < static_cast<int64_t>(source_info.width) * 16 ||
      destination_info.rowbytes < static_cast<int64_t>(destination_info.width) * 16)
    return kPfErrBadCallbackParam;
  const int64_t sl=(std::max<int64_t>)(source_rect->left,0), st=(std::max<int64_t>)(source_rect->top,0),
      sr=(std::min<int64_t>)(source_rect->right,source_info.width), sb=(std::min<int64_t>)(source_rect->bottom,source_info.height);
  const int64_t dl=destination_x+sl-source_rect->left, dt=destination_y+st-source_rect->top;
  const int64_t cdl=(std::max<int64_t>)(dl,0), cdt=(std::max<int64_t>)(dt,0),
      cdr=(std::min<int64_t>)(dl+sr-sl,destination_info.width), cdb=(std::min<int64_t>)(dt+sb-st,destination_info.height);
  if(sr<=sl||sb<=st||cdr<=cdl||cdb<=cdt||source_opacity==0) return 0;
  const int64_t csl=sl+cdl-dl,cst=st+cdt-dt;
  const std::size_t width=static_cast<std::size_t>(cdr-cdl),height=static_cast<std::size_t>(cdb-cdt);
  if(width>SIZE_MAX/height||width*height>16'777'216) return kPfErrBadCallbackParam;
  using Pixel=std::array<float,4>; std::vector<Pixel> snapshot;
  try{snapshot.resize(width*height);}catch(const std::bad_alloc&){return 4;}
  const auto* source=static_cast<const unsigned char*>(source_info.data); auto* destination=static_cast<unsigned char*>(destination_info.data);
  for(std::size_t row=0;row<height;++row) std::memcpy(snapshot.data()+row*width,
      source+static_cast<std::size_t>(cst+row)*source_info.rowbytes+static_cast<std::size_t>(csl)*sizeof(Pixel),width*sizeof(Pixel));
  const double opacity=source_opacity/255.0;
  for(std::size_t row=0;row<height;++row){const int64_t output_y=cdt+static_cast<int64_t>(row);
    if((field==1&&(output_y&1))||(field==2&&!(output_y&1)))continue;
    for(std::size_t column=0;column<width;++column){const Pixel& input=snapshot[row*width+column];
      auto* output=reinterpret_cast<Pixel*>(destination+static_cast<std::size_t>(output_y)*destination_info.rowbytes+static_cast<std::size_t>(cdl+column)*sizeof(Pixel));
      const double destination_alpha=(*output)[0];
      for(int channel=0;channel<4;++channel){
        if(transfer_mode==0)(*output)[channel]=static_cast<float>(input[channel]*opacity+(*output)[channel]*(1.0-opacity));
        else if(transfer_mode==2)(*output)[channel]=static_cast<float>(input[channel]*opacity+(*output)[channel]*(1.0-input[0]*opacity));
        else (*output)[channel]=static_cast<float>((*output)[channel]+input[channel]*opacity*(1.0-destination_alpha));
      }
    }
  }
  return 0;
}

int32_t __cdecl composite_rect8(void* effect_ref, LegacyRect* source_rect,
                                int32_t source_opacity, void* source_world,
                                int32_t destination_x, int32_t destination_y,
                                int32_t field, int32_t transfer_mode,
                                void* destination_world) {
  DispatchWorldFormat source_info{}, destination_info{};
  if (!resolve_dispatch_world_format(source_world, source_info) ||
      !resolve_dispatch_world_format(destination_world, destination_info) ||
      source_info.pixel_format != destination_info.pixel_format)
    return kPfErrBadCallbackParam;
  if (source_info.pixel_format == kPixelFormatArgb32)
    return composite_rect_registered<uint8_t, 255>(effect_ref, source_rect, source_opacity,
        source_world, destination_x, destination_y, field, transfer_mode, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb64)
    return composite_rect_registered<uint16_t, 32768>(effect_ref, source_rect, source_opacity,
        source_world, destination_x, destination_y, field, transfer_mode, destination_world);
  if (source_info.pixel_format == kPixelFormatArgb128)
    return composite_rect_float(effect_ref, source_rect, source_opacity, source_world,
        destination_x, destination_y, field, transfer_mode, destination_world);
  return kPfErrBadCallbackParam;
}

bool verify_world_transform_composite_rect() {
  DispatchWorldFormatScope dispatch_worlds;
  auto make_world = [&](std::array<std::byte, 64>& world, unsigned char* pixels,
                       int32_t rowbytes, int32_t width, int32_t height) {
    world.fill(std::byte{});
    std::memcpy(world.data() + 24, &pixels, sizeof(pixels));
    std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + 36, &width, sizeof(width));
    std::memcpy(world.data() + 40, &height, sizeof(height));
    dispatch_worlds.register_world(world.data(), kPixelFormatArgb32);
  };
  auto run_pixel_case = [&](int32_t mode, const std::array<uint8_t, 4>& expected) {
    std::array<unsigned char, 4> source{128, 64, 32, 16};
    std::array<unsigned char, 4> destination{64, 20, 10, 5};
    std::array<std::byte, 64> source_world{}, destination_world{};
    make_world(source_world, source.data(), 4, 1, 1);
    make_world(destination_world, destination.data(), 4, 1, 1);
    LegacyRect rect{0, 0, 1, 1};
    return composite_rect8(&source_world, &rect, 128, &source_world, 0, 0, 0, mode,
                           &destination_world) == 0 && destination == expected;
  };
  if (!run_pixel_case(0, {96, 42, 21, 11}) ||
      !run_pixel_case(1, {112, 44, 22, 11}) ||
      !run_pixel_case(2, {112, 47, 24, 12})) {
    std::cerr << "composite diagnostic: pixel matrix\n";
    return false;
  }
  // Golden byte-identity anchor for every transfer mode transfer_rect serves
  // (issue #1178): the per-pixel blend was extracted into apply_transfer_blend
  // so transform_world can reuse it, and these 39 cases pin transfer_rect's
  // output for one representative pixel/opacity so the extraction cannot drift
  // that widely-used path. Values captured from the pre-extraction build.
  {
    int dummy_effect = 0;
    auto run_transfer_case = [&](int32_t mode, uint32_t mode_flags, uint8_t rgb_only,
                                 const std::array<uint8_t, 4>& expected) {
      std::array<unsigned char, 4> src{128, 64, 32, 16};
      std::array<unsigned char, 4> dst{64, 20, 10, 5};
      std::array<std::byte, 64> sw{}, dw{};
      make_world(sw, src.data(), 4, 1, 1);
      make_world(dw, dst.data(), 4, 1, 1);
      std::array<std::byte, 12> cm{};
      const uint8_t op8 = 128;
      const uint16_t op16 = 32768;
      std::memcpy(cm.data(), &mode, sizeof(mode));
      std::memcpy(cm.data() + 8, &op8, sizeof(op8));
      std::memcpy(cm.data() + 9, &rgb_only, sizeof(rgb_only));
      std::memcpy(cm.data() + 10, &op16, sizeof(op16));
      LegacyRect r{0, 0, 1, 1};
      return transfer_rect(&dummy_effect, 0, mode_flags, 0, &r, &sw, cm.data(),
                           nullptr, 0, 0, &dw) == 0 &&
             dst == std::array<unsigned char, 4>{expected[0], expected[1],
                                                 expected[2], expected[3]};
    };
    // Three variants pin the extraction across the sub-paths a single flag set
    // would miss (the review of #1178 flagged the coverage gap): normal, the
    // premultiplied-composite branch (mode_flags==1), and the colour-only
    // branch (rgb_only==1). Values captured from the pre-extraction build.
    static constexpr std::array<std::array<uint8_t, 4>, 39> kTransferGoldens{{
        {96, 42, 21, 11},  {112, 44, 22, 11}, {112, 47, 24, 12}, {160, 42, 21, 11},
        {112, 32, 16, 8},  {112, 27, 14, 7},  {112, 32, 16, 8},  {112, 28, 14, 7},
        {112, 28, 14, 7},  {112, 28, 14, 7},  {112, 28, 14, 7},  {112, 31, 16, 8},
        {112, 30, 15, 7},  {112, 28, 14, 7},  {112, 29, 14, 7},  {112, 29, 14, 7},
        {112, 30, 16, 9},  {48, 20, 10, 5},   {37, 20, 10, 5},   {48, 20, 10, 5},
        {59, 20, 10, 5},   {112, 52, 26, 13}, {128, 20, 10, 5},  {112, 29, 14, 7},
        {112, 27, 14, 7},  {112, 32, 16, 8},  {112, 30, 15, 7},  {112, 29, 14, 7},
        {112, 27, 14, 7},  {112, 32, 16, 8},  {112, 16, 0, 0},   {112, 20, 2, 0},
        {112, 27, 14, 7},  {112, 28, 14, 7},  {112, 27, 14, 7},  {112, 31, 16, 8},
        {112, 28, 14, 7},  {112, 24, 12, 6},  {112, 32, 19, 12},
    }};
    // mode_flags==1: the premultiplied-composite branch.
    static constexpr std::array<std::array<uint8_t, 4>, 39> kTransferGoldensPremul{{
        {96, 42, 21, 11},  {112, 39, 19, 10}, {112, 45, 23, 11}, {160, 55, 28, 14},
        {112, 32, 16, 8},  {112, 27, 14, 7},  {112, 32, 16, 8},  {112, 28, 14, 7},
        {112, 28, 14, 7},  {112, 28, 14, 7},  {112, 28, 14, 7},  {112, 31, 16, 8},
        {112, 30, 15, 7},  {112, 28, 14, 7},  {112, 29, 14, 7},  {112, 29, 14, 7},
        {112, 30, 16, 9},  {48, 20, 10, 5},   {37, 20, 10, 5},   {48, 20, 10, 5},
        {59, 20, 10, 5},   {112, 52, 26, 13}, {128, 20, 10, 5},  {112, 29, 14, 7},
        {112, 27, 14, 7},  {112, 32, 16, 8},  {112, 30, 15, 7},  {112, 29, 14, 7},
        {112, 27, 14, 7},  {112, 32, 16, 8},  {112, 16, 0, 0},   {112, 20, 2, 0},
        {112, 27, 14, 7},  {112, 28, 14, 7},  {112, 27, 14, 7},  {112, 31, 16, 8},
        {112, 28, 14, 7},  {112, 24, 12, 6},  {112, 32, 19, 12},
    }};
    // rgb_only==1: the colour-only branch (alpha kept, except the alpha-only
    // matte modes 17-20 which write alpha regardless of rgb_only).
    static constexpr std::array<std::array<uint8_t, 4>, 39> kTransferGoldensRgbOnly{{
        {64, 42, 21, 11},  {64, 44, 22, 11},  {64, 47, 24, 12},  {64, 42, 21, 11},
        {64, 52, 26, 13},  {64, 12, 6, 3},    {64, 50, 25, 13},  {64, 15, 6, 3},
        {64, 15, 6, 3},    {64, 15, 6, 3},    {64, 20, 10, 5},   {64, 42, 21, 11},
        {64, 32, 16, 8},   {64, 20, 10, 5},   {64, 23, 9, 2},    {64, 23, 9, 2},
        {64, 34, 24, 19},  {48, 20, 10, 5},   {37, 20, 10, 5},   {48, 20, 10, 5},
        {59, 20, 10, 5},   {64, 52, 26, 13},  {64, 20, 10, 5},   {64, 23, 11, 5},
        {64, 10, 5, 2},    {64, 47, 25, 13},  {64, 32, 16, 8},   {64, 23, 11, 5},
        {64, 10, 5, 2},    {64, 52, 26, 13},  {64, 0, 0, 0},     {64, 0, 0, 0},
        {64, 10, 5, 2},    {64, 20, 10, 5},   {64, 10, 5, 2},    {64, 42, 21, 11},
        {64, 20, 10, 5},   {64, 0, 0, 0},     {64, 50, 45, 42},
    }};
    struct GoldenVariant {
      uint32_t mode_flags;
      uint8_t rgb_only;
      const std::array<std::array<uint8_t, 4>, 39>* goldens;
    };
    const std::array<GoldenVariant, 3> variants{{
        {0, 0, &kTransferGoldens},
        {1, 0, &kTransferGoldensPremul},
        {0, 1, &kTransferGoldensRgbOnly},
    }};
    for (const auto& variant : variants) {
      for (int32_t mode = 0; mode <= 38; ++mode) {
        if (!run_transfer_case(mode, variant.mode_flags, variant.rgb_only,
                               (*variant.goldens)[mode])) {
          std::cerr << "composite diagnostic: transfer_rect golden mode_flags="
                    << variant.mode_flags << " rgb_only=" << (int)variant.rgb_only
                    << " mode=" << mode << "\n";
          return false;
        }
      }
    }
  }

  constexpr int32_t rowbytes = 16;
  std::array<unsigned char, rowbytes * 3 + 16> source_guarded{};
  std::array<unsigned char, rowbytes * 3 + 16> destination_guarded{};
  source_guarded.fill(0xA5);
  destination_guarded.fill(0xCC);
  auto* source = source_guarded.data() + 8;
  auto* destination = destination_guarded.data() + 8;
  for (int y = 0; y < 3; ++y) {
    for (int x = 0; x < 3; ++x) {
      const std::array<unsigned char, 4> pixel{
          255, static_cast<unsigned char>(10 + y * 3 + x), 0, 0};
      std::memcpy(source + y * rowbytes + x * 4, pixel.data(), 4);
    }
  }
  std::array<std::byte, 64> source_world{}, destination_world{};
  make_world(source_world, source, rowbytes, 3, 3);
  make_world(destination_world, destination, rowbytes, 3, 3);
  LegacyRect rect{0, 0, 3, 3};
  if (composite_rect8(&source_world, &rect, 255, &source_world, -1, 0, 1, 0,
                      &destination_world) != 0) {
    std::cerr << "composite diagnostic: clipped upper field call\n";
    return false;
  }
  // Clipping drops source column zero; upper field updates destination rows 0 and 2 only.
  if (destination[1] != 11 || destination[5] != 12 || destination[rowbytes] != 0xCC ||
      destination[2 * rowbytes + 1] != 17 || destination[2 * rowbytes + 5] != 18) {
    std::cerr << "composite diagnostic: clipped upper field values\n";
    return false;
  }
  for (int y = 0; y < 3; ++y) {
    for (int x = 12; x < rowbytes; ++x) {
      if (destination[y * rowbytes + x] != 0xCC) return false;
    }
  }
  std::memset(destination, 0xCC, rowbytes * 3);
  if (composite_rect8(&source_world, &rect, 255, &source_world, 0, 0, 2, 0,
                      &destination_world) != 0 || destination[0] != 0xCC ||
      destination[rowbytes] != 255 || destination[rowbytes + 1] != 13 ||
      destination[2 * rowbytes] != 0xCC) {
    std::cerr << "composite diagnostic: lower field\n";
    return false;
  }
  if (!std::all_of(source_guarded.begin(), source_guarded.begin() + 8,
                   [](unsigned char value) { return value == 0xA5; }) ||
      !std::all_of(source_guarded.end() - 8, source_guarded.end(),
                   [](unsigned char value) { return value == 0xA5; }) ||
      !std::all_of(destination_guarded.begin(), destination_guarded.begin() + 8,
                   [](unsigned char value) { return value == 0xCC; }) ||
      !std::all_of(destination_guarded.end() - 8, destination_guarded.end(),
                   [](unsigned char value) { return value == 0xCC; })) {
    std::cerr << "composite diagnostic: guards\n";
    return false;
  }

  std::array<unsigned char, 12> alias_pixels{255, 1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0};
  std::array<std::byte, 64> alias_world{};
  make_world(alias_world, alias_pixels.data(), 12, 3, 1);
  LegacyRect alias_rect{0, 0, 2, 1};
  if (composite_rect8(&alias_world, &alias_rect, 255, &alias_world, 1, 0, 0, 0,
                      &alias_world) != 0 || alias_pixels[5] != 1 || alias_pixels[9] != 2) {
    std::cerr << "composite diagnostic: alias\n";
    return false;
  }
  alias_pixels.fill(0x5a);
  const auto alias_sentinel = alias_pixels;
  LegacyRect outside_rect{5, 0, 9, 1};
  if (composite_rect8(&alias_world, &outside_rect, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != 0 || alias_pixels != alias_sentinel) {
    std::cerr << "composite diagnostic: outside no-op\n";
    return false;
  }
  LegacyRect inverted_rect{2, 1, 1, 0};
  if (composite_rect8(&alias_world, &inverted_rect, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != 0 || alias_pixels != alias_sentinel) {
    std::cerr << "composite diagnostic: inverted no-op\n";
    return false;
  }
  if (composite_rect8(nullptr, &alias_rect, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, nullptr, 255, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, &alias_rect, 256, &alias_world, 0, 0, 0, 0,
                      &alias_world) != kPfErrBadCallbackParam ||
      composite_rect8(&alias_world, &alias_rect, 255, &alias_world, 0, 0, 3, 0,
                      &alias_world) != kPfErrBadCallbackParam) {
    std::cerr << "composite diagnostic: invalid arguments\n";
    return false;
  }

  // A padded ARGB16 row can resemble 32F by width; provenance must win over layout.
  std::array<uint16_t, 12> source16{32768, 0, 32768, 1, 32768, 32768, 0, 32767};
  std::array<uint16_t, 12> destination16{};
  std::array<std::byte, 64> registered16{}, shallow16{}, output16{};
  auto make16 = [](auto& world, void* data) {
    world.fill(std::byte{});
    const int32_t flags = 1, rowbytes = 24, width = 2, height = 1;
    std::memcpy(world.data() + 16, &flags, sizeof(flags));
    std::memcpy(world.data() + 24, &data, sizeof(data));
    std::memcpy(world.data() + 32, &rowbytes, sizeof(rowbytes));
    std::memcpy(world.data() + 36, &width, sizeof(width));
    std::memcpy(world.data() + 40, &height, sizeof(height));
  };
  make16(registered16, source16.data());
  shallow16 = registered16;
  make16(output16, destination16.data());
  if (!dispatch_worlds.register_world(registered16.data(), kPixelFormatArgb64) ||
      !dispatch_worlds.register_world(output16.data(), kPixelFormatArgb64)) {
    std::cerr << "composite diagnostic: register 16\n";
    return false;
  }
  LegacyRect rect16{0, 0, 2, 1};
  if (composite_rect8(registered16.data(), &rect16, 255, shallow16.data(), 0, 0, 0, 0,
      output16.data()) != 0 ||
      !std::equal(source16.begin(), source16.begin() + 8, destination16.begin())) {
    std::cerr << "composite diagnostic: copy 16\n";
    return false;
  }

  std::atomic_bool thread16{false}, thread32{false};
  std::thread deep_thread([&] {
    DispatchWorldFormatScope scope;
    scope.register_world(registered16.data(), kPixelFormatArgb64);
    scope.register_world(output16.data(), kPixelFormatArgb64);
    thread16 = composite_rect8(registered16.data(), &rect16, 255, registered16.data(),
                               0, 0, 0, 0, output16.data()) == 0;
  });
  std::thread float_thread([&] {
    DispatchWorldFormatScope scope;
    scope.register_world(registered16.data(), kPixelFormatArgb128);
    scope.register_world(output16.data(), kPixelFormatArgb128);
    thread32 = composite_rect8(registered16.data(), &rect16, 255, registered16.data(),
                               0, 0, 0, 0, output16.data()) == kPfErrBadCallbackParam;
  });
  deep_thread.join();
  float_thread.join();
  if (!thread16 || !thread32) {
    std::cerr << "composite diagnostic: concurrency " << thread16 << ',' << thread32 << '\n';
    return false;
  }
  std::array<float, 8> source32{{0.5f,2.0f,-0.5f,4.0f, 1.0f,8.0f,0.25f,-2.0f}};
  std::array<float, 8> destination32{};
  LocalEffectWorld source_world32{}, destination_world32{};
  source_world32.data=source32.data(); source_world32.rowbytes=32;
  source_world32.width=2; source_world32.height=1;
  destination_world32.data=destination32.data(); destination_world32.rowbytes=32;
  destination_world32.width=2; destination_world32.height=1;
  const bool source32_registered =
      dispatch_worlds.register_world(&source_world32, kPixelFormatArgb128);
  const bool destination32_registered =
      dispatch_worlds.register_world(&destination_world32, kPixelFormatArgb128);
  const int32_t composite32_result = composite_rect8(
      &source_world32, &rect16, 255, &source_world32, 0, 0, 0, 0, &destination_world32);
  if (!source32_registered || !destination32_registered || composite32_result != 0 ||
      destination32 != source32) {
    std::cerr << "float composite diagnostic: source_registered=" << source32_registered
              << ", destination_registered=" << destination32_registered
              << ", result=" << composite32_result;
    for (float value : destination32) std::cerr << ',' << value;
    std::cerr << '\n';
    return false;
  }
  return true;
}


namespace {
struct WorldTransformSuite1 {
  decltype(&composite_rect8) composite_rect;
  decltype(&blend_world) blend;
  decltype(&convolve_world) convolve;
  decltype(&copy_world8) copy;
  decltype(&copy_world_hq) copy_hq;
  decltype(&transfer_rect) transfer_rect;
  decltype(&transform_world) transform_world;
};
static_assert(sizeof(WorldTransformSuite1) == 7 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, composite_rect) == 0 * sizeof(void*));
static_assert(offsetof(WorldTransformSuite1, transform_world) == 6 * sizeof(void*));
WorldTransformSuite1 g_world_transform_suite1{};
std::array<void*, 7> g_fill_matte_suite2{};
}  // namespace

const void* provide_world_transform1(void*) {
  g_world_transform_suite1 = {&composite_rect8, &blend_world, &convolve_world,
      &copy_world8, &copy_world_hq, &transfer_rect, &transform_world};
  return &g_world_transform_suite1;
}

const void* provide_fill_matte2(void*) {
  void* callbacks[] = {reinterpret_cast<void*>(&fill_world8),
      reinterpret_cast<void*>(&fill_world16), reinterpret_cast<void*>(&fill_world_float),
      reinterpret_cast<void*>(&premultiply_world8), reinterpret_cast<void*>(&premultiply_color8),
      reinterpret_cast<void*>(&premultiply_color16),
      reinterpret_cast<void*>(&premultiply_color_float)};
  std::copy(std::begin(callbacks), std::end(callbacks), g_fill_matte_suite2.begin());
  return g_fill_matte_suite2.data();
}

}  // namespace aexcompat::pf_world_transform
