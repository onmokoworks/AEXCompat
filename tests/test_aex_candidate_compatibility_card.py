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


aex_candidate_compatibility_card = load_tool("aex_candidate_compatibility_card")


def safety_flags() -> dict:
    return {
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
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
    }


def candidate_row() -> dict:
    return {
        "relative_path": "AEPluginBuild\\ScatterMap.aex",
        "file_name": "ScatterMap.aex",
        "size_bytes": 201216,
        "mtime_utc": "2026-03-26T18:34:27+00:00",
        "review_bucket": "primary_fixture_candidate",
        "suggested_next_action": "manual provenance/license review before any approval artifact",
        "risk_flags": [],
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "fixture_candidate_reasons": ["pe-valid", "x64", "dll-image", "pipl-signal"],
        "machine_label": "x64",
        "dll_image": True,
        "pipl_signal_present": True,
        "pipl_resource_data_entry_count": 1,
        "pipl_resource_total_size": 314,
        "pipl_resource_entries": [{"name": 16000, "language": 1033, "size_bytes": 314}],
        "resource_types": ["#16", "PIPL"],
        "effect_main_export_present": True,
        "effect_main_marker_present": True,
        "aegp_marker_count": 0,
        "import_dll_names": ["KERNEL32.dll", "VCRUNTIME140.dll"],
    }


def candidate_matrix_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_matrix",
        "matrix_state": "candidate_matrix_ready",
        "rows": [candidate_row()],
        "summary": {"candidate_count": 1},
        **safety_flags(),
    }


def pipl_catalog_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_resource_catalog",
        "catalog_state": "pipl_resource_catalog_ready_no_payload",
        "payload_policy": "metadata_only_no_resource_payload",
        "rows": [
            {
                "relative_path": "AEPluginBuild\\ScatterMap.aex",
                "file_name": "ScatterMap.aex",
                "size_bytes": 201216,
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 95,
                "metadata_state": "pipl_resource_metadata_ready_no_payload",
                "machine_label": "x64",
                "resource_type_details": [{"type": "#16", "entry_count": 1}, {"type": "PIPL", "entry_count": 1}],
                "pipl_signal_present": True,
                "pipl_resource_type_present": True,
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 314,
                "pipl_resource_entries": [
                    {"type": "PIPL", "name": 16000, "language": 1033, "size_bytes": 314, "reserved": 0}
                ],
                "effect_main_export_present": True,
                "effect_main_marker_present": True,
                "aegp_marker_count": 0,
                "imported_dll_count": 2,
                "payload_policy": "metadata_only_no_resource_payload",
            }
        ],
        "summary": {"plugin_count": 1},
        **safety_flags(),
    }


def fixture_result(case_id: str) -> dict:
    return {
        "case_id": case_id,
        "pattern": "gradient",
        "input_ppm": "D:\\secret\\input.ppm",
        "output_ppm": "D:\\secret\\output.ppm",
        "inspect": {"width": 16, "height": 12, "bytes": 576},
        "identity_check": {"width": 16, "height": 12, "bytes": 576, "pixel_match": True, "dimension_match": True},
        "mock_state": "mock_identity_completed_route_closed",
    }


def candidate_runner_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_no_load_test_runner",
        "runner_state": "candidate_no_load_test_runner_passed_native_closed",
        "runner_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "no_load_execution_performed": True,
        "native_execution_performed": False,
        "real_render_execution_performed": False,
        "real_ofx_route_execution_performed": False,
        "worker_invoked": True,
        "ofx_mock_invoked": True,
        "ofx_runtime_invoked": False,
        "worker_identity_passed": True,
        "ofx_noop_identity_passed": True,
        "image_fixture_validation_passed": True,
        "image_smoke_identity_passed": True,
        "render_contract_review_ready": True,
        "ofx_route_contract_review_ready": True,
        "blocked_load_aex_verified": True,
        "image_fixture_case_count": 1,
        "executed_worker_case_count": 1,
        "executed_ofx_noop_case_count": 1,
        "planned_no_load_case_count": 2,
        "blocked_case_count": 1,
        "worker_report": {"fixture_results": [fixture_result("gradient_small")]},
        "ofx_noop_report": {"fixture_results": [fixture_result("gradient_small")]},
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "real_render_open": False,
        "real_route_open": False,
        **safety_flags(),
    }


