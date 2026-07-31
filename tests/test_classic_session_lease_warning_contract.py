from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RENDER_SESSION = ROOT / "broker" / "crates" / "broker" / "src" / "render_session.rs"
CLASSIC_WRAPPER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render" / "session.rs"


def test_classic_wrapper_uses_the_shared_typed_close_report_validator():
    session = RENDER_SESSION.read_text(encoding="utf-8")
    wrapper = CLASSIC_WRAPPER.read_text(encoding="utf-8")

    assert "pub(crate) enum FinalReportValidation" in session
    assert "pub(crate) enum CloseReportInvariant" in session
    assert "pub(crate) fn validate_final_report(" in session
    assert "pub(crate) fn validate_close_report(" in session
    assert "fn validated_wrapper_final_report(" in wrapper

    start = wrapper.index("fn render_classic_via_length_one_session(")
    end = wrapper.index("\nfn close_failure_diagnostic(", start)
    body = wrapper[start:end]
    assert "validated_wrapper_final_report(&close, request.smart)" in body
    assert "the render session did not close cleanly" not in body
    assert "suite_lease_warning" not in body
    assert "let close = session.close();" in body
    assert body.index("validated_wrapper_final_report(&close, request.smart)") < body.index(
        "png_written"
    )


def test_close_failure_evidence_is_bounded_to_invariant_and_lease_counters():
    wrapper = CLASSIC_WRAPPER.read_text(encoding="utf-8")
    start = wrapper.index("fn close_failure_diagnostic(")
    end = wrapper.index("\n/// Static configuration", start)
    diagnostic = wrapper[start:end]

    assert "invariant.as_str()" in diagnostic
    for key in (
        "suite_acquires",
        "suite_releases",
        "live_suite_lease_count",
        "live_suite_reference_count",
    ):
        assert f'counter("{key}")' in diagnostic
    assert "live_suite_leases" not in diagnostic
    assert "plugin_path" not in diagnostic
