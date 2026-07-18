#!/usr/bin/env python3
"""Compare a raw RGBA oracle buffer with an AE PNG or EXR render."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
import sys
from pathlib import Path
from typing import Sequence

try:
    from tools.ae_png_depth_inspect import decode_png
except ModuleNotFoundError:  # Direct execution places tools/ on sys.path.
    from ae_png_depth_inspect import decode_png


CHANNELS = ("r", "g", "b", "a")
RAW_FORMATS = ("rgba8", "rgba16le", "rgba32f-le")


class InputError(ValueError):
    """Raised when an input cannot be compared."""


def _json_number(value: float) -> float | str:
    if math.isnan(value):
        return "NaN"
    if value == math.inf:
        return "Infinity"
    if value == -math.inf:
        return "-Infinity"
    return value


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_raw(path: Path, width: int, height: int, raw_format: str,
             integer_max: int = 65535) -> list[float]:
    data = path.read_bytes()
    samples = width * height * 4
    sizes = {"rgba8": 1, "rgba16le": 2, "rgba32f-le": 4}
    expected = samples * sizes[raw_format]
    if len(data) != expected:
        raise InputError(f"raw byte count is {len(data)}, expected {expected}")
    if raw_format == "rgba8":
        return [value / 255.0 for value in data]
    if raw_format == "rgba16le":
        if integer_max <= 0 or integer_max > 65535:
            raise InputError("--raw-integer-max must be in 1..65535")
        return [value / integer_max for value in struct.unpack(f"<{samples}H", data)]
    return list(struct.unpack(f"<{samples}f", data))


def _coerce_rgba(array: object) -> tuple[int, int, list[float]]:
    try:
        shape = array.shape  # type: ignore[attr-defined]
    except AttributeError as exc:
        raise InputError("EXR decoder returned no pixel array") from exc
    if len(shape) == 2:
        array = array[:, :, None]  # type: ignore[index]
        shape = array.shape  # type: ignore[attr-defined]
    if len(shape) != 3 or shape[2] not in (1, 3, 4):
        raise InputError(f"unsupported EXR shape: {tuple(shape)}")
    height, width, count = map(int, shape)
    flat = array.astype("float64", copy=False).reshape(-1, count)  # type: ignore[attr-defined]
    values: list[float] = []
    for pixel in flat:
        if count == 1:
            values.extend((float(pixel[0]),) * 3 + (1.0,))
        elif count == 3:
            values.extend(map(float, pixel))
            values.append(1.0)
        else:
            values.extend(map(float, pixel))
    return width, height, values


def load_render(path: Path) -> tuple[int, int, list[float], str]:
    if path.suffix.lower() == ".exr":
        try:
            import OpenEXR
        except ImportError as exc:
            raise InputError(
                "EXR support requires the OpenEXR package from requirements-dev.txt"
            ) from exc
        try:
            with OpenEXR.File(str(path)) as infile:
                if len(infile.parts) != 1:
                    raise InputError("only single-part EXR renders are supported")
                channels = infile.channels()
                name = next((candidate for candidate in ("RGBA", "RGB", "Y")
                             if candidate in channels), None)
                if name is None:
                    raise InputError(
                        f"EXR has no RGBA, RGB, or Y channel group: {sorted(channels)}"
                    )
                width, height, values = _coerce_rgba(channels[name].pixels)
        except InputError:
            raise
        except Exception as exc:
            raise InputError(f"unable to decode EXR: {exc}") from exc
        return width, height, values, "exr"
    try:
        metadata, decoded = decode_png(path)
        width = metadata["width"]
        height = metadata["height"]
        bit_depth = metadata["bit_depth"]
        if bit_depth == 8:
            samples = decoded
            maximum = 255
        else:
            samples = struct.unpack(f">{width * height * 4}H", decoded)
            maximum = 65535
        values = [value / maximum for value in samples]
    except Exception as exc:
        raise InputError(f"unable to decode PNG: {exc}") from exc
    return width, height, values, f"png_rgba{bit_depth}"


def compare(raw_path: Path, render_path: Path, width: int, height: int,
            raw_format: str = "rgba8", tolerance: float = 0.0,
            raw_integer_max: int = 65535) -> dict[str, object]:
    if width <= 0 or height <= 0:
        raise InputError("dimensions must be positive")
    if not math.isfinite(tolerance) or tolerance < 0:
        raise InputError("tolerance must be a finite non-negative number")
    expected = load_raw(raw_path, width, height, raw_format, raw_integer_max)
    actual_width, actual_height, actual, render_format = load_render(render_path)
    if (actual_width, actual_height) != (width, height):
        raise InputError(
            f"dimension mismatch: raw is {width}x{height}, render is "
            f"{actual_width}x{actual_height}"
        )

    sums = [0.0] * 4
    maxima = [0.0] * 4
    exact_mismatches = 0
    over_tolerance = 0
    first: dict[str, object] | None = None
    for index, (expected_value, actual_value) in enumerate(zip(expected, actual)):
        channel = index % 4
        delta = abs(actual_value - expected_value)
        sums[channel] += delta
        maxima[channel] = max(maxima[channel], delta)
        if actual_value != expected_value:
            exact_mismatches += 1
        if not math.isfinite(delta) or delta > tolerance:
            over_tolerance += 1
            if first is None:
                pixel = index // 4
                first = {
                    "x": pixel % width,
                    "y": pixel // width,
                    "channel": CHANNELS[channel],
                    "expected": _json_number(expected_value),
                    "actual": _json_number(actual_value),
                    "abs_error": _json_number(delta),
                }

    pixels = width * height
    raw_depth = {"rgba8": 8, "rgba16le": 16, "rgba32f-le": 32}[raw_format]
    render_depth = {"png_rgba8": 8, "png_rgba16": 16}.get(render_format, 32)
    if raw_depth != render_depth:
        claim_level = "cross_precision_export_only"
    elif render_format == "exr" and raw_format == "rgba32f-le":
        claim_level = "float_export_exact" if exact_mismatches == 0 else "float_export_tolerance"
    else:
        claim_level = "export_exact" if exact_mismatches == 0 else "export_tolerance"

    return {
        "schema_version": 1,
        "match": over_tolerance == 0,
        "dimensions": {"width": width, "height": height},
        "formats": {"raw": raw_format, "render": render_format},
        "comparison_boundary": {
            "expected": "host_raw_world",
            "actual": "ae_export_artifact",
            "claim_level": claim_level,
            "raw_world_exact": False,
        },
        "tolerance": tolerance,
        "raw_integer_max": raw_integer_max if raw_format == "rgba16le" else None,
        "hashes": {
            "raw_sha256": _sha256(raw_path),
            "render_sha256": _sha256(render_path),
        },
        "exact_mismatched_channels": exact_mismatches,
        "over_tolerance_channels": over_tolerance,
        "max_abs_error": {
            name: _json_number(maxima[i]) for i, name in enumerate(CHANNELS)
        },
        "mean_abs_error": {
            name: _json_number(sums[i] / pixels) for i, name in enumerate(CHANNELS)
        },
        "first_mismatch": first,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--raw", required=True, type=Path)
    parser.add_argument("--render", required=True, type=Path)
    parser.add_argument("--width", required=True, type=int)
    parser.add_argument("--height", required=True, type=int)
    parser.add_argument("--raw-format", choices=RAW_FORMATS, default="rgba8")
    parser.add_argument("--raw-integer-max", type=int, default=65535)
    parser.add_argument("--tolerance", type=float, default=0.0)
    parser.add_argument("--out", type=Path, help="write JSON here instead of stdout")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        report = compare(args.raw, args.render, args.width, args.height,
                         args.raw_format, args.tolerance, args.raw_integer_max)
    except (InputError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    payload = json.dumps(report, indent=2, sort_keys=True, allow_nan=False) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(payload, encoding="utf-8", newline="\n")
    else:
        sys.stdout.write(payload)
    return 0 if report["match"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
