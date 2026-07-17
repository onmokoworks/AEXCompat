import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_effect_matrix_covers_six_paths_and_continues_after_failures():
    evidence = json.loads(
        (ROOT / "analysis" / "EFFECT_UI_COMPATIBILITY_MATRIX_RESULT_2026-07-15.json").read_text()
    )
    assert len(evidence["cases"]) == 6
    assert evidence["full_compatibility_fixture"]["passed_count"] == 6
    partial = evidence["partial_compatibility_fixture"]
    assert partial["case_count"] == 6
    assert partial["passed_count"] == 3
    assert partial["failed_count"] == 3
    assert partial["classic_cases_passed"] == 3
    assert partial["smartfx_cases_failed"] == 3
    assert partial["smartfx_failure_stage"] == "result_rect_validation"
    assert partial["all_cases_completed_after_first_failure"] is True
    assert partial["failed_cases_created_no_output"] is True
    assert all(evidence["invariants"].values())


def test_matrix_uses_fresh_render_calls_and_bounded_failure_summaries():
    source = (ROOT / "broker" / "crates" / "harness" / "src" / "main.rs").read_text()
    matrix = source[source.index("fn run_effect_matrix") : source.index("fn json_after_marker")]
    assert "for smart in [false, true]" in matrix
    assert "for (pixel_format, format_name) in formats" in matrix
    assert "render_experimental_image_at_time_with_format" in matrix
    assert "matrix_error_summary(&message)" in matrix
    assert "message.chars().take(512).collect()" in source
    assert '"--render-experimental-matrix"' in source
