import importlib.util
import json
import sys
import time
import unittest
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_candidate_ofx_host_binary_review_request = load_tool(
    "aex_candidate_ofx_host_binary_review_request"
)


CANDIDATE = r"AEPluginBuild\ScatterMap.aex"
SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
    "ofx_runtime_invoked": False,
    "host_process_launch_enabled": False,
    "private_payload_copied": False,
    "aex_file_opened": False,
    "aex_file_hashed": False,
    "aex_file_copied": False,
    "aepx_file_modified": False,
    "aep_binary_modified": False,
    "ae_project_write_performed": False,
    "ofx_plugin_built": False,
    "ofx_describe_performed": False,
    "ofx_render_performed": False,
    "aex_render_performed": False,
    "render_validation_performed": False,
    "ppm_pixel_read_performed": False,
    "pipl_payload_parsed": False,
    "parameter_schema_emitted": False,
    "redacted_schema_emitted": False,
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "resource_payload_opened": False,
    "resource_payload_extracted": False,
    "raw_payload_serialized": False,
    "ofx_host_binary_opened": False,
    "ofx_host_binary_hashed": False,
    "ofx_host_binary_copied": False,
    "ofx_host_binary_executed": False,
    "ofx_plugin_binary_opened": False,
    "ofx_plugin_binary_hashed": False,
    "ofx_plugin_binary_copied": False,
}


def prerequisite_audit_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_prerequisite_audit",
        "candidate_relative_path": CANDIDATE,
        "runtime_prerequisite_audit_state": "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed",
        "runtime_prerequisite_audit_ready": True,
        "runtime_approval_verified_not_approved": True,
        "runtime_approval_gate_stays_closed": True,
        "runtime_containment_contract_ready": True,
        "runtime_containment_selftest_synthetic_passed": True,
        "parameter_schema_review_policy_ready": True,
        "render_validation_contract_ready": True,
        "ofx_route_contract_closed": True,
        "mock_route_ready": True,
        "path_acceptance_closed": True,
        "runtime_invocation_prerequisites_ready": False,
        "approval_can_be_issued_now": False,
        "runtime_approval_satisfied": False,
        "explicit_runtime_approval_present": False,
        "fixture_approval_satisfied": False,
        "ofx_host_binary_review_ready": False,
        "runtime_containment_selftest_ready": False,
        "schema_and_render_validation_ready": False,
        "real_render_open": False,
        "real_route_open": False,
        "ofx_runtime_invoked": False,
        "host_process_launch_enabled": False,
        "path_acceptance_ready": False,
        "ppm_pixel_read_performed": False,
        "runtime_prerequisite_audit_path_payload_exported": False,
        "failed_evidence_count": 0,
        "blocking_prerequisite_count": 4,
        "prerequisite_gaps": [
            {"id": "explicit_runtime_approval"},
            {"id": "fixture_approval"},
            {"id": "ofx_host_binary_review"},
            {"id": "real_schema_and_render_validation"},
        ],
        **SAFETY_FALSE,
    }


def host_harness_dryrun_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_dryrun",
        "candidate_relative_path": CANDIDATE,
        "harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
        "harness_dryrun_ready": True,
        "dry_run_only": True,
        "host_harness_kind": "ofx_noop_host_harness_planning",
        "planned_case_count": 2,
        "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "source_bridge_allowed_route": "no_op_identity_only",
        "source_mock_route_ready": True,
        "source_real_route_open": False,
        "source_ofx_runtime_invoked": False,
        "source_ofx_describe_ready": False,
        "source_ofx_render_ready": False,
        "host_harness_path_payload_exported": False,
        "requires_future_runtime_approval": True,
        "ofx_runtime_invoked": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_file_opened": False,
        **SAFETY_FALSE,
    }


def host_harness_selftest_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_selftest",
        "candidate_relative_path": CANDIDATE,
        "host_harness_selftest_state": "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        "host_harness_selftest_ready": True,
        "host_harness_kind": "ofx_noop_host_harness_synthetic_selftest",
        "synthetic_only": True,
        "synthetic_contract_checks_performed": True,
        "checked_case_count": 2,
        "descriptor_contract_checked": True,
        "render_identity_contract_checked": True,
        "ppm_pixel_read_performed": False,
        "ofx_runtime_invoked": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "host_harness_path_payload_exported": False,
        "requires_future_runtime_approval": True,
        "aex_file_opened": False,
        **SAFETY_FALSE,
    }


def runtime_boundary_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_boundary_contract",
        "candidate_relative_path": CANDIDATE,
        "candidate_ofx_runtime_boundary_contract_state": (
            "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        ),
        "contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
        "runtime_boundary_ready": True,
        "source_host_harness_selftest_state": (
            "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed"
        ),
        "source_fixture_approval_satisfied": False,
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "host_process_launch_enabled": False,
        "path_acceptance_ready": False,
        "real_route_open": False,
        "mock_route_ready": True,
        "ofx_runtime_invoked": False,
        "ppm_pixel_read_performed": False,
        "ofx_host_path_payload_supplied": False,
        "ofx_plugin_binary_path_payload_supplied": False,
        "runtime_boundary_path_payload_exported": False,
        "requires_future_runtime_approval": True,
        "aex_file_opened": False,
        **SAFETY_FALSE,
    }


