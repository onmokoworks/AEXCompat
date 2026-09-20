"""Bounded CUDA/input/intensity response, not an Adobe pixel-equivalence oracle."""
import hashlib
import json
import math
import os
import statistics
import struct
import subprocess
import time
from pathlib import Path, PureWindowsPath

import pytest
from PIL import Image

from test_fast_grain_fixed_diagnostic_card import CARD_SHA256, source_pixels
from test_render_fixture_semantic_response import HEIGHT, ROOT, WIDTH

PIXELS = WIDTH * HEIGHT
CASES = (("a-0", False, 0), ("a-100", False, 100),
         ("b-0", True, 0), ("b-100", True, 100))
PLUGIN_SHA = "13de85b4a300be6df2c40152259269442b4983924f86ad5ec093c04a2be39643"
PLUGIN_SIZE = 1568256


def require_assertions():
    if not __debug__:
        raise RuntimeError("evidence validation requires Python assertions")


def sha(data):
    return hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        assert key not in result, f"duplicate JSON key: {key}"
        result[key] = value
    return result


def read_json(path):
    return json.loads(path.read_bytes(), object_pairs_hook=unique_object)


def float_bytes(rgba):
    return struct.pack(f"<{len(rgba)}f", *(v / 255.0 for v in rgba))


def argb_float_bytes(rgba):
    assert len(rgba) == PIXELS * 16
    return b"".join(rgba[i + 12:i + 16] + rgba[i:i + 12]
                    for i in range(0, len(rgba), 16))


def argb8_bytes(rgba):
    assert len(rgba) == PIXELS * 4
    return b"".join(rgba[i + 3:i + 4] + rgba[i:i + 3]
                    for i in range(0, len(rgba), 4))


def assert_sha256(value):
    assert isinstance(value, str) and len(value) == 64
    assert all(character in "0123456789abcdef" for character in value)


def float_preview(raw):
    require_assertions()
    assert len(raw) == PIXELS * 16
    values = struct.unpack(f"<{PIXELS * 4}f", raw)
    assert all(math.isfinite(v) for v in values)
    assert all(abs(v - 1.0) <= 1e-6 for v in values[3::4])
    # Reproduce the documented f32-to-RGBA8 preview conversion, not a vendor
    # algorithm: the multiplication itself rounds to float32 before round().
    return bytes(int(math.floor(struct.unpack("<f", struct.pack(
        "<f", min(1.0, max(0.0, v)) * 255.0))[0] + 0.5)) for v in values)


def assert_response(outputs):
    require_assertions()
    assert set(outputs) == {case[0] for case in CASES}
    metrics = {}
    for prefix, alternate in (("a", False), ("b", True)):
        source = source_pixels(alternate)
        zero, high = outputs[f"{prefix}-0"], outputs[f"{prefix}-100"]
        assert len(zero) == len(high) == len(source) == PIXELS * 4
        assert zero[3::4] == high[3::4] == bytes([255]) * PIXELS
        assert max(abs(a - b) for a, b in zip(zero, source)) <= 1
        residual = [high[i] - source[i] for i in range(len(source)) if i % 4 != 3]
        changed = sum(any(abs(high[i + c] - source[i + c]) > 1 for c in range(3))
                      for i in range(0, len(source), 4))
        assert changed > PIXELS // 20
        assert statistics.pstdev(residual) > 1.0
        assert sha(high) != CARD_SHA256
        metrics[prefix] = {"changed_pixels": changed,
                           "residual_stddev": statistics.pstdev(residual),
                           "residual_mean": statistics.mean(residual)}
        if not alternate:
            assert sum(v > 1 for v in residual) > len(residual) // 100
            assert sum(v < -1 for v in residual) > len(residual) // 100
            assert abs(statistics.mean(residual)) < 32.0
    # A source-independent noise/card generator is not a working grain effect.
    assert outputs["a-100"] != outputs["b-100"]
    def rgb_mae(left, right):
        return statistics.mean(abs(left[i] - right[i])
                               for i in range(len(left)) if i % 4 != 3)
    matched = sum(rgb_mae(outputs[f"{p}-100"], outputs[f"{p}-0"])
                  for p in ("a", "b"))
    swapped = rgb_mae(outputs["a-100"], outputs["b-0"]) + rgb_mae(
        outputs["b-100"], outputs["a-0"])
    assert matched + 1.0 < swapped
    metrics["source_anchoring"] = {"matched_mae_sum": matched, "swapped_mae_sum": swapped}
    return metrics


