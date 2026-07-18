#include "render_subsystem.h"

namespace aexcompat::render {

int dispatch(RenderContext& context) {
  if (!context.request || !context.hooks.guarded_effect_main ||
      !context.hooks.cleanup || !context.hooks.dependencies_ready)
    return -1;

  // Module audit happens in the supplied guarded EffectMain path.  The flag is
  // retained here to make that dependency explicit at the translation-unit
  // boundary and to reject an unprepared audit request before any selector.
  if (context.module_audit_required && !context.hooks.dependencies_ready(context.request))
    return -2;
  if (!context.module_audit_required && !context.hooks.dependencies_ready(context.request))
    return -2;

  context.selector_started = true;
  context.primary_error = context.hooks.guarded_effect_main(context.request);

  // Cleanup is unconditional after selector admission.  The host hook owns
  // sequence/frame setdown, pre-render-data deletion, GPU setdown, suite
  // release, world unregistering, and automatic parameter checkins.
  context.cleanup_started = true;
  context.cleanup_error = context.hooks.cleanup(context.request);
  return context.primary_error != 0 ? context.primary_error : context.cleanup_error;
}

}  // namespace aexcompat::render
