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


aex_candidate_no_load_test_runner_dryrun = load_tool("aex_candidate_no_load_test_runner_dryrun")


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


def candidate_handoff() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_test_handoff_packet",
        "handoff_state": "candidate_test_handoff_ready_no_load_native_closed",
        "handoff_packet_ready": True,
        "candidate_relative_path": CANDIDATE,
        "no_load_test_handoff_ready": True,
        "native_test_handoff_ready": False,
        "approval_request_ready": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "candidate_dependencies_clear": True,
        "global_dependency_blockers_apply_to_candidate": False,
        "runtime_containment_selftest_passed": True,
        "synthetic_subprocess_only": True,
        "path_policy_selftest_passed": True,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "candidate_path_string_accepted": False,
        "raw_input_paths_serialized": False,
        "no_load_image_test_ready": True,
        "image_fixture_validation_passed": True,
        "worker_identity_passed": True,
        "ofx_identity_passed": True,
        "no_load_render_contract_ready": True,
        "real_render_open": False,
        "no_load_validation_ready": True,
        "no_load_ofx_mock_ready": True,
        "real_route_open": False,
        "mock_route_ready": True,
        **SAFETY_FALSE,
    }


def target_candidate() -> dict:
    return {
        "relative_path": CANDIDATE,
        "candidate_policy_state": "eligible_for_manual_policy_review",
        "native_load_approval": "not_granted",
    }


def fixtures() -> list[dict]:
    return [
        {"case_id": "first", "pattern": "gradient", "ppm_path": r"D:\local\first.ppm"},
        {"case_id": "second", "pattern": "checker", "ppm_path": r"D:\local\second.ppm"},
    ]


def image_suite() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_suite",
        "suite_state": "image_fixture_suite_ready",
        "target_candidate": target_candidate(),
        "fixture_count": 2,
        "fixtures": fixtures(),
        **SAFETY_FALSE,
    }


def image_validation() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_validation",
        "validation_state": "image_fixture_validation_passed_no_load",
        "validation_passed": True,
        "target_candidate": target_candidate(),
        "summary": {"fixture_count": 2, "passed_count": 2, "failed_count": 0},
        "fixture_results": [
            {"case_id": "first", "pattern": "gradient", "validation_status": "passed"},
            {"case_id": "second", "pattern": "checker", "validation_status": "passed"},
        ],
        **SAFETY_FALSE,
    }


def image_suite_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_suite_worker_selftest",
        "suite_selftest_state": "image_suite_worker_selftest_passed",
        "target_candidate": target_candidate(),
        "fixture_count": 2,
        "fixture_results": [
            {"case_id": "first", "identity_check": {"pixel_match": True, "dimension_match": True}},
            {"case_id": "second", "identity_check": {"pixel_match": True, "dimension_match": True}},
        ],
        **SAFETY_FALSE,
    }


def ofx_suite_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_suite_noop_selftest",
        "ofx_suite_selftest_state": "ofx_suite_noop_identity_passed_route_closed",
        "target_candidate": target_candidate(),
        "fixture_count": 2,
        "fixture_results": [
            {
                "case_id": "first",
                "mock_state": "mock_identity_completed_route_closed",
                "identity_check": {"pixel_match": True, "dimension_match": True},
            },
            {
                "case_id": "second",
                "mock_state": "mock_identity_completed_route_closed",
                "identity_check": {"pixel_match": True, "dimension_match": True},
            },
        ],
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


