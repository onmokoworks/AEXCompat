from tests import source_owners

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"


def test_smart32_cpu_cli_accepts_explicit_frame_and_fps():
    source = source_owners.harness_windows_text()
    assert '"--render-experimental-smart-32-cpu-time"' in source
    assert "current_time: frame" in source
    assert "total_time: frame.saturating_add(1)" in source
    assert "time_scale: fps" in source
    assert "RenderPixelFormat::Argb32f" in source
    assert "RenderGpuBackend::Cpu" in source
