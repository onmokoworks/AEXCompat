import codecs
import re
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
INSTRUMENTS = ROOT / "instruments"

PROPERTY_KEYS = {
    "display_name": "eman",
    "match_name": "ANMe",
    "entry_point": "4668",
}

PIPL_STRING_PROPERTY = re.compile(
    r'"(?:eman|gtac|ANMe)"\s*,\s*0\s*,\s*0x0\s*,\s*(\d+)\s*,\s*0x0\s*,\s*"((?:\\.|[^"\\])*)"'
)

# Entry points are resolved inside each AEX module, so the conventional PF export
# is intentionally shared. Display and match names have no equivalent exception.
INTENTIONAL_SHARED_IDENTITIES = {
    "entry_point": {
        "EffectMain": "module-local standard PF entry point",
    },
}


def _decode_pipl_string(value: str) -> str:
    decoded = codecs.decode(value, "unicode_escape")
    # PiPL strings carry a leading Pascal-style byte. Some checked-in resources
    # encode a stale length, so identify it by syntax rather than trusting it.
    if re.match(r"^(?:\\x[0-9A-Fa-f]{2}|\\[0-7]{1,3})", value):
        decoded = decoded[1:]
    return decoded.rstrip("\0")


def parse_pipl_identity(path: Path) -> dict[str, str]:
    source = path.read_text(encoding="utf-8")
    identity = {}
    for field, key in PROPERTY_KEYS.items():
        match = re.search(
            rf'"{re.escape(key)}"\s*,\s*0\s*,\s*0x0\s*,\s*\d+\s*,\s*0x0\s*,\s*"((?:\\.|[^"\\])*)"',
            source,
        )
        if match is None:
            raise AssertionError(f"{path.relative_to(ROOT)}: missing PiPL {field} ({key})")
        identity[field] = _decode_pipl_string(match.group(1))
    return identity


def _decode_pipl_bytes(value: str) -> bytes:
    decoded = bytearray()
    index = 0
    while index < len(value):
        if value[index] != "\\":
            decoded.extend(value[index].encode("ascii"))
            index += 1
            continue
        if index + 1 >= len(value):
            raise AssertionError(f"unterminated PiPL escape: {value!r}")
        if value[index + 1] == "x":
            escape = value[index + 2 : index + 4]
            if len(escape) != 2 or not re.fullmatch(r"[0-9A-Fa-f]{2}", escape):
                raise AssertionError(f"invalid PiPL hex escape: {value!r}")
            decoded.append(int(escape, 16))
            index += 4
            continue
        if value[index + 1] in "01234567":
            end = index + 1
            while end < min(index + 4, len(value)) and value[end] in "01234567":
                end += 1
            decoded.append(int(value[index + 1 : end], 8))
            index = end
            continue
        raise AssertionError(f"unsupported PiPL escape: {value!r}")
    return bytes(decoded)


def instrument_pipl_identities() -> dict[Path, dict[str, str]]:
    resources = sorted(INSTRUMENTS.rglob("*.rc"))
    pipl_resources = [
        path
        for path in resources
        if re.search(r"\bPiPL\b", path.read_text(encoding="utf-8"))
    ]
    assert pipl_resources, "no instrument PiPL resources found"
    return {path: parse_pipl_identity(path) for path in pipl_resources}


def duplicate_identities(
    identities: dict[Path, dict[str, str]],
) -> dict[str, dict[str, list[Path]]]:
    duplicates = {}
    for field in PROPERTY_KEYS:
        by_value = defaultdict(list)
        for path, identity in identities.items():
            by_value[identity[field]].append(path)
        duplicates[field] = {
            value: paths for value, paths in by_value.items() if len(paths) > 1
        }
    return duplicates


def _unexpected_duplicate_message(duplicates: dict[str, dict[str, list[Path]]]) -> str:
    problems = []
    for field, values in duplicates.items():
        allowed = INTENTIONAL_SHARED_IDENTITIES.get(field, {})
        for value, paths in values.items():
            if value in allowed:
                continue
            locations = ", ".join(path.relative_to(ROOT).as_posix() for path in paths)
            problems.append(f"{field} {value!r}: {locations}")
    return "\n".join(problems)


def test_all_instrument_pipl_identities_are_complete_and_unique():
    identities = instrument_pipl_identities()
    duplicates = duplicate_identities(identities)

    for path, identity in identities.items():
        assert all(identity.values()), f"{path.relative_to(ROOT)}: empty PiPL identity field"

    message = _unexpected_duplicate_message(duplicates)
    assert not message, "unexpected instrument PiPL identity duplicates:\n" + message


def test_all_instrument_pipl_string_lengths_and_pascal_prefixes_match():
    failures = []
    for path in sorted(INSTRUMENTS.rglob("*.rc")):
        source = path.read_text(encoding="utf-8")
        for match in PIPL_STRING_PROPERTY.finditer(source):
            declared = int(match.group(1))
            payload = _decode_pipl_bytes(match.group(2))
            prefix = payload[0] if payload else None
            text = payload[1:].split(b"\0", 1)[0] if payload else b""
            if declared != len(payload) or prefix != len(text):
                failures.append(
                    f"{path.relative_to(ROOT)}: declared={declared} bytes={len(payload)} "
                    f"prefix={prefix} text={len(text)}"
                )
    assert not failures, "PiPL string property drift:\n" + "\n".join(failures)


def test_intentional_entry_point_sharing_is_narrow_and_detected():
    duplicates = duplicate_identities(instrument_pipl_identities())
    allowed = INTENTIONAL_SHARED_IDENTITIES["entry_point"]

    assert set(duplicates["entry_point"]) == set(allowed), (
        "entry-point sharing changed; classify every shared value explicitly: "
        f"detected={sorted(duplicates['entry_point'])}, allowed={sorted(allowed)}"
    )
    assert all(allowed.values()), "intentional identity sharing requires a rationale"
