from tests import source_owners
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]








def test_auto_render_approves_adjacent_delay_load_dependencies_before_dispatch():
    harness = source_owners.harness_windows_text()
    assert "fn approved_adjacent_dependencies(" in harness
    assert "discover_adjacent_imports(&aex_path)?" in harness
    assert "inspect_experimental_with_approved_dependencies_and_diagnostics" in harness
    assert "render_experimental_image_with_approved_dependencies" in harness
    assert "render_experimental_image_with_approved_dependencies_and_deep16_png" in harness
    assert "let use_approved_dependencies = auto_path || !approved_dependencies.is_empty();" in harness
    assert "approved_dependencies.clone()" in harness
