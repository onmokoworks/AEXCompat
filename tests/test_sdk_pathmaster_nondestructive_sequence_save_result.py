import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_PATHMASTER_NONDESTRUCTIVE_SEQUENCE_SAVE_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"
PROBE = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_flat_copy_does_not_replace_the_running_sequence():
    save = result()["save_copy"]
    assert save["sequence_setup_error"] == 0
    assert save["get_flattened_sequence_data_error"] == 0
    assert save["original_sequence_preserved"] is True
    assert save["flattened_copy_is_distinct"] is True
    assert save["flattened_copy_host_disposed"] is True
    assert save["render_with_original_after_save_error"] == 0
    assert save["sequence_setdown_error"] == 0


def test_host_and_plugin_each_dispose_their_owned_handle_once():
    evidence = result()
    ownership = evidence["ownership"]
    render = evidence["continued_path_render"]
    assert ownership["handles_created"] == ownership["handles_disposed"] == 2
    assert ownership["handle_lifetimes_balanced"] is True
    assert ownership["suite_acquires"] == ownership["suite_releases"] == 12
    assert ownership["suite_leases_balanced"] is True
    assert render["checkout_calls"] == render["checkin_calls"] == 1
    assert render["path_lifetimes_balanced"] is True
    assert render["invalid_path_operations"] == 0
    assert render["guard_bytes_intact"] is True


def test_modern_sequence_save_selector_is_abi_bound_and_user_accessible():
    evidence = result()
    worker = source_owners.worker_text()
    broker = BROKER.read_text(encoding="utf-8")
    harness = HARNESS.read_text(encoding="utf-8")
    probe = PROBE.read_text(encoding="utf-8")
    assert evidence["abi"]["get_flattened_sequence_data_selector"] == 28
    assert "PF_Cmd_GET_FLATTENED_SEQUENCE_DATA" in probe
    assert "constexpr int32_t kGetFlattenedSequenceData = 28;" in worker
    assert '"get_flattened_sequence_data"' in broker
    assert "pub fn probe_experimental_copied_flattened_sequence" in broker
    assert "non-destructive sequence save worker contract failed" in broker
    assert 'args[1] == "--probe-experimental-copied-flattened-sequence"' in harness
    assert "Probe non-destructive sequence save" in harness
