#pragma once

namespace aexcompat::worker_runtime::cluster_effect_state {

// Per-member reset contract (#405, #1880), shared by render swaps and discovery
// columns (including re-inspection of the same member):
//   * parameter records, animation/extent, keyframe checkout ledger, arbitrary
//     telemetry, UI state, pending user changes, and ALL checkout runtime state;
//   * compute-cache classes, values (via their delete callbacks), and receipts;
//   * custom UI and info-text telemetry, and PF state statistics.
// Call only at the existing quiescent member boundary, after arbitrary defaults
// have been disposed and outgoing GLOBAL_SETDOWN has run, before new bootstrap.
// Compute-cache delete callbacks must still be callable at this boundary.
// This is not resource teardown: handle/suite/world ownership and leak balances
// remain cumulative for the entire process. PF state token invalidation remains
// the responsibility of GLOBAL_SETDOWN, not this statistics reset. The caller
// retains ownership of bootstrap buffers, module/string-table activation, BEE
// attribution windows, and invocation payloads, with their existing ordering.
void reset();

// Discovery's separate mask-scene boundary (#1764). Run AFTER outgoing
// GLOBAL_SETDOWN, so teardown can still see its masks, and before reset()/setup.
// Keep mask/stream ids monotonic. Live stream refs, values or AddKeyframes
// transactions (and allocation failure) must reject the reset; callers must
// propagate false as swap failure. Render requests retain their own mask scene
// lifecycle, so reset() deliberately does not call this helper.
bool reset_member_mask_scene();

}  // namespace aexcompat::worker_runtime::cluster_effect_state
