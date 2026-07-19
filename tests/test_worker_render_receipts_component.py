from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAIN = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
HEADER = (ROOT / "minihost" / "src" / "worker_render_receipts.hpp").read_text(
    encoding="utf-8")
SOURCE = (ROOT / "minihost" / "src" / "worker_render_receipts.cpp").read_text(
    encoding="utf-8")
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")


def test_receipt_registry_is_a_compiled_owner_with_one_registration_path():
    assert CMAKE.count("src/worker_render_receipts.cpp") == 1
    assert '#include "worker_render_receipts.hpp"' in MAIN
    assert "struct ReceiptDraft" in HEADER
    assert "std::unordered_map<void*, std::unique_ptr<Receipt>> g_receipts" in SOURCE
    assert "g_receipts" not in MAIN
    assert MAIN.count("render_receipts::register_receipt(") == 4


def test_registry_never_calls_world_registry_while_holding_receipt_mutex():
    registration = SOURCE[SOURCE.index("int32_t register_receipt("):
                          SOURCE.index("int32_t get_world(")]
    assert registration.index("register_borrowed_view(") < registration.index(
        "bool committed")
    checkin = SOURCE[SOURCE.index("int32_t checkin("):
                     SOURCE.index("bool checkin_if_live(")]
    assert checkin.index("g_receipts.extract(found)") < checkin.index(
        "unregister_borrowed_view(")
    assert "g_receipts.insert(std::move(receipt))" in checkin


def test_stats_bounds_snapshots_and_opaque_generations_are_owned_by_service():
    assert "kMaxReceiptCount = 32" in HEADER
    assert "kMaxReceiptBytes = 64ULL * 1024 * 1024" in HEADER
    assert "g_receipt_generation{1}" in SOURCE
    assert "g_world_generation{1}" in SOURCE
    assert "ReceiptSnapshot" in HEADER
    for api in ("get_world", "checkin", "checkin_if_live", "snapshot",
                "statistics", "lifetimes_balanced"):
        assert api in HEADER + SOURCE
