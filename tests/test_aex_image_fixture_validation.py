import importlib.util
import json
import sys
import time
import unittest
import uuid
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


aex_image_fixture_validation = load_tool("aex_image_fixture_validation")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


def create_ppm(name: str, width: int, height: int, pattern: str) -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    # Windows' wall-clock resolution can return the same time_ns() value for
    # consecutive calls, while the fixture writer intentionally uses create-new.
    path = root / f"{uuid.uuid4().hex}-{name}.ppm"
    image = ppm_fixture_tool.generate_image(width, height, pattern)
    ppm_fixture_tool.write_ppm_create_new(path, image)
    return path


def make_suite(width: int = 5, height: int = 4) -> dict:
    first = create_ppm("validation-first", width, height, "gradient")
    second = create_ppm("validation-second", 7, 3, "checker")
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_suite",
        "suite_state": "image_fixture_suite_ready",
        "target_candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
        "fixtures": [
            {
                "case_id": "first",
                "pattern": "gradient",
                "width": width,
                "height": height,
                "pixel_bytes": width * height * 3,
                "ppm_path": str(first),
            },
            {
                "case_id": "second",
                "pattern": "checker",
                "width": 7,
                "height": 3,
                "pixel_bytes": 7 * 3 * 3,
                "ppm_path": str(second),
            },
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexImageFixtureValidationTests(unittest.TestCase):
    def test_cli_exit_code_matches_validation_state_and_writes_report(self):
        suite = make_suite()
        suite_path = LAB_ROOT / "target" / "image-fixture-suite" / f"{time.time_ns()}-cli-suite.local.json"
        suite_path.parent.mkdir(parents=True, exist_ok=True)
        suite_path.write_text(json.dumps(suite), encoding="utf-8")
        original = aex_image_fixture_validation.build_validation_report

        for failed in (False, True):
            with self.subTest(failed=failed):
                def build_report(source, path):
                    mutated = json.loads(json.dumps(source))
                    if failed:
                        mutated["fixtures"][0]["width"] = 999999
                    return original(mutated, path)

                output = LAB_ROOT / "target" / "image-fixture-validation" / f"{time.time_ns()}-cli.local.json"
                argv = [
                    "aex_image_fixture_validation.py",
                    "--image-suite", str(suite_path),
                    "--out", str(output),
                ]
                with mock.patch.object(
                    aex_image_fixture_validation,
                    "build_validation_report",
                    side_effect=build_report,
                ), mock.patch.object(sys, "argv", argv):
                    exit_code = aex_image_fixture_validation.main()
                report = json.loads(output.read_text(encoding="utf-8"))
                self.assertEqual(report["validation_passed"], not failed)
                self.assertEqual(
                    report["validation_state"],
                    "image_fixture_validation_failed" if failed
                    else "image_fixture_validation_passed_no_load",
                )
                self.assertEqual(exit_code, 1 if failed else 0)

    def test_fixture_paths_are_unique_even_for_repeated_names(self):
        first = create_ppm("same-name", 2, 2, "checker")
        second = create_ppm("same-name", 2, 2, "checker")
        self.assertNotEqual(first, second)

    def test_validation_passes_generated_ppm_suite_and_records_hashes(self):
        report = aex_image_fixture_validation.build_validation_report(make_suite(), Path("suite.json"))
        self.assertEqual(report["report_kind"], "aex_image_fixture_validation")
        self.assertEqual(report["validation_state"], "image_fixture_validation_passed_no_load")
        self.assertTrue(report["validation_passed"])
        self.assertEqual(report["summary"]["fixture_count"], 2)
        self.assertEqual(report["summary"]["failed_count"], 0)
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        for row in report["fixture_results"]:
            self.assertEqual(row["validation_status"], "passed")
            self.assertEqual(len(row["pixel_sha256"]), 64)
            self.assertEqual(len(row["file_sha256"]), 64)

    def test_manifest_mismatch_is_reported_without_opening_runtime(self):
        suite = make_suite()
        suite["fixtures"][0]["width"] = 99
        report = aex_image_fixture_validation.build_validation_report(suite, Path("suite.json"))
        self.assertEqual(report["validation_state"], "image_fixture_validation_failed")
        self.assertFalse(report["validation_passed"])
        self.assertIn("first: manifest width does not match PPM width", report["errors"])
        self.assertFalse(report["native_load_performed"])

    def test_invalid_or_unsafe_suite_is_rejected(self):
        suite = make_suite()
        suite["suite_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_image_fixture_validation.build_validation_report(suite, Path("suite.json"))

        suite = make_suite()
        suite["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_image_fixture_validation.build_validation_report(suite, Path("suite.json"))

    def test_paths_are_confined_and_output_is_create_new(self):
        suite_root = LAB_ROOT / "target" / "image-fixture-suite"
        suite_root.mkdir(parents=True, exist_ok=True)
        source = suite_root / f"{time.time_ns()}-validation-suite.local.json"
        source.write_text(json.dumps(make_suite()), encoding="utf-8")
        loaded, resolved = aex_image_fixture_validation.load_image_suite(source)
        self.assertEqual(loaded["report_kind"], "aex_image_fixture_suite")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-validation-suite.json"
        outside.write_text(json.dumps(make_suite()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_image_fixture_validation.load_image_suite(outside)

        payload = aex_image_fixture_validation.build_validation_report(loaded, resolved)
        out = LAB_ROOT / "target" / "image-fixture-validation" / f"{time.time_ns()}-validation.local.json"
        written = aex_image_fixture_validation.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_image_fixture_validation.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_image_fixture_validation.write_json_create_new(
                LAB_ROOT / "target" / "outside-validation.json",
                payload,
            )


if __name__ == "__main__":
    unittest.main()
