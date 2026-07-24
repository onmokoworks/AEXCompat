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


aex_candidate_load_gate_dryrun = load_tool("aex_candidate_load_gate_dryrun")


def make_candidate() -> dict:
    return {
        "relative_path": "AEPluginBuild\\ScatterMap.aex",
        "file_name": "ScatterMap.aex",
        "compatibility_class": "classic_pf_effect_candidate",
        "effect_main_export_present": True,
        "aegp_marker_count": 0,
        "approval_state": "not_approved_for_load",
    }


def make_worker_design() -> dict:
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


def make_worker_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_no_load_worker_selftest",
        "worker_selftest_passed": True,
        "steps": [
            {"step": "hello"},
            {"step": "inspect_environment"},
            {"step": "inspect_ppm"},
            {"step": "transform_ppm_identity"},
            {"step": "blocked_load_aex", "code": "blocked_action"},
            {"step": "quit"},
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_fixture_decision() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "manifest_kind": "aex_fixture_decision_manifest",
        "decision_state": "hold_for_manual_review",
        "approval_state": "not_approved_for_load_gate",
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_fixture_approval() -> dict:
    approval = make_fixture_decision()
    approval.update(
        {
            "manifest_kind": "aex_fixture_approval_manifest",
            "approval_state": "user_approved_for_load_gate",
            "explicit_user_approval": True,
            "approved_actions": ["prepare_native_load_gate"],
        }
    )
    approval.pop("decision_state")
    return approval


def make_manual_review() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_fixture_manual_review_packet",
        "review_packet_state": "fixture_manual_review_packet_ready_no_load",
        "manual_review_ready": True,
        "approval_ready": False,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_candidate_scope() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_dependency_scope_packet",
        "candidate_dependency_scope_state": "candidate_dependency_scope_ready_no_load",
        "candidate_scope_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate_dependency_blockers_present": False,
        "candidate_dependency_blocker_count": 0,
        "candidate_dependency_review_count": 0,
        "candidate_dependency_missing_or_api_set_review_count": 0,
        "candidate_dependency_found_paths_exported": False,
        "global_dependency_blockers_present": True,
        "global_dependency_blockers_apply_to_candidate": False,
        "scoped_gate_recommendation": "candidate_dependencies_clear_global_gate_still_closed",
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_source_load_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_load_gate_check",
        "primary_review_candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
        "gate_state": "closed_dependency_review_or_invalid_approval",
        "dependency_native_load_recommendation": "do_not_open_native_load_gate",
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
    inputs = {
        "worker_design": make_worker_design(),
        "worker_design_path": Path("design.json"),
        "worker_selftest": make_worker_selftest(),
        "worker_selftest_path": Path("selftest.json"),
        "fixture_decision": make_fixture_decision(),
        "fixture_decision_path": Path("decision.json"),
        "fixture_manual_review": make_manual_review(),
        "fixture_manual_review_path": Path("manual-review.json"),
        "candidate_dependency_scope": make_candidate_scope(),
        "candidate_dependency_scope_path": Path("scope.json"),
        "source_load_gate": make_source_load_gate(),
        "source_load_gate_path": Path("load-gate.json"),
    }
    inputs.update(overrides)
    return aex_candidate_load_gate_dryrun.build_candidate_load_gate_dryrun(**inputs)


class AexCandidateLoadGateDryrunTests(unittest.TestCase):
    def test_candidate_deps_clear_but_fixture_approval_missing_keeps_gate_closed(self):
        report = build_report()

        self.assertEqual(report["report_kind"], "aex_candidate_load_gate_dryrun")
        self.assertEqual(report["candidate_load_gate_dryrun_state"], "candidate_load_gate_dryrun_ready_no_load")
        self.assertEqual(
            report["candidate_scoped_load_gate_dry_run_state"],
            "closed_dry_run_candidate_deps_clear_fixture_approval_missing_global_deps_blocked",
        )
        self.assertFalse(report["fixture_approval_satisfied"])
        self.assertTrue(report["candidate_dependencies_clear"])
        self.assertFalse(report["candidate_dependency_blockers_present"])
        self.assertTrue(report["global_dependency_blockers_present"])
        self.assertFalse(report["global_dependency_blockers_apply_to_candidate"])
        self.assertEqual(report["native_load_gate"], "closed")
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        by_gate = {gate["gate"]: gate for gate in report["gates"]}
        self.assertEqual(by_gate["G3_manual_fixture_approval"]["status"], "not_satisfied")
        self.assertEqual(by_gate["G5_candidate_scoped_native_load_gate_dry_run"]["status"], "closed")

    def test_explicit_approval_can_only_report_ready_without_loading(self):
        scope = make_candidate_scope()
        scope["global_dependency_blockers_present"] = False
        report = build_report(
            fixture_decision=make_fixture_approval(),
            candidate_dependency_scope=scope,
            source_load_gate=None,
            source_load_gate_path=None,
        )

        self.assertEqual(
            report["candidate_scoped_load_gate_dry_run_state"],
            "candidate_scoped_preconditions_satisfied_no_load_performed",
        )
        self.assertTrue(report["fixture_approval_satisfied"])
        self.assertTrue(report["candidate_gate_ready_for_separate_loader_design"])
        self.assertEqual(report["native_load_gate"], "closed_dry_run_ready_for_separate_loader_design")
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertIn("load_aex_dll", report["blocked_actions"])

    def test_candidate_dependency_blocker_closes_dry_run(self):
        scope = make_candidate_scope()
        scope["candidate_dependency_blockers_present"] = True
        scope["candidate_dependency_blocker_count"] = 1
        scope["global_dependency_blockers_apply_to_candidate"] = True
        report = build_report(candidate_dependency_scope=scope)

        self.assertEqual(
            report["candidate_scoped_load_gate_dry_run_state"],
            "closed_dry_run_candidate_dependency_blockers_or_reviews_present",
        )
        self.assertFalse(report["candidate_dependencies_clear"])
        self.assertTrue(report["candidate_dependency_blockers_present"])
        self.assertFalse(report["native_load_performed"])

    def test_unsafe_or_mismatched_evidence_invalidates_report(self):
        selftest = make_worker_selftest()
        selftest["aex_file_opened"] = True
        report = build_report(worker_selftest=selftest)
        self.assertEqual(report["candidate_scoped_load_gate_dry_run_state"], "invalid_evidence_closed")
        self.assertEqual(
            report["candidate_load_gate_dryrun_state"],
            "candidate_load_gate_dryrun_invalid_evidence_closed",
        )
        self.assertIn("worker selftest aex_file_opened must be false", report["gate_errors"])

        scope = make_candidate_scope()
        scope["candidate_relative_path"] = "Other.aex"
        report = build_report(candidate_dependency_scope=scope)
        self.assertEqual(report["candidate_scoped_load_gate_dry_run_state"], "invalid_evidence_closed")
        self.assertTrue(any("candidate_relative_path" in error for error in report["gate_errors"]))

    def test_cli_exit_follows_gate_errors(self):
        args = SimpleNamespace(
            worker_design="design.json",
            worker_selftest="selftest.json",
            fixture_decision="decision.json",
            fixture_manual_review="manual-review.json",
            candidate_dependency_scope="scope.json",
            source_load_gate=None,
            out="report.json",
        )
        loaders = (
            "load_worker_design",
            "load_worker_selftest",
            "load_fixture_decision",
            "load_fixture_manual_review",
            "load_candidate_dependency_scope",
        )
        for gate_errors, expected_exit in (([], 0), (["invalid evidence"], 1)):
            report = {"gate_errors": gate_errors}
            with (
                mock.patch.object(aex_candidate_load_gate_dryrun, "parse_args", return_value=args),
                mock.patch.multiple(
                    aex_candidate_load_gate_dryrun,
                    **{name: mock.Mock(return_value=({}, Path(f"{name}.json"))) for name in loaders},
                ),
                mock.patch.object(
                    aex_candidate_load_gate_dryrun,
                    "build_candidate_load_gate_dryrun",
                    return_value=report,
                ),
                mock.patch.object(
                    aex_candidate_load_gate_dryrun,
                    "write_json_create_new",
                    return_value=Path("report.json"),
                ) as write_report,
            ):
                self.assertEqual(aex_candidate_load_gate_dryrun.main(), expected_exit)
                write_report.assert_called_once_with(Path("report.json"), report)

    def test_paths_are_confined_and_output_is_create_new(self):
        roots = {
            "design": LAB_ROOT / "target" / "worker-design",
            "selftest": LAB_ROOT / "target" / "worker-selftest",
            "decision": LAB_ROOT / "target" / "fixture-approval",
            "manual": LAB_ROOT / "target" / "fixture-manual-review",
            "scope": LAB_ROOT / "target" / "candidate-dependency-scope",
            "load_gate": LAB_ROOT / "target" / "load-gate",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = time.time_ns()
        paths = {
            "design": roots["design"] / f"{stamp}-design.local.json",
            "selftest": roots["selftest"] / f"{stamp}-selftest.local.json",
            "decision": roots["decision"] / f"{stamp}-decision.local.json",
            "manual": roots["manual"] / f"{stamp}-manual.local.json",
            "scope": roots["scope"] / f"{stamp}-scope.local.json",
            "load_gate": roots["load_gate"] / f"{stamp}-load-gate.local.json",
        }
        payloads = {
            "design": make_worker_design(),
            "selftest": make_worker_selftest(),
            "decision": make_fixture_decision(),
            "manual": make_manual_review(),
            "scope": make_candidate_scope(),
            "load_gate": make_source_load_gate(),
        }
        for key, path in paths.items():
            path.write_text(json.dumps(payloads[key]), encoding="utf-8")

        design, design_path = aex_candidate_load_gate_dryrun.load_worker_design(paths["design"])
        selftest, selftest_path = aex_candidate_load_gate_dryrun.load_worker_selftest(paths["selftest"])
        decision, decision_path = aex_candidate_load_gate_dryrun.load_fixture_decision(paths["decision"])
        manual, manual_path = aex_candidate_load_gate_dryrun.load_fixture_manual_review(paths["manual"])
        scope, scope_path = aex_candidate_load_gate_dryrun.load_candidate_dependency_scope(paths["scope"])
        gate, gate_path = aex_candidate_load_gate_dryrun.load_source_load_gate(paths["load_gate"])
        self.assertEqual(design_path, paths["design"].resolve())
        self.assertEqual(selftest_path, paths["selftest"].resolve())
        self.assertEqual(decision_path, paths["decision"].resolve())
        self.assertEqual(manual_path, paths["manual"].resolve())
        self.assertEqual(scope_path, paths["scope"].resolve())
        self.assertEqual(gate_path, paths["load_gate"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-decision.json"
        outside.write_text(json.dumps(make_fixture_decision()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_load_gate_dryrun.load_fixture_decision(outside)

        report = aex_candidate_load_gate_dryrun.build_candidate_load_gate_dryrun(
            worker_design=design,
            worker_design_path=design_path,
            worker_selftest=selftest,
            worker_selftest_path=selftest_path,
            fixture_decision=decision,
            fixture_decision_path=decision_path,
            fixture_manual_review=manual,
            fixture_manual_review_path=manual_path,
            candidate_dependency_scope=scope,
            candidate_dependency_scope_path=scope_path,
            source_load_gate=gate,
            source_load_gate_path=gate_path,
        )
        out = LAB_ROOT / "target" / "candidate-load-gate" / f"{time.time_ns()}-candidate-load-gate.local.json"
        written = aex_candidate_load_gate_dryrun.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_load_gate_dryrun.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_load_gate_dryrun.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-load-gate.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
