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


aex_fixture_approval_verifier = load_tool("aex_fixture_approval_verifier")


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
CANDIDATE = r"AEPluginBuild\ScatterMap.aex"


def fixture_decision_hold() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_decision_manifest",
        "decision_state": "hold_for_manual_review",
        "approval_state": "not_approved_for_load_gate",
        "explicit_user_approval": False,
        "candidate_relative_path": CANDIDATE,
        "approved_actions": [],
        **SAFETY_FALSE,
    }


def fixture_approval() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_approval_manifest",
        "approval_state": "user_approved_for_load_gate",
        "explicit_user_approval": True,
        "candidate_relative_path": CANDIDATE,
        "approved_actions": ["prepare_native_load_gate"],
        **SAFETY_FALSE,
    }


def manual_review(approval_ready: bool = False) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_manual_review_packet",
        "review_packet_state": "fixture_manual_review_packet_ready_no_load",
        "manual_review_ready": True,
        "approval_ready": approval_ready,
        "approval_blocker_count": 0 if approval_ready else 4,
        "candidate_relative_path": CANDIDATE,
        **SAFETY_FALSE,
    }


def dependency_scope() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_dependency_scope_packet",
        "candidate_dependency_scope_state": "candidate_dependency_scope_ready_no_load",
        "candidate_scope_ready": True,
        "candidate_relative_path": CANDIDATE,
        "candidate_dependency_blockers_present": False,
        "candidate_dependency_blocker_count": 0,
        "candidate_dependency_missing_or_api_set_review_count": 0,
        "candidate_dependency_found_paths_exported": False,
        "global_dependency_blockers_apply_to_candidate": False,
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
        "raw_input_paths_serialized": False,
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
        "candidate_relative_path": CANDIDATE,
        "native_load_gate": "closed",
        "fixture_approval_satisfied": False,
        **SAFETY_FALSE,
    }


def build_report(
    decision: dict | None = None,
    review: dict | None = None,
    scope: dict | None = None,
    path_policy: dict | None = None,
    load_gate: dict | None = None,
) -> dict:
    return aex_fixture_approval_verifier.build_fixture_approval_verifier(
        fixture_decision=decision or fixture_decision_hold(),
        fixture_decision_path=LAB_ROOT / "target" / "fixture-approval" / "decision.local.json",
        fixture_manual_review=review or manual_review(),
        fixture_manual_review_path=LAB_ROOT / "target" / "fixture-manual-review" / "manual.local.json",
        candidate_dependency_scope=scope or dependency_scope(),
        candidate_dependency_scope_path=LAB_ROOT / "target" / "candidate-dependency-scope" / "scope.local.json",
        path_policy_selftest=path_policy or path_policy_selftest(),
        path_policy_selftest_path=LAB_ROOT / "target" / "native-loader-path-policy-selftest" / "path.local.json",
        candidate_load_gate=load_gate or candidate_load_gate(),
        candidate_load_gate_path=LAB_ROOT / "target" / "candidate-load-gate" / "gate.local.json",
    )


