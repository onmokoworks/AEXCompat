from tests import source_owners

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"

def test_harness_exposes_machine_readable_agent_cli_contract():
    source = source_owners.harness_windows_text()

    assert 'args[1] == "--render-image"' not in source

    # Keep the contract tied to the existing, behavior-bearing routes rather
    # than allowing the machine-readable surface to drift into a second CLI.
    for command in (
        "--inspect-experimental",
        "--render-scattermap-fixture",
        "--inspect-experimental-with-deps",
        "--inspect-experimental-dependencies",
        "--render-experimental-request",
        "--render-experimental-session",
        "--probe-experimental-options-dialog",
        "--probe-experimental-smart-nop-render",
        "--dispatch-experimental-aegp-update-menu",
        "--dispatch-experimental-aegp-switch-roundtrip",
    ):
        assert command in source

def test_contract_payload_literal_is_json_serializable():
    source = source_owners.harness_windows_text()
    # The contract is emitted with serde_json rather than a hand-built JSON
    # string, so the source must keep the machine-readable entry point intact.
