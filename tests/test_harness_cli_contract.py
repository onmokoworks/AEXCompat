from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"


def test_harness_exposes_machine_readable_agent_cli_contract():
    source = HARNESS.read_text(encoding="utf-8")

    assert 'const CLI_CONTRACT_SCHEMA: &str = "aexcompat.harness-cli-contract";' in source
    assert 'args[1] == "--print-cli-contract"' in source
    assert 'args[1] == "--help"' in source
    assert '"success_stdout": "json"' in source
    assert '"failure_stderr": true' in source
    assert '"unknown_or_malformed_arguments": "launch_gui"' in source
    assert 'args[1] == "--render-scattermap-fixture"' in source
    assert 'args[1] == "--render-image"' not in source
    assert "render_scattermap_fixture" in source

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
    source = HARNESS.read_text(encoding="utf-8")
    assert "fn cli_contract() -> serde_json::Value" in source
    assert '"aexcompat.harness-cli-contract"' in source
    # The contract is emitted with serde_json rather than a hand-built JSON
    # string, so the source must keep the machine-readable entry point intact.
    assert 'serde_json::to_string_pretty(&cli_contract())' in source
