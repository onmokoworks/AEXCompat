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


aex_fixture_approval_request_packet = load_tool("aex_fixture_approval_request_packet")


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


def approval_verifier() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_approval_verifier",
        "approval_verifier_state": "fixture_approval_verifier_ready_no_approval",
        "approval_verifier_ready": True,
        "candidate_relative_path": CANDIDATE,
        "approval_manifest_kind": "aex_fixture_decision_manifest",
        "decision_state": "hold_for_manual_review",
        "approval_state": "not_approved_for_load_gate",
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "manual_review_ready": True,
        "manual_review_approval_ready": False,
        "approval_blocker_count": 4,
        "candidate_dependencies_clear": True,
        "candidate_dependency_blockers_present": False,
        "path_policy_closed": True,
        "candidate_load_gate_closed": True,
        "source_candidate_load_gate_state": "candidate_load_gate_dryrun_ready_no_load",
        "current_approval_evaluation": {
            "valid": False,
            "reasons": ["manifest_kind_not_approval", "explicit_user_approval_missing"],
        },
        "synthetic_approval_checks_passed": True,
        "synthetic_approval_checks": [
            {"case": "current_hold_manifest_rejected", "valid": False, "approval_only_prepares_next_gate": False},
            {"case": "missing_explicit_user_approval_rejected", "valid": False, "approval_only_prepares_next_gate": True},
            {"case": "forbidden_runtime_action_rejected", "valid": False, "approval_only_prepares_next_gate": False},
            {"case": "manual_review_not_ready_rejected", "valid": False, "approval_only_prepares_next_gate": True},
            {"case": "future_valid_shape_only_prepares_next_gate", "valid": True, "approval_only_prepares_next_gate": True},
        ],
        "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "blocked_actions": ["load_aex_dll", "render_with_aex", "route_through_ofx"],
        **SAFETY_FALSE,
    }


def manual_review() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_manual_review_packet",
        "review_packet_state": "fixture_manual_review_packet_ready_no_load",
        "manual_review_ready": True,
        "approval_ready": False,
        "candidate_relative_path": CANDIDATE,
        "approval_blocker_count": 4,
        "approval_blockers": [
            {
                "id": "manual_fixture_review_pending",
                "severity": "blocker",
                "next_action": "Complete provenance/license/safety review.",
            }
        ],
        "recommended_next_decision": "keep_hold_pending_manual_review",
        **SAFETY_FALSE,
    }


def build_packet(verifier: dict | None = None, review: dict | None = None) -> dict:
    return aex_fixture_approval_request_packet.build_fixture_approval_request_packet(
        approval_verifier=verifier or approval_verifier(),
        approval_verifier_path=LAB_ROOT
        / "target"
        / "fixture-approval-verifier"
        / "approval-verifier.local.json",
        fixture_manual_review=review or manual_review(),
        fixture_manual_review_path=LAB_ROOT
        / "target"
        / "fixture-manual-review"
        / "manual.local.json",
    )


class AexFixtureApprovalRequestPacketTests(unittest.TestCase):
    def test_builds_request_packet_without_creating_approval(self):
        packet = build_packet()
        self.assertEqual(packet["report_kind"], "aex_fixture_approval_request_packet")
        self.assertEqual(packet["approval_request_state"], "fixture_approval_request_ready_pending_manual_approval")
        self.assertTrue(packet["approval_request_ready"])
        self.assertFalse(packet["approval_can_be_issued_now"])
        self.assertFalse(packet["approval_manifest_created"])
        self.assertTrue(packet["requires_explicit_user_approval"])
        self.assertEqual(packet["required_approval_token_name"], "APPROVE_AEX_LOAD_GATE")
        self.assertFalse(packet["current_fixture_approval_valid"])
        self.assertFalse(packet["fixture_approval_satisfied"])
        self.assertFalse(packet["manual_review_approval_ready"])
        self.assertTrue(packet["approval_gate_stays_closed"])
        self.assertEqual(packet["native_load_gate"], "closed")
        self.assertEqual(packet["approval_blocker_count"], 4)
        self.assertEqual(len(packet["approval_blockers"]), 1)
        self.assertIn("explicit_user_approval", {item["id"] for item in packet["review_checklist"]})
        self.assertTrue(packet["synthetic_approval_checks_summary"]["future_valid_shape_only_prepares_next_gate"])
        self.assertIn("load_aex_dll", packet["forbidden_actions_after_approval"])
        self.assertFalse(packet["native_load_enabled"])
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["dll_load_performed"])
        self.assertFalse(packet["aex_file_opened"])

    def test_rejects_verifier_that_already_treats_approval_as_valid(self):
        verifier = approval_verifier()
        verifier["current_fixture_approval_valid"] = True
        with self.assertRaises(ValueError) as ctx:
            build_packet(verifier=verifier)
        self.assertIn("current fixture approval must be invalid", str(ctx.exception))

    def test_rejects_verifier_with_open_gate(self):
        verifier = approval_verifier()
        verifier["approval_gate_stays_closed"] = False
        with self.assertRaises(ValueError) as ctx:
            build_packet(verifier=verifier)
        self.assertIn("approval gate must stay closed", str(ctx.exception))

    def test_rejects_manual_review_candidate_mismatch(self):
        review = manual_review()
        review["candidate_relative_path"] = r"Other\Candidate.aex"
        with self.assertRaises(ValueError) as ctx:
            build_packet(review=review)
        self.assertIn("candidate_relative_path must match verifier", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        verifier_root = LAB_ROOT / "target" / "fixture-approval-verifier"
        manual_root = LAB_ROOT / "target" / "fixture-manual-review"
        verifier_root.mkdir(parents=True, exist_ok=True)
        manual_root.mkdir(parents=True, exist_ok=True)
        verifier_path = verifier_root / f"{time.time_ns()}-{os.getpid()}-approval-verifier.local.json"
        manual_path = manual_root / f"{time.time_ns()}-{os.getpid()}-manual.local.json"
        verifier_path.write_text(json.dumps(approval_verifier()), encoding="utf-8")
        manual_path.write_text(json.dumps(manual_review()), encoding="utf-8")

        verifier, resolved_verifier = aex_fixture_approval_request_packet.load_approval_verifier(verifier_path)
        review, resolved_review = aex_fixture_approval_request_packet.load_fixture_manual_review(manual_path)
        self.assertEqual(resolved_verifier, verifier_path.resolve())
        self.assertEqual(resolved_review, manual_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-approval-verifier.local.json"
        outside.write_text(json.dumps(approval_verifier()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_fixture_approval_request_packet.load_approval_verifier(outside)

        packet = aex_fixture_approval_request_packet.build_fixture_approval_request_packet(
            approval_verifier=verifier,
            approval_verifier_path=resolved_verifier,
            fixture_manual_review=review,
            fixture_manual_review_path=resolved_review,
        )
        out = LAB_ROOT / "target" / "fixture-approval-request" / f"{time.time_ns()}-{os.getpid()}-request.local.json"
        written = aex_fixture_approval_request_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_approval_request_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_fixture_approval_request_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-approval-request.json",
                packet,
            )


if __name__ == "__main__":
    unittest.main()
