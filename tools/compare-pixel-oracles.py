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
RAW_FORMATS = (
    "rgba8", "rgba16le", "rgba32f-le",
    "argb8", "argb16le-ae", "argb32f-le",
)


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


def _load_artifact_metadata(path: Path, *, schema: str, data_path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise InputError(f"invalid artifact metadata {path}: {exc}") from exc
    required = {
        "schema", "schema_version", "width", "height", "channel_order",
        "endianness", "premultiplication", "working_space", "render_mode",
        "data_file", "data_size_bytes", "data_sha256", "comparison_boundaries",
        "comparison_identity",
    }
    missing = sorted(required - value.keys()) if isinstance(value, dict) else sorted(required)
    if missing:
        raise InputError(f"artifact metadata missing keys: {', '.join(missing)}")
    if value["schema"] != schema or value["schema_version"] != 1:
        raise InputError(f"unexpected artifact schema: {value['schema']!r} v{value['schema_version']!r}")
    if value["data_file"] != data_path.name:
        raise InputError("artifact metadata data_file does not name the compared file")
    if value["data_size_bytes"] != data_path.stat().st_size:
        raise InputError("artifact metadata data_size_bytes does not match the compared file")
    if value["data_sha256"] != _sha256(data_path):
        raise InputError("artifact metadata data_sha256 does not match the compared file")
    if (type(value["width"]) is not int or type(value["height"]) is not int or
            value["width"] <= 0 or value["height"] <= 0):
        raise InputError("artifact metadata dimensions must be positive integers")
    if value["premultiplication"] not in ("straight", "premultiplied", "opaque"):
        raise InputError("artifact metadata premultiplication is not canonical")
    if value["working_space"] != "None" or value["render_mode"] != "software":
        raise InputError("artifact metadata render conditions are not canonical")
    identity = value["comparison_identity"]
    identity_keys = {"plugin_sha256", "input_sha256", "world_sha256", "render_path",
                     "pixel_format", "timing", "requested_parameters", "origin"}
    if not isinstance(identity, dict) or set(identity) != identity_keys:
        raise InputError("artifact comparison_identity keys are not strict")
    for key in ("plugin_sha256", "input_sha256", "world_sha256"):
        digest = identity[key]
        if (not isinstance(digest, str) or len(digest) != 64 or
                any(char not in "0123456789abcdef" for char in digest)):
            raise InputError(f"artifact comparison_identity {key} is invalid")
    timing = identity["timing"]
    if (identity["render_path"] not in ("classic", "smartfx") or
            identity["pixel_format"] not in ("argb8", "argb16", "argb32f") or
            not isinstance(identity["requested_parameters"], list) or
            not isinstance(timing, dict) or
            set(timing) != {"current_time", "time_step", "total_time", "time_scale"} or
            any(type(item) is not int for item in timing.values())):
        raise InputError("artifact comparison_identity values are not canonical")
    origin = identity["origin"]
    if (not isinstance(origin, dict) or set(origin) != {"x", "y"} or
            any(type(item) is not int for item in origin.values()) or
            value.get("origin") != origin):
        raise InputError("artifact comparison_identity origin is not canonical")
    if schema == "aexcompat.render_raw":
        exact_keys = required | {
            "rowbytes", "row_padding", "source_world_rowbytes",
            "source_world_row_padding", "pixel_format", "component_bytes",
            "component_representation", "origin",
        }
        expected = {
            "channel_order": "ARGB", "endianness": "little", "pixel_format": "argb32f",
            "component_bytes": 4, "component_representation": "ieee754_binary32_raw_words",
            "rowbytes": value["width"] * 16, "row_padding": "excluded",
            "source_world_rowbytes": None, "source_world_row_padding": "not_transported",
            "comparison_boundaries": {"aex_arithmetic": "internal_world_raw",
                                      "host_export": "not_applicable"},
        }
    else:
        exact_keys = required | {
            "storage", "compression", "pixel_format", "exr_file_channel_order",
            "channel_type", "source_world_rowbytes", "source_world_row_padding",
            "source_transport_order", "word_comparison", "rgb_policy", "origin",
        }
        expected = {
            "channel_order": "RGBA", "endianness": "little", "pixel_format": "float32",
            "exr_file_channel_order": ["A", "B", "G", "R"], "channel_type": "FLOAT32",
            "storage": "scanline", "compression": "none", "rgb_policy": "preserve",
            "word_comparison": "raw_u32_little_endian",
            "source_world_rowbytes": None, "source_world_row_padding": "not_transported",
            "source_transport_order": "RGBA",
            "comparison_boundaries": {"aex_arithmetic": "compare_source_raw_world",
                                      "host_export": "compare_float32_exr_raw_u32"},
        }
    for key, expected_value in expected.items():
        if value.get(key) != expected_value:
            raise InputError(f"artifact metadata {key} is not canonical")
    if set(value) != exact_keys:
        raise InputError("artifact metadata keys are not strict")
    return value


def _load_exr_rgba_u32(path: Path) -> tuple[int, int, object]:
    try:
        import OpenEXR
        import numpy as np
    except ImportError as exc:
        raise InputError("raw-u32 EXR comparison requires OpenEXR and numpy") from exc
    try:
        with OpenEXR.File(str(path)) as infile:
            if len(infile.parts) != 1:
                raise InputError("only single-part EXR renders are supported")
            channels = infile.channels()
            if "RGBA" not in channels:
                raise InputError(f"EXR has no RGBA channel group: {sorted(channels)}")
            pixels = channels["RGBA"].pixels
            if len(pixels.shape) != 3 or pixels.shape[2] != 4 or pixels.dtype != np.float32:
                raise InputError(f"EXR RGBA is not FLOAT32 RGBA: {pixels.shape}, {pixels.dtype}")
            height, width, _ = pixels.shape
            words = pixels.view(np.uint32).reshape(-1, 4).copy()
    except InputError:
        raise
    except Exception as exc:
        raise InputError(f"unable to decode EXR raw words: {exc}") from exc
    return int(width), int(height), words


def compare_raw_u32(raw_path: Path, render_path: Path, raw_metadata_path: Path,
                    render_metadata_path: Path) -> dict[str, object]:
    raw_meta = _load_artifact_metadata(
        raw_metadata_path, schema="aexcompat.render_raw", data_path=raw_path)
    exr_meta = _load_artifact_metadata(
        render_metadata_path, schema="aexcompat.render_exr", data_path=render_path)
    if raw_meta.get("pixel_format") != "argb32f" or raw_meta["channel_order"] != "ARGB":
        raise InputError("raw-u32 comparison requires argb32f ARGB render-raw metadata")
    if raw_meta["endianness"] != "little" or exr_meta["endianness"] != "little":
        raise InputError("raw-u32 comparison requires little-endian artifacts")
    if exr_meta["channel_order"] != "RGBA" or exr_meta.get("pixel_format") != "float32":
        raise InputError("raw-u32 comparison requires FLOAT32 RGBA render-exr metadata")
    dimensions = (raw_meta["width"], raw_meta["height"])
    if dimensions != (exr_meta["width"], exr_meta["height"]):
        raise InputError("artifact metadata dimensions do not match")
    for condition in ("premultiplication", "working_space", "render_mode"):
        if raw_meta[condition] != exr_meta[condition]:
            raise InputError(f"artifact condition mismatch: {condition}")
    if raw_meta["comparison_identity"] != exr_meta["comparison_identity"]:
        raise InputError("artifact comparison_identity does not match")
    width, height = map(int, dimensions)
    try:
        import numpy as np
    except ImportError as exc:
        raise InputError("raw-u32 comparison requires numpy") from exc
    raw = raw_path.read_bytes()
    if len(raw) != width * height * 16:
        raise InputError("argb32f raw byte count does not match metadata dimensions")
    argb = np.frombuffer(raw, dtype="<u4").reshape(-1, 4)
    actual_width, actual_height, actual = _load_exr_rgba_u32(render_path)
    if (actual_width, actual_height) != (width, height):
        raise InputError("decoded EXR dimensions do not match artifact metadata")
    mismatch_mask = np.empty((width * height, 4), dtype=np.bool_)
    for rgba_channel, argb_channel in enumerate((1, 2, 3, 0)):
        mismatch_mask[:, rgba_channel] = argb[:, argb_channel] != actual[:, rgba_channel]
    mismatch_count = int(np.count_nonzero(mismatch_mask))
    first = None
    if mismatch_count:
        index = int(np.flatnonzero(mismatch_mask.reshape(-1))[0])
        pixel, channel = divmod(index, 4)
        expected_word = int(argb[pixel, (1, 2, 3, 0)[channel]])
        actual_word = int(actual[pixel, channel])
        first = {"x": pixel % width, "y": pixel // width,
                 "channel": CHANNELS[channel],
                 "expected_u32": f"0x{expected_word:08x}",
                 "actual_u32": f"0x{actual_word:08x}"}
    return {
        "schema": "aexcompat.raw_u32_comparison", "schema_version": 1,
        "match": mismatch_count == 0,
        "dimensions": {"width": width, "height": height},
        "comparison_boundary": {"expected": "internal_world_raw",
                                "actual": "float32_exr_channel_words",
                                "claim_level": "raw_u32_exact"},
        "artifact_conditions": {key: raw_meta[key] for key in
                                ("premultiplication", "working_space", "render_mode")},
        "difference_layers": {
            "aex_arithmetic": {"boundary": "internal_world_raw",
                               "status": "authority_not_compared"},
            "host_export": {"boundary": "internal_world_raw_to_float32_exr",
                            "status": "exact" if mismatch_count == 0 else "different",
                            "raw_u32_mismatched_channels": mismatch_count},
        },
        "hashes": {"raw_sha256": raw_meta["data_sha256"],
                   "render_sha256": exr_meta["data_sha256"]},
        "raw_u32_mismatched_channels": mismatch_count,
        "first_mismatch": first,
    }


def load_raw(path: Path, width: int, height: int, raw_format: str,
             integer_max: int = 65535) -> list[float]:
    data = path.read_bytes()
    samples = width * height * 4
    sizes = {
        "rgba8": 1, "rgba16le": 2, "rgba32f-le": 4,
        "argb8": 1, "argb16le-ae": 2, "argb32f-le": 4,
    }
    expected = samples * sizes[raw_format]
    if len(data) != expected:
        raise InputError(f"raw byte count is {len(data)}, expected {expected}")
    if raw_format in ("rgba8", "argb8"):
        values = [value / 255.0 for value in data]
    elif raw_format in ("rgba16le", "argb16le-ae"):
        if raw_format == "argb16le-ae":
            integer_max = 32768
        if integer_max <= 0 or integer_max > 65535:
            raise InputError("--raw-integer-max must be in 1..65535")
        values = [value / integer_max for value in struct.unpack(f"<{samples}H", data)]
    else:
        values = list(struct.unpack(f"<{samples}f", data))
    if raw_format.startswith("argb"):
        values = [component for pixel in zip(*[iter(values)] * 4)
                  for component in (*pixel[1:], pixel[0])]
    return values


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
                "EXR support requires the OpenEXR package from the uv dev environment (uv sync)"
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
    raw_depth = {
        "rgba8": 8, "rgba16le": 16, "rgba32f-le": 32,
        "argb8": 8, "argb16le-ae": 16, "argb32f-le": 32,
    }[raw_format]
    render_depth = {"png_rgba8": 8, "png_rgba16": 16}.get(render_format, 32)
    if raw_depth != render_depth:
        claim_level = "cross_precision_export_only"
    elif render_format == "exr" and raw_format in ("rgba32f-le", "argb32f-le"):
        claim_level = "float_export_exact" if exact_mismatches == 0 else "float_export_tolerance"
    else:
        claim_level = "export_exact" if exact_mismatches == 0 else "export_tolerance"

    return {
        "schema_version": 1,
        "match": over_tolerance == 0,
        "dimensions": {"width": width, "height": height},
        "formats": {"raw": raw_format, "render": render_format},
        "comparison_boundary": {
            "expected": "provided_raw_buffer",
            "actual": "ae_export_artifact",
            "claim_level": claim_level,
            "raw_world_exact": False,
        },
        "difference_layers": {
            "aex_arithmetic": {
                "boundary": "internal_world_raw",
                "status": "not_evaluated_by_export_comparison",
                "authority": "not_bound_without_artifact_metadata",
            },
            "host_export": {
                "boundary": "internal_world_raw_to_ae_export_artifact",
                "status": "exact" if exact_mismatches == 0 else "different",
                "exact_mismatched_channels": exact_mismatches,
                "over_tolerance_channels": over_tolerance,
            },
        },
        "tolerance": tolerance,
        "raw_integer_max": (
            32768 if raw_format == "argb16le-ae"
            else raw_integer_max if raw_format == "rgba16le" else None
        ),
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
    parser.add_argument("--width", type=int)
    parser.add_argument("--height", type=int)
    parser.add_argument("--raw-format", choices=RAW_FORMATS, default="rgba8")
    parser.add_argument("--raw-integer-max", type=int, default=65535)
    parser.add_argument("--tolerance", type=float, default=0.0)
    parser.add_argument("--raw-u32", action="store_true",
                        help="compare bound PF32 raw and EXR channel words exactly")
    parser.add_argument("--raw-metadata", type=Path)
    parser.add_argument("--render-metadata", type=Path)
    parser.add_argument("--out", type=Path, help="write JSON here instead of stdout")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.raw_u32:
            if not args.raw_metadata or not args.render_metadata:
                raise InputError("--raw-u32 requires --raw-metadata and --render-metadata")
            report = compare_raw_u32(args.raw, args.render, args.raw_metadata,
                                     args.render_metadata)
        else:
            if args.width is None or args.height is None:
                raise InputError("export comparison requires --width and --height")
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
