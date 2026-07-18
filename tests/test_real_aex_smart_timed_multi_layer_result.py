import hashlib
import json
import math
import struct
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis/REAL_AEX_SMART_TIMED_MULTI_LAYER_RESULT_2026-07-17.json"


def _sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def _oracle(inputs, depth):
    streams = [inputs[name] for name in ("current", "past", "future")]
    output = []
    for offset in range(0, len(streams[0]), 4):
        for channel in range(4):
            values = [stream[offset + channel] for stream in streams]
            if depth == 8:
                output.append((values[0] + 2 * values[1] + 3 * values[2] + 3) // 6)
            elif depth == 16:
                typed = [(value * 32768 + 127) // 255 for value in values]
                mixed = (typed[0] + 2 * typed[1] + 3 * typed[2] + 3) // 6
                output.append((min(mixed, 32768) * 255 + 16384) // 32768)
            else:
                typed = [_f32(value / 255.0) for value in values]
                mixed = _f32(_f32(typed[0] + _f32(2.0 * typed[1])) + _f32(3.0 * typed[2]))
                mixed = _f32(mixed / 6.0)
                scaled = _f32(max(0.0, min(1.0, mixed)) * 255.0)
                output.append(math.floor(scaled + 0.5))
    return bytes(output)


def test_real_smartfx_timed_multilayer_evidence_is_authenticated():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    assert evidence["status"] == "passed"
    assert evidence["artifacts"]["source"]["path"] == "minihost/src/l2_main.cpp"
    assert evidence["artifacts"]["worker"]["path"] == "target/minihost-build/aex_smart_worker.exe"
    assert evidence["dimensions"] == [2, 2]
    assert evidence["render_time"] == {"value": 6, "scale": 8}
    assert [item["transport_key"] for item in evidence["requested_layers"]] == [
        "v1|1|6|8", "v1|1|1|3", "v1|1|5|4"
    ]
    assert [item["checkout_id"] for item in evidence["requested_layers"]] == [101, 202, 303]
    assert [item["relation"] for item in evidence["requested_layers"]] == [
        "current", "non_current", "non_current"
    ]
    for artifact in evidence["artifacts"].values():
        path = ROOT / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert _sha256(path) == artifact["sha256"]


def test_all_depths_match_probe_independent_pixel_oracle_and_runtime_report():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    inputs = {name: bytes(values) for name, values in evidence["input_pixels_rgba8"].items()}
    assert [run["depth"] for run in evidence["runs"]] == [8, 16, 32]
    for run in evidence["runs"]:
        expected = _oracle(inputs, run["depth"])
        output_path = ROOT / run["output"]["path"]
        report_path = ROOT / run["report"]["path"]
        assert expected == bytes(run["expected_output_rgba8"])
        assert Image.open(output_path).convert("RGBA").tobytes() == expected
        for artifact, path in ((run["output"], output_path), (run["report"], report_path)):
            assert path.stat().st_size == artifact["size_bytes"]
            assert _sha256(path) == artifact["sha256"]
        report = json.loads(report_path.read_text(encoding="utf-8"))
        assert report["passed"] is True
        assert run["status"] == "render_completed"
        assert report["render_path"] == "smartfx"
        assert report["pixel_format"] == run["pixel_format"]
        assert report["smart_render_error"] == run["smart_render_error"] == 0
        assert report["output_sha256"] == run["internal_output_sha256"]
        assert report["guard_bytes_intact"] is True
        assert report["worker_diagnostics"]["classification"] == "ok"
        assert report["worker_diagnostics"]["exit_code"] == 0
        completed = {
            event["stage"]: event["errors"]
            for event in report["worker_diagnostics"]["stage_events"]
            if event["state"] == "end"
        }
        assert completed["sequence_setup"]["error"] == 0
        assert completed["smart_pre_render"]["error"] == run["pre_render_error"] == 0
        assert completed["smart_render_cpu"]["error"] == 0


def test_authenticated_probe_makes_balanced_checkout_a_success_invariant():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    source = (ROOT / evidence["artifacts"]["probe_source"]["path"]).read_text(encoding="utf-8")
    assert "g_successful_checkouts == 3" in source
    assert "checkin_layer_pixels" in source
    assert "g_successful_checkins != g_successful_checkouts" in source
    assert all(run["smart_render_error"] == 0 for run in evidence["runs"])
    assert evidence["assertions"]["ae_process_touched"] is False
    assert all(value for name, value in evidence["assertions"].items()
               if name != "ae_process_touched")
