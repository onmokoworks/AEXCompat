from tests import source_owners

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "windows.rs"


def test_harness_exposes_approved_dependency_inspect_cli():
    source = source_owners.harness_windows_text()
    command = 'args[1] == "--inspect-experimental-with-deps"'

    assert command in source
    assert "resolve_dependency_closure" in source
    assert (
        "inspect_experimental_with_approved_dependencies_and_diagnostics"
        in source
    )
    assert '"dependency_closure"' in source
    assert '"plugin_identity"' in source
    assert '"size_bytes"' in source
    assert '"unresolved_imports"' in source
    assert '"rejected_import_name_count"' in source


def test_dependency_inspect_cli_requires_explicit_roots_and_keeps_default_route():
    source = source_owners.harness_windows_text()
    dependency_route = source.index(
        'args[1] == "--inspect-experimental-with-deps"'
    )
    default_route = source.index('args[1] == "--inspect-experimental"')

    assert dependency_route < default_route
    assert "const MAX_ROOTS: usize = aexcompat_broker::plugin_dependency_closure::MAX_SEARCH_ROOTS" in source
    assert "at most {MAX_ROOTS} total dependency search roots may be supplied" in source
    assert "dependency root is not a directory" in source
