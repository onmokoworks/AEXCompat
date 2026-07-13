#!/usr/bin/env python3
import hashlib
import math
import struct


MASK32 = 0xFFFFFFFF


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def hash_pixel(x: int, y: int, seed: int, channel: int) -> float:
    value = (x * 374761393 + y * 668265263 + seed * 2246822519 + channel * 3266489917) & MASK32
    value ^= value >> 13
    value = (value * 274177) & MASK32
    value ^= value >> 16
    value = (value * 1900813) & MASK32
    value ^= value >> 13
    ratio = f32(f32(float(value)) / f32(float(MASK32)))
    return f32(f32(ratio * f32(2.0)) - f32(1.0))


def rust_round(value: float) -> int:
    return math.floor(value + 0.5) if value >= 0 else math.ceil(value - 0.5)


def gradient(width: int, height: int) -> bytes:
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            pixels.extend((
                255,
                x * 255 // (width - 1),
                y * 255 // (height - 1),
                (x + y) * 255 // (width + height - 2),
            ))
    return bytes(pixels)


def render_default(width: int = 16, height: int = 12) -> bytes:
    source = gradient(width, height)
    output = bytearray(width * height * 4)
    amount = f32(5.0)
    for y in range(height):
        for x in range(width):
            dx = rust_round(f32(hash_pixel(x, y, 0, 0) * amount))
            dy = rust_round(f32(hash_pixel(x, y, 0, 1) * amount))
            sx = min(width - 1, max(0, x + dx))
            sy = min(height - 1, max(0, y + dy))
            source_offset = (sy * width + sx) * 4
            output_offset = (y * width + x) * 4
            output[output_offset:output_offset + 4] = source[source_offset:source_offset + 4]
    return bytes(output)


def hashes() -> tuple[str, str]:
    source = gradient(16, 12)
    output = render_default()
    return hashlib.sha256(source).hexdigest().upper(), hashlib.sha256(output).hexdigest().upper()


if __name__ == "__main__":
    input_hash, output_hash = hashes()
    print(f"input_sha256={input_hash}")
    print(f"output_sha256={output_hash}")
