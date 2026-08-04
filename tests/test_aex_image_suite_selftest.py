import importlib.util
import json
import os
import sys
import time
import unittest
import uuid
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_image_suite_selftest = load_tool("aex_image_suite_selftest")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


def create_ppm(name: str, width: int, height: int, pattern: str) -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{uuid.uuid4().hex}-{name}.ppm"
    image = ppm_fixture_tool.generate_image(width, height, pattern)
    ppm_fixture_tool.write_ppm_create_new(path, image)
    return path


def make_suite() -> dict:
    first = create_ppm("suite-first", 5, 4, "gradient")
    second = create_ppm("suite-second", 7, 3, "checker")
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_suite",
        "suite_state": "image_fixture_suite_ready",
        "target_candidate": {
            "relative_path": "AEPluginBuild\\ScatterMap.aex",
            "candidate_policy_state": "eligible_for_manual_policy_review",
            "native_load_approval": "not_granted",
        },
        "fixtures": [
            {"case_id": "first", "pattern": "gradient", "ppm_path": str(first)},
            {"case_id": "second", "pattern": "checker", "ppm_path": str(second)},
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexImageSuiteSelftestTests(unittest.TestCase):
    def test_suite_selftest_spawns_worker_for_all_fixtures(self):
        suite = make_suite()
        report = aex_image_suite_selftest.run_suite_selftest(
            suite=suite,
            suite_path=Path("suite.json"),
            worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
            output_prefix=f"{time.time_ns()}-{os.getpid()}-suite-selftest",
        )
        self.assertEqual(report["report_kind"], "aex_image_suite_worker_selftest")
        self.assertEqual(report["suite_selftest_state"], "image_suite_worker_selftest_passed")
        self.assertEqual(report["fixture_count"], 2)
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["render_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertEqual([step["step"] for step in report["steps"]], [
            "hello",
            "inspect_environment",
            "blocked_load_aex",
            "quit",
        ])
        for result in report["fixture_results"]:
            self.assertTrue(Path(result["output_ppm"]).exists())
            self.assertTrue(result["identity_check"]["pixel_match"])
            self.assertTrue(result["identity_check"]["dimension_match"])

    def test_invalid_or_unsafe_suite_is_rejected(self):
        suite = make_suite()
        suite["suite_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_image_suite_selftest.run_suite_selftest(
                suite=suite,
                suite_path=Path("suite.json"),
                worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                output_prefix=f"{time.time_ns()}-{os.getpid()}-bad",
            )

        suite = make_suite()
        suite["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_image_suite_selftest.run_suite_selftest(
                suite=suite,
                suite_path=Path("suite.json"),
                worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                output_prefix=f"{time.time_ns()}-{os.getpid()}-bad",
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        suite_root = LAB_ROOT / "target" / "image-fixture-suite"
        suite_root.mkdir(parents=True, exist_ok=True)
        source = suite_root / f"{time.time_ns()}-{os.getpid()}-suite-source.local.json"
        source.write_text(json.dumps(make_suite()), encoding="utf-8")
        loaded, resolved = aex_image_suite_selftest.load_image_suite(source)
        self.assertEqual(loaded["report_kind"], "aex_image_fixture_suite")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-suite.json"
        outside.write_text(json.dumps(make_suite()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_image_suite_selftest.load_image_suite(outside)

        payload = {
            "schema_version": 1,
            "report_kind": "aex_image_suite_worker_selftest",
            "native_load_performed": False,
        }
        out = LAB_ROOT / "target" / "image-suite-selftest" / f"{time.time_ns()}-{os.getpid()}-suite-selftest.local.json"
        written = aex_image_suite_selftest.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_image_suite_selftest.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_image_suite_selftest.write_json_create_new(LAB_ROOT / "target" / "outside-suite-selftest.json", payload)


if __name__ == "__main__":
    unittest.main()
