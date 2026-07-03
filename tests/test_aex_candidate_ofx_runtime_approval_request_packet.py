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


aex_candidate_ofx_runtime_approval_request_packet = load_tool(
    "aex_candidate_ofx_runtime_approval_request_packet"
)


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
}


def boundary_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_runtime_boundary_contract",
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate_ofx_runtime_boundary_contract_state": (
            "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
        ),
        "contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
        "runtime_boundary_ready": True,
        "boundary_contract_ready": True,
        "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "source_harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
        "source_host_harness_selftest_state": "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        "source_ofx_route_contract_state": "ofx_route_contract_ready_route_closed",
        "source_native_runtime_contract_state": "runtime_containment_contract_ready_no_load",
        "source_fixture_approval_satisfied": False,
        "no_load_boundary_contract_created": True,
        "approval_gate_count": 6,
        "required_runtime_evidence_count": 6,
        "required_before_runtime_invocation": [
            "explicit user approval for OFX runtime instantiation",
            "explicit user fixture approval before any AEX path is accepted",
        ],
        "ofx_runtime_allowed_now": False,
        "ofx_runtime_invocation_ready": False,
        "ofx_runtime_instantiation_ready": False,
        "ofx_runtime_instantiation_performed": False,
        "ofx_binary_build_allowed_now": False,
        "ofx_binary_built": False,
        "real_ofx_describe_allowed_now": False,
        "real_ofx_render_allowed_now": False,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "mock_route_ready": True,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "ppm_pixel_read_performed": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "runtime_boundary_path_payload_exported": False,
        "ofx_host_path_payload_supplied": False,
        "ofx_plugin_binary_path_payload_supplied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "host_process_launch_enabled": False,
        "requires_future_runtime_approval": True,
        "runtime_approval_required_before_invocation": True,
        "requires_future_fixture_approval": True,
        "requires_future_render_validation_approval": True,
        "blocked_actions": [
            "instantiate_ofx_runtime",
            "launch_ofx_host_process",
            "accept_ofx_host_path",
            "accept_ofx_plugin_binary_path",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
        ],
        **SAFETY_FALSE,
    }


def write_boundary(name: str, payload: dict | None = None) -> Path:
    root = LAB_ROOT / "target" / "candidate-ofx-runtime-boundary-contract"
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload or boundary_payload()), encoding="utf-8")
    return path


class AexCandidateOfxRuntimeApprovalRequestPacketTests(unittest.TestCase):
    def test_builds_pending_runtime_approval_request_without_approval(self):
        stamp = time.time_ns()
        boundary_path = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-boundary-contract"
            / f"ae-candidate-ofx-runtime-boundary-contract-{stamp}.local.json"
        )
        report = aex_candidate_ofx_runtime_approval_request_packet.build_runtime_approval_request_packet(
            runtime_boundary_contract=boundary_payload(),
            runtime_boundary_contract_path=boundary_path,
        )

        self.assertEqual(report["report_kind"], "aex_candidate_ofx_runtime_approval_request_packet")
        self.assertEqual(
            report["runtime_approval_request_state"],
            "candidate_ofx_runtime_approval_request_ready_pending_manual_approval",
        )
        self.assertTrue(report["runtime_approval_request_ready"])
        self.assertTrue(report["runtime_approval_request_created"])
        self.assertFalse(report["runtime_approval_can_be_issued_now"])
        self.assertFalse(report["runtime_approval_manifest_created"])
        self.assertTrue(report["runtime_approval_gate_stays_closed"])
        self.assertTrue(report["requires_explicit_user_approval"])
        self.assertEqual(report["required_approval_token_name"], "APPROVE_OFX_RUNTIME_INVOCATION")
        self.assertTrue(report["approval_token_not_stored_in_manifest"])
        self.assertEqual(report["source_boundary_contract_state"], boundary_payload()["candidate_ofx_runtime_boundary_contract_state"])
        self.assertEqual(report["approval_blocker_count"], 5)
        self.assertEqual(report["review_checklist_count"], 6)
        self.assertFalse(report["ofx_runtime_invocation_ready"])
        self.assertFalse(report["host_process_launch_enabled"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["real_route_open"])
        self.assertTrue(report["mock_route_ready"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertFalse(report["ppm_pixel_read_performed"])
        self.assertFalse(report["runtime_approval_path_payload_exported"])
        self.assertIn("instantiate_ofx_runtime", report["blocked_actions"])
        self.assertIn("launch_ofx_host_process", report["blocked_actions"])

    def test_rejects_open_or_invocation_ready_boundary(self):
        boundary = boundary_payload()
        boundary["ofx_runtime_invocation_ready"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_approval_request_packet.build_runtime_approval_request_packet(
                runtime_boundary_contract=boundary,
                runtime_boundary_contract_path=Path("target/candidate-ofx-runtime-boundary-contract/boundary.json"),
            )

        boundary = boundary_payload()
        boundary["real_route_open"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_approval_request_packet.build_runtime_approval_request_packet(
                runtime_boundary_contract=boundary,
                runtime_boundary_contract_path=Path("target/candidate-ofx-runtime-boundary-contract/boundary.json"),
            )

    def test_loads_boundary_and_writes_create_new_under_root(self):
        stamp = time.time_ns()
        boundary_path = write_boundary(f"ae-candidate-ofx-runtime-boundary-contract-{stamp}.local.json")
        boundary, resolved_boundary = aex_candidate_ofx_runtime_approval_request_packet.load_runtime_boundary_contract(
            Path("target") / "candidate-ofx-runtime-boundary-contract" / boundary_path.name
        )
        report = aex_candidate_ofx_runtime_approval_request_packet.build_runtime_approval_request_packet(
            runtime_boundary_contract=boundary,
            runtime_boundary_contract_path=resolved_boundary,
        )
        out = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-approval-request"
            / f"ae-candidate-ofx-runtime-approval-request-{stamp}.local.json"
        )
        written = aex_candidate_ofx_runtime_approval_request_packet.write_json_create_new(out, report)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_runtime_approval_request_packet.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_approval_request_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-runtime-approval-request.json", report
            )
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_approval_request_packet.load_runtime_boundary_contract(
                LAB_ROOT / "target" / "outside-candidate-ofx-runtime-boundary-contract.json"
            )


if __name__ == "__main__":
    unittest.main()
