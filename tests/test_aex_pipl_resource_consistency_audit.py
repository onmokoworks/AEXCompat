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
aex_pipl_parser_gate = load_tool("aex_pipl_parser_gate")
aex_pipl_resource_consistency_audit = load_tool("aex_pipl_resource_consistency_audit")


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


def make_synthetic_selftest() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_synthetic_pipl_parser_selftest",
        "selftest_state": "synthetic_pipl_parser_selftest_passed_no_real_payload",
        "synthetic_parser_ready": True,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "synthetic_payloads_used": True,
        "synthetic_payloads_serialized": False,
        "raw_payload_serialized": False,
        "summary": {
            "case_count": 5,
            "passed_count": 5,
            "failed_count": 0,
            "raw_payload_serialized_count": 0,
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
        "aex_render_performed": False,
        "render_validation_performed": False,
        "pipl_payload_parsed": False,
        "parameter_schema_emitted": False,
        "redacted_schema_emitted": False,
    }


def build_sources() -> tuple[dict, Path, dict, Path, dict, Path]:
    static_path = LAB_ROOT / "target" / "aex-static-probe" / f"{time.time_ns()}-{os.getpid()}-audit-static.local.json"
    catalog_path = LAB_ROOT / "target" / "pipl-resource-catalog" / f"{time.time_ns()}-{os.getpid()}-audit-catalog.local.json"
    gate_path = LAB_ROOT / "target" / "pipl-parser-gate" / f"{time.time_ns()}-{os.getpid()}-audit-gate.local.json"
    for path in (static_path, catalog_path, gate_path):
        path.parent.mkdir(parents=True, exist_ok=True)
    static_report = make_static_report()
    catalog = aex_pipl_resource_catalog.build_catalog(static_report, static_path)
    gate = aex_pipl_parser_gate.build_parser_gate(
        pipl_catalog=catalog,
        pipl_catalog_path=catalog_path,
        synthetic_selftest=make_synthetic_selftest(),
        synthetic_selftest_path=LAB_ROOT / "target" / "synthetic-pipl-parser-selftest" / "selftest.local.json",
    )
    return static_report, static_path, catalog, catalog_path, gate, gate_path


def write_json(path: Path, payload: dict) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


