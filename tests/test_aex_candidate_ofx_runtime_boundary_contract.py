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


aex_candidate_ofx_runtime_boundary_contract = load_tool("aex_candidate_ofx_runtime_boundary_contract")


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


def bridge_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_bridge_packet",
        "bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "bridge_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "bridge_allowed_route": "no_op_identity_only",
        "mock_route_ready": True,
        "real_route_open": False,
        "real_ofx_route_ready": False,
        "ofx_runtime_invoked": False,
        "aex_runtime_invoked": False,
        "ofx_describe_ready": False,
        "ofx_render_ready": False,
        "render_equivalence_claim_ready": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "ofx_bridge_path_payload_exported": False,
        **SAFETY_FALSE,
    }


def dryrun_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_dryrun",
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
        "harness_dryrun_ready": True,
        "dry_run_only": True,
        "would_execute": False,
        "execution_performed": False,
        "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "source_bridge_allowed_route": "no_op_identity_only",
        "planned_real_describe_case_count": 0,
        "planned_real_render_case_count": 0,
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
        **SAFETY_FALSE,
    }


def harness_selftest_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_ofx_host_harness_selftest",
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "host_harness_selftest_state": "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        "host_harness_selftest_ready": True,
        "host_harness_kind": "ofx_noop_host_harness_synthetic_selftest",
        "synthetic_only": True,
        "synthetic_contract_checks_performed": True,
        "real_harness_execution_performed": False,
        "source_harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
        "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
        "source_bridge_allowed_route": "no_op_identity_only",
        "checked_case_count": 2,
        "checked_noop_describe_case_count": 1,
        "checked_noop_render_case_count": 1,
        "checked_real_describe_case_count": 0,
        "checked_real_render_case_count": 0,
        "case_passed_count": 2,
        "descriptor_contract_checked": True,
        "render_identity_contract_checked": True,
        "ppm_pixel_read_performed": False,
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
        **SAFETY_FALSE,
    }


def ofx_route_contract_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
        "route_contract": {
            "real_route_open": False,
            "mock_route_ready": True,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
            "allowed_route": "no_op_identity_only",
            "forbidden_route": "real_aex_backed_ofx_describe_or_render",
        },
        "describe_contract": {"state": "blocked_pending_native_loader_and_schema"},
        "render_contract": {"state": "blocked_pending_render_harness"},
        "blocked_actions": [
            "build_ofx_binary",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "route_through_ofx",
        ],
        **SAFETY_FALSE,
    }


def native_runtime_contract_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_contract",
        "native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
        "contract_state": "runtime_containment_contract_ready_path_acceptance_closed",
        "runtime_containment_ready": True,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "runtime_approval_required_before_load": True,
        "native_load_gate": "closed",
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "process_isolation_required": True,
        "controller_loads_aex": False,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate_dependencies_clear": True,
        "fixture_approval_satisfied": False,
        "blocked_actions": [
            "accept_aex_path",
            "open_aex_file",
            "load_aex_dll",
            "call_EffectMain",
            "render_with_aex",
        ],
        **SAFETY_FALSE,
    }


def write_payload(root_name: str, name: str, payload: dict) -> Path:
    root = LAB_ROOT / "target" / root_name
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


def build_report() -> dict:
    stamp = time.time_ns()
    return aex_candidate_ofx_runtime_boundary_contract.build_boundary_contract(
        candidate_ofx_bridge=bridge_payload(),
        candidate_ofx_bridge_path=Path(f"target/candidate-ofx-bridge/bridge-{stamp}.json"),
        host_harness_dryrun=dryrun_payload(),
        host_harness_dryrun_path=Path(f"target/candidate-ofx-host-harness-dryrun/dryrun-{stamp}.json"),
        host_harness_selftest=harness_selftest_payload(),
        host_harness_selftest_path=Path(f"target/candidate-ofx-host-harness-selftest/selftest-{stamp}.json"),
        ofx_route_contract=ofx_route_contract_payload(),
        ofx_route_contract_path=Path(f"target/ofx-route-contract/route-{stamp}.json"),
        native_runtime_contract=native_runtime_contract_payload(),
        native_runtime_contract_path=Path(f"target/native-loader-runtime-contract/native-{stamp}.json"),
    )


