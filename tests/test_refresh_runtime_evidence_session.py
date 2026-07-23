from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_runtime_refresh_routes_removed_one_shot_commands_through_session_adapter():
    source = (ROOT / "tools" / "refresh-runtime-evidence.ps1").read_text(encoding="utf-8")
    assert "refresh-runtime-session.py" in source
    assert "Ensure-SessionHarness" in source
    assert "Invoke-LegacySessionRender" in source
    assert "--plugin-sha256" in source
    assert "unsupported" in source.lower()


def test_session_adapter_preserves_raw_depth_and_supported_harness_contract():
    source = (ROOT / "tools" / "refresh-runtime-session.py").read_text(encoding="utf-8")
    assert '"--render-experimental-session"' in source
    assert '"argb16": ".rgba16le"' in source
    assert '"argb32f": ".rgba32f-le"' in source
    assert "Image.frombytes" in source
    assert "plugin-sha256" in source
