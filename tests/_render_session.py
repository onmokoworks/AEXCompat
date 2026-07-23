"""Small adapter for built-artifact probes after the one-shot CLI removal.

The production experimental harness accepts image files and writes a PNG
preview plus a depth-preserving raw sidecar.  The older probe tests used the
deleted worker argv directly with raw RGBA files, so keep their byte-oriented
oracles while entering through the supported session-only harness command.
"""

import json
import subprocess
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "target" / "release" / "aexcompat-harness.exe"


def run_session_render(
    tmp_path: Path,
    plugin: Path,
    input_path: Path,
    output_path: Path,
    *,
    width: int,
    height: int,
    pixel_format: str = "argb8",
    smart: bool = False,
    current_time: int = 0,
    total_time: int = 1,
    time_scale: int = 1,
):
    """Run one current session render and materialize the legacy raw oracle."""

    assert HARNESS.is_file(), (
        "aexcompat-harness.exe is not built; build the broker Release harness "
        "before running built-artifact tests"
    )
    session_input = tmp_path / f"{output_path.stem}-session-input.png"
    if input_path.suffix.lower() == ".png":
        session_input.write_bytes(input_path.read_bytes())
    else:
        raw = input_path.read_bytes()
        assert len(raw) == width * height * 4
        Image.frombytes("RGBA", (width, height), raw).save(session_input)
    session_output = tmp_path / f"{output_path.stem}-session-output.png"
    completed = subprocess.run(
        [
            str(HARNESS),
            "--render-experimental-session",
            str(plugin),
            str(session_input),
            str(session_output),
            pixel_format,
            "smart" if smart else "classic",
            str(current_time),
            str(total_time),
            str(time_scale),
        ],
        cwd=ROOT,
        text=True,
        encoding="utf-8",
        errors="replace",
        capture_output=True,
        timeout=60,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
    report = json.loads(completed.stdout)
    # Keep the old report contract where it is still meaningful to the probe
    # oracle; all other session diagnostics remain available unchanged.
    report.setdefault("status", "render_completed" if report.get("passed") else "render_failed")
    report.setdefault("render_error", 0 if report.get("passed") else 1)

    if report.get("empty_result_rect") and not session_output.exists():
        output_path.write_bytes(b"")
        return report
    if pixel_format == "argb8":
        pixels = Image.open(session_output).convert("RGBA").tobytes()
    else:
        suffix = {"argb16": ".rgba16le", "argb32f": ".rgba32f-le"}[pixel_format]
        raw_output = session_output.with_suffix(suffix)
        assert raw_output.is_file(), f"missing session raw sidecar: {raw_output}"
        pixels = raw_output.read_bytes()
    output_path.write_bytes(pixels)
    return report