class AexCandidateOfxRuntimeBoundaryContractTests(unittest.TestCase):
    def test_builds_no_load_runtime_boundary_contract(self):
        report = build_report()

        self.assertEqual(report["report_kind"], "aex_candidate_ofx_runtime_boundary_contract")
        self.assertEqual(
            report["candidate_ofx_runtime_boundary_contract_state"],
            "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed",
        )
        self.assertEqual(report["contract_state"], "candidate_ofx_runtime_boundary_contract_ready_runtime_closed")
        self.assertTrue(report["runtime_boundary_ready"])
        self.assertTrue(report["boundary_contract_ready"])
        self.assertEqual(report["source_bridge_state"], "candidate_ofx_bridge_ready_no_load_route_closed")
        self.assertEqual(report["source_harness_dryrun_state"], "candidate_ofx_host_harness_dryrun_ready_route_closed")
        self.assertEqual(
            report["source_host_harness_selftest_state"],
            "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        )
        self.assertEqual(report["source_ofx_route_contract_state"], "ofx_route_contract_ready_route_closed")
        self.assertEqual(report["source_native_runtime_contract_state"], "runtime_containment_contract_ready_no_load")
        self.assertFalse(report["ofx_runtime_allowed_now"])
        self.assertFalse(report["ofx_runtime_invocation_ready"])
        self.assertFalse(report["ofx_runtime_instantiation_performed"])
        self.assertFalse(report["host_process_launch_enabled"])
        self.assertFalse(report["ofx_host_path_payload_supplied"])
        self.assertFalse(report["ofx_plugin_binary_path_payload_supplied"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["aex_path_acceptance_enabled"])
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["real_route_open"])
        self.assertTrue(report["mock_route_ready"])
        self.assertTrue(report["no_op_identity_route_preserved"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertFalse(report["ppm_pixel_read_performed"])
        self.assertFalse(report["runtime_boundary_path_payload_exported"])
        self.assertTrue(report["runtime_approval_required_before_invocation"])
        self.assertTrue(report["requires_future_fixture_approval"])
        self.assertTrue(report["requires_future_render_validation_approval"])
        self.assertEqual(report["approval_gate_count"], 6)
        self.assertIn("instantiate_ofx_runtime", report["blocked_actions"])
        self.assertIn("accept_ofx_host_path", report["blocked_actions"])
        self.assertIn("load_ofx_plugin", report["blocked_actions"])

    def test_rejects_open_runtime_or_route_evidence(self):
        bridge = bridge_payload()
        bridge["real_route_open"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_boundary_contract.build_boundary_contract(
                candidate_ofx_bridge=bridge,
                candidate_ofx_bridge_path=Path("target/candidate-ofx-bridge/bridge.json"),
                host_harness_dryrun=dryrun_payload(),
                host_harness_dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
                host_harness_selftest=harness_selftest_payload(),
                host_harness_selftest_path=Path("target/candidate-ofx-host-harness-selftest/selftest.json"),
                ofx_route_contract=ofx_route_contract_payload(),
                ofx_route_contract_path=Path("target/ofx-route-contract/route.json"),
                native_runtime_contract=native_runtime_contract_payload(),
                native_runtime_contract_path=Path("target/native-loader-runtime-contract/native.json"),
            )

        selftest = harness_selftest_payload()
        selftest["real_harness_execution_performed"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_boundary_contract.build_boundary_contract(
                candidate_ofx_bridge=bridge_payload(),
                candidate_ofx_bridge_path=Path("target/candidate-ofx-bridge/bridge.json"),
                host_harness_dryrun=dryrun_payload(),
                host_harness_dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
                host_harness_selftest=selftest,
                host_harness_selftest_path=Path("target/candidate-ofx-host-harness-selftest/selftest.json"),
                ofx_route_contract=ofx_route_contract_payload(),
                ofx_route_contract_path=Path("target/ofx-route-contract/route.json"),
                native_runtime_contract=native_runtime_contract_payload(),
                native_runtime_contract_path=Path("target/native-loader-runtime-contract/native.json"),
            )

        native = native_runtime_contract_payload()
        native["path_acceptance_ready"] = True
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_boundary_contract.build_boundary_contract(
                candidate_ofx_bridge=bridge_payload(),
                candidate_ofx_bridge_path=Path("target/candidate-ofx-bridge/bridge.json"),
                host_harness_dryrun=dryrun_payload(),
                host_harness_dryrun_path=Path("target/candidate-ofx-host-harness-dryrun/dryrun.json"),
                host_harness_selftest=harness_selftest_payload(),
                host_harness_selftest_path=Path("target/candidate-ofx-host-harness-selftest/selftest.json"),
                ofx_route_contract=ofx_route_contract_payload(),
                ofx_route_contract_path=Path("target/ofx-route-contract/route.json"),
                native_runtime_contract=native,
                native_runtime_contract_path=Path("target/native-loader-runtime-contract/native.json"),
            )

    def test_loads_sources_and_writes_create_new_under_root(self):
        stamp = time.time_ns()
        bridge_path = write_payload(
            "candidate-ofx-bridge",
            f"ae-candidate-ofx-bridge-{stamp}.local.json",
            bridge_payload(),
        )
        dryrun_path = write_payload(
            "candidate-ofx-host-harness-dryrun",
            f"ae-candidate-ofx-host-harness-dryrun-{stamp}.local.json",
            dryrun_payload(),
        )
        selftest_path = write_payload(
            "candidate-ofx-host-harness-selftest",
            f"ae-candidate-ofx-host-harness-selftest-{stamp}.local.json",
            harness_selftest_payload(),
        )
        route_path = write_payload(
            "ofx-route-contract",
            f"ae-ofx-route-contract-{stamp}.local.json",
            ofx_route_contract_payload(),
        )
        native_path = write_payload(
            "native-loader-runtime-contract",
            f"ae-native-loader-runtime-contract-{stamp}.local.json",
            native_runtime_contract_payload(),
        )
        bridge, resolved_bridge = aex_candidate_ofx_runtime_boundary_contract.load_candidate_ofx_bridge(
            Path("target") / "candidate-ofx-bridge" / bridge_path.name
        )
        dryrun, resolved_dryrun = aex_candidate_ofx_runtime_boundary_contract.load_harness_dryrun(
            Path("target") / "candidate-ofx-host-harness-dryrun" / dryrun_path.name
        )
        selftest, resolved_selftest = aex_candidate_ofx_runtime_boundary_contract.load_harness_selftest(
            Path("target") / "candidate-ofx-host-harness-selftest" / selftest_path.name
        )
        route, resolved_route = aex_candidate_ofx_runtime_boundary_contract.load_ofx_route_contract(
            Path("target") / "ofx-route-contract" / route_path.name
        )
        native, resolved_native = aex_candidate_ofx_runtime_boundary_contract.load_native_runtime_contract(
            Path("target") / "native-loader-runtime-contract" / native_path.name
        )
        report = aex_candidate_ofx_runtime_boundary_contract.build_boundary_contract(
            candidate_ofx_bridge=bridge,
            candidate_ofx_bridge_path=resolved_bridge,
            host_harness_dryrun=dryrun,
            host_harness_dryrun_path=resolved_dryrun,
            host_harness_selftest=selftest,
            host_harness_selftest_path=resolved_selftest,
            ofx_route_contract=route,
            ofx_route_contract_path=resolved_route,
            native_runtime_contract=native,
            native_runtime_contract_path=resolved_native,
        )
        out = (
            LAB_ROOT
            / "target"
            / "candidate-ofx-runtime-boundary-contract"
            / f"ae-candidate-ofx-runtime-boundary-contract-{stamp}.local.json"
        )
        written = aex_candidate_ofx_runtime_boundary_contract.write_json_create_new(out, report)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_ofx_runtime_boundary_contract.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_boundary_contract.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-ofx-runtime-boundary-contract.json", report
            )
        with self.assertRaises(ValueError):
            aex_candidate_ofx_runtime_boundary_contract.load_harness_selftest(
                LAB_ROOT / "target" / "outside-candidate-ofx-host-harness-selftest.json"
            )


if __name__ == "__main__":
    unittest.main()
