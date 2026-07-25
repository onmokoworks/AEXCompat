from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESOLVER = (
    ROOT / "broker/crates/broker/src/plugin_dependency_closure.rs"
).read_text(encoding="utf-8")
BROKER = (ROOT / "broker/crates/broker/src/image_render.rs").read_text(
    encoding="utf-8"
)
SWEEP = (
    ROOT / "bridges/aviutl2-multifilter/examples/discover_sweep.rs"
).read_text(encoding="utf-8")
ADMISSION = (ROOT / "minihost/src/worker_runtime_admission.cpp").read_text(
    encoding="utf-8"
)


def test_dependency_diagnostics_are_bounded_and_path_free_by_contract():
    assert "MAX_DEPENDENCY_DIAGNOSTICS: usize = 64" in RESOLVER
    assert "MAX_DEPENDENCY_CANDIDATES: usize = 64" in RESOLVER
    assert "DependencyResolutionDiagnostic" in RESOLVER
    for field in (
        "import_basename",
        "normalized_identity",
        "import_kind",
        "requesting_machine",
        "candidate_machine",
        "machine_compatible",
        "search_classification",
        "candidate_count",
        "load_stage",
        "win32_load_error_code",
    ):
        assert f'"{field}"' in RESOLVER
    for classification in (
        "plugin_dir",
        "configured_root",
        "system",
        "not_found",
        "ambiguous",
    ):
        assert f'"{classification}"' in RESOLVER
    assert '"maximum_records": MAX_DEPENDENCY_DIAGNOSTICS' in RESOLVER
    assert '"maximum_candidates": MAX_DEPENDENCY_CANDIDATES' in RESOLVER
    assert "canonicalize(root.join(\"System32\"))" in RESOLVER
    assert "is_api_set_name(basename)" in RESOLVER
    assert "normal_imports: Vec<String>" in RESOLVER
    assert "delay_imports: Vec<String>" in RESOLVER


def test_worker_load_failure_marker_is_stage_and_error_only():
    assert (
        '"stage:load_failure stage=" << stage'
        in ADMISSION
    )
    assert '" win32_error=" << static_cast<unsigned long>(error)' in ADMISSION
    for stage in (
        "set_default_dll_directories",
        "add_dll_directory",
        "load_library",
    ):
        assert f'report_load_failure("{stage}"' in ADMISSION
        assert f'"{stage}"' in BROKER
    marker = ADMISSION[
        ADMISSION.index("int report_load_failure"):
        ADMISSION.index("}  // namespace")
    ]
    assert "plugin_path" not in marker
    assert "wstring" not in marker
    assert "load_failure_marker(stderr, exit_code)" in BROKER
    assert "if exit_code != 11" in BROKER


def test_broker_and_sweep_propagate_only_vetted_dependency_diagnostics():
    assert "dependency_diagnostics_report" in SWEEP
    assert "fn dependency_report_for_dispatch" in SWEEP
    assert "if load_failure.len() != 2" in SWEEP
    assert '.get("stage")' in SWEEP
    assert ".and_then(Value::as_str)" in SWEEP
    assert '.get("win32_error_code")' in SWEEP
    assert SWEEP.count('"dependency_diagnostics"') >= 7
    assert "dependency_diagnostics_truncated" in SWEEP
    assert "dependency_report_for_dispatch(" in SWEEP
    assert '"error": dispatch_error' in SWEEP
