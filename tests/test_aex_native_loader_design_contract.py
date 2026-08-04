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


aex_native_loader_design_contract = load_tool("aex_native_loader_design_contract")


def candidate_path() -> str:
    return "AEPluginBuild\\ScatterMap.aex"


def make_worker_design() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_worker_sandbox_design_packet",
        "design_state": "no_load_worker_boundary_only",
        "primary_review_candidate": {
            "relative_path": candidate_path(),
            "compatibility_class": "classic_pf_effect_candidate",
            "effect_main_export_present": True,
            "aegp_marker_count": 0,
            "approval_state": "not_approved_for_load",
        },
        "blocked_actions": ["load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_sandbox_policy() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_sandbox_policy_packet",
        "sandbox_policy_state": "policy_ready_no_native_load",
        "required_before_native_load": [
            "explicit user fixture approval artifact",
            "passing load gate using the approved fixture",
            "local dependency availability check in a separate no-load preflight",
            "worker process isolation design review",
            "crash/timeout containment plan",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_candidate_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_load_gate_dryrun",
        "candidate_load_gate_dryrun_state": "candidate_load_gate_dryrun_ready_no_load",
        "candidate_scoped_load_gate_dry_run_state": (
            "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
        ),
        "native_load_gate": "closed",
        "fixture_approval_satisfied": False,
        "fixture_decision_manifest_kind": "aex_fixture_decision_manifest",
        "fixture_decision_state": "hold_for_manual_review",
        "fixture_approval_state": "not_approved_for_load_gate",
        "candidate_dependencies_clear": True,
        "candidate_dependency_blockers_present": False,
        "candidate_relative_path": candidate_path(),
        "source_load_gate_state": "closed_dependency_review_or_invalid_approval",
        "source_load_gate_dependency_recommendation": "do_not_open_native_load_gate",
        "global_dependency_blockers_present": True,
        "global_dependency_blockers_apply_to_candidate": False,
        "scoped_gate_recommendation": "candidate_dependencies_clear_global_gate_still_closed",
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_loader_stub() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_stub_report",
        "stub_state": "refused_gate_closed",
        "loader_action": "no_op",
        "accepted_aex_path": None,
        "primary_review_candidate": {"relative_path": candidate_path()},
        "blocked_actions": ["accept_aex_path", "open_aex_file", "load_aex_dll", "call_EffectMain"],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_render_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_render_validation_contract",
        "contract_state": "render_validation_contract_ready_render_closed",
        "real_render_open": False,
        "no_load_validation_ready": True,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_ofx_route_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def build_contract(**overrides):
    inputs = {
        "worker_design": make_worker_design(),
        "worker_design_path": Path("worker-design.json"),
        "sandbox_policy": make_sandbox_policy(),
        "sandbox_policy_path": Path("sandbox-policy.json"),
        "candidate_load_gate": make_candidate_gate(),
        "candidate_load_gate_path": Path("candidate-gate.json"),
        "native_loader_stub": make_loader_stub(),
        "native_loader_stub_path": Path("loader-stub.json"),
        "render_validation_contract": make_render_contract(),
        "render_validation_contract_path": Path("render-contract.json"),
        "ofx_route_contract": make_ofx_route_contract(),
        "ofx_route_contract_path": Path("ofx-contract.json"),
    }
    inputs.update(overrides)
    return aex_native_loader_design_contract.build_native_loader_design_contract(**inputs)


class AexNativeLoaderDesignContractTests(unittest.TestCase):
    def test_design_contract_is_ready_but_keeps_loader_closed(self):
        contract = build_contract()

        self.assertEqual(contract["report_kind"], "aex_native_loader_design_contract")
        self.assertEqual(contract["native_loader_design_state"], "native_loader_design_ready_loader_closed")
        self.assertEqual(
            contract["contract_state"],
            "native_loader_design_contract_ready_loader_closed_pending_fixture_approval",
        )
        self.assertTrue(contract["loader_design_ready"])
        self.assertEqual(contract["candidate_relative_path"], candidate_path())
        self.assertTrue(contract["candidate_dependencies_clear"])
        self.assertFalse(contract["fixture_approval_satisfied"])
        self.assertEqual(contract["native_load_gate"], "closed")
        self.assertFalse(contract["accepts_aex_path"])
        self.assertIsNone(contract["accepted_aex_path"])
        self.assertFalse(contract["controller_loads_aex"])
        self.assertEqual(contract["approval_contract"]["approval_token_name"], "APPROVE_AEX_LOAD_GATE")
        self.assertTrue(contract["approval_contract"]["approval_does_not_permit_native_load"])
        self.assertEqual(
            contract["dependency_gate_contract"]["required_dependency_review_endpoint"],
            "manual_loader_design_review_only_no_auto_approval",
        )
        self.assertFalse(contract["native_load_enabled"])
        self.assertFalse(contract["native_load_performed"])
        self.assertFalse(contract["dll_load_performed"])
        self.assertFalse(contract["aex_file_opened"])
        self.assertIn("accept_aex_path", contract["blocked_actions"])
        self.assertTrue(contract["loader_contract"]["separate_process_required"])
        self.assertFalse(contract["loader_contract"]["accepts_aex_path"])

    def test_rejects_unsafe_or_mismatched_evidence(self):
        candidate_gate = make_candidate_gate()
        candidate_gate["fixture_approval_satisfied"] = True
        with self.assertRaises(ValueError):
            build_contract(candidate_load_gate=candidate_gate)

        stub = make_loader_stub()
        stub["accepted_aex_path"] = candidate_path()
        with self.assertRaises(ValueError):
            build_contract(native_loader_stub=stub)

        worker = make_worker_design()
        worker["primary_review_candidate"]["relative_path"] = "Other.aex"
        with self.assertRaises(ValueError):
            build_contract(worker_design=worker)

        render_contract = make_render_contract()
        render_contract["real_render_open"] = True
        with self.assertRaises(ValueError):
            build_contract(render_validation_contract=render_contract)

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "worker": LAB_ROOT / "target" / "worker-design",
            "sandbox": LAB_ROOT / "target" / "sandbox-policy",
            "candidate_gate": LAB_ROOT / "target" / "candidate-load-gate",
            "stub": LAB_ROOT / "target" / "native-loader-stub",
            "render": LAB_ROOT / "target" / "render-validation-contract",
            "ofx": LAB_ROOT / "target" / "ofx-route-contract",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        paths = {
            "worker": roots["worker"] / f"{stamp}-worker-design.local.json",
            "sandbox": roots["sandbox"] / f"{stamp}-sandbox-policy.local.json",
            "candidate_gate": roots["candidate_gate"] / f"{stamp}-candidate-gate.local.json",
            "stub": roots["stub"] / f"{stamp}-loader-stub.local.json",
            "render": roots["render"] / f"{stamp}-render-contract.local.json",
            "ofx": roots["ofx"] / f"{stamp}-ofx-contract.local.json",
        }
        payloads = {
            "worker": make_worker_design(),
            "sandbox": make_sandbox_policy(),
            "candidate_gate": make_candidate_gate(),
            "stub": make_loader_stub(),
            "render": make_render_contract(),
            "ofx": make_ofx_route_contract(),
        }
        for key, path in paths.items():
            path.write_text(json.dumps(payloads[key]), encoding="utf-8")

        worker, worker_path = aex_native_loader_design_contract.load_worker_design(paths["worker"])
        sandbox, sandbox_path = aex_native_loader_design_contract.load_sandbox_policy(paths["sandbox"])
        candidate_gate, candidate_gate_path = aex_native_loader_design_contract.load_candidate_load_gate(
            paths["candidate_gate"]
        )
        stub, stub_path = aex_native_loader_design_contract.load_native_loader_stub(paths["stub"])
        render, render_path = aex_native_loader_design_contract.load_render_validation_contract(paths["render"])
        ofx, ofx_path = aex_native_loader_design_contract.load_ofx_route_contract(paths["ofx"])
        self.assertEqual(worker_path, paths["worker"].resolve())
        self.assertEqual(sandbox_path, paths["sandbox"].resolve())
        self.assertEqual(candidate_gate_path, paths["candidate_gate"].resolve())
        self.assertEqual(stub_path, paths["stub"].resolve())
        self.assertEqual(render_path, paths["render"].resolve())
        self.assertEqual(ofx_path, paths["ofx"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-loader-stub.json"
        outside.write_text(json.dumps(make_loader_stub()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_native_loader_design_contract.load_native_loader_stub(outside)

        contract = aex_native_loader_design_contract.build_native_loader_design_contract(
            worker_design=worker,
            worker_design_path=worker_path,
            sandbox_policy=sandbox,
            sandbox_policy_path=sandbox_path,
            candidate_load_gate=candidate_gate,
            candidate_load_gate_path=candidate_gate_path,
            native_loader_stub=stub,
            native_loader_stub_path=stub_path,
            render_validation_contract=render,
            render_validation_contract_path=render_path,
            ofx_route_contract=ofx,
            ofx_route_contract_path=ofx_path,
        )
        out = LAB_ROOT / "target" / "native-loader-design" / f"{time.time_ns()}-{os.getpid()}-loader-design.local.json"
        written = aex_native_loader_design_contract.write_json_create_new(out, contract)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_native_loader_design_contract.write_json_create_new(out, contract)
        with self.assertRaises(ValueError):
            aex_native_loader_design_contract.write_json_create_new(
                LAB_ROOT / "target" / "outside-loader-design.json",
                contract,
            )


if __name__ == "__main__":
    unittest.main()
