import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_SHIFTER_TRANSFORM_SAMPLING_RESULT_2026-07-15.json"
def test_shifter_covers_classic_transform_and_smart_sampling_at_all_depths():
    result = json.loads(RESULT.read_text(encoding="utf-8"))
    classic = result["classic"]
    smart = result["smart"]

    assert result["result"] == "classic_and_smart_render_completed"
    assert classic["pixel_formats"] == ["argb8", "argb16", "argb32f"]
    assert classic["zero_translation_rgba_oracle_exact"] is True
    assert classic["translation_test"] == [1, 1]
    assert smart["pixel_formats"] == ["argb8", "argb16", "argb32f"]
    assert smart["argb8_sampling_blend_oracle_exact"] is True
    assert smart["param_checkout_calls"] == smart["automatic_param_checkins"] == 2
    assert smart["automatic_pre_render_handle_disposals"] == 1
    for section in (classic, smart):
        assert section["suite_leases_balanced"] is True
        assert section["handle_lifetimes_balanced"] is True
        assert section["world_lifetimes_balanced"] is True

