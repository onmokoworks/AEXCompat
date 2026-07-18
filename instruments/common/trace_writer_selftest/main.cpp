#include "trace_writer.hpp"

#include <iostream>
#include <thread>
#include <vector>

int main() {
  aexcompat::TraceWriter writer("minihost", "trace-writer-selftest", "synthetic-null");
  if (writer.requested() && !writer.enabled()) return 16;
  if (!writer.enabled()) return 2;
  writer.session_start();
  writer.selector_dispatch(std::string(300, 'x'));
  writer.suite_acquire("C:\\private\\plugin-bytes", 1, true);
  writer.suite_acquire("prefix \\\\server\\share\\private", 1, true);
  writer.suite_acquire("prefix /home/private/plugin", 1, true);
  writer.suite_acquire(std::string("bad-utf8-\xc0\xaf", 11), 1, true);
  writer.suite_acquire("bad\ncontrol", 1, true);
  std::vector<std::thread> threads;
  for (int thread = 0; thread < 8; ++thread) {
    threads.emplace_back([&writer]() {
      for (int event = 0; event < 64; ++event) writer.callback_invoke();
    });
  }
  for (auto& thread : threads) thread.join();
  writer.selector_dispatch("PF_Cmd_GLOBAL_SETUP");
  writer.suite_acquire("Synthetic Suite", 1, true);
  writer.world_descriptor(16, 12, 64, "argb8");
  writer.callback_invoke();
  writer.suite_release("Synthetic Suite", 1, true);
  writer.session_end();
  return 0;
}
