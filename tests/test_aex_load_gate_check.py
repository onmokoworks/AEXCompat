import importlib.util
import json
import sys
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_load_gate_check = load_tool("aex_load_gate_check")


def make_candidate() -> dict:
    return {
        "relative_path": "AEPluginBuild\\ScatterMap.aex",
        "file_name": "ScatterMap.aex",
        "compatibility_class": "classic_pf_effect_candidate",
        "effect_main_export_present": True,
        "aegp_marker_count": 0,
        "approval_state": "not_approved_for_load",
    }


def make_design_packet() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_worker_sandbox_design_packet",
        "design_state": "no_load_worker_boundary_only",
        "primary_review_candidate": make_candidate(),
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


def make_approval() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_approval_manifest",
        "approval_state": "user_approved_for_load_gate",
        "explicit_user_approval": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "approved_actions": ["prepare_native_load_gate"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
    }


def make_dependency_review(recommendation: str = "manual_loader_design_review_only_no_auto_approval") -> dict:
    review_state = "dependency_review_ready_for_manual_loader_design_no_load"
    if recommendation == "do_not_open_native_load_gate":
        review_state = "dependency_review_pending_native_load_blocked"
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_dependency_review_packet",
        "review_state": review_state,
        "native_load_recommendation": recommendation,
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


