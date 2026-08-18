// Self-test for the Sweet Pea bootstrap decision (issue #1279): U.dll's own
// `U_SP_Birth` is preferred over starting ae_sweetpea directly, because only
// that path registers the host plug-in whose Startup message makes
// `U_SP_GetSPBasicSuite` stop answering 11. The decision is asked on every
// suite acquire (the suite resolver is not cached), so what it must pin is
// how often it lets an attempt through: once per mapping, never again after
// a failure on that same mapping, and re-entrantly safe because the attempt
// is recorded before the call. Teardown has to unwind through whichever layer
// actually started Sweet Pea, on the mapping it started.
//
// This covers the pure decision only; the glue in
// worker_host_suite_wiring.cpp that feeds it GetModuleHandleW/GetProcAddress
// and calls U_SP_Birth is exercised by the real worker.
#include "worker_sweetpea_bootstrap.hpp"

#include <cstdio>
#include <string>
#include <vector>

namespace {

namespace sweetpea = aexcompat::worker_runtime::sweetpea_bootstrap;

using sweetpea::Decision;
using sweetpea::State;

const char* name(Decision decision) {
  switch (decision) {
    case Decision::AlreadyBootstrappedThroughU: return "already_through_u";
    case Decision::CallUSpBirth: return "call_u_sp_birth";
    case Decision::StartDirectly: return "start_directly";
    case Decision::Nothing: return "nothing";
  }
  return "?";
}

struct Failures {
  std::vector<std::string> messages;

  void expect(const char* label, Decision expected, Decision actual) {
    if (expected == actual) return;
    messages.push_back(std::string(label) + ":expected=" + name(expected) +
                       ":actual=" + name(actual));
  }
  void expect(const char* label, unsigned long expected,
              unsigned long actual) {
    if (expected == actual) return;
    messages.push_back(std::string(label) +
                       ":expected=" + std::to_string(expected) +
                       ":actual=" + std::to_string(actual));
  }
  void expect(const char* label, const void* expected, const void* actual) {
    if (expected == actual) return;
    messages.push_back(std::string(label) +
                       (expected ? ":expected=module" : ":expected=null") +
                       (actual ? ":actual=module" : ":actual=null"));
  }
};

// Drives the decision the way ensure_sweetpea_started does: record the
// attempt first, then apply the outcome. `asks` is how many suite acquires
// arrive; the counters in State say how many attempts they produced.
void drive(State& state, HMODULE u_module, bool u_sp_birth_exported,
           HMODULE sweetpea_module, bool u_succeeds, bool direct_succeeds,
           int asks) {
  for (int index = 0; index < asks; ++index) {
    switch (sweetpea::decide(state, u_module, u_sp_birth_exported,
                             sweetpea_module)) {
      case Decision::AlreadyBootstrappedThroughU:
      case Decision::Nothing:
        break;
      case Decision::CallUSpBirth:
        sweetpea::begin_u_sp_birth(state, u_module);
        sweetpea::finish_u_sp_birth(state, u_module, u_succeeds);
        break;
      case Decision::StartDirectly:
        sweetpea::begin_direct_start(state, sweetpea_module);
        sweetpea::finish_direct_start(state, direct_succeeds);
        break;
    }
  }
}

}  // namespace

