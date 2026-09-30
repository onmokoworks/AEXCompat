#include "worker_dvacore_main_thread.hpp"

#include <cstdio>

using aexcompat::worker_runtime::dvacore_main_thread::Action;
using aexcompat::worker_runtime::dvacore_main_thread::decide;

int main() {
  const bool unregistered = decide(0, 41) == Action::Register;
  const bool already_main = decide(41, 41) == Action::AlreadyMain;
  const bool other_main = decide(42, 41) == Action::OtherMain;
  const bool invalid_dispatch = decide(0, 0) == Action::InvalidThread;
  const bool zero_not_main = decide(41, 0) == Action::InvalidThread;
  const bool passed = unregistered && already_main && other_main &&
                      invalid_dispatch && zero_not_main;
  std::printf(
      "{\"worker_dvacore_main_thread_selftest\":\"%s\","
      "\"checks\":5}\n",
      passed ? "passed" : "failed");
  return passed ? 0 : 1;
}
