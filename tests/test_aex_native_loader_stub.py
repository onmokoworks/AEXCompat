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


aex_native_loader_stub = load_tool("aex_native_loader_stub")


def make_gate_report(gate_state: str = "closed_missing_or_invalid_approval") -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_load_gate_check",
        "approval_state": "present",
        "gate_state": gate_state,
        "gate_errors": ["fixture decision is not an approval: hold_for_manual_review"]
        if gate_state != "preconditions_satisfied_no_load_performed"
        else [],
        "primary_review_candidate": {
            "relative_path": "AEPluginBuild\\ScatterMap.aex",
            "compatibility_class": "classic_pf_effect_candidate",
        },
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "blocked_actions": [
            "load_aex_dll",
            "call_EffectMain",
            "render_with_aex",
            "route_through_ofx",
        ],
    }


class AexNativeLoaderStubTests(unittest.TestCase):
    def test_closed_gate_is_refused_without_opening_aex(self):
        report = aex_native_loader_stub.build_stub_report(make_gate_report(), Path("gate.json"))
        self.assertEqual(report["report_kind"], "aex_native_loader_stub_report")
        self.assertEqual(report["stub_state"], "refused_gate_closed")
        self.assertEqual(report["loader_action"], "no_op")
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertIsNone(report["accepted_aex_path"])
        self.assertIn("accept_aex_path", report["blocked_actions"])

    def test_invalid_evidence_is_refused(self):
        gate = make_gate_report()
        gate["native_load_performed"] = True
        report = aex_native_loader_stub.build_stub_report(gate, Path("gate.json"))
        self.assertEqual(report["stub_state"], "invalid_evidence_refused")
        self.assertIn("load gate native_load_performed must be false", report["refusal_reasons"])
        self.assertFalse(report["native_load_performed"])

    def test_cli_exit_only_fails_for_invalid_evidence(self):
        args = SimpleNamespace(load_gate="gate.json", out="report.json")
        for report, expected_exit in (
            ({"stub_state": "refused_gate_closed", "refusal_reasons": ["gate closed"]}, 0),
            ({"stub_state": "invalid_evidence_refused", "refusal_reasons": ["invalid evidence"]}, 1),
        ):
            with (
                mock.patch.object(aex_native_loader_stub, "parse_args", return_value=args),
                mock.patch.object(
                    aex_native_loader_stub,
                    "load_gate_report",
                    return_value=({}, Path("gate.json")),
                ),
                mock.patch.object(aex_native_loader_stub, "build_stub_report", return_value=report),
                mock.patch.object(
                    aex_native_loader_stub,
                    "write_json_create_new",
                    return_value=Path("report.json"),
                ) as write_report,
            ):
                self.assertEqual(aex_native_loader_stub.main(), expected_exit)
                write_report.assert_called_once_with(Path("report.json"), report)

    def test_ready_gate_still_does_not_load(self):
        report = aex_native_loader_stub.build_stub_report(
            make_gate_report("preconditions_satisfied_no_load_performed"),
            Path("gate.json"),
        )
        self.assertEqual(report["stub_state"], "stub_ready_no_load_performed")
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertTrue(any("separate loader implementation" in reason for reason in report["refusal_reasons"]))

    def test_paths_are_confined_and_output_is_create_new(self):
        gate_root = LAB_ROOT / "target" / "load-gate"
        gate_root.mkdir(parents=True, exist_ok=True)
        gate = gate_root / f"{time.time_ns()}-loader-stub-gate.json"
        gate.write_text(json.dumps(make_gate_report()), encoding="utf-8")
        loaded, resolved = aex_native_loader_stub.load_gate_report(gate)
        self.assertEqual(loaded["report_kind"], "aex_load_gate_check")
        self.assertEqual(resolved, gate.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-gate.json"
        outside.write_text(json.dumps(make_gate_report()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_native_loader_stub.load_gate_report(outside)

        payload = aex_native_loader_stub.build_stub_report(loaded, resolved)
        out = LAB_ROOT / "target" / "native-loader-stub" / f"{time.time_ns()}-stub.local.json"
        written = aex_native_loader_stub.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_native_loader_stub.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_native_loader_stub.write_json_create_new(LAB_ROOT / "target" / "outside-stub.json", payload)


if __name__ == "__main__":
    unittest.main()
