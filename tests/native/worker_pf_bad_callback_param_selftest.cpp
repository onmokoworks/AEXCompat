#include "worker_pf_world_transform_runtime.hpp"
#include "worker_extended_diag.hpp"
#include "worker_world_safety.hpp"

#include <cstdlib>
#include <iostream>

int main() {
  // The operand trace is part of what this harness pins (issue #1289), and
  // `extended_diag_enabled()` caches its answer on first use, so the variable
  // has to be set before any callback runs. Everything else the harness calls
  // becomes chattier on stderr, which nothing here reads.
#ifdef _WIN32
  SetEnvironmentVariableW(L"AEXCOMPAT_EXTENDED_DIAG", L"1");
#else
  setenv("AEXCOMPAT_EXTENDED_DIAG", "1", 1);
#endif
  if (!aexcompat::l2_detail::extended_diag_enabled()) return 10;
  uint32_t calls{};
  int32_t last_x{}, last_y{};
  uint8_t last_opacity{};
  const auto configure_with_owned_hook = [&](bool (*world_pixels_owned)(void*)) {
    aexcompat::pf_world_transform::configure({
        {&aexcompat::world_safety::bounded_typed_world,
         &aexcompat::world_safety::resolve_registered_dispatch_world,
         +[]() -> const char* { return "argb8"; },
         +[](const char*) -> bool { return true; },
         &aexcompat::world_safety::bounded_argb8_world,
         world_pixels_owned},
        {&calls, &last_x, &last_y, &last_opacity}});
  };
  // This harness has no ownership registry: every world is either
  // dispatch-registered or foreign, which is what lets the clipping self-test
  // exercise the foreign-operand fallback directly.
  configure_with_owned_hook(+[](void*) -> bool { return false; });
  if (!aexcompat::pf_world_transform::verify_world_transform_blend()) return 1;
  if (!aexcompat::pf_world_transform::verify_bad_callback_param_contract()) return 2;
  if (!aexcompat::pf_world_transform::verify_copy_world_clipping()) return 3;
  if (!aexcompat::pf_world_transform::verify_world_transform_affine()) return 4;
  // TRANSFORM_WORLD shares that fallback (issue #1289): the admitting side
  // here, the gate below.
  if (!aexcompat::pf_world_transform::verify_transform_world_foreign_operand(true))
    return 7;
  // The fallback's gate: with `world_pixels_owned` answering true (a stand-in
  // for "this base is a host allocation") the same foreign world the clipping
  // test admits must be refused, and a null hook must disable the fallback
  // outright.
  configure_with_owned_hook(+[](void*) -> bool { return true; });
  if (!aexcompat::pf_world_transform::verify_copy_foreign_world_gate()) return 5;
  if (!aexcompat::pf_world_transform::verify_transform_world_foreign_operand(false))
    return 8;
  configure_with_owned_hook(nullptr);
  if (!aexcompat::pf_world_transform::verify_copy_foreign_world_gate()) return 6;
  if (!aexcompat::pf_world_transform::verify_transform_world_foreign_operand(false))
    return 9;
  std::cout << "{\"pf_bad_callback_param\":\"passed\"}\n";
  return 0;
}
