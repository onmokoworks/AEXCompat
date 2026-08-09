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
// The offsets come from the generated ABI contract, so a regenerated contract
// moves the test with it - except for the two literal 472/480 assertions at the
// end, which deliberately pin today's SDK layout so a silent contract change
// has to be noticed rather than absorbed.

#include "generated/aex_abi_contract.hpp"
#include "worker_callback_diagnostics.hpp"
#include "worker_effect_bootstrap.hpp"
#include "worker_pf_sampling_runtime.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_pf_utility_callback_table.hpp"
#include "worker_suite_registry.hpp"

#include <array>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <string_view>
#include <vector>

namespace contract = aexcompat::abi::x86_64_windows;
namespace boot = aexcompat::worker_runtime::effect_bootstrap;

#ifndef _WIN32
// The macOS-local selftest links only the sampling and table owners. COPY's
// behavior has its own tests; this stand-in supplies the same typed symbol so
// get_callback_addr's address selection can be exercised without pulling in
// the full render/world graph.
namespace aexcompat::pf_world_transform {
int32_t copy_world8(void*, void*, void*, const LegacyRect*, const LegacyRect*) {
  return 4;
}
}  // namespace aexcompat::pf_world_transform
#endif

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

// A 2x2 source world, reused across depths by reassigning `pixel_bytes`, and
// resolved through the host hook the sampling runtime asks for. The pixel values
// do not matter - only that a sample with no effect_ref reaches them and reports
// success.
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
    alignas(16) std::array<unsigned char, 16> destination{};
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
    check(depth.subpixel(nullptr, half, half, nullptr, destination.data()) == 516, message);

    std::snprintf(message, sizeof(message), "subpixel_sample%s rejects a null destination",
                  depth.name);
    check(depth.subpixel(nullptr, half, half, params.data(), nullptr) == 516, message);

    // A world the resolver rejects is still refused. This stub keys on
    // pixel_bytes; the production resolver bounds-checks the struct instead
    // (worker_world_safety.cpp). Either way the point is that dropping the
    // effect_ref check did not drop the world check with it.
    World foreign;
    foreign.pixel_bytes = depth.pixel_bytes == 4 ? 8 : 4;
    auto foreign_params = sampling_params(&foreign);
    std::snprintf(message, sizeof(message), "subpixel_sample%s refuses an unresolvable world",
                  depth.name);
    check(depth.subpixel(nullptr, half, half, foreign_params.data(), destination.data()) == 516,
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

  // The callbacks that were missing in #777 and #793. The null checks below cannot catch a wiring
  // regression on their own - this function filled every slot itself - so they
  // stand as a readable name for the offsets, and the literal offsets are what
  // actually pin the layout.
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
  void* iterate_origin_non_clip{};
  void* iterate_generic_callback{};
  std::memcpy(&iterate_origin_non_clip,
              state.utils.data() + contract::UTILS_ITERATE_ORIGIN_NON_CLIP_SRC_OFFSET,
              sizeof(iterate_origin_non_clip));
  std::memcpy(&iterate_generic_callback,
              state.utils.data() + contract::UTILS_ITERATE_GENERIC_OFFSET,
              sizeof(iterate_generic_callback));
  check(iterate_origin_non_clip != nullptr,
        "utils.iterate_origin_non_clip_src is wired");
  check(iterate_generic_callback != nullptr, "utils.iterate_generic is wired");
  check(contract::UTILS_ITERATE_ORIGIN_NON_CLIP_SRC_OFFSET == 448,
        "iterate_origin_non_clip_src sits at 448");
  check(contract::UTILS_ITERATE_GENERIC_OFFSET == 456,
        "iterate_generic sits at 456");
  for (const auto [offset, name] : std::array{
           std::pair{contract::UTILS_GET_CALLBACK_ADDR_OFFSET, "utils.get_callback_addr"},
           std::pair{contract::UTILS_ANSI_COS_OFFSET, "utils.ansi_cos"},
           std::pair{contract::UTILS_ANSI_SQRT_OFFSET, "utils.ansi_sqrt"},
           std::pair{contract::UTILS_ANSI_ASIN_OFFSET, "utils.ansi_asin"},
           std::pair{contract::UTILS_ANSI_ACOS_OFFSET, "utils.ansi_acos"}}) {
    void* callback{};
    std::memcpy(&callback, state.utils.data() + offset, sizeof(callback));
    char message[96]{};
    std::snprintf(message, sizeof(message), "%s is wired", name);
    check(callback != nullptr, message);
  }
  check(contract::UTILS_GET_CALLBACK_ADDR_OFFSET == 192, "get_callback_addr sits at 192");
  check(contract::UTILS_ANSI_COS_OFFSET == 232, "ansi.cos sits at 232");
  check(contract::UTILS_ANSI_SQRT_OFFSET == 312, "ansi.sqrt sits at 312");
  check(contract::UTILS_ANSI_ASIN_OFFSET == 344, "ansi.asin sits at 344");
  check(contract::UTILS_ANSI_ACOS_OFFSET == 352, "ansi.acos sits at 352");
  // The eight ANSI slots the contract left out until issue #981. Same defect
  // shape as 472/480 above: no offset meant no binding, no binding meant a
  // null in the table, and a plug-in calling one jumped to address 0.
  check(contract::UTILS_ANSI_ATAN_OFFSET == 208, "ansi.atan sits at 208");
  check(contract::UTILS_ANSI_ATAN2_OFFSET == 216, "ansi.atan2 sits at 216");
  check(contract::UTILS_ANSI_EXP_OFFSET == 240, "ansi.exp sits at 240");
  check(contract::UTILS_ANSI_FLOOR_OFFSET == 256, "ansi.floor sits at 256");
  check(contract::UTILS_ANSI_FMOD_OFFSET == 264, "ansi.fmod sits at 264");
  check(contract::UTILS_ANSI_LOG_OFFSET == 280, "ansi.log sits at 280");
  check(contract::UTILS_ANSI_LOG10_OFFSET == 288, "ansi.log10 sits at 288");
  check(contract::UTILS_ANSI_TAN_OFFSET == 320, "ansi.tan sits at 320");
}

