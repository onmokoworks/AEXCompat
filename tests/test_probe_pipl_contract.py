import re
from functools import reduce
from operator import or_
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
INSTRUMENTS = ROOT / "instruments"

# The corrected #287 scope is intentionally exact. The three #286 fixtures are
# already covered by that closed issue, and the independently checked
# transform-multimatrix oracle already has a valid compiled PiPL.
PROBES = (
    "pf-adv-time-probe",
    "pf-ae-adv-item-probe",
    "pf-ae-channel-native-probe",
    "pf-aegp-async-cancel-probe",
    "pf-aegp-async-layer-receipt-probe",
    "pf-aegp-external-cache-roundtrip-probe",
    "pf-aegp-fast-blur-probe",
    "pf-aegp-layer-options-probe",
    "pf-aegp-layer-receipt-probe",
    "pf-aegp-owned-world-probe",
    "pf-convolve-depth-probe",
    "pf-fill-premultiply-probe",
    "pf-transfer-mask-probe",
    "pf-transfer-rect-probe",
    "pf-transform-affine-probe",
)

STRING_PROPERTY = re.compile(
    r'"MIB8",\s*"(?P<key>.{4})",\s*0(?:L)?,\s*(?:0x)?0(?:L)?,\s*'
    r'(?P<length>\d+)(?:L)?,\s*(?:0x)?0(?:L)?,\s*'
    r'"(?P<value>(?:\\.|[^"\\])*)"'
)
OLGE_PROPERTY = re.compile(r'"OLGe",\s*0L,\s*4L,\s*(?P<value>\d+)L')
OUT_FLAGS_ASSIGNMENT = re.compile(
    r"\b(?:out|out_data)->out_flags\s*=\s*(?P<value>[^;]+);"
)
PASCAL_PROPERTIES = {"eman", "gtac", "ANMe"}
OUT_FLAG_VALUES = {
    "PF_OutFlag_PIX_INDEPENDENT": 1 << 10,
    "PF_OutFlag_DEEP_COLOR_AWARE": 1 << 25,
}


def _single_source(probe: str, suffix: str) -> Path:
    paths = list((INSTRUMENTS / probe).glob(f"*{suffix}"))
    assert len(paths) == 1, f"{probe}: expected one {suffix} source, found {paths}"
    return paths[0]


def _decode_rc_string(value: str) -> bytes:
    decoded = bytearray()
    offset = 0
    escapes = {"n": 10, "r": 13, "t": 9, "\\": 92, '"': 34}
    while offset < len(value):
        if value[offset] != "\\":
            decoded.append(ord(value[offset]))
            offset += 1
            continue
        marker = value[offset + 1]
        if marker == "x":
            # rc.exe consumes exactly two hex digits for these PiPL fixtures.
            decoded.append(int(value[offset + 2 : offset + 4], 16))
            offset += 4
        elif marker in "01234567":
            end = offset + 2
            while end < min(offset + 4, len(value)) and value[end] in "01234567":
                end += 1
            decoded.append(int(value[offset + 1 : end], 8))
            offset = end
        else:
            decoded.append(escapes[marker])
            offset += 2
    return bytes(decoded)


def _runtime_out_flags(source: str) -> int:
    match = OUT_FLAGS_ASSIGNMENT.search(source)
    assert match is not None, "missing PF_Cmd_GLOBAL_SETUP out_flags assignment"
    tokens = [token.strip() for token in match.group("value").split("|")]
    unknown = set(tokens) - set(OUT_FLAG_VALUES)
    assert not unknown, f"classify new out_flags tokens before updating PiPL: {unknown}"
    return reduce(or_, (OUT_FLAG_VALUES[token] for token in tokens), 0)


@pytest.mark.parametrize("probe", PROBES)
def test_probe_pipl_string_properties_match_their_payloads(probe: str):
    source = _single_source(probe, ".rc").read_text(encoding="utf-8")
    properties = {
        match.group("key"): (int(match.group("length")), _decode_rc_string(match.group("value")))
        for match in STRING_PROPERTY.finditer(source)
    }
    assert {"dnik", "eman", "gtac", "4668", "ANMe"} <= properties.keys()

    for key, (declared_length, payload) in properties.items():
        assert declared_length == len(payload), (
            f"{probe} {key}: declared {declared_length}, actual {len(payload)}"
        )
        assert declared_length % 4 == 0, f"{probe} {key}: payload is not DWORD-aligned"
        if key in PASCAL_PROPERTIES:
            text_end = 1 + payload[0]
            assert text_end <= len(payload), f"{probe} {key}: Pascal length overflows payload"
            assert payload[text_end:] == bytes(len(payload) - text_end), (
                f"{probe} {key}: non-NUL Pascal padding"
            )


@pytest.mark.parametrize("probe", PROBES)
def test_probe_pipl_olge_matches_global_setup_out_flags(probe: str):
    rc_source = _single_source(probe, ".rc").read_text(encoding="utf-8")
    cpp_source = _single_source(probe, ".cpp").read_text(encoding="utf-8")
    olge = OLGE_PROPERTY.search(rc_source)
    assert olge is not None, f"{probe}: missing numeric OLGe property"
    assert int(olge.group("value")) == _runtime_out_flags(cpp_source)
