#include "worker_pf_world_transform_runtime.hpp"
#include "worker_world_safety.hpp"

#include <iostream>

int main() {
  uint32_t calls{};
  int32_t last_x{}, last_y{};
  uint8_t last_opacity{};
  aexcompat::pf_world_transform::configure({
      {&aexcompat::world_safety::bounded_typed_world,
       &aexcompat::world_safety::resolve_registered_dispatch_world,
       +[]() -> const char* { return "argb8"; },
       +[](const char*) -> bool { return true; },
       &aexcompat::world_safety::bounded_argb8_world},
      {&calls, &last_x, &last_y, &last_opacity}});
  if (!aexcompat::pf_world_transform::verify_world_transform_blend()) return 1;
  if (!aexcompat::pf_world_transform::verify_bad_callback_param_contract()) return 2;
  std::cout << "{\"pf_bad_callback_param\":\"passed\"}\n";
  return 0;
}
