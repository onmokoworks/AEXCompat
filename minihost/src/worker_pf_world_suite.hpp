#pragma once

#include "worker_world_registry.hpp"

#include <array>

namespace aexcompat::l2_detail {

struct WorldSuite {
  decltype(&world_registry::new_world) new_world;
  decltype(&world_registry::dispose_world) dispose_world;
  decltype(&world_registry::get_pixel_format) get_pixel_format;
};
extern WorldSuite g_world_suite;
extern std::array<void*, 2> g_world_suite1;

bool verify_world_double_dispose_rejected();
bool verify_world_allocation_limit_rejected();
bool verify_owned_world_snapshot_is_atomic();
bool verify_owned_world_snapshot_concurrent_dispose();

}  // namespace aexcompat::l2_detail
