from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost" / "src" / "worker_session.hpp").read_text(
    encoding="utf-8"
)
SOURCE = (ROOT / "minihost" / "src" / "worker_session.cpp").read_text(
    encoding="utf-8"
)
EARLY = (ROOT / "minihost" / "src" / "l2_mode_execution.cpp").read_text(
    encoding="utf-8"
)
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")


def test_session_is_the_single_post_admission_module_owner():
    assert CMAKE.count("src/worker_session.cpp") == 1
    assert '#include "worker_session.hpp"' in MAIN
    assert "WorkerSession session(runtime_context, &trace_writer, &g_trace_writer)" in MAIN
    assert "FreeLibrary(module)" not in MAIN
    assert MAIN.count("session.finish(") >= 10
    assert "FreeLibrary(module_);" in SOURCE


def test_terminal_cleanup_order_is_explicit_and_idempotent():
    finish = SOURCE[SOURCE.index("int WorkerSession::finish"):]
    audit = finish.index("capture_terminal_audit()")
    trace = finish.index("stop_trace()")
    unload = finish.index("unload_module()")
    stdout = finish.index("restore_stdout()")
    assert audit < trace < unload < stdout
    assert "if (terminal_audit_captured_)" in SOURCE
    assert "if (!trace_started_) return" in SOURCE
    assert "if (!module_) return" in SOURCE
    assert "if (!stdout_redirected_) return" in SOURCE


def test_audit_and_trace_remain_fail_closed_across_early_returns():
    capture = SOURCE[SOURCE.index("bool WorkerSession::capture_terminal_audit"):
                     SOURCE.index("void WorkerSession::stop_trace")]
    assert "audit.pre_unload = capture_module_audit()" in capture
    assert 'audit.pre_unload.status == "passed"' in capture
    assert "module_audit_passed()" in capture
    assert "return 14" in SOURCE[SOURCE.index("int WorkerSession::finish"):]
    assert EARLY.count("prepare_protocol_report(r.context)") == 4


def test_post_unload_report_path_stops_trace_before_unload_and_stdout_restore():
    shutdown = SOURCE[SOURCE.index("bool WorkerSession::shutdown_before_report"):
                      SOURCE.index("int WorkerSession::finish")]
    assert shutdown.index("capture_terminal_audit()") < shutdown.index("stop_trace()")
    assert shutdown.index("stop_trace()") < shutdown.index("unload_module()")
    assert shutdown.index("unload_module()") < shutdown.index("restore_stdout()")
    death = MAIN.index("registration.hook(global_refcon, registration.refcon)")
    assert death < MAIN.index("session.shutdown_before_report()", death)
    assert "return session.finish_integrated_report(passed ? 0 : 23)" in MAIN
    integrated = SOURCE[SOURCE.index("int WorkerSession::finish_integrated_report"):
                        SOURCE.index("int WorkerSession::finish(")]
    assert "emit_audit_failure()" not in integrated
    assert "return exit_code" in integrated
