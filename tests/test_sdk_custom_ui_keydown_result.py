import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_sdk_custom_ui_keydown_uses_measured_abi_and_complete_lifecycle() -> None:
    result = json.loads(
        (ROOT / "analysis" / "SDK_CUSTOM_UI_KEYDOWN_RESULT_2026-07-15.json").read_text(
            encoding="utf-8"
        )
    )

    assert result["abi"]["size"] == 20
    assert result["abi"]["offsets"] == {
        "when": 0,
        "screen_point": 4,
        "keycode": 12,
        "modifiers": 16,
    }
    assert result["input"]["keycode"] == 0x80000041
    for observation in result["observations"]:
        assert observation["lifecycle_errors"] == [0, 0, 0, 0, 0]
        assert observation["context_stable"]
        assert observation["host_state_cleared_after_close"]
    assert all(result["invariants"].values())
