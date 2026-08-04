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


