#include "native_stdout_guard.hpp"

#include <fcntl.h>
#include <io.h>

#include <cstdio>
#include <iostream>

namespace aexcompat::worker_runtime {
namespace {

int g_saved_stdout_fd{-1};
int g_native_stdout_sink_fd{-1};

}  // namespace

bool redirect_native_stdout() {
  std::cout.flush();
  std::fflush(stdout);
  g_saved_stdout_fd = _dup(_fileno(stdout));
  g_native_stdout_sink_fd = _open("NUL", _O_WRONLY);
  if (g_saved_stdout_fd < 0 || g_native_stdout_sink_fd < 0 ||
      _dup2(g_native_stdout_sink_fd, _fileno(stdout)) != 0) {
    if (g_saved_stdout_fd >= 0) _close(g_saved_stdout_fd);
    if (g_native_stdout_sink_fd >= 0) _close(g_native_stdout_sink_fd);
    g_saved_stdout_fd = g_native_stdout_sink_fd = -1;
    return false;
  }
  return true;
}

void restore_native_stdout() {
  if (g_saved_stdout_fd < 0) return;
  std::cout.flush();
  std::fflush(stdout);
  _dup2(g_saved_stdout_fd, _fileno(stdout));
  _close(g_saved_stdout_fd);
  if (g_native_stdout_sink_fd >= 0) _close(g_native_stdout_sink_fd);
  g_saved_stdout_fd = g_native_stdout_sink_fd = -1;
}

}  // namespace aexcompat::worker_runtime
