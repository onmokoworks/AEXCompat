from tests import source_owners

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"

def test_harness_exposes_approved_dependency_inspect_cli():
    source = source_owners.harness_windows_text()
    command = 'args[1] == "--inspect-experimental-with-deps"'

    assert command in source

def test_dependency_inspect_cli_requires_explicit_roots_and_keeps_default_route():
    source = source_owners.harness_windows_text()
    dependency_route = source.index(
        'args[1] == "--inspect-experimental-with-deps"'
    )
    default_route = source.index('args[1] == "--inspect-experimental"')

    assert dependency_route < default_route
