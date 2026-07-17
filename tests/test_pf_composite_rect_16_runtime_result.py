import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "PF_COMPOSITE_RECT_16_RUNTIME_RESULT_2026-07-16.json"


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _load_result() -> dict:
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_evidence_authenticates_current_source_worker_probe_and_input():
    result = _load_result()
    assert result["result"] == "runtime_render_succeeded"
    for artifact in result["authenticated_artifacts"].values():
        path = ROOT / artifact["path"]
        assert path.is_file()
        assert path.stat().st_size == artifact["size_bytes"]
        assert _sha256(path) == artifact["sha256"]


def test_runtime_reports_and_outputs_authenticate_success_and_balanced_ownership():
    result = _load_result()
    assert [run["pixel_format"] for run in result["runs"]] == ["argb16"]
    for run in result["runs"]:
        report_path = ROOT / run["report"]["path"]
        assert report_path.stat().st_size == run["report"]["size_bytes"]
        assert _sha256(report_path) == run["report"]["sha256"]
        report = json.loads(report_path.read_text(encoding="utf-8"))
        assert report["pixel_format"] == run["pixel_format"]
        assert run["exit_code"] == 0
        assert report["status"] == run["status"] == "render_completed"
        assert report["render_error"] == run["render_error"] == 0
        assert report["input_sha256"] == run["report_input_sha256"]
        assert report["output_sha256"] == run["report_internal_output_sha256"]
        assert report["width"] == run["width"] == 37
        assert report["height"] == run["height"] == 23
        assert report["rowbytes"] == run["rowbytes"] == 296
        assert report["bytes_written_per_row"] == run["bytes_written_per_row"] == 296
        for field in (
            "guard_bytes_intact",
            "suite_leases_balanced",
            "handle_lifetimes_balanced",
            "world_lifetimes_balanced",
            "param_checkouts_balanced",
        ):
            assert report[field] is run[field] is True
        assert report["suite_acquires"] == run["suite_acquires"] == 2
        assert report["suite_releases"] == run["suite_releases"] == 2
        assert report["live_suite_lease_count"] == run["live_suite_lease_count"] == 0
        assert report["worlds_created"] == run["worlds_created"] == 1
        assert report["worlds_disposed"] == run["worlds_disposed"] == 1
        output = run["external_output"]
        output_path = ROOT / output["path"]
        assert output_path.stat().st_size == output["size_bytes"]
        assert _sha256(output_path) == output["sha256"]


def test_scope_does_not_overclaim_pixel_compatibility():
    result = _load_result()
    assert result["pending"]["argb32f"].startswith("pending")
    assert result["pending"]["real_after_effects_pixel_oracle"] == "pending"
    assert result["pending"]["exact_adobe_pixel_equivalence"].startswith("not claimed")
    assert "ARGB16" in result["claim"]
    assert "intact guards" in result["claim"]
