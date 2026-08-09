import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TRANSPORT = ROOT / "target/image-transport"
# The broker runs the worker with <repository>/target as its CWD, and the
# loader derives the transport boundary from that. Launching from ROOT
# makes every sidecar look like it sits outside the boundary.
WORKER_CWD = ROOT / "target"

def _workers():
    builds = [ROOT / "target/minihost-build-v18", ROOT / "target/minihost-build"]
    build = next((candidate for candidate in builds if candidate.is_dir()), builds[0])
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
            cwd=WORKER_CWD, text=True, capture_output=True, timeout=30, check=False,
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
        completed = subprocess.run([str(worker), "--self-test-parameter-animation"], cwd=WORKER_CWD,
                                   text=True, capture_output=True, timeout=30, check=False)
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout)["parameter_animation_transport"] == "passed"

def test_parameter_registry_accepts_more_than_1024_entries():
    for worker in _workers():
        assert worker.is_file(), f"missing VS2022 worker: {worker}"
        completed = subprocess.run(
            [str(worker), "--self-test-parameter-registry-capacity"],
            cwd=WORKER_CWD, text=True, capture_output=True, timeout=30, check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout)["parameter_registry_capacity"] == "passed"

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

def test_sidecar_is_confined_to_broker_owned_transport_and_trailers_are_peeled():
    worker = _workers()[0]
    outside = ROOT / "target/parameter-animation-outside.json"
    outside.parent.mkdir(parents=True, exist_ok=True)
    outside.write_text(json.dumps(_valid()), encoding="utf-8")
    try:
        completed = subprocess.run([str(worker), "--self-test-parameter-animation-sidecar", str(outside.resolve())],
                                   cwd=WORKER_CWD, text=True, capture_output=True, timeout=30, check=False)
        assert completed.returncode == 3
    finally:
        outside.unlink(missing_ok=True)
