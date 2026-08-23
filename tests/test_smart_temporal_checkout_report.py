import json
import shutil
import subprocess
from pathlib import Path

import pytest

from _compile_cache import compile_and_link


ROOT = Path(__file__).resolve().parents[1]


def test_temporal_checkout_report_keeps_legacy_and_separates_ledgers(tmp_path):
    compiler = shutil.which("c++")
    if compiler is None:
        pytest.skip("c++ compiler is unavailable")
    executable = tmp_path / "temporal-checkout-report-selftest"
    compile_and_link(
        compiler,
        [ROOT / "tests" / "native" / "worker_temporal_checkout_report_selftest.cpp"],
        executable,
        compile_args=("-std=c++17", "-I", str(ROOT / "minihost" / "src")),
    )
    completed = subprocess.run(
        [str(executable)], check=True, text=True, capture_output=True
    )
    report = json.loads(completed.stdout)
    assert report == {
        # Frozen consumers keep seeing the layer-ledger value under the old,
        # historically inaccurate name.
        "rejected_temporal_param_checkouts": 17,
        "rejected_temporal_layer_checkouts": 17,
        "rejected_temporal_parameter_checkouts": 23,
    }
