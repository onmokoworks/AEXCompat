"""Real-worker custom-UI self-test for issue #242.

Drives the built render worker directly (one-shot --render-image) against the
authored pf_custom_ui_probe fixture, once with a render-path click trailer and
once with a draw trailer, and checks the public custom_ui_* contract the broker
gate enforces. This crosses the real AEX boundary for render-path custom UI: the
probe opens the host color picker, invalidates the control, flags its color
parameter changed on a click, and paints one drawbot rectangle on a draw.

It also pins the #259 regression: a real custom-UI plug-in must render cleanly
(render_error 0) after a click; the classic finalize path used to double-close
the UI context and turn a clean render into render_error -5.

Registered in built_artifact_tests.txt; run with --run-built-artifact-tests
after building the worker and the fixture (tools/build-pf-custom-ui-probe.ps1).
"""

import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER = ROOT / "target/minihost-build/aex_render_worker.exe"
AEX = ROOT / "target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex"


def _render(tmp_path, trailer):
    sha = hashlib.sha256(AEX.read_bytes()).hexdigest()
    width, height = 16, 12
    input_path = tmp_path / "input.rgba"
    output_path = tmp_path / "output.rgba"
    input_path.write_bytes(bytes([10, 20, 30, 255]) * (width * height))
    argv = [
        str(WORKER),
        "--render-image",
        str(AEX),
        sha,
        "v2|",
        str(input_path),
        str(output_path),
        str(width),
        str(height),
        "0",
        "1",
        "1",
        "1",
        trailer,
    ]
    completed = subprocess.run(
        argv, cwd=ROOT, text=True, capture_output=True, timeout=60
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    return json.loads(completed.stdout), output_path


def test_real_custom_ui_click_renders_and_meets_the_gate(tmp_path):
    report, output_path = _render(tmp_path, "click:v1|8|6|0.85|0.2|0.6|1.0")
    # Clean render after the click (issue #259: no spurious -5).
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    # The render-path click contract the broker gate enforces.
    assert report["custom_ui_click_dispatched"] is True
    assert report["custom_ui_click_error"] == 0
    assert report["custom_ui_click_out_flags"] & 9 == 9
    assert report["custom_ui_click_changed_value"] is True
    assert report["app_color_picker_calls"] == 1
    assert report["app_invalidate_rect_calls"] == 1
    assert report["custom_ui_lifecycle_errors"] == [0, 0, 0, 0]
    assert report["custom_ui_context_closed"] is True
    assert output_path.stat().st_size == 16 * 12 * 4


def test_real_custom_ui_draw_renders_and_meets_the_gate(tmp_path):
    report, output_path = _render(tmp_path, "draw:v1")
    assert report["status"] == "render_completed"
    assert report["render_error"] == 0
    assert report["custom_ui_draw_dispatched"] is True
    assert report["custom_ui_draw_error"] == 0
    assert report["custom_ui_draw_out_flags"] & 1 == 1
    assert report["custom_ui_lifecycle_errors"] == [0, 0, 0, 0]
    assert report["custom_ui_context_closed"] is True
    assert output_path.stat().st_size == 16 * 12 * 4
