from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]


def test_production_worker_reports_bounded_missing_suites():
    source = source_owners.L2_MAIN.read_text(encoding="utf-8")
    report = (ROOT / "minihost" / "src" / "worker_render_report.cpp").read_text(
        encoding="utf-8"
    )
    registry = (ROOT / "minihost" / "src" / "worker_suite_registry.cpp").read_text(
        encoding="utf-8"
    )
    assert "constexpr std::size_t kMaxMissingSuites = 16" in registry
    assert "record_missing_suite(safe_name, version)" in registry
    assert "const bool valid_name" in registry
    assert "if (!valid_name || version <= 0) return" in registry
    assert source.count("missing_suites_report_json()") >= 2
    assert "value.missing_suites_json" in report
    assert "v.missing_suites_json" in report


def test_broker_uses_structured_report_not_stderr_for_missing_suites():
    source = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text(
        encoding="utf-8"
    )
    # The slice used to end at fn diagnostics_contains_gpu_stage, which existed
    # only to infer a GPU failure for the one-shot CPU retry and went with the
    # one-shot transport (#365). fn failed_module_audit_summary is the next
    # top-level item after worker_diagnostics now.
    diagnostics = source[
        source.index("fn worker_diagnostics(") : source.index("fn failed_module_audit_summary(")
    ]
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


def test_unsupported_suite_slots_flow_from_worker_report_to_broker_diagnostics():
    header = (ROOT / "minihost" / "src" / "worker_suite_registry.hpp").read_text(
        encoding="utf-8"
    )
    registry = (ROOT / "minihost" / "src" / "worker_suite_registry.cpp").read_text(
        encoding="utf-8"
    )
    source = source_owners.L2_MAIN.read_text(encoding="utf-8")
    l2_report_header = (ROOT / "minihost" / "src" / "worker_report.hpp").read_text(
        encoding="utf-8"
    )
    l2_report = (ROOT / "minihost" / "src" / "worker_report.cpp").read_text(
        encoding="utf-8"
    )
    report = (ROOT / "minihost" / "src" / "worker_render_report.cpp").read_text(
        encoding="utf-8"
    )
    smart = (ROOT / "minihost" / "src" / "worker_smart_report.cpp").read_text(
        encoding="utf-8"
    )
    broker = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text(
        encoding="utf-8"
    )

    assert "unsupported_suite_slot()" in header
    assert "constexpr std::size_t kMaxUnsupportedSuiteCalls = 32" in registry
    assert '"stage:suite_slot_unsupported suite="' in registry
    assert "unsupported_suite_calls" in registry
    assert "unsupported_suite_calls_report_json()" in source
    assert "unsupported_suite_calls_json" in l2_report_header
    assert "c.unsupported_suite_calls_json" in l2_report
    assert "unsupported_suite_calls_report_json()" in report
    assert "unsupported_suite_calls_report_json()" in smart
    assert "fn propagate_unsupported_suite_calls(" in broker
    assert 'worker_report["unsupported_suite_calls"]' in broker
    # Every broker path that turns a worker report into public diagnostics must
    # lift the structured suite records, or a compatibility gap stops being a
    # reproducible diagnostic. The count used to be `>= 3` because the one-shot
    # dispatch propagated twice (initial launch and CPU retry); #365 deleted it,
    # so assert the two surviving paths by name instead of by a floor that a
    # future deletion could satisfy while dropping the render path.
    for owner, marker in (
        ("inspection", "fn inspect_experimental_with_diagnostics_and_runtime_policy("),
        ("session render", "fn render_classic_via_length_one_session("),
    ):
        start = broker.index(marker)
        end = broker.index("\n}\n", start)
        body = broker[start:end]
        assert "propagate_missing_suites(&mut diagnostics" in body, owner
        assert "propagate_unsupported_suite_calls(&mut diagnostics" in body, owner
