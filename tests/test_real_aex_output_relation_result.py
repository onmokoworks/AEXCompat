import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "REAL_AEX_OUTPUT_RELATION_RESULT_2026-07-15.json"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"


def test_real_effect_matrix_distinguishes_changed_and_passthrough_outputs():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    cmyk = result["fixtures"]["CMYKMisreg"]
    assert cmyk["applicable_cases"] == cmyk["pixels_changed_cases"] == 6
    assert cmyk["differing_pixels_per_case"] == 37 * 23
    path = result["fixtures"]["PathArray"]
    assert path["classic_argb8"] == {
        "output_relation": "pixels_changed",
        "differing_input_pixels": 37 * 23,
    }
    assert path["smartfx_argb8"] == {
        "output_relation": "pixel_exact_passthrough",
        "differing_input_pixels": 0,
    }
    assert all(result["policy"].values())


def test_output_relation_is_computed_and_exposed_without_redefining_pass():
    source = HARNESS.read_text(encoding="utf-8")
    assert 'case["output_relation"]' in source
    assert '"pixel_exact_passthrough"' in source
    assert '"pixels_changed"' in source
    assert '"different_dimensions"' in source
    assert 'case["differing_input_pixels"]' in source
    assert 'format!("pixels changed: {count}")' in source
