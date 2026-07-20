import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis/SDK_COLORGRID_ARBITRARY_TIMELINE_RESULT_2026-07-17.json"


def _load():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_timeline_artifacts_are_authenticated():
    artifacts = _load()["authenticated_artifacts"]
    for artifact in artifacts.values():
        path = ROOT / artifact["path"]
        assert path.is_file(), artifact["path"]
        assert path.stat().st_size == artifact["size_bytes"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == artifact["sha256"]


def test_midpoint_is_distinct_balanced_and_exception_free():
    result = _load()
    assert result["result"] == "two_key_arbitrary_timeline_rendered_with_balanced_ownership"
    left, midpoint, right = result["runs"]
    assert result["timeline"]["worker_argv_time_scale"] == 24
    assert len({run["internal_output_sha256"] for run in result["runs"]}) == 3
    assert len({run["file_output_sha256"] for run in result["runs"]}) == 3
    assert midpoint["arbitrary_interpolation_amount"] == 0.5
    assert midpoint["arbitrary_interpolation_calls"] == 1
    assert midpoint["arbitrary_new_calls"] == 1
    for run in (left, midpoint, right):
        assert run["exit_code"] == 0
        assert run["status"] == "render_completed"
        assert run["handles_created"] == run["handles_disposed"]
        assert run["handle_lifetimes_balanced"] is True
        assert run["exception_code"] == 0
    assert left["handles_created"] == right["handles_created"] == 5
    assert midpoint["handles_created"] == 7


def test_midpoint_stress_reproduced_one_safe_interpolated_signature():
    stress = _load()["midpoint_stress"]
    assert stress["iterations"] >= 20
    assert stress["successful_iterations"] == stress["iterations"]
    assert stress["failed_iterations"] == 0
    assert stress["observed_signature_count"] == 1
    assert stress["exit_code"] == 0
    assert stress["status"] == "render_completed"
    assert stress["arbitrary_interpolation_amount"] == 0.5
    assert stress["arbitrary_interpolation_calls"] == 1
    assert stress["arbitrary_new_calls"] == 1
    assert stress["handles_created"] == stress["handles_disposed"] == 7
    assert stress["handle_lifetimes_balanced"] is True
    assert stress["exception_code"] == 0


def test_sidecar_contains_two_distinct_144_byte_keys():
    sidecar = json.loads((ROOT / _load()["authenticated_artifacts"]["sidecar"]["path"]).read_text(encoding="utf-8"))
    keys = sidecar["parameters"][0]["keys"]
    assert sidecar["schema_version"] == 1
    assert sidecar["parameters"][0]["slot"] == 1
    assert len(keys) == 2
    assert len(keys[0]["value"]["value"]) == 144
    assert len(keys[1]["value"]["value"]) == 144
    assert keys[0]["value"]["value"] != keys[1]["value"]["value"]
