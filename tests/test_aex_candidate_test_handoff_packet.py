import importlib.util
import json
import os
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


aex_candidate_test_handoff_packet = load_tool("aex_candidate_test_handoff_packet")


CANDIDATE = r"AEPluginBuild\ScatterMap.aex"
SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
    "private_payload_copied": False,
    "aex_file_opened": False,
}


def approval_request() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_approval_request_packet",
        "approval_request_state": "fixture_approval_request_ready_pending_manual_approval",
        "approval_request_ready": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "candidate_relative_path": CANDIDATE,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "manual_review_approval_ready": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "candidate_dependencies_clear": True,
        "path_policy_closed": True,
        "candidate_load_gate_closed": True,
        "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        **SAFETY_FALSE,
    }


def candidate_load_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_load_gate_dryrun",
        "candidate_relative_path": CANDIDATE,
        "candidate_load_gate_dryrun_state": "candidate_load_gate_dryrun_ready_no_load",
        "candidate_scoped_load_gate_dry_run_state": (
            "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
        ),
        "native_load_gate": "closed",
        "fixture_approval_satisfied": False,
        "candidate_dependencies_clear": True,
        "candidate_dependency_blockers_present": False,
        "global_dependency_blockers_present": True,
        "global_dependency_blockers_apply_to_candidate": False,
        "source_load_gate_dependency_recommendation": "do_not_open_native_load_gate",
        **SAFETY_FALSE,
    }


def native_loader_design() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_design_contract",
        "candidate_relative_path": CANDIDATE,
        "native_loader_design_state": "native_loader_design_ready_loader_closed",
        "contract_state": "native_loader_design_contract_ready_loader_closed_pending_fixture_approval",
        "loader_design_ready": True,
        "native_load_gate": "closed",
        "fixture_approval_satisfied": False,
        "candidate_dependencies_clear": True,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "controller_loads_aex": False,
        "approval_required_before_aex_path": True,
        "runtime_approval_required_before_load": True,
        "separate_process_required": True,
        **SAFETY_FALSE,
    }


def native_loader_runtime() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_contract",
        "candidate_relative_path": CANDIDATE,
        "native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
        "contract_state": "runtime_containment_contract_ready_path_acceptance_closed",
        "runtime_containment_ready": True,
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "native_load_gate": "closed",
        "fixture_approval_satisfied": False,
        "candidate_dependencies_clear": True,
        "controller_loads_aex": False,
        "broker_selftest_passed": True,
        "process_isolation_required": True,
        **SAFETY_FALSE,
    }


def native_loader_runtime_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_selftest",
        "runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
        "runtime_containment_selftest_passed": True,
        "synthetic_subprocess_only": True,
        "normal_exit_case_passed": True,
        "stderr_capture_passed": True,
        "timeout_case_passed": True,
        "child_cleanup_passed": True,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        **SAFETY_FALSE,
    }


def path_policy_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_path_policy_selftest",
        "path_policy_selftest_state": "closed_path_policy_selftest_passed_no_aex_path",
        "path_policy_selftest_passed": True,
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "candidate_path_string_accepted": False,
        "absolute_path_rejected": True,
        "traversal_rejected": True,
        "non_aex_suffix_rejected": True,
        "redaction_passed": True,
        "raw_input_paths_serialized": False,
        **SAFETY_FALSE,
    }


def image_fixture_validation() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_validation",
        "validation_state": "image_fixture_validation_passed_no_load",
        "validation_passed": True,
        **SAFETY_FALSE,
    }


def image_input_smoke() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_input_smoke_tool",
        "smoke_state": "image_input_smoke_passed_route_closed",
        "worker_identity_passed": True,
        "ofx_identity_passed": True,
        **SAFETY_FALSE,
    }


def render_validation_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_render_validation_contract",
        "contract_state": "render_validation_contract_ready_render_closed",
        "real_render_open": False,
        "no_load_validation_ready": True,
        **SAFETY_FALSE,
    }


def ofx_route_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
        **SAFETY_FALSE,
    }