void get_callback_addr_is_typed_bounded_and_clears_failures() {
  for (const auto [id, expected] : std::array{
           std::pair{2, reinterpret_cast<void*>(&subpixel_sample8)},
           std::pair{3, reinterpret_cast<void*>(&area_sample8)},
           std::pair{9, reinterpret_cast<void*>(&copy_world8)},
           std::pair{31, reinterpret_cast<void*>(&subpixel_sample16)},
           std::pair{32, reinterpret_cast<void*>(&area_sample16)}}) {
    void* callback = reinterpret_cast<void*>(1);
    check(get_callback_addr(nullptr, 1, 0, id, &callback) == 0,
          "get_callback_addr accepts a supported callback id");
    check(callback == expected, "get_callback_addr returns the typed callback");
  }
  void* callback = reinterpret_cast<void*>(1);
  check(get_callback_addr(nullptr, 1, 0, 999, &callback) == 516,
        "get_callback_addr rejects an unknown callback id");
  check(callback == nullptr, "get_callback_addr clears output on rejection");
  check(get_callback_addr(nullptr, 1, 0, 9, nullptr) == 516,
        "get_callback_addr rejects a null output pointer");
}

// Every named source assigned a distinguishable, non-null, aligned stand-in,
// filled in reverse binding order: the result must still follow the generated
// ABI offsets, which is what proves construction is independent of initializer
// position (issue #792). One filler convention so the two callers below compare
// against the same values.
aexcompat::pf_utility_callbacks::Sources every_named_source() {
  aexcompat::pf_utility_callbacks::Sources sources{};
  for (std::size_t reverse = aexcompat::pf_utility_callbacks::BINDINGS.size();
       reverse > 0; --reverse) {
    const auto& binding = aexcompat::pf_utility_callbacks::BINDINGS[reverse - 1];
    sources.*(binding.source) = reinterpret_cast<void*>((reverse + 1) * sizeof(void*));
  }
  return sources;
}

void production_utility_builder_is_offset_indexed() {
  const auto sources = every_named_source();
  const auto callbacks = aexcompat::pf_utility_callbacks::build(sources);
  for (const auto& binding : aexcompat::pf_utility_callbacks::BINDINGS) {
    const auto index = aexcompat::pf_utility_callbacks::index_of(binding.offset);
    check(index < callbacks.size(), "named utility binding belongs to generated contract");
    check(callbacks[index] == sources.*(binding.source),
          "named utility source is installed at its own generated offset");
  }
}

