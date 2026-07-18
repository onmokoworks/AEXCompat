import json
import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost/src/l2_main.cpp"
DISPATCH_SOURCE = ROOT / "minihost/src/l2_cli_dispatch.cpp"
TRANSPORT = ROOT / "target/image-transport"


def worker_source():
    return SOURCE.read_text(encoding="utf-8") + "\n" + DISPATCH_SOURCE.read_text(encoding="utf-8")


def _workers():
    build = ROOT / "target/minihost-build-v18"
    return [build / "aex_render_worker.exe", build / "aex_smart_worker.exe"]


def _run_sidecar(worker: Path, document, name="parameter-animation-test.json"):
    TRANSPORT.mkdir(parents=True, exist_ok=True)
    path = (TRANSPORT / name).resolve()
    if isinstance(document, str):
        path.write_text(document, encoding="utf-8")
    else:
        path.write_text(json.dumps(document, separators=(",", ":")), encoding="utf-8")
    try:
        return subprocess.run(
            [str(worker), "--self-test-parameter-animation-sidecar", str(path)],
            cwd=ROOT, text=True, capture_output=True, timeout=30, check=False,
        )
    finally:
        path.unlink(missing_ok=True)


def _valid():
    return {"schema_version": 1, "parameters": [{"slot": 2, "keys": [
        {"time": {"value": 0, "scale": 24}, "interpolation": "linear",
         "value": {"type": "scalar", "value": 1.25}},
        {"time": {"value": 1, "scale": 24}, "interpolation": "hold",
         "value": {"type": "scalar", "value": 2.5}},
    ]}]}


def _valid_arbitrary():
    return {"schema_version": 1, "parameters": [{"slot": 3, "keys": [
        {"time": {"value": 0, "scale": 24}, "interpolation": "linear",
         "value": {"type": "arbitrary", "value": [67, 71, 0, 1]}},
        {"time": {"value": 12, "scale": 24}, "interpolation": "hold",
         "value": {"type": "arbitrary", "value": [67, 71, 2, 3]}},
    ]}]}


def test_native_timeline_evaluation_and_param_utils():
    for worker in _workers():
        assert worker.is_file(), f"missing VS2022 worker: {worker}"
        completed = subprocess.run([str(worker), "--self-test-parameter-animation"], cwd=ROOT,
                                   text=True, capture_output=True, timeout=30, check=False)
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout)["parameter_animation_transport"] == "passed"


def test_param_utils_direction_state_and_checkout_safety_are_wired():
    source = worker_source()
    assert "const bool greater = direction == 0 || direction == 0x1000" in source
    assert "const bool inclusive = direction == 0x1000 || direction == 0x1001" in source
    assert "timeline->keys.size() - 1 - offset" in source
    assert "for (const auto &timeline : g_parameter_timelines)" in source
    assert "std::lock_guard<std::mutex> lock(g_keyframe_checkout_mutex)" in source
    assert "std::floor(key.scalar) != key.scalar" in source


def test_strict_sidecar_accepts_schema_and_rejects_malformed_documents():
    worker = _workers()[0]
    accepted = _run_sidecar(worker, _valid())
    assert accepted.returncode == 0, accepted.stderr or accepted.stdout
    invalid = []
    unknown = _valid(); unknown["extra"] = 1; invalid.append(unknown)
    duplicate_slot = _valid(); duplicate_slot["parameters"].append(duplicate_slot["parameters"][0]); invalid.append(duplicate_slot)
    zero_scale = _valid(); zero_scale["parameters"][0]["keys"][0]["time"]["scale"] = 0; invalid.append(zero_scale)
    unordered = _valid(); unordered["parameters"][0]["keys"][1]["time"] = {"value": 0, "scale": 48}; invalid.append(unordered)
    bad_interp = _valid(); bad_interp["parameters"][0]["keys"][0]["interpolation"] = "bezier"; invalid.append(bad_interp)
    too_many = _valid(); too_many["parameters"][0]["keys"] = too_many["parameters"][0]["keys"][:1] * 257; invalid.append(too_many)
    for index, document in enumerate(invalid):
        rejected = _run_sidecar(worker, document, f"parameter-animation-invalid-{index}.json")
        assert rejected.returncode == 3, (index, rejected.stdout, rejected.stderr)
    duplicate_field = '{"schema_version":1,"schema_version":1,"parameters":[]}'
    assert _run_sidecar(worker, duplicate_field, "parameter-animation-duplicate.json").returncode == 3
    nonfinite = json.dumps(_valid()).replace("1.25", "1e999")
    assert _run_sidecar(worker, nonfinite, "parameter-animation-nonfinite.json").returncode == 3


def test_arbitrary_sidecar_is_bounded_and_strict():
    worker = _workers()[0]
    accepted = _run_sidecar(worker, _valid_arbitrary(), "parameter-animation-arbitrary.json")
    assert accepted.returncode == 0, accepted.stderr or accepted.stdout
    invalid = []
    empty = _valid_arbitrary(); empty["parameters"][0]["keys"][0]["value"]["value"] = []; invalid.append(empty)
    wide = _valid_arbitrary(); wide["parameters"][0]["keys"][0]["value"]["value"] = [256]; invalid.append(wide)
    mixed = _valid_arbitrary(); mixed["parameters"][0]["keys"][1]["value"] = {"type": "scalar", "value": 1}; invalid.append(mixed)
    for index, document in enumerate(invalid):
        rejected = _run_sidecar(worker, document, f"parameter-animation-arbitrary-invalid-{index}.json")
        assert rejected.returncode == 3, (index, rejected.stdout, rejected.stderr)


def test_arbitrary_runtime_uses_adjacent_keys_and_owned_handles():
    source = worker_source()
    assert "(now - left) / (right_time - left)" in source
    assert "write<void*>(extra, 16, owned[selected])" in source
    assert "write<void*>(extra, 24, owned[right])" in source
    assert "write<void*>(new_extra, 16, &preallocated)" in source
    assert "replacement = preallocated" in source
    assert "replacement != preallocated" in source
    assert "void* interpolated = created" in source
    assert "interpolated != created" in source
    assert "timeline_controls_slot" in source
    assert "if (timeline_controls_slot) continue" in source
    assert "const bool compared = invoke_entry_seh" not in source
    assert "apply_arbitrary_parameter_animation" in source


def test_sidecar_is_confined_to_broker_owned_transport_and_trailers_are_peeled():
    worker = _workers()[0]
    outside = ROOT / "target/parameter-animation-outside.json"
    outside.parent.mkdir(parents=True, exist_ok=True)
    outside.write_text(json.dumps(_valid()), encoding="utf-8")
    try:
        completed = subprocess.run([str(worker), "--self-test-parameter-animation-sidecar", str(outside.resolve())],
                                   cwd=ROOT, text=True, capture_output=True, timeout=30, check=False)
        assert completed.returncode == 3
    finally:
        outside.unlink(missing_ok=True)
    source = worker_source()
    assert 'flag == L"--aux-manifest-v1"' in source
    assert 'flag == L"--parameter-animation-v1"' in source
    assert source.count("while (effective_argc >= 3)") == 1
