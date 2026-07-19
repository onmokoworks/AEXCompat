import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "tests" / "fixtures" / "ntsc_rs_pr51_runtime_summary.json"


def test_ntsc_rs_three_depth_smartfx_auto_contract():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    results = evidence["smartfx_auto"]["results"]
    assert [item["depth"] for item in results] == ["argb8", "argb16", "argb32f"]
    assert all(item["classification"] == "ok" for item in results)
    assert all(item["timeline_events"] == 25 for item in results)


def test_ntsc_rs_classic_only_selector_failure_keeps_timeline():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    for result in evidence["classic_only"]["results"]:
        assert result["classification"] == "selector_error"
        assert result["error_code"] == 516
        assert result["timeline_events"] == 17


def test_ntsc_rs_world_metadata_is_full_hd_and_bounded():
    world = json.loads(EVIDENCE.read_text(encoding="utf-8"))["world"]
    assert (world["width"], world["height"]) == (1920, 1080)
    assert world["extent_hint"] == {"left": 0, "top": 0, "right": 1920, "bottom": 1080}
    assert world["premultiplication"] == "premultiplied"
