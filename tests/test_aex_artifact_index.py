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


aex_artifact_index = load_tool("aex_artifact_index")


def payload_for(label: str) -> dict:
    spec = aex_artifact_index.ARTIFACT_SPECS[label]
    payload = {
        "schema_version": 1,
        "publication_status": "local-only",
        spec["kind_key"]: spec["kind"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }
    if label == "safety_audit":
        payload.update({"audit_passed": True, "audit_state": "no_load_chain_verified"})
    if label == "publication_boundary":
        payload.update({"publishable_now": False, "boundary_state": "local_only_not_publishable"})
    return payload


def write_payload(label: str, name: str, payload: dict | None = None) -> Path:
    root = aex_artifact_index.ARTIFACT_SPECS[label]["root"]
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload or payload_for(label)), encoding="utf-8")
    return path


class AexArtifactIndexTests(unittest.TestCase):
    def test_preferred_pattern_rank_prefers_real_chain_names(self):
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(Path("ae-load-gate-with-hold-1.local.json"), ["ae-load-gate-with-hold-*.local.json"]),
            aex_artifact_index.preferred_pattern_rank(Path("178-test.local.json"), ["ae-load-gate-with-hold-*.local.json"]),
        )

    def test_candidate_compatibility_card_pattern_excludes_probe_names(self):
        patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_compatibility_card"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-compat-card-1780662293457.local.json"), patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-compat-card-probe-1780662000000.local.json"), patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-compat-card-1780663857449134900.local.json"), patterns
            ),
            0,
        )

    def test_candidate_image_and_ofx_bridge_patterns_exclude_test_ns_names(self):
        image_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_image_compat_mock"]["patterns"]
        bridge_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_bridge"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-image-compat-mock-1780663165368.local.json"), image_patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-image-compat-mock-1780663857449134900.local.json"), image_patterns
            ),
            0,
        )
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-bridge-1780663875265.local.json"), bridge_patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-bridge-1780663857449134900.local.json"), bridge_patterns
            ),
            0,
        )
        harness_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_host_harness_dryrun"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-host-harness-dryrun-1780664019023.local.json"), harness_patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-host-harness-dryrun-1780664019023123456.local.json"), harness_patterns
            ),
            0,
        )
        selftest_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_host_harness_selftest"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-host-harness-selftest-1780664019023.local.json"), selftest_patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-host-harness-selftest-1780664019023123456.local.json"), selftest_patterns
            ),
            0,
        )
        boundary_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_runtime_boundary_contract"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-boundary-contract-1780664019023.local.json"), boundary_patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-boundary-contract-1780664019023123456.local.json"),
                boundary_patterns,
            ),
            0,
        )
        request_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_runtime_approval_request"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-approval-request-1780664019023.local.json"), request_patterns
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-approval-request-1780664019023123456.local.json"),
                request_patterns,
            ),
            0,
        )
        verifier_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_runtime_approval_verifier"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-approval-verifier-1780664019023.local.json"),
                verifier_patterns,
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-approval-verifier-1780664019023123456.local.json"),
                verifier_patterns,
            ),
            0,
        )
        audit_patterns = aex_artifact_index.ARTIFACT_SPECS["candidate_ofx_runtime_prerequisite_audit"]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-prerequisite-audit-1780664019023.local.json"),
                audit_patterns,
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-runtime-prerequisite-audit-1780664019023123456.local.json"),
                audit_patterns,
            ),
            0,
        )
        host_review_patterns = aex_artifact_index.ARTIFACT_SPECS[
            "candidate_ofx_host_binary_review_request"
        ]["patterns"]
        self.assertGreater(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-host-binary-review-request-1780664019023.local.json"),
                host_review_patterns,
            ),
            0,
        )
        self.assertEqual(
            aex_artifact_index.preferred_pattern_rank(
                Path("ae-candidate-ofx-host-binary-review-request-1780664019023123456.local.json"),
                host_review_patterns,
            ),
            0,
        )

    def test_completeness_score_prefers_real_artifact_payload_shape(self):
        label = "publication_boundary"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "source_safety_audit": "target/safety-audit/audit.json",
            "publication_blockers": ["local-only"],
            "redacted_local_summary": {"artifact_labels": []},
        }
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )

    def test_aepx_redacted_text_inventory_spec_is_indexed(self):
        label = "aepx_redacted_text_inventory"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "inventory_state": "aepx_redacted_text_inventory_ready_no_write",
            "inventory_contract": {"state": "redacted_text_inventory_ready_no_write"},
            "summary": {"text_node_count": 1},
            "redaction_policy": {"state": "text_inventory_redacted_metadata_only"},
            "text_payload_exported": False,
            "text_payload_hash_exported": False,
            "bdata_payload_exported": False,
            "raw_text_fields_present": False,
            "value_hashes_emitted": False,
        }
        self.assertEqual(aex_artifact_index.ARTIFACT_SPECS[label]["kind"], "aepx_redacted_text_inventory")
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["inventory_state"], "aepx_redacted_text_inventory_ready_no_write")
        self.assertFalse(states["text_payload_hash_exported"])
        self.assertFalse(states["value_hashes_emitted"])

    def test_aepx_redacted_text_classifier_spec_is_indexed(self):
        label = "aepx_redacted_text_classifier"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "classifier_state": "aepx_redacted_text_classifier_ready_no_write",
            "classifier_ready": True,
            "source_chain_valid": True,
            "inventory_rows_classified": True,
            "row_count_matches_inventory_summary": True,
            "redacted_text_classification_ready": True,
            "project_write_recommendation": "do_not_write_project_files",
            "project_write_ready": False,
            "project_write_allowed_now": False,
            "classifier_approves_project_write": False,
            "schema_write_allowed_now": False,
            "approved_write_candidate_count": 0,
            "classification_row_count": 1,
            "no_write_row_count": 1,
            "unknown_row_count": 0,
            "text_payload_exported": False,
            "text_payload_hash_exported": False,
            "bdata_payload_exported": False,
            "raw_text_fields_present": False,
            "value_hashes_emitted": False,
            "absolute_source_paths_in_classifier_rows": False,
            "raw_payload_serialized": False,
            "classifier_contract": {"state": "redacted_text_classifier_ready_no_write"},
            "classification_rows": [{"row_id": "text_0000", "classification": "review_label_like_string_candidate_no_write"}],
            "summary": {"classification_row_count": 1},
        }
        self.assertEqual(aex_artifact_index.ARTIFACT_SPECS[label]["kind"], "aepx_redacted_text_classifier")
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["classifier_state"], "aepx_redacted_text_classifier_ready_no_write")
        self.assertTrue(states["classifier_ready"])
        self.assertFalse(states["project_write_ready"])
        self.assertFalse(states["classifier_approves_project_write"])
        self.assertFalse(states["absolute_source_paths_in_classifier_rows"])

    def test_synthetic_pipl_payload_parser_spec_is_indexed(self):
        label = "synthetic_pipl_payload_parser"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "synthetic_payload_parser_state": "synthetic_pipl_payload_parser_ready_real_payload_closed",
            "synthetic_payload_parser_ready": True,
            "synthetic_parser_implemented": True,
            "synthetic_bounds_harness_reused": True,
            "synthetic_payload_cases_passed": True,
            "synthetic_payloads_used": True,
            "synthetic_payloads_serialized": False,
            "real_payload_input_allowed_now": False,
            "output_metadata_only": True,
            "real_pipl_payload_parser_enabled": False,
            "real_pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
            "parser_case_count": 8,
            "parser_case_passed_count": 8,
            "parser_case_failed_count": 0,
            "parser_api_contract": {"state": "synthetic_parser_implementation_ready_real_payload_closed"},
            "parser_case_results": [{"case_id": "valid", "passed": True}],
            "blockers": ["real_payload_adapter_not_enabled"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_synthetic_pipl_payload_parser",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["synthetic_payload_parser_state"],
            "synthetic_pipl_payload_parser_ready_real_payload_closed",
        )
        self.assertTrue(states["synthetic_payload_parser_ready"])
        self.assertTrue(states["synthetic_parser_implemented"])
        self.assertFalse(states["real_payload_input_allowed_now"])
        self.assertTrue(states["output_metadata_only"])
        self.assertEqual(states["parser_case_failed_count"], 0)

    def test_pipl_resource_consistency_audit_spec_is_indexed(self):
        label = "pipl_resource_consistency_audit"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "audit_state": "pipl_resource_consistency_audit_passed_no_payload",
            "audit_passed": True,
            "metadata_consistency_ready": True,
            "source_chain_valid": True,
            "catalog_summary_recomputed": True,
            "catalog_rows_recomputed": True,
            "gate_budget_rows_recomputed": True,
            "gate_summary_recomputed": True,
            "real_payload_input_allowed_now": False,
            "real_pipl_payload_parser_enabled": False,
            "real_pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "resource_payload_extracted": False,
            "raw_payload_serialized": False,
            "checks": [{"check_id": "catalog_rows_recomputed_from_static", "passed": True}],
            "summary": {"catalog_row_count": 2, "gate_budget_row_count": 2},
            "blockers": ["real_payload_adapter_not_enabled"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_pipl_resource_consistency_audit",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["audit_state"], "pipl_resource_consistency_audit_passed_no_payload")
        self.assertTrue(states["audit_passed"])
        self.assertTrue(states["metadata_consistency_ready"])
        self.assertTrue(states["source_chain_valid"])
        self.assertTrue(states["catalog_rows_recomputed"])
        self.assertTrue(states["gate_budget_rows_recomputed"])
        self.assertFalse(states["resource_payload_extracted"])
        self.assertFalse(states["raw_payload_serialized"])

    def test_pipl_payload_adapter_review_spec_is_indexed(self):
        label = "pipl_payload_adapter_review"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "adapter_review_state": "pipl_payload_adapter_review_ready_real_payload_closed",
            "adapter_review_ready": True,
            "source_chain_valid": True,
            "synthetic_parser_contract_reviewed": True,
            "metadata_consistency_reviewed": True,
            "metadata_budget_reviewed": True,
            "parameter_schema_reviewed": True,
            "redaction_policy_reviewed": True,
            "ofx_describe_policy_reviewed": True,
            "real_payload_adapter_allowed_now": False,
            "real_payload_input_allowed_now": False,
            "real_pipl_payload_parser_enabled": False,
            "real_pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "resource_payload_extracted": False,
            "raw_payload_serialized": False,
            "output_metadata_only": True,
            "parameter_schema_emission_allowed_now": False,
            "parameter_schema_emitted": False,
            "redacted_schema_emitted": False,
            "review_item_count": 6,
            "blocking_review_item_count": 6,
            "adapter_review_contract": {"state": "review_packet_ready_real_payload_closed"},
            "review_items": [{"item_id": "real_payload_input_boundary"}],
            "candidate_review_budget": {"eligible_future_parser_candidate_count": 1},
            "blockers": ["real_payload_access_not_approved"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_pipl_payload_adapter_review_packet",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["adapter_review_state"], "pipl_payload_adapter_review_ready_real_payload_closed")
        self.assertTrue(states["adapter_review_ready"])
        self.assertFalse(states["real_payload_adapter_allowed_now"])
        self.assertFalse(states["parameter_schema_emission_allowed_now"])

    def test_fixture_manual_review_packet_spec_is_indexed(self):
        label = "fixture_manual_review_packet"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "review_packet_state": "fixture_manual_review_packet_ready_no_load",
            "manual_review_ready": True,
            "approval_ready": False,
            "approval_blocker_count": 3,
            "recommended_next_decision": "keep_hold_pending_manual_review",
            "decision_summary": {"dossier_state": "manual_review_pending"},
            "dependency_summary": {"native_load_recommendation": "do_not_open_native_load_gate"},
            "load_gate_summary": {"gate_state": "closed_dependency_review_or_invalid_approval"},
        }
        self.assertEqual(aex_artifact_index.ARTIFACT_SPECS[label]["kind"], "aex_fixture_manual_review_packet")
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["review_packet_state"], "fixture_manual_review_packet_ready_no_load")
        self.assertTrue(states["manual_review_ready"])
        self.assertFalse(states["approval_ready"])

    def test_fixture_approval_verifier_spec_is_indexed(self):
        label = "fixture_approval_verifier"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "approval_verifier_state": "fixture_approval_verifier_ready_no_approval",
            "approval_verifier_ready": True,
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
            "current_approval_evaluation": {"valid": False, "reasons": ["manifest_kind_not_approval"]},
            "synthetic_approval_checks_passed": True,
            "synthetic_approval_checks": [{"case": "current_hold_manifest_rejected", "valid": False}],
            "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
            "approval_token_not_stored_in_manifest": True,
            "approval_only_prepares_next_gate": True,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_fixture_approval_verifier",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["approval_verifier_state"], "fixture_approval_verifier_ready_no_approval")
        self.assertTrue(states["approval_verifier_ready"])
        self.assertFalse(states["current_fixture_approval_valid"])
        self.assertFalse(states["fixture_approval_satisfied"])
        self.assertTrue(states["approval_gate_stays_closed"])
        self.assertFalse(states["manual_review_approval_ready"])
        self.assertTrue(states["path_policy_closed"])
        self.assertTrue(states["candidate_load_gate_closed"])
        self.assertTrue(states["synthetic_approval_checks_passed"])
        self.assertEqual(states["required_approval_token_name"], "APPROVE_AEX_LOAD_GATE")

    def test_fixture_approval_request_spec_is_indexed(self):
        label = "fixture_approval_request"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "approval_request_state": "fixture_approval_request_ready_pending_manual_approval",
            "approval_request_ready": True,
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "requires_explicit_user_approval": True,
            "current_fixture_approval_valid": False,
            "fixture_approval_satisfied": False,
            "manual_review_ready": True,
            "manual_review_approval_ready": False,
            "approval_blocker_count": 4,
            "approval_gate_stays_closed": True,
            "native_load_gate": "closed",
            "candidate_dependencies_clear": True,
            "path_policy_closed": True,
            "candidate_load_gate_closed": True,
            "review_checklist": [{"id": "explicit_user_approval"}],
            "approval_blockers": [{"id": "manual_fixture_review_pending"}],
            "forbidden_actions_after_approval": ["load_aex_dll"],
            "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
            "approval_token_not_stored_in_manifest": True,
            "approval_only_prepares_next_gate": True,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_fixture_approval_request_packet",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["approval_request_state"], "fixture_approval_request_ready_pending_manual_approval")
        self.assertTrue(states["approval_request_ready"])
        self.assertFalse(states["approval_can_be_issued_now"])
        self.assertFalse(states["approval_manifest_created"])
        self.assertTrue(states["requires_explicit_user_approval"])
        self.assertFalse(states["current_fixture_approval_valid"])
        self.assertEqual(states["native_load_gate"], "closed")
        self.assertEqual(states["required_approval_token_name"], "APPROVE_AEX_LOAD_GATE")

    def test_fixture_provenance_review_spec_is_indexed(self):
        label = "fixture_provenance_review"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "provenance_review_state": "fixture_provenance_review_packet_ready_no_load",
            "provenance_review_ready": True,
            "manual_review_source_ready": True,
            "approval_request_source_ready": True,
            "provenance_status": "unknown_requires_user_review",
            "license_status": "unknown_requires_user_review",
            "safety_review_status": "pending_user_review_no_load",
            "publication_boundary_status": "local_only_not_publishable",
            "local_fixture_safety_status": "no_load_evidence_ready_pending_manual_review",
            "approval_request_ready": True,
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "requires_explicit_user_approval": True,
            "current_fixture_approval_valid": False,
            "fixture_approval_satisfied": False,
            "approval_gate_stays_closed": True,
            "native_load_gate": "closed",
            "manual_review_approval_ready": False,
            "native_load_gate_stays_closed": True,
            "required_approval_token_name": "APPROVE_AEX_LOAD_GATE",
            "approval_token_not_stored_in_manifest": True,
            "approval_only_prepares_next_gate": True,
            "review_item_count": 6,
            "blocking_review_item_count": 3,
            "review_question_count": 4,
            "unanswered_review_question_count": 4,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "raw_input_paths_serialized": False,
            "provenance_review_items": [{"item_id": "provenance_confirmation"}],
            "review_questions": [{"question_id": "provenance_confirmed"}],
            "blockers": ["provenance_not_confirmed"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_fixture_provenance_review_packet",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["provenance_review_state"], "fixture_provenance_review_packet_ready_no_load")
        self.assertTrue(states["provenance_review_ready"])
        self.assertTrue(states["manual_review_source_ready"])
        self.assertTrue(states["approval_request_source_ready"])
        self.assertEqual(states["provenance_status"], "unknown_requires_user_review")
        self.assertEqual(states["license_status"], "unknown_requires_user_review")
        self.assertFalse(states["approval_can_be_issued_now"])
        self.assertFalse(states["approval_manifest_created"])
        self.assertFalse(states["current_fixture_approval_valid"])
        self.assertFalse(states["fixture_approval_satisfied"])
        self.assertTrue(states["native_load_gate_stays_closed"])
        self.assertFalse(states["aex_file_hashed"])
        self.assertFalse(states["aex_file_copied"])

    def test_fixture_provenance_answer_template_spec_is_indexed(self):
        label = "fixture_provenance_answer_template"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "template_state": "fixture_provenance_answer_template_ready_all_answers_pending_no_load",
            "template_ready": True,
            "answer_template_only": True,
            "source_provenance_review_ready": True,
            "provenance_status": "unknown_requires_user_review",
            "license_status": "unknown_requires_user_review",
            "answer_template_entry_count": 2,
            "answers_present": False,
            "answered_question_count": 0,
            "pending_answer_count": 2,
            "all_answers_pending": True,
            "user_answer_artifact_required": True,
            "answer_template_approves_fixture": False,
            "answer_template_approves_publication": False,
            "answer_template_approves_native_load": False,
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "current_fixture_approval_valid": False,
            "fixture_approval_satisfied": False,
            "approval_gate_stays_closed": True,
            "native_load_gate": "closed",
            "native_load_gate_stays_closed": True,
            "accepted_aex_path": None,
            "raw_input_paths_serialized": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "answer_template_entries": [{"question_id": "provenance_confirmed"}],
            "answer_schema": {"schema_state": "pending_answers_template_no_approval"},
            "blockers": ["user_answers_not_recorded"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_fixture_provenance_answer_template",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["template_state"],
            "fixture_provenance_answer_template_ready_all_answers_pending_no_load",
        )
        self.assertTrue(states["template_ready"])
        self.assertTrue(states["answer_template_only"])
        self.assertTrue(states["source_provenance_review_ready"])
        self.assertFalse(states["answers_present"])
        self.assertEqual(states["answered_question_count"], 0)
        self.assertEqual(states["pending_answer_count"], 2)
        self.assertTrue(states["all_answers_pending"])
        self.assertFalse(states["answer_template_approves_fixture"])
        self.assertFalse(states["answer_template_approves_native_load"])
        self.assertFalse(states["approval_can_be_issued_now"])
        self.assertFalse(states["fixture_approval_satisfied"])
        self.assertEqual(states["native_load_gate"], "closed")

    def test_fixture_provenance_answer_validator_selftest_spec_is_indexed(self):
        label = "fixture_provenance_answer_validator_selftest"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "validator_selftest_state": "fixture_provenance_answer_validator_selftest_passed_no_user_answers",
            "validator_ready": True,
            "source_answer_template_ready": True,
            "answer_template_only": True,
            "real_user_answer_artifact_consumed": False,
            "synthetic_user_answers_used": True,
            "synthetic_payloads_serialized": False,
            "answer_schema_validated": True,
            "synthetic_case_count": 8,
            "synthetic_case_passed_count": 8,
            "synthetic_case_failed_count": 0,
            "synthetic_valid_case_count": 2,
            "synthetic_rejected_case_count": 6,
            "answers_present": False,
            "answered_question_count": 0,
            "answers_validated_for_manual_review": False,
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "current_fixture_approval_valid": False,
            "fixture_approval_satisfied": False,
            "approval_gate_stays_closed": True,
            "native_load_gate": "closed",
            "native_load_gate_stays_closed": True,
            "accepted_aex_path": None,
            "raw_input_paths_serialized": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "answer_validation_contract": {"contract_state": "fixture_provenance_answer_validation_contract_ready_no_user_answers"},
            "synthetic_case_results": [{"case_id": "valid_hold_answers_no_approval", "passed": True}],
            "blockers": ["real_user_answers_not_supplied"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_fixture_provenance_answer_validator_selftest",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["validator_selftest_state"],
            "fixture_provenance_answer_validator_selftest_passed_no_user_answers",
        )
        self.assertTrue(states["validator_ready"])
        self.assertFalse(states["real_user_answer_artifact_consumed"])
        self.assertTrue(states["synthetic_user_answers_used"])
        self.assertFalse(states["synthetic_payloads_serialized"])
        self.assertTrue(states["answer_schema_validated"])
        self.assertEqual(states["synthetic_case_failed_count"], 0)
        self.assertFalse(states["answers_present"])
        self.assertFalse(states["answers_validated_for_manual_review"])
        self.assertFalse(states["approval_can_be_issued_now"])
        self.assertFalse(states["fixture_approval_satisfied"])
        self.assertEqual(states["native_load_gate"], "closed")

    def test_candidate_test_handoff_spec_is_indexed(self):
        label = "candidate_test_handoff"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "handoff_state": "candidate_test_handoff_ready_no_load_native_closed",
            "handoff_packet_ready": True,
            "no_load_test_handoff_ready": True,
            "native_test_handoff_ready": False,
            "approval_request_ready": True,
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "requires_explicit_user_approval": True,
            "fixture_approval_satisfied": False,
            "native_load_gate": "closed",
            "candidate_dependencies_clear": True,
            "global_dependency_blockers_apply_to_candidate": False,
            "native_loader_design_ready": True,
            "runtime_containment_ready": True,
            "runtime_containment_selftest_passed": True,
            "synthetic_subprocess_only": True,
            "normal_exit_case_passed": True,
            "stderr_capture_passed": True,
            "timeout_case_passed": True,
            "child_cleanup_passed": True,
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "path_policy_selftest_passed": True,
            "candidate_path_string_accepted": False,
            "raw_input_paths_serialized": False,
            "no_load_image_test_ready": True,
            "image_fixture_validation_state": "image_fixture_validation_passed_no_load",
            "image_fixture_validation_passed": True,
            "image_input_smoke_state": "image_input_smoke_passed_route_closed",
            "worker_identity_passed": True,
            "ofx_identity_passed": True,
            "no_load_render_contract_ready": True,
            "real_render_open": False,
            "no_load_validation_ready": True,
            "no_load_ofx_mock_ready": True,
            "real_route_open": False,
            "mock_route_ready": True,
            "handoff_blocker_count": 5,
            "handoff_checks": [{"id": "approval_request"}],
            "handoff_blockers": [{"id": "manual_fixture_approval_pending"}],
            "allowed_no_load_handoff_actions": ["rerun_image_input_smoke_identity"],
            "forbidden_handoff_actions": ["accept_aex_path"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_test_handoff_packet",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["handoff_state"], "candidate_test_handoff_ready_no_load_native_closed")
        self.assertTrue(states["handoff_packet_ready"])
        self.assertTrue(states["no_load_test_handoff_ready"])
        self.assertFalse(states["native_test_handoff_ready"])
        self.assertTrue(states["runtime_containment_selftest_passed"])
        self.assertTrue(states["no_load_image_test_ready"])
        self.assertTrue(states["image_fixture_validation_passed"])
        self.assertTrue(states["no_load_render_contract_ready"])
        self.assertTrue(states["no_load_ofx_mock_ready"])
        self.assertEqual(states["handoff_blocker_count"], 5)

    def test_candidate_test_runner_dryrun_spec_is_indexed(self):
        label = "candidate_test_runner_dryrun"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "runner_dryrun_state": "candidate_no_load_test_runner_dryrun_ready_native_closed",
            "runner_dryrun_ready": True,
            "dry_run_only": True,
            "would_execute": False,
            "execution_performed": False,
            "no_load_test_plan_ready": True,
            "native_test_plan_ready": False,
            "real_render_plan_ready": False,
            "real_ofx_route_plan_ready": False,
            "image_fixture_case_count": 2,
            "planned_no_load_case_count": 10,
            "planned_native_case_count": 0,
            "planned_real_render_case_count": 0,
            "planned_real_ofx_route_case_count": 0,
            "blocked_case_count": 12,
            "image_fixture_validation_passed": True,
            "worker_suite_identity_passed": True,
            "ofx_suite_identity_passed": True,
            "image_smoke_identity_passed": True,
            "render_contract_review_ready": True,
            "ofx_route_contract_review_ready": True,
            "approval_manifest_created": False,
            "fixture_approval_satisfied": False,
            "native_load_gate": "closed",
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "real_render_open": False,
            "real_route_open": False,
            "planned_tests": [{"case_id": "first"}],
            "blocked_cases": [{"case_id": "accept_aex_path"}],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_no_load_test_runner_dryrun",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["runner_dryrun_state"],
            "candidate_no_load_test_runner_dryrun_ready_native_closed",
        )
        self.assertTrue(states["runner_dryrun_ready"])
        self.assertTrue(states["dry_run_only"])
        self.assertFalse(states["would_execute"])
        self.assertFalse(states["execution_performed"])
        self.assertTrue(states["no_load_test_plan_ready"])
        self.assertFalse(states["native_test_plan_ready"])
        self.assertEqual(states["planned_native_case_count"], 0)

    def test_candidate_test_runner_spec_is_indexed(self):
        label = "candidate_test_runner"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "runner_state": "candidate_no_load_test_runner_passed_native_closed",
            "runner_ready": True,
            "dry_run_only": False,
            "would_execute": True,
            "execution_performed": True,
            "no_load_test_plan_ready": True,
            "no_load_execution_performed": True,
            "native_test_plan_ready": False,
            "native_execution_performed": False,
            "real_render_plan_ready": False,
            "real_render_execution_performed": False,
            "real_ofx_route_plan_ready": False,
            "real_ofx_route_execution_performed": False,
            "worker_invoked": True,
            "ofx_mock_invoked": True,
            "ofx_runtime_invoked": False,
            "worker_identity_passed": True,
            "ofx_noop_identity_passed": True,
            "blocked_load_aex_verified": True,
            "image_fixture_validation_passed": True,
            "image_smoke_identity_passed": True,
            "render_contract_review_ready": True,
            "ofx_route_contract_review_ready": True,
            "image_fixture_case_count": 2,
            "planned_no_load_case_count": 10,
            "planned_native_case_count": 0,
            "planned_real_render_case_count": 0,
            "planned_real_ofx_route_case_count": 0,
            "executed_worker_case_count": 2,
            "executed_ofx_noop_case_count": 2,
            "executed_worker_lifecycle_case_count": 4,
            "executed_native_case_count": 0,
            "executed_real_render_case_count": 0,
            "executed_real_ofx_route_case_count": 0,
            "blocked_case_count": 12,
            "approval_manifest_created": False,
            "fixture_approval_satisfied": False,
            "native_load_gate": "closed",
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "real_render_open": False,
            "real_route_open": False,
            "planned_tests": [{"case_id": "first"}],
            "blocked_cases": [{"case_id": "accept_aex_path"}],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_no_load_test_runner",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["runner_state"], "candidate_no_load_test_runner_passed_native_closed")
        self.assertTrue(states["runner_ready"])
        self.assertTrue(states["no_load_execution_performed"])
        self.assertFalse(states["native_execution_performed"])
        self.assertTrue(states["worker_invoked"])
        self.assertTrue(states["ofx_mock_invoked"])
        self.assertFalse(states["ofx_runtime_invoked"])
        self.assertTrue(states["blocked_load_aex_verified"])
        self.assertEqual(states["executed_native_case_count"], 0)

    def test_candidate_dependency_scope_spec_is_indexed(self):
        label = "candidate_dependency_scope"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "candidate_dependency_scope_state": "candidate_dependency_scope_ready_no_load",
            "candidate_scope_ready": True,
            "candidate_dependency_blockers_present": False,
            "candidate_dependency_blocker_count": 0,
            "candidate_dependency_review_count": 0,
            "candidate_dependency_missing_or_api_set_review_count": 0,
            "candidate_dependency_found_paths_exported": False,
            "global_dependency_blockers_present": True,
            "global_dependency_blockers_apply_to_candidate": False,
            "scoped_gate_recommendation": "candidate_dependencies_clear_global_gate_still_closed",
            "candidate_dependency_summary": {"candidate_dependency_count": 3},
            "global_dependency_summary": {"global_blocker_count": 1},
        }
        self.assertEqual(aex_artifact_index.ARTIFACT_SPECS[label]["kind"], "aex_candidate_dependency_scope_packet")
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["candidate_dependency_scope_state"], "candidate_dependency_scope_ready_no_load")
        self.assertFalse(states["candidate_dependency_blockers_present"])
        self.assertEqual(states["candidate_dependency_blocker_count"], 0)

    def test_candidate_compatibility_card_spec_is_indexed(self):
        label = "candidate_compatibility_card"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "compatibility_card_state": "candidate_compatibility_card_ready_no_load",
            "compatibility_card_ready": True,
            "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
            "unsafe_exports_present": False,
            "absolute_ppm_paths_exported": False,
            "absolute_aex_paths_exported": False,
            "resource_payload_opened": False,
            "resource_payload_extracted": False,
            "raw_payload_serialized": False,
            "pipl_payload_parsed": False,
            "parameter_schema_emitted": False,
            "redacted_schema_emitted": False,
            "approval_can_be_issued_now": False,
            "approval_manifest_created": False,
            "current_fixture_approval_valid": False,
            "fixture_approval_satisfied": False,
            "approval_gate_stays_closed": True,
            "native_load_gate": "closed",
            "native_load_gate_stays_closed": True,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "real_render_open": False,
            "real_route_open": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "candidate_metadata": {"file_name": "ScatterMap.aex"},
            "no_load_test_card": {"worker_identity_passed": True},
            "gate_card": {"native_load_gate": "closed"},
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_compatibility_card",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["compatibility_card_state"], "candidate_compatibility_card_ready_no_load")
        self.assertTrue(states["compatibility_card_ready"])
        self.assertFalse(states["unsafe_exports_present"])
        self.assertFalse(states["absolute_ppm_paths_exported"])
        self.assertFalse(states["absolute_aex_paths_exported"])
        self.assertFalse(states["approval_can_be_issued_now"])
        self.assertFalse(states["fixture_approval_satisfied"])
        self.assertEqual(states["native_load_gate"], "closed")

    def test_candidate_image_compat_mock_spec_is_indexed(self):
        label = "candidate_image_compat_mock"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "mock_state": "candidate_image_compat_mock_passed_no_load",
            "mock_ready": True,
            "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
            "operation": "invert",
            "source_compatibility_card_state": "candidate_compatibility_card_ready_no_load",
            "source_compatibility_card_ready": True,
            "source_native_load_gate": "closed",
            "source_native_load_gate_stays_closed": True,
            "source_real_render_open": False,
            "source_real_route_open": False,
            "source_path_acceptance_ready": False,
            "source_aex_path_acceptance_enabled": False,
            "source_fixture_approval_satisfied": False,
            "source_absolute_ppm_paths_exported": False,
            "source_absolute_aex_paths_exported": False,
            "input_ppm_absolute_path_exported": False,
            "output_ppm_absolute_path_exported": False,
            "transform_check": {
                "pixel_match_expected": True,
                "dimension_match_expected": True,
                "input_dimension_match": True,
            },
            "candidate_image_mock_performed": True,
            "mock_transform_performed": True,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_image_compat_mock",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["mock_state"], "candidate_image_compat_mock_passed_no_load")
        self.assertTrue(states["mock_ready"])
        self.assertEqual(states["operation"], "invert")
        self.assertFalse(states["input_ppm_absolute_path_exported"])
        self.assertFalse(states["output_ppm_absolute_path_exported"])
        self.assertTrue(states["candidate_image_mock_performed"])
        self.assertTrue(states["mock_transform_performed"])
        self.assertEqual(states["source_native_load_gate"], "closed")

    def test_candidate_ofx_bridge_spec_is_indexed(self):
        label = "candidate_ofx_bridge"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
            "bridge_ready": True,
            "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
            "candidate_image_mock_available": True,
            "ofx_closed_route_contract_available": True,
            "ofx_bridge_packet_created": True,
            "source_compatibility_card_state": "candidate_compatibility_card_ready_no_load",
            "source_image_mock_state": "candidate_image_compat_mock_passed_no_load",
            "source_ofx_facade_state": "deferred_loader_not_ready",
            "source_ofx_route_contract_state": "ofx_route_contract_ready_route_closed",
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
            "image_surface": {"operation": "invert"},
            "ofx_bridge_plan": {"state": "mock_output_bound_to_closed_ofx_contract"},
            "gate_card": {"native_load_gate": "closed"},
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_bridge_packet",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["bridge_state"], "candidate_ofx_bridge_ready_no_load_route_closed")
        self.assertTrue(states["bridge_ready"])
        self.assertEqual(states["bridge_allowed_route"], "no_op_identity_only")
        self.assertFalse(states["real_route_open"])
        self.assertFalse(states["real_ofx_route_ready"])
        self.assertFalse(states["ofx_runtime_invoked"])
        self.assertFalse(states["ofx_bridge_path_payload_exported"])

    def test_candidate_ofx_host_harness_dryrun_spec_is_indexed(self):
        label = "candidate_ofx_host_harness_dryrun"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
            "harness_dryrun_ready": True,
            "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
            "dry_run_only": True,
            "would_execute": False,
            "execution_performed": False,
            "host_harness_kind": "ofx_noop_host_harness_planning",
            "planned_cases": [{"case_id": "noop_describe_contract"}],
            "allowed_harness_actions": ["plan_noop_describe_contract"],
            "planned_case_count": 2,
            "planned_noop_describe_case_count": 1,
            "planned_noop_render_case_count": 1,
            "planned_real_describe_case_count": 0,
            "planned_real_render_case_count": 0,
            "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
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
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_host_harness_dryrun",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["harness_dryrun_state"], "candidate_ofx_host_harness_dryrun_ready_route_closed")
        self.assertTrue(states["harness_dryrun_ready"])
        self.assertTrue(states["dry_run_only"])
        self.assertEqual(states["planned_case_count"], 2)
        self.assertEqual(states["source_bridge_allowed_route"], "no_op_identity_only")
        self.assertFalse(states["real_route_open"])
        self.assertFalse(states["ofx_runtime_invoked"])
        self.assertFalse(states["host_harness_path_payload_exported"])

    def test_candidate_ofx_host_harness_selftest_spec_is_indexed(self):
        label = "candidate_ofx_host_harness_selftest"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "host_harness_selftest_state": "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
            "host_harness_selftest_ready": True,
            "host_harness_kind": "ofx_noop_host_harness_synthetic_selftest",
            "selftest_state": "candidate_ofx_host_harness_selftest_passed_no_load",
            "selftest_ready": True,
            "synthetic_only": True,
            "synthetic_contract_checks_performed": True,
            "synthetic_contract_execution_performed": True,
            "real_execution_performed": False,
            "real_harness_execution_performed": False,
            "dry_run_consumed": True,
            "planned_cases_verified": True,
            "checked_case_count": 2,
            "checked_noop_describe_case_count": 1,
            "checked_noop_render_case_count": 1,
            "checked_real_describe_case_count": 0,
            "checked_real_render_case_count": 0,
            "case_result_count": 2,
            "case_passed_count": 2,
            "case_failed_count": 0,
            "descriptor_contract_checked": True,
            "render_identity_contract_checked": True,
            "synthetic_descriptor_created": True,
            "synthetic_render_contract_created": True,
            "case_results": [{"case_id": "noop_describe_contract", "case_status": "passed"}],
            "allowed_selftest_actions": ["validate_planned_cases_shape"],
            "source_harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
            "source_harness_dryrun_ready": True,
            "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
            "source_bridge_allowed_route": "no_op_identity_only",
            "candidate_mock_surface_reused": True,
            "candidate_mock_surface_reused_as_string_only": True,
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
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_host_harness_selftest",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["host_harness_selftest_state"],
            "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed",
        )
        self.assertTrue(states["host_harness_selftest_ready"])
        self.assertTrue(states["synthetic_only"])
        self.assertEqual(states["checked_case_count"], 2)
        self.assertEqual(states["case_passed_count"], 2)
        self.assertFalse(states["real_harness_execution_performed"])
        self.assertFalse(states["ppm_pixel_read_performed"])
        self.assertFalse(states["ofx_runtime_invoked"])
        self.assertFalse(states["host_harness_path_payload_exported"])

    def test_candidate_ofx_runtime_boundary_contract_spec_is_indexed(self):
        label = "candidate_ofx_runtime_boundary_contract"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "candidate_ofx_runtime_boundary_contract_state": (
                "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
            ),
            "candidate_ofx_runtime_boundary_state": "candidate_ofx_runtime_boundary_ready_no_load_route_closed",
            "contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
            "runtime_boundary_ready": True,
            "runtime_boundary_kind": "candidate_ofx_runtime_boundary_contract",
            "boundary_contract_ready": True,
            "runtime_boundary_plan": {"boundary_kind": "future_ofx_runtime_boundary"},
            "host_process_policy": {"state": "closed_no_host_process_launch"},
            "ofx_plugin_binary_policy": {"state": "closed_no_plugin_binary_path"},
            "required_before_runtime_invocation": ["explicit user approval"],
            "source_bridge_state": "candidate_ofx_bridge_ready_no_load_route_closed",
            "source_bridge_ready": True,
            "source_bridge_allowed_route": "no_op_identity_only",
            "source_harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
            "source_harness_dryrun_ready": True,
            "source_harness_dryrun_only": True,
            "source_host_harness_selftest_state": (
                "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed"
            ),
            "source_host_harness_selftest_ready": True,
            "source_harness_kind": "ofx_noop_host_harness_synthetic_selftest",
            "source_synthetic_only": True,
            "source_real_harness_execution_performed": False,
            "source_ppm_pixel_read_performed": False,
            "source_ofx_route_contract_state": "ofx_route_contract_ready_route_closed",
            "source_ofx_route_allowed_route": "no_op_identity_only",
            "source_ofx_route_real_route_open": False,
            "source_ofx_route_mock_route_ready": True,
            "source_native_runtime_contract_state": "runtime_containment_contract_ready_no_load",
            "source_native_runtime_contract_ready": True,
            "source_native_runtime_path_acceptance_ready": False,
            "source_native_runtime_process_isolation_required": True,
            "source_native_runtime_candidate_dependencies_clear": True,
            "source_fixture_approval_satisfied": False,
            "no_load_boundary_contract_created": True,
            "runtime_boundary_step_count": 6,
            "approval_gate_count": 6,
            "required_runtime_evidence_count": 6,
            "ofx_runtime_allowed_now": False,
            "ofx_runtime_invocation_ready": False,
            "ofx_runtime_instantiation_ready": False,
            "ofx_runtime_instantiation_performed": False,
            "ofx_binary_build_allowed_now": False,
            "ofx_binary_built": False,
            "real_ofx_describe_allowed_now": False,
            "real_ofx_render_allowed_now": False,
            "real_route_open": False,
            "real_ofx_route_ready": False,
            "mock_route_ready": True,
            "no_op_identity_route_preserved": True,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
            "ofx_describe_ready": False,
            "ofx_render_ready": False,
            "render_equivalence_claim_ready": False,
            "ppm_pixel_read_performed": False,
            "absolute_ppm_paths_exported": False,
            "absolute_aex_paths_exported": False,
            "runtime_boundary_path_payload_exported": False,
            "ofx_host_path_payload_supplied": False,
            "ofx_plugin_binary_path_payload_supplied": False,
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "host_process_launch_enabled": False,
            "requires_future_runtime_approval": True,
            "runtime_approval_required_before_invocation": True,
            "requires_future_fixture_approval": True,
            "requires_future_render_validation_approval": True,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_runtime_boundary_contract",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["candidate_ofx_runtime_boundary_contract_state"],
            "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed",
        )
        self.assertTrue(states["runtime_boundary_ready"])
        self.assertEqual(states["source_ofx_route_allowed_route"], "no_op_identity_only")
        self.assertFalse(states["ofx_runtime_invocation_ready"])
        self.assertFalse(states["host_process_launch_enabled"])
        self.assertFalse(states["runtime_boundary_path_payload_exported"])

    def test_candidate_ofx_runtime_approval_request_spec_is_indexed(self):
        label = "candidate_ofx_runtime_approval_request"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "runtime_approval_request_state": "candidate_ofx_runtime_approval_request_ready_pending_manual_approval",
            "runtime_approval_request_ready": True,
            "runtime_approval_request_created": True,
            "runtime_approval_can_be_issued_now": False,
            "runtime_approval_manifest_created": False,
            "runtime_approval_gate_stays_closed": True,
            "approval_request_kind": "ofx_runtime_invocation_manual_approval_request",
            "requires_explicit_user_approval": True,
            "required_approval_token_name": "APPROVE_OFX_RUNTIME_INVOCATION",
            "approval_token_not_stored_in_manifest": True,
            "approval_only_prepares_runtime_review": True,
            "source_boundary_contract_state": "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed",
            "source_boundary_contract_ready": True,
            "source_contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
            "source_ofx_runtime_allowed_now": False,
            "source_ofx_runtime_invocation_ready": False,
            "source_host_process_launch_enabled": False,
            "source_path_acceptance_ready": False,
            "source_real_route_open": False,
            "source_mock_route_ready": True,
            "source_ofx_runtime_invoked": False,
            "source_ppm_pixel_read_performed": False,
            "source_fixture_approval_satisfied": False,
            "source_requires_future_runtime_approval": True,
            "source_requires_future_fixture_approval": True,
            "source_requires_future_render_validation_approval": True,
            "review_checklist": [{"id": "explicit_runtime_approval"}],
            "review_checklist_count": 6,
            "approval_blockers": [{"id": "explicit_runtime_approval"}],
            "approval_blocker_count": 5,
            "blocked_actions_after_request": ["instantiate_ofx_runtime"],
            "ofx_runtime_allowed_now": False,
            "ofx_runtime_invocation_ready": False,
            "ofx_runtime_instantiation_performed": False,
            "host_process_launch_enabled": False,
            "ofx_host_path_payload_supplied": False,
            "ofx_plugin_binary_path_payload_supplied": False,
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "real_route_open": False,
            "real_ofx_route_ready": False,
            "mock_route_ready": True,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
            "ofx_describe_ready": False,
            "ofx_render_ready": False,
            "render_equivalence_claim_ready": False,
            "ppm_pixel_read_performed": False,
            "absolute_ppm_paths_exported": False,
            "absolute_aex_paths_exported": False,
            "runtime_approval_path_payload_exported": False,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_runtime_approval_request_packet",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["runtime_approval_request_state"],
            "candidate_ofx_runtime_approval_request_ready_pending_manual_approval",
        )
        self.assertTrue(states["runtime_approval_request_ready"])
        self.assertFalse(states["runtime_approval_can_be_issued_now"])
        self.assertFalse(states["runtime_approval_manifest_created"])
        self.assertEqual(states["required_approval_token_name"], "APPROVE_OFX_RUNTIME_INVOCATION")
        self.assertFalse(states["ofx_runtime_invocation_ready"])
        self.assertFalse(states["runtime_approval_path_payload_exported"])

    def test_candidate_ofx_runtime_approval_verifier_spec_is_indexed(self):
        label = "candidate_ofx_runtime_approval_verifier"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "runtime_approval_verifier_state": "candidate_ofx_runtime_approval_verifier_ready_no_approval",
            "runtime_approval_verifier_ready": True,
            "runtime_approval_verified_not_approved": True,
            "source_runtime_approval_request_state": (
                "candidate_ofx_runtime_approval_request_ready_pending_manual_approval"
            ),
            "source_runtime_approval_request_ready": True,
            "runtime_approval_request_state": "candidate_ofx_runtime_approval_request_ready_pending_manual_approval",
            "runtime_approval_request_ready": True,
            "runtime_approval_request_created": True,
            "runtime_approval_can_be_issued_now": False,
            "runtime_approval_manifest_created": False,
            "runtime_approval_gate_stays_closed": True,
            "current_runtime_approval_valid": False,
            "runtime_approval_satisfied": False,
            "runtime_approval_gate_closed": True,
            "boundary_contract_cross_checked": True,
            "boundary_contract_matches_request": True,
            "explicit_runtime_approval_present": False,
            "requires_explicit_user_approval": True,
            "required_approval_token_name": "APPROVE_OFX_RUNTIME_INVOCATION",
            "approval_token_not_stored_in_manifest": True,
            "approval_only_prepares_runtime_review": True,
            "review_checklist_count": 6,
            "approval_blocker_count": 5,
            "request_blockers_clear": False,
            "source_boundary_contract_state": "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed",
            "source_boundary_contract_ready": True,
            "source_contract_state": "candidate_ofx_runtime_boundary_contract_ready_runtime_closed",
            "source_ofx_runtime_allowed_now": False,
            "source_ofx_runtime_invocation_ready": False,
            "source_host_process_launch_enabled": False,
            "source_path_acceptance_ready": False,
            "source_real_route_open": False,
            "source_mock_route_ready": True,
            "source_ofx_runtime_invoked": False,
            "source_ppm_pixel_read_performed": False,
            "source_fixture_approval_satisfied": False,
            "source_requires_future_runtime_approval": True,
            "source_requires_future_fixture_approval": True,
            "source_requires_future_render_validation_approval": True,
            "runtime_boundary_ready": True,
            "fixture_approval_satisfied": False,
            "ofx_host_binary_review_ready": False,
            "runtime_containment_selftest_ready": False,
            "schema_and_render_validation_ready": False,
            "path_acceptance_closed": True,
            "current_runtime_approval_evaluation": {
                "valid": False,
                "reasons": ["manifest_kind_not_runtime_approval"],
            },
            "synthetic_runtime_approval_checks_passed": True,
            "synthetic_runtime_approval_checks": [{"case": "current_request_packet_rejected"}],
            "blocked_actions_after_verification": ["instantiate_ofx_runtime"],
            "ofx_runtime_allowed_now": False,
            "ofx_runtime_invocation_ready": False,
            "ofx_runtime_instantiation_ready": False,
            "ofx_runtime_instantiation_performed": False,
            "host_process_launch_enabled": False,
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "real_route_open": False,
            "mock_route_ready": True,
            "ofx_runtime_invoked": False,
            "ppm_pixel_read_performed": False,
            "runtime_approval_path_payload_exported": False,
            "runtime_approval_verifier_path_payload_exported": False,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_runtime_approval_verifier",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["runtime_approval_verifier_state"],
            "candidate_ofx_runtime_approval_verifier_ready_no_approval",
        )
        self.assertTrue(states["runtime_approval_verified_not_approved"])
        self.assertFalse(states["current_runtime_approval_valid"])
        self.assertFalse(states["runtime_approval_satisfied"])
        self.assertTrue(states["boundary_contract_cross_checked"])
        self.assertTrue(states["boundary_contract_matches_request"])
        self.assertFalse(states["explicit_runtime_approval_present"])
        self.assertFalse(states["runtime_approval_verifier_path_payload_exported"])

    def test_candidate_ofx_runtime_prerequisite_audit_spec_is_indexed(self):
        label = "candidate_ofx_runtime_prerequisite_audit"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "runtime_prerequisite_audit_state": "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed",
            "runtime_prerequisite_audit_ready": True,
            "runtime_prerequisites_complete": False,
            "runtime_prerequisites_all_satisfied": False,
            "runtime_invocation_prerequisites_ready": False,
            "approval_can_be_issued_now": False,
            "failed_evidence_count": 0,
            "blocking_prerequisite_count": 4,
            "runtime_prerequisite_count": 8,
            "runtime_prerequisite_satisfied_count": 4,
            "runtime_prerequisite_blocker_count": 4,
            "runtime_prerequisite_rows": [{"id": "runtime_approval_request_verified_not_approved"}],
            "runtime_prerequisite_blockers": [{"id": "explicit_runtime_approval"}],
            "prerequisite_audit_checks": [{"id": "runtime_approval_request_verified_not_approved"}],
            "prerequisite_gaps": [{"id": "explicit_runtime_approval"}],
            "audit_summary": {
                "runtime_prerequisite_count": 8,
                "blocking_prerequisite_count": 4,
                "failed_evidence_count": 0,
            },
            "runtime_approval_verified_not_approved": True,
            "current_runtime_approval_valid": False,
            "runtime_approval_satisfied": False,
            "runtime_approval_gate_stays_closed": True,
            "boundary_contract_cross_checked": True,
            "boundary_contract_matches_request": True,
            "request_blockers_clear": False,
            "explicit_runtime_approval_present": False,
            "fixture_approval_satisfied": False,
            "ofx_host_binary_review_ready": False,
            "runtime_containment_contract_ready": True,
            "runtime_containment_selftest_ready": False,
            "runtime_containment_selftest_synthetic_passed": True,
            "runtime_containment_selftest_passed": True,
            "runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
            "synthetic_subprocess_only": True,
            "normal_exit_case_passed": True,
            "stderr_capture_passed": True,
            "timeout_case_passed": True,
            "child_cleanup_passed": True,
            "parameter_schema_review_policy_ready": True,
            "parameter_schema_review_state": "parameter_schema_review_ready_no_payload",
            "payload_parser_enabled": False,
            "redacted_schema_available": False,
            "ofx_describe_mapping_ready": False,
            "schema_and_render_validation_ready": False,
            "render_validation_contract_ready": True,
            "render_validation_contract_state": "render_validation_contract_ready_render_closed",
            "no_load_validation_ready": True,
            "real_render_open": False,
            "ofx_route_contract_closed": True,
            "real_route_open": False,
            "mock_route_ready": True,
            "path_acceptance_closed": True,
            "ofx_runtime_invocation_ready": False,
            "host_process_launch_enabled": False,
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "ofx_runtime_invoked": False,
            "ppm_pixel_read_performed": False,
            "runtime_prerequisite_audit_path_payload_exported": False,
            "blocked_actions_after_audit": ["instantiate_ofx_runtime"],
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
            "aex_file_hashed": False,
            "aex_file_copied": False,
            "pipl_payload_parsed": False,
            "resource_payload_opened": False,
            "raw_payload_serialized": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_runtime_prerequisite_audit",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["runtime_prerequisite_audit_state"],
            "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed",
        )
        self.assertTrue(states["runtime_prerequisite_audit_ready"])
        self.assertFalse(states["runtime_invocation_prerequisites_ready"])
        self.assertEqual(states["failed_evidence_count"], 0)
        self.assertEqual(states["blocking_prerequisite_count"], 4)
        self.assertTrue(states["runtime_containment_selftest_synthetic_passed"])
        self.assertFalse(states["runtime_containment_selftest_ready"])
        self.assertTrue(states["render_validation_contract_ready"])
        self.assertFalse(states["runtime_prerequisite_audit_path_payload_exported"])

    def test_candidate_ofx_host_binary_review_request_spec_is_indexed(self):
        label = "candidate_ofx_host_binary_review_request"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "review_request_kind": "ofx_host_binary_provenance_manual_review_request",
            "ofx_host_binary_review_request_state": (
                "candidate_ofx_host_binary_review_request_ready_pending_manual_review"
            ),
            "ofx_host_binary_review_request_ready": True,
            "ofx_host_binary_review_ready": False,
            "ofx_host_binary_review_satisfied": False,
            "ofx_host_binary_review_can_be_approved_now": False,
            "ofx_host_binary_review_manifest_created": False,
            "ofx_host_binary_review_gate_stays_closed": True,
            "host_binary_review_request_state": (
                "candidate_ofx_host_binary_review_request_ready_pending_manual_review"
            ),
            "host_binary_review_request_ready": True,
            "host_binary_review_request_created": True,
            "host_binary_review_can_be_approved_now": False,
            "host_binary_review_ready": False,
            "host_binary_review_satisfied": False,
            "host_binary_review_manifest_created": False,
            "host_binary_review_gate_stays_closed": True,
            "requires_explicit_host_binary_review": True,
            "host_binary_path_acceptance_ready": False,
            "host_binary_path_payload_exported": False,
            "source_prerequisite_audit_state": (
                "candidate_ofx_runtime_prerequisite_audit_ready_gates_closed"
            ),
            "source_failed_evidence_count": 0,
            "source_blocking_prerequisite_count": 4,
            "source_runtime_invocation_prerequisites_ready": False,
            "source_ofx_host_binary_review_ready": False,
            "source_harness_dryrun_state": "candidate_ofx_host_harness_dryrun_ready_route_closed",
            "source_harness_dryrun_ready": True,
            "source_host_harness_selftest_state": (
                "candidate_ofx_host_harness_selftest_passed_synthetic_route_closed"
            ),
            "source_host_harness_selftest_ready": True,
            "source_boundary_contract_state": (
                "candidate_ofx_runtime_boundary_contract_ready_no_runtime_route_closed"
            ),
            "source_boundary_contract_ready": True,
            "source_boundary_host_process_launch_enabled": False,
            "source_boundary_ofx_host_path_payload_supplied": False,
            "source_boundary_ofx_plugin_binary_path_payload_supplied": False,
            "review_checklist": [{"id": "host_binary_identity_required"}],
            "review_checklist_count": 8,
            "review_blockers": [{"id": "host_binary_identity_required"}],
            "review_blocker_count": 7,
            "host_binary_review_blockers": [{"id": "host_binary_identity_required"}],
            "host_binary_review_blocker_count": 7,
            "ready_no_load_evidence_count": 4,
            "source_no_load_evidence_count": 4,
            "manual_review_required": True,
            "explicit_user_review_required": True,
            "approval_only_prepares_host_binary_review": True,
            "review_does_not_accept_paths": True,
            "review_does_not_launch_host": True,
            "review_does_not_invoke_runtime": True,
            "runtime_invocation_prerequisites_ready": False,
            "ofx_runtime_allowed_now": False,
            "ofx_runtime_invocation_ready": False,
            "host_process_launch_enabled": False,
            "path_acceptance_ready": False,
            "accepted_aex_path": None,
            "accepted_ofx_host_path": None,
            "accepted_ofx_plugin_binary_path": None,
            "real_route_open": False,
            "mock_route_ready": True,
            "ofx_runtime_invoked": False,
            "ppm_pixel_read_performed": False,
            "blocked_actions_after_request": ["accept_ofx_host_path"],
            "ofx_host_binary_review_path_payload_exported": False,
            "host_binary_review_path_payload_exported": False,
            "ofx_host_binary_opened": False,
            "ofx_host_binary_hashed": False,
            "ofx_host_binary_copied": False,
            "ofx_host_binary_executed": False,
            "ofx_plugin_binary_opened": False,
            "ofx_plugin_binary_hashed": False,
            "ofx_plugin_binary_copied": False,
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_candidate_ofx_host_binary_review_request",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["ofx_host_binary_review_request_state"],
            "candidate_ofx_host_binary_review_request_ready_pending_manual_review",
        )
        self.assertTrue(states["host_binary_review_request_ready"])
        self.assertFalse(states["ofx_host_binary_review_ready"])
        self.assertFalse(states["host_binary_review_can_be_approved_now"])
        self.assertEqual(states["review_checklist_count"], 8)
        self.assertEqual(states["host_binary_review_blocker_count"], 7)
        self.assertFalse(states["host_binary_path_acceptance_ready"])
        self.assertFalse(states["ofx_runtime_invoked"])
        self.assertFalse(states["host_binary_review_path_payload_exported"])
        safety = aex_artifact_index.safety_summary(complete)
        self.assertFalse(safety["ofx_host_binary_opened"])
        self.assertFalse(safety["ofx_plugin_binary_hashed"])

    def test_candidate_load_gate_dryrun_spec_is_indexed(self):
        label = "candidate_load_gate_dryrun"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "candidate_load_gate_dryrun_state": "candidate_load_gate_dryrun_ready_no_load",
            "candidate_scoped_load_gate_dry_run_state": (
                "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
            ),
            "gate_state": "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked",
            "candidate_gate_ready_for_separate_loader_design": False,
            "native_load_gate": "closed",
            "fixture_approval_satisfied": False,
            "fixture_decision_manifest_kind": "aex_fixture_decision_manifest",
            "fixture_decision_state": "hold_for_manual_review",
            "fixture_approval_state": "not_approved_for_load_gate",
            "candidate_scope_ready": True,
            "candidate_dependencies_clear": True,
            "candidate_dependency_blockers_present": False,
            "candidate_dependency_blocker_count": 0,
            "candidate_dependency_review_count": 0,
            "candidate_dependency_missing_or_api_set_review_count": 0,
            "candidate_dependency_found_paths_exported": False,
            "global_dependency_blockers_present": True,
            "global_dependency_blockers_apply_to_candidate": False,
            "source_load_gate_state": "closed_dependency_review_or_invalid_approval",
            "source_load_gate_dependency_recommendation": "do_not_open_native_load_gate",
            "scoped_gate_recommendation": "candidate_dependencies_clear_global_gate_still_closed",
            "gates": [],
        }
        self.assertEqual(aex_artifact_index.ARTIFACT_SPECS[label]["kind"], "aex_candidate_load_gate_dryrun")
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["candidate_load_gate_dryrun_state"], "candidate_load_gate_dryrun_ready_no_load")
        self.assertTrue(states["candidate_dependencies_clear"])
        self.assertFalse(states["fixture_approval_satisfied"])
        self.assertEqual(states["fixture_decision_state"], "hold_for_manual_review")
        self.assertEqual(states["native_load_gate"], "closed")

    def test_native_loader_design_contract_spec_is_indexed(self):
        label = "native_loader_design_contract"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "native_loader_design_state": "native_loader_design_ready_loader_closed",
            "contract_state": "native_loader_design_contract_ready_loader_closed_pending_fixture_approval",
            "loader_design_ready": True,
            "native_load_gate": "closed",
            "approval_required_before_aex_path": True,
            "runtime_approval_required_before_load": True,
            "separate_process_required": True,
            "accepts_aex_path": False,
            "accepted_aex_path": None,
            "controller_loads_aex": False,
            "candidate_dependencies_clear": True,
            "fixture_approval_satisfied": False,
            "candidate_load_gate_state": "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked",
            "source_candidate_load_gate_state": (
                "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
            ),
            "source_stub_state": "refused_gate_closed",
            "source_sandbox_policy_state": "policy_ready_no_native_load",
            "source_render_contract_state": "render_validation_contract_ready_render_closed",
            "source_ofx_route_contract_state": "ofx_route_contract_ready_route_closed",
            "loader_contract": {"state": "loader_contract_defined_acceptance_closed"},
            "approval_contract": {"approval_does_not_permit_native_load": True},
            "dependency_gate_contract": {
                "required_dependency_review_endpoint": "manual_loader_design_review_only_no_auto_approval"
            },
            "phase_plan": [],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_native_loader_design_contract",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["native_loader_design_state"], "native_loader_design_ready_loader_closed")
        self.assertTrue(states["loader_design_ready"])
        self.assertFalse(states["accepts_aex_path"])
        self.assertIsNone(states["accepted_aex_path"])
        self.assertFalse(states["controller_loads_aex"])

    def test_native_loader_broker_selftest_spec_is_indexed(self):
        label = "native_loader_broker_selftest"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "broker_selftest_state": "pathless_native_loader_broker_selftest_passed",
            "pathless_broker_ready": True,
            "native_loader_design_ready": True,
            "accepts_aex_path": False,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "blocked_action_count": 6,
            "candidate_dependencies_clear": True,
            "fixture_approval_satisfied": False,
            "blocked_action_checks": [
                {"message_type": "accept_aex_path", "code": "blocked_action", "path_payload_supplied": False}
            ],
            "steps": [{"step": "hello"}, {"step": "inspect_environment"}],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_native_loader_broker_selftest",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["broker_selftest_state"], "pathless_native_loader_broker_selftest_passed")
        self.assertTrue(states["pathless_broker_ready"])
        self.assertFalse(states["accepts_aex_path"])
        self.assertIsNone(states["accepted_aex_path"])
        self.assertFalse(states["path_payload_supplied"])
        self.assertEqual(states["blocked_action_count"], 6)

    def test_native_loader_runtime_contract_spec_is_indexed(self):
        label = "native_loader_runtime_contract"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
            "contract_state": "runtime_containment_contract_ready_path_acceptance_closed",
            "runtime_containment_ready": True,
            "path_allowlist_state": "closed_no_aex_paths_accepted",
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "approval_required_before_aex_path": True,
            "runtime_approval_required_before_load": True,
            "native_load_gate": "closed",
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "broker_selftest_passed": True,
            "pathless_broker_ready": True,
            "native_loader_design_ready": True,
            "process_isolation_required": True,
            "controller_loads_aex": False,
            "candidate_dependencies_clear": True,
            "fixture_approval_satisfied": False,
            "source_native_loader_design_state": "native_loader_design_ready_loader_closed",
            "source_broker_selftest_state": "pathless_native_loader_broker_selftest_passed",
            "source_sandbox_policy_state": "policy_ready_no_native_load",
            "source_candidate_load_gate_state": "candidate_load_gate_dryrun_ready_no_load",
            "source_candidate_scoped_load_gate_state": (
                "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked"
            ),
            "path_policy": {"state": "closed_no_aex_paths_accepted"},
            "timeout_policy": {"requires_review_before_first_load": True},
            "blocked_actions": ["accept_aex_path", "load_aex_dll"],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_native_loader_runtime_contract",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["native_loader_runtime_contract_state"],
            "runtime_containment_contract_ready_no_load",
        )
        self.assertTrue(states["runtime_containment_ready"])
        self.assertEqual(states["path_allowlist_state"], "closed_no_aex_paths_accepted")
        self.assertFalse(states["path_acceptance_ready"])
        self.assertFalse(states["aex_path_acceptance_enabled"])
        self.assertTrue(states["broker_selftest_passed"])
        self.assertTrue(states["process_isolation_required"])

    def test_native_loader_runtime_selftest_spec_is_indexed(self):
        label = "native_loader_runtime_selftest"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
            "runtime_containment_selftest_passed": True,
            "runtime_containment_ready": True,
            "path_allowlist_state": "closed_no_aex_paths_accepted",
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "source_native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
            "synthetic_subprocess_only": True,
            "normal_exit_case_passed": True,
            "stderr_capture_passed": True,
            "timeout_case_passed": True,
            "child_cleanup_passed": True,
            "child_cases": [{"case": "normal_exit"}, {"case": "timeout_termination"}],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_native_loader_runtime_selftest",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(states["runtime_selftest_state"], "runtime_containment_selftest_passed_no_load")
        self.assertTrue(states["runtime_containment_selftest_passed"])
        self.assertTrue(states["synthetic_subprocess_only"])
        self.assertTrue(states["normal_exit_case_passed"])
        self.assertTrue(states["stderr_capture_passed"])
        self.assertTrue(states["timeout_case_passed"])
        self.assertTrue(states["child_cleanup_passed"])
        self.assertFalse(states["path_acceptance_ready"])

    def test_native_loader_path_policy_selftest_spec_is_indexed(self):
        label = "native_loader_path_policy_selftest"
        sparse = payload_for(label)
        complete = {
            **sparse,
            "path_policy_selftest_state": "closed_path_policy_selftest_passed_no_aex_path",
            "path_policy_selftest_passed": True,
            "source_runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
            "source_runtime_selftest_passed": True,
            "path_allowlist_state": "closed_no_aex_paths_accepted",
            "path_acceptance_ready": False,
            "aex_path_acceptance_enabled": False,
            "accepted_aex_path": None,
            "path_payload_supplied": False,
            "synthetic_path_inputs_only": True,
            "candidate_path_string_accepted": False,
            "absolute_path_rejected": True,
            "traversal_rejected": True,
            "non_aex_suffix_rejected": True,
            "redaction_passed": True,
            "raw_input_paths_serialized": False,
            "path_case_count": 4,
            "path_cases": [{"case": "candidate_like_aex_path_stays_closed", "accepted": False}],
        }
        self.assertEqual(
            aex_artifact_index.ARTIFACT_SPECS[label]["kind"],
            "aex_native_loader_path_policy_selftest",
        )
        self.assertGreater(
            aex_artifact_index.artifact_completeness_score(label, complete),
            aex_artifact_index.artifact_completeness_score(label, sparse),
        )
        states = aex_artifact_index.artifact_state(complete)
        self.assertEqual(
            states["path_policy_selftest_state"],
            "closed_path_policy_selftest_passed_no_aex_path",
        )
        self.assertTrue(states["path_policy_selftest_passed"])
        self.assertFalse(states["candidate_path_string_accepted"])
        self.assertTrue(states["absolute_path_rejected"])
        self.assertTrue(states["traversal_rejected"])
        self.assertTrue(states["non_aex_suffix_rejected"])
        self.assertTrue(states["redaction_passed"])
        self.assertFalse(states["raw_input_paths_serialized"])

    def test_index_errors_catches_open_runtime_flags_and_publishable_boundary(self):
        items = [
            {
                "label": "ofx_noop_mock",
                "found": True,
                "safety_flags": {"ofx_route_invoked": True},
                "states": {},
            },
            {
                "label": "safety_audit",
                "found": True,
                "safety_flags": {},
                "states": {"audit_passed": True},
            },
            {
                "label": "publication_boundary",
                "found": True,
                "safety_flags": {},
                "states": {"publishable_now": True},
            },
        ]
        errors = aex_artifact_index.index_errors(items)
        self.assertIn("ofx_noop_mock ofx_route_invoked must be false", errors)
        self.assertIn("publication_boundary publishable_now must be false", errors)

    def test_output_is_create_new_under_index_root(self):
        payload = {
            "schema_version": 1,
            "report_kind": "aex_artifact_index",
            "native_load_performed": False,
        }
        out = LAB_ROOT / "target" / "artifact-index" / f"{time.time_ns()}-index.local.json"
        written = aex_artifact_index.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_artifact_index.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_artifact_index.write_json_create_new(LAB_ROOT / "target" / "outside-index.json", payload)


if __name__ == "__main__":
    unittest.main()
