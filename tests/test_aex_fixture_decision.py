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


aex_fixture_decision = load_tool("aex_fixture_decision")
aex_load_gate_check = load_tool("aex_load_gate_check")


def make_candidate(relative_path: str = "AEPluginBuild\\ScatterMap.aex") -> dict:
    return {
        "relative_path": relative_path,
        "file_name": Path(relative_path).name,
        "size_bytes": 201216,
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "machine_label": "x64",
        "pipl_signal_present": True,
        "effect_main_export_present": True,
        "effect_main_marker_present": True,
        "aegp_marker_count": 0,
        "resource_types": ["PIPL", "#16"],
        "import_dll_names": ["KERNEL32.dll"],
        "review_status": "static_review_candidate",
    }


def make_review_manifest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_review_manifest",
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "selected_candidates": [make_candidate()],
        "hold_candidates": [make_candidate("AEPluginBuild\\MaskOffset.aex")],
    }


def make_design_packet() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_worker_sandbox_design_packet",
        "design_state": "no_load_worker_boundary_only",
        "primary_review_candidate": {
            "relative_path": "AEPluginBuild\\ScatterMap.aex",
            "compatibility_class": "classic_pf_effect_candidate",
            "effect_main_export_present": True,
            "aegp_marker_count": 0,
            "approval_state": "not_approved_for_load",
        },
        "blocked_actions": ["load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
    }


def make_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_no_load_worker_selftest",
        "worker_selftest_passed": True,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "steps": [
            {"step": "hello"},
            {"step": "inspect_environment"},
            {"step": "inspect_ppm"},
            {"step": "transform_ppm_identity"},
            {"step": "blocked_load_aex", "code": "blocked_action"},
            {"step": "quit"},
        ],
    }


def make_dependency_review() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_dependency_review_packet",
        "review_state": "dependency_review_ready_for_manual_loader_design_no_load",
        "native_load_recommendation": "manual_loader_design_review_only_no_auto_approval",
        "review_items": [],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexFixtureDecisionTests(unittest.TestCase):
    def test_hold_decision_is_not_approval_and_preserves_safety(self):
        decision = aex_fixture_decision.build_decision_manifest(
            make_review_manifest(),
            Path("fixture-review.json"),
            decision="hold",
            reason="manual provenance review pending",
        )
        self.assertEqual(decision["manifest_kind"], "aex_fixture_decision_manifest")
        self.assertEqual(decision["decision_state"], "hold_for_manual_review")
        self.assertEqual(decision["approval_state"], "not_approved_for_load_gate")
        self.assertFalse(decision["explicit_user_approval"])
        self.assertFalse(decision["native_load_performed"])
        self.assertEqual(decision["candidate_relative_path"], "AEPluginBuild\\ScatterMap.aex")

    def test_approve_requires_explicit_user_approval_and_token(self):
        with self.assertRaises(ValueError):
            aex_fixture_decision.build_decision_manifest(
                make_review_manifest(),
                Path("fixture-review.json"),
                decision="approve",
                explicit_user_approval=True,
                approval_token="wrong",
            )
        approval = aex_fixture_decision.build_decision_manifest(
            make_review_manifest(),
            Path("fixture-review.json"),
            decision="approve",
            explicit_user_approval=True,
            approval_token=aex_fixture_decision.APPROVAL_TOKEN,
        )
        self.assertEqual(approval["manifest_kind"], "aex_fixture_approval_manifest")
        self.assertEqual(approval["approval_state"], "user_approved_for_load_gate")
        self.assertTrue(approval["explicit_user_approval"])
        self.assertIn("prepare_native_load_gate", approval["approved_actions"])
        self.assertFalse(approval["native_load_performed"])

    def test_gate_stays_closed_with_hold_decision_manifest(self):
        hold = aex_fixture_decision.build_decision_manifest(
            make_review_manifest(),
            Path("fixture-review.json"),
            decision="hold",
        )
        report = aex_load_gate_check.build_gate_report(
            design_packet=make_design_packet(),
            design_packet_path=Path("design.json"),
            worker_selftest=make_selftest(),
            worker_selftest_path=Path("selftest.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency-review.json"),
            fixture_approval=hold,
            fixture_approval_path=Path("hold.json"),
        )
        self.assertEqual(report["gate_state"], "closed_missing_or_invalid_approval")
        self.assertTrue(any("fixture decision is not an approval" in error for error in report["gate_errors"]))
        self.assertFalse(report["native_load_performed"])

    def test_paths_are_confined_and_create_new(self):
        review_root = LAB_ROOT / "target" / "fixture-review"
        review_root.mkdir(parents=True, exist_ok=True)
        source = review_root / f"{time.time_ns()}-decision-source.json"
        source.write_text(json.dumps(make_review_manifest()), encoding="utf-8")
        loaded, resolved = aex_fixture_decision.load_review_manifest(source)
        self.assertEqual(loaded["manifest_kind"], "aex_fixture_review_manifest")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-review.json"
        outside.write_text(json.dumps(make_review_manifest()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_fixture_decision.load_review_manifest(outside)

        payload = aex_fixture_decision.build_decision_manifest(loaded, resolved, decision="reject")
        out = LAB_ROOT / "target" / "fixture-approval" / f"{time.time_ns()}-reject.local.json"
        written = aex_fixture_decision.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_fixture_decision.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_fixture_decision.write_json_create_new(LAB_ROOT / "target" / "outside-decision.json", payload)


if __name__ == "__main__":
    unittest.main()
