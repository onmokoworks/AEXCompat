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


aex_candidate_matrix = load_tool("aex_candidate_matrix")


def entry(relative_path: str, **overrides) -> dict:
    base = {
        "relative_path": relative_path,
        "file_name": Path(relative_path).name,
        "size_bytes": 200000,
        "mtime_utc": "2026-01-01T00:00:00+00:00",
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "pipl_signal_present": True,
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "fixture_candidate_reasons": ["pe-valid", "x64", "dll-image", "pipl-signal", "EffectMain-export"],
        "markers": {"effect_main_marker_present": True, "ae_plugin_marker_count": 0},
        "pe": {
            "machine_label": "x64",
            "characteristics_flags": {"dll": True},
            "export_summary": {"effect_main_export_present": True},
            "import_summary": {"dll_names": ["KERNEL32.dll", "VCRUNTIME140.dll"]},
            "resource_summary": {
                "type_details": [{"type": "#16", "entry_count": 1}, {"type": "PIPL", "entry_count": 1}],
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 12,
                "pipl_resource_entries": [
                    {
                        "type": "PIPL",
                        "name": 16000,
                        "language": 1033,
                        "data_rva": 0x1300,
                        "size_bytes": 12,
                        "codepage": 0,
                        "reserved": 0,
                    }
                ],
            },
        },
    }
    base.update(overrides)
    return base


def make_report() -> dict:
    entries = [
        entry("AEPluginBuild\\ScatterMap.aex"),
        entry(
            "AEPluginBuild\\MaskOffset.aex",
            compatibility_class="classic_pf_effect_with_aegp_markers",
            markers={"effect_main_marker_present": True, "ae_plugin_marker_count": 3},
            fixture_candidate_score=75,
        ),
        entry(
            "AEPluginBuild\\GpuThing.aex",
            pe={
                "machine_label": "x64",
                "characteristics_flags": {"dll": True},
                "export_summary": {"effect_main_export_present": True},
                "import_summary": {"dll_names": ["KERNEL32.dll", "OPENGL32.dll", "ucrtbased.dll"]},
                "resource_summary": {
                    "type_details": [{"type": "PIPL", "entry_count": 1}],
                    "pipl_resource_data_entry_count": 1,
                    "pipl_resource_total_size": 20,
                    "pipl_resource_entries": [{"type": "PIPL", "name": 16000, "language": 1033, "size_bytes": 20}],
                },
            },
        ),
    ]
    return {
        "schema_version": 3,
        "publication_status": "local-only",
        "report_kind": "aex_static_probe",
        "summary": {"aex_count": len(entries)},
        "entries": entries,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
    }


class AexCandidateMatrixTests(unittest.TestCase):
    def test_matrix_classifies_primary_hold_and_dependency_buckets(self):
        matrix = aex_candidate_matrix.build_candidate_matrix(make_report(), Path("report.json"))
        self.assertEqual(matrix["report_kind"], "aex_candidate_matrix")
        self.assertEqual(matrix["matrix_state"], "candidate_matrix_ready")
        self.assertFalse(matrix["native_load_performed"])
        self.assertFalse(matrix["aex_file_opened"])

        by_path = {row["relative_path"]: row for row in matrix["rows"]}
        self.assertEqual(by_path["AEPluginBuild\\ScatterMap.aex"]["review_bucket"], "primary_fixture_candidate")
        self.assertEqual(
            by_path["AEPluginBuild\\MaskOffset.aex"]["review_bucket"],
            "hold_for_host_contract_review",
        )
        self.assertIn("aegp_markers_present", by_path["AEPluginBuild\\MaskOffset.aex"]["risk_flags"])
        self.assertEqual(
            by_path["AEPluginBuild\\GpuThing.aex"]["review_bucket"],
            "dependency_or_environment_review",
        )
        self.assertIn("debug_runtime_imports_present", by_path["AEPluginBuild\\GpuThing.aex"]["risk_flags"])
        self.assertIn("graphics_or_gpu_imports_present", by_path["AEPluginBuild\\GpuThing.aex"]["risk_flags"])
        self.assertEqual(matrix["summary"]["primary_candidate_count"], 1)

    def test_invalid_or_unsafe_source_report_is_rejected(self):
        report = make_report()
        report["schema_version"] = 2
        with self.assertRaises(ValueError):
            aex_candidate_matrix.build_candidate_matrix(report, Path("report.json"))

        report = make_report()
        report["entries"][0]["native_load_performed"] = True
        with self.assertRaises(ValueError):
            aex_candidate_matrix.build_candidate_matrix(report, Path("report.json"))

    def test_paths_are_confined_and_output_is_create_new(self):
        report_root = LAB_ROOT / "target" / "aex-static-probe"
        report_root.mkdir(parents=True, exist_ok=True)
        source = report_root / f"{time.time_ns()}-{os.getpid()}-candidate-source.local.json"
        source.write_text(json.dumps(make_report()), encoding="utf-8")
        loaded, resolved = aex_candidate_matrix.load_static_report(source)
        self.assertEqual(loaded["report_kind"], "aex_static_probe")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-candidate.json"
        outside.write_text(json.dumps(make_report()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_candidate_matrix.load_static_report(outside)

        payload = aex_candidate_matrix.build_candidate_matrix(loaded, resolved)
        out = LAB_ROOT / "target" / "candidate-matrix" / f"{time.time_ns()}-{os.getpid()}-candidate-matrix.local.json"
        written = aex_candidate_matrix.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_candidate_matrix.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_candidate_matrix.write_json_create_new(LAB_ROOT / "target" / "outside-candidate-matrix.json", payload)


if __name__ == "__main__":
    unittest.main()
