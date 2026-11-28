#include "worker_abi_offsets.hpp"
#include "worker_ui_event_execution.hpp"

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstring>
#include <mutex>
#include <utility>
#include <vector>

namespace abi_contract = aexcompat::abi::x86_64_windows;
namespace abi_offsets = aexcompat::worker_abi_offsets;

namespace aexcompat::worker_runtime::ui_event_execution {

CustomUiTelemetry& custom_ui_telemetry() {
  static CustomUiTelemetry telemetry;
  return telemetry;
}
namespace {
struct InfoTextTelemetry {
  std::mutex mutex;
  uint32_t calls{};
  std::string last_text;
};

InfoTextTelemetry& info_text_telemetry() {
  static InfoTextTelemetry telemetry;
  return telemetry;
}

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

void record_info_text(std::string text) {
  auto& telemetry = info_text_telemetry();
  std::lock_guard<std::mutex> lock(telemetry.mutex);
  telemetry.last_text = std::move(text);
  ++telemetry.calls;
}

InfoTextTelemetrySnapshot snapshot_info_text_telemetry() {
  auto& telemetry = info_text_telemetry();
  std::lock_guard<std::mutex> lock(telemetry.mutex);
  return {telemetry.calls, telemetry.last_text};
}

void reset_info_text_telemetry() {
  auto& telemetry = info_text_telemetry();
  std::lock_guard<std::mutex> lock(telemetry.mutex);
  telemetry.calls = 0;
  telemetry.last_text.clear();
}

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
  write<void*>(extra, abi_contract::EVENT_CONTEXT_OFFSET, r.context_slot);
  write<int32_t>(extra, abi_contract::EVENT_TYPE_OFFSET,
      (r.lifecycle || r.idle || r.keydown || r.mouse_exited)
      ? 0 : (r.draw ? 4 : ((r.click || r.drag) ? 2 : 9)));
  if (r.draw) {
    write_rect(extra.data() + 16, 203);
    write<int32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_offsets::kDrawDepth, 32);
  } else if (r.click || r.drag) {
    // PF_Point stores horizontal/x first, then vertical/y (also for drag/key).
    write<uint32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_WHEN_OFFSET, 1);
    write<int32_t>(extra,
        abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_SCREEN_POINT_OFFSET + abi_offsets::kPointHorizontal, r.click_x);
    write<int32_t>(extra,
        abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_SCREEN_POINT_OFFSET + abi_offsets::kPointVertical, r.click_y);
    write<int32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_NUM_CLICKS_OFFSET, 1);
    write<int32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_MODIFIERS_OFFSET, 0);
  } else {
    write<int32_t>(extra,
        abi_contract::EVENT_UNION_OFFSET + abi_contract::ADJUST_CURSOR_SCREEN_POINT_OFFSET + abi_offsets::kPointHorizontal, 101);
    write<int32_t>(extra,
        abi_contract::EVENT_UNION_OFFSET + abi_contract::ADJUST_CURSOR_SCREEN_POINT_OFFSET + abi_offsets::kPointVertical, 101);
    write<int32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::ADJUST_CURSOR_MODIFIERS_OFFSET, 0);
    write<int32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::ADJUST_CURSOR_SET_CURSOR_OFFSET,
        0);
  }
  if (r.window_type == 2) {
    write<int32_t>(extra, abi_contract::EVENT_EFFECT_WINDOW_OFFSET + abi_contract::EFFECT_WINDOW_INDEX_OFFSET,
        1);
    write<int32_t>(extra, abi_contract::EVENT_EFFECT_WINDOW_OFFSET + abi_contract::EFFECT_WINDOW_AREA_OFFSET,
        2);
  } else {
    write_rect(extra.data() + 80, 200);
  }
  if (r.window_type != 2) {
    write<int32_t>(*r.input, abi_contract::IN_WIDTH_OFFSET, 200);
    write<int32_t>(*r.input, abi_contract::IN_HEIGHT_OFFSET, 200);
    write<void*>(extra, abi_contract::EVENT_CALLBACKS_OFFSET + abi_offsets::kEventCallbacksRefcon,
        r.ui_context);
    write<void*>(extra, abi_contract::EVENT_CALLBACKS_OFFSET + abi_offsets::kEventCallbacksLayerToComp,
        r.transform_point);
    write<void*>(extra, abi_contract::EVENT_CALLBACKS_OFFSET + abi_offsets::kEventCallbacksCompToLayer,
        r.transform_point);
    write<void*>(extra, abi_contract::EVENT_CALLBACKS_OFFSET + abi_offsets::kEventCallbacksSourceToFrame,
        r.transform_point_simple);
    write<void*>(extra, abi_contract::EVENT_CALLBACKS_OFFSET + abi_offsets::kEventCallbacksFrameToSource,
        r.transform_point_simple);
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
          read<void*>(extra, abi_contract::EVENT_CONTEXT_OFFSET) == r.context_slot && hooks.context_stable();
      if (index == count - 1) {
        for (std::size_t slot = 0; slot < result.plugin_state_before_close.size(); ++slot)
          result.plugin_state_before_close[slot] =
              static_cast<uintptr_t>(r.plugin_state[slot]);
      }
      write<int32_t>(extra, abi_contract::EVENT_TYPE_OFFSET, events[index]);
      if (r.keydown && index == 2) {
        write<uint32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::KEY_DOWN_WHEN_OFFSET, 1);
        write<int32_t>(extra,
            abi_contract::EVENT_UNION_OFFSET + abi_contract::KEY_DOWN_SCREEN_POINT_OFFSET + abi_offsets::kPointHorizontal, r.click_x);
        write<int32_t>(extra,
            abi_contract::EVENT_UNION_OFFSET + abi_contract::KEY_DOWN_SCREEN_POINT_OFFSET + abi_offsets::kPointVertical, r.click_y);
        write<uint32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::KEY_DOWN_KEYCODE_OFFSET,
            r.keydown_code);
        write<uint32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::KEY_DOWN_MODIFIERS_OFFSET,
            r.keydown_modifiers);
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
    result.drag_requested = read<uint8_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_SEND_DRAG_OFFSET) != 0;
    for (int32_t step = 1; result.drag_requested && step <= r.drag_steps; ++step) {
      write<int32_t>(extra, abi_contract::EVENT_TYPE_OFFSET, 3);
      write<int32_t>(extra,
          abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_SCREEN_POINT_OFFSET + abi_offsets::kPointHorizontal,
          r.click_x + (r.drag_end_x - r.click_x) * step / r.drag_steps);
      write<int32_t>(extra,
          abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_SCREEN_POINT_OFFSET + abi_offsets::kPointVertical,
          r.click_y + (r.drag_end_y - r.click_y) * step / r.drag_steps);
      write<uint8_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_LAST_TIME_OFFSET,
          step == r.drag_steps ? 1 : 0);
      write<int32_t>(extra, abi_contract::EVENT_OUT_FLAGS_OFFSET, 0);
      exception_code = 0;
      result.event_error = hooks.invoke_entry(
          r.entry, kEvent, r.input->data(), r.output->data(), params.data(),
          nullptr, extra.data(), &exception_code);
      ++result.drag_calls;
      if (exception_code != 0) { result.event_error = 512; break; }
    }
    result.drag_terminated = result.drag_calls == static_cast<uint32_t>(r.drag_steps) &&
        read<uint8_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_SEND_DRAG_OFFSET) == 0 && read<uint8_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::DO_CLICK_LAST_TIME_OFFSET) != 0;
  }
  result.cursor = r.adjust_cursor ? read<int32_t>(extra, abi_contract::EVENT_UNION_OFFSET + abi_contract::ADJUST_CURSOR_SET_CURSOR_OFFSET) : 0;
  result.event_out_flags = read<int32_t>(extra, abi_contract::EVENT_OUT_FLAGS_OFFSET);
  result.changed_value = definitions.size() > 1 &&
      (read<uint32_t>(definitions[1], abi_contract::PARAM_UU_OFFSET) & 1u) != 0;
  return true;
}

}  // namespace aexcompat::worker_runtime::ui_event_execution