def answer_validator_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_provenance_answer_validator_selftest",
        "validator_selftest_state": "fixture_provenance_answer_validator_selftest_passed_no_user_answers",
        "validator_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "real_user_answer_artifact_consumed": False,
        "synthetic_payloads_serialized": False,
        "answer_schema_validated": True,
        "answers_present": False,
        "answers_validated_for_manual_review": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "accepted_aex_path": None,
        "synthetic_case_count": 8,
        "synthetic_case_passed_count": 8,
        "synthetic_case_failed_count": 0,
        **safety_flags(),
    }


class AexCandidateCompatibilityCardTests(unittest.TestCase):
    def test_build_card_combines_metadata_and_no_load_results_without_paths(self):
        card = aex_candidate_compatibility_card.build_candidate_compatibility_card(
            candidate_matrix=candidate_matrix_payload(),
            candidate_matrix_path=Path("target/candidate-matrix/matrix.json"),
            pipl_catalog=pipl_catalog_payload(),
            pipl_catalog_path=Path("target/pipl-resource-catalog/catalog.json"),
            candidate_runner=candidate_runner_payload(),
            candidate_runner_path=Path("target/candidate-test-runner/runner.json"),
            answer_validator=answer_validator_payload(),
            answer_validator_path=Path("target/fixture-provenance-answer-validator-selftest/validator.json"),
        )
        self.assertEqual(card["report_kind"], "aex_candidate_compatibility_card")
        self.assertEqual(card["compatibility_card_state"], "candidate_compatibility_card_ready_no_load")
        self.assertTrue(card["compatibility_card_ready"])
        self.assertEqual(card["candidate_relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertEqual(card["candidate_metadata"]["file_name"], "ScatterMap.aex")
        self.assertEqual(card["candidate_metadata"]["pipl_metadata"]["payload_policy"], "metadata_only_no_resource_payload")
        self.assertTrue(card["no_load_test_card"]["worker_identity_passed"])
        self.assertTrue(card["no_load_test_card"]["ofx_noop_identity_passed"])
        self.assertFalse(card["no_load_test_card"]["ppm_paths_exported"])
        self.assertFalse(card["absolute_ppm_paths_exported"])
        self.assertFalse(card["absolute_aex_paths_exported"])
        worker_case = card["no_load_test_card"]["worker_fixture_results"][0]
        self.assertNotIn("input_ppm", worker_case)
        self.assertNotIn("output_ppm", worker_case)
        self.assertFalse(worker_case["ppm_paths_exported"])
        self.assertFalse(card["approval_can_be_issued_now"])
        self.assertFalse(card["fixture_approval_satisfied"])
        self.assertEqual(card["native_load_gate"], "closed")
        self.assertFalse(card["aex_file_opened"])
        self.assertFalse(card["aex_file_hashed"])
        self.assertFalse(card["aex_file_copied"])
        self.assertFalse(card["resource_payload_opened"])
        self.assertFalse(card["resource_payload_extracted"])
        self.assertFalse(card["pipl_payload_parsed"])

    def test_rejects_candidate_mismatch(self):
        validator = answer_validator_payload()
        validator["candidate_relative_path"] = "Other\\Different.aex"
        with self.assertRaises(ValueError) as context:
            aex_candidate_compatibility_card.build_candidate_compatibility_card(
                candidate_matrix=candidate_matrix_payload(),
                candidate_matrix_path=Path("target/candidate-matrix/matrix.json"),
                pipl_catalog=pipl_catalog_payload(),
                pipl_catalog_path=Path("target/pipl-resource-catalog/catalog.json"),
                candidate_runner=candidate_runner_payload(),
                candidate_runner_path=Path("target/candidate-test-runner/runner.json"),
                answer_validator=validator,
                answer_validator_path=Path("target/fixture-provenance-answer-validator-selftest/validator.json"),
            )
        self.assertIn("candidate_relative_path must match", str(context.exception))

    def test_rejects_open_real_route_or_payload_parse(self):
        runner = candidate_runner_payload()
        runner["real_route_open"] = True
        with self.assertRaises(ValueError) as context:
            aex_candidate_compatibility_card.build_candidate_compatibility_card(
                candidate_matrix=candidate_matrix_payload(),
                candidate_matrix_path=Path("target/candidate-matrix/matrix.json"),
                pipl_catalog=pipl_catalog_payload(),
                pipl_catalog_path=Path("target/pipl-resource-catalog/catalog.json"),
                candidate_runner=runner,
                candidate_runner_path=Path("target/candidate-test-runner/runner.json"),
                answer_validator=answer_validator_payload(),
                answer_validator_path=Path("target/fixture-provenance-answer-validator-selftest/validator.json"),
            )
        self.assertIn("real_route_open must be false", str(context.exception))

        catalog = pipl_catalog_payload()
        catalog["resource_payload_extracted"] = True
        with self.assertRaises(ValueError) as context:
            aex_candidate_compatibility_card.build_candidate_compatibility_card(
                candidate_matrix=candidate_matrix_payload(),
                candidate_matrix_path=Path("target/candidate-matrix/matrix.json"),
                pipl_catalog=catalog,
                pipl_catalog_path=Path("target/pipl-resource-catalog/catalog.json"),
                candidate_runner=candidate_runner_payload(),
                candidate_runner_path=Path("target/candidate-test-runner/runner.json"),
                answer_validator=answer_validator_payload(),
                answer_validator_path=Path("target/fixture-provenance-answer-validator-selftest/validator.json"),
            )
        self.assertIn("resource_payload_extracted must be false", str(context.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "candidate-matrix": candidate_matrix_payload(),
            "pipl-resource-catalog": pipl_catalog_payload(),
            "candidate-test-runner": candidate_runner_payload(),
            "fixture-provenance-answer-validator-selftest": answer_validator_payload(),
        }
        stamp = time.time_ns()
        paths = {}
        for root_name, payload in roots.items():
            root = LAB_ROOT / "target" / root_name
            root.mkdir(parents=True, exist_ok=True)
            path = root / f"{stamp}-{root_name}.local.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            paths[root_name] = path

        matrix, matrix_path = aex_candidate_compatibility_card.load_candidate_matrix(paths["candidate-matrix"])
        catalog, catalog_path = aex_candidate_compatibility_card.load_pipl_catalog(paths["pipl-resource-catalog"])
        runner, runner_path = aex_candidate_compatibility_card.load_candidate_runner(paths["candidate-test-runner"])
        validator, validator_path = aex_candidate_compatibility_card.load_answer_validator(
            paths["fixture-provenance-answer-validator-selftest"]
        )
        card = aex_candidate_compatibility_card.build_candidate_compatibility_card(
            candidate_matrix=matrix,
            candidate_matrix_path=matrix_path,
            pipl_catalog=catalog,
            pipl_catalog_path=catalog_path,
            candidate_runner=runner,
            candidate_runner_path=runner_path,
            answer_validator=validator,
            answer_validator_path=validator_path,
        )
        out = LAB_ROOT / "target" / "candidate-compat-card" / f"{stamp}-card.local.json"
        written = aex_candidate_compatibility_card.write_json_create_new(out, card)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_compatibility_card.write_json_create_new(out, card)
        with self.assertRaises(ValueError):
            aex_candidate_compatibility_card.write_json_create_new(
                LAB_ROOT / "target" / "outside-card.local.json",
                card,
            )


if __name__ == "__main__":
    unittest.main()
