#include "l2_mode_execution.hpp"

#include <cassert>
#include <cstdint>
#include <string>

namespace {
struct State {
  int setdowns{};
  int defaults_disposed{};
  int reports{};
  int32_t reported_setdown_error{};
  std::string status;
};

bool dispose_defaults(void* opaque) {
  ++static_cast<State*>(opaque)->defaults_disposed;
  return true;
}
int32_t setdown(void* opaque) {
  ++static_cast<State*>(opaque)->setdowns;
  return 0;
}
bool prepare(void*) { return true; }
void report(void* opaque, const char* status, int32_t, int32_t,
            int32_t setdown_error) {
  auto& state = *static_cast<State*>(opaque);
  ++state.reports;
  state.status = status;
  state.reported_setdown_error = setdown_error;
}

aexcompat::l2mode::Request request(State& state,
                                   aexcompat::l2mode::EarlyMode mode) {
  aexcompat::l2mode::Hooks hooks{};
  hooks.global_setdown = &setdown;
  hooks.prepare_protocol_report = &prepare;
  hooks.dispose_arbitrary_defaults = &dispose_defaults;
  hooks.report_parameters = &report;
  return {mode, &state, hooks, 0, 0, true, nullptr};
}
}  // namespace

int main() {
  wchar_t program[] = L"worker";
  wchar_t command[] = L"--l2-params-inspect-cleanup-contained-v1";
  wchar_t plugin[] = L"fixture.aex";
  wchar_t sha[] = L"0000000000000000000000000000000000000000000000000000000000000000";
  wchar_t second_request[] = L"second-request";
  wchar_t* accepted_argv[] = {program, command, plugin, sha};
  wchar_t* reused_argv[] = {program, command, plugin, sha, second_request};
  assert(aexcompat::l2mode::cleanup_contained_params_only_command(
      4, accepted_argv));
  assert(!aexcompat::l2mode::cleanup_contained_params_only_command(
      5, reused_argv));

  State ordinary;
  assert(aexcompat::l2mode::run_early_mode(request(
             ordinary, aexcompat::l2mode::EarlyMode::ParametersOnly)) == 0);
  assert(ordinary.defaults_disposed == 1);
  assert(ordinary.setdowns == 1);
  assert(ordinary.reports == 1);
  assert(ordinary.status == "parameters_inspected");
  assert(ordinary.reported_setdown_error == 0);

  State contained;
  assert(aexcompat::l2mode::run_early_mode(request(
             contained,
             aexcompat::l2mode::EarlyMode::CleanupContainedParametersOnly)) ==
         0);
  assert(contained.defaults_disposed == 1);
  assert(contained.setdowns == 0);
  assert(contained.reports == 1);
  assert(contained.status == "parameters_inspected_cleanup_contained");
  assert(contained.reported_setdown_error == -1);

  assert(aexcompat::l2mode::select_early_mode(false, false, false, false,
                                              true) ==
         aexcompat::l2mode::EarlyMode::CleanupContainedParametersOnly);
  return 0;
}
