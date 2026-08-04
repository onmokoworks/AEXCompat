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


aex_candidate_ofx_host_harness_selftest = load_tool("aex_candidate_ofx_host_harness_selftest")


SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
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
    "pipl_payload_parsed": False,
    "parameter_schema_emitted": False,
    "redacted_schema_emitted": False,
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "resource_payload_opened": False,
    "resource_payload_extracted": False,
    "raw_payload_serialized": False,
}


BLOCKED = [
    "accept_aex_path",
    "open_aex_file",
    "hash_aex_file",
    "copy_selected_aex_fixture",
    "load_aex_dll",
    "load_dependency_dll",
    "call_EffectMain",
    "dispatch_PF_Cmd",
    "start_after_effects",
    "render_with_aex",
    "route_through_real_ofx",
    "build_ofx_binary",
    "instantiate_ofx_runtime",
    "ofx_describe_from_aex",
    "ofx_render_with_aex",
    "read_ppm_pixels",
    "open_candidate_mock_ppm",
    "claim_render_equivalence",
    "parse_real_pipl_payload",
    "emit_parameter_schema",
    "emit_redacted_schema",
]


def dryrun_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_dryrun",
        "source_candidate_ofx_bridge": "target/candidate-ofx-bridge/ae-candidate-ofx-bridge-1.local.json",
        "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
        "harness_dryrun_ready": True,
        "dry_run_only": True,
        "would_execute": False,
        "execution_performed": False,
        "host_harness_kind": "ofx_noop_host_harness_planning",
        "allowed_harness_actions": [
            "plan_noop_describe_contract",
            "plan_noop_render_identity_contract",
            "reuse_candidate_mock_image_surface_relative_paths",
        ],
        "planned_cases": [
            {
                "case_id": "noop_describe_contract",
                "phase": "describe",
                "would_execute": False,
                "planned_action": "validate_static_noop_describe_shape_only",
                "uses_aex_metadata": False,
                "uses_real_ofx_runtime": False,
                "blocked_real_action": "ofx_describe_from_aex",
            },
            {
                "case_id": "noop_render_identity_contract",
                "phase": "render",
                "would_execute": False,
                "planned_action": "bind_candidate_mock_output_as_future_noop_input",
                "candidate_mock_output_ppm_relative": (
                    "target/candidate-image-compat-mock/ae-candidate-image-compat-mock-1-invert.ppm"
                ),
                "uses_aex_pixels": False,
                "uses_real_ofx_runtime": False,
                "blocked_real_action": "ofx_render_with_aex",
            },
        ],
        "planned_case_count": 2,
        "planned_noop_describe_case_count": 1,
        "planned_noop_render_case_count": 1,
        "planned_real_describe_case_count": 0,
        "planned_real_render_case_count": 0,
        "source_bridge_allowed_route": "no_op_identity_only",
        "source_mock_route_ready": True,
        "source_real_route_open": False,
        "source_real_ofx_route_ready": False,
        "source_ofx_runtime_invoked": False,
        "source_aex_runtime_invoked": False,
        "source_ofx_describe_ready": False,
        "source_ofx_render_ready": False,
        "source_render_equivalence_claim_ready": False,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "host_harness_path_payload_exported": False,
        "requires_future_runtime_approval": True,
        "blocked_actions": BLOCKED,
        **SAFETY_FALSE,
    }


def write_dryrun(name: str, payload: dict | None = None) -> Path:
    root = LAB_ROOT / "target" / "candidate-ofx-host-harness-dryrun"
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload or dryrun_payload()), encoding="utf-8")
    return path


