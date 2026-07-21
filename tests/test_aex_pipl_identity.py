"""Machine-portable self-tests for the static real PiPL identity parser.

These build synthetic PiPL resource payloads in memory (matching the Windows
byte layout emitted by the Adobe PiPL tool) and exercise the bounded, fail-closed
decode without any real .aex file, DLL load, or After Effects invocation.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

TOOLS_ROOT = Path(__file__).resolve().parents[1] / "tools"
if str(TOOLS_ROOT) not in sys.path:
    sys.path.insert(0, str(TOOLS_ROOT))

import aex_pipl_identity as ident


def _property(canonical_key: str, data: bytes, vendor: str = "8BIM") -> bytes:
    # On disk both the vendor and key OSTypes are stored little-endian, i.e.
    # their ASCII bytes are reversed relative to the canonical four-char code.
    block = vendor[::-1].encode("ascii") + canonical_key[::-1].encode("ascii")
    block += struct.pack("<I", 0)
    block += struct.pack("<I", len(data))
    block += data
    block += b"\x00" * ((-len(data)) % 4)
    return block


def _payload(properties: list[bytes]) -> bytes:
    header = struct.pack("<I", 1) + b"\x00\x00" + struct.pack("<H", len(properties)) + b"\x00\x00"
    return header + b"".join(properties)


def _kind(code: str) -> bytes:
    # Kind property value is itself a little-endian four-char code.
    return _property("kind", code[::-1].encode("ascii"))


def _effect_payload(symbol: str = "EffectMain") -> bytes:
    return _payload([
        _kind("eFKT"),
        _property("name", bytes([len("Demo")]) + b"Demo"),
        _property("catg", bytes([len("Sample")]) + b"Sample"),
        _property("eMNA", bytes([len("ADBE Demo")]) + b"ADBE Demo"),
        _property("eVER", struct.pack("<I", 1081345)),  # 2.1
        _property("8664", symbol.encode("ascii") + b"\x00"),
    ])


def test_effect_payload_parses_identity():
    record = ident.parse_pipl_payload(_effect_payload())
    assert record["parse_state"] == "parsed"
    assert record["kind_code"] == "eFKT"
    assert record["kind_label"] == "AEEffect"
    assert record["entrypoint_win64"] == "EffectMain"
    assert record["name"] == "Demo"
    assert record["category"] == "Sample"
    assert record["match_name"] == "ADBE Demo"
    assert record["version"]["display"] == "2.1.0"
    assert record["version"]["vers"] == 2 and record["version"]["subvers"] == 1


def test_lowercase_entrypoint_symbol_is_valid():
    record = ident.parse_pipl_payload(_effect_payload(symbol="entryPointFunc"))
    assert record["parse_state"] == "parsed"
    assert record["entrypoint_win64"] == "entryPointFunc"


def test_aegp_kind_is_not_effect():
    payload = _payload([_kind("AEgx"), _property("8664", b"EntryPointFunc\x00")])
    record = ident.parse_pipl_payload(payload)
    assert record["parse_state"] == "parsed"
    assert record["kind_code"] == "AEgx"
    assert record["kind_label"] == "AEGP"


def test_non_adobe_vendor_property_is_ignored_but_bounded():
    payload = _payload([
        _kind("eFKT"),
        _property("8664", b"EffectMain\x00"),
        _property("nmXX", b"opaque-private-bytes", vendor="VEND"),
    ])
    record = ident.parse_pipl_payload(payload)
    assert record["parse_state"] == "parsed"
    assert record["kind_code"] == "eFKT"
    # the private vendor block is recorded with a vendor prefix, not decoded
    assert any(key.startswith("VEND:") for key in record["property_keys"])


def test_bad_header_is_invalid():
    payload = bytearray(_effect_payload())
    payload[4] = 0x01  # reserved byte must be zero
    record = ident.parse_pipl_payload(bytes(payload))
    assert record["parse_state"] == "invalid"
    assert record["reason"] == "bad_pipl_header"


def test_truncated_payload_is_invalid():
    payload = _effect_payload()[:-1]
    record = ident.parse_pipl_payload(payload)
    assert record["parse_state"] == "invalid"


def test_length_overflow_is_invalid():
    payload = bytearray(_effect_payload())
    # Corrupt the first property's length field (offset 10 + 12) to overflow.
    struct.pack_into("<I", payload, 10 + 12, 0xFFFFFFFF)
    record = ident.parse_pipl_payload(bytes(payload))
    assert record["parse_state"] == "invalid"
    assert record["reason"] in {"property_length_overflow", "property_padding_overflow"}


def test_nonzero_padding_is_invalid():
    # name "Sam" -> length 4 (1 length byte + 3 chars) needs no pad; use "Sa" to force pad.
    payload = _payload([
        _kind("eFKT"),
        _property("8664", b"EffectMain\x00"),
        _property("name", bytes([2]) + b"Sa"),  # 3 bytes -> padded to 4
    ])
    payload = bytearray(payload)
    payload[-1] = 0x7F  # corrupt the pad byte
    record = ident.parse_pipl_payload(bytes(payload))
    assert record["parse_state"] == "invalid"
    assert record["reason"] == "nonzero_property_padding"


def test_short_payload_is_invalid():
    assert ident.parse_pipl_payload(b"")["reason"] == "payload_shorter_than_header"
    assert ident.parse_pipl_payload(None)["reason"] == "no_payload"


def test_classify_single_effect():
    assert ident._classify([ident.parse_pipl_payload(_effect_payload())]) == "effect"


def test_classify_multiple_effect_is_ambiguous():
    records = [ident.parse_pipl_payload(_effect_payload()) for _ in range(2)]
    assert ident._classify(records) == "ambiguous_multiple_effect"


def test_classify_effect_and_aegp_is_ambiguous():
    aegp = _payload([_kind("AEgx"), _property("8664", b"EntryPointFunc\x00")])
    records = [ident.parse_pipl_payload(_effect_payload()), ident.parse_pipl_payload(aegp)]
    assert ident._classify(records) == "ambiguous_effect_and_aegp"


def test_classify_effect_missing_entrypoint():
    payload = _payload([
        _kind("eFKT"),
        _property("name", bytes([len("Demo")]) + b"Demo"),
    ])
    assert ident._classify([ident.parse_pipl_payload(payload)]) == "effect_missing_win64_entrypoint"


def test_classify_invalid_when_any_record_invalid():
    good = ident.parse_pipl_payload(_effect_payload())
    bad = ident.parse_pipl_payload(_effect_payload()[:-1])
    assert ident._classify([good, bad]) == "invalid_pipl"