int main() {
  // Fake mappings: only identity matters to the latch.
  alignas(16) static unsigned char first_mapping[16]{};
  alignas(16) static unsigned char second_mapping[16]{};
  alignas(16) static unsigned char sp_mapping[16]{};
  const HMODULE first = reinterpret_cast<HMODULE>(first_mapping);
  const HMODULE second = reinterpret_cast<HMODULE>(second_mapping);
  const HMODULE sweetpea_module = reinterpret_cast<HMODULE>(sp_mapping);

  Failures failures;
  std::size_t checks = 0;
  const auto check = [&](auto&&... args) {
    ++checks;
    failures.expect(std::forward<decltype(args)>(args)...);
  };

  // A closure with U.dll takes U's bootstrap, and ten acquires produce one
  // attempt, not ten.
  {
    State state;
    check("u_dll_present_prefers_u_sp_birth", Decision::CallUSpBirth,
          sweetpea::decide(state, first, true, sweetpea_module));
    drive(state, first, true, sweetpea_module, true, true, 10);
    check("u_bootstrap_is_attempted_once", 1UL,
          static_cast<unsigned long>(state.u_attempts));
    check("u_bootstrap_never_starts_sweetpea_directly", 0UL,
          static_cast<unsigned long>(state.direct_attempts));
    check("u_bootstrap_is_done", Decision::AlreadyBootstrappedThroughU,
          sweetpea::decide(state, first, true, sweetpea_module));
    // A member without U.dll afterwards must not start Sweet Pea again, and
    // neither must a second U.dll mapping.
    check("absent_member_after_u_bootstrap",
          Decision::AlreadyBootstrappedThroughU,
          sweetpea::decide(state, nullptr, false, nullptr));
    check("second_mapping_after_u_bootstrap",
          Decision::AlreadyBootstrappedThroughU,
          sweetpea::decide(state, second, true, sweetpea_module));
    check("teardown_runs_on_the_bootstrapped_mapping",
          static_cast<const void*>(first),
          static_cast<const void*>(sweetpea::teardown_u_module(state)));
  }

  // No U.dll in the closure: the direct ae_sweetpea start is what is left,
  // and it too is attempted once however many acquires arrive.
  {
    State state;
    check("no_u_dll_starts_directly", Decision::StartDirectly,
          sweetpea::decide(state, nullptr, false, sweetpea_module));
    drive(state, nullptr, false, sweetpea_module, true, true, 10);
    check("direct_start_is_attempted_once", 1UL,
          static_cast<unsigned long>(state.direct_attempts));
    check("direct_start_tears_down_through_sweetpea",
          static_cast<const void*>(nullptr),
          static_cast<const void*>(sweetpea::teardown_u_module(state)));
    // The member that finally maps U.dll still runs U's bootstrap: the
    // direct start left the adapter unregistered, which is the whole point.
    check("later_u_dll_member_still_births", Decision::CallUSpBirth,
          sweetpea::decide(state, first, true, sweetpea_module));
    drive(state, first, true, sweetpea_module, true, true, 5);
    check("late_u_bootstrap_is_attempted_once", 1UL,
          static_cast<unsigned long>(state.u_attempts));
    check("late_u_bootstrap_switches_teardown",
          static_cast<const void*>(first),
          static_cast<const void*>(sweetpea::teardown_u_module(state)));
  }

  // A U.dll mapping without the export never blocks the direct start.
  {
    State state;
    check("u_dll_without_the_export_starts_directly", Decision::StartDirectly,
          sweetpea::decide(state, first, false, sweetpea_module));
  }

  // A failed U_SP_Birth is recorded against its mapping and not retried on
  // it, but the direct start still happens and a later mapping is asked.
  {
    State state;
    drive(state, first, true, sweetpea_module, false, true, 10);
    check("failed_u_bootstrap_is_attempted_once", 1UL,
          static_cast<unsigned long>(state.u_attempts));
    check("failed_u_bootstrap_falls_back_to_one_direct_start", 1UL,
          static_cast<unsigned long>(state.direct_attempts));
    check("failed_mapping_is_not_retried", Decision::Nothing,
          sweetpea::decide(state, first, true, sweetpea_module));
    check("a_different_mapping_is_still_tried", Decision::CallUSpBirth,
          sweetpea::decide(state, second, true, sweetpea_module));
    check("failed_u_bootstrap_does_not_claim_teardown",
          static_cast<const void*>(nullptr),
          static_cast<const void*>(sweetpea::teardown_u_module(state)));
  }

  // A failing direct start is not retried on the same ae_sweetpea mapping:
  // the decision is asked on every suite acquire, so retrying there would
  // re-run SPInit/SPStartupPlugins per acquire. A member that maps
  // ae_sweetpea for the first time is a different mapping and is asked.
  {
    State state;
    drive(state, nullptr, false, nullptr, true, false, 10);
    check("failed_direct_start_is_attempted_once_per_mapping", 1UL,
          static_cast<unsigned long>(state.direct_attempts));
    check("failed_direct_start_is_not_retried_on_the_same_mapping",
          Decision::Nothing,
          sweetpea::decide(state, nullptr, false, nullptr));
    check("a_newly_mapped_sweetpea_is_asked", Decision::StartDirectly,
          sweetpea::decide(state, nullptr, false, sweetpea_module));
    drive(state, nullptr, false, sweetpea_module, true, false, 4);
    check("the_new_mapping_is_also_attempted_once", 2UL,
          static_cast<unsigned long>(state.direct_attempts));
  }

  // Re-entrancy: U_SP_Birth ends in SPStartupPlugins, which can acquire a
  // suite and re-enter the decision before the call returns. Because the
  // attempt is recorded first, that inner ask must not start a second
  // bootstrap.
  {
    State state;
    check("reentrant_ask_before_the_call", Decision::CallUSpBirth,
          sweetpea::decide(state, first, true, sweetpea_module));
    sweetpea::begin_u_sp_birth(state, first);
    // Not StartDirectly: U's own SPInit is still in flight inside that call.
    check("reentrant_ask_during_the_call", Decision::Nothing,
          sweetpea::decide(state, first, true, sweetpea_module));
    sweetpea::finish_u_sp_birth(state, first, true);
    check("reentrancy_leaves_one_u_attempt", 1UL,
          static_cast<unsigned long>(state.u_attempts));
    check("reentrancy_starts_no_direct_bootstrap", 0UL,
          static_cast<unsigned long>(state.direct_attempts));
  }

  // Same for the direct start: a suite acquire from inside SPStartupPlugins
  // must not be answered with "start it again".
  {
    State state;
    sweetpea::begin_direct_start(state, sweetpea_module);
    check("reentrant_ask_during_the_direct_call", Decision::Nothing,
          sweetpea::decide(state, nullptr, false, sweetpea_module));
    // Not even U's bootstrap, which would run SPInit underneath the one that
    // has not returned yet.
    check("reentrant_ask_during_the_direct_call_with_u", Decision::Nothing,
          sweetpea::decide(state, first, true, sweetpea_module));
    sweetpea::finish_direct_start(state, true);
    check("u_bootstrap_resumes_after_the_direct_call", Decision::CallUSpBirth,
          sweetpea::decide(state, first, true, sweetpea_module));
    check("direct_reentrancy_leaves_one_attempt", 1UL,
          static_cast<unsigned long>(state.direct_attempts));
  }

  std::string joined;
  for (const auto& message : failures.messages) {
    if (!joined.empty()) joined += ",";
    joined += "\"" + message + "\"";
  }
  std::printf(
      "{\"worker_sweetpea_bootstrap_selftest\":\"%s\",\"checks\":%zu,"
      "\"failures\":[%s]}\n",
      failures.messages.empty() ? "passed" : "failed", checks,
      joined.c_str());
  return failures.messages.empty() ? 0 : 1;
}
