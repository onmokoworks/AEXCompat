import json
import os
import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def source_text():
    return SOURCE.read_text(encoding="utf-8")


def worker(name):
    configured = os.environ.get(f"AEXCOMPAT_{name.upper()}_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / f"aex_{name}_worker.exe",
        ROOT / "target" / "minihost-build-v18" / f"aex_{name}_worker.exe",
    ]
    return next((path for path in candidates if path and path.is_file()), None)


def test_legacy_fill_callbacks_match_sdk_slots_and_reuse_suite_v2_implementations():
    text = source_text()
    expected_offsets = {
        "kUtilsFill": (9, "fill_world8"),
        "kUtilsPremultiply": (12, "premultiply_world8"),
        "kUtilsPremultiplyColor": (13, "premultiply_color8"),
        "kUtilsFill16": (61, "fill_world16"),
        "kUtilsPremultiplyColor16": (62, "premultiply_color16"),
    }
    for constant, (slot, callback) in expected_offsets.items():
        assert f"static_assert({constant} == {slot} * sizeof(void*));" in text
        assert f"write(utils, {constant}, &{callback});" in text
    assert "static_assert(kUtilsSize == 69 * sizeof(void*));" in text
    assert re.search(
        r"g_fill_matte_suite2\[0\].*?&fill_world8.*?"
        r"g_fill_matte_suite2\[1\].*?&fill_world16.*?"
        r"g_fill_matte_suite2\[4\].*?&premultiply_color8.*?"
        r"g_fill_matte_suite2\[5\].*?&premultiply_color16",
        text,
        re.DOTALL,
    )
    assert "wire_legacy_fill_matte_callbacks(utils);" in text


def test_legacy_fill_native_guards_errors_and_non_null_callbacks_in_both_workers():
    for name in ("render", "smart"):
        executable = worker(name)
        assert executable is not None, f"build aex_{name}_worker before running this test"
        completed = subprocess.run(
            [str(executable), "--self-test-pf-fill-matte-legacy"],
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
        assert completed.returncode == 0, completed.stderr or completed.stdout
        assert json.loads(completed.stdout) == {
            "pf_fill_matte_legacy_callbacks": "passed"
        }