def assert_report(report, source, output, intensity, identities, *, pixel_format="argb32f"):
    require_assertions()
    assert pixel_format in ("argb32f", "argb8")
    assert type(report["schema_version"]) is int and report["schema_version"] == 1
    assert report["stage"] == "interactive_image_render"
    expected_transport = ("native_raw+rgba8_png_preview"
                          if pixel_format == "argb32f" else "rgba8_png")
    assert report["output_transport"] == expected_transport
    for key in ("passed", "output_pixels_valid", "gpu_render_possible", "gpu_render_dispatched",
                "cuda_context_used", "guard_bytes_intact", "suite_leases_balanced",
                "handle_lifetimes_balanced", "world_lifetimes_balanced",
                "param_checkouts_balanced", "parameter_count_contract_ok",
                "smart_render_selector_dispatched"):
        assert report[key] is True, key
    assert report["gpu_fallback_used"] is False
    assert report["gpu_fallback_reason"] is None
    assert report["worker_classification"] == "ok"
    assert report["render_path"] == "smartfx"
    assert report["pixel_format"] == pixel_format
    assert report["cuda_upload_bytes"] == report["cuda_download_bytes"] == PIXELS * 16
    for key in ("gpu_device_setup_error", "gpu_device_setdown_error",
                "gpu_device_setdown_exception_code", "cuda_sync_failures",
                "pre_render_error", "smart_render_error", "smart_render_selector_error",
                "last_seh_exception_code"):
        assert type(report[key]) is int and report[key] == 0, key
    assert (report["width"], report["height"]) == (WIDTH, HEIGHT)
    assert (report["current_time"], report["time_step"], report["total_time"],
            report["time_scale"]) == (0, 1, 300, 30)
    if pixel_format == "argb32f":
        assert report["gpu_attempt"] is None
        assert report["output_sha256"] == sha(argb_float_bytes(output))
        assert report["input_sha256"] == sha(argb_float_bytes(float_bytes(source)))
    else:
        assert report["output_sha256"] == sha(argb8_bytes(output))
        assert report["input_sha256"] == sha(argb8_bytes(source))
        attempt = report["gpu_attempt"]
        assert attempt["setup_dispatched"] is True
        assert attempt["render_dispatched"] is True
        assert attempt["fallback_used"] is False
        assert attempt["fallback_reason"] == ""
        assert attempt["internal_pixel_format"] == "argb32f"
        for key in ("setup_error", "pre_error", "render_error", "setdown_error",
                    "cleanup_error", "lifecycle_error"):
            assert type(attempt[key]) is int and attempt[key] == 0, key
        assert attempt["internal_float_input_sha256"] == sha(
            argb_float_bytes(float_bytes(source)))
        assert_sha256(attempt["internal_float_output_sha256"])
    gpu = report["gpu_memory"]
    assert gpu["lifetimes_balanced"] is True
    assert gpu["allocations_created"] == gpu["allocations_freed"]
    assert gpu["live_allocation_count"] == gpu["live_bytes"] == gpu["invalid_operations"] == 0
    parameters = report["requested_parameters"]
    assert all(type(p["slot"]) is int for p in parameters)
    assert len({p["slot"] for p in parameters}) == len(parameters)
    assert len({p["id"] for p in parameters}) == len(parameters)
    by_slot = {p["slot"]: p for p in parameters}
    expected = {1: intensity, 2: 1.25, 3: 18, 4: 24, 9: 1,
                10: 0, 11: 0, 13: 8, 14: 100}
    assert set(by_slot) == set(expected) | {8}
    for slot, value in expected.items():
        assert type(by_slot[slot]["value"]) in (int, float)
        assert by_slot[slot]["value"] == value
        assert by_slot[slot]["id"] == f"param_{slot}"
        assert by_slot[slot]["kind"] == ("integer" if slot in (9, 10, 11, 13) else "float")
    assert by_slot[8]["id"] == "param_8"
    assert by_slot[8]["kind"] == "arbitrary_text"
    assert by_slot[8]["value"] == {"bytes": 11}
    diagnostics = report["worker_diagnostics"]
    assert diagnostics["classification"] == "ok"
    assert type(diagnostics["exit_code"]) is int and diagnostics["exit_code"] == 0
    assert diagnostics["last_completed_stage"] == "global_setdown"
    for key in ("failure_stage", "first_failure_stage", "active_stage", "load_failure", "kill_reason"):
        assert diagnostics[key] is None, key
    for key in ("missing_suites", "suite_acquire_failures", "unsupported_suite_calls",
                "callback_denials", "callback_addr_denials"):
        assert diagnostics[key] == []
        assert diagnostics[key + "_truncated"] is False
    assert diagnostics["suite_timeline_truncated"] is False
    assert diagnostics["stderr_truncated"] is False
    assert any(event.get("stage") == "smart_render_gpu" and event.get("state") == "end"
               and event.get("errors", {}).get("error") == 0
               for event in diagnostics["stage_events"])
    for action in ("acquire", "release"):
        assert any(event.get("name") == "AEGP Stream Suite" and event.get("version") == 10
                   and event.get("action") == action and event.get("result") == 0
                   and event.get("selector") == "SMART_PRE_RENDER"
                   for event in diagnostics["suite_timeline"])
    identity = diagnostics["execution_identity"]
    assert type(identity["schema_version"]) is int and identity["schema_version"] == 1
    assert identity["worker"]["sha256"] == identities["worker"]["sha256"]
    assert identity["worker"]["size_bytes"] == identities["worker"]["size_bytes"]
    assert identity["worker"]["binding"] == "broker_authenticated_pinned_stage"
    assert identity["plugin_images"] == [{
        "basename": "Fast Grain.aex", "plugin_index": 0,
        "sha256": identities["plugin"]["sha256"],
        "size_bytes": identities["plugin"]["size_bytes"],
        "binding_status": "same_file_identity_matches_loaded_module"}]