def build_packet(**overrides: dict) -> dict:
    return aex_candidate_test_handoff_packet.build_candidate_test_handoff_packet(
        approval_request=overrides.get("approval_request") or approval_request(),
        approval_request_path=LAB_ROOT / "target" / "fixture-approval-request" / "request.local.json",
        candidate_load_gate=overrides.get("candidate_load_gate") or candidate_load_gate(),
        candidate_load_gate_path=LAB_ROOT / "target" / "candidate-load-gate" / "gate.local.json",
        native_loader_design=overrides.get("native_loader_design") or native_loader_design(),
        native_loader_design_path=LAB_ROOT / "target" / "native-loader-design" / "design.local.json",
        native_loader_runtime=overrides.get("native_loader_runtime") or native_loader_runtime(),
        native_loader_runtime_path=LAB_ROOT / "target" / "native-loader-runtime-contract" / "runtime.local.json",
        native_loader_runtime_selftest=overrides.get("native_loader_runtime_selftest")
        or native_loader_runtime_selftest(),
        native_loader_runtime_selftest_path=LAB_ROOT
        / "target"
        / "native-loader-runtime-selftest"
        / "runtime-selftest.local.json",
        path_policy_selftest=overrides.get("path_policy_selftest") or path_policy_selftest(),
        path_policy_selftest_path=LAB_ROOT
        / "target"
        / "native-loader-path-policy-selftest"
        / "path.local.json",
        image_fixture_validation=overrides.get("image_fixture_validation") or image_fixture_validation(),
        image_fixture_validation_path=LAB_ROOT
        / "target"
        / "image-fixture-validation"
        / "validation.local.json",
        image_input_smoke=overrides.get("image_input_smoke") or image_input_smoke(),
        image_input_smoke_path=LAB_ROOT / "target" / "image-input-smoke" / "smoke.local.json",
        render_validation_contract=overrides.get("render_validation_contract") or render_validation_contract(),
        render_validation_contract_path=LAB_ROOT
        / "target"
        / "render-validation-contract"
        / "render.local.json",
        ofx_route_contract=overrides.get("ofx_route_contract") or ofx_route_contract(),
        ofx_route_contract_path=LAB_ROOT / "target" / "ofx-route-contract" / "ofx.local.json",
    )


