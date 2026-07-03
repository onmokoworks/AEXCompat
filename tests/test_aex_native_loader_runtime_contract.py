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


aex_native_loader_runtime_contract = load_tool("aex_native_loader_runtime_contract")


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


def design_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_design_contract",
        "native_loader_design_state": "native_loader_design_ready_loader_closed",
        "contract_state": "native_loader_design_contract_ready_loader_closed_pending_fixture_approval",
        "loader_design_ready": True,
        "candidate_relative_path": "Adobe After Effects 2025/Support Files/Plug-ins/Effects/ScatterMap.aex",
        "native_load_gate": "closed",
        "approval_required_before_aex_path": True,
        "runtime_approval_required_before_load": True,
        "separate_process_required": True,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "controller_loads_aex": False,
        "candidate_dependencies_clear": True,
        "fixture_approval_satisfied": False,
        "source_stub_state": "refused_gate_closed",
        "source_sandbox_policy_state": "policy_ready_no_native_load",
        "source_render_contract_state": "render_validation_contract_ready_render_closed",
        "source_ofx_route_contract_state": "ofx_route_contract_ready_route_closed",
        "blocked_actions": [
            "accept_aex_path",
            "open_aex_file",
            "load_aex_dll",
            "call_EffectMain",
            "render_with_aex",
        ],
        **SAFETY_FALSE,
    }


def broker_selftest() -> dict:
    blocked_checks = [
        {"message_type": message_type, "code": "blocked_action", "path_payload_supplied": False}
        for message_type in (
            "accept_aex_path",
            "open_aex_file",
            "load_aex_dll",
            "call_effect_main",
            "render_frame",
            "route_through_ofx",
        )
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_broker_selftest",
        "source_native_loader_design_state": "native_loader_design_ready_loader_closed",
        "broker_selftest_state": "pathless_native_loader_broker_selftest_passed",
        "pathless_broker_ready": True,
        "native_loader_design_ready": True,
        "candidate_relative_path": "Adobe After Effects 2025/Support Files/Plug-ins/Effects/ScatterMap.aex",
        "candidate_dependencies_clear": True,
        "fixture_approval_satisfied": False,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "blocked_action_count": len(blocked_checks),
        "blocked_action_checks": blocked_checks,
        "steps": [{"step": "hello"}, {"step": "inspect_environment"}, {"step": "quit"}],
        **SAFETY_FALSE,
    }


def candidate_load_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_load_gate_dryrun",
        "candidate_load_gate_dryrun_state": "candidate_load_gate_dryrun_ready_no_load",
        "candidate_scoped_load_gate_dry_run_state": (
            "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
        ),
        "candidate_relative_path": "Adobe After Effects 2025/Support Files/Plug-ins/Effects/ScatterMap.aex",
        "native_load_gate": "closed",
        "fixture_approval_satisfied": False,
        "candidate_dependencies_clear": True,
        "candidate_dependency_blockers_present": False,
        **SAFETY_FALSE,
    }


def sandbox_policy() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_sandbox_policy_packet",
        "sandbox_policy_state": "policy_ready_no_native_load",
        "required_before_native_load": [
            "explicit user fixture approval artifact",
            "passing load gate using the approved fixture",
            "worker process isolation design review",
        ],
        **SAFETY_FALSE,
    }


def build_contract(
    design: dict | None = None,
    broker: dict | None = None,
    gate: dict | None = None,
    policy: dict | None = None,
) -> dict:
    return aex_native_loader_runtime_contract.build_runtime_contract(
        native_loader_design=design or design_contract(),
        native_loader_design_path=LAB_ROOT / "target" / "native-loader-design" / "design.local.json",
        broker_selftest=broker or broker_selftest(),
        broker_selftest_path=LAB_ROOT / "target" / "native-loader-broker-selftest" / "broker.local.json",
        candidate_load_gate=gate or candidate_load_gate(),
        candidate_load_gate_path=LAB_ROOT / "target" / "candidate-load-gate" / "gate.local.json",
        sandbox_policy=policy or sandbox_policy(),
        sandbox_policy_path=LAB_ROOT / "target" / "sandbox-policy" / "policy.local.json",
    )


