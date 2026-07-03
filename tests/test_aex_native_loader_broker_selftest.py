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


aex_native_loader_broker = load_tool("aex_native_loader_broker")
aex_native_loader_broker_selftest = load_tool("aex_native_loader_broker_selftest")


def make_design_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_design_contract",
        "native_loader_design_state": "native_loader_design_ready_loader_closed",
        "contract_state": "native_loader_design_contract_ready_loader_closed_pending_fixture_approval",
        "loader_design_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "candidate_dependencies_clear": True,
        "fixture_approval_satisfied": False,
        "native_load_gate": "closed",
        "approval_required_before_aex_path": True,
        "runtime_approval_required_before_load": True,
        "separate_process_required": True,
        "accepts_aex_path": False,
        "accepted_aex_path": None,
        "controller_loads_aex": False,
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def write_design_contract(payload: dict | None = None) -> Path:
    root = LAB_ROOT / "target" / "native-loader-design"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-broker-design.local.json"
    path.write_text(json.dumps(payload or make_design_contract()), encoding="utf-8")
    return path


class AexNativeLoaderBrokerSelftestTests(unittest.TestCase):
    def test_broker_direct_messages_are_pathless_and_closed(self):
        hello = aex_native_loader_broker.handle_message({"id": "hello", "type": "hello"})
        self.assertEqual(hello["type"], "hello_ack")
        self.assertEqual(hello["broker_kind"], "aex_pathless_native_loader_broker")
        self.assertFalse(hello["safety_state"]["accepts_aex_path"])
        self.assertIsNone(hello["safety_state"]["accepted_aex_path"])
        self.assertFalse(hello["safety_state"]["native_load_enabled"])

        blocked = aex_native_loader_broker.handle_message({"id": "blocked", "type": "accept_aex_path"})
        self.assertEqual(blocked["type"], "error")
        self.assertEqual(blocked["code"], "blocked_action")
        self.assertFalse(blocked["safety_state"]["accepts_aex_path"])
        self.assertIsNone(blocked["safety_state"]["accepted_aex_path"])

    def test_run_selftest_spawns_pathless_broker_and_reports_no_load(self):
        report = aex_native_loader_broker_selftest.run_selftest(
            broker_path=LAB_ROOT / "tools" / "aex_native_loader_broker.py",
            design_contract_path=write_design_contract(),
        )

        self.assertEqual(report["report_kind"], "aex_native_loader_broker_selftest")
        self.assertEqual(report["broker_selftest_state"], "pathless_native_loader_broker_selftest_passed")
        self.assertTrue(report["pathless_broker_ready"])
        self.assertTrue(report["native_loader_design_ready"])
        self.assertFalse(report["accepts_aex_path"])
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["path_payload_supplied"])
        self.assertEqual(report["blocked_action_count"], 6)
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertEqual([step["step"] for step in report["steps"]], [
            "hello",
            "inspect_environment",
            "blocked_pathless_native_actions",
            "quit",
        ])
        self.assertEqual(len(report["blocked_action_checks"]), 6)
        self.assertTrue(all(not item["path_payload_supplied"] for item in report["blocked_action_checks"]))

    def test_rejects_unsafe_design_contract_and_create_new_output(self):
        unsafe = make_design_contract()
        unsafe["accepts_aex_path"] = True
        with self.assertRaises(ValueError):
            aex_native_loader_broker_selftest.load_design_contract(write_design_contract(unsafe))

        unsafe = make_design_contract()
        unsafe["native_load_enabled"] = True
        with self.assertRaises(ValueError):
            aex_native_loader_broker_selftest.load_design_contract(write_design_contract(unsafe))

        report = aex_native_loader_broker_selftest.run_selftest(
            broker_path=LAB_ROOT / "tools" / "aex_native_loader_broker.py",
            design_contract_path=write_design_contract(),
        )
        out = LAB_ROOT / "target" / "native-loader-broker-selftest" / f"{time.time_ns()}-broker-selftest.local.json"
        written = aex_native_loader_broker_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_native_loader_broker_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_native_loader_broker_selftest.write_json_create_new(
                LAB_ROOT / "target" / "outside-broker-selftest.json",
                report,
            )

    def test_paths_are_confined(self):
        design_path = write_design_contract()
        loaded, resolved = aex_native_loader_broker_selftest.load_design_contract(design_path)
        self.assertEqual(loaded["report_kind"], "aex_native_loader_design_contract")
        self.assertEqual(resolved, design_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-design.json"
        outside.write_text(json.dumps(make_design_contract()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_native_loader_broker_selftest.load_design_contract(outside)

        with self.assertRaises(ValueError):
            aex_native_loader_broker_selftest.validate_broker_path(LAB_ROOT / "README.md")


if __name__ == "__main__":
    unittest.main()