class AexFixtureApprovalVerifierTests(unittest.TestCase):
    def test_builds_verifier_for_current_hold_without_approval(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_fixture_approval_verifier")
        self.assertEqual(report["approval_verifier_state"], "fixture_approval_verifier_ready_no_approval")
        self.assertTrue(report["approval_verifier_ready"])
        self.assertFalse(report["current_fixture_approval_valid"])
        self.assertFalse(report["fixture_approval_satisfied"])
        self.assertTrue(report["approval_gate_stays_closed"])
        self.assertTrue(report["manual_review_ready"])
        self.assertFalse(report["manual_review_approval_ready"])
        self.assertTrue(report["candidate_dependencies_clear"])
        self.assertTrue(report["path_policy_closed"])
        self.assertTrue(report["candidate_load_gate_closed"])
        self.assertTrue(report["synthetic_approval_checks_passed"])
        self.assertEqual(report["required_approval_token_name"], "APPROVE_AEX_LOAD_GATE")
        self.assertTrue(report["approval_token_not_stored_in_manifest"])
        self.assertTrue(report["approval_only_prepares_next_gate"])
        self.assertIn("manifest_kind_not_approval", report["current_approval_evaluation"]["reasons"])
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["aex_file_opened"])

    def test_evaluates_future_valid_approval_shape_when_manual_review_ready(self):
        evaluation = aex_fixture_approval_verifier.evaluate_approval_manifest(
            fixture_approval(),
            candidate_relative_path=CANDIDATE,
            manual_review_approval_ready=True,
            dependencies_clear=True,
            path_policy_closed=True,
        )
        self.assertTrue(evaluation["valid"])
        self.assertTrue(evaluation["approval_only_prepares_next_gate"])
        self.assertEqual(evaluation["forbidden_approved_actions"], [])

    def test_rejects_approval_with_forbidden_runtime_action(self):
        unsafe = fixture_approval()
        unsafe["approved_actions"] = ["prepare_native_load_gate", "load_aex_dll"]
        evaluation = aex_fixture_approval_verifier.evaluate_approval_manifest(
            unsafe,
            candidate_relative_path=CANDIDATE,
            manual_review_approval_ready=True,
            dependencies_clear=True,
            path_policy_closed=True,
        )
        self.assertFalse(evaluation["valid"])
        self.assertIn("forbidden_runtime_action_approved", evaluation["reasons"])
        self.assertEqual(evaluation["forbidden_approved_actions"], ["load_aex_dll"])

    def test_rejects_approval_when_manual_review_not_ready(self):
        evaluation = aex_fixture_approval_verifier.evaluate_approval_manifest(
            fixture_approval(),
            candidate_relative_path=CANDIDATE,
            manual_review_approval_ready=False,
            dependencies_clear=True,
            path_policy_closed=True,
        )
        self.assertFalse(evaluation["valid"])
        self.assertIn("manual_review_not_approval_ready", evaluation["reasons"])

    def test_rejects_mismatched_candidate_scope(self):
        unsafe_scope = dependency_scope()
        unsafe_scope["candidate_relative_path"] = r"Other\Candidate.aex"
        with self.assertRaises(ValueError) as ctx:
            build_report(scope=unsafe_scope)
        self.assertIn("candidate dependency scope candidate_relative_path must match fixture decision", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "decision": LAB_ROOT / "target" / "fixture-approval",
            "manual": LAB_ROOT / "target" / "fixture-manual-review",
            "scope": LAB_ROOT / "target" / "candidate-dependency-scope",
            "path": LAB_ROOT / "target" / "native-loader-path-policy-selftest",
            "gate": LAB_ROOT / "target" / "candidate-load-gate",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        paths = {
            "decision": roots["decision"] / f"{time.time_ns()}-{os.getpid()}-decision.local.json",
            "manual": roots["manual"] / f"{time.time_ns()}-{os.getpid()}-manual.local.json",
            "scope": roots["scope"] / f"{time.time_ns()}-{os.getpid()}-scope.local.json",
            "path": roots["path"] / f"{time.time_ns()}-{os.getpid()}-path.local.json",
            "gate": roots["gate"] / f"{time.time_ns()}-{os.getpid()}-gate.local.json",
        }
        paths["decision"].write_text(json.dumps(fixture_decision_hold()), encoding="utf-8")
        paths["manual"].write_text(json.dumps(manual_review()), encoding="utf-8")
        paths["scope"].write_text(json.dumps(dependency_scope()), encoding="utf-8")
        paths["path"].write_text(json.dumps(path_policy_selftest()), encoding="utf-8")
        paths["gate"].write_text(json.dumps(candidate_load_gate()), encoding="utf-8")

        decision, decision_path = aex_fixture_approval_verifier.load_fixture_decision(paths["decision"])
        manual, manual_path = aex_fixture_approval_verifier.load_fixture_manual_review(paths["manual"])
        scope, scope_path = aex_fixture_approval_verifier.load_candidate_dependency_scope(paths["scope"])
        path_policy, path_policy_path = aex_fixture_approval_verifier.load_path_policy_selftest(paths["path"])
        gate, gate_path = aex_fixture_approval_verifier.load_candidate_load_gate(paths["gate"])

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-decision.local.json"
        outside.write_text(json.dumps(fixture_decision_hold()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_fixture_approval_verifier.load_fixture_decision(outside)

        report = aex_fixture_approval_verifier.build_fixture_approval_verifier(
            fixture_decision=decision,
            fixture_decision_path=decision_path,
            fixture_manual_review=manual,
            fixture_manual_review_path=manual_path,
            candidate_dependency_scope=scope,
            candidate_dependency_scope_path=scope_path,
            path_policy_selftest=path_policy,
            path_policy_selftest_path=path_policy_path,
            candidate_load_gate=gate,
            candidate_load_gate_path=gate_path,
        )
        out = LAB_ROOT / "target" / "fixture-approval-verifier" / f"{time.time_ns()}-{os.getpid()}-approval-verifier.local.json"
        written = aex_fixture_approval_verifier.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_approval_verifier.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_fixture_approval_verifier.write_json_create_new(
                LAB_ROOT / "target" / "outside-approval-verifier.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
