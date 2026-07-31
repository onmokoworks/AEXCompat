from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BROKER_SESSION = ROOT / "broker" / "crates" / "broker" / "src" / "image_render" / "session.rs"
LIVE_SESSION = ROOT / "broker" / "crates" / "harness" / "src" / "windows" / "live_session.rs"
APP = ROOT / "broker" / "crates" / "harness" / "src" / "windows" / "app.rs"
INSPECTION = ROOT / "broker" / "crates" / "broker" / "src" / "image_render" / "inspection_and_probes.rs"


def test_resident_session_uses_the_existing_smart_selection_and_reports_it():
    broker_session = BROKER_SESSION.read_text(encoding="utf-8")
    live_session = LIVE_SESSION.read_text(encoding="utf-8")
    app = APP.read_text(encoding="utf-8")
    inspection = INSPECTION.read_text(encoding="utf-8")

    # The session receives a closed, validated selection snapshot; it cannot
    # pair a caller-provided free-form source with an arbitrary selector path.
    assert "pub enum InteractiveCapabilitySource" in broker_session
    assert "pub struct InteractiveSessionSelection" in broker_session
    assert "interactive render capability selection is invalid" in broker_session
    assert "smart: request.selection.path.is_smart()" in broker_session
    assert '"render_path": self.selection.path.report_name()' in broker_session
    assert '"smart_capability_source": self.selection.source.report_name()' in broker_session
    assert '"smart_capability_identity": self.selection.capability_identity' in broker_session
    assert '"smart_capability_version": self.selection.capability_version' in broker_session
    assert "pub fn annotate_interactive_selection" in broker_session

    # A SmartFX selection is session-keyed and reaches both resident open and
    # the one-shot infrastructure fallback without being changed to classic.
    assert "selection: request.selection," in live_session
    assert "request.selection.path.is_smart()," in live_session
    assert "annotate_interactive_selection(" in live_session
    assert "interactive_selection_failure(" in live_session
    assert "selection: aexcompat_broker::image_render::InteractiveSessionSelection" in live_session
    assert "INTERACTIVE_CAPABILITY_VERSION" in live_session
    assert "out_flags2" in live_session

    # Inspection is the only capability authority. Missing, malformed, stale,
    # or contradictory facts clear the prior state and block every render path.
    assert "inspected_render_capability" in app
    assert "self.smart_render_capability = None;" in app
    assert "no valid SmartFX/classic capability inspection is available" in app
    assert "automatic render path does not match" in live_session
    assert "inspection report has no valid out_flags2" in inspection
    assert ".get(\"out_flags2\")" in inspection
    capability_slice = inspection[
        inspection.index('let advertised_out_flags ='):inspection.index('let audio_effect_only =')
    ]
    assert ".unwrap_or(0)" not in capability_slice
    assert "let live_eligible = host_context.is_none()" in app
    assert "let live_eligible = !smart" not in app
    assert "annotate_interactive_selection(" in app
    for source in (
        "advertised_smart",
        "advertised_classic",
        "manual_smart",
        "manual_classic",
    ):
        assert source in broker_session
