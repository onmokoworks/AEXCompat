"""MSVC must not lose a line to a multi-byte comment.

MSVC reads a source file with no BOM and no `/utf-8` as the current code page.
On a Japanese-locale Windows that is cp932, where `0x81-0x9F` and `0xE0-0xFC`
are lead bytes: a UTF-8 character whose last byte is one of those, sitting
immediately before a bare LF, pairs with the newline. A line comment then runs
into the next line and the declaration there disappears.

That is not hypothetical - it removed `constexpr int32_t kChannelMaskArgb` from
`worker_smart_dispatch.cpp` and broke the minihost build on a clean checkout
(issue #708). It survived review and CI because the author's working copy had
CRLF, where the `0x0D` absorbs the lead byte and the LF still ends the line,
and because neither CI job builds minihost.

The check normalizes CRLF to LF first, so it sees what git stores
(`core.autocrlf=input`) and fails the same way the compiler would, whatever the
working copy happens to hold.
"""

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIR = ROOT / "minihost" / "src"
SOURCE_SUFFIXES = {".c", ".cpp", ".h", ".hpp", ".inc"}
# A cp932 lead byte immediately before a bare LF: the pair swallows the newline.
HAZARD = re.compile(rb"[\x81-\x9f\xe0-\xfc]\n")


def test_no_multibyte_byte_sits_immediately_before_a_bare_newline() -> None:
    offenders = []
    for path in sorted(SOURCE_DIR.rglob("*")):
        if path.suffix.lower() not in SOURCE_SUFFIXES:
            continue
        data = path.read_bytes().replace(b"\r\n", b"\n")
        for match in HAZARD.finditer(data):
            line = data[: match.start()].count(b"\n") + 1
            offenders.append(f"{path.relative_to(ROOT).as_posix()}:{line}")
    assert not offenders, (
        "a cp932 lead byte sits immediately before a newline, so MSVC on a "
        "Japanese-locale Windows loses the following line: " + ", ".join(offenders)
    )


def test_the_check_would_catch_the_shape_that_broke_the_build(tmp_path) -> None:
    # The exact shape from issue #708: a comment ending in a full-width stop,
    # then the declaration that vanished.
    broken = "// SmartRender が同じ値を見るべき。\nconstexpr int x = 0;\n"
    assert HAZARD.search(broken.encode("utf-8"))
    # The same text with CRLF does not trip the compiler, and must not trip the
    # check either once it is normalized - otherwise the check would fire on
    # every Japanese comment rather than on the hazardous placement.
    assert HAZARD.search(broken.encode("utf-8").replace(b"\n", b"\r\n")) is None
    # Plain ASCII, and a multi-byte character that is not last on its line, are
    # both fine.
    assert HAZARD.search(b"constexpr int x = 0;\n") is None
    assert HAZARD.search("// 。 tail\nint x;\n".encode("utf-8")) is None
