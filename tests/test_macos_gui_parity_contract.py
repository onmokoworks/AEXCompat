from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MACOS_GUI = ROOT / "broker/crates/harness/src/macos.rs"
GUI_STATE = ROOT / "broker/crates/harness/src/gui_state.rs"
DECISION = ROOT / "analysis/MACOS_WINDOWS_GUI_PARITY_2026-07-25.md"




def test_parity_decision_separates_editing_from_windows_only_diagnostics():
    decision = DECISION.read_text(encoding="utf-8")

    assert "Treat the Windows harness as the functional reference" in decision
    assert "Dependency approval / sealing" in decision
    assert "Custom UI / AEGP probes" in decision
    assert "deliberately Windows-only" in decision
    assert "Do not create parity issues merely because" in decision
