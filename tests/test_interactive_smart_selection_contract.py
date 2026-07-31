from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BROKER_SESSION = ROOT / "broker" / "crates" / "broker" / "src" / "image_render" / "session.rs"
LIVE_SESSION = ROOT / "broker" / "crates" / "harness" / "src" / "windows" / "live_session.rs"
APP = ROOT / "broker" / "crates" / "harness" / "src" / "windows" / "app.rs"


def test_resident_session_uses_the_existing_smart_selection_and_reports_it():
    broker_session = BROKER_SESSION.read_text(encoding="utf-8")
    live_session = LIVE_SESSION.read_text(encoding="utf-8")
    app = APP.read_text(encoding="utf-8")

    # The session receives the already-decided path; it does not infer support
    # from a plug-in name or choose a separate resident-only policy.
    assert "smart: request.smart" in broker_session
    assert '"render_path": if self.smart { "smartfx" } else { "classic" }' in broker_session
    assert '"smart_capability_source": self.smart_capability_source' in broker_session
    assert 'summary["smart_capability_source"]' in broker_session

    # A SmartFX selection is session-keyed and reaches both the resident open
    # and its one-shot infrastructure fallback without being changed to classic.
    assert "smart: request.smart," in live_session
    assert "smart_capability_source: &request.smart_capability_source," in live_session
    assert "request.smart," in live_session
    assert "smart: bool," in live_session

    # Inspection remains the normal source, with an explicit GUI override and
    # the pre-existing no-capability classic default recorded rather than guessed.
    assert "selected_smart_capability_source" in app
    assert "let live_eligible = host_context.is_none()" in app
    assert "let live_eligible = !smart" not in app
    for source in (
        "inspection_out_flags2",
        "explicit_gui_override",
        "classic_default_no_capability_report",
    ):
        assert source in live_session
