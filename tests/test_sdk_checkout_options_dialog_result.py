import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_CHECKOUT_OPTIONS_DIALOG_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
MODE_EXECUTION = ROOT / "minihost" / "src" / "l2_mode_execution.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_checkout_advertises_and_completes_options_dialog_selector():
    dialog = result()["advertised_fixture"]
    assert dialog["dialog_advertised"] is True
    assert dialog["selector_dispatched"] is True
    assert dialog["selector_error"] == dialog["exception_code"] == 0
    assert dialog["display_error_message"] is True
    assert "platform-specific options dialog" in dialog["return_message"]
    assert dialog["global_setdown_error"] == 0
    assert dialog["handle_lifetimes_balanced"] is True


def test_unadvertised_effect_is_refused_without_selector_dispatch():
    refused = result()["unadvertised_fixture"]
    assert refused["dialog_advertised"] is False
    assert refused["selector_dispatched"] is False
    assert refused["worker_status"] == "dialog_not_advertised"
    assert refused["worker_exit_code"] == 21
    assert refused["global_setdown_error"] == 0


def test_dialog_boundary_is_abi_bound_isolated_and_exposed():
    evidence = result()
    worker = source_owners.worker_text()
    mode_execution = MODE_EXECUTION.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")
    harness = source_owners.harness_windows_text()
    assert evidence["abi"] == {
        "selector": 9,
        "i_do_dialog_flag": 32,
        "display_error_message_flag": 256,
    }
    assert "constexpr int32_t kDoDialog = 9;" in worker
    assert "dialog_advertised" in mode_execution
    assert "invoke_entry_seh(b.entry, kDoDialog" in worker
    assert "pub fn probe_experimental_options_dialog" in broker
    assert '"do_dialog"' in broker
    assert 'args[1] == "--probe-experimental-options-dialog"' in harness
    assert "Probe options dialog" in harness
