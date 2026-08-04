from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
REPORT = (ROOT / "minihost" / "src" / "worker_report.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost" / "src" / "worker_report.hpp").read_text(encoding="utf-8")
L2 = source_owners.l2_translation_unit_text()
BROKER = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
SELECTOR = (ROOT / "minihost" / "src" / "worker_selector_dispatch.cpp").read_text(
    encoding="utf-8"
)




def test_broker_preserves_bounded_timeline_schema_on_failure():
    assert "const MAX_SUITE_TIMELINE_EVENTS: usize = 512;" in BROKER
    assert "fn propagate_suite_timeline(" in BROKER
    assert "timeline.len() >= MAX_SUITE_TIMELINE_EVENTS" in BROKER
    assert "const MAX_SUITE_NAME_LEN: usize = 64;" in BROKER
    assert "const MAX_SUITE_VERSION: i64 = u16::MAX as i64;" in BROKER
    assert 'let Some(selector) = event["selector"]' in BROKER
    assert ".filter(|selector| schema_safe_suite_selector(selector))" in BROKER
    assert '"sequence": sequence' in BROKER
    assert '"action": action' in BROKER
    assert '"result": result' in BROKER
    assert "propagate_suite_timeline(&mut diagnostics, report);" in BROKER
    assert 'diagnostics["suite_timeline_truncated"] = Value::Bool(truncated);' in BROKER
    assert 'diagnostics["missing_suites_truncated"] = Value::Bool(truncated);' in BROKER
    # The structured report remains an Err for a non-ok worker; telemetry is
    # evidence, never a success downgrade.
    assert 'if isolated.classification.as_str() != "ok"' in BROKER
    assert 'return Err(invalid(format!(' in BROKER