def build_report(**overrides: dict) -> dict:
    return aex_candidate_no_load_test_runner_dryrun.build_candidate_no_load_test_runner_dryrun(
        candidate_handoff=overrides.get("candidate_handoff") or candidate_handoff(),
        candidate_handoff_path=LAB_ROOT / "target" / "candidate-test-handoff" / "handoff.local.json",
        image_suite=overrides.get("image_suite") or image_suite(),
        image_suite_path=LAB_ROOT / "target" / "image-fixture-suite" / "suite.local.json",
        image_validation=overrides.get("image_validation") or image_validation(),
        image_validation_path=LAB_ROOT / "target" / "image-fixture-validation" / "validation.local.json",
        image_suite_selftest=overrides.get("image_suite_selftest") or image_suite_selftest(),
        image_suite_selftest_path=LAB_ROOT / "target" / "image-suite-selftest" / "worker.local.json",
        ofx_suite_selftest=overrides.get("ofx_suite_selftest") or ofx_suite_selftest(),
        ofx_suite_selftest_path=LAB_ROOT / "target" / "ofx-suite-selftest" / "ofx.local.json",
        image_input_smoke=overrides.get("image_input_smoke") or image_input_smoke(),
        image_input_smoke_path=LAB_ROOT / "target" / "image-input-smoke" / "smoke.local.json",
        render_validation_contract=overrides.get("render_validation_contract") or render_validation_contract(),
        render_validation_contract_path=LAB_ROOT / "target" / "render-validation-contract" / "render.local.json",
        ofx_route_contract=overrides.get("ofx_route_contract") or ofx_route_contract(),
        ofx_route_contract_path=LAB_ROOT / "target" / "ofx-route-contract" / "ofx-route.local.json",
    )