def validate_evidence(directory, *, verify_metrics=True):
    require_assertions()
    metadata = read_json(directory / "capture.json")
    assert type(metadata["schema_version"]) is int and metadata["schema_version"] == 1
    # Cycle 26 evidence predates this discriminator and is necessarily 32-bit.
    pixel_format = metadata.get("pixel_format", "argb32f")
    assert pixel_format in ("argb32f", "argb8")
    assert metadata["identities_before"] == metadata["identities_after"]
    identities = metadata["identities_before"]
    assert set(identities) == {"plugin", "worker", "harness"}
    for identity in identities.values():
        assert set(identity) == {"sha256", "size_bytes"}
        assert isinstance(identity["sha256"], str) and len(identity["sha256"]) == 64
        assert all(c in "0123456789abcdef" for c in identity["sha256"])
        assert type(identity["size_bytes"]) is int and identity["size_bytes"] > 0
    assert identities["plugin"] == {"sha256": PLUGIN_SHA, "size_bytes": PLUGIN_SIZE}
    assert set(metadata["cases"]) == {case[0] for case in CASES}
    outputs = {}
    for label, alternate, intensity in CASES:
        case = metadata["cases"][label]
        assert type(case["returncode"]) is int and case["returncode"] == 0
        assert case["intensity"] == intensity
        assert type(case["elapsed_seconds"]) in (int, float)
        assert math.isfinite(case["elapsed_seconds"]) and case["elapsed_seconds"] > 0
        command = case["command"]
        assert isinstance(command, list) and len(command) == 13
        assert command[1:3] == ["--headless", "--render-experimental-session-param"]
        assert command[6:] == [pixel_format, "smart", "0", "300", "30", "1",
                               str(intensity)]
        for index, name in ((0, "aexcompat-harness.exe"), (3, "Fast Grain.aex"),
                            (4, f"{label}-source.png"), (5, f"{label}.png")):
            assert PureWindowsPath(command[index]).name == name
        source = source_pixels(alternate)
        with Image.open(directory / f"{label}-source.png") as image:
            assert image.size == (WIDTH, HEIGHT)
            assert image.convert("RGBA").tobytes() == source
        with Image.open(directory / f"{label}.png") as image:
            assert image.size == (WIDTH, HEIGHT)
            outputs[label] = image.convert("RGBA").tobytes()
        raw_path = directory / f"{label}.rgba32f-le"
        if pixel_format == "argb32f":
            rendered = raw_path.read_bytes()
            assert outputs[label] == float_preview(rendered)
            if intensity == 0:
                samples = struct.unpack(f"<{PIXELS * 4}f", rendered)
                assert max(abs(v - b / 255.0)
                           for v, b in zip(samples, source)) <= 1 / 255 + 1e-6
        else:
            assert not raw_path.exists()
            rendered = outputs[label]
        report = read_json(directory / f"{label}.stdout.json")
        assert (directory / f"{label}.stderr.txt").read_bytes() == b""
        assert report["output_png"] == command[5]
        if pixel_format == "argb32f":
            assert (PureWindowsPath(report["output_raw"])
                    == PureWindowsPath(command[5]).with_suffix(".rgba32f-le"))
        else:
            assert report["output_raw"] is None
        assert_report(report, source, rendered, intensity, identities,
                      pixel_format=pixel_format)
    metrics = assert_response(outputs)
    if verify_metrics:
        assert read_json(directory / "verification.json") == metrics
    return metrics


