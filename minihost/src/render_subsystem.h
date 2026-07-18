#pragma once

// Render dispatch is deliberately opaque to the command-line worker.  The
// runtime owns PF world, parameter, suite, and module-audit state; this
// boundary only owns selector admission, error priority, and cleanup order.
// Keeping it in a normal header/cpp pair prevents render code from being
// textually included into l2_main.cpp.
namespace aexcompat::render {

enum class RenderKind { Classic, SmartPreRenderAndRender };

struct HostHooks {
  // Calls EffectMain through the host's SEH/module-audit guarded path.
  int (*guarded_effect_main)(void* request){};
  // Restores world registrations, suite leases, parameter checkouts and
  // pre-render data.  It must be safe after a partially completed dispatch.
  int (*cleanup)(void* request){};
  // Verifies the runtime prepared all dependencies before selector admission.
  bool (*dependencies_ready)(void* request){};
};

struct RenderContext {
  RenderKind kind{};
  void* request{};
  HostHooks hooks{};
  bool module_audit_required{};
  bool selector_started{};
  bool cleanup_started{};
  int primary_error{};
  int cleanup_error{};
};

// Dispatches a fully prepared request exactly once.  A cleanup failure only
// wins when the selector path succeeded, preserving the native failure
// priority used by Classic and SmartFX GPU/CPU lifecycles.
int dispatch(RenderContext& context);

}  // namespace aexcompat::render
