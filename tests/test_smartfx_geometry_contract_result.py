import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "SMARTFX_GEOMETRY_CONTRACT_RESULT_2026-07-19.json"


def _sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_geometry_evidence_is_authenticated_and_refresh_scripted():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    assert evidence["generated_by"] == "tools/refresh-smartfx-geometry-evidence.ps1"
    for artifact in evidence["artifacts"].values():
        path = ROOT / artifact["path"]
        assert not Path(artifact["path"]).is_absolute()
        assert path.stat().st_size == artifact["size_bytes"]
        assert _sha256(path) == artifact["sha256"]


def test_probe_runs_cover_every_mode_at_every_depth_with_identical_geometry():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    runs = evidence["probe_runs"]
    seen = {(run["command"], run["render_time"]) for run in runs}
    assert seen == {
        (command, mode)
        for command in ("--smart-image", "--smart-image16", "--smart-image32")
        for mode in (0, 1, 2, 3)
    }
    geometry_fields = (
        "result_rect", "max_result_rect", "returns_extra_pixels",
        "result_within_request", "extra_pixels_contract_violation",
        "empty_result_rect", "smart_render_selector_dispatched",
        "width", "height", "pre_render_error", "smart_render_error",
        "result_rects_valid",
    )
    by_mode = {}
    for run in runs:
        assert run["pre_render_error"] == 0
        assert run["smart_render_error"] == 0
        assert run["result_rects_valid"] is True
        key = {field: run[field] for field in geometry_fields}
        by_mode.setdefault(run["render_time"], []).append(key)
    for mode, keys in by_mode.items():
        assert all(key == keys[0] for key in keys), f"mode {mode} differs across depths"
    # The mode table pins the contract: extra-pixels flag admits the overrun,
    # the flagless overrun is diagnosed, and the empty result skips the
    # selector.
    assert by_mode[1][0]["returns_extra_pixels"] is True
    assert by_mode[1][0]["result_within_request"] is False
    assert by_mode[1][0]["extra_pixels_contract_violation"] is False
    assert by_mode[2][0]["returns_extra_pixels"] is False
    assert by_mode[2][0]["extra_pixels_contract_violation"] is True
    assert by_mode[3][0]["empty_result_rect"] is True
    assert by_mode[3][0]["smart_render_selector_dispatched"] is False
    assert by_mode[3][0]["width"] == 0 and by_mode[3][0]["height"] == 0


def test_real_aex_runs_are_depth_consistent_without_fixture_branches():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    runs = evidence["real_aex_runs"]
    assert [run["command"] for run in runs] == [
        "--smart-image", "--smart-image16", "--smart-image32"
    ]
    for run in runs:
        assert run["pre_render_error"] == 0
        assert run["smart_render_error"] == 0
        assert run["result_rects_valid"] is True
        assert run["malformed_checkout_request_count"] == 0
        assert run["empty_checkout_pixel_denial_count"] == 0
    geometry = [
        {field: run[field] for field in (
            "result_rect", "max_result_rect", "returns_extra_pixels",
            "result_within_request", "extra_pixels_contract_violation",
            "empty_result_rect", "width", "height",
        )}
        for run in runs
    ]
    assert all(entry == geometry[0] for entry in geometry)
    assert evidence["assertions"]["ae_process_touched"] is False
    assert all(value for name, value in evidence["assertions"].items()
               if name != "ae_process_touched")
