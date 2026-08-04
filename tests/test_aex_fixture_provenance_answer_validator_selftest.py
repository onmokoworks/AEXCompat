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


aex_fixture_provenance_answer_validator_selftest = load_tool(
    "aex_fixture_provenance_answer_validator_selftest"
)


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


class AexFixtureProvenanceAnswerValidatorSelftestTests(unittest.TestCase):
    def test_build_selftest_validates_synthetic_cases_without_user_answers(self):
        template = answer_template_payload()
        report = aex_fixture_provenance_answer_validator_selftest.build_fixture_provenance_answer_validator_selftest(
            answer_template=template,
            answer_template_path=Path("target/fixture-provenance-answer-template/template.json"),
        )

        self.assertEqual(report["report_kind"], "aex_fixture_provenance_answer_validator_selftest")
        self.assertEqual(
            report["validator_selftest_state"],
            "fixture_provenance_answer_validator_selftest_passed_no_user_answers",
        )
        self.assertTrue(report["validator_ready"])
        self.assertTrue(report["source_answer_template_ready"])
        self.assertTrue(report["answer_template_only"])
        self.assertFalse(report["real_user_answer_artifact_consumed"])
        self.assertTrue(report["synthetic_user_answers_used"])
        self.assertFalse(report["synthetic_payloads_serialized"])
        self.assertTrue(report["answer_schema_validated"])
        self.assertEqual(report["synthetic_case_count"], 8)
        self.assertEqual(report["synthetic_case_passed_count"], 8)
        self.assertEqual(report["synthetic_case_failed_count"], 0)
        self.assertGreater(report["synthetic_valid_case_count"], 0)
        self.assertGreater(report["synthetic_rejected_case_count"], 0)
        self.assertFalse(report["answers_present"])
        self.assertEqual(report["answered_question_count"], 0)
        self.assertFalse(report["approval_can_be_issued_now"])
        self.assertFalse(report["approval_manifest_created"])
        self.assertFalse(report["fixture_approval_satisfied"])
        self.assertEqual(report["native_load_gate"], "closed")
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["raw_input_paths_serialized"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["aex_file_hashed"])
        self.assertFalse(report["aex_file_copied"])

    def test_validate_user_answer_artifact_accepts_synthetic_hold_answers(self):
        template = answer_template_payload()
        answer = aex_fixture_provenance_answer_validator_selftest.synthetic_answer_artifact(
            template,
            "not_confirmed_keep_hold",
        )
        self.assertEqual(
            aex_fixture_provenance_answer_validator_selftest.validate_user_answer_artifact(template, answer),
            [],
        )

    def test_validate_user_answer_artifact_rejects_approval_tokens_and_actions(self):
        template = answer_template_payload()
        answer = aex_fixture_provenance_answer_validator_selftest.synthetic_answer_artifact(
            template,
            "confirmed_local_only",
        )
        answer["approval_token"] = "APPROVE_AEX_LOAD_GATE"
        answer["approved_actions"] = ["prepare_native_load_gate"]
        errors = aex_fixture_provenance_answer_validator_selftest.validate_user_answer_artifact(template, answer)
        self.assertIn("user answers contains forbidden key approval_token", errors)
        self.assertIn("user answers contains forbidden key approved_actions", errors)

    def test_validate_user_answer_artifact_rejects_missing_or_unknown_questions(self):
        template = answer_template_payload()
        answer = aex_fixture_provenance_answer_validator_selftest.synthetic_answer_artifact(
            template,
            "not_confirmed_keep_hold",
        )
        answer["answers"] = answer["answers"][:-1]
        errors = aex_fixture_provenance_answer_validator_selftest.validate_user_answer_artifact(template, answer)
        self.assertTrue(any("exactly one answer" in error for error in errors))
        self.assertTrue(any("missing question ids" in error for error in errors))

        answer = aex_fixture_provenance_answer_validator_selftest.synthetic_answer_artifact(
            template,
            "not_confirmed_keep_hold",
        )
        answer["answers"][0]["question_id"] = "unknown_question"
        errors = aex_fixture_provenance_answer_validator_selftest.validate_user_answer_artifact(template, answer)
        self.assertTrue(any("unknown question ids" in error for error in errors))

    def test_rejects_template_with_real_answers_or_open_gate(self):
        template = answer_template_payload()
        template["answers_present"] = True
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_answer_validator_selftest.build_fixture_provenance_answer_validator_selftest(
                answer_template=template,
                answer_template_path=Path("target/fixture-provenance-answer-template/template.json"),
            )
        self.assertIn("answers_present must be false", str(context.exception))

        template = answer_template_payload()
        template["native_load_gate"] = "open"
        with self.assertRaises(ValueError) as context:
            aex_fixture_provenance_answer_validator_selftest.build_fixture_provenance_answer_validator_selftest(
                answer_template=template,
                answer_template_path=Path("target/fixture-provenance-answer-template/template.json"),
            )
        self.assertIn("native_load_gate must be closed", str(context.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        root = LAB_ROOT / "target" / "fixture-provenance-answer-template"
        root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        source = root / f"{stamp}-answer-template.local.json"
        source.write_text(json.dumps(answer_template_payload()), encoding="utf-8")

        loaded, resolved = aex_fixture_provenance_answer_validator_selftest.load_answer_template(source)
        self.assertEqual(loaded["report_kind"], "aex_fixture_provenance_answer_template")
        self.assertEqual(resolved, source.resolve())

        report = aex_fixture_provenance_answer_validator_selftest.build_fixture_provenance_answer_validator_selftest(
            answer_template=loaded,
            answer_template_path=resolved,
        )
        out = (
            LAB_ROOT
            / "target"
            / "fixture-provenance-answer-validator-selftest"
            / f"{stamp}-validator-selftest.local.json"
        )
        written = aex_fixture_provenance_answer_validator_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_provenance_answer_validator_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_fixture_provenance_answer_validator_selftest.write_json_create_new(
                LAB_ROOT / "target" / "outside-validator-selftest.local.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
