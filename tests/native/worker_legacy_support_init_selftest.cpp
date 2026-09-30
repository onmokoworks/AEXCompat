// Self-test for the U.dll legacy-support latch (issue #1063): the decision
// that "U.dll is not mapped yet" never latches, so a session whose first
// plug-in has no U.dll still births the allocator when a later plug-in maps
// it, while one mapping is birthed once and a different mapping is birthed
// again. This covers the pure decision only; the ten-line glue in
// l2_main_support.inc that feeds it GetModuleHandleW/GetProcAddress and calls
// U_Birth is exercised by the real worker (a U.dll plug-in following a
// U.dll-free one in a discovery session).
#include "worker_legacy_support_init.hpp"

#include <cstdio>
#include <string>
#include <vector>

namespace {

using aexcompat::worker_runtime::legacy_support::Decision;
using aexcompat::worker_runtime::legacy_support::State;
using aexcompat::worker_runtime::legacy_support::decide;
using aexcompat::worker_runtime::legacy_support::note_called;

const char* name(Decision decision) {
  switch (decision) {
    case Decision::Absent: return "absent";
    case Decision::AlreadyInitialized: return "already_initialized";
    case Decision::NoBirthExport: return "no_birth_export";
    case Decision::Call: return "call";
  }
  return "?";
}

struct Check {
  const char* label;
  Decision expected;
  Decision actual;
};

}  // namespace

int main() {
  // Fake mappings: only identity matters to the latch.
  alignas(16) static unsigned char first_mapping[16]{};
  alignas(16) static unsigned char second_mapping[16]{};
  const HMODULE first = reinterpret_cast<HMODULE>(first_mapping);
  const HMODULE second = reinterpret_cast<HMODULE>(second_mapping);

  std::vector<Check> checks;
  State state;

  // Session member 1 (no U.dll in its closure): absent, and nothing latched.
  checks.push_back({"first_member_without_u_dll", Decision::Absent,
                    decide(state, nullptr, false)});
  // Session member 2 maps U.dll: the latch must still say Call.
  checks.push_back({"later_member_maps_u_dll", Decision::Call,
                    decide(state, first, true)});
  note_called(state, first);
  // Member 3 on the same mapping: birthed once.
  checks.push_back({"same_mapping_is_birthed_once", Decision::AlreadyInitialized,
                    decide(state, first, true)});
  // A member without U.dll after the birth: absent, still nothing to do,
  // and the birthed mapping stays remembered.
  checks.push_back({"absent_after_birth", Decision::Absent,
                    decide(state, nullptr, false)});
  checks.push_back({"birth_survives_an_absent_member", Decision::AlreadyInitialized,
                    decide(state, first, true)});
  // U.dll unloaded and mapped again: a fresh allocator, birthed again.
  checks.push_back({"remapped_u_dll_is_birthed_again", Decision::Call,
                    decide(state, second, true)});
  note_called(state, second);
  checks.push_back({"remapping_is_then_birthed_once", Decision::AlreadyInitialized,
                    decide(state, second, true)});
  // A mapping without the export is reported, not called, and never counts
  // as birthed: the same state still says Call once the export is there.
  State no_export;
  checks.push_back({"no_export_is_not_called", Decision::NoBirthExport,
                    decide(no_export, first, false)});
  checks.push_back({"no_export_state_still_calls_when_exported", Decision::Call,
                    decide(no_export, first, true)});
  // Absent is never confused with a birthed mapping: a null module after a
  // birth is still Absent, not AlreadyInitialized.
  State birthed_null;
  note_called(birthed_null, first);
  checks.push_back({"absent_is_not_the_birthed_mapping", Decision::Absent,
                    decide(birthed_null, nullptr, true)});

  bool passed = true;
  std::string failures;
  for (const auto& check : checks) {
    if (check.expected == check.actual) continue;
    passed = false;
    if (!failures.empty()) failures += ",";
    failures += std::string("\"") + check.label + ":expected=" +
                name(check.expected) + ":actual=" + name(check.actual) + "\"";
  }
  std::printf(
      "{\"worker_legacy_support_init_selftest\":\"%s\",\"checks\":%zu,"
      "\"failures\":[%s]}\n",
      passed ? "passed" : "failed", checks.size(), failures.c_str());
  return passed ? 0 : 1;
}
