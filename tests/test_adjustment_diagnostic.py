import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from PIL import Image
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
RUNNER = ROOT / "tools" / "run-adjustment-diagnostic.py"


def _identity(path: Path, relative: str):
    data = path.read_bytes()
    return {"path": relative, "sha256": hashlib.sha256(data).hexdigest(), "size_bytes": len(data)}


def _fixture(tmp_path: Path):
    source = tmp_path / "fixture"
    (source / "artifacts").mkdir(parents=True)
    (source / "inputs").mkdir()
    (source / "runner").mkdir()
    (source / "artifacts" / "independent-fixture.aex").write_bytes(b"independent-aex-fixture")
    (source / "runner" / "harness.exe").write_bytes(b"adapter-runner-placeholder")
    Image.new("RGBA", (4, 3), (40, 80, 120, 255)).save(source / "inputs" / "opaque.png")
    alpha = Image.new("RGBA", (4, 3), (0, 0, 0, 0))
    alpha.putpixel((1, 1), (240, 10, 20, 180))
    alpha.putpixel((2, 1), (20, 220, 30, 220))
    alpha.save(source / "inputs" / "alpha.png")
    adapter = tmp_path / "adapter.py"
    adapter.write_text(
        "import argparse, hashlib, json, os, struct\n"
        "from pathlib import Path\n"
        "from PIL import Image\n"
        "p=argparse.ArgumentParser()\n"
        "[p.add_argument(x) for x in ('--depth','--render-path','--runner','--plugin','--input','--output','--request','--world-dump-dir')]\n"
        "a=p.parse_args(); bpp={'argb8':4,'argb16':8,'argb32f':16}[a.depth]\n"
        "mode=os.environ['AEXCOMPAT_ADJUSTMENT_APPLICATION']; scene=json.loads(Path(os.environ['AEXCOMPAT_ADJUSTMENT_SCENE']).read_text())['scene_id']\n"
        "with Image.open(a.input) as im: image=im.convert('RGBA'); input_image=image.copy(); image.save(a.output, format='PNG'); w,h=image.size\n"
        "if mode == 'adjustment' and scene == 'alpha_extent_roi': image.putpixel((0,0),(1,2,3,255)); image.save(a.output, format='PNG')\n"
        "raw=bytearray(input_image.tobytes()); [raw.__setitem__(offset+channel,(raw[offset+channel]*raw[offset+3]+127)//255) for offset in range(0,len(raw),4) for channel in range(3)]\n"
        "encoded=bytes(raw) if a.depth == 'argb8' else b''.join(int((x*32768+127)//255).to_bytes(2,'little') for x in raw) if a.depth == 'argb16' else b''.join(struct.pack('<f',x/255.0) for x in raw)\n"
        "Path(a.world_dump_dir).mkdir(parents=True, exist_ok=True); Path(a.world_dump_dir, '000-input.raw').write_bytes(encoded); Path(a.world_dump_dir, '001-output.raw').write_bytes((b'd' if mode == 'direct' or scene == 'opaque_full_frame' else b'a')*(w*h*bpp))\n"
        "world={'width':w,'height':h,'row_bytes':w*bpp,'pixel_format':a.depth,'premultiplication':'premultiplied','extent_hint':{'left':0,'top':0,'right':w,'bottom':h}}\n"
        "print(json.dumps({'depth':a.depth,'classification':'ok','selector':{'render_path':a.render_path,'completed':True,'error_code':0},'input_world':world,'world':world,'raw_input':None,'raw_output':None,'output_sha256':hashlib.sha256(Path(a.output).read_bytes()).hexdigest(),'suite_timeline':[],'parameter_metadata':[],'oracle':{'state':'not_captured','identity_match':False,'exact':False}}))\n",
        encoding="utf-8",
    )
    manifest = {
        "schema_version": 1,
        "diagnostic_id": "fixture-adjustment-diagnostic",
        "plugin": {"aex": _identity(source / "artifacts" / "independent-fixture.aex", "artifacts/independent-fixture.aex"), "dependencies": []},
        "runner": _identity(source / "runner" / "harness.exe", "runner/harness.exe"),
        "requested_depths": ["argb8", "argb16"],
        "execution": {
            "render_path": "smartfx", "time": {"value": 3, "scale": 30}, "parameters": [{"index": 1, "type": "slider", "value": 50}],
            "resolution": {"width": 4, "height": 3}, "downsample": {"x": 1, "y": 1}, "premultiplication": "premultiplied", "alpha_policy": "premultiplied",
            "color_management": {"enabled": False, "working_space": None}, "linear_light": False, "renderer": "software",
        },
        "sources": [
            {"source_id": "opaque", "artifact": _identity(source / "inputs" / "opaque.png", "inputs/opaque.png"), "role": "opaque_full_frame"},
            {"source_id": "alpha", "artifact": _identity(source / "inputs" / "alpha.png", "inputs/alpha.png"), "role": "alpha_material"},
        ],
    }
    manifest_path = source / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return manifest_path, adapter


