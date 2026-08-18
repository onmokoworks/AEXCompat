#include "worker_host_guard_selftests.hpp"

#include "render_pixel_buffer.hpp"
#include "worker_host_suite_catalog.hpp"
#include "worker_pf_pixel_format_registry.hpp"
#include "worker_selector_dispatch.hpp"
#include "worker_world_registry.hpp"

#include <algorithm>
#include <cstddef>
#include <cstring>
#include <string>

#include <windows.h>

namespace aexcompat::host_guard_selftests {

using render_safety::OutputPixelBuffer;

namespace {

Hooks g_hooks{};
uint32_t g_cleanup_safety_selftest_calls{};

void __cdecl cleanup_safety_selftest_fault(void*) {
  ++g_cleanup_safety_selftest_calls;
  RaiseException(EXCEPTION_ACCESS_VIOLATION, 0, 0, nullptr);
}

}  // namespace

void configure(Hooks hooks) { g_hooks = hooks; }

uint32_t cleanup_safety_selftest_calls() { return g_cleanup_safety_selftest_calls; }

bool verify_pf_adv_app_suite_versions() {
  const void* suite1 = nullptr;
  const void* suite2 = nullptr;
  const uint32_t acquires_before = g_hooks.suite_acquire_count();
  const uint32_t releases_before = g_hooks.suite_release_count();
  bool ok = g_hooks.acquire_suite("PF AE Adv App Suite", 1, &suite1) == 0 &&
      g_hooks.acquire_suite("PF AE Adv App Suite", 2, &suite2) == 0;
  auto* slots1 = static_cast<void* const*>(suite1);
  auto* slots2 = static_cast<void* const*>(suite2);
  ok = ok && suite1 == aexcompat::worker_runtime::host_suites::adv_app_suite(1) &&
       suite2 == aexcompat::worker_runtime::host_suites::adv_app_suite(2) &&
      suite1 != suite2 && slots1 && slots2;
  if (slots1 && slots2) {
    ok = ok && std::all_of(slots1, slots1 + 10,
                           [](void* callback) { return callback != nullptr; }) &&
        std::all_of(slots2, slots2 + 11,
                    [](void* callback) { return callback != nullptr; });
    using UnsupportedProjectOperation = int32_t(__cdecl*)();
    using UnsupportedInfoColor = int32_t(__cdecl*)(uint32_t);
    using InfoDrawText3Plus = int32_t(__cdecl*)(
        const char*, const char*, const char*, const char*, const char*);
    using InfoDrawText = int32_t(__cdecl*)(const char*, const char*);
    using InfoDrawText3 = int32_t(__cdecl*)(const char*, const char*, const char*);
    for (std::size_t slot = 0; slot < 6; ++slot)
      ok = reinterpret_cast<UnsupportedProjectOperation>(slots1[slot])() != 0 && ok;
    // slot 9 (InfoDrawText3Plus) is now implemented as a no-op that reports
    // success; every argument is optional so all-null is accepted, and only an
    // over-long (>=256) string is rejected with 4 (issue #1055). slot 7
    // (InfoDrawColor) is still an unsupported stub.
    const std::string over_long(300, 'x');
    ok = ok && reinterpret_cast<UnsupportedInfoColor>(slots1[7])(0) != 0 &&
        reinterpret_cast<InfoDrawText3Plus>(slots1[9])(
            nullptr, nullptr, nullptr, nullptr, nullptr) == 0 &&
        reinterpret_cast<InfoDrawText3Plus>(slots1[9])(
            "l1", nullptr, "l2jl", nullptr, "l3jl") == 0 &&
        reinterpret_cast<InfoDrawText3Plus>(slots1[9])(
            over_long.c_str(), nullptr, nullptr, nullptr, nullptr) != 0 &&
        reinterpret_cast<InfoDrawText3Plus>(slots2[9])(
            nullptr, nullptr, nullptr, nullptr, nullptr) == 0 &&
        reinterpret_cast<InfoDrawText>(slots1[6])("suite1-line1", "suite1-line2") == 0 &&
        reinterpret_cast<InfoDrawText3>(slots1[8])(
            "suite1-line1", "suite1-line2", "suite1-line3") == 0;
    // slots 6 and 8 take the same `...Z0` arguments slot 9 does, so an absent
    // line is a null, not a bad parameter (issue #1280: Particle_Playground
    // ends its RENDER with PF_InfoDrawText("Number of particles: N", NULL) and
    // returned the rejection as PF_Err_OUT_OF_MEMORY for the whole frame).
    // Over-long strings stay rejected on every argument, which is what keeps
    // an unterminated one from being read past its end.
    //
    // The cases below go through v1 only: the catalog gives v1 and v2 the same
    // three info-text pointers, which is asserted here rather than assumed, so
    // running each case through both versions would run the same code twice.
    // The `slots2[9]` call above predates that assertion and is left alone.
    ok = ok && slots1[6] == slots2[6] && slots1[8] == slots2[8] &&
        slots1[9] == slots2[9] &&
        reinterpret_cast<InfoDrawText>(slots1[6])("suite1-line1", nullptr) == 0 &&
        reinterpret_cast<InfoDrawText>(slots1[6])(nullptr, "suite1-line2") == 0 &&
        reinterpret_cast<InfoDrawText>(slots1[6])(nullptr, nullptr) == 0 &&
        reinterpret_cast<InfoDrawText>(slots1[6])(over_long.c_str(), nullptr) != 0 &&
        reinterpret_cast<InfoDrawText>(slots1[6])(nullptr, over_long.c_str()) != 0 &&
        reinterpret_cast<InfoDrawText3>(slots1[8])("l1", nullptr, nullptr) == 0 &&
        reinterpret_cast<InfoDrawText3>(slots1[8])(nullptr, nullptr, nullptr) == 0 &&
        reinterpret_cast<InfoDrawText3>(slots1[8])(
            nullptr, over_long.c_str(), nullptr) != 0;

    // The bounds check stops at the first argument with no terminator in
    // reach, and nothing after it is read - not by the check and not by the
    // trace the same change added. A caller that got one pointer wrong may
    // have got the rest wrong too, and a diagnostic that dereferences them
    // would fault exactly where the plain build returns a clean 4. The later
    // arguments here point at a page with no access, so reading one at all
    // ends the process instead of reporting a failure.
    // Not an early return on failure: the two suites acquired above are
    // released below, and leaving without that would unbalance the leases for
    // the rest of the process.
    void* const no_access =
        VirtualAlloc(nullptr, 4096, MEM_RESERVE | MEM_COMMIT, PAGE_NOACCESS);
    ok = no_access != nullptr && ok;
    if (no_access) {
      const auto* unreadable = static_cast<const char*>(no_access);
      ok = reinterpret_cast<InfoDrawText>(slots1[6])(over_long.c_str(),
                                                     unreadable) == 4 && ok;
      ok = reinterpret_cast<InfoDrawText3>(slots1[8])(
               over_long.c_str(), unreadable, unreadable) == 4 && ok;
      ok = reinterpret_cast<InfoDrawText3Plus>(slots1[9])(
               over_long.c_str(), unreadable, unreadable, unreadable,
               unreadable) == 4 && ok;
      // Rejection in the middle of the list, not at its head: this is the case
      // that distinguishes stopping at the first bad argument from merely
      // skipping it, and the three above all pass either way.
      ok = reinterpret_cast<InfoDrawText3>(slots1[8])("ok", over_long.c_str(),
                                                      unreadable) == 4 && ok;
      ok = reinterpret_cast<InfoDrawText3Plus>(slots1[9])(
               "ok", nullptr, over_long.c_str(), unreadable,
               unreadable) == 4 && ok;
      ok = VirtualFree(no_access, 0, MEM_RELEASE) != 0 && ok;
    }
  }
  ok = g_hooks.release_suite("PF AE Adv App Suite", 2) == 0 &&
      g_hooks.release_suite("PF AE Adv App Suite", 1) == 0 && ok;
  return ok && g_hooks.suite_acquire_count() == acquires_before + 2 &&
      g_hooks.suite_release_count() == releases_before + 2 &&
      g_hooks.suite_leases_balanced();
}

bool verify_pf_pixel_format_suite_versions() {
  using namespace aexcompat::l2_detail;
  using namespace aexcompat::world_registry;
  constexpr int32_t kPublicArgb8 = 0x62677261;
  constexpr int32_t kPublicArgb16 = 0x62677241;
  constexpr int32_t kPublicArgb32f = 0x62675241;
  const void* suite1 = nullptr;
  const void* suite2 = nullptr;
  const uint32_t acquires_before = g_hooks.suite_acquire_count();
  const uint32_t releases_before = g_hooks.suite_release_count();
  const Statistics worlds_before = statistics();
  bool ok = g_hooks.acquire_suite("PF Pixel Format Suite", 1, &suite1) == 0 &&
      g_hooks.acquire_suite("PF Pixel Format Suite", 2, &suite2) == 0;
  auto* slots1 = static_cast<void* const*>(suite1);
  auto* slots2 = static_cast<void* const*>(suite2);
  ok = ok && suite1 && suite2 && suite1 != suite2 && slots1 && slots2 &&
      std::all_of(slots1, slots1 + 8,
                  [](void* callback) { return callback != nullptr; }) &&
      std::all_of(slots2, slots2 + 2,
                  [](void* callback) { return callback != nullptr; }) &&
      slots1[0] == slots2[0] && slots1[1] == slots2[1];

  if (slots1) {
    using Add = int32_t(__cdecl*)(void*, int32_t);
    using Clear = int32_t(__cdecl*)(void*);
    using NewWorld = int32_t(__cdecl*)(void*, uint32_t, uint32_t, int32_t,
                                       int32_t, void*);
    using DisposeWorld = int32_t(__cdecl*)(void*, void*);
    using GetPixelFormat = int32_t(__cdecl*)(const void*, int32_t*);
    using GetColor = int32_t(__cdecl*)(int32_t, void*);
    using ConvertColor = int32_t(__cdecl*)(int32_t, float, float, float, float,
                                           void*);
    auto add = reinterpret_cast<Add>(slots1[0]);
    auto clear = reinterpret_cast<Clear>(slots1[1]);
    auto new_world = reinterpret_cast<NewWorld>(slots1[2]);
    auto dispose = reinterpret_cast<DisposeWorld>(slots1[3]);
    auto get_format = reinterpret_cast<GetPixelFormat>(slots1[4]);
    auto get_black = reinterpret_cast<GetColor>(slots1[5]);
    auto get_white = reinterpret_cast<GetColor>(slots1[6]);
    auto convert = reinterpret_cast<ConvertColor>(slots1[7]);
    void* effect_ref = reinterpret_cast<void*>(1);
    g_global_setup_active = true;
    ok = clear(effect_ref) == 0 &&
        add(effect_ref, kPublicArgb8) == 0 && ok;
    g_global_setup_active = false;

    const std::array<std::array<int32_t, 4>, 3> world_cases{{
        {kPublicArgb8, 0, 8, 2},
        {kPublicArgb16, 3, 16, 3},
        {kPublicArgb32f, 2, 32, 3},
    }};
    for (const auto& world_case : world_cases) {
      std::array<std::byte, world_safety::kEffectWorldSize> world{};
      int32_t format{};
      int32_t rowbytes{};
      int32_t world_flags{};
      const bool created =
          new_world(effect_ref, 2, 3, world_case[1], world_case[0],
                    world.data()) == 0;
      if (created) {
        std::memcpy(&world_flags, world.data() + 16, sizeof(world_flags));
        std::memcpy(&rowbytes, world.data() + 32, sizeof(rowbytes));
      }
      const bool queried = created &&
          get_format(world.data(), &format) == 0 &&
          format == world_case[0] && rowbytes == world_case[2] &&
          world_flags == world_case[3];
      const bool disposed = created && dispose(effect_ref, world.data()) == 0;
      ok = created && queried && disposed && ok;
    }

    int32_t format{};
    std::array<uint8_t, 4> black{};
    std::array<uint8_t, 4> white{};
    std::array<uint8_t, 4> converted{};
    std::array<float, 4> converted_float{};
    std::array<std::byte, world_safety::kEffectWorldSize> invalid_world{};
    ok = get_black(kPublicArgb8, black.data()) == 0 &&
        black == std::array<uint8_t, 4>{255, 0, 0, 0} &&
        get_white(kPublicArgb8, white.data()) == 0 &&
        white == std::array<uint8_t, 4>{255, 255, 255, 255} &&
        convert(kPublicArgb8, 0.5f, -1.0f, 0.5f, 2.0f,
                converted.data()) == 0 &&
        converted == std::array<uint8_t, 4>{128, 0, 128, 255} &&
        convert(kPublicArgb32f, 0.5f, 0.25f, 0.75f, 1.0f,
                converted_float.data()) == 0 &&
        converted_float == std::array<float, 4>{0.5f, 0.25f, 0.75f, 1.0f} &&
        convert(0, 0.0f, 0.0f, 0.0f, 0.0f, converted.data()) != 0 &&
        get_format(nullptr, &format) != 0 &&
        new_world(effect_ref, 1, 1, 4, kPublicArgb8,
                  invalid_world.data()) != 0 && ok;
    g_global_setup_active = true;
    ok = clear(effect_ref) == 0 && ok;
    g_global_setup_active = false;
  }

  const bool released_v2 =
      g_hooks.release_suite("PF Pixel Format Suite", 2) == 0;
  const bool released_v1 =
      g_hooks.release_suite("PF Pixel Format Suite", 1) == 0;
  ok = released_v2 && released_v1 && ok;
  const Statistics worlds_after = statistics();
  return ok && worlds_after.created == worlds_before.created + 3 &&
      worlds_after.disposed == worlds_before.disposed + 3 &&
      worlds_after.live_count == worlds_before.live_count &&
      worlds_after.live_bytes == worlds_before.live_bytes &&
      g_hooks.suite_acquire_count() == acquires_before + 2 &&
      g_hooks.suite_release_count() == releases_before + 2 &&
      g_hooks.suite_leases_balanced();
}

bool verify_render_output_safety() {
  auto& telemetry = aexcompat::worker_runtime::selector_dispatch_telemetry();
  OutputPixelBuffer output(257);
  if (!output || !output.sentinels_intact() || !output.guard_pages_intact()) return false;
  std::memset(output.data(), 0x11, output.size());
  if (!output.sentinels_intact()) return false;
  output.data()[output.size() + OutputPixelBuffer::kSentinelBytes + 16] = 0x22;
  const bool oversized_overrun_detected = !output.sentinels_intact();
  const bool reset_ok = output.reset(8193) && output.sentinels_intact() &&
      output.guard_pages_intact();

  g_cleanup_safety_selftest_calls = 0;
  telemetry.selector.clear();
  telemetry.error = 0;
  const int32_t original_render_error = -37;
  const int32_t cleanup_error = aexcompat::worker_runtime::invoke_smart_pre_render_cleanup_seh(
      &cleanup_safety_selftest_fault, reinterpret_cast<void*>(1));
  return oversized_overrun_detected && reset_ok && cleanup_error == 512 &&
      original_render_error == -37 && g_cleanup_safety_selftest_calls == 1 &&
      telemetry.selector == "SMART_PRE_RENDER_CLEANUP" && telemetry.error == 512;
}

}  // namespace aexcompat::host_guard_selftests
