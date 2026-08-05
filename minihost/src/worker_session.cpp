#include "worker_session.hpp"

#include "runtime_module_audit.hpp"
#include "trace_writer.hpp"
#include "worker_aegp_compute_cache.hpp"

#include <iostream>
#include <utility>

namespace aexcompat::worker_runtime {

WorkerSession::WorkerSession(RuntimeContext& context, TraceWriter* trace_writer,
                             TraceWriter** active_trace_writer) noexcept
    : plugin_path_(std::move(context.plugin_path)),
      module_(context.module),
      sealed_directory_cookie_(context.sealed_directory_cookie),
      search_directory_cookies_(std::move(context.search_directory_cookies)),
      restore_native_stdout_(context.restore_native_stdout),
      stdout_redirected_(context.stdout_redirected),
      trace_writer_(trace_writer),
      active_trace_writer_(active_trace_writer) {
  context = {};
  if (trace_writer_ && trace_writer_->enabled()) {
    if (active_trace_writer_) *active_trace_writer_ = trace_writer_;
    trace_writer_->session_start();
    trace_started_ = true;
  }
}

WorkerSession::~WorkerSession() {
  (void)finish(0);
}

bool WorkerSession::set_pre_unload_hook(PreUnloadHook hook,
                                        void* context) noexcept {
  if (!hook || pre_unload_hook_registered_ || pre_unload_hook_invoked_ ||
      terminal_audit_captured_)
    return false;
  pre_unload_hook_ = hook;
  pre_unload_context_ = context;
  pre_unload_hook_registered_ = true;
  return true;
}

bool WorkerSession::set_swap_quiesce_hook(PreUnloadHook hook,
                                          void* context) noexcept {
  if (!hook || swap_quiesce_hook_ || terminal_audit_captured_) return false;
  swap_quiesce_hook_ = hook;
  swap_quiesce_context_ = context;
  return true;
}

bool WorkerSession::quiesce_once() noexcept {
  if (pre_unload_hook_invoked_) {
    pre_unload_hook_passed_ =
        pre_unload_hook_passed_ && compute_cache::unload_safe();
    if (!pre_unload_hook_passed_) terminal_audit_passed_ = false;
    return pre_unload_hook_passed_;
  }
  pre_unload_hook_invoked_ = true;
  const PreUnloadHook hook = pre_unload_hook_;
  void* const context = pre_unload_context_;
  pre_unload_hook_ = nullptr;
  pre_unload_context_ = nullptr;
  pre_unload_hook_passed_ =
      (!hook || hook(context)) && compute_cache::unload_safe();
  if (!pre_unload_hook_passed_) terminal_audit_passed_ = false;
  return pre_unload_hook_passed_;
}

bool WorkerSession::capture_terminal_audit() noexcept {
  // Quiesce before observing the terminal module/lease state. This ordering is
  // also used by every finish path because they all capture the audit first.
  const bool quiesced = quiesce_once();
  if (terminal_audit_captured_) return terminal_audit_passed_;
  terminal_audit_captured_ = true;
  terminal_audit_passed_ = terminal_audit_passed_ && quiesced;
  ModuleAuditReport& audit = module_audit_report();
  if (audit.required && module_) {
    audit.pre_unload = capture_module_audit();
    terminal_audit_passed_ = terminal_audit_passed_ &&
                             audit.pre_unload.status == "passed" &&
                             module_audit_passed();
  } else if (audit.recorded && module_) {
    // Recorded, never enforced (issue #751): the terminal snapshot reaches
    // the report, and its status never turns this lifecycle's verdict.
    audit.pre_unload = capture_module_audit();
  }
  return terminal_audit_passed_;
}

void WorkerSession::stop_trace() noexcept {
  if (!trace_started_) return;
  trace_writer_->session_end();
  if (active_trace_writer_ && *active_trace_writer_ == trace_writer_)
    *active_trace_writer_ = nullptr;
  trace_started_ = false;
}

void WorkerSession::unload_module() noexcept {
  // GLOBAL_SETDOWN may discover live Compute Cache work after the terminal
  // hook was registered. Re-check its process-sticky gate at the actual unload
  // boundary so neither FreeLibrary nor deferred-release cleanup can discard
  // a module whose callbacks remain reachable.
  if (!compute_cache::unload_safe()) {
    terminal_audit_passed_ = false;
    return;
  }
  if (deferred_module_release_) {
    // Deferred release (issue #474): nothing is freed mid-process. The
    // current and retired plug-in images and the directory cookies all
    // unload in one loader-ordered pass at process exit.
    if (module_) retired_modules_.push_back(module_);
    module_ = nullptr;
    retired_modules_.clear();
    sealed_directory_cookie_ = nullptr;
    search_directory_cookies_.clear();
    return;
  }
  if (!module_) {
    release_directory_cookies();
    return;
  }
  // Defensive coverage for future unload paths that do not capture an audit.
  // Never unload code while its owner reports that callbacks may still run.
  if (!quiesce_once()) return;
  FreeLibrary(module_);
  module_ = nullptr;
  release_directory_cookies();
}

void WorkerSession::release_directory_cookies() noexcept {
  for (DLL_DIRECTORY_COOKIE cookie : search_directory_cookies_)
    if (cookie) RemoveDllDirectory(cookie);
  search_directory_cookies_.clear();
  if (sealed_directory_cookie_) {
    RemoveDllDirectory(sealed_directory_cookie_);
    sealed_directory_cookie_ = nullptr;
  }
}

void WorkerSession::restore_stdout() noexcept {
  if (!stdout_redirected_) return;
  if (restore_native_stdout_) restore_native_stdout_();
  stdout_redirected_ = false;
}

void WorkerSession::emit_audit_failure() noexcept {
  if (audit_failure_reported_) return;
  audit_failure_reported_ = true;
  std::cout << "{\"schema_version\":1,\"stage\":\"module_audit\","
               "\"status\":\"module_audit_failed\",\"module_audit\":"
            << module_audit_json() << "}\n";
}

bool WorkerSession::prepare_protocol_report() {
  const bool passed = capture_terminal_audit();
  restore_stdout();
  if (!passed) emit_audit_failure();
  return passed;
}

bool WorkerSession::shutdown_before_report() {
  const bool passed = capture_terminal_audit();
  stop_trace();
  unload_module();
  restore_stdout();
  return passed;
}

int WorkerSession::finish_integrated_report(int exit_code) noexcept {
  (void)capture_terminal_audit();
  stop_trace();
  unload_module();
  restore_stdout();
  // Suppress destructor fallback: this path has already serialized the audit
  // inside its own compatibility report, including rejection details.
  audit_failure_reported_ = true;
  return exit_code;
}

int WorkerSession::finish(int exit_code) noexcept {
  const bool passed = capture_terminal_audit();
  stop_trace();
  unload_module();
  restore_stdout();
  if (!passed) {
    emit_audit_failure();
    return 14;
  }
  return exit_code;
}

bool WorkerSession::swap_release_module(uint32_t outgoing_index) noexcept {
  if (!module_ || swap_pending_ || terminal_audit_captured_) return false;
  // Per-swap quiescence only: the session-global pre-unload hook (BIB
  // teardown, #395) is NOT run or consumed here — it fires exactly once at
  // the terminal lifecycle. An unset swap hook quiesces trivially.
  if (swap_quiesce_hook_ && !swap_quiesce_hook_(swap_quiesce_context_))
    return false;
  ModuleAuditReport& audit = module_audit_report();
  ModuleAuditSnapshot pre_unload;
  if (audit.required) {
    pre_unload = capture_module_audit();
    if (pre_unload.status != "passed" || !module_audit_passed()) return false;
  }
  // Deferred release (issue #474): the outgoing plug-in image is retired,
  // not freed — its logical teardown already happened (GLOBAL_SETDOWN), and
  // the image stays mapped until the loader-ordered unload at process exit.
  // Freeing it here would let closure atexit handlers later dereference the
  // unmapped image.
  retired_modules_.push_back(module_);
  module_ = nullptr;
  swap_pending_ = true;
  swap_outgoing_index_ = outgoing_index;
  swap_pre_unload_ = std::move(pre_unload);
  return true;
}

void WorkerSession::swap_abandon() noexcept {
  if (!swap_pending_) return;
  // The pending epoch closes on the state after the failed load (nothing
  // new mapped); capture/record are no-ops unless the audit is observing.
  ModuleAuditSnapshot post_failure = capture_module_audit();
  record_module_audit_epoch(swap_outgoing_index_, std::move(swap_pre_unload_),
                            std::move(post_failure));
  swap_pre_unload_ = {};
  swap_pending_ = false;
}

bool WorkerSession::swap_adopt_module(HMODULE module,
                                      const std::filesystem::path& plugin_path,
                                      uint32_t incoming_index) noexcept {
  (void)incoming_index;  // the epoch records the outgoing index by design
  if (!swap_pending_ || !module) return false;
  ModuleAuditReport& audit = module_audit_report();
  audit.plugin_path = plugin_path;
  if (audit.required) {
    // Capture and judge before taking ownership: a rejected plug-in is
    // deferred to process exit like every other image (issue #474) and never
    // enters the session's ownership state.
    ModuleAuditSnapshot post_load = capture_module_audit();
    record_module_audit_epoch(swap_outgoing_index_, std::move(swap_pre_unload_),
                              post_load);
    if (post_load.status != "passed" || !module_audit_passed()) {
      retired_modules_.push_back(module);
      return false;
    }
  }
  swap_pending_ = false;
  module_ = module;
  plugin_path_ = plugin_path;
  return true;
}

}  // namespace aexcompat::worker_runtime
