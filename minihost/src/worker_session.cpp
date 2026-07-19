#include "worker_session.hpp"

#include "runtime_module_audit.hpp"
#include "trace_writer.hpp"

#include <iostream>
#include <utility>

namespace aexcompat::worker_runtime {

WorkerSession::WorkerSession(RuntimeContext& context, TraceWriter* trace_writer,
                             TraceWriter** active_trace_writer) noexcept
    : plugin_path_(std::move(context.plugin_path)),
      module_(context.module),
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

bool WorkerSession::quiesce_once() noexcept {
  if (pre_unload_hook_invoked_) return pre_unload_hook_passed_;
  pre_unload_hook_invoked_ = true;
  const PreUnloadHook hook = pre_unload_hook_;
  void* const context = pre_unload_context_;
  pre_unload_hook_ = nullptr;
  pre_unload_context_ = nullptr;
  pre_unload_hook_passed_ = !hook || hook(context);
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
  if (!module_) return;
  // Defensive coverage for future unload paths that do not capture an audit.
  // Never unload code while its owner reports that callbacks may still run.
  if (!quiesce_once()) return;
  FreeLibrary(module_);
  module_ = nullptr;
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

}  // namespace aexcompat::worker_runtime
