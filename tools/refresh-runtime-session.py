"""Run a legacy raw-image evidence probe through the supported session harness.

The worker's one-shot image commands were removed in #365.  Runtime evidence
still records raw RGBA inputs and outputs, so this adapter converts the input
to the session harness PNG transport and materializes the legacy raw output
after the session completes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tempfile
from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "target" / "release" / "aexcompat-harness.exe"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _raw_bytes_per_pixel(pixel_format: str) -> int:
    return {"argb8": 4, "argb16": 8, "argb32f": 16}[pixel_format]


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plugin", required=True, type=Path)
    parser.add_argument("--plugin-sha256", required=True)
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--width", required=True, type=int)
    parser.add_argument("--height", required=True, type=int)
    parser.add_argument("--pixel-format", choices=("argb8", "argb16", "argb32f"), required=True)
    parser.add_argument("--current-time", type=int, required=True)
    parser.add_argument("--total-time", type=int, required=True)
    parser.add_argument("--time-scale", type=int, required=True)
    parser.add_argument("--smart", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    plugin = args.plugin if args.plugin.is_absolute() else ROOT / args.plugin
    input_path = args.input if args.input.is_absolute() else ROOT / args.input
    output_path = args.output if args.output.is_absolute() else ROOT / args.output

    if not HARNESS.is_file():
        raise RuntimeError(
            f"session harness is missing: {HARNESS}; build "
            "aexcompat-harness with --release first"
        )
    if not plugin.is_file():
        raise RuntimeError(f"plugin is missing: {plugin}")
    actual_plugin_sha = _sha256(plugin)
    if actual_plugin_sha != args.plugin_sha256.lower():
        raise RuntimeError(
            f"plugin SHA-256 mismatch: expected {args.plugin_sha256.lower()}, "
            f"actual {actual_plugin_sha}"
        )
    if not input_path.is_file():
        raise RuntimeError(f"input is missing: {input_path}")
    if args.width <= 0 or args.height <= 0:
        raise RuntimeError("width and height must be positive")

    expected_input_size = args.width * args.height * 4
    if input_path.suffix.lower() == ".png":
        session_input_source = input_path
    else:
        raw_input = input_path.read_bytes()
        if len(raw_input) != expected_input_size:
            raise RuntimeError(
                f"raw input size mismatch: expected {expected_input_size}, "
                f"actual {len(raw_input)}"
            )
        session_input_source = None

    output_path.parent.mkdir(parents=True, exist_ok=True)
    temp_root = ROOT / "target" / "tmp"
    temp_root.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="refresh-session-", dir=temp_root) as temporary:
        temporary_root = Path(temporary)
        session_input = temporary_root / "input.png"
        if session_input_source is not None:
            shutil.copyfile(session_input_source, session_input)
        else:
            Image.frombytes("RGBA", (args.width, args.height), input_path.read_bytes()).save(
                session_input
            )
        session_output = temporary_root / "output.png"
        command = [
            str(HARNESS),
            "--render-experimental-session",
            str(plugin),
            str(session_input),
            str(session_output),
            args.pixel_format,
            "smart" if args.smart else "classic",
            str(args.current_time),
            str(args.total_time),
            str(args.time_scale),
        ]
        import subprocess

        completed = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            encoding="utf-8",
            errors="replace",
            capture_output=True,
            timeout=60,
        )
        if completed.returncode != 0:
            raise RuntimeError(
                "session harness failed with exit code "
                f"{completed.returncode}: {completed.stdout}{completed.stderr}"
            )
        try:
            report = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"session harness returned invalid JSON: {completed.stdout}") from error
        if report.get("passed") is not True:
            raise RuntimeError(
                "session harness reported an unsuccessful render: "
                + json.dumps(report, ensure_ascii=False, sort_keys=True)
            )

        if report.get("empty_result_rect") and not session_output.exists():
            output_bytes = b""
        elif args.pixel_format == "argb8":
            if not session_output.is_file():
                raise RuntimeError("session harness did not create PNG output")
            output_bytes = Image.open(session_output).convert("RGBA").tobytes()
        else:
            sidecar = session_output.with_suffix(
                {"argb16": ".rgba16le", "argb32f": ".rgba32f-le"}[args.pixel_format]
            )
            if not sidecar.is_file():
                raise RuntimeError(f"session harness did not create raw sidecar: {sidecar}")
            output_bytes = sidecar.read_bytes()

        expected_output_size = args.width * args.height * _raw_bytes_per_pixel(args.pixel_format)
        if len(output_bytes) != expected_output_size and not report.get("empty_result_rect"):
            raise RuntimeError(
                f"raw output size mismatch: expected {expected_output_size}, "
                f"actual {len(output_bytes)}"
            )
        output_path.write_bytes(output_bytes)

    report["refresh_adapter"] = {
        "command": "render-experimental-session",
        "pixel_format": args.pixel_format,
        "plugin_sha256": actual_plugin_sha,
        "output_size_bytes": len(output_bytes),
    }
    print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError) as error:
        print(f"refresh-runtime-session: {error}", file=sys.stderr)
        raise SystemExit(1)
