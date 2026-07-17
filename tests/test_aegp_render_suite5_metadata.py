from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


def test_render_suite5_metadata_uses_exact_frozen_abi():
    text = SOURCE.read_text(encoding="utf-8")
    assert "struct AegpTimeStamp" in text
    assert "sizeof(AegpTimeStamp) == 4" in text
    assert "sizeof(AegpRenderSuite5) == 14 * sizeof(void*)" in text
    assert "offsetof(AegpRenderSuite5, guid) == 13 * sizeof(void*)" in text


def test_render_sufficiency_requires_equivalent_options_and_roi_coverage():
    text = SOURCE.read_text(encoding="utf-8")
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
    text = SOURCE.read_text(encoding="utf-8")
    for marker in (
        "g_render_project_timestamp{1}",
        "bump_render_project_timestamp()",
        "store_render_timestamp(timestamp, g_render_project_timestamp.load())",
        "observed != g_render_project_timestamp.load()",
        "observed == g_render_project_timestamp.load()",
        "item != aegp_comp_item_handle()",
    ):
        assert marker in text


def test_receipt_guid_is_stable_owned_memory_and_rejects_stale_receipts():
    text = SOURCE.read_text(encoding="utf-8")
    declaration = text.index("int32_t __cdecl render_guid_reject(")
    start = text.index("int32_t __cdecl render_guid_reject(", declaration + 1)
    body = text[start:text.index("int32_t aegp_world_type_from_format", start)]
    for marker in (
        "g_async_receipts.find(receipt)",
        "guid = found->second->guid",
        'new_aegp_mem_handle(1, "render receipt guid"',
        "lock_aegp_mem_handle(*out, &bytes)",
        "unlock_aegp_mem_handle(*out)",
    ):
        assert marker in body
