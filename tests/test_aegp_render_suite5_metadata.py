from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
SOURCE = source_owners.L2_MAIN
RECEIPTS = ROOT / "minihost" / "src" / "worker_render_receipts.cpp"
EXTERNAL_RUNTIME = ROOT / "minihost" / "src" / "worker_aegp_external_render_runtime.cpp"


def test_render_suite5_metadata_uses_exact_frozen_abi():
    text = source_owners.worker_text()
    assert "struct AegpTimeStamp" in text
    assert "sizeof(AegpTimeStamp) == 4" in text
    assert "sizeof(AegpRenderSuite5) == 14 * sizeof(void*)" in text
    assert "offsetof(AegpRenderSuite5, guid) == 13 * sizeof(void*)" in text


def test_render_sufficiency_requires_equivalent_options_and_roi_coverage():
    text = source_owners.worker_text()
    declaration = text.index("int32_t __cdecl render_sufficient_reject(")
    start = text.index("int32_t __cdecl render_sufficient_reject(", declaration + 1)
    body = text[start:text.index("int32_t __cdecl render_sound_reject", start)]
    for marker in (
        "snapshot_render_options(rendered, first)",
        "snapshot_render_options(proposed, second)",
        "first.item == second.item",
        "same_rational(first.time, second.time)",
        "first.world_type == second.world_type",
        "rendered_roi.left <= proposed_roi.left",
        "rendered_roi.bottom >= proposed_roi.bottom",
    ):
        assert marker in body


def test_timestamp_change_and_worthwhile_queries_share_one_project_epoch():
    host = SOURCE.read_text(encoding="utf-8")
    text = EXTERNAL_RUNTIME.read_text(encoding="utf-8")
    for marker in (
        "g_project_generation{1}",
        "bump_project_generation()",
        "store_timestamp(output, g_project_generation.load())",
        "observed != g_project_generation.load()",
        "g_timestamp_exhausted.load()",
        "!g_hooks.valid_item(item)",
    ):
        assert marker in text
    for marker in ("ExternalRenderedFrame", "g_project_generation{1}",
                   "g_timestamp_exhausted", "g_cache"):
        assert marker not in host


def test_receipt_guid_is_stable_owned_memory_and_rejects_stale_receipts():
    text = source_owners.worker_text()
    declaration = text.index("int32_t __cdecl render_guid_reject(")
    start = text.index("int32_t __cdecl render_guid_reject(", declaration + 1)
    body = text[start:text.index("bool world_lifetimes_balanced();", start)]
    for marker in (
        "render_receipts::snapshot(receipt, snapshot)",
        "snapshot.guid",
        'new_aegp_mem_handle(1, "render receipt guid"',
        "lock_aegp_mem_handle(*out, &bytes)",
        "unlock_aegp_mem_handle(*out)",
    ):
        assert marker in body
