// Two host defects that together kept AE's own Displacement from rendering
// through the SmartFX route (issue #777).
//
//  1. The sampling callbacks rejected a null `effect_ref`. Displacement's
//     SMART_RENDER pixel function calls PF_SUBPIXEL_SAMPLE with a null ref -
//     the host's `in_data->effect_ref` is populated, the plug-in just does not
//     pass it - so every pixel took a 4 back and the plug-in returned it as its
//     own PF_Err_OUT_OF_MEMORY.
//
//  2. `subpixel_sample16` and `area_sample16` were missing from the utility
//     callback table. PF_UtilCallbacks has them between `host_resize_handle`
//     (offset 464) and `fill16` (488), so slots 472 and 480 stayed null and a
//     deep-colour plug-in that sampled jumped to address 0.
//
// Both are checked here against the generated ABI contract rather than against
// hand-written offsets, so a regenerated contract moves the test with it.

#include "generated/aex_abi_contract.hpp"
#include "worker_effect_bootstrap.hpp"
#include "worker_pf_sampling_runtime.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_suite_registry.hpp"

#include <array>
#include <cstdio>
#include <cstring>

namespace contract = aexcompat::abi::x86_64_windows;
namespace boot = aexcompat::worker_runtime::effect_bootstrap;

// The sampling TU reaches the suite registry only through the unsupported-slot
// recorder for the batch sampling suite, which nothing here exercises. Stubbed
// so this test links two translation units instead of the registry's whole
// dependency chain.
namespace aexcompat::worker_runtime {
int32_t record_unsupported_suite_call(UnsupportedSuiteId, uint32_t) noexcept { return 4; }
}  // namespace aexcompat::worker_runtime

