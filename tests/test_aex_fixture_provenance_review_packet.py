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


aex_fixture_provenance_review_packet = load_tool("aex_fixture_provenance_review_packet")


def manual_review_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_manual_review_packet",
        "review_packet_state": "fixture_manual_review_packet_ready_no_load",
        "manual_review_ready": True,
        "approval_ready": False,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate": {
            "file_name": "ScatterMap.aex",
            "size_bytes": 123456,
        },
        "wiztree_aex_inventory": {
            "inventory_state": "wiztree_csv_read_metadata_only",
            "candidate_match_count": 1,
            "candidate_size_match": True,
        },
        "approval_blocker_count": 4,
        "approval_blockers": [{"id": "manual_fixture_review_pending"}],
        "manual_review_questions": ["Who owns this local fixture candidate?"],
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
    }


def approval_request_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_approval_request_packet",
        "approval_request_state": "fixture_approval_request_ready_pending_manual_approval",
        "approval_request_ready": True,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "approval_blocker_count": 4,
        "approval_blockers": [{"id": "explicit_user_approval_missing"}],
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
    }


class AexFixtureProvenanceReviewPacketTests(unittest.TestCase):
    def test_build_packet_keeps_fixture_review_no_load_and_no_approval(self):
        packet = aex_fixture_provenance_review_packet.build_fixture_provenance_review_packet(
            fixture_manual_review=manual_review_payload(),
            fixture_manual_review_path=Path("target/fixture-manual-review/manual.json"),
            approval_request=approval_request_payload(),
            approval_request_path=Path("target/fixture-approval-request/request.json"),
        )

        self.assertEqual(packet["report_kind"], "aex_fixture_provenance_review_packet")
        self.assertEqual(packet["provenance_review_state"], "fixture_provenance_review_packet_ready_no_load")
        self.assertTrue(packet["provenance_review_ready"])
        self.assertTrue(packet["manual_review_source_ready"])
        self.assertTrue(packet["approval_request_source_ready"])
        self.assertEqual(packet["candidate_relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertEqual(packet["provenance_status"], "unknown_requires_user_review")
        self.assertEqual(packet["license_status"], "unknown_requires_user_review")
        self.assertEqual(packet["native_load_gate"], "closed")
        self.assertFalse(packet["approval_can_be_issued_now"])
        self.assertFalse(packet["approval_manifest_created"])
        self.assertFalse(packet["current_fixture_approval_valid"])
        self.assertFalse(packet["fixture_approval_satisfied"])
        self.assertTrue(packet["approval_gate_stays_closed"])
        self.assertFalse(packet["native_load_enabled"])
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["dll_load_performed"])
        self.assertFalse(packet["render_performed"])
        self.assertFalse(packet["ae_invoked"])
        self.assertFalse(packet["ofx_route_invoked"])
        self.assertFalse(packet["private_payload_copied"])
        self.assertFalse(packet["aex_file_opened"])
        self.assertFalse(packet["aex_file_hashed"])
        self.assertFalse(packet["aex_file_copied"])
        self.assertIsNone(packet["accepted_aex_path"])
        self.assertFalse(packet["raw_input_paths_serialized"])
        self.assertIn("open_aex_file", packet["blocked_actions"])
        self.assertIn("hash_aex_file", packet["blocked_actions"])
        self.assertIn("copy_selected_aex_fixture", packet["blocked_actions"])
        self.assertIn("load_dependency_dll", packet["blocked_actions"])
        self.assertIn("dispatch_PF_Cmd", packet["blocked_actions"])
        self.assertIn("provenance_not_confirmed", packet["blockers"])

    def test_rejects_source_that_can_issue_approval_manifest(self):
        approval_request = approval_request_payload()
        approval_request["approval_can_be_issued_now"] = True
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_review_packet.build_fixture_provenance_review_packet(
                fixture_manual_review=manual_review_payload(),
                fixture_manual_review_path=Path("target/fixture-manual-review/manual.json"),
                approval_request=approval_request,
                approval_request_path=Path("target/fixture-approval-request/request.json"),
            )
        self.assertIn("approval_can_be_issued_now must be false", str(context.exception))

    def test_rejects_candidate_mismatch(self):
        approval_request = approval_request_payload()
        approval_request["candidate_relative_path"] = "Other\\Different.aex"
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_review_packet.build_fixture_provenance_review_packet(
                fixture_manual_review=manual_review_payload(),
                fixture_manual_review_path=Path("target/fixture-manual-review/manual.json"),
                approval_request=approval_request,
                approval_request_path=Path("target/fixture-approval-request/request.json"),
            )
        self.assertIn("candidate_relative_path must match", str(context.exception))

    def test_rejects_approval_like_source_keys(self):
        manual_review = manual_review_payload()
        manual_review["manifest_kind"] = "aex_fixture_decision_manifest"
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_review_packet.build_fixture_provenance_review_packet(
                fixture_manual_review=manual_review,
                fixture_manual_review_path=Path("target/fixture-manual-review/manual.json"),
                approval_request=approval_request_payload(),
                approval_request_path=Path("target/fixture-approval-request/request.json"),
            )
        self.assertIn("must not contain manifest_kind", str(context.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        manual_root = LAB_ROOT / "target" / "fixture-manual-review"
        request_root = LAB_ROOT / "target" / "fixture-approval-request"
        manual_root.mkdir(parents=True, exist_ok=True)
        request_root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        manual_path = manual_root / f"{stamp}-provenance-manual.local.json"
        request_path = request_root / f"{stamp}-provenance-request.local.json"
        manual_path.write_text(json.dumps(manual_review_payload()), encoding="utf-8")
        request_path.write_text(json.dumps(approval_request_payload()), encoding="utf-8")

        loaded_manual, resolved_manual = aex_fixture_provenance_review_packet.load_fixture_manual_review(manual_path)
        loaded_request, resolved_request = aex_fixture_provenance_review_packet.load_approval_request(request_path)
        self.assertEqual(loaded_manual["report_kind"], "aex_fixture_manual_review_packet")
        self.assertEqual(loaded_request["report_kind"], "aex_fixture_approval_request_packet")
        self.assertEqual(resolved_manual, manual_path.resolve())
        self.assertEqual(resolved_request, request_path.resolve())

        packet = aex_fixture_provenance_review_packet.build_fixture_provenance_review_packet(
            fixture_manual_review=loaded_manual,
            fixture_manual_review_path=resolved_manual,
            approval_request=loaded_request,
            approval_request_path=resolved_request,
        )
        out = LAB_ROOT / "target" / "fixture-provenance-review" / f"{stamp}-provenance.local.json"
        written = aex_fixture_provenance_review_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_provenance_review_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_fixture_provenance_review_packet.write_json_create_new(
                LAB_ROOT / "target" / "outside-provenance.local.json",
                packet,
            )


if __name__ == "__main__":
    unittest.main()
