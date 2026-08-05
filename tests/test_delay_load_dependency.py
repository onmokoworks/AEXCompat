from tests import source_owners
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def test_auto_render_approves_adjacent_delay_load_dependencies_before_dispatch():
    harness = source_owners.harness_windows_text()