class AexNativeLoaderRuntimeContractTests(unittest.TestCase):
    def test_builds_closed_runtime_containment_contract(self):
        contract = build_contract()
        self.assertEqual(contract["report_kind"], "aex_native_loader_runtime_contract")
        self.assertEqual(contract["native_loader_runtime_contract_state"], "runtime_containment_contract_ready_no_load")
        self.assertTrue(contract["runtime_containment_ready"])
        self.assertEqual(contract["path_allowlist_state"], "closed_no_aex_paths_accepted")
        self.assertFalse(contract["path_acceptance_ready"])
        self.assertFalse(contract["aex_path_acceptance_enabled"])
        self.assertIsNone(contract["accepted_aex_path"])
        self.assertFalse(contract["path_payload_supplied"])
        self.assertTrue(contract["broker_selftest_passed"])
        self.assertTrue(contract["process_isolation_required"])
        self.assertEqual(contract["path_policy"]["state"], "closed_no_aex_paths_accepted")
        self.assertTrue(contract["timeout_policy"]["requires_review_before_first_load"])
        self.assertEqual(contract["crash_containment_policy"]["state"], "out_of_process_crash_containment_required")
        self.assertEqual(contract["blocked_action_count"], len(contract["blocked_actions"]))
        self.assertFalse(contract["native_load_enabled"])
        self.assertFalse(contract["native_load_performed"])
        self.assertFalse(contract["dll_load_performed"])
        self.assertFalse(contract["aex_file_opened"])

    def test_rejects_design_contract_that_accepts_aex_paths(self):
        unsafe = design_contract()
        unsafe["accepts_aex_path"] = True
        with self.assertRaises(ValueError) as ctx:
            build_contract(design=unsafe)
        self.assertIn("accepts_aex_path must be false", str(ctx.exception))

    def test_rejects_broker_selftest_with_path_payload(self):
        unsafe = broker_selftest()
        unsafe["path_payload_supplied"] = True
        unsafe["blocked_action_checks"][0]["path_payload_supplied"] = True
        with self.assertRaises(ValueError) as ctx:
            build_contract(broker=unsafe)
        self.assertIn("path_payload_supplied must be false", str(ctx.exception))

    def test_rejects_candidate_gate_after_fixture_approval(self):
        unsafe = candidate_load_gate()
        unsafe["fixture_approval_satisfied"] = True
        with self.assertRaises(ValueError) as ctx:
            build_contract(gate=unsafe)
        self.assertIn("fixture approval must not be satisfied", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "design": LAB_ROOT / "target" / "native-loader-design",
            "broker": LAB_ROOT / "target" / "native-loader-broker-selftest",
            "gate": LAB_ROOT / "target" / "candidate-load-gate",
            "policy": LAB_ROOT / "target" / "sandbox-policy",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        paths = {
            "design": roots["design"] / f"{time.time_ns()}-design.local.json",
            "broker": roots["broker"] / f"{time.time_ns()}-broker.local.json",
            "gate": roots["gate"] / f"{time.time_ns()}-gate.local.json",
            "policy": roots["policy"] / f"{time.time_ns()}-policy.local.json",
        }
        paths["design"].write_text(json.dumps(design_contract()), encoding="utf-8")
        paths["broker"].write_text(json.dumps(broker_selftest()), encoding="utf-8")
        paths["gate"].write_text(json.dumps(candidate_load_gate()), encoding="utf-8")
        paths["policy"].write_text(json.dumps(sandbox_policy()), encoding="utf-8")

        loaded_design, resolved_design = aex_native_loader_runtime_contract.load_native_loader_design(paths["design"])
        loaded_broker, resolved_broker = aex_native_loader_runtime_contract.load_broker_selftest(paths["broker"])
        loaded_gate, resolved_gate = aex_native_loader_runtime_contract.load_candidate_load_gate(paths["gate"])
        loaded_policy, resolved_policy = aex_native_loader_runtime_contract.load_sandbox_policy(paths["policy"])
        self.assertEqual(loaded_design["report_kind"], "aex_native_loader_design_contract")
        self.assertEqual(loaded_broker["report_kind"], "aex_native_loader_broker_selftest")

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-runtime-design.local.json"
        outside.write_text(json.dumps(design_contract()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_native_loader_runtime_contract.load_native_loader_design(outside)

        contract = aex_native_loader_runtime_contract.build_runtime_contract(
            native_loader_design=loaded_design,
            native_loader_design_path=resolved_design,
            broker_selftest=loaded_broker,
            broker_selftest_path=resolved_broker,
            candidate_load_gate=loaded_gate,
            candidate_load_gate_path=resolved_gate,
            sandbox_policy=loaded_policy,
            sandbox_policy_path=resolved_policy,
        )
        out = LAB_ROOT / "target" / "native-loader-runtime-contract" / f"{time.time_ns()}-runtime.local.json"
        written = aex_native_loader_runtime_contract.write_json_create_new(out, contract)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_native_loader_runtime_contract.write_json_create_new(out, contract)
        with self.assertRaises(ValueError):
            aex_native_loader_runtime_contract.write_json_create_new(
                LAB_ROOT / "target" / "outside-runtime-contract.json",
                contract,
            )


if __name__ == "__main__":
    unittest.main()
