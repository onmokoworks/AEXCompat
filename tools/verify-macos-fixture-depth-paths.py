#!/usr/bin/env python3
"""Exercise deep Classic and SmartFX fixtures through the macOS shipping CLI."""

import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
WIDTH = 4
HEIGHT = 3


def smoke(
    harness: Path, aex: Path, scratch: Path, depth: str, render_path: str
) -> None:
    name = f"{render_path}-{depth}"
    component = {"argb8": 1, "argb16": 2, "argb32f": 4}[depth]
    packed = WIDTH * 4 * component
    rowbytes = packed + 4 * component
    fixture = {
        "schema": "aexcompat.render_fixture",
        "schema_version": 2,
        "primary_layer": "primary.png",
        "parameters": [],
        "matrix": [],
        "pixel_format": depth,
        "render_path": render_path,
        "premultiplication": "straight",
        "timing": {"current_time": 0, "time_step": 1, "total_time": 1, "time_scale": 1},
        "final_artifact": "raw",
        "checkpoints": [{"id": "input", "stage": f"{render_path}-input"}],
        "worlds": {
            "primary": {
                "pixel_format": depth,
                "width": WIDTH,
                "height": HEIGHT,
                "rowbytes": rowbytes,
                "row_padding": 4 * component,
                "padding_byte": 90,
                "origin": {"x": 2, "y": -1},
                "extent": {"left": 0, "top": 0, "right": WIDTH, "bottom": HEIGHT},
            },
            "secondary": [],
        },
    }
    fixture_path = scratch / f"{name}.json"
    fixture_path.write_text(json.dumps(fixture), encoding="utf-8")
    output = scratch / name
    result = subprocess.run(
        [
            str(harness),
            "--headless",
            "--render-fixture",
            str(aex),
            str(fixture_path),
            str(output),
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=180,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"{name} fixture CLI failed: {result.stderr.strip()}\n{result.stdout.strip()}"
        )
    report = json.loads(result.stdout)
    assert report["schema_version"] == 2 and report["complete"] is True, report
    assert len(report["cases"]) == 1, report
    case = report["cases"][0]
    identity = case["case_identity"]["sha256"]
    assert case["case_identity"]["pixel_format"] == depth, case
    assert case["artifact_directory"] == f"cases/{identity}", case
    directory = output / case["artifact_directory"]
    final = (directory / "final/output.bin").read_bytes()
    final_meta = json.loads(
        (directory / "final/output.json").read_text(encoding="utf-8")
    )
    input_raw = (directory / "checkpoints/input/output.bin").read_bytes()
    input_meta = json.loads(
        (directory / "checkpoints/input/output.json").read_text(encoding="utf-8")
    )
    assert len(final) == packed * HEIGHT and any(final), name
    assert len(input_raw) == rowbytes * HEIGHT and any(input_raw), name
    assert final_meta["comparison_identity"]["fixture_case"]["sha256"] == identity
    assert input_meta["comparison_identity"]["fixture_case"]["sha256"] == identity
    assert input_meta["pixel_format"] == depth and input_meta["rowbytes"] == rowbytes
    assert input_meta["row_padding"] == 4 * component
    assert input_meta["origin"] == {"x": 2, "y": -1}
    for row in range(HEIGHT):
        assert input_raw[row * rowbytes + packed : (row + 1) * rowbytes] == bytes(
            [90]
        ) * (4 * component)
    packed_input = b"".join(
        input_raw[row * rowbytes : row * rowbytes + packed] for row in range(HEIGHT)
    )
    if render_path == "smart":
        assert final == packed_input, "SmartFX probe did not copy input pixels"
    else:
        assert final != packed_input, "Classic probe did not apply its color"
        pixel_size = 4 * component
        first_pixel = final[:pixel_size]
        assert final == first_pixel * (WIDTH * HEIGHT), (
            "Classic color fill was not uniform"
        )
        if depth == "argb16":
            channels = sorted(struct.unpack("<4H", first_pixel))
            assert channels == [16448, 16448, 16448, 32768], channels
        elif depth == "argb32f":
            channels = sorted(struct.unpack("<4f", first_pixel))
            assert abs(channels[0] - 128 / 255) < 1e-6, channels
            assert all(abs(channel - channels[0]) < 1e-6 for channel in channels[:3]), (
                channels
            )
            assert abs(channels[3] - 1.0) < 1e-6, channels
    print(
        f"macos_fixture_smoke={name} aex_sha256={hashlib.sha256(aex.read_bytes()).hexdigest()} case={identity}"
    )


def main() -> None:
    if len(sys.argv) not in (3, 4):
        raise SystemExit(
            "usage: verify-macos-fixture-depth-paths.py <harness> <deep-classic.aex> [smart.aex]"
        )
    inputs = [Path(argument).resolve(strict=True) for argument in sys.argv[1:]]
    harness, deep_aex = inputs[:2]
    with tempfile.TemporaryDirectory(prefix="aexcompat-macos-depth-") as directory:
        scratch = Path(directory)
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools/generate-oracle-rgba-input.py"),
                "--width",
                str(WIDTH),
                "--height",
                str(HEIGHT),
                "--alpha-mode",
                "opaque",
                "--out",
                str(scratch / "primary.png"),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        smoke(harness, deep_aex, scratch, "argb16", "classic")
        smoke(harness, deep_aex, scratch, "argb32f", "classic")
        if len(inputs) == 3:
            smoke(harness, inputs[2], scratch, "argb8", "smart")


if __name__ == "__main__":
    main()
