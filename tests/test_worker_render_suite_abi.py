from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
SOURCE = ROOT / "minihost" / "src" / "worker_suite_abi.cpp"
MAIN = source_owners.L2_SOURCE
def test_render_suite_cluster_is_owned_by_worker_suite_abi():
    header = HEADER.read_text(encoding="utf-8")
    source = SOURCE.read_text(encoding="utf-8")
    main = MAIN.read_text(encoding="utf-8")
    for suite, slots in (
        ("AegpLayerRenderOptionsSuite1", 14),
        ("AegpLayerRenderOptionsSuite2", 15),
        ("AegpRenderOptionsSuite1", 17),
        ("AegpRenderOptionsSuite4", 23),
    ):
        assert f"struct {suite}" in header
        assert f"sizeof({suite}) == {slots} * sizeof(void*)" in header
        assert f"alignof({suite}) == alignof(void*)" in header
        assert f"struct {suite}" not in main
    assert "#if 0  // Callback implementations moved" not in main
    assert "g_render_options_mutex" not in main
    assert "g_layer_render_options_mutex" not in main
    assert "g_aegp_layer_render_options_suite1" in source
    assert "g_aegp_render_options_suite4" in source


def test_clean_room_value_abi_and_explicit_calling_convention_are_frozen():
    header = HEADER.read_text(encoding="utf-8")
    assert "sizeof(AegpTime) == 8" in header
    assert "offsetof(AegpTime, scale) == 4" in header
    assert "sizeof(AegpRect) == 16" in header
    assert "offsetof(AegpRect, bottom) == 12" in header
    assert header.count("(__cdecl*)") >= 20


def test_every_implementation_family_has_decltype_contract_checks():
    main = MAIN.read_text(encoding="utf-8")
    for callback in (
        "new_layer_render_options", "new_from_downstream_of_effect",
        "get_layer_render_matte", "render_options_new_from_item",
        "render_options_get_roi", "render_options_set_quality",
    ):
        assert f"decltype(&{callback})" in main
