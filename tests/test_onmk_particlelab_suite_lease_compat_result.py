import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
EVIDENCE = ROOT / "analysis" / "ONMK_PARTICLELAB_SUITE_LEASE_COMPAT_RESULT_2026-07-15.json"
SOURCE = source_owners.L2_MAIN
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"


def test_particlelab_valid_image_is_preserved_with_suite_warning():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    observation = evidence["observation"]
    assert observation["pre_render_error"] == 0
    assert observation["smart_render_error"] == 0
    assert observation["output_dimensions"] == [1848, 1848]
    assert observation["suite_acquires"] == observation["suite_releases"] + 1
    assert observation["suite_lease_warning"] is True
    assert observation["passed"] is True


def test_ownership_and_guard_failures_remain_hard_failures():
    observation = json.loads(EVIDENCE.read_text(encoding="utf-8"))["observation"]
    assert observation["guard_bytes_intact"] is True
    assert observation["handle_lifetimes_balanced"] is True
    assert observation["world_lifetimes_balanced"] is True
    assert observation["param_checkouts_balanced"] is True
    source = source_owners.worker_text()
    assert "smart.guards_intact && handle_lifetimes_balanced()" in source
    assert "world_lifetimes_balanced() &&" in source
    assert "param_checkouts_balanced() &&" in source
    assert "(!g_render_click_enabled && !g_render_draw_enabled)" in source
    assert "g_render_ui_context_closed) ? 0 : 22" in source


def test_suite_warning_is_forwarded_to_the_ui_report():
    broker = BROKER.read_text(encoding="utf-8")
    for field in (
        "suite_lease_warning",
        "suite_leases_balanced",
        "live_suite_leases",
        "handle_lifetimes_balanced",
        "world_lifetimes_balanced",
        "param_checkouts_balanced",
    ):
        assert f'"{field}": worker_report.get("{field}")' in broker


def test_classic_and_smartfx_deep_color_matrix_is_fixed():
    evidence = json.loads(EVIDENCE.read_text(encoding="utf-8"))
    matrix = {
        (entry["render_path"], entry["pixel_format"]): entry
        for entry in evidence["depth_matrix"]
    }
    assert set(matrix) == {
        ("classic", "argb16"),
        ("classic", "argb32f"),
        ("smartfx", "argb16"),
        ("smartfx", "argb32f"),
    }
    assert matrix[("classic", "argb16")]["output_dimensions"] == [37, 23]
    assert matrix[("classic", "argb32f")]["output_dimensions"] == [37, 23]
    assert matrix[("smartfx", "argb16")]["output_dimensions"] == [1848, 1848]
    assert matrix[("smartfx", "argb32f")]["output_dimensions"] == [1848, 1848]
    assert all(entry["passed"] for entry in matrix.values())
