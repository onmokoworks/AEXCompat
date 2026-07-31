from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
MAIN = source_owners.L2_MAIN.read_text(encoding="utf-8")
HEADER = (ROOT / "minihost" / "src" / "worker_render_receipts.hpp").read_text(
    encoding="utf-8")
SOURCE = (ROOT / "minihost" / "src" / "worker_render_receipts.cpp").read_text(
    encoding="utf-8")
CMAKE = (ROOT / "minihost" / "CMakeLists.txt").read_text(encoding="utf-8")
REGISTRATION_SOURCES = MAIN + "\n" + "\n".join(
    (ROOT / "minihost" / "src" / name).read_text(encoding="utf-8")
    for name in (
        "worker_aegp_staged_item_runtime.cpp",
        "worker_aegp_external_render_runtime.cpp",
        "worker_aegp_async_layer_runtime.cpp",
        "worker_aegp_layer_render_runtime.cpp",
        "worker_aegp_item_render_runtime.cpp",
    )
)


def test_receipt_registry_is_a_compiled_owner_with_explicit_binding_paths():
    assert CMAKE.count("src/worker_render_receipts.cpp") == 1
    assert '#include "worker_render_receipts.hpp"' in MAIN
    assert "struct ReceiptDraft" in HEADER
    assert "std::unordered_map<void*, std::unique_ptr<Receipt>> g_receipts" in SOURCE
    assert "g_receipts" not in MAIN
    assert REGISTRATION_SOURCES.count(
        "render_receipts::register_scene_receipt("
    ) == 3
    assert REGISTRATION_SOURCES.count(
        "render_receipts::register_unbound_receipt("
    ) == 2
    assert "render_receipts::register_receipt(" not in REGISTRATION_SOURCES
    assert "register_receipt_impl" in SOURCE


def test_registry_never_calls_world_registry_while_holding_receipt_mutex():
    registration = SOURCE[SOURCE.index("int32_t register_receipt_impl("):
                          SOURCE.index("int32_t get_world(")]
    assert registration.index("g_receipts.emplace(key") < registration.index(
        "register_borrowed_view(")
    assert "published = true" in registration
    assert "g_reserved_count" in registration
    checkin = SOURCE[SOURCE.index("int32_t checkin("):
                     SOURCE.index("bool checkin_if_live(")]
    assert checkin.index("g_receipts.extract(found)") < checkin.index(
        "unregister_borrowed_view(")
    assert "ownership_mismatch" in checkin


def test_inflight_admission_and_unregister_outcomes_are_explicit():
    assert "g_live_bytes + g_reserved_bytes" in SOURCE
    assert "reserved_count" in HEADER
    world_header = (ROOT / "minihost" / "src" /
                    "worker_world_registry.hpp").read_text(encoding="utf-8")
    for outcome in ("removed", "already_absent", "ownership_mismatch"):
        assert outcome in world_header


def test_stats_bounds_snapshots_and_opaque_generations_are_owned_by_service():
    assert "kMaxReceiptCount = 32" in HEADER
    assert "kMaxReceiptBytes = 64ULL * 1024 * 1024" in HEADER
    assert "g_receipt_generation{1}" in SOURCE
    assert "g_world_generation{1}" in SOURCE
    assert "ReceiptSnapshot" in HEADER
    for api in ("get_world", "checkin", "checkin_if_live", "snapshot",
                "statistics", "lifetimes_balanced"):
        assert api in HEADER + SOURCE
