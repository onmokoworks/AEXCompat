from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MACOS_GUI = ROOT / "broker/crates/harness/src/macos.rs"
GUI_STATE = ROOT / "broker/crates/harness/src/gui_state.rs"
DECISION = ROOT / "analysis/MACOS_WINDOWS_GUI_PARITY_2026-07-25.md"


def test_macos_gui_keeps_the_generic_effect_editing_workspace():
    source = MACOS_GUI.read_text(encoding="utf-8")

    for contract in (
        'SidePanel::left("effect_controls")',
        '"Effect Controls"',
        '"Reset All"',
        '"Auto Update"',
        'ViewerMode::Input, "INPUT"',
        'ViewerMode::Output, "AEX OUTPUT"',
        'ViewerMode::Compare, "COMPARE"',
        '"Fit"',
        "show_viewer_texture",
        "parameter_payload",
    ):
        assert contract in source

    forbidden_plugin_identities = (
        "OLMBlur",
        "f0611785e7b14ac4fcfc75f23b8862beb4539eee52d25d472556849535e96e5b",
    )
    for identity in forbidden_plugin_identities:
        assert identity not in source


def test_gui_state_owns_reset_and_live_render_debounce():
    source = GUI_STATE.read_text(encoding="utf-8")

    assert "Duration::from_millis(500)" in source
    assert "pub(crate) fn reset_all" in source
    assert "pub(crate) fn parameter_changed" in source
    assert "pub(crate) fn take_due" in source
    assert "fn live_render_debounces_parameter_changes" in source
    assert "fn live_render_waits_for_readiness_and_disables_cleanly" in source


def test_parity_decision_separates_editing_from_windows_only_diagnostics():
    decision = DECISION.read_text(encoding="utf-8")

    assert "Treat the Windows harness as the functional reference" in decision
    assert "Dependency approval / sealing" in decision
    assert "Custom UI / AEGP probes" in decision
    assert "deliberately Windows-only" in decision
    assert "Do not create parity issues merely because" in decision
