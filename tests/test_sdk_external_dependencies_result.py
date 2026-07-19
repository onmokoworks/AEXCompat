import json
from pathlib import Path
import source_owners


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_EXTERNAL_DEPENDENCIES_RESULT_2026-07-15.json"
WORKER = source_owners.L2_MAIN
MODE_EXECUTION = ROOT / "minihost" / "src" / "l2_mode_execution.cpp"
BROKER = ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs"
HARNESS = ROOT / "broker" / "crates" / "harness" / "src" / "main.rs"
PROBE = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"


def result():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def test_convolutrix_returns_a_bounded_host_owned_dependency_string():
    query = result()["all_dependencies"]
    assert query["check_type"] == 1
    assert query["selector_error"] == query["exception_code"] == 0
    assert query["dependency_text"] == "All Dependencies requested."
    assert query["dependency_bytes"] == len(query["dependency_text"]) + 1 == 28
    assert query["handle_returned"] is True
    assert query["handle_valid"] is True
    assert query["nul_terminated"] is True
    assert query["handle_host_disposed"] is True
    assert query["handles_created"] == query["handles_disposed"] == 1


def test_null_missing_dependency_handle_is_a_valid_empty_result():
    query = result()["missing_dependencies_none"]
    assert query["check_type"] == 2
    assert query["selector_error"] == query["exception_code"] == 0
    assert query["dependency_text"] == ""
    assert query["dependency_bytes"] == 0
    assert query["handle_returned"] is False
    assert query["null_handle_is_valid_empty_result"] is True
    assert query["handles_created"] == query["handles_disposed"] == 0


def test_external_dependency_boundary_is_abi_bound_isolated_and_exposed():
    evidence = result()
    worker = source_owners.worker_text()
    mode_execution = MODE_EXECUTION.read_text(encoding="utf-8")
    broker = BROKER.read_text(encoding="utf-8")
    harness = HARNESS.read_text(encoding="utf-8")
    probe = PROBE.read_text(encoding="utf-8")
    assert evidence["abi"]["selector"] == 16
    assert evidence["abi"]["extra_handle_offset"] == 8
    assert "PF_ExtDependenciesExtra::dependencies_strH" in probe
    assert "PF_Cmd_GET_EXTERNAL_DEPENDENCIES" in probe
    assert "constexpr int32_t kGetExternalDependencies = 16;" in worker
    assert "kMaxDependencyBytes = 64 * 1024" in mode_execution
    assert "invoke_entry_seh(b.entry, kGetExternalDependencies" in worker
    assert "pub fn inspect_experimental_external_dependencies" in broker
    assert '"get_external_dependencies"' in broker
    assert 'args[1] == "--inspect-experimental-dependencies"' in harness
    assert "Inspect missing dependencies" in harness
