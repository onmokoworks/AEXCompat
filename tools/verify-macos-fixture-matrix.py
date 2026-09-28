#!/usr/bin/env python3
"""Run the public Classic AEX through the shipping macOS fixture CLI."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
WIDTH = 4
HEIGHT = 3


def parameter(
    slot: int, kind: str, value: float, layer_path: str | None = None
) -> dict:
    return {
        "slot": slot,
        "name": "layer" if kind == "layer" else "amount",
        "kind": kind,
        "minimum": 0.0,
        "maximum": 0.0 if kind == "layer" else 255.0,
        "value": value,
        "choices": [],
        "color": [0, 0, 0, 0],
        "components": [0.0, 0.0, 0.0],
        "component_count": 0,
        "layer_path": layer_path,
        "enabled": True,
        "visible": True,
        "supervised": False,
        "control_size": [0, 0],
    }


def checked_artifact(
    directory: Path, case_hash: str, stage: str, rowbytes: int
) -> bytes:
    target = directory / stage
    metadata = json.loads((target / "output.json").read_text(encoding="utf-8"))
    raw = (target / "output.bin").read_bytes()
    assert metadata["width"] == WIDTH and metadata["height"] == HEIGHT, metadata
    assert metadata["rowbytes"] == rowbytes, metadata
    assert metadata["comparison_identity"]["fixture_case"]["sha256"] == case_hash
    assert len(raw) == rowbytes * HEIGHT and any(raw), stage
    return raw


def run_once(
    harness: Path, aex: Path, scratch: Path, run_number: int
) -> tuple[list[str], list[str]]:
    output = scratch / f"output-{run_number}"
    result = subprocess.run(
        [
            str(harness),
            "--headless",
            "--render-fixture",
            str(aex),
            str(scratch / "fixture.json"),
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
            f"fixture CLI failed ({result.returncode}): {result.stderr.strip()}\n"
            f"stdout: {result.stdout.strip()}"
        )
    report = json.loads(result.stdout)
    assert report["schema"] == "aexcompat.render_fixture_report", report
    assert report["schema_version"] == 2 and report["complete"] is True, report
    assert len(report["cases"]) == 2, report
    identities = []
    outputs = []
    for case in report["cases"]:
        identity = case["case_identity"]["sha256"]
        assert len(identity) == 64 and all(c in "0123456789abcdef" for c in identity)
        relative = Path(case["artifact_directory"])
        assert relative.parts == ("cases", identity), relative
        directory = output / relative
        final = checked_artifact(directory, identity, "final", 16)
        primary = checked_artifact(directory, identity, "checkpoints/primary", 20)
        secondary = checked_artifact(directory, identity, "checkpoints/secondary", 24)
        for row in range(HEIGHT):
            assert primary[row * 20 + 16 : (row + 1) * 20] == bytes([165]) * 4
            assert secondary[row * 24 + 16 : (row + 1) * 24] == bytes([90]) * 8
        for name, rowbytes, padding, origin in (
            ("primary", 20, 4, {"x": 2, "y": -1}),
            ("secondary", 24, 8, {"x": -2, "y": 3}),
        ):
            metadata = json.loads(
                (directory / "checkpoints" / name / "output.json").read_text(
                    encoding="utf-8"
                )
            )
            assert (
                metadata["rowbytes"] == rowbytes and metadata["row_padding"] == padding
            ), metadata
            assert metadata["origin"] == origin, metadata
        identities.append(identity)
        outputs.append(hashlib.sha256(final).hexdigest())
    assert identities[0] != identities[1], identities
    assert outputs[0] != outputs[1], "matrix values did not change rendered pixels"
    return identities, outputs


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit(
            "usage: verify-macos-fixture-matrix.py <harness> <public-layer-probe.aex>"
        )
    harness, aex = (Path(argument).resolve(strict=True) for argument in sys.argv[1:])
    with tempfile.TemporaryDirectory(prefix="aexcompat-macos-fixture-") as directory:
        scratch = Path(directory)
        for name, alpha_mode in (
            ("primary", "opaque"),
            ("secondary", "vertical-gradient"),
        ):
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "tools/generate-oracle-rgba-input.py"),
                    "--width",
                    str(WIDTH),
                    "--height",
                    str(HEIGHT),
                    "--alpha-mode",
                    alpha_mode,
                    "--out",
                    str(scratch / f"{name}.png"),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
        fixture = {
            "schema": "aexcompat.render_fixture",
            "schema_version": 2,
            "primary_layer": "primary.png",
            "parameters": [
                parameter(1, "layer", 0.0, "secondary.png"),
                parameter(2, "float", 20.0),
            ],
            "matrix": [{"slot": 2, "values": [20.0, 255.0]}],
            "pixel_format": "argb8",
            "render_path": "classic",
            "premultiplication": "straight",
            "timing": {
                "current_time": 0,
                "time_step": 1,
                "total_time": 1,
                "time_scale": 1,
            },
            "final_artifact": "raw",
            "checkpoints": [
                {"id": "primary", "stage": "classic-input"},
                {"id": "secondary", "stage": "classic-layer-slot1"},
            ],
            "worlds": {
                "primary": {
                    "pixel_format": "argb8",
                    "width": WIDTH,
                    "height": HEIGHT,
                    "rowbytes": 20,
                    "row_padding": 4,
                    "padding_byte": 165,
                    "origin": {"x": 2, "y": -1},
                    "extent": {"left": 0, "top": 0, "right": WIDTH, "bottom": HEIGHT},
                },
                "secondary": [
                    {
                        "slot": 1,
                        "pixel_format": "argb8",
                        "width": WIDTH,
                        "height": HEIGHT,
                        "rowbytes": 24,
                        "row_padding": 8,
                        "padding_byte": 90,
                        "origin": {"x": -2, "y": 3},
                        "extent": {
                            "left": 0,
                            "top": 0,
                            "right": WIDTH,
                            "bottom": HEIGHT,
                        },
                    }
                ],
            },
        }
        (scratch / "fixture.json").write_text(json.dumps(fixture), encoding="utf-8")
        first = run_once(harness, aex, scratch, 0)
        second = run_once(harness, aex, scratch, 1)
        assert first == second, "case identities or rendered pixels were not stable"
        print(
            f"macos_fixture_matrix_passed=true cases=2 repeat=2 aex_sha256={hashlib.sha256(aex.read_bytes()).hexdigest()}"
        )


if __name__ == "__main__":
    main()