class AexPiplResourceConsistencyAuditTests(unittest.TestCase):
    def test_audit_recomputes_catalog_and_gate_metadata(self):
        static_report, static_path, catalog, catalog_path, gate, gate_path = build_sources()
        report = aex_pipl_resource_consistency_audit.build_consistency_audit(
            static_report=static_report,
            static_report_path=static_path,
            pipl_catalog=catalog,
            pipl_catalog_path=catalog_path,
            pipl_parser_gate=gate,
            pipl_parser_gate_path=gate_path,
        )

        self.assertEqual(report["report_kind"], "aex_pipl_resource_consistency_audit")
        self.assertEqual(report["audit_state"], "pipl_resource_consistency_audit_passed_no_payload")
        self.assertTrue(report["audit_passed"])
        self.assertTrue(report["source_chain_valid"])
        self.assertTrue(report["catalog_summary_recomputed"])
        self.assertTrue(report["catalog_rows_recomputed"])
        self.assertTrue(report["gate_budget_rows_recomputed"])
        self.assertTrue(report["gate_summary_recomputed"])
        self.assertTrue(report["metadata_consistency_ready"])
        self.assertFalse(report["real_payload_input_allowed_now"])
        self.assertFalse(report["real_pipl_payload_parser_enabled"])
        self.assertFalse(report["real_pipl_payload_parsed"])
        self.assertFalse(report["resource_payload_opened"])
        self.assertFalse(report["resource_payload_extracted"])
        self.assertFalse(report["raw_payload_serialized"])
        self.assertFalse(report["pipl_payload_parsed"])
        self.assertFalse(report["parameter_schema_emitted"])
        self.assertEqual(report["summary"]["catalog_row_count"], 2)
        self.assertEqual(report["summary"]["gate_budget_row_count"], 2)
        self.assertEqual(report["summary"]["pipl_resource_entry_count"], 2)
        self.assertTrue(all(check["passed"] for check in report["checks"]))

    def test_source_or_summary_mismatch_is_reported_not_silently_accepted(self):
        static_report, static_path, catalog, catalog_path, gate, gate_path = build_sources()
        catalog = dict(catalog)
        catalog["source_static_report"] = str(catalog_path)
        catalog["summary"] = dict(catalog["summary"])
        catalog["summary"]["plugin_count"] = 99
        report = aex_pipl_resource_consistency_audit.build_consistency_audit(
            static_report=static_report,
            static_report_path=static_path,
            pipl_catalog=catalog,
            pipl_catalog_path=catalog_path,
            pipl_parser_gate=gate,
            pipl_parser_gate_path=gate_path,
        )

        self.assertFalse(report["audit_passed"])
        self.assertFalse(report["source_chain_valid"])
        self.assertFalse(report["catalog_summary_recomputed"])
        failed = {check["check_id"] for check in report["checks"] if not check["passed"]}
        self.assertIn("catalog_source_static_report_matches", failed)
        self.assertIn("catalog_summary_recomputed_from_static", failed)

    def test_unsafe_payload_or_gate_state_is_rejected(self):
        static_report, static_path, catalog, catalog_path, gate, gate_path = build_sources()
        catalog = dict(catalog)
        catalog["resource_payload_extracted"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_pipl_resource_consistency_audit.build_consistency_audit(
                static_report=static_report,
                static_report_path=static_path,
                pipl_catalog=catalog,
                pipl_catalog_path=catalog_path,
                pipl_parser_gate=gate,
                pipl_parser_gate_path=gate_path,
            )
        self.assertIn("resource_payload_extracted must be false", str(ctx.exception))

        gate = dict(gate)
        gate["resource_payload_opened"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_pipl_resource_consistency_audit.build_consistency_audit(
                static_report=static_report,
                static_report_path=static_path,
                pipl_catalog=build_sources()[2],
                pipl_catalog_path=catalog_path,
                pipl_parser_gate=gate,
                pipl_parser_gate_path=gate_path,
            )
        self.assertIn("resource_payload_opened must be false", str(ctx.exception))

    def test_paths_are_confined_and_report_is_create_new(self):
        static_report, static_path, catalog, catalog_path, gate, gate_path = build_sources()
        write_json(static_path, static_report)
        write_json(catalog_path, catalog)
        write_json(gate_path, gate)

        loaded_static, resolved_static = aex_pipl_resource_consistency_audit.load_static_report(static_path)
        loaded_catalog, resolved_catalog = aex_pipl_resource_consistency_audit.load_pipl_catalog(catalog_path)
        loaded_gate, resolved_gate = aex_pipl_resource_consistency_audit.load_pipl_parser_gate(gate_path)
        self.assertEqual(loaded_static["report_kind"], "aex_static_probe")
        self.assertEqual(loaded_catalog["report_kind"], "aex_pipl_resource_catalog")
        self.assertEqual(loaded_gate["report_kind"], "aex_pipl_parser_gate")
        self.assertEqual(resolved_static, static_path.resolve())
        self.assertEqual(resolved_catalog, catalog_path.resolve())
        self.assertEqual(resolved_gate, gate_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-static.local.json"
        write_json(outside, static_report)
        with self.assertRaises(ValueError):
            aex_pipl_resource_consistency_audit.load_static_report(outside)

        report = aex_pipl_resource_consistency_audit.build_consistency_audit(
            static_report=loaded_static,
            static_report_path=resolved_static,
            pipl_catalog=loaded_catalog,
            pipl_catalog_path=resolved_catalog,
            pipl_parser_gate=loaded_gate,
            pipl_parser_gate_path=resolved_gate,
        )
        out = LAB_ROOT / "target" / "pipl-resource-consistency-audit" / f"{time.time_ns()}-{os.getpid()}-audit.local.json"
        written = aex_pipl_resource_consistency_audit.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_pipl_resource_consistency_audit.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_pipl_resource_consistency_audit.write_json_create_new(
                LAB_ROOT / "target" / "outside-audit.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
