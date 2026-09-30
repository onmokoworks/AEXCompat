import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"


@pytest.mark.parametrize("kind", ("discovery", "classic", "smart"))
def test_native_pipl_parser_self_test_passes_all_effect_workers(kind):
    worker = BUILD / "aex_worker.exe"
    if not worker.exists():
        pytest.skip("build the worker into target/minihost-build before running this native test")
    expected = {
        "pipl_entrypoint": "passed",
        "kind_discriminator": True,
        "code_win64_x86": True,
        "bounded": True,
        "aegp_not_effect": True,
    }
    completed = subprocess.run(
        [str(worker), "--kind", kind, "--self-test-pipl-entrypoint"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stdout) == expected


@pytest.mark.parametrize("kind", ("discovery", "classic", "smart"))
def test_native_plugin_data_self_test_passes_all_effect_workers(kind):
    worker = BUILD / "aex_worker.exe"
    if not worker.exists():
        pytest.skip("build the worker into target/minihost-build before running this native test")
    expected = {
        "plugin_data_entrypoint": "passed",
        "v2": True,
        "v1_fallback": True,
        "multi_effect": True,
        "bounded": True,
        "fail_closed": True,
    }
    completed = subprocess.run(
        [str(worker), "--kind", kind, "--self-test-plugin-data-entrypoint"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stdout) == expected
