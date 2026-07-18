import hashlib
import json
import os
from pathlib import Path, PureWindowsPath


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "AEGP_ASYNC_RECEIPT_RUNTIME_RESULT_2026-07-16.json"


def _load():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def test_receipt_self_test_evidence_is_balanced_and_direct():
    evidence = _load()
    runtime = evidence["focused_native_self_test"]
    stdout = runtime["stdout"]

    assert evidence["result"] == "focused_receipt_self_test_and_current_histogrid_broker_test_passed"
    assert runtime["exit_code"] == 0
    assert stdout["aegp_async_receipt"] == "passed"
    assert stdout["created"] == stdout["checked_in"] == 4
    assert stdout["live"] == stdout["live_bytes"] == 0
    assert stdout["invalid_operations"] >= 4


def test_histogrid_evidence_does_not_overclaim_receipt_counters_or_real_ae():
    evidence = _load()
    integrated = evidence["histogrid_broker_integration"]

    assert integrated["exit_code"] == 0
    assert integrated["test_result"]["executed"] == integrated["test_result"]["passed"] == 1
    assert integrated["runtime_preconditions_observed"][
        "therefore_test_did_not_take_missing_artifact_early_return"
    ] is True
    assert integrated["receipt_counter_visibility"]["exposed_by_broker_report"] is False
    audit = integrated["required_module_audit"]
    assert audit["required_by_production_dispatch"] is True
    assert audit["validation_result"] == "passed"
    assert audit["phase_count_required"] == 3
    assert audit["unknown_count_required"] == 0
    assert any("No execution in Adobe After Effects" in item for item in evidence["scope"]["not_proven"])
    assert any("Adobe After Effects installation" in item for item in evidence["pending"])


def test_recorded_source_and_artifact_hashes_match_current_files():
    evidence = _load()
    records = evidence["source_provenance"]["files"]
    records += evidence["focused_native_self_test"]["artifact"],
    records += evidence["focused_native_self_test"]["report_artifact"],
    records += evidence["histogrid_broker_integration"]["artifacts"]
    records += evidence["production_artifact_snapshot"]

    for record in records:
        path = ROOT / record["path"]
        assert path.is_file(), record["path"]
        assert path.stat().st_size == record["size_bytes"]
        assert _sha256(path) == record["sha256"]


def _installed_sdk_file(record):
    candidates = [Path(record["path"])]
    sdk_root = os.environ.get("AFTER_EFFECTS_SDK_ROOT")
    if sdk_root:
        parts = PureWindowsPath(record["path"]).parts
        if "Examples" in parts:
            candidates.append(Path(sdk_root).joinpath(*parts[parts.index("Examples"):]))
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise AssertionError(
        "SDK source was not found at the recorded path or under AFTER_EFFECTS_SDK_ROOT: "
        + record["path"]
    )


def test_installed_sdk_abi_sources_match_recorded_provenance():
    evidence = _load()["abi_source"]

    for key in ("current_header", "legacy_header"):
        record = evidence[key]
        path = _installed_sdk_file(record)
        assert path.stat().st_size == record["size_bytes"]
        assert _sha256(path) == record["sha256"]

    assert evidence["legacy_header"]["observations"][0].endswith("numeric acquisition version 5.")
