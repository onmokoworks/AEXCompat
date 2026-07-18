// Copyright (c) AEXCompat contributors.
// Independent object-file boundary for native AEGP scene self-test support.

#include "worker_aegp_scene.hpp"

bool scene_selftests_translation_unit_linked() noexcept {
  return scene_translation_unit_linked() && scene_context()->hooks.suite_lease_balanced();
}
