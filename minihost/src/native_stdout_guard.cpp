#include "native_stdout_guard.hpp"

#include "worker_extended_diag.hpp"

#include <fcntl.h>
#include <io.h>

#include <algorithm>
#include <climits>
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
  // The sink is NUL by default: the plug-in shares this process's stdout
  // with the worker's final report, and a plug-in that writes there would
  // corrupt the JSON the broker parses. Under AEXCOMPAT_EXTENDED_DIAG it
  // becomes stderr instead, which carries no protocol and is where the
  // worker's own stage/diag lines already go. Plug-ins do write diagnostics
  // there - DeepGlow2 has parameters that dump its PreRender rects and
  // buffer dimensions to std::cout - and discarding them left the host
  // guessing at what the plug-in was telling it (issue #905).
  g_native_stdout_sink_fd = aexcompat::l2_detail::extended_diag_enabled()
      ? _dup(_fileno(stderr))
      : _open("NUL", _O_WRONLY);
  if (g_saved_stdout_fd < 0 || g_native_stdout_sink_fd < 0 ||
      _dup2(g_native_stdout_sink_fd, _fileno(stdout)) != 0) {
    if (g_saved_stdout_fd >= 0) _close(g_saved_stdout_fd);
    if (g_native_stdout_sink_fd >= 0) _close(g_native_stdout_sink_fd);
    g_saved_stdout_fd = g_native_stdout_sink_fd = -1;
    return false;
  }
  return true;
}

bool restore_native_stdout() {
  if (g_saved_stdout_fd < 0) return true;
  // Flush only the C stream while its fd still names the sink. A plug-in can
  // leave printf output pending, and restoring first would release that text
  // into the JSON protocol. Do not touch the shared C++ stream here: legacy
  // plug-ins can damage its global state during render.
  const bool flushed = std::fflush(stdout) == 0;
  const bool restored = _dup2(g_saved_stdout_fd, _fileno(stdout)) == 0;
  _close(g_saved_stdout_fd);
  if (g_native_stdout_sink_fd >= 0) _close(g_native_stdout_sink_fd);
  g_saved_stdout_fd = g_native_stdout_sink_fd = -1;
  return flushed && restored;
}

bool emit_protocol_stdout(std::string_view report) {
  const int fd = _fileno(stdout);
  std::size_t written = 0;
  while (written < report.size()) {
    const auto remaining = report.size() - written;
    const unsigned int chunk = static_cast<unsigned int>(
        std::min<std::size_t>(remaining, static_cast<std::size_t>(INT_MAX)));
    const int result = _write(fd, report.data() + written, chunk);
    if (result <= 0) return false;
    written += static_cast<std::size_t>(result);
  }
  return true;
}

bool selftest_native_stdout_routing() {
  if (!redirect_native_stdout()) return false;
  std::printf("native-stdout-marker%c", 10);
  // Deliberately leave the marker buffered. restore_native_stdout must flush
  // it to the sink before the protocol fd is restored.
  return restore_native_stdout();
}

}  // namespace aexcompat::worker_runtime
