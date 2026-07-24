#pragma once

#include "runtime_module_audit.hpp"
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
  // Returns true only after all plug-in callbacks and worker threads that may
  // execute plug-in code have quiesced. A false result prevents module unload
  // and makes the terminal lifecycle fail closed. The noexcept function type
  // also makes an escaping C++ exception terminate rather than cross the host
  // lifecycle boundary.
  using PreUnloadHook = bool(__cdecl*)(void*) noexcept;

  WorkerSession(RuntimeContext& context, TraceWriter* trace_writer,
                TraceWriter** active_trace_writer) noexcept;
  ~WorkerSession();

  WorkerSession(const WorkerSession&) = delete;
  WorkerSession& operator=(const WorkerSession&) = delete;

  HMODULE module() const noexcept { return module_; }
  const std::filesystem::path& plugin_path() const noexcept {
    return plugin_path_;
  }

  // Registers the session's single pre-unload barrier. Registration is only
  // accepted before terminal audit/quiescence begins.
  bool set_pre_unload_hook(PreUnloadHook hook, void* context) noexcept;

  // Cluster-session plug-in swap (docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md
  // §4.1): releases only the plug-in image while the AddDllDirectory cookie
  // and the pinned closure dependencies stay loaded. swap_release_module runs
  // the quiescence barrier, captures the epoch's pre_unload snapshot, and
  // frees the plug-in HMODULE; swap_adopt_module captures post_load, records
  // the epoch {outgoing_index, pre_unload, post_load}, and takes ownership of
  // the next plug-in only on success (a rejected module is freed inside the
  // session and never adopted). A false return is a swap failure
  // (quiescence, audit, or FreeLibrary): the session cannot safely continue
  // and the caller must terminate with the dedicated swap-failure exit code.
  bool swap_release_module(uint32_t outgoing_index) noexcept;
  bool swap_adopt_module(HMODULE module, const std::filesystem::path& plugin_path,
                         uint32_t incoming_index) noexcept;

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
  bool quiesce_once() noexcept;
  bool capture_terminal_audit() noexcept;
  void stop_trace() noexcept;
  void unload_module() noexcept;
  void restore_stdout() noexcept;
  void emit_audit_failure() noexcept;

  std::filesystem::path plugin_path_;
  HMODULE module_{};
  DLL_DIRECTORY_COOKIE sealed_directory_cookie_{};
  RuntimeStdoutRestore restore_native_stdout_{};
  bool stdout_redirected_{};
  bool terminal_audit_captured_{};
  bool terminal_audit_passed_{true};
  PreUnloadHook pre_unload_hook_{};
  void* pre_unload_context_{};
  bool pre_unload_hook_registered_{};
  bool pre_unload_hook_invoked_{};
  bool pre_unload_hook_passed_{true};
  bool audit_failure_reported_{};
  // Cluster swap state: set between swap_release_module and
  // swap_adopt_module; the pending epoch holds the outgoing plug-in's
  // pre_unload snapshot until the incoming plug-in's post_load completes it.
  bool swap_pending_{};
  uint32_t swap_outgoing_index_{};
  ModuleAuditSnapshot swap_pre_unload_{};
  TraceWriter* trace_writer_{};
  TraceWriter** active_trace_writer_{};
  bool trace_started_{};
};

}  // namespace aexcompat::worker_runtime
