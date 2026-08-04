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


aex_pipl_resource_catalog = load_tool("aex_pipl_resource_catalog")


def make_entry(name: str = "Synthetic.aex") -> dict:
    return {
        "relative_path": name,
        "file_name": Path(name).name,
        "size_bytes": 4096,
        "compatibility_class": "classic_pf_effect_candidate",
        "fixture_candidate_score": 95,
        "pipl_signal_present": True,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "markers": {
            "pipl_ascii_marker_present": False,
            "pipl_ascii_marker_count": 0,
            "effect_main_marker_present": True,
            "effect_main_marker_count": 1,
            "ae_plugin_marker_count": 0,
        },
        "pe": {
            "machine_label": "x64",
            "section_count": 2,
            "export_summary": {"effect_main_export_present": True},
            "import_summary": {"dll_count": 1},
            "resource_summary": {
                "resource_dir_present": True,
                "resource_parse_truncated": False,
                "type_count": 2,
                "type_details": [{"type": "PIPL", "entry_count": 1}, {"type": "#16", "entry_count": 1}],
                "resource_data_entry_count": 2,
                "pipl_resource_type_present": True,
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 326,
                "pipl_resource_entries": [
                    {
                        "type": "PIPL",
                        "name": 16000,
                        "language": 1033,
                        "data_rva": 213616,
                        "size_bytes": 326,
                        "codepage": 0,
                        "reserved": 0,
                    }
                ],
            },
        },
    }


def make_static_report() -> dict:
    entries = [make_entry("One.aex"), make_entry("Two.aex")]
    entries[1]["pe"]["resource_summary"]["pipl_resource_total_size"] = 512
    entries[1]["pe"]["resource_summary"]["pipl_resource_entries"][0]["size_bytes"] = 512
    return {
        "schema_version": 3,
        "publication_status": "local-only",
        "report_kind": "aex_static_probe",
        "summary": {
            "aex_count": 2,
            "pipl_resource_entry_count": 2,
            "pipl_resource_total_size": 838,
            "effect_main_export_count": 2,
        },
        "entries": entries,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
    }


class AexPiplResourceCatalogTests(unittest.TestCase):
    def test_catalog_summarizes_pipl_metadata_without_payload(self):
        catalog = aex_pipl_resource_catalog.build_catalog(make_static_report(), Path("static.json"))
        self.assertEqual(catalog["report_kind"], "aex_pipl_resource_catalog")
        self.assertEqual(catalog["catalog_state"], "pipl_resource_catalog_ready_no_payload")
        self.assertEqual(catalog["payload_policy"], "metadata_only_no_resource_payload")
        self.assertFalse(catalog["native_load_performed"])
        self.assertFalse(catalog["dll_load_performed"])
        self.assertFalse(catalog["aex_file_opened"])
        self.assertFalse(catalog["resource_payload_extracted"])
        self.assertEqual(catalog["summary"]["plugin_count"], 2)
        self.assertEqual(catalog["summary"]["pipl_resource_entry_count"], 2)
        self.assertEqual(catalog["summary"]["pipl_resource_total_size"], 838)
        self.assertEqual(catalog["summary"]["pipl_resource_metadata_ready_count"], 2)
        self.assertEqual(catalog["summary"]["resource_type_counts"]["PIPL"], 2)
        row = catalog["rows"][0]
        self.assertEqual(row["metadata_state"], "pipl_resource_metadata_ready_no_payload")
        self.assertEqual(row["pipl_resource_entries"][0]["name"], 16000)
        self.assertNotIn("payload", row["pipl_resource_entries"][0])

    def test_invalid_static_report_or_safety_flag_is_rejected(self):
        report = make_static_report()
        report["schema_version"] = 2
        with self.assertRaises(ValueError):
            aex_pipl_resource_catalog.build_catalog(report, Path("static.json"))

        report = make_static_report()
        report["entries"][0]["private_payload_copied"] = True
        with self.assertRaises(ValueError):
            aex_pipl_resource_catalog.build_catalog(report, Path("static.json"))

    def test_paths_are_confined_and_report_is_create_new(self):
        static_root = LAB_ROOT / "target" / "aex-static-probe"
        static_root.mkdir(parents=True, exist_ok=True)
        static_path = static_root / f"{time.time_ns()}-{os.getpid()}-static.local.json"
        static_path.write_text(json.dumps(make_static_report()), encoding="utf-8")
        report, resolved = aex_pipl_resource_catalog.load_static_report(static_path)
        self.assertEqual(report["report_kind"], "aex_static_probe")
        self.assertEqual(resolved, static_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-static.json"
        outside.write_text(json.dumps(make_static_report()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_pipl_resource_catalog.load_static_report(outside)

        catalog = aex_pipl_resource_catalog.build_catalog(report, resolved)
        out = LAB_ROOT / "target" / "pipl-resource-catalog" / f"{time.time_ns()}-{os.getpid()}-catalog.local.json"
        written = aex_pipl_resource_catalog.write_json_create_new(out, catalog)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_pipl_resource_catalog.write_json_create_new(out, catalog)
        with self.assertRaises(ValueError):
            aex_pipl_resource_catalog.write_json_create_new(LAB_ROOT / "target" / "outside-catalog.json", catalog)


if __name__ == "__main__":
    unittest.main()
