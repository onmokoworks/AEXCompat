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


aex_native_loader_path_policy_selftest = load_tool("aex_native_loader_path_policy_selftest")


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


def runtime_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_runtime_selftest",
        "runtime_selftest_state": "runtime_containment_selftest_passed_no_load",
        "runtime_containment_selftest_passed": True,
        "source_native_loader_runtime_contract_state": "runtime_containment_contract_ready_no_load",
        "runtime_containment_ready": True,
        "path_allowlist_state": "closed_no_aex_paths_accepted",
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "accepted_aex_path": None,
        "path_payload_supplied": False,
        "synthetic_subprocess_only": True,
        "normal_exit_case_passed": True,
        "stderr_capture_passed": True,
        "timeout_case_passed": True,
        "child_cleanup_passed": True,
        "blocked_actions": ["accept_aex_path", "open_aex_file", "load_aex_dll"],
        **SAFETY_FALSE,
    }


class AexNativeLoaderPathPolicySelftestTests(unittest.TestCase):
    def test_builds_closed_path_policy_selftest_without_raw_paths(self):
        report = aex_native_loader_path_policy_selftest.build_path_policy_selftest(
            runtime_selftest=runtime_selftest(),
            runtime_selftest_path=LAB_ROOT / "target" / "native-loader-runtime-selftest" / "runtime.local.json",
        )
        self.assertEqual(report["report_kind"], "aex_native_loader_path_policy_selftest")
        self.assertEqual(report["path_policy_selftest_state"], "closed_path_policy_selftest_passed_no_aex_path")
        self.assertTrue(report["path_policy_selftest_passed"])
        self.assertFalse(report["path_acceptance_ready"])
        self.assertFalse(report["aex_path_acceptance_enabled"])
        self.assertIsNone(report["accepted_aex_path"])
        self.assertFalse(report["path_payload_supplied"])
        self.assertTrue(report["synthetic_path_inputs_only"])
        self.assertFalse(report["candidate_path_string_accepted"])
        self.assertTrue(report["absolute_path_rejected"])
        self.assertTrue(report["traversal_rejected"])
        self.assertTrue(report["non_aex_suffix_rejected"])
        self.assertTrue(report["redaction_passed"])
        self.assertFalse(report["raw_input_paths_serialized"])
        self.assertEqual(report["path_case_count"], 4)
        serialized = json.dumps(report, ensure_ascii=False)
        self.assertNotIn(r"APPROVED_ROOT\Plugins\Effects\Candidate.aex", serialized)
        self.assertNotIn(r"C:\Private\Fixture\Candidate.aex", serialized)
        self.assertNotIn(r"APPROVED_ROOT\..\Outside\Candidate.aex", serialized)
        self.assertFalse(report["native_load_enabled"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["aex_file_opened"])

    def test_classifies_synthetic_path_inputs(self):
        cases = {
            r"APPROVED_ROOT\Plugins\Effects\Candidate.aex": "path_acceptance_gate_closed",
            r"C:\Private\Fixture\Candidate.aex": "absolute_path_not_allowed_while_gate_closed",
            r"APPROVED_ROOT\..\Outside\Candidate.aex": "traversal_component_rejected",
            r"APPROVED_ROOT\Plugins\Effects\Candidate.txt": "non_aex_suffix_rejected",
        }
        for path_text, expected_reason in cases.items():
            with self.subTest(path_text=path_text):
                result = aex_native_loader_path_policy_selftest.classify_path_input(path_text)
                self.assertFalse(result["accepted"])
                self.assertEqual(result["reason"], expected_reason)

    def test_rejects_runtime_selftest_that_accepts_paths(self):
        unsafe = runtime_selftest()
        unsafe["path_acceptance_ready"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_native_loader_path_policy_selftest.build_path_policy_selftest(
                runtime_selftest=unsafe,
                runtime_selftest_path=LAB_ROOT / "target" / "native-loader-runtime-selftest" / "runtime.local.json",
            )
        self.assertIn("path_acceptance_ready must be false", str(ctx.exception))

    def test_rejects_runtime_selftest_with_open_native_flag(self):
        unsafe = runtime_selftest()
        unsafe["aex_file_opened"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_native_loader_path_policy_selftest.build_path_policy_selftest(
                runtime_selftest=unsafe,
                runtime_selftest_path=LAB_ROOT / "target" / "native-loader-runtime-selftest" / "runtime.local.json",
            )
        self.assertIn("aex_file_opened must be false", str(ctx.exception))

    def test_paths_are_confined_and_output_is_create_new(self):
        root = LAB_ROOT / "target" / "native-loader-runtime-selftest"
        root.mkdir(parents=True, exist_ok=True)
        source = root / f"{time.time_ns()}-runtime-selftest.local.json"
        source.write_text(json.dumps(runtime_selftest()), encoding="utf-8")
        loaded, resolved = aex_native_loader_path_policy_selftest.load_runtime_selftest(source)
        self.assertEqual(loaded["report_kind"], "aex_native_loader_runtime_selftest")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-runtime-selftest.local.json"
        outside.write_text(json.dumps(runtime_selftest()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_native_loader_path_policy_selftest.load_runtime_selftest(outside)

        report = aex_native_loader_path_policy_selftest.build_path_policy_selftest(
            runtime_selftest=loaded,
            runtime_selftest_path=resolved,
        )
        out = (
            LAB_ROOT
            / "target"
            / "native-loader-path-policy-selftest"
            / f"{time.time_ns()}-path-policy.local.json"
        )
        written = aex_native_loader_path_policy_selftest.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_native_loader_path_policy_selftest.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_native_loader_path_policy_selftest.write_json_create_new(
                LAB_ROOT / "target" / "outside-path-policy.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
