from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_production_worker_reports_bounded_missing_suites():
    source = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
    registry = (ROOT / "minihost" / "src" / "worker_suite_registry.cpp").read_text(
        encoding="utf-8"
    )
    assert "constexpr std::size_t kMaxMissingSuites = 16" in registry
    assert "record_missing_suite(safe_name, version)" in registry
    assert "const bool valid_name" in registry
    assert "if (!valid_name || version <= 0) return" in registry
    assert source.count("<< missing_suites_report_json()") >= 2


def test_broker_uses_structured_report_not_stderr_for_missing_suites():
    source = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text(
        encoding="utf-8"
    )
    diagnostics = source[source.index("fn worker_diagnostics(") : source.index("fn diagnostics_contains_gpu_stage")]
    assert "missing_suite_event" not in diagnostics
    assert 'worker_report["missing_suites"]' in diagnostics
    assert "propagate_missing_suites(&mut diagnostics, report)" in source
    inspection = source[
        source.index("fn inspect_experimental_with_diagnostics_and_runtime_policy(") :
        source.index("pub fn probe_experimental_custom_ui_cursor(")
    ]
    assert "let worker_report: Option<Value>" in inspection
    assert inspection.index("propagate_missing_suites(&mut diagnostics, report)") < inspection.index(
        'if isolated.classification.as_str() != "ok"'
    )
