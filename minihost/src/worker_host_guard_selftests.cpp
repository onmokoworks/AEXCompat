#include "worker_host_guard_selftests.hpp"

#include "render_pixel_buffer.hpp"
#include "worker_host_suite_catalog.hpp"
#include "worker_selector_dispatch.hpp"

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
  }
  ok = g_hooks.release_suite("PF AE Adv App Suite", 2) == 0 &&
      g_hooks.release_suite("PF AE Adv App Suite", 1) == 0 && ok;
  return ok && g_hooks.suite_acquire_count() == acquires_before + 2 &&
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
