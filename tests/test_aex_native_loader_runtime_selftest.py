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


aex_native_loader_runtime_selftest = load_tool("aex_native_loader_runtime_selftest")


SAFETY_FALSE = {
    "native_load_enabled": False,
    "native_load_performed": False,
    "dll_load_performed": False,
    "render_performed": False,
    "ae_invoked": False,
    "ofx_route_invoked": False,
    "private_payload_copied": False,
    "aex_file_opened": False,
}


def runtime_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_contract",
        "native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
        "contract_state": "runtime_containment_contract_ready_path_acceptance_closed",
        "runtime_containment_ready": True,
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "broker_selftest_passed": True,
        "process_isolation_required": True,
        "native_load_gate": "closed",
        "candidate_dependencies_clear": True,
        "fixture_approval_satisfied": False,
        "blocked_actions": ["accept_aex_path", "open_aex_file", "load_aex_dll", "call_EffectMain"],
        **SAFETY_FALSE,
    }


class AexNativeLoaderRuntimeSelftestTests(unittest.TestCase):
    def test_builds_no_load_synthetic_runtime_selftest(self):
        report = aex_native_loader_runtime_selftest.build_runtime_selftest(
            runtime_contract=runtime_contract(),
            runtime_contract_path=LAB_ROOT / "target" / "native-loader-runtime-contract" / "runtime.local.json",
            timeout_ms=1000,
            cleanup_timeout_ms=2000,
        )
        self.assertEqual(report["report_kind"], "aex_native_loader_runtime_selftest")
        self.assertEqual(report["runtime_selftest_state"], "runtime_containment_selftest_passed_no_load")
        self.assertTrue(report["runtime_containment_selftest_passed"])
        self.assertTrue(report["synthetic_subprocess_only"])
        self.assertTrue(report["normal_exit_case_passed"])
        self.assertTrue(report["stderr_capture_passed"])
        self.assertTrue(report["timeout_case_passed"])
        self.assertTrue(report["child_cleanup_passed"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["aex_path_acceptance_enabled"])
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["path_payload_supplied"])
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["aex_file_opened"])

    def test_rejects_contract_that_accepts_paths(self):
        unsafe = runtime_contract()
        unsafe["aex_path_acceptance_enabled"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_native_loader_runtime_selftest.build_runtime_selftest(
                runtime_contract=unsafe,
                runtime_contract_path=LAB_ROOT / "target" / "native-loader-runtime-contract" / "runtime.local.json",
            )
        self.assertIn("aex_path_acceptance_enabled must be false", str(ctx.exception))

    def test_rejects_contract_with_native_load_flag(self):
        unsafe = runtime_contract()
        unsafe["native_load_performed"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_native_loader_runtime_selftest.build_runtime_selftest(
                runtime_contract=unsafe,
                runtime_contract_path=LAB_ROOT / "target" / "native-loader-runtime-contract" / "runtime.local.json",
            )
        self.assertIn("native_load_performed must be false", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        root = LAB_ROOT / "target" / "native-loader-runtime-contract"
        root.mkdir(parents=True, exist_ok=True)
        source = root / f"{time.time_ns()}-{os.getpid()}-runtime-contract.local.json"
        source.write_text(json.dumps(runtime_contract()), encoding="utf-8")
        loaded, resolved = aex_native_loader_runtime_selftest.load_runtime_contract(source)
        self.assertEqual(loaded["report_kind"], "aex_native_loader_runtime_contract")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-runtime-contract.local.json"
        outside.write_text(json.dumps(runtime_contract()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_native_loader_runtime_selftest.load_runtime_contract(outside)

        report = aex_native_loader_runtime_selftest.build_runtime_selftest(
            runtime_contract=loaded,
            runtime_contract_path=resolved,
            timeout_ms=1000,
            cleanup_timeout_ms=2000,
        )
        out = LAB_ROOT / "target" / "native-loader-runtime-selftest" / f"{time.time_ns()}-{os.getpid()}-runtime-selftest.local.json"
        written = aex_native_loader_runtime_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_native_loader_runtime_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_native_loader_runtime_selftest.write_json_create_new(
                LAB_ROOT / "target" / "outside-runtime-selftest.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
