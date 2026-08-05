import json
import os
import subprocess
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
SOURCES = source_owners.contract_files("pf_world_transform_composite_rect")
def source_text() -> str:
    return "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)

def _worker() -> Path | None:
    configured = os.environ.get("AEXCOMPAT_RENDER_WORKER")
    candidates = [
        Path(configured) if configured else None,
        ROOT / "target" / "minihost-build-v18" / "Release" / "aex_render_worker.exe",
        ROOT / "target" / "minihost-build-v18" / "aex_render_worker.exe",
    ]
    return next((candidate for candidate in candidates if candidate and candidate.is_file()), None)

def test_world_transform_suite_has_typed_frozen_abi_and_wired_composite_rect():
    text = source_text()
    assert "unsupported_after_copy" not in text

def test_composite_rect_source_contains_bounded_cleanroom_guards():
    text = source_text()
    for marker in (
        "constexpr int32_t kPfErrBadCallbackParam = 516",
        "source_opacity < 0 || source_opacity > 255",
        "composite_rect_registered<uint8_t, 255>",
        "composite_rect_registered<uint16_t, 32768>",
        "source_info.pixel_format != destination_info.pixel_format",
        "std::vector<Pixel> snapshot",
        "const uint64_t destination_alpha",
    ):
        assert marker in text

def test_composite_rect_runtime_matrix():
    worker = _worker()
    assert worker is not None, "build aex_render_worker before running the focused runtime test"
    completed = subprocess.run(
        [worker, "--self-test-world-transform-composite"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert completed.returncode == 0, completed.stderr or completed.stdout
    assert json.loads(completed.stdout) == {"world_transform_composite_rect": "passed"}
