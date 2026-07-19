#pragma once

#include "worker_parameter_execution.hpp"

#include <array>
#include <cstdint>

namespace aexcompat::worker_runtime::ui_event_execution {

using parameter_execution::BufferIn;
using parameter_execution::BufferOut;
using parameter_execution::EffectEntry;
using parameters::RequestedAssignments;

struct Request {
  EffectEntry entry{};
  BufferIn* input{};
  BufferOut* output{};
  int32_t params_error{};
  bool parameter_count_contract_valid{};
  const RequestedAssignments* assignments{};
  bool assignment_mode{};
  bool adjust_cursor{};
  bool draw{};
  bool click{};
  bool drag{};
  bool lifecycle{};
  bool idle{};
  bool keydown{};
  bool mouse_exited{};
  int32_t click_x{};
  int32_t click_y{};
  int32_t drag_end_x{};
  int32_t drag_end_y{};
  int32_t drag_steps{};
  uint32_t keydown_code{};
  uint32_t keydown_modifiers{};
  int32_t window_type{};
  void* context_slot{};
  void* ui_context{};
  intptr_t* plugin_state{};
  void* transform_point{};
  void* transform_point_simple{};
};

struct Hooks {
  int32_t (*invoke_entry)(EffectEntry, int32_t, void*, void*, void**, void*,
                          void*, uint32_t*){};
  void* (*enter_ui_context)(int32_t){};
  void (*leave_ui_context)(void*){};
  bool (*context_stable)(){};
  void (*set_context_tool)(int32_t){};
};

struct Result {
  int32_t event_error{-1};
  int32_t cursor{};
  int32_t event_out_flags{};
  bool changed_value{};
  std::array<int32_t, 5> lifecycle_errors{-1, -1, -1, -1, -1};
  std::array<uintptr_t, 4> plugin_state_before_close{};
  bool lifecycle_context_stable{true};
  bool lifecycle_host_state_cleared{};
  bool event_assignments_applied{};
  bool drag_requested{};
  uint32_t drag_calls{};
  bool drag_terminated{};
};

bool dispatch(const Request&, const Hooks&, Result&);

}  // namespace aexcompat::worker_runtime::ui_event_execution
