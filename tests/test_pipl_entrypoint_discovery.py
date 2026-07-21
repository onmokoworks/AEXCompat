import json
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "target" / "minihost-build"


@pytest.mark.parametrize(
    "name", ("aex_l2_worker.exe", "aex_render_worker.exe", "aex_smart_worker.exe")
)
def test_native_pipl_parser_self_test_passes_all_effect_workers(name):
    worker = BUILD / name
    if not worker.exists():
        pytest.skip(f"build {name} into target/minihost-build before running this native test")
    expected = {
        "pipl_entrypoint": "passed",
        "kind_discriminator": True,
        "code_win64_x86": True,
        "bounded": True,
        "aegp_not_effect": True,
    }
    completed = subprocess.run(
        [str(worker), "--self-test-pipl-entrypoint"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert completed.returncode == 0, completed.stderr
    assert json.loads(completed.stdout) == expected
