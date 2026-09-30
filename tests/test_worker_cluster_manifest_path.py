"""Manifest parents may be junctions; file aliases and malformed documents fail."""

import json
import os
import subprocess

import pytest

from _native_selftest import locate


def write_manifest(folder):
    document = {
        "schema": "cluster-manifest-v2",
        "plugins": [{"path": str(folder / "fixture.aex"), "sha256": "a" * 64}],
        "search_dirs": [str(folder)],
        "module_bound": 64,
    }
    path = folder / "cluster-manifest.json"
    path.write_text(json.dumps(document), encoding="utf-8")
    return path, document


def inspect(path):
    return subprocess.run(
        [str(locate("worker_cluster_manifest_path_probe.exe")), str(path)],
        capture_output=True, text=True, timeout=30, check=False,
    )


@pytest.mark.skipif(os.name != "nt", reason="Windows junction path contract")
def test_cluster_manifest_accepts_junction_parent_and_validates_document(tmp_path):
    physical = tmp_path / "physical"
    physical.mkdir()
    path, document = write_manifest(physical)
    direct = inspect(path)
    assert direct.returncode == 0, direct.stderr
    assert json.loads(direct.stdout) == {"plugins": 1, "search_dirs": 1, "module_bound": 64}
    junction = tmp_path / "target"
    created = subprocess.run(
        ["cmd", "/c", "mklink", "/J", str(junction), str(physical)],
        capture_output=True, text=True, check=False,
    )
    assert created.returncode == 0, created.stderr
    try:
        through_junction = inspect(junction / path.name)
        assert through_junction.returncode == 0, through_junction.stderr
        assert json.loads(through_junction.stdout) == json.loads(direct.stdout)
        document["module_bound"] = 0
        path.write_text(json.dumps(document), encoding="utf-8")
        assert inspect(junction / path.name).returncode == 3
        path.write_text('{"schema":"cluster-manifest-v2","schema":"duplicate"}', encoding="utf-8")
        assert inspect(junction / path.name).returncode == 3
        path.write_text("", encoding="utf-8")
        assert inspect(junction / path.name).returncode == 3
    finally:
        junction.rmdir()


@pytest.mark.skipif(os.name != "nt", reason="Windows junction path contract")
def test_cluster_manifest_rejects_leaf_alias(tmp_path):
    physical = tmp_path / "physical"
    physical.mkdir()
    manifest, _ = write_manifest(physical)
    alias = tmp_path / manifest.name
    try:
        alias.symlink_to(manifest)
    except OSError:
        pytest.skip("Windows file symlink privilege unavailable")
    assert inspect(alias).returncode == 3
    renamed = physical / "manifest-alias.json"
    renamed.symlink_to(manifest)
    assert inspect(renamed).returncode == 3
