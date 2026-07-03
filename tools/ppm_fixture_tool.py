#!/usr/bin/env python3
"""Tiny PPM fixture generator/transformer for AEX compatibility tests."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = LAB_ROOT / "target" / "ppm-fixtures"
MAX_PIXELS = 4096 * 4096


@dataclass(frozen=True)
class PpmImage:
    width: int
    height: int
    pixels: bytes


def path_has_traversal(path: Path) -> bool:
    return any(part in ("..", ".") for part in path.parts)


def validate_output_path(path: Path) -> Path:
    if path_has_traversal(path):
        raise ValueError("output path must not contain traversal components")
    if path.suffix.lower() != ".ppm":
        raise ValueError("output path must have .ppm extension")
    FIXTURE_ROOT.mkdir(parents=True, exist_ok=True)
    absolute = path if path.is_absolute() else LAB_ROOT / path
    if not absolute.resolve(strict=False).is_relative_to(FIXTURE_ROOT.resolve(strict=True)):
        raise ValueError(f"output path must stay under {FIXTURE_ROOT}")
    absolute.parent.mkdir(parents=True, exist_ok=True)
    if not absolute.parent.resolve(strict=True).is_relative_to(FIXTURE_ROOT.resolve(strict=True)):
        raise ValueError(f"output parent must resolve under {FIXTURE_ROOT}")
    return absolute


def write_ppm_create_new(path: Path, image: PpmImage) -> None:
    if image.width <= 0 or image.height <= 0:
        raise ValueError("image dimensions must be positive")
    if image.width * image.height > MAX_PIXELS:
        raise ValueError("image exceeds max fixture pixel count")
    expected = image.width * image.height * 3
    if len(image.pixels) != expected:
        raise ValueError(f"pixel byte count mismatch: expected {expected}, got {len(image.pixels)}")
    absolute = validate_output_path(path)
    header = f"P6\n{image.width} {image.height}\n255\n".encode("ascii")
    with absolute.open("xb") as handle:
        handle.write(header)
        handle.write(image.pixels)


def read_token(data: bytes, offset: int) -> tuple[bytes, int]:
    while offset < len(data) and data[offset] in b" \t\r\n":
        offset += 1
    if offset < len(data) and data[offset] == ord("#"):
        while offset < len(data) and data[offset] not in b"\r\n":
            offset += 1
        return read_token(data, offset)
    start = offset
    while offset < len(data) and data[offset] not in b" \t\r\n":
        offset += 1
    return data[start:offset], offset


def read_ppm(path: Path) -> PpmImage:
    data = path.read_bytes()
    magic, offset = read_token(data, 0)
    if magic != b"P6":
        raise ValueError("only binary P6 PPM is supported")
    width_token, offset = read_token(data, offset)
    height_token, offset = read_token(data, offset)
    max_token, offset = read_token(data, offset)
    width = int(width_token)
    height = int(height_token)
    max_value = int(max_token)
    if width <= 0 or height <= 0 or width * height > MAX_PIXELS:
        raise ValueError("invalid or too-large PPM dimensions")
    if max_value != 255:
        raise ValueError("only max value 255 is supported")
    while offset < len(data) and data[offset] in b" \t\r\n":
        offset += 1
        break
    pixels = data[offset:]
    expected = width * height * 3
    if len(pixels) != expected:
        raise ValueError(f"pixel byte count mismatch: expected {expected}, got {len(pixels)}")
    return PpmImage(width, height, pixels)


def generate_image(width: int, height: int, pattern: str) -> PpmImage:
    if width <= 0 or height <= 0 or width * height > MAX_PIXELS:
        raise ValueError("invalid or too-large dimensions")
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            if pattern == "checker":
                value = 255 if (x // 8 + y // 8) % 2 == 0 else 32
                pixels.extend((value, value, value))
            elif pattern == "solid":
                pixels.extend((96, 144, 224))
            else:
                r = int(255 * x / max(1, width - 1))
                g = int(255 * y / max(1, height - 1))
                b = (r ^ g) & 0xFF
                pixels.extend((r, g, b))
    return PpmImage(width, height, bytes(pixels))


def transform_image(image: PpmImage, operation: str) -> PpmImage:
    if operation == "identity":
        return image
    if operation == "invert":
        return PpmImage(image.width, image.height, bytes(255 - value for value in image.pixels))
    raise ValueError(f"unsupported operation: {operation}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="PPM fixture tool for no-load AEX tests")
    sub = parser.add_subparsers(dest="command", required=True)

    gen = sub.add_parser("generate")
    gen.add_argument("--width", type=int, default=64)
    gen.add_argument("--height", type=int, default=64)
    gen.add_argument("--pattern", choices=["gradient", "checker", "solid"], default="gradient")
    gen.add_argument("--out", required=True)

    transform = sub.add_parser("transform")
    transform.add_argument("--input", required=True)
    transform.add_argument("--operation", choices=["identity", "invert"], required=True)
    transform.add_argument("--out", required=True)

    inspect = sub.add_parser("inspect")
    inspect.add_argument("--input", required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.command == "generate":
        image = generate_image(args.width, args.height, args.pattern)
        write_ppm_create_new(Path(args.out), image)
        print(args.out)
    elif args.command == "transform":
        image = read_ppm(Path(args.input))
        write_ppm_create_new(Path(args.out), transform_image(image, args.operation))
        print(args.out)
    elif args.command == "inspect":
        image = read_ppm(Path(args.input))
        print(f"ppm width={image.width} height={image.height} bytes={len(image.pixels)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

