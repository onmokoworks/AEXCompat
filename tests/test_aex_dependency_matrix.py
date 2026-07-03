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


aex_dependency_matrix = load_tool("aex_dependency_matrix")


def make_candidate_matrix() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_matrix",
        "matrix_state": "candidate_matrix_ready",
        "rows": [
            {
                "relative_path": "AEPluginBuild\\ScatterMap.aex",
                "review_bucket": "primary_fixture_candidate",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 95,
                "import_dll_names": [
                    "api-ms-win-crt-runtime-l1-1-0.dll",
                    "KERNEL32.dll",
                    "VCRUNTIME140.dll",
                ],
            },
            {
                "relative_path": "AEPluginBuild\\GpuDebug.aex",
                "review_bucket": "dependency_or_environment_review",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 83,
                "import_dll_names": [
                    "OPENGL32.dll",
                    "gdi32.dll",
                    "oleaut32.dll",
                    "ucrtbased.dll",
                    "custom_runtime.dll",
                ],
            },
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexDependencyMatrixTests(unittest.TestCase):
    def test_dependency_matrix_classifies_dlls_and_candidate_risks(self):
        matrix = aex_dependency_matrix.build_dependency_matrix(make_candidate_matrix(), Path("candidate.json"))
        self.assertEqual(matrix["report_kind"], "aex_dependency_matrix")
        self.assertEqual(matrix["dependency_matrix_state"], "dependency_matrix_ready")
        self.assertEqual(matrix["availability_check"], "not_performed")
        self.assertFalse(matrix["native_load_performed"])
        self.assertFalse(matrix["aex_file_opened"])

        by_dll = {row["dll_name"]: row for row in matrix["dependency_rows"]}
        self.assertEqual(by_dll["kernel32.dll"]["category"], "core_windows")
        self.assertEqual(by_dll["api-ms-win-crt-runtime-l1-1-0.dll"]["category"], "windows_crt_api_set")
        self.assertEqual(by_dll["vcruntime140.dll"]["category"], "release_crt_runtime")
        self.assertEqual(by_dll["opengl32.dll"]["category"], "graphics_or_gpu")
        self.assertEqual(by_dll["gdi32.dll"]["category"], "windows_gui")
        self.assertEqual(by_dll["oleaut32.dll"]["category"], "com_ole")
        self.assertEqual(by_dll["ucrtbased.dll"]["category"], "debug_crt_runtime")
        self.assertEqual(by_dll["custom_runtime.dll"]["category"], "manual_review")

        by_candidate = {row["relative_path"]: row for row in matrix["candidate_rows"]}
        self.assertEqual(by_candidate["AEPluginBuild\\ScatterMap.aex"]["dependency_risk_flags"], [])
        risky = by_candidate["AEPluginBuild\\GpuDebug.aex"]["dependency_risk_flags"]
        self.assertIn("debug_runtime_dependency", risky)
        self.assertIn("graphics_or_gpu_dependency", risky)
        self.assertIn("gui_dependency", risky)
        self.assertIn("com_or_ole_dependency", risky)
        self.assertIn("unknown_dependency_review", risky)

    def test_invalid_or_unsafe_source_matrix_is_rejected(self):
        matrix = make_candidate_matrix()
        matrix["matrix_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_dependency_matrix.build_dependency_matrix(matrix, Path("candidate.json"))

        matrix = make_candidate_matrix()
        matrix["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_dependency_matrix.build_dependency_matrix(matrix, Path("candidate.json"))

    def test_paths_are_confined_and_output_is_create_new(self):
        matrix_root = LAB_ROOT / "target" / "candidate-matrix"
        matrix_root.mkdir(parents=True, exist_ok=True)
        source = matrix_root / f"{time.time_ns()}-dependency-source.local.json"
        source.write_text(json.dumps(make_candidate_matrix()), encoding="utf-8")
        loaded, resolved = aex_dependency_matrix.load_candidate_matrix(source)
        self.assertEqual(loaded["report_kind"], "aex_candidate_matrix")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-dependency.json"
        outside.write_text(json.dumps(make_candidate_matrix()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_dependency_matrix.load_candidate_matrix(outside)

        payload = aex_dependency_matrix.build_dependency_matrix(loaded, resolved)
        out = LAB_ROOT / "target" / "dependency-matrix" / f"{time.time_ns()}-dependency.local.json"
        written = aex_dependency_matrix.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_dependency_matrix.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_dependency_matrix.write_json_create_new(LAB_ROOT / "target" / "outside-dependency.json", payload)


if __name__ == "__main__":
    unittest.main()
