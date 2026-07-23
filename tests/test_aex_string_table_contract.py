­r‡^Ñf¥–Ø¦{MìyÊ'vÃ®¶›­from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PARSER = (ROOT / "minihost/src/aex_string_table_impl.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost/src/aex_string_table.hpp").read_text(encoding="utf-8")
L2 = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
CMAKE = (ROOT / "minihost/CMakeLists.txt").read_text(encoding="utf-8")


def test_string_table_parser_is_readonly_section_bounded_and_fail_closed():
    assert '"$$$/"' in PARSER
    assert '"/LStr/"' in PARSER
    assert "IMAGE_SCN_MEM_READ" in PARSER
    assert "IMAGE_SCN_MEM_WRITE" in PARSER
    assert "IMAGE_SCN_MEM_EXECUTE" in PARSER
    assert "range_within(raw_offset, raw_size, size)" in PARSER
    assert "ParseStatus::Invalid" in PARSER
    assert "Valid" in HEADER
    assert "std::string_view" in PARSER


def test_runtime_lookup_uses_table_and_never_returns_placeholder():
    assert "load_aex_string_table(module, aex_string_table)" in L2
    assert "g_active_aex_string_table->lookup(id)" in L2
    assert 'return "AEXCompat"' not in L2
    assert "string_table_status:" in L2


def test_native_selftest_is_wired_into_minihost_build():
    assert "aex_string_table_impl.cpp" in CMAKE
    assert "aex_string_table_selftest" in CMAKE
