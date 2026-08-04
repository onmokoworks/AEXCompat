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


aex_fixture_provenance_answer_template = load_tool("aex_fixture_provenance_answer_template")


def provenance_review_payload() -> dict:
    questions = [
        {
            "question_id": "provenance_confirmed",
            "subject": "Candidate provenance is known and acceptable for local fixture use",
            "status": "requires_user_review",
            "source": "manual_review",
            "required_evidence": "User-confirmed source/provenance note for the selected AEX.",
            "approval_effect": "does_not_approve_native_load",
        },
        {
            "question_id": "license_scope_confirmed",
            "subject": "License scope is acceptable for local-only testing",
            "status": "requires_user_review",
            "source": "manual_review",
            "required_evidence": "User-confirmed license scope.",
            "approval_effect": "does_not_approve_native_load",
        },
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_review_packet",
        "provenance_review_state": "fixture_provenance_review_packet_ready_no_load",
        "provenance_review_ready": True,
        "manual_review_source_ready": True,
        "approval_request_source_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "provenance_status": "unknown_requires_user_review",
        "license_status": "unknown_requires_user_review",
        "local_fixture_safety_status": "no_load_evidence_ready_pending_manual_review",
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "requires_explicit_user_approval": True,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "native_load_gate_stays_closed": True,
        "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
        "approval_token_not_stored_in_manifest": True,
        "approval_only_prepares_next_gate": True,
        "review_question_count": len(questions),
        "unanswered_review_question_count": len(questions),
        "review_questions": questions,
        "blocked_actions": [
            "create_approval_manifest",
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
            "route_through_ofx",
        ],
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
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
    }


class AexFixtureProvenanceAnswerTemplateTests(unittest.TestCase):
    def test_build_template_keeps_all_answers_pending_and_no_approval(self):
        payload = provenance_review_payload()
        template = aex_fixture_provenance_answer_template.build_fixture_provenance_answer_template(
            provenance_review=payload,
            provenance_review_path=Path("target/fixture-provenance-review/provenance.json"),
        )

        self.assertEqual(template["report_kind"], "aex_fixture_provenance_answer_template")
        self.assertEqual(
            template["template_state"],
            "fixture_provenance_answer_template_ready_all_answers_pending_no_load",
        )
        self.assertTrue(template["template_ready"])
        self.assertTrue(template["answer_template_only"])
        self.assertTrue(template["source_provenance_review_ready"])
        self.assertEqual(template["provenance_status"], "unknown_requires_user_review")
        self.assertEqual(template["license_status"], "unknown_requires_user_review")
        self.assertFalse(template["answers_present"])
        self.assertTrue(template["all_answers_pending"])
        self.assertEqual(template["answer_template_entry_count"], 2)
        self.assertEqual(template["pending_answer_count"], 2)
        self.assertEqual(template["answered_question_count"], 0)
        self.assertTrue(template["user_answer_artifact_required"])
        self.assertFalse(template["answer_template_approves_fixture"])
        self.assertFalse(template["answer_template_approves_publication"])
        self.assertFalse(template["answer_template_approves_native_load"])
        self.assertFalse(template["approval_can_be_issued_now"])
        self.assertFalse(template["approval_manifest_created"])
        self.assertFalse(template["current_fixture_approval_valid"])
        self.assertFalse(template["fixture_approval_satisfied"])
        self.assertEqual(template["native_load_gate"], "closed")
        self.assertIsNone(template["accepted_aex_path"])
        self.assertFalse(template["raw_input_paths_serialized"])
        self.assertFalse(template["aex_file_opened"])
        self.assertFalse(template["aex_file_hashed"])
        self.assertFalse(template["aex_file_copied"])
        for entry in template["answer_template_entries"]:
            self.assertEqual(entry["answer_status"], "pending_user_answer")
            self.assertFalse(entry["answer_text_present"])
            self.assertFalse(entry["answer_evidence_present"])
            self.assertTrue(entry["answer_does_not_approve_fixture"])
            self.assertTrue(entry["answer_does_not_enable_native_load"])
        self.assertIn("approval_token", template["answer_schema"]["forbidden_fields"])
        self.assertIn("raw_payload", template["answer_schema"]["forbidden_fields"])

    def test_rejects_source_with_approval_like_key(self):
        payload = provenance_review_payload()
        payload["approved_actions"] = ["prepare_native_load_gate"]
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_answer_template.build_fixture_provenance_answer_template(
                provenance_review=payload,
                provenance_review_path=Path("target/fixture-provenance-review/provenance.json"),
            )
        self.assertIn("must not contain approved_actions", str(context.exception))

    def test_rejects_source_with_open_native_gate(self):
        payload = provenance_review_payload()
        payload["native_load_gate"] = "open"
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_answer_template.build_fixture_provenance_answer_template(
                provenance_review=payload,
                provenance_review_path=Path("target/fixture-provenance-review/provenance.json"),
            )
        self.assertIn("native_load_gate must be closed", str(context.exception))

    def test_rejects_answered_source_questions(self):
        payload = provenance_review_payload()
        payload["unanswered_review_question_count"] = 1
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_answer_template.build_fixture_provenance_answer_template(
                provenance_review=payload,
                provenance_review_path=Path("target/fixture-provenance-review/provenance.json"),
            )
        self.assertIn("questions must all remain unanswered", str(context.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        root = LAB_ROOT / "target" / "fixture-provenance-review"
        root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        source = root / f"{stamp}-provenance.local.json"
        source.write_text(json.dumps(provenance_review_payload()), encoding="utf-8")

        loaded, resolved = aex_fixture_provenance_answer_template.load_provenance_review(source)
        self.assertEqual(loaded["report_kind"], "aex_fixture_provenance_review_packet")
        self.assertEqual(resolved, source.resolve())

        template = aex_fixture_provenance_answer_template.build_fixture_provenance_answer_template(
            provenance_review=loaded,
            provenance_review_path=resolved,
        )
        out = LAB_ROOT / "target" / "fixture-provenance-answer-template" / f"{stamp}-answer-template.local.json"
        written = aex_fixture_provenance_answer_template.write_json_create_new(out, template)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_provenance_answer_template.write_json_create_new(out, template)
        with self.assertRaises(ValueError):
            aex_fixture_provenance_answer_template.write_json_create_new(
                LAB_ROOT / "target" / "outside-answer-template.local.json",
                template,
            )


if __name__ == "__main__":
    unittest.main()