def _report_validator():
    report = json.loads((ROOT / "schemas" / "adjustment-diagnostic-report.schema.json").read_text(encoding="utf-8"))
    conformance = json.loads((ROOT / "schemas" / "conformance-manifest.schema.json").read_text(encoding="utf-8"))
    registry = Registry().with_resource(conformance["$id"], Resource.from_contents(conformance))
    return Draft202012Validator(report, registry=registry)


def test_pairwise_adapter_runs_two_standard_scenes_and_preserves_pixel_diff(tmp_path):
    manifest, adapter = _fixture(tmp_path)
    output = tmp_path / "diagnostic"
    result = subprocess.run([sys.executable, str(RUNNER), "--manifest", str(manifest), "--out", str(output), "--adapter-command", str(adapter)], cwd=ROOT, capture_output=True, text=True, env={**os.environ, "PYTHONUTF8": "1"})
    assert result.returncode == 3, result.stderr
    report = json.loads((output / "report.json").read_text(encoding="utf-8"))
    _report_validator().validate(report)
    assert report["execution_evidence"] == "adapter"
    assert [scene["scene_id"] for scene in report["scenes"]] == ["opaque_full_frame", "alpha_extent_roi"]
    assert report["pairs"][0]["classification"] == "equivalent"
    assert report["pairs"][1]["classification"] == "pixel_diverged"
    assert "composite_input" in report["pairs"][1]["cause_candidates"]
    assert report["pairs"][1]["diff"]["depths"][0]["mismatched_pixels"] > 0
    assert report["pairs"][1]["layer_stacks"]["adjustment"][-1]["kind"] == "adjustment"
    assert all(item["identity_match"] for item in report["pairs"][1]["composite_input"]["raw_inputs"])
    assert report["pairs"][0]["direct"]["diagnostics"]["session"]["application_mode"].endswith("-direct")
    assert report["pairs"][0]["direct"]["results"][0]["raw_input"]["path"].startswith("pairs/opaque_full_frame-direct/")
    assert (output / "inputs" / "alpha_extent_roi.png").is_file()


def test_missing_scene_capable_external_is_blocked_and_not_success(tmp_path):
    manifest, _ = _fixture(tmp_path)
    output = tmp_path / "blocked"
    result = subprocess.run([sys.executable, str(RUNNER), "--manifest", str(manifest), "--out", str(output)], cwd=ROOT, capture_output=True, text=True, env={**os.environ, "PYTHONUTF8": "1"})
    assert result.returncode == 3
    report = json.loads((output / "report.json").read_text(encoding="utf-8"))
    _report_validator().validate(report)
    assert report["status"] == "blocked_external"
    assert report["execution_evidence"] == "none"
    assert all(pair["classification"] == "blocked_external" for pair in report["pairs"])
    assert all(pair["classification"] != "equivalent" for pair in report["pairs"])


def test_source_contract_reuses_issue4_runner_and_fail_closed_labels():
    source = RUNNER.read_text(encoding="utf-8")
    assert 'CONFORMANCE_RUNNER = ROOT / "tools" / "run-conformance-bundle.py"' in source
    assert '"--allow-failures"' in source
    assert 'AEXCOMPAT_ADJUSTMENT_APPLICATION' in source
    assert '"blocked_external"' in source
    assert 'execution_evidence' in source
