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


aex_image_fixture_suite = load_tool("aex_image_fixture_suite")


def make_policy() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_sandbox_policy_packet",
        "sandbox_policy_state": "policy_ready_no_native_load",
        "primary_policy_candidate": {
            "relative_path": "AEPluginBuild\\ScatterMap.aex",
            "candidate_policy_state": "eligible_for_manual_policy_review",
            "native_load_approval": "not_granted",
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexImageFixtureSuiteTests(unittest.TestCase):
    def test_suite_generates_ppm_fixtures_and_manifest(self):
        suite_id = f"{time.time_ns()}-{os.getpid()}-suite"
        suite = aex_image_fixture_suite.build_suite(make_policy(), Path("policy.json"), suite_id=suite_id)
        self.assertEqual(suite["report_kind"], "aex_image_fixture_suite")
        self.assertEqual(suite["suite_state"], "image_fixture_suite_ready")
        self.assertEqual(suite["fixture_count"], 4)
        self.assertEqual(suite["target_candidate"]["relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertFalse(suite["native_load_performed"])
        self.assertFalse(suite["render_performed"])
        self.assertFalse(suite["aex_file_opened"])
        for fixture in suite["fixtures"]:
            path = Path(fixture["ppm_path"])
            self.assertTrue(path.exists())
            self.assertTrue(path.resolve().is_relative_to((LAB_ROOT / "target" / "ppm-fixtures").resolve()))
            self.assertEqual(fixture["pixel_bytes"], fixture["width"] * fixture["height"] * 3)

    def test_invalid_or_unsafe_policy_is_rejected(self):
        policy = make_policy()
        policy["sandbox_policy_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_image_fixture_suite.build_suite(policy, Path("policy.json"), suite_id=f"{time.time_ns()}-{os.getpid()}-bad")

        policy = make_policy()
        policy["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_image_fixture_suite.build_suite(policy, Path("policy.json"), suite_id=f"{time.time_ns()}-{os.getpid()}-bad")

    def test_paths_are_confined_and_outputs_are_create_new(self):
        policy_root = LAB_ROOT / "target" / "sandbox-policy"
        policy_root.mkdir(parents=True, exist_ok=True)
        source = policy_root / f"{time.time_ns()}-{os.getpid()}-image-policy.local.json"
        source.write_text(json.dumps(make_policy()), encoding="utf-8")
        loaded, resolved = aex_image_fixture_suite.load_sandbox_policy(source)
        self.assertEqual(loaded["packet_kind"], "aex_sandbox_policy_packet")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-image-policy.json"
        outside.write_text(json.dumps(make_policy()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_image_fixture_suite.load_sandbox_policy(outside)

        suite_id = f"{time.time_ns()}-{os.getpid()}-write"
        payload = aex_image_fixture_suite.build_suite(loaded, resolved, suite_id=suite_id)
        out = LAB_ROOT / "target" / "image-fixture-suite" / f"{time.time_ns()}-{os.getpid()}-image-suite.local.json"
        written = aex_image_fixture_suite.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_image_fixture_suite.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_image_fixture_suite.write_json_create_new(LAB_ROOT / "target" / "outside-image-suite.json", payload)


if __name__ == "__main__":
    unittest.main()
