import importlib.util
import json
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_safety_chain_audit = load_tool("aex_safety_chain_audit")


def artifact_payloads() -> dict[str, dict]:
    candidate = {
        "relative_path": "AEPluginBuild\\ScatterMap.aex",
        "approval_state": "not_approved_for_load",
    }
    return {
        "static_report": {
            "publication_status": "local-only",
            "report_kind": "aex_static_probe",
            "aex_count": 1,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
        },
        "fixture_manifest": {
            "publication_status": "local-only",
            "manifest_kind": "aex_fixture_review_manifest",
            "selected_candidates": [{"relative_path": "AEPluginBuild\\ScatterMap.aex"}],
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
        },
        "fixture_decision": {
            "publication_status": "local-only",
            "manifest_kind": "aex_fixture_decision_manifest",
            "decision_state": "hold_for_manual_review",
            "approval_state": "not_approved_for_load_gate",
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
        },
        "worker_design": {
            "publication_status": "local-only",
            "packet_kind": "aex_worker_sandbox_design_packet",
            "primary_review_candidate": candidate,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
        },
        "worker_selftest": {
            "publication_status": "local-only",
            "report_kind": "aex_no_load_worker_selftest",
            "worker_selftest_passed": True,
            "steps": [{"step": "blocked_load_aex", "code": "blocked_action"}],
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
        },
        "dependency_review": {
            "publication_status": "local-only",
            "packet_kind": "aex_dependency_review_packet",
            "review_state": "dependency_review_pending_native_load_blocked",
            "native_load_recommendation": "do_not_open_native_load_gate",
            "review_items": [],
            "native_load_enabled": False,
            "native_load_performed": False,
            "dll_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
        },
        "load_gate": {
            "publication_status": "local-only",
            "report_kind": "aex_load_gate_check",
            "gate_state": "closed_dependency_review_or_invalid_approval",
            "approval_state": "present",
            "dependency_review_state": "present",
            "dependency_native_load_recommendation": "do_not_open_native_load_gate",
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
        },
        "native_loader_stub": {
            "publication_status": "local-only",
            "report_kind": "aex_native_loader_stub_report",
            "stub_state": "refused_gate_closed",
            "accepted_aex_path": None,
            "native_load_enabled": False,
            "native_load_performed": False,
            "render_performed": False,
            "ae_invoked": False,
            "ofx_route_invoked": False,
            "private_payload_copied": False,
            "aex_file_opened": False,
        },
        "ofx_facade": {
            "publication_status": "local-only",
            "packet_kind": "aex_ofx_facade_deferred_packet",
            "facade_state": "deferred_loader_not_ready",
            "ofx_route_action": "no_op",
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
        },
        "ofx_noop_mock": {
            "publication_status": "local-only",
            "report_kind": "aex_ofx_noop_mock_selftest",
            "mock_state": "mock_identity_completed_route_closed",
            "identity_check": {"pixel_match": True, "dimension_match": True},
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
        },
    }


def write_payloads(payloads: dict[str, dict]) -> dict[str, Path]:
    paths = {}
    for label, payload in payloads.items():
        root = aex_safety_chain_audit.ROOTS[label]
        root.mkdir(parents=True, exist_ok=True)
        path = root / f"{time.time_ns()}-{label}.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        paths[label] = path
    return paths


