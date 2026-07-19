import hashlib
import importlib.util
import io
import json
import struct
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
if str(ROOT / "tools") not in sys.path:
    sys.path.insert(0, str(ROOT / "tools"))


def load_runner_module():
    spec = importlib.util.spec_from_file_location("run_conformance_bundle", RUNNER)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


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
            "render_path": "smartfx",
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
        "[p.add_argument(x) for x in ('--depth','--render-path','--runner','--plugin','--input','--output','--request','--world-dump-dir')]\n"
        "a=p.parse_args(); bpp={'argb8':4,'argb16':8,'argb32f':16}[a.depth]\n"
        "data=b'output-'+a.depth.encode(); open(a.output,'wb').write(data)\n"
        "import os; os.makedirs(a.world_dump_dir); open(os.path.join(a.world_dump_dir,'000-smart-input-2x2.raw'),'wb').write(b'i'*(4*bpp)); open(os.path.join(a.world_dump_dir,'001-smart-output-2x2.raw'),'wb').write(b'o'*(4*bpp))\n"
        "w={'width':2,'height':2,'row_bytes':2*bpp,'pixel_format':a.depth,'premultiplication':'straight','extent_hint':{'left':0,'top':0,'right':2,'bottom':2}}\n"
        "print(json.dumps({'depth':a.depth,'classification':'ok','selector':{'render_path':a.render_path,'completed':True,'error_code':0},'input_world':w,'world':w,'raw_input':None,'raw_output':None,'output_sha256':hashlib.sha256(data).hexdigest(),'suite_timeline':[],'parameter_metadata':[{'index':1,'type':'float','initial_value':25,'host_range':{'minimum':0,'maximum':100},'user_range':{'minimum':10,'maximum':90}}],'oracle':{'state':'not_captured','identity_match':False,'exact':False}}))\n",
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
    assert report["parameters"] == [{
        "index": 1,
        "type": "float",
        "initial_value": 25,
        "host_range": {"minimum": 0, "maximum": 100},
        "user_range": {"minimum": 10, "maximum": 90},
    }]
    assert report["results"][0]["raw_input"]["path"].startswith("raw/argb8/")


def test_all_classic_and_smartfx_depths_have_canonical_commands():
    module = load_runner_module()
    assert module.DEPTH_COMMANDS == {
        ("classic", "argb8"): "--render-experimental-request",
        ("classic", "argb16"): "--render-experimental-request-16",
        ("classic", "argb32f"): "--render-experimental-request-32",
        ("smartfx", "argb8"): "--render-experimental-smart-request",
        ("smartfx", "argb16"): "--render-experimental-smart-request-16",
        ("smartfx", "argb32f"): "--render-experimental-smart-request-32-cpu",
    }


def test_render_path_echo_rejects_each_mismatch_and_contradiction():
    module = load_runner_module()
    assert module.reported_render_path_matches({}, "classic")
    assert not module.reported_render_path_matches({"render_path": "smartfx"}, "classic")
    assert not module.reported_render_path_matches(
        {"render_path": "classic", "selector": {"render_path": "smartfx"}}, "classic"
    )
    assert not module.reported_render_path_matches(
        {"render_path": "smartfx", "selector": {"render_path": "classic"}}, "classic"
    )
    assert module.is_crash_exit_code(-11)
    assert module.is_crash_exit_code(0xC0000005)
    assert not module.is_crash_exit_code(1)


@pytest.mark.parametrize("plugin_kind", ["aegp_candidate", "invalid_pipl", "unknown_no_effect_entrypoint"])
def test_plugin_kind_maps_to_loader_error(plugin_kind):
    module = load_runner_module()
    result = module.normalize_structured_failure(
        "argb8",
        {
            "classification": "nonzero_exit",
            "plugin_kind": plugin_kind,
            "missing_suites": [{"name": "PF World Suite", "version": 2}],
        },
        {
            "width": 2,
            "height": 2,
            "row_bytes": 8,
            "pixel_format": "argb8",
            "premultiplication": "straight",
            "extent_hint": {"left": 0, "top": 0, "right": 2, "bottom": 2},
        },
        "classic",
    )
    assert result["classification"] == "loader_error"
    assert result["plugin_kind"] == plugin_kind
    assert result["selector"]["render_path"] == "classic"
    assert "missing_suites" not in result


