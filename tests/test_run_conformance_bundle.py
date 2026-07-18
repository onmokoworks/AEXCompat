import hashlib
import json
import subprocess
import sys
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator
from PIL import Image
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "tools" / "run-conformance-bundle.py"
SCHEMAS = ROOT / "schemas"


def identity(path: Path, relative: str):
    raw = path.read_bytes()
    return {"path": relative, "sha256": hashlib.sha256(raw).hexdigest(), "size_bytes": len(raw)}


def fixture(tmp_path: Path):
    source = tmp_path / "fixture"
    (source / "artifacts").mkdir(parents=True)
    (source / "inputs").mkdir()
    (source / "runner").mkdir()
    plugin = source / "artifacts" / "effect.aex"
    image = source / "inputs" / "input.png"
    harness = source / "runner" / "harness.exe"
    plugin.write_bytes(b"aex")
    Image.new("RGBA", (2, 2), (10, 20, 30, 255)).save(image)
    harness.write_bytes(b"runner")
    manifest = {
        "schema_version": 1,
        "fixture_id": "runner-test",
        "plugin": {"aex": identity(plugin, "artifacts/effect.aex"), "dependencies": []},
        "input": identity(image, "inputs/input.png"),
        "runner": identity(harness, "runner/harness.exe"),
        "requested_depths": ["argb8", "argb16"],
        "execution": {
            "time": {"value": 0, "scale": 30},
            "parameters": [{"index": 1, "type": "slider", "value": 50}],
            "premultiplication": "straight",
            "color_management": {"enabled": False, "working_space": None},
            "linear_light": False,
            "renderer": "AEXCompat CPU",
        },
        "oracle": {"state": "not_captured", "identity_match": False},
    }
    path = source / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    adapter = tmp_path / "adapter.py"
    adapter.write_text(
        "import argparse, hashlib, json\n"
        "p=argparse.ArgumentParser()\n"
        "[p.add_argument(x) for x in ('--depth','--runner','--plugin','--input','--output','--request','--world-dump-dir')]\n"
        "a=p.parse_args(); bpp={'argb8':4,'argb16':8,'argb32f':16}[a.depth]\n"
        "data=b'output-'+a.depth.encode(); open(a.output,'wb').write(data)\n"
        "import os; os.makedirs(a.world_dump_dir); open(os.path.join(a.world_dump_dir,'000-smart-input-2x2.raw'),'wb').write(b'i'*(4*bpp)); open(os.path.join(a.world_dump_dir,'001-smart-output-2x2.raw'),'wb').write(b'o'*(4*bpp))\n"
        "w={'width':2,'height':2,'row_bytes':2*bpp,'pixel_format':a.depth,'premultiplication':'straight','extent_hint':{'left':0,'top':0,'right':2,'bottom':2}}\n"
        "print(json.dumps({'depth':a.depth,'classification':'ok','selector':{'render_path':'smartfx','completed':True,'error_code':0},'input_world':w,'world':w,'raw_input':None,'raw_output':None,'output_sha256':hashlib.sha256(data).hexdigest(),'suite_timeline':[],'oracle':{'state':'not_captured','identity_match':False,'exact':False}}))\n",
        encoding="utf-8",
    )
    return path, adapter


def invoke(manifest: Path, output: Path, adapter: Path):
    return subprocess.run(
        [sys.executable, str(RUNNER), "--manifest", str(manifest), "--out", str(output), "--adapter-command", str(adapter)],
        cwd=ROOT, capture_output=True, text=True,
    )


def report_validator():
    manifest_schema = json.loads((SCHEMAS / "conformance-manifest.schema.json").read_text())
    report_schema = json.loads((SCHEMAS / "conformance-report.schema.json").read_text())
    registry = Registry().with_resource(manifest_schema["$id"], Resource.from_contents(manifest_schema))
    return Draft202012Validator(report_schema, registry=registry)


def test_creates_self_contained_schema_valid_bundle(tmp_path):
    manifest, adapter = fixture(tmp_path)
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    report_validator().validate(report)
    assert (output / "artifacts" / "effect.aex").read_bytes() == b"aex"
    assert (output / "inputs" / "input.png").is_file()
    assert (output / "outputs" / "argb16.png").is_file()
    metadata = json.loads((output / "diagnostics" / "run.json").read_text())
    assert len(metadata["bundle_runner"]["sha256"]) == 64
    assert metadata["status"] == "completed"
    assert report["parameters"][0]["initial_value"] == 50
    assert report["results"][0]["raw_input"]["path"].startswith("raw/argb8/")


def test_refuses_existing_bundle_and_identity_mismatch(tmp_path):
    manifest, adapter = fixture(tmp_path)
    output = tmp_path / "bundle"
    output.mkdir()
    assert invoke(manifest, output, adapter).returncode != 0
    output.rmdir()
    (manifest.parent / "inputs" / "input.png").write_bytes(b"changed")
    assert invoke(manifest, output, adapter).returncode != 0
    assert output.exists()
    assert (output / "diagnostics" / "failure.json").is_file()
    assert not (output / "report.json").exists()


