import importlib.util
import struct
import zlib
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
DECODER_PATH = ROOT / "tools" / "decode-pf-path-curve-oracle.py"
SPEC = importlib.util.spec_from_file_location("pf_path_curve_decoder", DECODER_PATH)
DECODER = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
SPEC.loader.exec_module(DECODER)


def golden_stream() -> bytes:
    record = bytearray(DECODER.RECORD_SIZE)
    struct.pack_into("<iI", record, 0, -1, 9)
    struct.pack_into("<QiiQ", record, 8, 0x7FF8000000000000, -7, -8, DECODER.SENTINEL_BITS)
    record[32:37] = bytes((1, 1, 1, 1, 0))
    struct.pack_into("<Q", record, 40, DECODER.SENTINEL_BITS)
    struct.pack_into("<iQQiQQQQi", record, 48, -9, 1, 2, -10, 3, 4, 5, 6, -11)
    crc = zlib.crc32(record) & 0xFFFFFFFF
    return DECODER.HEADER.pack(
        DECODER.MAGIC, DECODER.VERSION, DECODER.HEADER.size,
        DECODER.RECORD_SIZE, 1, crc
    ) + record


def test_decoder_golden_layout_and_raw_bits():
    decoded = DECODER.decode_bytes(golden_stream())
    assert decoded["magic"] == "PFPCURV"
    assert decoded["record_size"] == 112
    assert decoded["crc32"] == "ef37337c"
    record = decoded["records"][0]
    assert record["frequency"] == -1
    assert record["length_case_name"] == "nan"
    assert record["requested_length"]["bits"] == "0x7ff8000000000000"
    assert record["sentinel_bits"] == "0x7ff4a5a5deadbeef"
    assert record["eval_error"] == -9
    assert record["deriv_error"] == -10
    assert record["prep_state"] == [1, 1, 1, 1, 0]
    assert record["cleanup_error"] == -11


def test_decoder_rejects_crc_corruption():
    damaged = bytearray(golden_stream())
    damaged[-1] ^= 1
    with pytest.raises(ValueError, match="CRC32 mismatch"):
        DECODER.decode_bytes(damaged)


def test_argb8_and_argb16_are_reversible_and_identical():
    data = golden_stream()
    data += b"\0" * (-len(data) % 4)
    pixels8 = [tuple(data[i:i + 4]) for i in range(0, len(data), 4)]
    pixels16 = [tuple(channel * 257 for channel in pixel) for pixel in pixels8]
    assert DECODER.argb_bytes(pixels8, 8) == data
    assert DECODER.argb_bytes(pixels16, 16) == data


def test_decoder_rejects_non_reversible_argb16_channel():
    with pytest.raises(ValueError, match=r"byte \* 257"):
        DECODER.argb_bytes([(0, 257, 514, 258)], 16)


def test_png_transport_requires_opaque_alpha(tmp_path):
    from PIL import Image

    image = Image.new("RGBA", (1, 1), (1, 2, 3, 254))
    path = tmp_path / "damaged.png"
    image.save(path)
    with pytest.raises(ValueError, match="fully opaque"):
        DECODER.decode_image(path)
