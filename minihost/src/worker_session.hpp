#pragma once

#include "worker_runtime_admission.hpp"

#include <filesystem>

namespace aexcompat {
class TraceWriter;
}

namespace aexcompat::worker_runtime {

// Owns every process-wide resource acquired after runtime admission. Cleanup is
// deliberately centralized so every early return observes the same ordering:
// terminal module audit, trace end, module unload, then native stdout restore.
class WorkerSession final {
 public:
  WorkerSession(RuntimeContext& context, TraceWriter* trace_writer,
                TraceWriter** active_trace_writer) noexcept;
  ~WorkerSession();

  WorkerSession(const WorkerSession&) = delete;
  WorkerSession& operator=(const WorkerSession&) = delete;

  HMODULE module() const noexcept { return module_; }
  const std::filesystem::path& plugin_path() const noexcept {
    return plugin_path_;
  }

  // Captures the terminal loaded-module set before protocol output and restores
  // stdout. On audit rejection it emits the canonical fail-closed report.
  bool prepare_protocol_report();

  // Used where post-unload ownership state is part of the report (AEGP paths).
  // This performs the complete lifecycle before returning to the reporter.
  bool shutdown_before_report();

  // Completes a path whose protocol report already embeds module-audit state.
  // Unlike finish(), this preserves that path's historical exit-code contract
  // and does not append the standalone module-audit failure record.
  int finish_integrated_report(int exit_code) noexcept;

  // Completes an early-return path. A late audit rejection overrides the
  // caller's result with the historical module-audit exit code (14).
  int finish(int exit_code) noexcept;

 private:
  bool capture_terminal_audit() noexcept;
  void stop_trace() noexcept;
  void unload_module() noexcept;
  void restore_stdout() noexcept;
  void emit_audit_failure() noexcept;

  std::filesystem::path plugin_path_;
  HMODULE module_{};
  RuntimeStdoutRestore restore_native_stdout_{};
  bool stdout_redirected_{};
  bool terminal_audit_captured_{};
  bool terminal_audit_passed_{true};
  bool audit_failure_reported_{};
  TraceWriter* trace_writer_{};
  TraceWriter** active_trace_writer_{};
  bool trace_started_{};
};

}  // namespace aexcompat::worker_runtime
