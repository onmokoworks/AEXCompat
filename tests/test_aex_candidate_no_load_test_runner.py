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


aex_candidate_no_load_test_runner = load_tool("aex_candidate_no_load_test_runner")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


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


def create_ppm(name: str, width: int, height: int, pattern: str) -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{os.getpid()}-{name}.ppm"
    ppm_fixture_tool.write_ppm_create_new(path, ppm_fixture_tool.generate_image(width, height, pattern))
    return path


def target_candidate() -> dict:
    return {
        "relative_path": CANDIDATE,
        "candidate_policy_state": "eligible_for_manual_policy_review",
        "native_load_approval": "not_granted",
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


def image_suite(fixtures: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_suite",
        "suite_state": "image_fixture_suite_ready",
        "target_candidate": target_candidate(),
        "fixture_count": len(fixtures),
        "fixtures": fixtures,
        **SAFETY_FALSE,
    }


def image_validation(fixtures: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_validation",
        "validation_state": "image_fixture_validation_passed_no_load",
        "validation_passed": True,
        "target_candidate": target_candidate(),
        "summary": {"fixture_count": len(fixtures), "passed_count": len(fixtures), "failed_count": 0},
        "fixture_results": [
            {"case_id": fixture["case_id"], "pattern": fixture["pattern"], "validation_status": "passed"}
            for fixture in fixtures
        ],
        **SAFETY_FALSE,
    }


def image_suite_selftest(fixtures: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_suite_worker_selftest",
        "suite_selftest_state": "image_suite_worker_selftest_passed",
        "target_candidate": target_candidate(),
        "fixture_count": len(fixtures),
        "fixture_results": [
            {"case_id": fixture["case_id"], "identity_check": {"pixel_match": True, "dimension_match": True}}
            for fixture in fixtures
        ],
        **SAFETY_FALSE,
    }


def ofx_suite_selftest(fixtures: list[dict]) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_suite_noop_selftest",
        "ofx_suite_selftest_state": "ofx_suite_noop_identity_passed_route_closed",
        "target_candidate": target_candidate(),
        "fixture_count": len(fixtures),
        "fixture_results": [
            {
                "case_id": fixture["case_id"],
                "mock_state": "mock_identity_completed_route_closed",
                "identity_check": {"pixel_match": True, "dimension_match": True},
            }
            for fixture in fixtures
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


def ofx_packet() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_ofx_facade_deferred_packet",
        "facade_state": "deferred_loader_not_ready",
        "source_stub_state": "refused_gate_closed",
        "ofx_route_action": "no_op",
        "primary_review_candidate": target_candidate(),
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "blocked_actions": ["ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx"],
    }


def write_json(root_name: str, name: str, payload: dict) -> Path:
    root = LAB_ROOT / "target" / root_name
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{os.getpid()}-{name}.local.json"
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


def write_source_bundle() -> dict:
    first = create_ppm("runner-first", 5, 4, "gradient")
    second = create_ppm("runner-second", 7, 3, "checker")
    fixtures = [
        {"case_id": "first", "pattern": "gradient", "ppm_path": str(first)},
        {"case_id": "second", "pattern": "checker", "ppm_path": str(second)},
    ]
    paths = {
        "candidate_handoff": write_json("candidate-test-handoff", "handoff", candidate_handoff()),
        "image_suite": write_json("image-fixture-suite", "suite", image_suite(fixtures)),
        "image_validation": write_json("image-fixture-validation", "validation", image_validation(fixtures)),
        "image_suite_selftest": write_json("image-suite-selftest", "worker", image_suite_selftest(fixtures)),
        "ofx_suite_selftest": write_json("ofx-suite-selftest", "ofx", ofx_suite_selftest(fixtures)),
        "image_input_smoke": write_json("image-input-smoke", "smoke", image_input_smoke()),
        "render_validation_contract": write_json(
            "render-validation-contract",
            "render",
            render_validation_contract(),
        ),
        "ofx_route_contract": write_json("ofx-route-contract", "route", ofx_route_contract()),
        "ofx_packet": write_json("ofx-facade", "ofx-packet", ofx_packet()),
    }
    payloads, source_paths, expected = aex_candidate_no_load_test_runner.load_and_validate_sources(
        candidate_handoff_path=paths["candidate_handoff"],
        image_suite_path=paths["image_suite"],
        image_validation_path=paths["image_validation"],
        image_suite_selftest_path=paths["image_suite_selftest"],
        ofx_suite_selftest_path=paths["ofx_suite_selftest"],
        image_input_smoke_path=paths["image_input_smoke"],
        render_validation_contract_path=paths["render_validation_contract"],
        ofx_route_contract_path=paths["ofx_route_contract"],
    )
    dryrun_path = write_json("candidate-test-runner-dryrun", "runner-dryrun", expected)
    return {
        "paths": paths,
        "source_payloads": payloads,
        "source_paths": source_paths,
        "expected": expected,
        "runner_dryrun_path": dryrun_path,
    }


class AexCandidateNoLoadTestRunnerTests(unittest.TestCase):
    def test_executes_only_worker_and_ofx_noop_identity(self):
        bundle = write_source_bundle()
        runner_dryrun, runner_dryrun_path = aex_candidate_no_load_test_runner.load_runner_dryrun(
            bundle["runner_dryrun_path"]
        )
        ofx_packet_payload = json.loads(bundle["paths"]["ofx_packet"].read_text(encoding="utf-8"))
        report = aex_candidate_no_load_test_runner.build_candidate_no_load_test_runner(
            runner_dryrun=runner_dryrun,
            runner_dryrun_path=runner_dryrun_path,
            source_payloads=bundle["source_payloads"],
            source_paths=bundle["source_paths"],
            expected_dryrun_report=bundle["expected"],
            ofx_packet=ofx_packet_payload,
            ofx_packet_path=bundle["paths"]["ofx_packet"],
            worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
            output_prefix=f"{time.time_ns()}-{os.getpid()}-candidate-runner",
        )

        self.assertEqual(report["report_kind"], "aex_candidate_no_load_test_runner")
        self.assertEqual(report["runner_state"], "candidate_no_load_test_runner_passed_native_closed")
        self.assertTrue(report["runner_ready"])
        self.assertFalse(report["dry_run_only"])
        self.assertTrue(report["execution_performed"])
        self.assertTrue(report["no_load_execution_performed"])
        self.assertFalse(report["native_execution_performed"])
        self.assertTrue(report["worker_invoked"])
        self.assertTrue(report["ofx_mock_invoked"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertTrue(report["worker_identity_passed"])
        self.assertTrue(report["ofx_noop_identity_passed"])
        self.assertTrue(report["blocked_load_aex_verified"])
        self.assertEqual(report["executed_worker_case_count"], 2)
        self.assertEqual(report["executed_ofx_noop_case_count"], 2)
        self.assertEqual(report["executed_native_case_count"], 0)
        self.assertEqual(report["native_load_gate"], "closed")
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["aex_path_acceptance_enabled"])
        self.assertFalse(report["real_render_open"])
        self.assertFalse(report["real_route_open"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertIn("--aex-path", report["forbidden_cli_inputs"])
        for result in report["worker_report"]["fixture_results"] + report["ofx_noop_report"]["fixture_results"]:
            self.assertTrue(Path(result["output_ppm"]).exists())
            self.assertTrue(result["identity_check"]["pixel_match"])
            self.assertTrue(result["identity_check"]["dimension_match"])

    def test_rejects_dryrun_that_already_executed_or_has_native_plan(self):
        bundle = write_source_bundle()
        runner_dryrun = dict(bundle["expected"])
        runner_dryrun["execution_performed"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_candidate_no_load_test_runner.build_candidate_no_load_test_runner(
                runner_dryrun=runner_dryrun,
                runner_dryrun_path=bundle["runner_dryrun_path"],
                source_payloads=bundle["source_payloads"],
                source_paths=bundle["source_paths"],
                expected_dryrun_report=bundle["expected"],
                ofx_packet=ofx_packet(),
                ofx_packet_path=bundle["paths"]["ofx_packet"],
                worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                output_prefix=f"{time.time_ns()}-{os.getpid()}-bad-runner",
            )
        self.assertIn("execution_performed must be false", str(ctx.exception))

        runner_dryrun = dict(bundle["expected"])
        runner_dryrun["native_test_plan_ready"] = True
        runner_dryrun["planned_native_case_count"] = 1
        with self.assertRaises(ValueError) as ctx:
            aex_candidate_no_load_test_runner.build_candidate_no_load_test_runner(
                runner_dryrun=runner_dryrun,
                runner_dryrun_path=bundle["runner_dryrun_path"],
                source_payloads=bundle["source_payloads"],
                source_paths=bundle["source_paths"],
                expected_dryrun_report=bundle["expected"],
                ofx_packet=ofx_packet(),
                ofx_packet_path=bundle["paths"]["ofx_packet"],
                worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                output_prefix=f"{time.time_ns()}-{os.getpid()}-bad-runner",
            )
        self.assertIn("native_test_plan_ready must be false", str(ctx.exception))

    def test_rejects_forbidden_cli_tokens(self):
        aex_candidate_no_load_test_runner.reject_forbidden_cli_inputs(["--ofx-packet", "packet.json"])
        for token in ("--aex", "--aex-path=private.aex", "--render", "APPROVE_AEX_LOAD_GATE"):
            with self.assertRaises(ValueError):
                aex_candidate_no_load_test_runner.reject_forbidden_cli_inputs([token])

    def test_paths_are_confined_and_report_is_create_new(self):
        bundle = write_source_bundle()
        loaded, resolved = aex_candidate_no_load_test_runner.load_runner_dryrun(bundle["runner_dryrun_path"])
        self.assertEqual(loaded["report_kind"], "aex_candidate_no_load_test_runner_dryrun")
        self.assertEqual(resolved, bundle["runner_dryrun_path"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-runner-dryrun.local.json"
        outside.write_text(json.dumps(bundle["expected"]), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_no_load_test_runner.load_runner_dryrun(outside)

        payload = {
            "schema_version": 1,
            "publication_status": "local-only",
            "report_kind": "aex_candidate_no_load_test_runner",
        }
        out = LAB_ROOT / "target" / "candidate-test-runner" / f"{time.time_ns()}-{os.getpid()}-runner.local.json"
        written = aex_candidate_no_load_test_runner.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_no_load_test_runner.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_candidate_no_load_test_runner.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-runner.json",
                payload,
            )


if __name__ == "__main__":
    unittest.main()