class AexLoadGateCheckTests(unittest.TestCase):
    def test_cli_exit_follows_gate_errors(self):
        args = SimpleNamespace(design_packet="d.json", worker_selftest="s.json", dependency_review="r.json", fixture_approval=None, out="o.json")
        with mock.patch.object(aex_load_gate_check, "load_evidence", return_value=({}, Path("d.json"), {}, Path("s.json"), {}, Path("r.json"), None, None)), mock.patch.object(aex_load_gate_check, "parse_args", return_value=args), mock.patch.object(aex_load_gate_check, "build_gate_report", return_value={"gate_errors": ["closed"]}), mock.patch.object(aex_load_gate_check, "write_json_create_new", return_value=Path("o.json")):
            self.assertEqual(aex_load_gate_check.main(), 1)

    def test_gate_closes_when_fixture_approval_is_missing(self):
        report = aex_load_gate_check.build_gate_report(
            design_packet=make_design_packet(),
            design_packet_path=Path("design.json"),
            worker_selftest=make_selftest(),
            worker_selftest_path=Path("selftest.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency-review.json"),
        )
        self.assertEqual(report["report_kind"], "aex_load_gate_check")
        self.assertEqual(report["gate_state"], "closed_missing_or_invalid_approval")
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertIn("fixture approval manifest is missing", report["gate_errors"])

    def test_gate_rejects_unsafe_evidence(self):
        design = make_design_packet()
        design["native_load_performed"] = True
        report = aex_load_gate_check.build_gate_report(
            design_packet=design,
            design_packet_path=Path("design.json"),
            worker_selftest=make_selftest(),
            worker_selftest_path=Path("selftest.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency-review.json"),
        )
        self.assertEqual(report["gate_state"], "invalid_evidence_closed")
        self.assertIn("design native_load_performed must be false", report["gate_errors"])

        selftest = make_selftest()
        selftest["steps"] = [{"step": "hello"}]
        report = aex_load_gate_check.build_gate_report(
            design_packet=make_design_packet(),
            design_packet_path=Path("design.json"),
            worker_selftest=selftest,
            worker_selftest_path=Path("selftest.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency-review.json"),
        )
        self.assertEqual(report["gate_state"], "invalid_evidence_closed")
        self.assertTrue(any("blocked_load_aex" in error for error in report["gate_errors"]))

    def test_gate_can_report_preconditions_satisfied_without_loading(self):
        report = aex_load_gate_check.build_gate_report(
            design_packet=make_design_packet(),
            design_packet_path=Path("design.json"),
            worker_selftest=make_selftest(),
            worker_selftest_path=Path("selftest.json"),
            dependency_review=make_dependency_review(),
            dependency_review_path=Path("dependency-review.json"),
            fixture_approval=make_approval(),
            fixture_approval_path=Path("approval.json"),
        )
        self.assertEqual(report["gate_state"], "preconditions_satisfied_no_load_performed")
        self.assertFalse(report["native_load_performed"])
        by_gate = {gate["gate"]: gate for gate in report["gates"]}
        self.assertEqual(by_gate["G5_native_load_gate"]["status"], "ready_for_separate_loader_design")

    def test_dependency_review_blocks_gate_even_with_fixture_approval(self):
        report = aex_load_gate_check.build_gate_report(
            design_packet=make_design_packet(),
            design_packet_path=Path("design.json"),
            worker_selftest=make_selftest(),
            worker_selftest_path=Path("selftest.json"),
            dependency_review=make_dependency_review("do_not_open_native_load_gate"),
            dependency_review_path=Path("dependency-review.json"),
            fixture_approval=make_approval(),
            fixture_approval_path=Path("approval.json"),
        )
        self.assertEqual(report["gate_state"], "closed_dependency_review_or_invalid_approval")
        self.assertIn("dependency review recommendation blocks native load", report["gate_errors"])
        self.assertEqual(report["dependency_native_load_recommendation"], "do_not_open_native_load_gate")

    def test_loader_gate_paths_are_confined_and_create_new(self):
        design_root = LAB_ROOT / "target" / "worker-design"
        selftest_root = LAB_ROOT / "target" / "worker-selftest"
        dependency_root = LAB_ROOT / "target" / "dependency-review"
        design_root.mkdir(parents=True, exist_ok=True)
        selftest_root.mkdir(parents=True, exist_ok=True)
        dependency_root.mkdir(parents=True, exist_ok=True)
        design = design_root / f"{time.time_ns()}-gate-design.json"
        selftest = selftest_root / f"{time.time_ns()}-gate-selftest.json"
        dependency = dependency_root / f"{time.time_ns()}-gate-dependency-review.json"
        design.write_text(json.dumps(make_design_packet()), encoding="utf-8")
        selftest.write_text(json.dumps(make_selftest()), encoding="utf-8")
        dependency.write_text(json.dumps(make_dependency_review()), encoding="utf-8")
        (
            loaded_design,
            resolved_design,
            loaded_selftest,
            resolved_selftest,
            loaded_dependency,
            resolved_dependency,
            approval,
            approval_path,
        ) = (
            aex_load_gate_check.load_evidence(
                design_packet_path=design,
                worker_selftest_path=selftest,
                dependency_review_path=dependency,
                approval_path=None,
            )
        )
        self.assertEqual(loaded_design["packet_kind"], "aex_worker_sandbox_design_packet")
        self.assertEqual(loaded_selftest["report_kind"], "aex_no_load_worker_selftest")
        self.assertEqual(loaded_dependency["packet_kind"], "aex_dependency_review_packet")
        self.assertEqual(resolved_design, design.resolve())
        self.assertEqual(resolved_selftest, selftest.resolve())
        self.assertEqual(resolved_dependency, dependency.resolve())
        self.assertIsNone(approval)
        self.assertIsNone(approval_path)

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside.json"
        outside.write_text(json.dumps(make_design_packet()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_load_gate_check.load_evidence(
                design_packet_path=outside,
                worker_selftest_path=selftest,
                dependency_review_path=dependency,
                approval_path=None,
            )

        payload = aex_load_gate_check.build_gate_report(
            design_packet=make_design_packet(),
            design_packet_path=design,
            worker_selftest=make_selftest(),
            worker_selftest_path=selftest,
            dependency_review=make_dependency_review(),
            dependency_review_path=dependency,
        )
        out = LAB_ROOT / "target" / "load-gate" / f"{time.time_ns()}-gate.local.json"
        written = aex_load_gate_check.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_load_gate_check.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_load_gate_check.write_json_create_new(LAB_ROOT / "target" / "outside-gate.json", payload)


if __name__ == "__main__":
    unittest.main()
