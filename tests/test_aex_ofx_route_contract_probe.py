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


aex_ofx_route_contract_probe = load_tool("aex_ofx_route_contract_probe")


def make_facade() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_ofx_facade_deferred_packet",
        "facade_state": "deferred_loader_not_ready",
        "ofx_route_action": "no_op",
        "mapping_plan": {"state": "planning_only"},
        "blocked_actions": [
            "load_aex_dll",
            "call_EffectMain",
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "route_through_ofx",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
    }


def make_ofx_suite() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_suite_noop_selftest",
        "source_facade_state": "deferred_loader_not_ready",
        "ofx_suite_selftest_state": "ofx_suite_noop_identity_passed_route_closed",
        "fixture_count": 2,
        "fixture_results": [
            {
                "case_id": "gradient",
                "mock_state": "mock_identity_completed_route_closed",
                "identity_check": {"pixel_match": True, "dimension_match": True},
            },
            {
                "case_id": "checker",
                "mock_state": "mock_identity_completed_route_closed",
                "identity_check": {"pixel_match": True, "dimension_match": True},
            },
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
    }


def make_image_validation() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_validation",
        "validation_state": "image_fixture_validation_passed_no_load",
        "validation_passed": True,
        "fixture_results": [{"case_id": "gradient"}, {"case_id": "checker"}],
        "summary": {
            "fixture_count": 2,
            "passed_count": 2,
            "failed_count": 0,
            "total_pixel_bytes": 93,
            "pattern_counts": {"checker": 1, "gradient": 1},
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_load_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_load_gate_check",
        "gate_state": "closed_dependency_review_or_invalid_approval",
        "dependency_native_load_recommendation": "do_not_open_native_load_gate",
        "gates": [{"gate": "G5_native_load_gate", "status": "closed"}],
        "blocked_actions": [
            "load_aex_dll",
            "call_EffectMain",
            "start_after_effects",
            "render_with_aex",
            "route_through_ofx",
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_dependency_review() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_dependency_review_packet",
        "review_state": "dependency_review_pending_native_load_blocked",
        "native_load_recommendation": "do_not_open_native_load_gate",
        "review_items": [{"dll_name": "ucrtbased.dll", "review_state": "native_load_blocker"}],
        "summary": {"native_load_blocker_count": 1},
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def build_report(**overrides):
    values = {
        "facade": make_facade(),
        "ofx_suite": make_ofx_suite(),
        "image_validation": make_image_validation(),
        "load_gate": make_load_gate(),
        "dependency_review": make_dependency_review(),
    }
    values.update(overrides)
    return aex_ofx_route_contract_probe.build_contract_report(
        facade=values["facade"],
        facade_path=Path("facade.json"),
        ofx_suite=values["ofx_suite"],
        ofx_suite_path=Path("suite.json"),
        image_validation=values["image_validation"],
        image_validation_path=Path("validation.json"),
        load_gate=values["load_gate"],
        load_gate_path=Path("gate.json"),
        dependency_review=values["dependency_review"],
        dependency_review_path=Path("review.json"),
    )


class AexOfxRouteContractProbeTests(unittest.TestCase):
    def test_contract_ready_keeps_real_route_closed(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_ofx_route_contract_probe")
        self.assertEqual(report["contract_state"], "ofx_route_contract_ready_route_closed")
        self.assertFalse(report["real_route_open"])
        self.assertTrue(report["mock_route_ready"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["ofx_plugin_built"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertFalse(report["ofx_render_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertEqual(report["route_contract"]["allowed_route"], "no_op_identity_only")
        self.assertEqual(report["describe_contract"]["state"], "blocked_pending_native_loader_and_schema")
        self.assertEqual(report["render_contract"]["state"], "blocked_pending_render_harness")
        self.assertEqual(report["image_contract"]["state"], "validated_noop_identity_inputs")
        self.assertIn("load_gate_closed", report["blockers"])
        self.assertIn("dependency_review_blocks_native_load", report["blockers"])

    def test_invalid_source_state_or_safety_flag_is_rejected(self):
        facade = make_facade()
        facade["ofx_route_invoked"] = True
        with self.assertRaises(ValueError):
            build_report(facade=facade)

        suite = make_ofx_suite()
        suite["fixture_results"][1]["identity_check"]["pixel_match"] = False
        with self.assertRaises(ValueError):
            build_report(ofx_suite=suite)

        gate = make_load_gate()
        gate["gate_state"] = "preconditions_satisfied_no_load_performed"
        with self.assertRaises(ValueError):
            build_report(load_gate=gate)

    def test_cross_artifact_fixture_count_must_match(self):
        validation = make_image_validation()
        validation["summary"]["fixture_count"] = 3
        with self.assertRaises(ValueError):
            build_report(image_validation=validation)

    def test_paths_are_confined_and_report_is_create_new(self):
        roots = {
            "facade": LAB_ROOT / "target" / "ofx-facade",
            "suite": LAB_ROOT / "target" / "ofx-suite-selftest",
            "validation": LAB_ROOT / "target" / "image-fixture-validation",
            "gate": LAB_ROOT / "target" / "load-gate",
            "review": LAB_ROOT / "target" / "dependency-review",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)

        stamp = f"{time.time_ns()}-{os.getpid()}"
        paths = {
            "facade": roots["facade"] / f"{stamp}-contract-facade.local.json",
            "suite": roots["suite"] / f"{stamp}-contract-suite.local.json",
            "validation": roots["validation"] / f"{stamp}-contract-validation.local.json",
            "gate": roots["gate"] / f"{stamp}-contract-gate.local.json",
            "review": roots["review"] / f"{stamp}-contract-review.local.json",
        }
        payloads = {
            "facade": make_facade(),
            "suite": make_ofx_suite(),
            "validation": make_image_validation(),
            "gate": make_load_gate(),
            "review": make_dependency_review(),
        }
        for label, path in paths.items():
            path.write_text(json.dumps(payloads[label]), encoding="utf-8")

        facade, facade_path = aex_ofx_route_contract_probe.load_facade(paths["facade"])
        suite, suite_path = aex_ofx_route_contract_probe.load_ofx_suite(paths["suite"])
        validation, validation_path = aex_ofx_route_contract_probe.load_image_validation(paths["validation"])
        gate, gate_path = aex_ofx_route_contract_probe.load_load_gate(paths["gate"])
        review, review_path = aex_ofx_route_contract_probe.load_dependency_review(paths["review"])
        self.assertEqual(facade_path, paths["facade"].resolve())
        self.assertEqual(suite_path, paths["suite"].resolve())
        self.assertEqual(validation_path, paths["validation"].resolve())
        self.assertEqual(gate_path, paths["gate"].resolve())
        self.assertEqual(review_path, paths["review"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-contract.json"
        outside.write_text(json.dumps(make_facade()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_ofx_route_contract_probe.load_facade(outside)

        report = aex_ofx_route_contract_probe.build_contract_report(
            facade=facade,
            facade_path=facade_path,
            ofx_suite=suite,
            ofx_suite_path=suite_path,
            image_validation=validation,
            image_validation_path=validation_path,
            load_gate=gate,
            load_gate_path=gate_path,
            dependency_review=review,
            dependency_review_path=review_path,
        )
        out = LAB_ROOT / "target" / "ofx-route-contract" / f"{time.time_ns()}-{os.getpid()}-contract.local.json"
        written = aex_ofx_route_contract_probe.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_ofx_route_contract_probe.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_ofx_route_contract_probe.write_json_create_new(
                LAB_ROOT / "target" / "outside-contract.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