def synthetic_outputs():
    outputs = {}
    for label, alternate, intensity in CASES:
        source = source_pixels(alternate)
        data = bytearray(source)
        if intensity:
            for i in range(0, len(data), 4):
                shift = 12 if (i // 4) % 2 else -12
                for c in range(3):
                    data[i + c] = min(255, max(0, data[i + c] + shift))
        outputs[label] = bytes(data)
    return outputs


@pytest.mark.parametrize("fault", ["passthrough", "fixed_noise", "swapped_sources", "watermark", "zero_effect",
    "constant_bias", "one_sided_noise", "alpha", "truncated", "card"])
def test_grain_response_oracle_rejects_false_success(fault):
    outputs = synthetic_outputs()
    assert_response(outputs)
    if fault == "passthrough":
        outputs["a-100"] = source_pixels()
    elif fault == "fixed_noise":
        outputs["b-100"] = outputs["a-100"]
    elif fault == "swapped_sources":
        outputs["a-100"], outputs["b-100"] = outputs["b-100"], outputs["a-100"]
    elif fault == "watermark":
        outputs["a-100"] = outputs["a-100"][:4] + source_pixels()[4:]
    elif fault == "zero_effect":
        outputs["a-0"] = outputs["a-100"]
    elif fault in ("constant_bias", "one_sided_noise"):
        data = bytearray(source_pixels())
        for i in range(0, len(data), 4):
            for c in range(3):
                data[i + c] += 10 if fault == "constant_bias" else 2 + (i // 4) % 20
        outputs["a-100"] = bytes(data)
    elif fault == "alpha":
        data = bytearray(outputs["a-100"])
        data[3] = 0
        outputs["a-100"] = bytes(data)
    elif fault == "card":
        outputs["a-100"] = outputs["b-100"] = bytes((0, 153, 255, 255)) * PIXELS
    else:
        outputs["a-100"] = outputs["a-100"][:-4]
    with pytest.raises(AssertionError):
        assert_response(outputs)


@pytest.mark.parametrize("fault", ["nan", "infinite", "alpha", "short"])
def test_float_validator_rejects_invalid_output(fault):
    raw = bytearray(float_bytes(source_pixels()))
    if fault == "short":
        raw = raw[:-4]
    else:
        offset = 12 if fault == "alpha" else 0
        value = {"nan": float("nan"), "infinite": float("inf"), "alpha": 0.0}[fault]
        raw[offset:offset + 4] = struct.pack("<f", value)
    with pytest.raises(AssertionError):
        float_preview(raw)


def test_installed_fast_grain_gpu_response(tmp_path):
    plugin_env = os.environ.get("AEXCOMPAT_TEST_FAST_GRAIN_GPU")
    if not plugin_env:
        pytest.skip("set AEXCOMPAT_TEST_FAST_GRAIN_GPU to installed Fast Grain.aex")
    require_assertions()
    assert os.name == "nt"
    plugin = Path(plugin_env).resolve()
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"
    worker = ROOT / "target/minihost-build/aex_worker.exe"

    def identities():
        return {name: {"sha256": sha(path.read_bytes()), "size_bytes": path.stat().st_size}
                for name, path in (("plugin", plugin), ("harness", harness), ("worker", worker))}

    metadata = {"schema_version": 1, "identities_before": identities(), "cases": {}}
    # Save every process report before applying the semantic oracle, so a
    # stricter offline validator never needs another vendor render to inspect it.
    for label, alternate, intensity in CASES:
        source = tmp_path / f"{label}-source.png"
        Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels(alternate)).save(source)
        command = [str(harness), "--headless", "--render-experimental-session-param",
                   str(plugin), str(source), str(tmp_path / f"{label}.png"),
                   "argb32f", "smart", "0", "300", "30", "1", str(intensity)]
        started = time.perf_counter()
        # This command includes inspection, which deliberately has no discovery
        # deadline. The interactive frame retains the broker's own deadline.
        result = subprocess.run(command, cwd=ROOT, capture_output=True)
        (tmp_path / f"{label}.stdout.json").write_bytes(result.stdout)
        (tmp_path / f"{label}.stderr.txt").write_bytes(result.stderr)
        metadata["cases"][label] = {"command": command, "intensity": intensity,
            "returncode": result.returncode, "elapsed_seconds": time.perf_counter() - started}
        (tmp_path / "capture.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    metadata["identities_after"] = identities()
    (tmp_path / "capture.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    metrics = validate_evidence(tmp_path, verify_metrics=False)
    (tmp_path / "verification.json").write_text(json.dumps(metrics, indent=2), encoding="utf-8")
    validate_evidence(tmp_path)


def test_installed_fast_grain_gpu_response_auto8(tmp_path):
    plugin_env = os.environ.get("AEXCOMPAT_TEST_FAST_GRAIN_AUTO8")
    if not plugin_env:
        pytest.skip("set AEXCOMPAT_TEST_FAST_GRAIN_AUTO8 to installed Fast Grain.aex")
    require_assertions()
    assert os.name == "nt"
    plugin = Path(plugin_env).resolve()
    harness = ROOT / "broker/target/release/aexcompat-harness.exe"
    worker = ROOT / "target/minihost-build/aex_worker.exe"

    def identities():
        return {name: {"sha256": sha(path.read_bytes()), "size_bytes": path.stat().st_size}
                for name, path in (("plugin", plugin), ("harness", harness), ("worker", worker))}

    metadata = {"schema_version": 1, "pixel_format": "argb8",
                "identities_before": identities(), "cases": {}}
    for label, alternate, intensity in CASES:
        source = tmp_path / f"{label}-source.png"
        Image.frombytes("RGBA", (WIDTH, HEIGHT), source_pixels(alternate)).save(source)
        command = [str(harness), "--headless", "--render-experimental-session-param",
                   str(plugin), str(source), str(tmp_path / f"{label}.png"),
                   "argb8", "smart", "0", "300", "30", "1", str(intensity)]
        started = time.perf_counter()
        # Discovery remains unbounded; the interactive frame owns its broker deadline.
        result = subprocess.run(command, cwd=ROOT, capture_output=True)
        (tmp_path / f"{label}.stdout.json").write_bytes(result.stdout)
        (tmp_path / f"{label}.stderr.txt").write_bytes(result.stderr)
        metadata["cases"][label] = {
            "command": command,
            "intensity": intensity,
            "returncode": result.returncode,
            "elapsed_seconds": time.perf_counter() - started,
        }
        (tmp_path / "capture.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    metadata["identities_after"] = identities()
    (tmp_path / "capture.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    metrics = validate_evidence(tmp_path, verify_metrics=False)
    (tmp_path / "verification.json").write_text(json.dumps(metrics, indent=2), encoding="utf-8")
    validate_evidence(tmp_path)


def test_recorded_fast_grain_gpu_response():
    directory = os.environ.get("AEXCOMPAT_FAST_GRAIN_GPU_EVIDENCE")
    if not directory:
        pytest.skip("set AEXCOMPAT_FAST_GRAIN_GPU_EVIDENCE for offline evidence validation")
    validate_evidence(Path(directory))


def test_recorded_fast_grain_gpu_response_auto8():
    directory = os.environ.get("AEXCOMPAT_FAST_GRAIN_AUTO8_EVIDENCE")
    if not directory:
        pytest.skip("set AEXCOMPAT_FAST_GRAIN_AUTO8_EVIDENCE for offline evidence validation")
    validate_evidence(Path(directory))
