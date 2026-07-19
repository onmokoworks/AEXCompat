#pragma once

#include "runtime_module_audit.hpp"
#include "worker_aegp_render_options.hpp"
#include "worker_aegp_scene.hpp"
#include "worker_aegp_scene_runtime.hpp"
#include "worker_pf_ae_channel_runtime.hpp"
#include "worker_pf_adv_time_suite.hpp"
#include "worker_pf_state_runtime.hpp"
#include "worker_pf_suites_internal.hpp"
#include "worker_pf_world_transform_runtime.hpp"
#include "worker_selector_dispatch.hpp"

namespace aexcompat::worker_runtime::entry_bootstrap {

struct Hooks {
  pf_state_runtime::HostHooks pf_state{};
  pf_ae_channel::HostHooks pf_ae_channel{};
  SceneContext scene{};
  scene_runtime::SceneRuntimeContext scene_runtime{};
  render_options::ItemValidator validate_item{};
  render_options::LayerInitializer initialize_layer{};
  void* effect_ref{};
  PfHostContext pf{};
  pf_world_transform::Context world_transform{};
  pf_adv_time::VerificationHooks adv_time{};
  FileSha256 hash{};
  AuditCapture audit_capture{};
  AuditPassed audit_passed{};
  SelectorDispatchTrace trace{};
};

// Installs all process-local host hooks in the historical order. The return
// values intentionally retain the worker's existing fail-closed exit codes.
int configure(const Hooks& hooks) noexcept;

}  // namespace aexcompat::worker_runtime::entry_bootstrap
