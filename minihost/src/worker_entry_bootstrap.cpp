#include "worker_entry_bootstrap.hpp"

#include "pf_cache_on_load_suite.hpp"

namespace aexcompat::worker_runtime::entry_bootstrap {

int configure(const Hooks& hooks) noexcept {
  pf_state_runtime::configure_host_hooks(hooks.pf_state);
  pf_ae_channel::configure_host_hooks(hooks.pf_ae_channel);
  if (!configure_scene_context(hooks.scene) || !scene_translation_unit_linked() ||
      !scene_runtime::configure_scene_runtime_context(hooks.scene_runtime) ||
      !scene_runtime::scene_runtime_translation_unit_linked() ||
      !scene_selftests_translation_unit_linked())
    return 23;
  render_options::configure_validators(hooks.validate_item, hooks.initialize_layer);
  suites::configure_cache_on_load_suite(hooks.effect_ref);
  configure_pf_host_context(hooks.pf);
  if (!pf_host_context_configured()) return 72;
  pf_world_transform::configure(hooks.world_transform);
  if (!pf_world_transform::configured()) return 74;
  if (!pf_adv_time::configure_verification_hooks(hooks.adv_time)) return 73;
  configure_runtime_module_hash(hooks.hash);
  configure_selector_dispatch_audit(hooks.audit_capture, hooks.audit_passed);
  configure_selector_dispatch_trace(hooks.trace);
  return 0;
}

}  // namespace aexcompat::worker_runtime::entry_bootstrap
