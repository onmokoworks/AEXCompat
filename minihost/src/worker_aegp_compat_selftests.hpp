#pragma once
namespace aexcompat::l2_detail {
struct AegpCompatSelftestHooks {
  bool (*legacy_effect_compat)(){};
  bool (*camera)(){};
  bool (*resizer_3d)(){};
  bool (*apply_effect)(){};
  bool (*effect_stack)(){};
  bool (*projector_levels)(){};
};
void configure_aegp_compat_selftests(AegpCompatSelftestHooks hooks);
bool verify_legacy_effect_compat_suites();
bool verify_aegp_get_effect_camera();
bool verify_aegp_resizer_3d_chain();
bool verify_aegp_apply_effect();
bool verify_aegp_effect_stack();
bool verify_aegp_projector_levels();
}