def test_semantic_report_is_reproducible(tmp_path):
    manifest, adapter = fixture(tmp_path)
    first, second = tmp_path / "first", tmp_path / "second"
    assert invoke(manifest, first, adapter).returncode == 0
    assert invoke(manifest, second, adapter).returncode == 0
    assert json.loads((first / "report.json").read_text()) == json.loads((second / "report.json").read_text())


def test_bundle_artifact_identities_match_every_file(tmp_path):
    manifest, adapter = fixture(tmp_path)
    output = tmp_path / "bundle"
    assert invoke(manifest, output, adapter).returncode == 0
    report = json.loads((output / "report.json").read_text())
    artifacts = [
        report["identities"]["aex"],
        report["identities"]["input"],
        report["identities"]["runner"],
    ]
    for result in report["results"]:
        artifacts.extend([result["raw_input"], result["raw_output"]])
    for artifact in artifacts:
        path = output / artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_failure_leaves_bounded_diagnostic_bundle(tmp_path):
    manifest, adapter = fixture(tmp_path)
    adapter.write_text("import sys; print('x'*100000, file=sys.stderr); raise SystemExit(3)\n", encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    assert {item["classification"] for item in report["results"]} == {"nonzero_exit"}
    detail = json.loads((output / "diagnostics" / "run.json").read_text())
    assert detail["depths"]["argb8"]["stderr"]["truncated"] is True
    assert len(detail["depths"]["argb8"]["stderr"]["text"].encode()) <= 65536


def test_adapter_cannot_claim_success_for_wrong_output_identity(tmp_path):
    manifest, adapter = fixture(tmp_path)
    source = adapter.read_text(encoding="utf-8")
    adapter.write_text(source.replace("hashlib.sha256(data).hexdigest()", "'0'*64"), encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    assert {item["classification"] for item in report["results"]} == {"invalid_output"}


def test_request_sidecar_records_execution_and_redacted_full_argv(tmp_path):
    manifest, adapter = fixture(tmp_path)
    output = tmp_path / "bundle"
    assert invoke(manifest, output, adapter).returncode == 0
    request = json.loads((output / "requests" / "argb8.json").read_text())
    assert request["timing"] == {"frame": 0, "time_scale": 30, "time_step": 1}
    assert request["assignments"] == [{"slot": 1, "value": 50}]
    run = json.loads((output / "diagnostics" / "run.json").read_text())
    argv = run["depths"]["argb8"]["argv"]
    assert [item["index"] for item in argv] == list(range(len(argv)))
    assert any(item["role"] == "request" for item in argv if item["kind"] == "path")
    assert str(manifest.parent) not in json.dumps(run)


def test_validator_failure_persists_failure_evidence_without_report(tmp_path):
    manifest, adapter = fixture(tmp_path)
    adapter.write_text(
        adapter.read_text(encoding="utf-8").replace("'row_bytes':2*bpp", "'row_bytes':1"),
        encoding="utf-8",
    )
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode != 0
    failure = json.loads((output / "diagnostics" / "failure.json").read_text())
    assert failure["stage"] == "validate_report"
    assert failure["report_written"] is False
    assert failure["report_status"] == "not_a_report"
    assert not (output / "report.json").exists()


def test_captured_oracle_is_compared_per_depth(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    oracle_root = manifest_path.parent / "oracle"
    oracle_root.mkdir()
    oracle_artifacts = {}
    for depth, bytes_per_pixel in (("argb8", 4), ("argb16", 8)):
        oracle = oracle_root / f"{depth}.raw"
        oracle.write_bytes(b"o" * (4 * bytes_per_pixel))
        oracle_artifacts[depth] = identity(oracle, f"oracle/{depth}.raw")
    manifest["oracle"] = {
        "state": "captured",
        "identity_match": True,
        "artifacts": oracle_artifacts,
    }
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    output = tmp_path / "bundle"
    assert invoke(manifest_path, output, adapter).returncode == 0
    report = json.loads((output / "report.json").read_text())
    assert {item["oracle"]["state"] for item in report["results"]} == {"captured"}
    assert {item["oracle"]["exact"] for item in report["results"]} == {True}


def test_source_has_no_host_reimplementation():
    source = RUNNER.read_text(encoding="utf-8")
    assert "DEPTH_COMMANDS" in source
    assert "subprocess.Popen" in source
    assert "capture_output" not in source
    assert "MAX_DIAGNOSTIC_BYTES" in source
    assert "LoadLibrary" not in source


def test_harness_exposes_depth_variants_of_typed_request_cli():
    source = (ROOT / "broker" / "crates" / "harness" / "src" / "main.rs").read_text()
    for flag in (
        '"--render-experimental-smart-request"',
        '"--render-experimental-smart-request-16"',
        '"--render-experimental-smart-request-32-cpu"',
    ):
        assert flag in source
    assert "let pixel_format = if command.contains(\"-16\")" in source