class AexCandidateOfxHostHarnessSelftestTests(unittest.TestCase):
    def test_builds_synthetic_no_load_selftest(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        dryrun_path = LAB_ROOT / "target" / "candidate-ofx-host-harness-dryrun" / f"ae-candidate-ofx-host-harness-dryrun-{stamp}.local.json"
        dryrun_path.parent.mkdir(parents=True, exist_ok=True)
        report = aex_candidate_ofx_host_harness_selftest.build_harness_selftest(
            dryrun=dryrun_payload(),
            dryrun_path=dryrun_path,
        )

        self.assertEqual(report["report_kind"], "aex_candidate_ofx_host_harness_selftest")
        self.assertEqual(
            report["host_harness_selftest_state"],
            "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        )
        self.assertTrue(report["host_harness_selftest_ready"])
        self.assertEqual(report["host_harness_kind"], "ofx_noop_host_harness_synthetic_selftest")
        self.assertEqual(report["selftest_state"], "candidate_ofx_host_harness_selftest_passed_no_load")
        self.assertTrue(report["selftest_ready"])
        self.assertTrue(report["synthetic_only"])
        self.assertTrue(report["synthetic_contract_checks_performed"])
        self.assertTrue(report["synthetic_contract_execution_performed"])
        self.assertFalse(report["real_execution_performed"])
        self.assertFalse(report["real_harness_execution_performed"])
        self.assertTrue(report["planned_cases_verified"])
        self.assertEqual(report["checked_case_count"], 2)
        self.assertEqual(report["checked_noop_describe_case_count"], 1)
        self.assertEqual(report["checked_noop_render_case_count"], 1)
        self.assertEqual(report["checked_real_describe_case_count"], 0)
        self.assertEqual(report["checked_real_render_case_count"], 0)
        self.assertEqual(report["case_result_count"], 2)
        self.assertEqual(report["case_passed_count"], 2)
        self.assertEqual(report["case_failed_count"], 0)
        self.assertTrue(report["descriptor_contract_checked"])
        self.assertTrue(report["render_identity_contract_checked"])
        self.assertTrue(report["synthetic_descriptor_created"])
        self.assertTrue(report["synthetic_render_contract_created"])
        self.assertTrue(report["candidate_mock_surface_reused_as_string_only"])
        self.assertFalse(report["ppm_pixel_read_performed"])
        self.assertFalse(report["real_route_open"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertFalse(report["ofx_render_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["pipl_payload_parsed"])
        self.assertTrue(report["requires_future_runtime_approval"])
        self.assertIn("instantiate_ofx_runtime", report["blocked_actions"])
        self.assertIn("read_ppm_pixels", report["blocked_actions"])
        self.assertIn("open_candidate_mock_ppm", report["blocked_actions"])
        self.assertIn("claim_render_equivalence", report["blocked_actions"])

    def test_rejects_executable_or_real_planned_dryrun(self):
        dryrun = dryrun_payload()
        dryrun["would_execute"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_selftest.build_harness_selftest(
                dryrun=dryrun,
                dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
            )

        dryrun = dryrun_payload()
        dryrun["planned_real_render_case_count"] = 1
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_selftest.build_harness_selftest(
                dryrun=dryrun,
                dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
            )

        dryrun = dryrun_payload()
        dryrun["planned_cases"][1]["candidate_mock_output_ppm_relative"] = "../escape.ppm"
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_selftest.build_harness_selftest(
                dryrun=dryrun,
                dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
            )

    def test_loads_dryrun_and_writes_create_new_under_root(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        dryrun_path = write_dryrun(f"ae-candidate-ofx-host-harness-dryrun-{stamp}.local.json")
        dryrun, resolved_dryrun = aex_candidate_ofx_host_harness_selftest.load_harness_dryrun(
            Path("target") / "candidate-ofx-host-harness-dryrun" / dryrun_path.name
        )
        report = aex_candidate_ofx_host_harness_selftest.build_harness_selftest(
            dryrun=dryrun,
            dryrun_path=resolved_dryrun,
        )
        out = LAB_ROOT / "target" / "candidate-ofx-host-harness-selftest" / f"ae-candidate-ofx-host-harness-selftest-{stamp}.local.json"
        written = aex_candidate_ofx_host_harness_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_host_harness_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_selftest.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-host-harness-selftest.json", report
            )
        with self.assertRaises(ValueError):
            aex_candidate_ofx_host_harness_selftest.load_harness_dryrun(
                LAB_ROOT / "target" / "outside-candidate-ofx-host-harness-dryrun.json"
            )


if __name__ == "__main__":
    unittest.main()
