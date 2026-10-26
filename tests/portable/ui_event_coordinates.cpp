// Link the production dispatcher with a minimal parameter host and inspect the
// event ABI delivered to an effect. No Windows process or Adobe host is implied.
#include "worker_ui_event_execution.hpp"
#include <cassert>
#include <cstring>
#include <vector>
using namespace aexcompat::worker_runtime;
namespace aexcompat::worker_runtime::parameters {
State& state() noexcept { static State state; return state; }
}
namespace aexcompat::worker_runtime::parameter_execution {
void initialize_parameter_definitions(Definitions&, int32_t, int32_t) {}
bool initialize_arbitrary_values(EffectEntry, BufferIn&, BufferOut&, Definitions&) { return true; }
bool validate_requested_assignments(const parameters::RequestedAssignments&) { return true; }
bool apply_requested_assignments(Definitions&, const parameters::RequestedAssignments&, int32_t, int32_t) { return true; }
ArbitraryValuesScope::~ArbitraryValuesScope() = default;
}
struct Point { int32_t h, v; };
struct Click { uint32_t modifiers; Point where; int32_t clicks; int32_t modifiers2; };
static_assert(offsetof(Click, where) == 4 && offsetof(Point, v) == 4);
static std::vector<Point> points;
static std::vector<int32_t> events;
static int32_t __cdecl effect(int32_t, void*, void*, void**, void*, void*) { return 0; }
static int32_t invoke(parameter_execution::EffectEntry, int32_t selector, void*, void*, void**, void*, void* raw, uint32_t*) {
  assert(selector == 15);
  auto* bytes = static_cast<unsigned char*>(raw);
  int32_t event{}; std::memcpy(&event, bytes + 8, 4);
  if (event == 2 || event == 3 || event == 10) {
    Click click{}; std::memcpy(&click, bytes + 16, sizeof(click));
    points.push_back(click.where); events.push_back(event);
  }
  if (event == 2) bytes[72] = 1;
  if (event == 3 && bytes[73]) bytes[72] = 0;
  return 0;
}
int main() {
  namespace ui = ui_event_execution;
  parameter_execution::BufferIn input{}; parameter_execution::BufferOut output{};
  parameters::RequestedAssignments assignments;
  intptr_t state[4]{};
  ui::Request request{};
  request.entry = effect; request.input = &input; request.output = &output;
  request.assignments = &assignments; request.parameter_count_contract_valid = true;
  request.context_slot = &state; request.ui_context = &state; request.plugin_state = state;
  request.window_type = 2; request.click_x = 17; request.click_y = -29;
  request.drag_end_x = 23; request.drag_end_y = -5; request.drag_steps = 3;
  ui::Hooks hooks{invoke, [](int32_t) -> void* { return &points; }, [](void*) {}, [] { return true; }, [](int32_t) {}};
  request.drag = true;
  ui::Result result;
  assert(ui::dispatch(request, hooks, result));
  assert(result.event_error == 0 && result.drag_terminated);
  assert(events == std::vector<int32_t>({2,3,3,3}));
  assert(points.size() == 4);
  for (int i = 0; i < 4; ++i) { assert(points[i].h == 17 + 2*i); assert(points[i].v == -29 + 8*i); }
  points.clear(); events.clear(); request.drag = false; request.keydown = true;
  assert(ui::dispatch(request, hooks, result));
  assert(events == std::vector<int32_t>({10}));
  assert(points[0].h == 17 && points[0].v == -29);
}
