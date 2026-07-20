#pragma once

#include <windows.h>

#include <cstdint>
#include <string>

namespace aexcompat::worker_runtime::minidump {

// Opt-in crash minidumps (issue #18) use a broker-created inherited pipe: the
// worker never receives a directory path or a dump-file handle. Reads
// AEXCOMPAT_MINIDUMP_HANDLE / AEXCOMPAT_MINIDUMP_ACK_HANDLE, preloads DbgHelp,
// starts the dedicated writer thread, and installs the top-level crash filter.
// Returns true when opt-in is off (no handle env) as well as on success;
// returns false only on a real misconfiguration (invalid handle, writer setup
// failure) so the worker can fail closed. The handle env vars are cleared so no
// child or re-resolution can reach them.
bool configure_from_inherited_handle();

// One-shot guard state and the broker's acknowledged capture size, exposed for
// the crash self-tests. `handle_configured` reports whether a usable inherited
// handle was accepted (i.e. opt-in was actually on).
bool attempted();
uint64_t written_bytes();
bool broker_rejected();
bool handle_configured();

struct SehDiagnosticsSink {
  uint32_t& code;
  uint64_t& address;
  std::string& module;
};

void classify_seh_exception(EXCEPTION_POINTERS* information,
                            SehDiagnosticsSink diagnostics);
int capture_seh_exception(EXCEPTION_POINTERS* information,
                          SehDiagnosticsSink diagnostics);
// Hands the exception to the dedicated writer thread (DbgHelp must not run on
// the faulting thread of an unstable process) and waits, bounded, for it.
void request_crash_minidump(EXCEPTION_POINTERS* information);
LONG WINAPI top_level_crash_filter(EXCEPTION_POINTERS* information);

}  // namespace aexcompat::worker_runtime::minidump
