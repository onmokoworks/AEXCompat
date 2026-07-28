import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_PATHMASTER_SEQUENCE_FLATTEN_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"
PROBE = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_pathmaster_flatten_resetup_uses_three_distinct_owned_handles():
    evidence = result()
    roundtrip = evidence["roundtrip"]
    ownership = evidence["ownership"]
    for key in (
        "sequence_setup_error",
        "sequence_flatten_error",
        "sequence_resetup_error",
        "frame_render_error",
        "sequence_setdown_error",
    ):
        assert roundtrip[key] == 0
    assert roundtrip["unflat_to_flat_handle_replaced"] is True
    assert roundtrip["flat_to_unflat_handle_replaced"] is True
    assert roundtrip["flattened_handle_host_disposed"] is True
    assert ownership["handles_created"] == ownership["handles_disposed"] == 3
    assert ownership["handle_lifetimes_balanced"] is True


def test_resetup_data_is_usable_by_a_real_path_render():
    render = result()["path_render_after_resetup"]
    assert render["checkout_calls"] == render["checkin_calls"] == 1
    assert render["mask_world_calls"] == 1
    assert render["path_lifetimes_balanced"] is True
    assert render["invalid_path_operations"] == 0
    assert render["guard_bytes_intact"] is True
    assert len(render["output_sha256"]) == 64


def test_flatten_selector_is_abi_bound_isolated_and_exposed_to_the_harness():
    evidence = result()
    worker = source_owners.worker_text()
    broker = BROKER.read_text(encoding="utf-8")
    harness = source_owners.harness_windows_text()
    probe = PROBE.read_text(encoding="utf-8")
    assert evidence["abi"]["sequence_flatten_selector"] == 7
    assert "PF_Cmd_SEQUENCE_FLATTEN" in probe
    assert "constexpr int32_t kSequenceFlatten = 7;" in worker
    assert '"sequence_flatten"' in broker
    assert "pub fn probe_experimental_flattened_sequence" in broker
    assert "flattened sequence worker contract failed" in broker
    assert 'args[1] == "--probe-experimental-flattened-sequence"' in harness
    assert "Probe sequence save/reload" in harness
