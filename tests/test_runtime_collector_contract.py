from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONFORMANCE = ROOT / "broker" / "crates" / "broker" / "src" / "conformance.rs"
IMAGE_RENDER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_runtime_collector_keeps_host_failures_and_checked_partial_extents():
    text = CONFORMANCE.read_text(encoding="utf-8")
    for marker in (
        "classify_broker_io_error",
        "Classification::InvalidOutput",
        "Classification::HostValidationError",
        "extent_hint.left < 0",
        "extent_hint.top < 0",
        "extent_hint.right > i64::try_from(width)",
        "extent_hint.bottom > i64::try_from(height)",
    ):
        assert marker in text


def test_runtime_report_promotes_native_suite_timeline():
    text = IMAGE_RENDER.read_text(encoding="utf-8")
    assert '("suite_timeline", "suite_timeline")' in text
    assert '"suite_timeline": initial_report.as_ref()' in text