// The negative coverage for the scan the worker's
// `--self-test-utility-callback-table` route runs: that route can only report
// what the shipping wiring produces, which is (correctly) no hole, so the
// behavior on a hole is pinned here instead.
//
// `bindings_cover_contract_once` is a static_assert over the bindings and the
// two tests above populate every source themselves, so neither can see the
// failure mode that produced #777 and #981: a slot nobody assigned, installed
// as a null pointer no host code reads.
void unwired_installed_slots_are_named_by_their_generated_offset() {
  namespace utils = aexcompat::pf_utility_callbacks;
  const auto installed = [](const boot::AbiHooks& abi) {
    boot::State state;
    boot::install_callback_tables(state, abi);
    return boot::unwired_installed_offsets(state);
  };

  const auto names_only = [](const std::vector<boot::UnwiredSlot>& slots,
                             const char* block, std::size_t offset) {
    return slots.size() == 1 && std::string_view(slots.front().block) == block &&
        slots.front().offset == offset;
  };

  boot::AbiHooks complete{};
  complete.utility_callbacks = utils::build(every_named_source());
  for (std::size_t index = 0; index < complete.input_callbacks.size(); ++index)
    complete.input_callbacks[index] =
        reinterpret_cast<void*>((index + 1) * sizeof(void*));
  static const std::array<void*, contract::UTILS_COLOR_CALLBACKS_SIZE / sizeof(void*)>
      color = [] {
        std::array<void*, contract::UTILS_COLOR_CALLBACKS_SIZE / sizeof(void*)> value{};
        for (std::size_t index = 0; index < value.size(); ++index)
          value[index] = reinterpret_cast<void*>((index + 1) * sizeof(void*));
        return value;
      }();
  complete.color_callbacks = color.data();
  complete.color_callbacks_size = sizeof(color);
  complete.basic_suite = reinterpret_cast<void*>(0x1000);
  complete.effect_ref = reinterpret_cast<void*>(0x2000);
  check(installed(complete).empty(),
        "a fully assigned AbiHooks installs no null callback");

  // One utility source left out, the way #981's eight were: the answer has to
  // name that slot and only that slot, block included - ten of the twelve inter
  // offsets are also valid utility offsets, so the number alone is ambiguous.
  boot::AbiHooks holed = complete;
  utils::Sources one_missing = every_named_source();
  one_missing.ansi_fmod = nullptr;
  holed.utility_callbacks = utils::build(one_missing);
  check(names_only(installed(holed), "utils", contract::UTILS_ANSI_FMOD_OFFSET),
        "a single unassigned utility source is named in the utils block");

  // The inter table is the same defect one struct field over, and a short brace
  // list in the production builder is not a compile error, so it has to be
  // caught here too.
  boot::AbiHooks inter_hole = complete;
  inter_hole.input_callbacks.back() = nullptr;
  check(names_only(installed(inter_hole), "inter",
                   contract::INPUT_CALLBACK_OFFSETS.back()),
        "an unassigned inter callback is named in the inter block");

  // The color block is copied whole or not at all, so a size the installer
  // refuses leaves every entry zero - every pointer in the block is reported.
  boot::AbiHooks color_mismatch = complete;
  color_mismatch.color_callbacks_size = sizeof(color) - sizeof(void*);
  check(installed(color_mismatch).size() ==
            contract::UTILS_COLOR_CALLBACKS_SIZE / sizeof(void*),
        "a color block the installer refused reports every pointer in it");

  // And a block that was copied with one null inside it, which an all-or-
  // nothing check would miss.
  auto holed_color = color;
  holed_color.back() = nullptr;
  boot::AbiHooks color_hole = complete;
  color_hole.color_callbacks = holed_color.data();
  check(names_only(installed(color_hole), "utils.color_callbacks",
                   sizeof(color) - sizeof(void*)),
        "a null inside an installed color block is named at its own offset");

  // The in_data links the same aggregate initializer supplies. `pica_basicP` is
  // how a plug-in acquires every suite it uses and `effect_ref` is the last
  // AbiHooks member, so a short or reordered initializer drops that one first.
  boot::AbiHooks no_basic_suite = complete;
  no_basic_suite.basic_suite = nullptr;
  check(names_only(installed(no_basic_suite), "in", contract::IN_PICA_BASICP_OFFSET),
        "a null pica_basicP is named in the in_data block");

  boot::AbiHooks no_effect_ref = complete;
  no_effect_ref.effect_ref = nullptr;
  check(names_only(installed(no_effect_ref), "in", contract::IN_EFFECT_REF_OFFSET),
        "a null effect_ref is named in the in_data block");
}

}  // namespace

int main() {
  aexcompat::callback_diagnostics::reset();
  a_null_effect_ref_still_samples();
  the_utility_table_is_wired_one_to_one();
  get_callback_addr_is_typed_bounded_and_clears_failures();
  production_utility_builder_is_offset_indexed();
  unwired_installed_slots_are_named_by_their_generated_offset();
  if (failures == 0)
    std::cout << "{\"pf_sampling_wiring_selftest\":\"passed\",\"callback_diagnostics\":"
              << aexcompat::callback_diagnostics::snapshot_json() << "}\n";
  return failures == 0 ? 0 : 1;
}
