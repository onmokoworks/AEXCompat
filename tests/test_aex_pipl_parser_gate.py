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


aex_pipl_parser_gate = load_tool("aex_pipl_parser_gate")


def make_pipl_catalog() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_resource_catalog",
        "catalog_state": "pipl_resource_catalog_ready_no_payload",
        "payload_policy": "metadata_only_no_resource_payload",
        "resource_payload_extracted": False,
        "rows": [
            {
                "relative_path": "AEPluginBuild\\ScatterMap.aex",
                "metadata_state": "pipl_resource_metadata_ready_no_payload",
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 326,
                "effect_main_export_present": True,
            },
            {
                "relative_path": "AEPluginBuild\\Helper.aex",
                "metadata_state": "pipl_resource_metadata_present_effect_main_missing",
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 256,
                "effect_main_export_present": False,
            },
        ],
        "summary": {
            "plugin_count": 2,
            "pipl_resource_entry_count": 2,
            "pipl_resource_total_size": 582,
            "pipl_resource_max_size": 326,
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
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


def build_gate(**overrides):
    values = {
        "pipl_catalog": make_pipl_catalog(),
        "synthetic_selftest": make_synthetic_selftest(),
    }
    values.update(overrides)
    return aex_pipl_parser_gate.build_parser_gate(
        pipl_catalog=values["pipl_catalog"],
        pipl_catalog_path=Path("catalog.json"),
        synthetic_selftest=values["synthetic_selftest"],
        synthetic_selftest_path=Path("selftest.json"),
    )


class AexPiplParserGateTests(unittest.TestCase):
    def test_gate_ready_for_review_but_real_parser_closed(self):
        gate = build_gate()
        self.assertEqual(gate["report_kind"], "aex_pipl_parser_gate")
        self.assertEqual(gate["gate_state"], "pipl_parser_gate_closed_no_real_payload")
        self.assertTrue(gate["gate_ready_for_review"])
        self.assertTrue(gate["metadata_budget_ready"])
        self.assertFalse(gate["real_pipl_payload_parser_enabled"])
        self.assertFalse(gate["real_pipl_payload_parsed"])
        self.assertFalse(gate["resource_payload_opened"])
        self.assertFalse(gate["raw_payload_serialized"])
        self.assertFalse(gate["pipl_payload_parsed"])
        self.assertFalse(gate["parameter_schema_emitted"])
        self.assertEqual(gate["summary"]["candidate_count"], 2)
        self.assertEqual(gate["summary"]["eligible_future_parser_candidate_count"], 1)
        self.assertGreaterEqual(gate["parser_input_contract"]["proposed_real_parser_limit_bytes"], 4096)
        self.assertFalse(gate["parser_input_contract"]["real_payload_input_allowed_now"])
        self.assertTrue(all(check["passed"] for check in gate["parser_gate_checks"]))
        eligible = gate["candidate_budget_rows"][0]
        self.assertEqual(eligible["parser_gate_action"], "eligible_for_future_real_payload_parser_review")
        self.assertFalse(eligible["resource_payload_opened"])
        self.assertFalse(eligible["resource_payload_serialized"])

    def test_invalid_sources_are_rejected(self):
        catalog = make_pipl_catalog()
        catalog["resource_payload_extracted"] = True
        with self.assertRaises(ValueError):
            build_gate(pipl_catalog=catalog)

        selftest = make_synthetic_selftest()
        selftest["raw_payload_serialized"] = True
        with self.assertRaises(ValueError):
            build_gate(synthetic_selftest=selftest)

    def test_paths_are_confined_and_report_is_create_new(self):
        roots = {
            "catalog": LAB_ROOT / "target" / "pipl-resource-catalog",
            "selftest": LAB_ROOT / "target" / "synthetic-pipl-parser-selftest",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = time.time_ns()
        paths = {
            "catalog": roots["catalog"] / f"{stamp}-catalog.local.json",
            "selftest": roots["selftest"] / f"{stamp}-selftest.local.json",
        }
        paths["catalog"].write_text(json.dumps(make_pipl_catalog()), encoding="utf-8")
        paths["selftest"].write_text(json.dumps(make_synthetic_selftest()), encoding="utf-8")

        catalog, catalog_path = aex_pipl_parser_gate.load_pipl_catalog(paths["catalog"])
        selftest, selftest_path = aex_pipl_parser_gate.load_synthetic_selftest(paths["selftest"])
        self.assertEqual(catalog_path, paths["catalog"].resolve())
        self.assertEqual(selftest_path, paths["selftest"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-gate.json"
        outside.write_text(json.dumps(make_pipl_catalog()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_pipl_parser_gate.load_pipl_catalog(outside)

        gate = aex_pipl_parser_gate.build_parser_gate(
            pipl_catalog=catalog,
            pipl_catalog_path=catalog_path,
            synthetic_selftest=selftest,
            synthetic_selftest_path=selftest_path,
        )
        out = LAB_ROOT / "target" / "pipl-parser-gate" / f"{time.time_ns()}-gate.local.json"
        written = aex_pipl_parser_gate.write_json_create_new(out, gate)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_pipl_parser_gate.write_json_create_new(out, gate)
        with self.assertRaises(ValueError):
            aex_pipl_parser_gate.write_json_create_new(LAB_ROOT / "target" / "outside-gate.json", gate)


if __name__ == "__main__":
    unittest.main()
