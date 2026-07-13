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


def render_case(width: int = 16, height: int = 12, *, amount: int = 5,
                direction: int = 3, seed: int = 0, repeat_edge: bool = True,
                mix: float = 100.0, luma_map: list[float] | None = None) -> bytes:
    source = gradient(width, height)
    output = bytearray(width * height * 4)
    amount_f = f32(float(amount))
    for y in range(height):
        for x in range(width):
            output_offset = (y * width + x) * 4
            pixel_amount = amount_f if luma_map is None else f32(amount_f * f32(luma_map[y * width + x]))
            dx = rust_round(f32(hash_pixel(x, y, seed, 0) * pixel_amount)) if direction in (1, 3) else 0
            dy = rust_round(f32(hash_pixel(x, y, seed, 1) * pixel_amount)) if direction in (2, 3) else 0
            raw_x, raw_y = x + dx, y + dy
            if not repeat_edge and not (0 <= raw_x < width and 0 <= raw_y < height):
                output[output_offset:output_offset + 4] = b"\0\0\0\0"
            else:
                sx = min(width - 1, max(0, raw_x))
                sy = min(height - 1, max(0, raw_y))
                source_offset = (sy * width + sx) * 4
                output[output_offset:output_offset + 4] = source[source_offset:source_offset + 4]
    if mix < 100.0:
        ratio = f32(mix / 100.0)
        inverse = f32(1.0 - ratio)
        for index in range(len(output)):
            value = f32(f32(f32(float(source[index])) * inverse) + f32(f32(float(output[index])) * ratio))
            output[index] = min(255, max(0, int(value)))
    return bytes(output)


def render_default(width: int = 16, height: int = 12) -> bytes:
    return render_case(width, height)


def gradient16(width: int, height: int) -> bytes:
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            pixels.extend(struct.pack("<4H", 32768, x * 32768 // (width - 1),
                                      y * 32768 // (height - 1),
                                      (x + y) * 32768 // (width + height - 2)))
    return bytes(pixels)


def render_default16(width: int = 16, height: int = 12) -> bytes:
    # The current fixture advertises deep color but its layer helpers copy
    # width*4 bytes per row. Model that observable behavior exactly.
    world = gradient16(width, height)
    source = b"".join(world[y * width * 8:y * width * 8 + width * 4]
                      for y in range(height))
    compact = bytearray(width * height * 4)
    for y in range(height):
        for x in range(width):
            dx = rust_round(f32(hash_pixel(x, y, 0, 0) * f32(5.0)))
            dy = rust_round(f32(hash_pixel(x, y, 0, 1) * f32(5.0)))
            sx, sy = min(width - 1, max(0, x + dx)), min(height - 1, max(0, y + dy))
            dst, src = (y * width + x) * 4, (sy * width + sx) * 4
            compact[dst:dst + 4] = source[src:src + 4]
    output = bytearray([0xCC] * width * height * 8)
    for y in range(height):
        output[y * width * 8:y * width * 8 + width * 4] = compact[y * width * 4:(y + 1) * width * 4]
    return bytes(output)


def render_default32f(width: int = 16, height: int = 12) -> bytes:
    rows = []
    for y in range(height):
        row = bytearray()
        for x in range(width):
            row.extend(struct.pack("<4f", 1.0, x / (width - 1), y / (height - 1),
                                   (x + y) / (width + height - 2)))
        rows.append(bytes(row))
    # The fixture's helpers copy width*4 bytes from each ARGB128 row.
    source = b"".join(row[:width * 4] for row in rows)
    compact = bytearray(width * height * 4)
    for y in range(height):
        for x in range(width):
            dx = rust_round(f32(hash_pixel(x, y, 0, 0) * f32(5.0)))
            dy = rust_round(f32(hash_pixel(x, y, 0, 1) * f32(5.0)))
            sx, sy = min(width - 1, max(0, x + dx)), min(height - 1, max(0, y + dy))
            dst, src = (y * width + x) * 4, (sy * width + sx) * 4
            compact[dst:dst + 4] = source[src:src + 4]
    output = bytearray([0xCC] * width * height * 16)
    for y in range(height):
        output[y * width * 16:y * width * 16 + width * 4] = compact[y * width * 4:(y + 1) * width * 4]
    return bytes(output)


def generated_luma_map(width: int, height: int, map_width: int, map_height: int,
                       invert: bool = False) -> list[float]:
    source = []
    for y in range(map_height):
        for x in range(map_width):
            value = (x + y) * 255 // (map_width + map_height - 2)
            luma = (0.2126 * value + 0.7152 * value + 0.0722 * value) / 255.0
            source.append(1.0 - luma if invert else luma)
    result = []
    for y in range(height):
        sy = min(map_height - 1, y * map_height // height)
        for x in range(width):
            sx = min(map_width - 1, x * map_width // width)
            result.append(source[sy * map_width + sx])
    return result


def hashes() -> tuple[str, str]:
    source = gradient(16, 12)
    output = render_default()
    return hashlib.sha256(source).hexdigest().upper(), hashlib.sha256(output).hexdigest().upper()


if __name__ == "__main__":
    input_hash, output_hash = hashes()
    print(f"input_sha256={input_hash}")
    print(f"output_sha256={output_hash}")
