from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "minihost" / "src" / "extended_inter_memory.cpp").read_text(
    encoding="utf-8"
)


def test_extended_inter_alloc_fails_closed_and_accepts_zero_size():
    assert "*out = nullptr" in SOURCE
    assert "size > kMaxAllocation" in SOURCE
    assert "std::max<std::size_t>(size, 1)" in SOURCE
    assert "std::free(allocation)" in SOURCE


def test_extended_inter_free_only_releases_host_owned_allocations():
    assert "g_owned_allocations" in SOURCE
    assert "if (!forget(allocation)) return 4" in SOURCE
    assert "*ptr = nullptr" in SOURCE
