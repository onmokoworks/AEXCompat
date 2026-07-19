#include "worker_aegp_compat_selftests.hpp"
#include "worker_mask_runtime_internal.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_handle_runtime.hpp"
#include "worker_pf_helper_runtime.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstring>
#include <limits>
#include <string>

namespace aexcompat::l2_detail {
namespace { AegpCompatSelftestHooks g_hooks; }

void configure_aegp_compat_selftests(AegpCompatSelftestHooks hooks) { g_hooks = hooks; }

bool verify_legacy_effect_compat_suites() {
  return g_hooks.legacy_effect_compat && g_hooks.legacy_effect_compat();
}
bool verify_aegp_get_effect_camera() { return g_hooks.camera && g_hooks.camera(); }
bool verify_aegp_resizer_3d_chain() { return g_hooks.resizer_3d && g_hooks.resizer_3d(); }
bool verify_aegp_apply_effect() { return g_hooks.apply_effect && g_hooks.apply_effect(); }
bool verify_aegp_effect_stack() { return g_hooks.effect_stack && g_hooks.effect_stack(); }
bool verify_aegp_projector_levels() { return g_hooks.projector_levels && g_hooks.projector_levels(); }

}  // namespace aexcompat::l2_detail
