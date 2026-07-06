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


aex_fixture_provenance_answer_intake = load_tool("aex_fixture_provenance_answer_intake")


def answer_template_payload() -> dict:
    entries = [
        {
            "question_id": "provenance_confirmed",
            "subject": "Candidate provenance is known and acceptable for local fixture use",
            "source": "manual_review",
            "required_evidence": "User-confirmed source/provenance note for the selected AEX.",
            "approval_effect": "does_not_approve_native_load",
            "answer_status": "pending_user_answer",
            "allowed_answer_statuses": [
                "confirmed_local_only",
                "not_confirmed_keep_hold",
                "reject_fixture",
                "needs_more_information",
            ],
            "answer_text_present": False,
            "answer_evidence_present": False,
            "answer_must_not_include_approval_token": True,
            "answer_does_not_approve_fixture": True,
            "answer_does_not_enable_native_load": True,
        },
        {
            "question_id": "license_scope_confirmed",
            "subject": "License scope is acceptable for local-only testing",
            "source": "manual_review",
            "required_evidence": "User-confirmed license scope.",
            "approval_effect": "does_not_approve_native_load",
            "answer_status": "pending_user_answer",
            "allowed_answer_statuses": [
                "confirmed_local_only",
                "not_confirmed_keep_hold",
                "reject_fixture",
                "needs_more_information",
            ],
            "answer_text_present": False,
            "answer_evidence_present": False,
            "answer_must_not_include_approval_token": True,
            "answer_does_not_approve_fixture": True,
            "answer_does_not_enable_native_load": True,
        },
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_answer_template",
        "template_state": "fixture_provenance_answer_template_ready_all_answers_pending_no_load",
        "template_ready": True,
        "answer_template_only": True,
        "source_provenance_review_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "provenance_status": "unknown_requires_user_review",
        "license_status": "unknown_requires_user_review",
        "local_fixture_safety_status": "no_load_evidence_ready_pending_manual_review",
        "answer_schema": {
            "schema_state": "pending_answers_template_no_approval",
            "required_fields": [
                "question_id",
                "answer_status",
                "answer_text",
                "answer_evidence_reference",
                "local_only_acknowledged",
            ],
            "allowed_answer_statuses": [
                "confirmed_local_only",
                "not_confirmed_keep_hold",
                "reject_fixture",
                "needs_more_information",
            ],
            "forbidden_fields": [
                "approval_token",
                "approval_token_value",
                "approved_actions",
                "accepted_aex_path",
                "copied_fixture_path",
                "aex_hash",
                "raw_payload",
            ],
        },
        "answer_template_entries": entries,
        "answer_template_entry_count": len(entries),
        "answers_present": False,
        "answered_question_count": 0,
        "pending_answer_count": len(entries),
        "all_answers_pending": True,
        "user_answer_artifact_required": True,
        "answer_template_approves_fixture": False,
        "answer_template_approves_publication": False,
        "answer_template_approves_native_load": False,
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
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
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
    }


def user_answers_payload(template: dict, status: str = "confirmed_local_only") -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_user_answers",
        "answer_state": "fixture_provenance_user_answers_recorded_no_approval",
        "candidate_relative_path": template["candidate_relative_path"],
        "answers": [
            {
                "question_id": entry["question_id"],
                "answer_status": status,
                "answer_text": f"human {status} answer for {entry['question_id']}",
                "answer_evidence_reference": "local-review-note-1",
                "local_only_acknowledged": True,
                "approval_effect": "does_not_approve_native_load",
            }
            for entry in template["answer_template_entries"]
        ],
        "answers_approve_fixture": False,
        "answers_approve_publication": False,
        "answers_approve_native_load": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "approval_gate_stays_closed": True,
        "native_load_gate": "closed",
        "accepted_aex_path": None,
        "raw_input_paths_serialized": False,
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


def build_intake(template: dict, answers: dict) -> dict:
    return aex_fixture_provenance_answer_intake.build_fixture_provenance_answer_intake(
        answer_template=template,
        answer_template_path=Path("target/fixture-provenance-answer-template/template.json"),
        user_answers=answers,
        user_answers_path=Path("target/fixture-provenance-user-answers/answers.json"),
    )