def test_explicit_loader_error_is_preserved():
    module = load_runner_module()
    result = module.normalize_structured_failure(
        "argb8",
        {"classification": "loader_error"},
        {},
        "classic",
    )
    assert result["classification"] == "loader_error"


def test_manifest_requires_a_valid_render_path(tmp_path):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    del document["execution"]["render_path"]
    manifest.write_text(json.dumps(document), encoding="utf-8")
    assert invoke(manifest, tmp_path / "missing", adapter).returncode != 0
    document["execution"]["render_path"] = "automatic"
    manifest.write_text(json.dumps(document), encoding="utf-8")
    assert invoke(manifest, tmp_path / "invalid", adapter).returncode != 0


@pytest.mark.parametrize("render_path", ["classic", "smartfx"])
def test_adapter_receives_and_reports_requested_render_path(tmp_path, render_path):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    document["execution"]["render_path"] = render_path
    manifest.write_text(json.dumps(document), encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    assert {item["selector"]["render_path"] for item in report["results"]} == {render_path}


@pytest.mark.parametrize(
    "value,scale,accepted",
    [
        (0, 1, True),
        (10_000_000, 1_000_000, True),
        (-1, 30, False),
        (10_000_001, 30, False),
        (0, 1_000_001, False),
    ],
)
def test_manifest_timing_schema_matches_harness_bounds(tmp_path, value, scale, accepted):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    document["execution"]["time"] = {"value": value, "scale": scale}
    manifest.write_text(json.dumps(document), encoding="utf-8")
    completed = invoke(manifest, tmp_path / "bundle", adapter)
    assert (completed.returncode == 0) is accepted


@pytest.mark.parametrize(
    "mutation",
    [
        lambda execution: execution["color_management"].update({"enabled": True}),
        lambda execution: execution["color_management"].update({"working_space": "sRGB"}),
        lambda execution: execution.update({"linear_light": True}),
        lambda execution: execution.update({"renderer": "arbitrary renderer"}),
    ],
)
def test_manifest_rejects_native_render_settings_the_harness_cannot_apply(
    tmp_path, mutation
):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    mutation(document["execution"])
    manifest.write_text(json.dumps(document), encoding="utf-8")
    completed = invoke(manifest, tmp_path / "bundle", adapter)
    assert completed.returncode != 0
    failure = json.loads(
        (tmp_path / "bundle" / "diagnostics" / "failure.json").read_text()
    )
    assert failure["stage"] == "validate_manifest"


@pytest.mark.parametrize("renderer", ["AEXCompat CPU", "software"])
def test_manifest_accepts_each_native_renderer_alias(tmp_path, renderer):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    document["execution"]["renderer"] = renderer
    manifest.write_text(json.dumps(document), encoding="utf-8")
    completed = invoke(manifest, tmp_path / "bundle", adapter)
    assert completed.returncode == 0, completed.stderr


@pytest.mark.parametrize(
    "reserved",
    [
        "manifest.json",
        "report.json",
        "Report.JSON",
        "Report.JSON/payload.bin",
        "manifest.json/child.bin",
        "diagnostics/input.bin",
        "outputs/input.bin",
        "raw/input.bin",
        "requests/input.bin",
        "target/input.bin",
    ],
)
def test_rejects_artifacts_that_collide_with_generated_bundle_paths(tmp_path, reserved):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    # Destination validation must reject reserved output namespaces before the
    # source artifact is resolved or copied.  Some cases intentionally describe
    # a child of the existing manifest.json file and therefore cannot be
    # materialized in the fixture filesystem.
    document["plugin"]["aex"] = {
        "path": reserved,
        "sha256": "0" * 64,
        "size_bytes": 1,
    }
    manifest.write_text(json.dumps(document), encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode != 0
    assert not (output / "report.json").exists()
    failure = json.loads((output / "diagnostics" / "failure.json").read_text())
    assert failure["stage"] == "validate_manifest"
    assert "collides with generated bundle content" in failure["error"]["text"]


@pytest.mark.parametrize("role", ["aex", "dependency", "input", "runner", "oracle"])
def test_reserved_path_check_applies_to_every_artifact_role(tmp_path, role):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    artifact = {"path": "report.json", "sha256": "0" * 64, "size_bytes": 1}
    if role == "aex":
        document["plugin"]["aex"] = artifact
    elif role == "dependency":
        document["plugin"]["dependencies"] = [artifact]
    elif role == "input":
        document["input"] = artifact
    elif role == "runner":
        document["runner"] = artifact
    else:
        document["requested_depths"] = ["argb8"]
        document["oracle"] = {
            "state": "captured",
            "identity_match": True,
            "artifacts": {"argb8": artifact},
        }
    manifest.write_text(json.dumps(document), encoding="utf-8")
    completed = invoke(manifest, tmp_path / "bundle", adapter)
    assert completed.returncode != 0
    failure = json.loads(
        (tmp_path / "bundle" / "diagnostics" / "failure.json").read_text()
    )
    assert "collides with generated bundle content" in failure["error"]["text"]


def test_parameter_metadata_must_match_across_depths(tmp_path):
    manifest, adapter = fixture(tmp_path)
    source = adapter.read_text(encoding="utf-8")
    adapter.write_text(
        source.replace("'initial_value':25", "'initial_value':25 if a.depth=='argb8' else 26"),
        encoding="utf-8",
    )
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode != 0
    assert not (output / "report.json").exists()
    failure = json.loads((output / "diagnostics" / "failure.json").read_text())
    assert "parameter metadata differs" in failure["error"]["text"]


@pytest.mark.parametrize("second_path", ["ARTIFACTS/EFFECT.AEX", "artifacts/effect.aex/child"])
def test_rejects_case_and_file_parent_artifact_aliases(tmp_path, second_path):
    manifest, adapter = fixture(tmp_path)
    document = json.loads(manifest.read_text(encoding="utf-8"))
    document["plugin"]["dependencies"] = [
        {"path": second_path, "sha256": "0" * 64, "size_bytes": 1}
    ]
    manifest.write_text(json.dumps(document), encoding="utf-8")
    completed = invoke(manifest, tmp_path / "bundle", adapter)
    assert completed.returncode != 0
    failure = json.loads(
        (tmp_path / "bundle" / "diagnostics" / "failure.json").read_text()
    )
    assert failure["stage"] == "validate_manifest"
    assert "artifact" in failure["error"]["text"]


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
    assert report["results"][0]["suite_timeline"] is None


def test_structured_failure_uses_meaningful_pre_render_error_over_sentinel(tmp_path):
    manifest, adapter = fixture(tmp_path)
    adapter.write_text(
        "import json; print(json.dumps({'smart_render_selector_error':-1,'pre_render_error':25,'smart_render_error':-1})); raise SystemExit(3)\n",
        encoding="utf-8",
    )
    output = tmp_path / "bundle"
    assert invoke(manifest, output, adapter).returncode == 0
    report = json.loads((output / "report.json").read_text())
    assert {item["classification"] for item in report["results"]} == {"selector_error"}
    assert {item["selector"]["error_code"] for item in report["results"]} == {25}


def test_structured_inspection_failure_preserves_bounded_plugin_kind(tmp_path):
    manifest, adapter = fixture(tmp_path)
    adapter.write_text(
        "import json; print(json.dumps({'classification':'nonzero_exit','exit_code':12,'plugin_kind':'unknown_no_effect_entrypoint'})); raise SystemExit(1)\n",
        encoding="utf-8",
    )
    output = tmp_path / "bundle"
    assert invoke(manifest, output, adapter).returncode == 0
    report = json.loads((output / "report.json").read_text())
    assert {item["plugin_kind"] for item in report["results"]} == {"unknown_no_effect_entrypoint"}


@pytest.mark.parametrize("constant", ["NaN", "Infinity", "-Infinity"])
def test_manifest_rejects_non_finite_json_numbers(tmp_path, constant):
    manifest, adapter = fixture(tmp_path)
    text = manifest.read_text(encoding="utf-8")
    manifest.write_text(text.replace('"value": 50', f'"value": {constant}'), encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode != 0
    assert not (output / "report.json").exists()
    failure = json.loads((output / "diagnostics" / "failure.json").read_text())
    assert failure["stage"] == "load_manifest"
    assert "non-finite JSON number" in failure["error"]["text"]


def test_manifest_rejects_overflowed_nested_json_number(tmp_path):
    manifest, adapter = fixture(tmp_path)
    text = manifest.read_text(encoding="utf-8")
    manifest.write_text(text.replace('"value": 50', '"value": [1, 1e400]'), encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode != 0
    assert not (output / "report.json").exists()
    failure = json.loads((output / "diagnostics" / "failure.json").read_text())
    assert failure["stage"] == "load_manifest"
    assert "non-finite JSON number" in failure["error"]["text"]


def test_layer_parameter_uses_absolute_pinned_bundle_transport(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    del adapter
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["execution"]["parameters"] = [
        {"index": 4, "type": "layer", "value": manifest["input"]["path"]}
    ]
    destination = tmp_path / "bundle" / "requests" / "argb8.json"
    runner = load_runner_module()
    runner.write_request_sidecar(manifest, "argb8", destination)
    request = json.loads(destination.read_text(encoding="utf-8"))
    assert request["assignments"] == [
        {
            "slot": 4,
            "layer": str(
                (tmp_path / "bundle").joinpath(*manifest["input"]["path"].split("/")).absolute()
            ),
        }
    ]


def test_layer_parameter_rejects_unpinned_bundle_path(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    del adapter
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["execution"]["parameters"] = [
        {"index": 4, "type": "layer", "value": "inputs/unpinned.png"}
    ]
    runner = load_runner_module()
    with pytest.raises(ValueError, match="must reference a pinned bundle artifact"):
        runner.write_request_sidecar(
            manifest, "argb8", tmp_path / "bundle" / "requests" / "argb8.json"
        )


def test_arbitrary_data_string_retains_text_transport(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    del adapter
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["execution"]["parameters"] = [
        {"index": 7, "type": "arbitrary_data", "value": "opaque payload"}
    ]
    destination = tmp_path / "bundle" / "requests" / "argb8.json"
    runner = load_runner_module()
    runner.write_request_sidecar(manifest, "argb8", destination)
    request = json.loads(destination.read_text(encoding="utf-8"))
    assert request["assignments"] == [
        {"slot": 7, "text": "opaque payload"}
    ]


@pytest.mark.parametrize(
    "expected_bytes,actual_bytes,mismatched",
    [
        (b"a" * 16, b"a" * 8, 2),
        (b"a" * 16, b"b" * 4 + b"a" * 4, 3),
        (b"a" * 3, b"a" * 4, 1),
        (b"a" * 3, b"a" * 3, 1),
    ],
)
def test_oracle_mismatch_counts_missing_extra_and_partial_pixels(
    tmp_path, expected_bytes, actual_bytes, mismatched
):
    runner = load_runner_module()
    expected = tmp_path / "expected.raw"
    actual = tmp_path / "actual.raw"
    expected.write_bytes(expected_bytes)
    actual.write_bytes(actual_bytes)
    assert runner._mismatched_pixels(expected, actual, "argb8") == mismatched


def test_oracle_comparison_short_read_fails_without_looping():
    runner = load_runner_module()
    with pytest.raises(ValueError, match="changed size during pixel comparison"):
        runner._read_exact_comparison_chunk(io.BytesIO(b"abc"), 4, "oracle")


@pytest.mark.parametrize("size,truncated", [(65535, False), (65536, False), (65537, True)])
def test_stderr_truncation_observes_exact_64k_boundary(tmp_path, size, truncated):
    manifest, adapter = fixture(tmp_path)
    adapter.write_text(
        f"import sys; sys.stderr.buffer.write(b'x'*{size}); raise SystemExit(3)\n",
        encoding="utf-8",
    )
    output = tmp_path / "bundle"
    assert invoke(manifest, output, adapter).returncode == 0
    detail = json.loads((output / "diagnostics" / "run.json").read_text())
    stderr = detail["depths"]["argb8"]["stderr"]
    assert stderr["bytes_kept"] == min(size, 65536)
    assert stderr["truncated"] is truncated


@pytest.mark.parametrize(
    "mode,samples",
    [
        ("premultiplied", bytes((50, 25, 13, 128))),
        ("opaque", bytes((100, 50, 25, 255))),
    ],
)
@pytest.mark.parametrize("depth", ["argb8", "argb16", "argb32f"])
def test_synthesized_raw_input_applies_declared_alpha_mode(tmp_path, mode, samples, depth):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["requested_depths"] = [depth]
    manifest["execution"]["premultiplication"] = mode
    image = manifest_path.parent / manifest["input"]["path"]
    Image.new("RGBA", (1, 1), (100, 50, 25, 128)).save(image)
    manifest["input"] = identity(image, manifest["input"]["path"])
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    adapter.write_text("raise SystemExit(3)\n", encoding="utf-8")

    output = tmp_path / "bundle"
    assert invoke(manifest_path, output, adapter).returncode == 0
    report = json.loads((output / "report.json").read_text())
    result = report["results"][0]
    if depth == "argb8":
        expected = samples
    elif depth == "argb16":
        expected = b"".join(struct.pack("<H", (sample * 32768 + 127) // 255) for sample in samples)
    else:
        expected = b"".join(struct.pack("<f", sample / 255.0) for sample in samples)
    assert (output / result["raw_input"]["path"]).read_bytes() == expected
    assert result["input_world"]["premultiplication"] == mode


def test_structured_nonzero_failure_preserves_protocol_and_missing_suites(tmp_path):
    manifest, adapter = fixture(tmp_path)
    adapter.write_text(
        "import json, sys\n"
        "payload={'classification':'missing_suite','missing_suites':[{'name':'PF World Suite','version':2}],"
        "'suite_timeline':None,'diagnostic_padding':'x'*70000}\n"
        "print(json.dumps(payload)); raise SystemExit(7)\n",
        encoding="utf-8",
    )
    output = tmp_path / "bundle"
    completed = invoke(manifest, output, adapter)
    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    assert {item["classification"] for item in report["results"]} == {"missing_suite"}
    assert report["results"][0]["missing_suites"] == [{"name": "PF World Suite", "version": 2}]
    detail = json.loads((output / "diagnostics" / "run.json").read_text())
    assert detail["depths"]["argb8"]["stdout"]["truncated"] is False
    assert detail["depths"]["argb8"]["stdout"]["bytes_kept"] > 65536


def test_captured_oracle_identity_is_not_derived_from_bytes(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    oracle_root = manifest_path.parent / "oracle"
    oracle_root.mkdir()
    oracle = oracle_root / "argb8.raw"
    oracle.write_bytes(b"o" * 16)
    manifest["requested_depths"] = ["argb8"]
    manifest["oracle"] = {
        "state": "captured",
        "identity_match": False,
        "artifacts": {"argb8": identity(oracle, "oracle/argb8.raw")},
    }
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    output = tmp_path / "bundle"
    assert invoke(manifest_path, output, adapter).returncode == 0
    oracle_result = json.loads((output / "report.json").read_text())["results"][0]["oracle"]
    assert oracle_result["identity_match"] is False
    assert oracle_result["exact"] is False
    assert oracle_result["mismatched_pixels"] == 0


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
    assert request["render_settings"] == {
        "premultiplication": "straight",
        "color_management": {"enabled": False, "working_space": None},
        "linear_light": False,
        "renderer": "AEXCompat CPU",
    }
    run = json.loads((output / "diagnostics" / "run.json").read_text())
    argv = run["depths"]["argb8"]["argv"]
    assert [item["index"] for item in argv] == list(range(len(argv)))
    assert any(item["role"] == "request" for item in argv if item["kind"] == "path")
    assert str(manifest.parent) not in json.dumps(run)


def test_request_sidecar_transports_dependencies_and_boolean_values(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    dependency = manifest_path.parent / "artifacts" / "helper.dll"
    dependency.write_bytes(b"pinned dependency")
    manifest["plugin"]["dependencies"] = [identity(dependency, "artifacts/helper.dll")]
    manifest["execution"]["parameters"] = [
        {"index": 1, "type": "checkbox", "value": True},
        {"index": 2, "type": "checkbox", "value": False},
    ]
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    output = tmp_path / "bundle"
    assert invoke(manifest_path, output, adapter).returncode == 0
    request = json.loads((output / "requests" / "argb8.json").read_text())
    assert request["dependencies"] == manifest["plugin"]["dependencies"]
    assert request["assignments"] == [{"slot": 1, "value": 1}, {"slot": 2, "value": 0}]


@pytest.mark.parametrize("value", [None, [1, None], [True, False]])
def test_manifest_rejects_parameter_values_without_a_typed_transport(tmp_path, value):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["execution"]["parameters"] = [{"index": 1, "type": "unsupported", "value": value}]
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    output = tmp_path / "bundle"
    assert invoke(manifest_path, output, adapter).returncode != 0
    failure = json.loads((output / "diagnostics" / "failure.json").read_text())
    assert failure["stage"] == "validate_manifest"


def test_failed_render_retains_captured_oracle_identity(tmp_path):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    oracle_root = manifest_path.parent / "oracle"
    oracle_root.mkdir()
    oracle = oracle_root / "argb8.raw"
    oracle.write_bytes(b"o" * 16)
    manifest["requested_depths"] = ["argb8"]
    manifest["oracle"] = {
        "state": "captured",
        "identity_match": True,
        "artifacts": {"argb8": identity(oracle, "oracle/argb8.raw")},
    }
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    adapter.write_text("raise SystemExit(3)\n", encoding="utf-8")
    output = tmp_path / "bundle"
    completed = invoke(manifest_path, output, adapter)
    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    report_validator().validate(report)
    assert report["results"][0]["oracle"] == {
        "state": "not_captured",
        "identity_match": True,
        "exact": False,
    }


@pytest.mark.parametrize("adapter_succeeds", [True, False])
def test_not_requested_oracle_state_is_retained_for_every_render_outcome(
    tmp_path, adapter_succeeds
):
    manifest_path, adapter = fixture(tmp_path)
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["requested_depths"] = ["argb8"]
    manifest["oracle"] = {"state": "not_requested", "identity_match": False}
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    if not adapter_succeeds:
        adapter.write_text("raise SystemExit(3)\n", encoding="utf-8")

    output = tmp_path / "bundle"
    completed = invoke(manifest_path, output, adapter)

    assert completed.returncode == 0, completed.stderr
    report = json.loads((output / "report.json").read_text())
    report_validator().validate(report)
    assert report["results"][0]["oracle"] == {
        "state": "not_requested",
        "identity_match": False,
        "exact": False,
    }


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
    assert "MAX_PROTOCOL_BYTES" in source
    assert "LoadLibrary" not in source


def test_harness_exposes_depth_variants_of_typed_request_cli():
    source = (ROOT / "broker" / "crates" / "harness" / "src" / "main.rs").read_text()
    for flag in (
        '"--render-experimental-request"',
        '"--render-experimental-request-16"',
        '"--render-experimental-request-32"',
        '"--render-experimental-smart-request"',
        '"--render-experimental-smart-request-16"',
        '"--render-experimental-smart-request-32-cpu"',
    ):
        assert flag in source
    assert "let pixel_format = if command.contains(\"-16\")" in source
    assert "typed_request_render_settings" in source
    assert "typed_request_dependencies" in source
    assert "inspect_experimental_with_approved_dependencies" in source
    assert "AEXCOMPAT_REPOSITORY_ROOT" in source


def test_native_render_settings_uses_checked_ascii_narrowing():
    source = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
    assert "bool narrow_ascii_checked(const std::wstring& text, std::string& output)" in source
    assert "output.push_back(static_cast<char>(character));" in source
    assert "narrow_ascii_checked(fields[1], premultiplication)" in source
    assert "narrow_ascii_checked(fields[5], renderer)" in source
    assert "std::string(fields[1].begin(), fields[1].end())" not in source
    assert "std::string(fields[5].begin(), fields[5].end())" not in source
