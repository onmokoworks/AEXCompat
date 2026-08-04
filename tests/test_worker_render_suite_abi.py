from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
HEADER = ROOT / "minihost" / "src" / "worker_suite_abi.hpp"
SOURCE = ROOT / "minihost" / "src" / "worker_suite_abi.cpp"
MAIN = source_owners.L2_SOURCE




def test_every_implementation_family_has_decltype_contract_checks():
    main = MAIN.read_text(encoding="utf-8")
    for callback in (
        "new_layer_render_options", "new_from_downstream_of_effect",
        "get_layer_render_matte", "render_options_new_from_item",
        "render_options_get_roi", "render_options_set_quality",
    ):
        assert f"decltype(&{callback})" in main
