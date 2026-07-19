#include "worker_aegp_init_execution.hpp"

#include <windows.h>

namespace aexcompat::worker_runtime::aegp_init {
namespace {

void record_error(int32_t error, RoundtripResult& result) {
  if (error != 0 && result.error == 0) result.error = error;
}

void accumulate(const EventResult& event, RoundtripResult& result) {
  result.hooks_invoked += event.invoked;
  record_error(event.error, result);
}

void accumulate_command(const EventResult& event, RoundtripResult& result) {
  result.command_hooks_invoked += event.invoked;
  result.command_handled_count += event.handled_count;
  record_error(event.error, result);
}

bool wait_for_request(const std::atomic_bool& requested) {
  for (int attempt = 0; attempt < 250 && !requested; ++attempt) Sleep(10);
  return requested.load();
}

bool wait_for_response(const std::atomic_bool& received) {
  for (int attempt = 0; attempt < 100 && !received; ++attempt) Sleep(10);
  return received.load();
}

}  // namespace

RoundtripResult run_roundtrips(const RoundtripRequest& request,
                               const RoundtripValidationHooks& validation) {
  RoundtripResult result;
  const auto& modes = request.modes;
  if (!modes.active_idle && !modes.comp_idle) return result;
  const auto* commands = request.inserted_commands;
  const auto& registered = state();
  if (!commands || commands->empty() || registered.command_registrations.empty() ||
      registered.idle_registrations.empty() ||
      (modes.comp_idle && registered.update_menu_registrations.empty())) {
    result.error = 4;
    return result;
  }
  if (modes.keyframe && (!request.keyframe_probe || !request.keyframe_probe->start())) result.error = 4;
  if (modes.seek && (!request.seek_probe || !request.seek_probe->start())) result.error = 4;
  if (modes.trim && (!request.trim_probe || !request.trim_probe->start())) result.error = 4;
  if (modes.switch_flags && (!request.switch_probe || !request.switch_probe->start())) result.error = 4;

  const int32_t command = commands->front();
  const auto dispatch_command = [&]() {
    accumulate_command(aexcompat::worker_runtime::aegp_init::dispatch_command(
        request.global_refcon, command, 0, 0), result);
  };
  const auto dispatch_update_menu = [&]() {
    const auto event = aexcompat::worker_runtime::aegp_init::dispatch_update_menu(
        request.global_refcon, 0);
    result.menu_hooks_invoked += event.invoked;
    record_error(event.error, result);
  };
  dispatch_command();
  const int32_t idle_tick_count = modes.comp_idle ? 3 : 1;
  for (int32_t tick = 0; tick < idle_tick_count; ++tick) {
    if (modes.comp_idle && request.scene_frame && !(modes.seek && tick > 1))
      *request.scene_frame = tick + 1;
    if (modes.comp_idle) dispatch_update_menu();
    if (modes.keyframe || modes.seek || modes.trim || modes.switch_flags) Sleep(30);
    const auto idle_event = aexcompat::worker_runtime::aegp_init::dispatch_idle(
        request.global_refcon);
    if (idle_event.idle_max_sleep >= 0 &&
        (result.idle_max_sleep < 0 || idle_event.idle_max_sleep < result.idle_max_sleep))
      result.idle_max_sleep = idle_event.idle_max_sleep;
    accumulate(idle_event, result);
    if (modes.keyframe && tick == 0 && request.keyframe_probe &&
        !wait_for_request(request.keyframe_probe->request_sent)) record_error(4, result);
    if (modes.seek && tick == 0 && request.seek_probe &&
        !wait_for_request(request.seek_probe->request_sent)) record_error(4, result);
    if (modes.trim && tick == 0 && request.trim_probe &&
        !wait_for_request(request.trim_probe->request_sent)) record_error(4, result);
    if (modes.switch_flags && tick == 0 && request.switch_probe &&
        !wait_for_request(request.switch_probe->request_sent)) record_error(4, result);
  }
  if (modes.keyframe && request.keyframe_probe) {
    const bool received = wait_for_response(request.keyframe_probe->response_received);
    request.keyframe_probe->stop();
    if ((!received || !request.keyframe_probe->response_valid || !validation.keyframe ||
         !validation.keyframe(validation.context))) record_error(4, result);
  }
  if (modes.seek && request.seek_probe) {
    const bool received = wait_for_response(request.seek_probe->ack_received);
    request.seek_probe->stop();
    if ((!received || !request.seek_probe->ack_valid || !validation.seek ||
         !validation.seek(validation.context))) record_error(4, result);
  }
  if (modes.trim && request.trim_probe) {
    const bool received = wait_for_response(request.trim_probe->ack_received);
    request.trim_probe->stop();
    if ((!received || !request.trim_probe->ack_valid || !validation.trim ||
         !validation.trim(validation.context))) record_error(4, result);
  }
  if (modes.switch_flags && request.switch_probe) {
    const bool received = wait_for_response(request.switch_probe->ack_received);
    request.switch_probe->stop();
    if ((!received || !request.switch_probe->ack_valid || !validation.switch_flags ||
         !validation.switch_flags(validation.context))) record_error(4, result);
  }
  // Disconnect external probes before OFF so target reader threads can join.
  // Always toggle OFF before unload so plug-in worker threads are joined.
  dispatch_command();
  if (modes.comp_idle) dispatch_update_menu();
  return result;
}

}  // namespace aexcompat::worker_runtime::aegp_init
