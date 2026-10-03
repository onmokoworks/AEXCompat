import hashlib
import json
import math
import os
import struct
import subprocess
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]


def worker():
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target/minihost-build-v18/Release/aex_worker.exe",
        ROOT / "target/minihost-build-v18/aex_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def write_transport(folder: Path, values=(0.0, 0.25, 1.0, 2.0), **sample_updates):
    raw = folder / "aux-1-0-0.f32le"
    payload = b"".join(struct.pack("<f", value) for value in values)
    raw.write_bytes(payload)
    sample = {
        "time": 0,
        "time_scale": 30,
        "path": str(raw.resolve()),
        "sampling": "hold",
        "interpretation": "depth",
        "expected_byte_length": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }
    sample.update(sample_updates)
    manifest = {
        "schema": "aux-manifest-v1",
        "nonce": "1",
        "channels": [{
            "param_index": 0,
            "type": 0x44505448,
            "name": "Depth",
            "data_type": "f32le",
            "dimension": 1,
            "width": 2,
            "height": 2,
            "samples": [sample],
        }],
    }
    path = folder / "aux-manifest-1.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    return path, manifest


def run_transport(path: Path, *, preserve_path=False):
    executable = worker()
    assert executable is not None, "build aex_worker.exe (pwsh -File tools/build-native.ps1) before running transport tests"
    return subprocess.run(
        [str(executable), "--kind", "classic", "--self-test-pf-ae-channel-transport", "--aux-manifest-v1", str(path if preserve_path else path.resolve())],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=30,
        check=False,
    )


def test_real_aux_manifest_exact_hold_handle_roundtrip(tmp_path):
    manifest, _ = write_transport(tmp_path)
    completed = run_transport(manifest)
    assert completed.returncode == 0, completed.stderr or completed.stdout
    report = json.loads(completed.stdout)
    assert report["pf_ae_channel_transport"] == "passed"
    assert report["row_bytes"] == 8
    assert report["duration"] == 1


def test_aux_manifest_rejects_hash_schema_and_unknown_sampling(tmp_path):
    manifest, value = write_transport(tmp_path)
    value["channels"][0]["samples"][0]["sha256"] = "0" * 64
    manifest.write_text(json.dumps(value), encoding="utf-8")
    assert run_transport(manifest).returncode != 0

    _, value = write_transport(tmp_path)
    value["channels"][0]["samples"][0]["sampling"] = "nearest"
    manifest.write_text(json.dumps(value), encoding="utf-8")
    assert run_transport(manifest).returncode != 0

    _, value = write_transport(tmp_path)
    value["unexpected"] = True
    manifest.write_text(json.dumps(value), encoding="utf-8")
    assert run_transport(manifest).returncode != 0


def test_aux_manifest_rejects_nonfinite_and_sidecar_outside_manifest_folder(tmp_path):
    manifest, _ = write_transport(tmp_path, values=(0.0, math.nan, 1.0, 2.0))
    assert run_transport(manifest).returncode != 0

    manifest_dir = tmp_path / "manifest"
    manifest_dir.mkdir()
    manifest, value = write_transport(manifest_dir)
    outside = tmp_path / "outside.f32le"
    outside.write_bytes((manifest_dir / "aux-1-0-0.f32le").read_bytes())
    value["channels"][0]["samples"][0]["path"] = str(outside.resolve())
    manifest.write_text(json.dumps(value), encoding="utf-8")
    assert run_transport(manifest).returncode != 0


@pytest.mark.skipif(os.name != "nt", reason="Windows junction path contract")
def test_aux_manifest_accepts_junction_parent_but_rejects_other_folder(tmp_path):
    physical = tmp_path / "physical-target"
    physical.mkdir()
    junction = tmp_path / "target"
    created = subprocess.run(
        ["cmd", "/c", "mklink", "/J", str(junction), str(physical)],
        text=True, capture_output=True, check=False,
    )
    assert created.returncode == 0, created.stderr or created.stdout
    try:
        manifest, value = write_transport(physical)
        value["channels"][0]["samples"][0]["path"] = str(junction / "aux-1-0-0.f32le")
        manifest.write_text(json.dumps(value), encoding="utf-8")
        accepted = run_transport(junction / manifest.name, preserve_path=True)
        assert accepted.returncode == 0, accepted.stderr or accepted.stdout
        assert json.loads(accepted.stdout)["pf_ae_channel_transport"] == "passed"

        manifest_alias = physical / "manifest-alias.json"
        try:
            manifest_alias.symlink_to(manifest)
        except OSError:
            pass  # Symlink creation is privilege-dependent on Windows.
        else:
            rejected_alias = run_transport(junction / manifest_alias.name, preserve_path=True)
            assert rejected_alias.returncode == 3, rejected_alias.stderr or rejected_alias.stdout

        outside = tmp_path / "outside.f32le"
        outside.write_bytes((physical / "aux-1-0-0.f32le").read_bytes())
        value["channels"][0]["samples"][0]["path"] = str(outside)
        manifest.write_text(json.dumps(value), encoding="utf-8")
        rejected = run_transport(junction / manifest.name, preserve_path=True)
        assert rejected.returncode == 3, rejected.stderr or rejected.stdout
    finally:
        junction.rmdir()




def test_rich_manifest_accepts_padded_negative_stride_and_preserves_logical_rows(tmp_path):
    # Physical rows are bottom-up and include one float of padding per row.
    physical = (3.0, 4.0, 99.0, 1.0, 2.0, 99.0)
    manifest, value = write_transport(tmp_path, values=physical)
    raw = tmp_path / "aux-1-0-0.f32le"
    payload = raw.read_bytes()
    channel = value["channels"][0]
    channel.update({
        "width": 2,
        "height": 2,
        "row_bytes": -12,
        "origin_x": -3,
        "origin_y": 7,
        "downsample_x_num": 1,
        "downsample_x_den": 2,
        "downsample_y_num": 2,
        "downsample_y_den": 3,
        "coordinate_space": "source_pixel",
        "units": "scene_depth",
    })
    sample = channel["samples"][0]
    sample["expected_byte_length"] = len(payload)
    sample["sha256"] = hashlib.sha256(payload).hexdigest()
    manifest.write_text(json.dumps(value), encoding="utf-8")
    completed = run_transport(manifest)
    assert completed.returncode == 0, completed.stderr or completed.stdout
    report = json.loads(completed.stdout)
    assert report["row_bytes"] == -12
    assert report["origin"] == [-3, 7]
    assert report["duration"] == 1
