import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_AUTOMATIC_OPTIONS_DIALOG_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"
FIXTURE = ROOT / "instruments" / "pf-auto-dialog-probe" / "pf_auto_dialog_probe.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_sequence_request_dispatches_dialog_in_strict_lifecycle_order():
    request = result()["automatic_request"]
    assert request["dialog_capability_advertised"] is True
    assert request["automatic_dialog_requested"] is True
    assert request["selector_dispatched"] is True
    assert request["selector_order"] == [
        "global_setup", "params_setup", "sequence_setup", "do_dialog",
        "sequence_setdown", "global_setdown",
    ]
    assert request["sequence_setup_error"] == request["dialog_error"] == 0
    assert request["sequence_setdown_error"] == request["global_setdown_error"] == 0
    assert request["all_exception_codes_zero"] is True
    assert request["handle_lifetimes_balanced"] is True


def test_dialog_capability_alone_does_not_trigger_automatic_dispatch():
    skipped = result()["capability_without_request"]
    assert skipped["dialog_capability_advertised"] is True
    assert skipped["automatic_dialog_requested"] is False
    assert skipped["selector_dispatched"] is False
    assert skipped["worker_status"] == "automatic_dialog_not_requested"
    assert skipped["worker_exit_code"] == 21


def test_automatic_dialog_is_cleanroom_abi_bound_isolated_and_exposed():
    evidence = result()
    worker = source_owners.worker_text()
    broker = BROKER.read_text(encoding="utf-8")
    harness = HARNESS.read_text(encoding="utf-8")
    fixture = FIXTURE.read_text(encoding="utf-8")
    assert evidence["abi"]["send_do_dialog_flag"] == 128
    assert "PF_OutFlag_SEND_DO_DIALOG" in fixture
    assert "constexpr uint32_t kOutFlagSendDoDialog = 1u << 7;" in worker
    assert "invoke_sequence_selector(entry, kSequenceSetup" in worker
    assert "pub fn probe_experimental_automatic_options_dialog" in broker
    assert '"automatic_dialog"' in broker
    assert 'args[1] == "--probe-experimental-automatic-options-dialog"' in harness
    assert "Probe automatic options dialog" in harness