class AexSafetyChainAuditTests(unittest.TestCase):
    def test_cli_exit_follows_audit_result_and_writes_report(self):
        with tempfile.TemporaryDirectory() as directory:
            temporary_root = Path(directory)
            roots = {
                label: temporary_root / "inputs" / label
                for label in aex_safety_chain_audit.ROOTS
            }
            output_root = temporary_root / "safety-audit"
            with mock.patch.object(aex_safety_chain_audit, "ROOTS", roots), mock.patch.object(
                aex_safety_chain_audit, "SAFETY_AUDIT_ROOT", output_root
            ):
                paths = write_payloads(artifact_payloads())
                output = output_root / "clean.json"
                argv = [
                    "aex_safety_chain_audit.py",
                    "--static-report", str(paths["static_report"]),
                    "--fixture-manifest", str(paths["fixture_manifest"]),
                    "--fixture-decision", str(paths["fixture_decision"]),
                    "--worker-design", str(paths["worker_design"]),
                    "--worker-selftest", str(paths["worker_selftest"]),
                    "--dependency-review", str(paths["dependency_review"]),
                    "--load-gate", str(paths["load_gate"]),
                    "--native-loader-stub", str(paths["native_loader_stub"]),
                    "--ofx-facade", str(paths["ofx_facade"]),
                    "--ofx-noop-mock", str(paths["ofx_noop_mock"]),
                    "--out", str(output),
                ]
                with mock.patch.object(sys, "argv", argv):
                    self.assertEqual(aex_safety_chain_audit.main(), 0)
                self.assertTrue(json.loads(output.read_text(encoding="utf-8"))["audit_passed"])

                payloads = artifact_payloads()
                payloads["ofx_noop_mock"]["ofx_route_invoked"] = True
                paths = write_payloads(payloads)
                output = output_root / "failed.json"
                argv[-1] = str(output)
                for index, label in enumerate(
                    (
                        "static_report",
                        "fixture_manifest",
                        "fixture_decision",
                        "worker_design",
                        "worker_selftest",
                        "dependency_review",
                        "load_gate",
                        "native_loader_stub",
                        "ofx_facade",
                        "ofx_noop_mock",
                    )
                ):
                    argv[2 + index * 2] = str(paths[label])
                with mock.patch.object(sys, "argv", argv):
                    self.assertEqual(aex_safety_chain_audit.main(), 1)
                report = json.loads(output.read_text(encoding="utf-8"))
                self.assertFalse(report["audit_passed"])
                self.assertIn("ofx_noop_mock ofx_route_invoked must be false", report["errors"])

    def test_audit_passes_clean_no_load_chain(self):
        paths = write_payloads(artifact_payloads())
        report = aex_safety_chain_audit.build_audit(paths)
        self.assertTrue(report["audit_passed"])
        self.assertEqual(report["audit_state"], "no_load_chain_verified")
        self.assertEqual(report["artifact_count"], 10)
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["ofx_route_invoked"])

    def test_audit_fails_on_open_route_or_approval(self):
        payloads = artifact_payloads()
        payloads["ofx_noop_mock"]["ofx_route_invoked"] = True
        report = aex_safety_chain_audit.build_audit(write_payloads(payloads))
        self.assertFalse(report["audit_passed"])
        self.assertIn("ofx_noop_mock ofx_route_invoked must be false", report["errors"])

        payloads = artifact_payloads()
        payloads["fixture_decision"]["manifest_kind"] = "aex_fixture_approval_manifest"
        payloads["fixture_decision"]["approval_state"] = "user_approved_for_load_gate"
        report = aex_safety_chain_audit.build_audit(write_payloads(payloads))
        self.assertFalse(report["audit_passed"])
        self.assertTrue(any("approval" in error for error in report["errors"]))

    def test_paths_are_confined_and_output_is_create_new(self):
        payloads = artifact_payloads()
        paths = write_payloads(payloads)
        loaded, resolved = aex_safety_chain_audit.load_artifact("load_gate", paths["load_gate"])
        self.assertEqual(loaded["report_kind"], "aex_load_gate_check")
        self.assertEqual(resolved, paths["load_gate"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-audit.json"
        outside.write_text(json.dumps(payloads["load_gate"]), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_safety_chain_audit.load_artifact("load_gate", outside)

        report = aex_safety_chain_audit.build_audit(paths)
        out = LAB_ROOT / "target" / "safety-audit" / f"{time.time_ns()}-audit.local.json"
        written = aex_safety_chain_audit.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_safety_chain_audit.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_safety_chain_audit.write_json_create_new(LAB_ROOT / "target" / "outside-audit.json", report)


if __name__ == "__main__":
    unittest.main()
