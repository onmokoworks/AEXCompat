#include "trace_writer.hpp"

#include <iostream>

int main() {
  aexcompat::TraceWriter writer("minihost", "trace-writer-selftest", "synthetic-null");
  if (!writer.enabled()) return 2;
  writer.session_start();
  writer.selector_dispatch("PF_Cmd_GLOBAL_SETUP");
  writer.suite_acquire("Synthetic Suite", 1, true);
  writer.world_descriptor(16, 12, 64, "argb8");
  writer.callback_invoke();
  writer.suite_release("Synthetic Suite", 1, true);
  writer.session_end();
  std::cout << writer.path().string() << '\n';
  return 0;
}
