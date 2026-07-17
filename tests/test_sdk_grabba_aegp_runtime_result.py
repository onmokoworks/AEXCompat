import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_GRABBA_AEGP_RUNTIME_RESULT_2026-07-16.json"


def _load():
    return json.loads(RESULT.read_text(encoding="utf-8"))


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _assert_three_phase_module_audit(record):
    assert record["module_audit_status"] == "passed"
    assert record["module_audit_phase_count"] == 3
    assert record["module_audit_unknown_count"] == 0
    phases = record["module_audit_phases"]
    assert set(phases) == {"post_load", "pre_unload", "observed_union"}
    assert all(phase == {"status": "passed", "unknown_count": 0} for phase in phases.values())


def test_grabba_initialization_records_entrypoint_hooks_and_balanced_leases():
    evidence = _load()
    initialization = evidence["initialization"]
    report = initialization["report"]

    assert evidence["result"] == "official_sdk_grabba_aegp_initialization_and_update_menu_passed"
    assert initialization["exit_code"] == 0
    assert report["entrypoint"] == "EntryPointFunc"
    assert report["init_error"] == 0
    for hook in ("command", "death", "idle", "update_menu"):
        assert report[f"{hook}_hooks_registered"] == 1
    assert report["death_hooks_invoked"] == 1
    assert report["death_error"] == 0
    assert report["suite_acquires"] == report["suite_releases"] == 3
    assert report["live_suite_reference_count"] == 0
    assert report["suite_leases_balanced"] is True
    _assert_three_phase_module_audit(report)


def test_every_grabba_route_records_three_phase_unknown_zero_module_audit():
    evidence = _load()
    _assert_three_phase_module_audit(evidence["initialization"]["report"])
    for route in ("update_menu", "idle", "command_roundtrip"):
        _assert_three_phase_module_audit(evidence[route])


def test_update_menu_dispatch_is_successful_and_balanced():
    update_menu = _load()["update_menu"]

    assert update_menu["exit_code"] == 0
    assert update_menu["status"] == "event_completed"
    assert update_menu["event_requested"] == "update_menu"
    assert update_menu["event_error"] == 0
    assert update_menu["hooks_invoked"] == 1
    assert update_menu["suite_acquires"] == update_menu["suite_releases"] == 5
    assert update_menu["live_suite_reference_count"] == 0
    assert update_menu["suite_leases_balanced"] is True
    assert "version 10" in update_menu["compatibility_boundary"]


def test_idle_and_command_roundtrip_evidence_is_successful_and_balanced():
    evidence = _load()
    idle = evidence["idle"]
    command = evidence["command_roundtrip"]

    assert idle["exit_code"] == idle["event_error"] == 0
    assert idle["hooks_invoked"] == 1
    assert idle["idle_max_sleep"] == 0
    assert idle["suite_acquires"] == idle["suite_releases"] == 3
    assert idle["suite_leases_balanced"] is True

    assert command["exit_code"] == command["event_error"] == 0
    assert command["command_hooks_invoked"] == command["command_handled_count"] == 2
    assert command["receipts_created"] == command["receipts_checked_in"] == 2
    assert command["live_receipts"] == 0
    assert command["render_performed"] is True
    assert command["suite_acquires"] == command["suite_releases"] == 9
    assert command["suite_leases_balanced"] is True


def test_evidence_explicitly_excludes_pixel_render_and_adobe_oracle():
    evidence = _load()
    not_proven = evidence["scope"]["not_proven"]

    assert evidence["initialization"]["report"]["render_performed"] is False
    assert any("No pixel render" in claim for claim in not_proven)
    assert any("no Adobe behavioral or pixel oracle is proven" in claim for claim in not_proven)


def test_authenticated_grabba_worker_and_harness_artifacts_match_current_files():
    records = _load()["authenticated_artifacts"]

    assert {record["role"] for record in records} == {
        "official_sdk_grabba_fixture",
        "aegp_worker",
        "command_harness",
    }
    for record in records:
        path = ROOT / record["path"]
        assert path.is_file(), record["path"]
        assert path.stat().st_size == record["size_bytes"]
        if record["role"] == "official_sdk_grabba_fixture":
            build_result = json.loads(
                (ROOT / record["build_result_path"]).read_text(encoding="utf-8-sig")
            )
            assert build_result["sdk_source_unchanged"] is True
            assert build_result["artifact_size"] == record["size_bytes"]
            assert build_result["artifact_sha256"] == _sha256(path)
        else:
            assert _sha256(path) == record["sha256"]


def test_recorded_source_and_production_artifact_hashes_match_current_files():
    evidence = _load()
    records = evidence["source_provenance"]["files"]
    records += evidence["production_artifact_snapshot"]

    for record in records:
        path = ROOT / record["path"]
        assert path.is_file(), record["path"]
        assert path.stat().st_size == record["size_bytes"]
        assert _sha256(path) == record["sha256"]


def test_authenticated_runtime_reports_match_current_files_and_routes():
    evidence = _load()
    records = evidence["authenticated_reports"]

    assert {record["route"] for record in records} == {
        "initialization",
        "update_menu",
        "idle",
        "command_roundtrip",
    }
    for record in records:
        path = ROOT / record["path"]
        assert path.is_file(), record["path"]
        assert path.stat().st_size == record["size_bytes"]
        assert _sha256(path) == record["sha256"]
        json.loads(path.read_text(encoding="utf-8-sig"))