class AexCandidateNoLoadTestRunnerDryrunTests(unittest.TestCase):
    def test_builds_dryrun_manifest_without_execution(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_candidate_no_load_test_runner_dryrun")
        self.assertEqual(report["runner_dryrun_state"], "candidate_no_load_test_runner_dryrun_ready_native_closed")
        self.assertTrue(report["runner_dryrun_ready"])
        self.assertTrue(report["dry_run_only"])
        self.assertFalse(report["would_execute"])
        self.assertFalse(report["execution_performed"])
        self.assertTrue(report["no_load_test_plan_ready"])
        self.assertFalse(report["native_test_plan_ready"])
        self.assertEqual(report["image_fixture_case_count"], 2)
        self.assertEqual(report["planned_no_load_case_count"], 16)
        self.assertEqual(report["planned_native_case_count"], 0)
        self.assertTrue(report["image_fixture_validation_passed"])
        self.assertTrue(report["worker_suite_identity_passed"])
        self.assertTrue(report["ofx_suite_identity_passed"])
        self.assertTrue(report["image_smoke_identity_passed"])
        self.assertTrue(report["render_contract_review_ready"])
        self.assertTrue(report["ofx_route_contract_review_ready"])
        self.assertEqual(report["native_load_gate"], "closed")
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["aex_path_acceptance_enabled"])
        self.assertFalse(report["real_render_open"])
        self.assertFalse(report["real_route_open"])
        self.assertIn("accept_aex_path", [item["case_id"] for item in report["blocked_cases"]])
        self.assertIn("--aex-path", report["forbidden_cli_inputs"])
        self.assertIn("APPROVE_AEX_LOAD_GATE", report["forbidden_cli_inputs"])
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["raw_payload_serialized"])

    def test_rejects_case_id_mismatch(self):
        validation = image_validation()
        validation["fixture_results"][1]["case_id"] = "different"
        with self.assertRaises(ValueError) as ctx:
            build_report(image_validation=validation)
        self.assertIn("case IDs must match", str(ctx.exception))

    def test_rejects_open_render_contract(self):
        render = render_validation_contract()
        render["real_render_open"] = True
        with self.assertRaises(ValueError) as ctx:
            build_report(render_validation_contract=render)
        self.assertIn("real_render_open must be false", str(ctx.exception))

    def test_rejects_native_ready_handoff(self):
        handoff = candidate_handoff()
        handoff["native_test_handoff_ready"] = True
        with self.assertRaises(ValueError) as ctx:
            build_report(candidate_handoff=handoff)
        self.assertIn("native_test_handoff_ready must be false", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "handoff": LAB_ROOT / "target" / "candidate-test-handoff",
            "suite": LAB_ROOT / "target" / "image-fixture-suite",
            "validation": LAB_ROOT / "target" / "image-fixture-validation",
            "worker": LAB_ROOT / "target" / "image-suite-selftest",
            "ofx": LAB_ROOT / "target" / "ofx-suite-selftest",
            "smoke": LAB_ROOT / "target" / "image-input-smoke",
            "render": LAB_ROOT / "target" / "render-validation-contract",
            "route": LAB_ROOT / "target" / "ofx-route-contract",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        paths = {
            "handoff": roots["handoff"] / f"{time.time_ns()}-{os.getpid()}-handoff.local.json",
            "suite": roots["suite"] / f"{time.time_ns()}-{os.getpid()}-suite.local.json",
            "validation": roots["validation"] / f"{time.time_ns()}-{os.getpid()}-validation.local.json",
            "worker": roots["worker"] / f"{time.time_ns()}-{os.getpid()}-worker.local.json",
            "ofx": roots["ofx"] / f"{time.time_ns()}-{os.getpid()}-ofx.local.json",
            "smoke": roots["smoke"] / f"{time.time_ns()}-{os.getpid()}-smoke.local.json",
            "render": roots["render"] / f"{time.time_ns()}-{os.getpid()}-render.local.json",
            "route": roots["route"] / f"{time.time_ns()}-{os.getpid()}-route.local.json",
        }
        payloads = {
            "handoff": candidate_handoff(),
            "suite": image_suite(),
            "validation": image_validation(),
            "worker": image_suite_selftest(),
            "ofx": ofx_suite_selftest(),
            "smoke": image_input_smoke(),
            "render": render_validation_contract(),
            "route": ofx_route_contract(),
        }
        for key, payload in payloads.items():
            paths[key].write_text(json.dumps(payload), encoding="utf-8")

        handoff, handoff_path = aex_candidate_no_load_test_runner_dryrun.load_candidate_handoff(paths["handoff"])
        suite, suite_path = aex_candidate_no_load_test_runner_dryrun.load_image_suite(paths["suite"])
        validation, validation_path = aex_candidate_no_load_test_runner_dryrun.load_image_validation(paths["validation"])
        worker, worker_path = aex_candidate_no_load_test_runner_dryrun.load_image_suite_selftest(paths["worker"])
        ofx, ofx_path = aex_candidate_no_load_test_runner_dryrun.load_ofx_suite_selftest(paths["ofx"])
        smoke, smoke_path = aex_candidate_no_load_test_runner_dryrun.load_image_input_smoke(paths["smoke"])
        render, render_path = aex_candidate_no_load_test_runner_dryrun.load_render_validation_contract(paths["render"])
        route, route_path = aex_candidate_no_load_test_runner_dryrun.load_ofx_route_contract(paths["route"])

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-runner.local.json"
        outside.write_text(json.dumps(candidate_handoff()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_no_load_test_runner_dryrun.load_candidate_handoff(outside)

        report = aex_candidate_no_load_test_runner_dryrun.build_candidate_no_load_test_runner_dryrun(
            candidate_handoff=handoff,
            candidate_handoff_path=handoff_path,
            image_suite=suite,
            image_suite_path=suite_path,
            image_validation=validation,
            image_validation_path=validation_path,
            image_suite_selftest=worker,
            image_suite_selftest_path=worker_path,
            ofx_suite_selftest=ofx,
            ofx_suite_selftest_path=ofx_path,
            image_input_smoke=smoke,
            image_input_smoke_path=smoke_path,
            render_validation_contract=render,
            render_validation_contract_path=render_path,
            ofx_route_contract=route,
            ofx_route_contract_path=route_path,
        )
        out = LAB_ROOT / "target" / "candidate-test-runner-dryrun" / f"{time.time_ns()}-{os.getpid()}-runner.local.json"
        written = aex_candidate_no_load_test_runner_dryrun.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_no_load_test_runner_dryrun.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_no_load_test_runner_dryrun.write_json_create_new(
                LAB_ROOT / "target" / "outside-runner.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
