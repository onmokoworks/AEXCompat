#pragma once

#include "runtime_module_audit.hpp"
#include "worker_runtime_admission.hpp"

#include <filesystem>
#include <vector>

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
  // accepted before terminal audit/quiescence begins. The hook is
  // session-global (today the BIB teardown, #395): it runs exactly once, at
  // the terminal lifecycle, and is never consumed by a cluster swap.
  bool set_pre_unload_hook(PreUnloadHook hook, void* context) noexcept;

  // Registers the per-swap quiescence barrier (issue #405). Distinct from the
  // session-global pre-unload hook: swap_release_module runs this before
  // freeing a swapped plug-in, without touching the terminal hook's latch, so
  // a session-global owner (BIB teardown) survives every swap and still runs
  // once at the end. Optional; an unset swap hook quiesces trivially.
  bool set_swap_quiesce_hook(PreUnloadHook hook, void* context) noexcept;

  // Cluster-session plug-in swap (docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md
  // §4.1, amended by issue #474's deferred release): swap_release_module runs
  // the per-swap quiescence hook (never the session-global pre-unload hook),
  // captures the epoch's pre_unload (post-logical-teardown) snapshot, and
  // retires the outgoing plug-in WITHOUT freeing it; swap_adopt_module
  // captures post_load, records the epoch
  // {outgoing_index, pre_unload, post_load}, and takes ownership of the next
  // plug-in only on success. A false return is a swap failure (quiescence,
  // audit, or load): the session cannot safely continue and the caller must
  // terminate with the dedicated swap-failure exit code.
  bool swap_release_module(uint32_t outgoing_index) noexcept;
  bool swap_adopt_module(HMODULE module, const std::filesystem::path& plugin_path,
                         uint32_t incoming_index) noexcept;

  // Deferred module release (issue #474): cluster sessions set this so the
  // terminal lifecycle never frees plug-in images or the sealed-directory
  // cookie mid-process. Everything unloads in one loader-ordered pass at
  // process exit, keeping the closure's CRT atexit handlers (e.g. the dvacore
  // notification registry and BIB) from dereferencing an already-unmapped
  // plug-in image.
  void set_deferred_module_release() noexcept {
    deferred_module_release_ = true;
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

  // Terminal boundary for isolated video render sessions whose complete
  // protocol report has already been prepared and synchronously emitted.
  // The process is deliberately ended without DLL/CRT detach so a plug-in's
  // post-GLOBAL_SETDOWN unload cannot stall an otherwise clean close.
  [[noreturn]] void terminate_after_protocol_report(int exit_code) noexcept;

 private:
  bool quiesce_once() noexcept;
  bool capture_terminal_audit() noexcept;
  void stop_trace() noexcept;
  void unload_module() noexcept;
  void release_directory_cookies() noexcept;
  bool restore_stdout() noexcept;
  void emit_audit_failure() noexcept;

  std::filesystem::path plugin_path_;
  HMODULE module_{};
  DLL_DIRECTORY_COOKIE sealed_directory_cookie_{};
  // In-place dependency search directory cookies (issue #751); released in
  // the same lifecycle order as the sealed cookie.
  std::vector<DLL_DIRECTORY_COOKIE> search_directory_cookies_{};
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
  bool protocol_report_prepared_{};
  // Per-swap quiescence hook (#405): invoked by swap_release_module before a
  // swapped plug-in is freed. Kept separate from the session-global
  // pre-unload hook (BIB teardown) so a swap never consumes the terminal
  // barrier.
  PreUnloadHook swap_quiesce_hook_{};
  void* swap_quiesce_context_{};
  // Cluster swap state: set between swap_release_module and
  // swap_adopt_module; the pending epoch holds the outgoing plug-in's
  // pre_unload snapshot until the incoming plug-in's post_load completes it.
  bool swap_pending_{};
  uint32_t swap_outgoing_index_{};
  ModuleAuditSnapshot swap_pre_unload_{};
  // Deferred release (issue #474): when set, swapped-out plug-in images move
  // here instead of being freed, and the terminal lifecycle unloads nothing
  // mid-process.
  bool deferred_module_release_{};
  std::vector<HMODULE> retired_modules_{};
  TraceWriter* trace_writer_{};
  TraceWriter** active_trace_writer_{};
  bool trace_started_{};
};

}  // namespace aexcompat::worker_runtime
