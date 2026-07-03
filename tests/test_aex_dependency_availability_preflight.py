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


aex_dependency_availability_preflight = load_tool("aex_dependency_availability_preflight")


def make_dependency_matrix() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_dependency_matrix",
        "dependency_matrix_state": "dependency_matrix_ready",
        "availability_check": "not_performed",
        "summary": {"candidate_count": 3},
        "dependency_rows": [
            {
                "dll_name": "kernel32.dll",
                "category": "core_windows",
                "candidate_count": 3,
                "review_buckets": ["primary_fixture_candidate"],
                "dependency_risk_flags": [],
                "example_candidates": ["AEPluginBuild\\ScatterMap.aex"],
            },
            {
                "dll_name": "api-ms-win-crt-runtime-l1-1-0.dll",
                "category": "windows_crt_api_set",
                "candidate_count": 2,
                "review_buckets": ["primary_fixture_candidate"],
                "dependency_risk_flags": [],
                "example_candidates": ["AEPluginBuild\\ScatterMap.aex"],
            },
            {
                "dll_name": "custom_runtime.dll",
                "category": "manual_review",
                "candidate_count": 1,
                "review_buckets": ["dependency_or_environment_review"],
                "dependency_risk_flags": ["unknown_dependency_review"],
                "example_candidates": ["AEPluginBuild\\Custom.aex"],
            },
            {
                "dll_name": "ucrtbased.dll",
                "category": "debug_crt_runtime",
                "candidate_count": 1,
                "review_buckets": ["dependency_or_environment_review"],
                "dependency_risk_flags": ["debug_runtime_dependency"],
                "example_candidates": ["AEPluginBuild\\Debug.aex"],
            },
        ],
        "candidate_rows": [],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexDependencyAvailabilityPreflightTests(unittest.TestCase):
    def test_preflight_checks_filesystem_existence_without_load(self):
        search_dir = LAB_ROOT / "target" / "test-inputs" / f"{time.time_ns()}-dependency-search"
        search_dir.mkdir(parents=True, exist_ok=True)
        (search_dir / "kernel32.dll").write_bytes(b"MZ")
        (search_dir / "ucrtbased.dll").write_bytes(b"MZ")

        report = aex_dependency_availability_preflight.build_dependency_preflight(
            make_dependency_matrix(),
            Path("dependency.json"),
            [search_dir],
        )

        self.assertEqual(report["report_kind"], "aex_dependency_availability_preflight")
        self.assertEqual(report["preflight_state"], "dependency_availability_preflight_ready_no_load")
        self.assertEqual(report["availability_check"], "filesystem_exists_only_no_load")
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["aex_file_opened"])

        by_dll = {row["dll_name"]: row for row in report["dependency_rows"]}
        self.assertEqual(by_dll["kernel32.dll"]["availability_status"], "found_in_search_path")
        self.assertEqual(
            by_dll["api-ms-win-crt-runtime-l1-1-0.dll"]["availability_status"],
            "api_set_virtual_or_not_found_review",
        )
        self.assertEqual(by_dll["custom_runtime.dll"]["availability_status"], "not_found_needs_review")
        self.assertEqual(by_dll["ucrtbased.dll"]["availability_status"], "found_in_search_path")
        self.assertEqual(by_dll["ucrtbased.dll"]["policy_review_state"], "default_deny_dependency")

        summary = report["summary"]
        self.assertEqual(summary["unique_dependency_count"], 4)
        self.assertEqual(summary["found_count"], 2)
        self.assertEqual(summary["api_set_virtual_or_not_found_review_count"], 1)
        self.assertEqual(summary["not_found_needs_review_count"], 1)
        self.assertEqual(summary["default_deny_dependency_count"], 1)

    def test_invalid_or_unsafe_source_matrix_is_rejected(self):
        matrix = make_dependency_matrix()
        matrix["availability_check"] = "filesystem_exists_only_no_load"
        with self.assertRaises(ValueError):
            aex_dependency_availability_preflight.build_dependency_preflight(matrix, Path("dependency.json"), [])

        matrix = make_dependency_matrix()
        matrix["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_dependency_availability_preflight.build_dependency_preflight(matrix, Path("dependency.json"), [])

        matrix = make_dependency_matrix()
        matrix["dependency_rows"][0]["dll_name"] = "..\\escape.dll"
        with self.assertRaises(ValueError):
            aex_dependency_availability_preflight.build_dependency_preflight(matrix, Path("dependency.json"), [])

    def test_paths_are_confined_and_output_is_create_new(self):
        matrix_root = LAB_ROOT / "target" / "dependency-matrix"
        matrix_root.mkdir(parents=True, exist_ok=True)
        source = matrix_root / f"{time.time_ns()}-dependency-preflight-source.local.json"
        source.write_text(json.dumps(make_dependency_matrix()), encoding="utf-8")
        loaded, resolved = aex_dependency_availability_preflight.load_dependency_matrix(source)
        self.assertEqual(loaded["report_kind"], "aex_dependency_matrix")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-preflight.json"
        outside.write_text(json.dumps(make_dependency_matrix()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_dependency_availability_preflight.load_dependency_matrix(outside)

        payload = aex_dependency_availability_preflight.build_dependency_preflight(loaded, resolved, [])
        out = LAB_ROOT / "target" / "dependency-preflight" / f"{time.time_ns()}-dependency-preflight.local.json"
        written = aex_dependency_availability_preflight.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_dependency_availability_preflight.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_dependency_availability_preflight.write_json_create_new(
                LAB_ROOT / "target" / "outside-preflight.json",
                payload,
            )


if __name__ == "__main__":
    unittest.main()
