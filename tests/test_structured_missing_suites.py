from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]




def test_broker_uses_structured_report_not_stderr_for_missing_suites():
    source = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
    # The slice used to end at fn diagnostics_contains_gpu_stage, which existed
    # only to infer a GPU failure for the one-shot CPU retry and went with the
    # one-shot transport (#365). fn failed_module_audit_summary is the next
    # top-level item after worker_diagnostics now.
    diagnostics = source[
        source.index("fn worker_diagnostics(") : source.index("fn failed_module_audit_summary(")
    ]
    assert "missing_suite_event" not in diagnostics
    assert 'worker_report.get("missing_suites")' in diagnostics
    assert "propagate_missing_suites(&mut diagnostics, report)" in source
    inspection = source[
        source.index("fn inspect_experimental_with_diagnostics_and_runtime_policy(") :
        source.index("pub fn probe_experimental_custom_ui_cursor(")
    ]
    assert "let worker_report: Option<Value>" in inspection
    assert inspection.index("propagate_missing_suites(&mut diagnostics, report)") < inspection.index(
        'if isolated.classification.as_str() != "ok"'
    )