def write_source(root_name: str, name: str, payload: dict) -> Path:
    root = LAB_ROOT / "target" / root_name
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


class AexCandidateOfxHostBinaryReviewRequestTests(unittest.TestCase):
    def test_builds_pending_host_binary_review_request_without_paths_or_runtime(self):
        stamp = time.time_ns()
        audit_path = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-prerequisite-audit"
            / f"ae-candidate-ofx-runtime-prerequisite-audit-{stamp}.local.json"
        )
        dryrun_path = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-host-harness-dryrun"
            / f"ae-candidate-ofx-host-harness-dryrun-{stamp}.local.json"
        )
        selftest_path = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-host-harness-selftest"
            / f"ae-candidate-ofx-host-harness-selftest-{stamp}.local.json"
        )
        boundary_path = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-boundary-contract"
            / f"ae-candidate-ofx-runtime-boundary-contract-{stamp}.local.json"
        )

        report = aex_candidate_ofx_host_binary_review_request.build_host_binary_review_request(
            prerequisite_audit=prerequisite_audit_payload(),
            prerequisite_audit_path=audit_path,
            host_harness_dryrun=host_harness_dryrun_payload(),
            host_harness_dryrun_path=dryrun_path,
            host_harness_selftest=host_harness_selftest_payload(),
            host_harness_selftest_path=selftest_path,
            runtime_boundary_contract=runtime_boundary_payload(),
            runtime_boundary_contract_path=boundary_path,
        )

        self.assertEqual(report["report_kind"], "aex_candidate_ofx_host_binary_review_request")
        self.assertEqual(report["review_request_kind"], "ofx_host_binary_provenance_manual_review_request")
        self.assertEqual(
            report["host_binary_review_request_state"],
            "candidate_ofx_host_binary_review_request_ready_pending_manual_review",
        )
        self.assertTrue(report["host_binary_review_request_ready"])
        self.assertTrue(report["host_binary_review_request_created"])
        self.assertFalse(report["host_binary_review_can_be_approved_now"])
        self.assertFalse(report["ofx_host_binary_review_ready"])
        self.assertFalse(report["host_binary_review_satisfied"])
        self.assertFalse(report["host_binary_review_manifest_created"])
        self.assertTrue(report["host_binary_review_gate_stays_closed"])
        self.assertTrue(report["requires_explicit_host_binary_review"])
        self.assertFalse(report["runtime_invocation_prerequisites_ready"])
        self.assertFalse(report["ofx_runtime_invocation_ready"])
        self.assertFalse(report["host_process_launch_enabled"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertIsNone(report["accepted_aex_path"])
        self.assertIsNone(report["accepted_ofx_host_path"])
        self.assertIsNone(report["accepted_ofx_plugin_binary_path"])
        self.assertFalse(report["ofx_host_path_payload_supplied"])
        self.assertFalse(report["ofx_plugin_binary_path_payload_supplied"])
        self.assertFalse(report["host_binary_review_path_payload_exported"])
        self.assertEqual(report["review_checklist_count"], 8)
        self.assertEqual(report["host_binary_review_blocker_count"], 7)
        self.assertEqual(
            {item["id"] for item in report["review_checklist"]},
            {
                "host_binary_identity_required",
                "host_binary_provenance_required",
                "host_binary_license_required",
                "host_binary_integrity_review_required",
                "host_shim_build_policy_required",
                "process_containment_required",
                "log_redaction_required",
                "runtime_approval_still_required",
            },
        )
        runtime_requirement = next(
            item for item in report["review_checklist"] if item["id"] == "runtime_approval_still_required"
        )
        self.assertTrue(runtime_requirement["satisfied"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertFalse(report["ofx_render_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["aex_file_hashed"])
        self.assertFalse(report["ppm_pixel_read_performed"])
        self.assertFalse(report["ofx_host_binary_opened"])
        self.assertFalse(report["ofx_host_binary_hashed"])
        self.assertFalse(report["ofx_host_binary_copied"])
        self.assertFalse(report["ofx_host_binary_executed"])
        self.assertFalse(report["ofx_plugin_binary_opened"])
        self.assertFalse(report["ofx_plugin_binary_hashed"])
        self.assertIn("accept_ofx_host_path", report["blocked_actions"])
        self.assertIn("open_ofx_host_binary", report["blocked_actions"])
        self.assertIn("execute_ofx_host_binary", report["blocked_actions"])
        self.assertIn("load_ofx_plugin", report["blocked_actions"])

    def test_rejects_sources_that_open_runtime_or_host_route(self):
        audit = prerequisite_audit_payload()
        audit["ofx_host_binary_review_ready"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_binary_review_request.build_host_binary_review_request(
                prerequisite_audit=audit,
                prerequisite_audit_path=Path("target/candidate-ofx-runtime-prerequisite-audit/audit.json"),
                host_harness_dryrun=host_harness_dryrun_payload(),
                host_harness_dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
                host_harness_selftest=host_harness_selftest_payload(),
                host_harness_selftest_path=Path("target/candidate-ofx-host-harness-selftest/selftest.json"),
                runtime_boundary_contract=runtime_boundary_payload(),
                runtime_boundary_contract_path=Path("target/candidate-ofx-runtime-boundary-contract/boundary.json"),
            )

        dryrun = host_harness_dryrun_payload()
        dryrun["source_real_route_open"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_binary_review_request.build_host_binary_review_request(
                prerequisite_audit=prerequisite_audit_payload(),
                prerequisite_audit_path=Path("target/candidate-ofx-runtime-prerequisite-audit/audit.json"),
                host_harness_dryrun=dryrun,
                host_harness_dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
                host_harness_selftest=host_harness_selftest_payload(),
                host_harness_selftest_path=Path("target/candidate-ofx-host-harness-selftest/selftest.json"),
                runtime_boundary_contract=runtime_boundary_payload(),
                runtime_boundary_contract_path=Path("target/candidate-ofx-runtime-boundary-contract/boundary.json"),
            )

        boundary = runtime_boundary_payload()
        boundary["host_process_launch_enabled"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_binary_review_request.build_host_binary_review_request(
                prerequisite_audit=prerequisite_audit_payload(),
                prerequisite_audit_path=Path("target/candidate-ofx-runtime-prerequisite-audit/audit.json"),
                host_harness_dryrun=host_harness_dryrun_payload(),
                host_harness_dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
                host_harness_selftest=host_harness_selftest_payload(),
                host_harness_selftest_path=Path("target/candidate-ofx-host-harness-selftest/selftest.json"),
                runtime_boundary_contract=boundary,
                runtime_boundary_contract_path=Path("target/candidate-ofx-runtime-boundary-contract/boundary.json"),
            )

    def test_loads_sources_and_writes_create_new_under_root(self):
        stamp = time.time_ns()
        audit_path = write_source(
            "candidate-ofx-runtime-prerequisite-audit",
            f"ae-candidate-ofx-runtime-prerequisite-audit-{stamp}.local.json",
            prerequisite_audit_payload(),
        )
        dryrun_path = write_source(
            "candidate-ofx-host-harness-dryrun",
            f"ae-candidate-ofx-host-harness-dryrun-{stamp}.local.json",
            host_harness_dryrun_payload(),
        )
        selftest_path = write_source(
            "candidate-ofx-host-harness-selftest",
            f"ae-candidate-ofx-host-harness-selftest-{stamp}.local.json",
            host_harness_selftest_payload(),
        )
        boundary_path = write_source(
            "candidate-ofx-runtime-boundary-contract",
            f"ae-candidate-ofx-runtime-boundary-contract-{stamp}.local.json",
            runtime_boundary_payload(),
        )

        audit, resolved_audit = aex_candidate_ofx_host_binary_review_request.load_prerequisite_audit(
            Path("target") / "candidate-ofx-runtime-prerequisite-audit" / audit_path.name
        )
        dryrun, resolved_dryrun = aex_candidate_ofx_host_binary_review_request.load_host_harness_dryrun(
            Path("target") / "candidate-ofx-host-harness-dryrun" / dryrun_path.name
        )
        selftest, resolved_selftest = aex_candidate_ofx_host_binary_review_request.load_host_harness_selftest(
            Path("target") / "candidate-ofx-host-harness-selftest" / selftest_path.name
        )
        boundary, resolved_boundary = aex_candidate_ofx_host_binary_review_request.load_runtime_boundary_contract(
            Path("target") / "candidate-ofx-runtime-boundary-contract" / boundary_path.name
        )
        report = aex_candidate_ofx_host_binary_review_request.build_host_binary_review_request(
            prerequisite_audit=audit,
            prerequisite_audit_path=resolved_audit,
            host_harness_dryrun=dryrun,
            host_harness_dryrun_path=resolved_dryrun,
            host_harness_selftest=selftest,
            host_harness_selftest_path=resolved_selftest,
            runtime_boundary_contract=boundary,
            runtime_boundary_contract_path=resolved_boundary,
        )
        out = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-host-binary-review-request"
            / f"ae-candidate-ofx-host-binary-review-request-{stamp}.local.json"
        )
        written = aex_candidate_ofx_host_binary_review_request.write_json_create_new(out, report)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_host_binary_review_request.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_binary_review_request.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-host-binary-review-request.json", report
            )
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_binary_review_request.load_prerequisite_audit(
                LAB_ROOT / "target" / "outside-candidate-ofx-runtime-prerequisite-audit.json"
            )


if __name__ == "__main__":
    unittest.main()