namespace {

int failures = 0;

void check(bool condition, const char* what) {
  if (condition) return;
  std::fprintf(stderr, "FAIL: %s\n", what);
  ++failures;
}

// One 2x2 source world per depth, resolved through the host hook the sampling
// runtime asks for. The pixel values do not matter - only that a sample with no
// effect_ref reaches them and reports success.
struct World {
  std::array<unsigned char, 2 * 2 * 16> pixels{};
  int32_t pixel_bytes{};
};
World g_world;

bool resolve_world(void* world, int32_t pixel_bytes, unsigned char*& pixels,
                   int32_t& rowbytes, int32_t& width, int32_t& height) {
  auto* w = static_cast<World*>(world);
  if (!w || w->pixel_bytes != pixel_bytes) return false;
  pixels = w->pixels.data();
  rowbytes = 2 * pixel_bytes;
  width = 2;
  height = 2;
  return true;
}

// PF_SampPB carries the world being sampled at offset 16; the subpixel and
// nearest paths read it from there and need nothing else from the block.
std::array<std::byte, 32> sampling_params(void* world) {
  std::array<std::byte, 32> params{};
  std::memcpy(params.data() + 16, &world, sizeof(world));
  return params;
}

// The area path additionally reads the sampling radii (16.16), the area, and
// the edge behavior, and rejects the block unless they are in range - which is
// the point: those checks must survive, only the effect_ref one goes.
std::array<std::byte, 32> area_sampling_params(void* world) {
  auto params = sampling_params(world);
  const int32_t half_pixel = 32768;  // 0.5 in 16.16
  const int32_t area = 65536;
  const uint32_t edge_behavior = 0;
  std::memcpy(params.data() + 0, &half_pixel, sizeof(half_pixel));
  std::memcpy(params.data() + 4, &half_pixel, sizeof(half_pixel));
  std::memcpy(params.data() + 8, &area, sizeof(area));
  std::memcpy(params.data() + 24, &edge_behavior, sizeof(edge_behavior));
  return params;
}

using SampleFn = int32_t(__cdecl*)(void*, int32_t, int32_t, const void*, void*);

void a_null_effect_ref_still_samples() {
  PfSamplingHostHooks hooks{};
  hooks.resolve_world = &resolve_world;
  configure_pf_sampling_runtime(hooks);

  const struct {
    const char* name;
    SampleFn subpixel;
    SampleFn nearest;
    SampleFn area;
    int32_t pixel_bytes;
  } depths[] = {
      {"8", &subpixel_sample8, &nearest_sample8, &area_sample8, 4},
      {"16", &subpixel_sample16, &nearest_sample16, &area_sample16, 8},
      {"float", &subpixel_sample_float, &nearest_sample_float, &area_sample_float, 16},
  };

  for (const auto& depth : depths) {
    g_world.pixel_bytes = depth.pixel_bytes;
    auto params = sampling_params(&g_world);
    auto area_params = area_sampling_params(&g_world);
    std::array<unsigned char, 16> destination{};
    // Mid-pixel in 16.16 fixed point, so the bilinear path runs rather than
    // landing exactly on a sample.
    const int32_t half = 32768;
    char message[96];

    std::snprintf(message, sizeof(message),
                  "subpixel_sample%s succeeds with a null effect_ref", depth.name);
    check(depth.subpixel(nullptr, half, half, params.data(), destination.data()) == 0, message);

    std::snprintf(message, sizeof(message),
                  "nearest_sample%s succeeds with a null effect_ref", depth.name);
    check(depth.nearest(nullptr, half, half, params.data(), destination.data()) == 0, message);

    std::snprintf(message, sizeof(message),
                  "area_sample%s succeeds with a null effect_ref", depth.name);
    check(depth.area(nullptr, half, half, area_params.data(), destination.data()) == 0, message);

    // The arguments that are actually load-bearing still fail closed: dropping
    // the effect_ref check must not have dropped these.
    std::snprintf(message, sizeof(message), "subpixel_sample%s rejects a null params block",
                  depth.name);
    check(depth.subpixel(nullptr, half, half, nullptr, destination.data()) == 4, message);

    std::snprintf(message, sizeof(message), "subpixel_sample%s rejects a null destination",
                  depth.name);
    check(depth.subpixel(nullptr, half, half, params.data(), nullptr) == 4, message);

    // A world this worker cannot resolve is still refused.
    World foreign;
    foreign.pixel_bytes = depth.pixel_bytes == 4 ? 8 : 4;
    auto foreign_params = sampling_params(&foreign);
    std::snprintf(message, sizeof(message), "subpixel_sample%s refuses an unresolvable world",
                  depth.name);
    check(depth.subpixel(nullptr, half, half, foreign_params.data(), destination.data()) == 4,
          message);
  }
}

// The utility table is written positionally from UTILITY_CALLBACK_OFFSETS, so a
// slot added in the middle shifts everything after it. Rather than spot-check
// the two new entries, assert the whole table round-trips: every hook lands at
// the offset its index names, and no slot is left null.
void the_utility_table_is_wired_one_to_one() {
  boot::State state;
  boot::AbiHooks abi;
  // Distinguishable, non-null, aligned stand-ins: index i becomes (i+1)*8.
  for (std::size_t i = 0; i < abi.utility_callbacks.size(); ++i)
    abi.utility_callbacks[i] = reinterpret_cast<void*>((i + 1) * sizeof(void*));
  abi.effect_ref = reinterpret_cast<void*>(0x1000);
  boot::install_callback_tables(state, abi);

  for (std::size_t i = 0; i < contract::UTILITY_CALLBACK_OFFSETS.size(); ++i) {
    void* written{};
    std::memcpy(&written, state.utils.data() + contract::UTILITY_CALLBACK_OFFSETS[i],
                sizeof(written));
    if (written == abi.utility_callbacks[i]) continue;
    std::fprintf(stderr, "FAIL: utility slot %zu (offset %zu) round-trip\n", i,
                 contract::UTILITY_CALLBACK_OFFSETS[i]);
    ++failures;
  }

  // The two that were missing, named explicitly so a regression reads clearly.
  void* subpixel16{};
  void* area16{};
  std::memcpy(&subpixel16, state.utils.data() + contract::UTILS_SUBPIXEL_SAMPLE16_OFFSET,
              sizeof(subpixel16));
  std::memcpy(&area16, state.utils.data() + contract::UTILS_AREA_SAMPLE16_OFFSET,
              sizeof(area16));
  check(subpixel16 != nullptr, "utils.subpixel_sample16 is wired");
  check(area16 != nullptr, "utils.area_sample16 is wired");
  check(contract::UTILS_SUBPIXEL_SAMPLE16_OFFSET == 472, "subpixel_sample16 sits at 472");
  check(contract::UTILS_AREA_SAMPLE16_OFFSET == 480, "area_sample16 sits at 480");
}

}  // namespace

int main() {
  a_null_effect_ref_still_samples();
  the_utility_table_is_wired_one_to_one();
  if (failures == 0) std::printf("{\"pf_sampling_wiring_selftest\":\"passed\"}\n");
  return failures == 0 ? 0 : 1;
}
