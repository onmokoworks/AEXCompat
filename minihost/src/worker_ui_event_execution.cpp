#include "worker_ui_event_execution.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstring>
#include <vector>

namespace aexcompat::worker_runtime::ui_event_execution {

CustomUiTelemetry& custom_ui_telemetry() {
  static CustomUiTelemetry telemetry;
  return telemetry;
}
namespace {
constexpr int32_t kEvent = 15;

template <typename T, std::size_t N>
void write(std::array<std::byte, N>& bytes, std::size_t offset, T value) {
  std::memcpy(bytes.data() + offset, &value, sizeof(value));
}
template <typename T, std::size_t N>
T read(const std::array<std::byte, N>& bytes, std::size_t offset) {
  T value{};
  std::memcpy(&value, bytes.data() + offset, sizeof(value));
  return value;
}
void write_rect(std::byte* bytes, int32_t size) {
  const std::array<int32_t, 4> rect{0, 0, size, size};
  std::memcpy(bytes, rect.data(), sizeof(rect));
}
struct UiScope {
  const Hooks& hooks;
  void* state{};
  UiScope(const Hooks& h, int32_t window_type) : hooks(h) {
    state = hooks.enter_ui_context(window_type);
  }
  ~UiScope() { hooks.leave_ui_context(state); }
};
}  // namespace

bool dispatch(const Request& r, const Hooks& hooks, Result& result) {
  if (!r.entry || !r.input || !r.output || !r.assignments || !r.context_slot ||
      !r.ui_context || !r.plugin_state || !hooks.invoke_entry ||
      !hooks.enter_ui_context || !hooks.leave_ui_context ||
      !hooks.context_stable || !hooks.set_context_tool) return false;

  parameter_execution::Definitions definitions(
      parameters::state().records.size() + 1);
  parameter_execution::initialize_parameter_definitions(definitions);
  const bool initialized = r.params_error == 0 &&
      r.parameter_count_contract_valid &&
      parameter_execution::initialize_arbitrary_values(
          r.entry, *r.input, *r.output, definitions);
  parameter_execution::ArbitraryValuesScope arbitrary_scope{
      initialized ? r.entry : nullptr, r.input, r.output, &definitions};
  result.event_assignments_applied = initialized &&
      (!r.assignment_mode ||
       (parameter_execution::validate_requested_assignments(*r.assignments) &&
        parameter_execution::apply_requested_assignments(definitions, *r.assignments)));
  std::vector<void*> params(definitions.size());
  for (std::size_t i = 0; i < definitions.size(); ++i)
    params[i] = definitions[i].data();

  std::array<std::byte, 208> extra{};
  UiScope ui_scope(hooks, r.window_type);
  if (!ui_scope.state) return false;
  write<void*>(extra, 0, r.context_slot);
  write<int32_t>(extra, 8, (r.lifecycle || r.idle || r.keydown || r.mouse_exited)
      ? 0 : (r.draw ? 4 : ((r.click || r.drag) ? 2 : 9)));
  if (r.draw) {
    write_rect(extra.data() + 16, 203);
    write<int32_t>(extra, 32, 32);
  } else if (r.click || r.drag) {
    write<uint32_t>(extra, 16, 1);
    write<int32_t>(extra, 20, r.click_y);
    write<int32_t>(extra, 24, r.click_x);
    write<int32_t>(extra, 28, 1);
    write<int32_t>(extra, 32, 0);
  } else {
    write<int32_t>(extra, 16, 101);
    write<int32_t>(extra, 20, 101);
    write<int32_t>(extra, 24, 0);
    write<int32_t>(extra, 28, 0);
  }
  if (r.window_type == 2) {
    write<int32_t>(extra, 80, 1);
    write<int32_t>(extra, 84, 2);
  } else {
    write_rect(extra.data() + 80, 200);
  }
  if (r.window_type != 2) {
    write<int32_t>(*r.input, 252, 200);
    write<int32_t>(*r.input, 256, 200);
    write<void*>(extra, 128, r.ui_context);
    write<void*>(extra, 136, r.transform_point);
    write<void*>(extra, 144, r.transform_point);
    write<void*>(extra, 168, r.transform_point_simple);
    write<void*>(extra, 176, r.transform_point_simple);
  }
  write_rect(extra.data() + 88, 203);
  uint32_t exception_code{};
  result.event_error = result.event_assignments_applied
      ? hooks.invoke_entry(r.entry, kEvent, r.input->data(), r.output->data(),
                           params.data(), nullptr, extra.data(), &exception_code)
      : -1;
  if (exception_code != 0) result.event_error = 512;

  if ((r.lifecycle || r.idle || r.keydown || r.mouse_exited) &&
      result.event_assignments_applied) {
    result.lifecycle_errors[0] = result.event_error;
    const std::array<int32_t, 5> events = r.idle
        ? std::array<int32_t, 5>{0, 1, 7, 5, 6}
        : (r.keydown ? std::array<int32_t, 5>{0, 1, 10, 5, 6}
          : (r.mouse_exited ? std::array<int32_t, 5>{0, 1, 11, 5, 6}
                            : std::array<int32_t, 5>{0, 1, 5, 6, -1}));
    const int count = (r.idle || r.keydown || r.mouse_exited) ? 5 : 4;
    for (int index = 1; index < count; ++index) {
      result.lifecycle_context_stable =
          result.lifecycle_context_stable &&
          read<void*>(extra, 0) == r.context_slot && hooks.context_stable();
      if (index == count - 1) {
        for (std::size_t slot = 0; slot < result.plugin_state_before_close.size(); ++slot)
          result.plugin_state_before_close[slot] =
              static_cast<uintptr_t>(r.plugin_state[slot]);
      }
      write<int32_t>(extra, 8, events[index]);
      if (r.keydown && index == 2) {
        write<uint32_t>(extra, 16, 1);
        write<int32_t>(extra, 20, r.click_y);
        write<int32_t>(extra, 24, r.click_x);
        write<uint32_t>(extra, 28, r.keydown_code);
        write<uint32_t>(extra, 32, r.keydown_modifiers);
      }
      exception_code = 0;
      result.lifecycle_errors[index] = hooks.invoke_entry(
          r.entry, kEvent, r.input->data(), r.output->data(), params.data(),
          nullptr, extra.data(), &exception_code);
      if (exception_code != 0) result.lifecycle_errors[index] = 512;
    }
    result.event_error = 0;
    for (const int32_t error : result.lifecycle_errors) {
      if (error != 0) { result.event_error = error; break; }
    }
    std::fill_n(r.plugin_state, 4, intptr_t{});
    hooks.set_context_tool(r.window_type);
    result.lifecycle_host_state_cleared =
        std::all_of(r.plugin_state, r.plugin_state + 4,
                    [](intptr_t state) { return state == 0; });
  }
  if (r.drag && result.event_error == 0) {
    result.drag_requested = read<uint8_t>(extra, 72) != 0;
    for (int32_t step = 1; result.drag_requested && step <= r.drag_steps; ++step) {
      write<int32_t>(extra, 8, 3);
      write<int32_t>(extra, 20,
          r.click_y + (r.drag_end_y - r.click_y) * step / r.drag_steps);
      write<int32_t>(extra, 24,
          r.click_x + (r.drag_end_x - r.click_x) * step / r.drag_steps);
      write<uint8_t>(extra, 73, step == r.drag_steps ? 1 : 0);
      write<int32_t>(extra, 204, 0);
      exception_code = 0;
      result.event_error = hooks.invoke_entry(
          r.entry, kEvent, r.input->data(), r.output->data(), params.data(),
          nullptr, extra.data(), &exception_code);
      ++result.drag_calls;
      if (exception_code != 0) { result.event_error = 512; break; }
    }
    result.drag_terminated = result.drag_calls == static_cast<uint32_t>(r.drag_steps) &&
        read<uint8_t>(extra, 72) == 0 && read<uint8_t>(extra, 73) != 0;
  }
  result.cursor = r.adjust_cursor ? read<int32_t>(extra, 28) : 0;
  result.event_out_flags = read<int32_t>(extra, 204);
  result.changed_value = definitions.size() > 1 &&
      (read<uint32_t>(definitions[1], 0) & 1u) != 0;
  return true;
}

}  // namespace aexcompat::worker_runtime::ui_event_execution
