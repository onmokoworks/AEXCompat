from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESOLVER = (
    ROOT / "broker" / "crates" / "broker" / "src" / "plugin_dependency_closure.rs"
).read_text(encoding="utf-8")
MULTIFILTER = (
    ROOT / "bridges" / "aviutl2-multifilter" / "src" / "lib.rs"
).read_text(encoding="utf-8")
SWEEP = (
    ROOT / "bridges" / "aviutl2-multifilter" / "examples" / "discover_sweep.rs"
).read_text(encoding="utf-8")


def test_runtime_dll_literals_stay_generic_and_root_bounded() -> None:
    assert "runtime_dll_literal_names(bytes, &imported)" in RESOLVER
    assert "CandidateOrigin::StringLiteral" in RESOLVER
    assert "resolve_name(&name, roots)?" in RESOLVER
    assert 'eq_ignore_ascii_case(".dll")' in RESOLVER
    assert "u16::from_le_bytes" in RESOLVER
    assert "ippcv" not in RESOLVER.lower()


def test_dependency_origin_reaches_persistent_and_sweep_diagnostics() -> None:
    for field in ("basename", "import_derived", "string_derived"):
        assert f"pub {field}:" in RESOLVER
        assert field in MULTIFILTER
        assert field in SWEEP
    assert "let provenance = cached_provenance(closure.provenance());" in MULTIFILTER
    assert '"dependency_provenance"' in SWEEP