class AexFixtureProvenanceAnswerIntakeTests(unittest.TestCase):
    def test_accepts_valid_confirmed_answers_without_echoing_answer_text(self):
        template = answer_template_payload()
        answers = user_answers_payload(template, "confirmed_local_only")
        report = build_intake(template, answers)

        self.assertEqual(report["report_kind"], "aex_fixture_provenance_answer_intake")
        self.assertEqual(report["intake_state"], "fixture_provenance_answers_accepted_for_manual_review")
        self.assertTrue(report["answers_accepted"])
        self.assertTrue(report["real_user_answer_artifact_consumed"])
        self.assertEqual(report["question_count"], 2)
        self.assertEqual(report["answered_question_count"], 2)
        self.assertEqual(report["pending_answer_count"], 0)
        self.assertEqual(report["rejection_reasons"], [])
        self.assertTrue(report["all_answers_confirmed_local_only"])
        self.assertTrue(report["answers_validated_for_manual_review"])
        self.assertEqual(
            report["recommended_next_decision"],
            "proceed_to_manual_review_then_explicit_decision_tool",
        )

        self.assertTrue(report["intake_is_not_approval"])
        self.assertFalse(report["approval_can_be_issued_now"])
        self.assertFalse(report["approval_manifest_created"])
        self.assertFalse(report["fixture_approval_satisfied"])
        self.assertEqual(report["native_load_gate"], "closed")
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["aex_file_hashed"])
        self.assertFalse(report["aex_file_copied"])

        serialized = json.dumps(report)
        self.assertNotIn("human confirmed_local_only answer", serialized)
        self.assertFalse(report["answer_text_echoed"])
        for row in report["answer_metadata_rows"]:
            self.assertNotIn("answer_text", row)
            self.assertGreater(row["answer_text_chars"], 0)
            self.assertFalse(row["answer_text_echoed"])

    def test_hold_and_reject_statuses_map_to_hold_and_reject_decisions(self):
        template = answer_template_payload()
        hold_report = build_intake(template, user_answers_payload(template, "not_confirmed_keep_hold"))
        self.assertTrue(hold_report["answers_accepted"])
        self.assertFalse(hold_report["all_answers_confirmed_local_only"])
        self.assertEqual(
            hold_report["recommended_next_decision"],
            "record_hold_decision_with_existing_decision_tool",
        )

        reject_report = build_intake(template, user_answers_payload(template, "reject_fixture"))
        self.assertTrue(reject_report["answers_accepted"])
        self.assertEqual(
            reject_report["recommended_next_decision"],
            "record_reject_decision_with_existing_decision_tool",
        )

    def test_rejects_missing_question_answer(self):
        template = answer_template_payload()
        answers = user_answers_payload(template)
        answers["answers"] = answers["answers"][:-1]
        report = build_intake(template, answers)
        self.assertFalse(report["answers_accepted"])
        self.assertEqual(report["intake_state"], "fixture_provenance_answers_rejected")
        self.assertEqual(report["answered_question_count"], 0)
        self.assertEqual(report["pending_answer_count"], 2)
        self.assertIn("user_answers_rejected", report["blockers"])
        self.assertTrue(any("missing question ids" in reason for reason in report["rejection_reasons"]))
        self.assertEqual(report["recommended_next_decision"], "fix_and_resubmit_user_answers")

    def test_rejects_empty_answer_text_and_missing_evidence(self):
        template = answer_template_payload()
        answers = user_answers_payload(template)
        answers["answers"][0]["answer_text"] = "   "
        answers["answers"][1]["answer_evidence_reference"] = ""
        report = build_intake(template, answers)
        self.assertFalse(report["answers_accepted"])
        self.assertTrue(any("answer_text must be a non-empty string" in reason for reason in report["rejection_reasons"]))
        self.assertTrue(
            any("answer_evidence_reference must be a non-empty string" in reason for reason in report["rejection_reasons"])
        )

    def test_rejects_unknown_answer_keys_and_unknown_question_ids(self):
        template = answer_template_payload()
        answers = user_answers_payload(template)
        answers["answers"][0]["extra_note"] = "should not be here"
        answers["answers"][1]["question_id"] = "unknown_question"
        report = build_intake(template, answers)
        self.assertFalse(report["answers_accepted"])
        self.assertTrue(any("unknown keys: extra_note" in reason for reason in report["rejection_reasons"]))
        self.assertTrue(any("unknown question ids" in reason for reason in report["rejection_reasons"]))

    def test_rejects_approval_style_keys_and_invalid_status(self):
        template = answer_template_payload()
        answers = user_answers_payload(template)
        answers["approval_token"] = "APPROVE_AEX_LOAD_GATE"
        answers["approved_actions"] = ["prepare_native_load_gate"]
        answers["answers"][0]["answer_status"] = "approve_native_load"
        report = build_intake(template, answers)
        self.assertFalse(report["answers_accepted"])
        self.assertIn("user answers contains forbidden key approval_token", report["rejection_reasons"])
        self.assertIn("user answers contains forbidden key approved_actions", report["rejection_reasons"])
        self.assertTrue(any("answer_status is not allowed" in reason for reason in report["rejection_reasons"]))

    def test_rejects_windows_absolute_path_values_in_answers(self):
        template = answer_template_payload()
        answers = user_answers_payload(template)
        answers["answers"][0]["answer_text"] = "source lives at D:\\Private\\Secret\\ScatterMap"
        report = build_intake(template, answers)
        self.assertFalse(report["answers_accepted"])
        self.assertTrue(
            any("windows absolute path value" in reason for reason in report["rejection_reasons"])
        )
        self.assertNotIn("D:\\Private\\Secret", json.dumps(report["answer_metadata_rows"]))

    def test_rejects_candidate_mismatch_and_open_gate_artifact(self):
        template = answer_template_payload()
        answers = user_answers_payload(template)
        answers["candidate_relative_path"] = "AEPluginBuild\\Other.aex"
        answers["native_load_gate"] = "open"
        report = build_intake(template, answers)
        self.assertFalse(report["answers_accepted"])
        self.assertIn("user answers candidate_relative_path must match template", report["rejection_reasons"])
        self.assertIn("user answers native_load_gate must be closed", report["rejection_reasons"])

    def test_invalid_template_raises_before_answer_validation(self):
        template = answer_template_payload()
        template["answers_present"] = True
        answers = user_answers_payload(answer_template_payload())
        with self.assertRaises(ValueError) as context:
            build_intake(template, answers)
        self.assertIn("answers_present must be false", str(context.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        template_root = LAB_ROOT / "target" / "fixture-provenance-answer-template"
        answers_root = LAB_ROOT / "target" / "fixture-provenance-user-answers"
        template_root.mkdir(parents=True, exist_ok=True)
        answers_root.mkdir(parents=True, exist_ok=True)
        stamp = time.time_ns()

        template_payload = answer_template_payload()
        template_path = template_root / f"{stamp}-answer-template.local.json"
        template_path.write_text(json.dumps(template_payload), encoding="utf-8")
        answers_path = answers_root / f"{stamp}-user-answers.local.json"
        answers_path.write_text(
            json.dumps(user_answers_payload(template_payload)),
            encoding="utf-8",
        )

        template, resolved_template = aex_fixture_provenance_answer_intake.load_answer_template(template_path)
        answers, resolved_answers = aex_fixture_provenance_answer_intake.load_user_answers(answers_path)
        self.assertEqual(resolved_template, template_path.resolve())
        self.assertEqual(resolved_answers, answers_path.resolve())

        with self.assertRaises(ValueError):
            aex_fixture_provenance_answer_intake.load_user_answers(template_path)

        report = aex_fixture_provenance_answer_intake.build_fixture_provenance_answer_intake(
            answer_template=template,
            answer_template_path=resolved_template,
            user_answers=answers,
            user_answers_path=resolved_answers,
        )
        self.assertTrue(report["answers_accepted"])

        out = (
            LAB_ROOT
            / "target"
            / "fixture-provenance-answer-intake"
            / f"{stamp}-answer-intake.local.json"
        )
        written = aex_fixture_provenance_answer_intake.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_provenance_answer_intake.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_fixture_provenance_answer_intake.write_json_create_new(
                LAB_ROOT / "target" / "outside-answer-intake.local.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
