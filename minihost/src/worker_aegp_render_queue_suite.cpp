#include "worker_aegp_render_queue_suite.hpp"
#include "worker_aegp_utility_suite.hpp"
#include "worker_suite_registry.hpp"

#include <iostream>
#include <string>

namespace aexcompat::l2_detail {
int32_t acquire_suite(const char*, int32_t, const void**);
int32_t release_suite(const char*, int32_t);
uint32_t suite_acquire_count();
uint32_t suite_release_count();
uint32_t live_suite_reference_count();
bool suite_leases_balanced();
std::string unsupported_suite_calls_report_json();

namespace {
int32_t __cdecl add_comp(void*, const char*) {
  return worker_runtime::record_unsupported_suite_call(
      worker_runtime::UnsupportedSuiteId::aegp_render_queue_1, 0);
}
int32_t __cdecl set_state(uint32_t) {
  return worker_runtime::record_unsupported_suite_call(
      worker_runtime::UnsupportedSuiteId::aegp_render_queue_1, 1);
}
int32_t __cdecl get_state(uint32_t* state) {
  if (!state) return 4;
  // No request field models an AE final/render-queue render. Classic/smart,
  // depth, quality and session lifetime do not imply a running render queue.
  *state = render_queue_stopped;
  return 0;
}
}  // namespace

RenderQueueSuite1 g_render_queue_suite1{&add_comp, &set_state, &get_state};

int selftest_render_queue_suite() {
  constexpr const char* name = "AEGP Render Queue Suite";
  const auto acquires = suite_acquire_count();
  const auto releases = suite_release_count();
  const auto live = live_suite_reference_count();
  const void* first = nullptr;
  const void* second = nullptr;
  const void* utility = nullptr;
  const void* rejected = reinterpret_cast<void*>(1);
  bool ok = acquire_suite(name, 1, &first) == 0;
  ok = (acquire_suite(name, 1, &second) == 0) && ok;
  ok = (acquire_suite("AEGP Utility Suite", 7, &utility) == 0) && ok;
  ok = (acquire_suite(name, 2, &rejected) != 0 && rejected == nullptr) && ok;
  uint32_t state = 99;
  int32_t plugin_id = -1;
  int32_t null_error = 0;
  int32_t add_error = 0;
  int32_t set_errors[4]{};
  bool unchanged = true;
  if (first && first == second && utility) {
    const auto* queue = static_cast<const RenderQueueSuite1*>(first);
    ok = (queue->get_state(&state) == 0 && state == 0) && ok;
    null_error = queue->get_state(nullptr);
    add_error = queue->add_comp(nullptr, "unused-output");
    // STOPPED, PAUSED, RENDERING and an invalid value all reject mutation.
    const uint32_t states[] = {0, 1, 2, UINT32_MAX};
    for (int i = 0; i < 4; ++i) {
      set_errors[i] = queue->set_state(states[i]);
      state = 99;
      unchanged = (queue->get_state(&state) == 0 && state == 0) && unchanged;
      ok = (set_errors[i] == 4) && ok;
    }
    const auto* util = static_cast<const UtilitySuite3*>(utility);
    ok = (util->register_with_aegp(nullptr, "render-queue-selftest", &plugin_id) == 0 &&
          plugin_id == 1 && util->register_with_aegp(nullptr, "test", nullptr) == 4) && ok;
  } else {
    ok = false;
  }
  // Release every acquired reference even if an assertion above failed.
  if (first) ok = (release_suite(name, 1) == 0) && ok;
  if (second) ok = (release_suite(name, 1) == 0) && ok;
  if (utility) ok = (release_suite("AEGP Utility Suite", 7) == 0) && ok;
  const auto acquired = suite_acquire_count() - acquires;
  const auto released = suite_release_count() - releases;
  const bool balanced = suite_leases_balanced() && live_suite_reference_count() == live;
  ok = ok && null_error == 4 && add_error == 4 && unchanged && balanced &&
       acquired == 3 && released == 3;
  // Emit observed records, not a fixed success fixture; pytest verifies slots/counts.
  std::cout << "{\"passed\":" << (ok ? "true" : "false")
            << ",\"state\":" << state << ",\"null_error\":" << null_error
            << ",\"add_error\":" << add_error << ",\"set_errors\":["
            << set_errors[0] << ',' << set_errors[1] << ',' << set_errors[2] << ','
            << set_errors[3] << "],\"plugin_id\":" << plugin_id
            << ",\"acquired\":" << acquired << ",\"released\":" << released
            << ",\"balanced\":" << (balanced ? "true" : "false")
            << unsupported_suite_calls_report_json() << "}\n";
  return ok ? 0 : 1;
}
}  // namespace aexcompat::l2_detail
