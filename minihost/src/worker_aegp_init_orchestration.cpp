#include "worker_aegp_init_orchestration.hpp"
#include "worker_suite_registry.hpp"

namespace aexcompat::worker_runtime::aegp_init {
namespace {

void record_error(int32_t error, int32_t& destination) {
  if (error != 0 && destination == 0) destination = error;
}

}  // namespace

OrchestrationResult run_orchestration(
    const OrchestrationRequest& request,
    const RoundtripValidationHooks& validation) {
  OrchestrationResult result;
  const auto entry_result = aegp_entry_guard::invoke(
      request.entry, request.basic_suite, 24, 0, 1,
      &result.global_refcon);
  result.init_error = entry_result.error;
  result.entry_fault = entry_result.fault;
  result.entry_exception_code = entry_result.seh_code;
  result.entry_invoked = entry_result.invoked;
  if (result.entry_fault != aegp_entry_guard::FaultKind::none)
    result.forced_suite_releases =
        worker_runtime::suite_registry().force_release_all();

  if (result.init_error == 0) {
    const bool command_ready = request.inserted_commands &&
        !request.inserted_commands->empty() &&
        !state().command_registrations.empty();
    if (request.modes.command_roundtrip && !command_ready) {
      result.event_error = 4;
    } else {
      const int32_t command = command_ready
          ? request.inserted_commands->front() : 0;
      const auto events = dispatch_basic_events(
          result.global_refcon, request.modes.update_menu, request.modes.idle,
          request.modes.command_roundtrip, command);
      result.hooks_invoked += events.hooks_invoked;
      result.menu_hooks_invoked += events.menu_hooks_invoked;
      result.command_hooks_invoked += events.command_hooks_invoked;
      result.command_handled_count += events.command_handled_count;
      result.idle_max_sleep = events.idle_max_sleep;
      record_error(events.error, result.event_error);
    }
  }

  if (result.init_error == 0) {
    const auto roundtrip = run_roundtrips(
        {result.global_refcon, request.inserted_commands, request.scene_frame,
         request.keyframe_probe, request.seek_probe, request.trim_probe,
         request.switch_probe, request.modes.roundtrip},
        validation);
    record_error(roundtrip.error, result.event_error);
    result.hooks_invoked += roundtrip.hooks_invoked;
    result.menu_hooks_invoked += roundtrip.menu_hooks_invoked;
    result.command_hooks_invoked += roundtrip.command_hooks_invoked;
    result.command_handled_count += roundtrip.command_handled_count;
    if (roundtrip.idle_max_sleep >= 0 &&
        (result.idle_max_sleep < 0 ||
         roundtrip.idle_max_sleep < result.idle_max_sleep)) {
      result.idle_max_sleep = roundtrip.idle_max_sleep;
    }
  }

  if (result.init_error == 0) {
    const auto death = dispatch_death(result.global_refcon);
    result.death_hooks_invoked += death.invoked;
    record_error(death.error, result.death_error);
  }
  return result;
}

}  // namespace aexcompat::worker_runtime::aegp_init