class AexCandidateTestHandoffPacketTests(unittest.TestCase):
    def test_builds_no_load_handoff_with_native_route_closed(self):
        packet = build_packet()
        self.assertEqual(packet["report_kind"], "aex_candidate_test_handoff_packet")
        self.assertEqual(packet["handoff_state"], "candidate_test_handoff_ready_no_load_native_closed")
        self.assertTrue(packet["handoff_packet_ready"])
        self.assertTrue(packet["no_load_test_handoff_ready"])
        self.assertFalse(packet["native_test_handoff_ready"])
        self.assertEqual(packet["candidate_relative_path"], CANDIDATE)
        self.assertFalse(packet["approval_can_be_issued_now"])
        self.assertFalse(packet["approval_manifest_created"])
        self.assertFalse(packet["fixture_approval_satisfied"])
        self.assertEqual(packet["native_load_gate"], "closed")
        self.assertFalse(packet["path_acceptance_ready"])
        self.assertFalse(packet["aex_path_acceptance_enabled"])
        self.assertIsNone(packet["accepted_aex_path"])
        self.assertFalse(packet["path_payload_supplied"])
        self.assertTrue(packet["runtime_containment_selftest_passed"])
        self.assertTrue(packet["synthetic_subprocess_only"])
        self.assertTrue(packet["child_cleanup_passed"])
        self.assertTrue(packet["no_load_image_test_ready"])
        self.assertTrue(packet["image_fixture_validation_passed"])
        self.assertTrue(packet["no_load_render_contract_ready"])
        self.assertTrue(packet["no_load_ofx_mock_ready"])
        self.assertFalse(packet["real_render_open"])
        self.assertFalse(packet["real_route_open"])
        self.assertIn("accept_aex_path", packet["forbidden_handoff_actions"])
        self.assertGreaterEqual(packet["handoff_blocker_count"], 5)
        self.assertFalse(packet["native_load_enabled"])
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["dll_load_performed"])
        self.assertFalse(packet["aex_file_opened"])

    def test_rejects_open_path_acceptance(self):
        runtime = native_loader_runtime()
        runtime["path_acceptance_ready"] = True
        with self.assertRaises(ValueError) as ctx:
            build_packet(native_loader_runtime=runtime)
        self.assertIn("path_acceptance_ready must be false", str(ctx.exception))

    def test_rejects_real_render_open(self):
        render_contract = render_validation_contract()
        render_contract["real_render_open"] = True
        with self.assertRaises(ValueError) as ctx:
            build_packet(render_validation_contract=render_contract)
        self.assertIn("real_render_open must be false", str(ctx.exception))

    def test_rejects_runtime_selftest_with_path_payload(self):
        selftest = native_loader_runtime_selftest()
        selftest["path_payload_supplied"] = True
        with self.assertRaises(ValueError) as ctx:
            build_packet(native_loader_runtime_selftest=selftest)
        self.assertIn("path_payload_supplied must be false", str(ctx.exception))

    def test_rejects_failed_image_fixture_validation(self):
        validation = image_fixture_validation()
        validation["validation_passed"] = False
        with self.assertRaises(ValueError) as ctx:
            build_packet(image_fixture_validation=validation)
        self.assertIn("validation_passed must be true", str(ctx.exception))

    def test_rejects_candidate_mismatch(self):
        gate = candidate_load_gate()
        gate["candidate_relative_path"] = r"Other\Candidate.aex"
        with self.assertRaises(ValueError) as ctx:
            build_packet(candidate_load_gate=gate)
        self.assertIn("candidate_relative_path must match approval request", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "approval": LAB_ROOT / "target" / "fixture-approval-request",
            "gate": LAB_ROOT / "target" / "candidate-load-gate",
            "design": LAB_ROOT / "target" / "native-loader-design",
            "runtime": LAB_ROOT / "target" / "native-loader-runtime-contract",
            "runtime_selftest": LAB_ROOT / "target" / "native-loader-runtime-selftest",
            "path": LAB_ROOT / "target" / "native-loader-path-policy-selftest",
            "validation": LAB_ROOT / "target" / "image-fixture-validation",
            "smoke": LAB_ROOT / "target" / "image-input-smoke",
            "render": LAB_ROOT / "target" / "render-validation-contract",
            "ofx": LAB_ROOT / "target" / "ofx-route-contract",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        paths = {
            "approval": roots["approval"] / f"{time.time_ns()}-{os.getpid()}-request.local.json",
            "gate": roots["gate"] / f"{time.time_ns()}-{os.getpid()}-gate.local.json",
            "design": roots["design"] / f"{time.time_ns()}-{os.getpid()}-design.local.json",
            "runtime": roots["runtime"] / f"{time.time_ns()}-{os.getpid()}-runtime.local.json",
            "runtime_selftest": roots["runtime_selftest"] / f"{time.time_ns()}-{os.getpid()}-runtime-selftest.local.json",
            "path": roots["path"] / f"{time.time_ns()}-{os.getpid()}-path.local.json",
            "validation": roots["validation"] / f"{time.time_ns()}-{os.getpid()}-validation.local.json",
            "smoke": roots["smoke"] / f"{time.time_ns()}-{os.getpid()}-smoke.local.json",
            "render": roots["render"] / f"{time.time_ns()}-{os.getpid()}-render.local.json",
            "ofx": roots["ofx"] / f"{time.time_ns()}-{os.getpid()}-ofx.local.json",
        }
        payloads = {
            "approval": approval_request(),
            "gate": candidate_load_gate(),
            "design": native_loader_design(),
            "runtime": native_loader_runtime(),
            "runtime_selftest": native_loader_runtime_selftest(),
            "path": path_policy_selftest(),
            "validation": image_fixture_validation(),
            "smoke": image_input_smoke(),
            "render": render_validation_contract(),
            "ofx": ofx_route_contract(),
        }
        for key, payload in payloads.items():
            paths[key].write_text(json.dumps(payload), encoding="utf-8")

        approval, approval_path = aex_candidate_test_handoff_packet.load_approval_request(paths["approval"])
        gate, gate_path = aex_candidate_test_handoff_packet.load_candidate_load_gate(paths["gate"])
        design, design_path = aex_candidate_test_handoff_packet.load_native_loader_design(paths["design"])
        runtime, runtime_path = aex_candidate_test_handoff_packet.load_native_loader_runtime(paths["runtime"])
        runtime_selftest, runtime_selftest_path = (
            aex_candidate_test_handoff_packet.load_native_loader_runtime_selftest(paths["runtime_selftest"])
        )
        path_policy, path_policy_path = aex_candidate_test_handoff_packet.load_path_policy_selftest(paths["path"])
        validation, validation_path = aex_candidate_test_handoff_packet.load_image_fixture_validation(
            paths["validation"]
        )
        smoke, smoke_path = aex_candidate_test_handoff_packet.load_image_input_smoke(paths["smoke"])
        render, render_path = aex_candidate_test_handoff_packet.load_render_validation_contract(paths["render"])
        ofx, ofx_path = aex_candidate_test_handoff_packet.load_ofx_route_contract(paths["ofx"])

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-handoff-request.local.json"
        outside.write_text(json.dumps(approval_request()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_test_handoff_packet.load_approval_request(outside)

        packet = aex_candidate_test_handoff_packet.build_candidate_test_handoff_packet(
            approval_request=approval,
            approval_request_path=approval_path,
            candidate_load_gate=gate,
            candidate_load_gate_path=gate_path,
            native_loader_design=design,
            native_loader_design_path=design_path,
            native_loader_runtime=runtime,
            native_loader_runtime_path=runtime_path,
            native_loader_runtime_selftest=runtime_selftest,
            native_loader_runtime_selftest_path=runtime_selftest_path,
            path_policy_selftest=path_policy,
            path_policy_selftest_path=path_policy_path,
            image_fixture_validation=validation,
            image_fixture_validation_path=validation_path,
            image_input_smoke=smoke,
            image_input_smoke_path=smoke_path,
            render_validation_contract=render,
            render_validation_contract_path=render_path,
            ofx_route_contract=ofx,
            ofx_route_contract_path=ofx_path,
        )
        out = LAB_ROOT / "target" / "candidate-test-handoff" / f"{time.time_ns()}-{os.getpid()}-handoff.local.json"
        written = aex_candidate_test_handoff_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_test_handoff_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_candidate_test_handoff_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-handoff.json",
                packet,
            )


if __name__ == "__main__":
    unittest.main()
