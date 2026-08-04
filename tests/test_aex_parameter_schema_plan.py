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


aex_parameter_schema_plan = load_tool("aex_parameter_schema_plan")


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
                "file_name": "ScatterMap.aex",
                "metadata_state": "pipl_resource_metadata_ready_no_payload",
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 326,
                "pipl_resource_entries": [
                    {"name": 16000, "language": 1033, "data_rva": 4096, "size_bytes": 326, "codepage": 0}
                ],
                "effect_main_export_present": True,
                "aegp_marker_count": 0,
            },
            {
                "relative_path": "AEPluginBuild\\Hosty.aex",
                "file_name": "Hosty.aex",
                "metadata_state": "pipl_resource_metadata_ready_no_payload",
                "pipl_resource_data_entry_count": 1,
                "pipl_resource_total_size": 256,
                "pipl_resource_entries": [
                    {"name": 16000, "language": 1033, "data_rva": 8192, "size_bytes": 256, "codepage": 0}
                ],
                "effect_main_export_present": True,
                "aegp_marker_count": 2,
            },
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "dll_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_candidate_matrix() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_matrix",
        "matrix_state": "candidate_matrix_ready",
        "rows": [
            {
                "relative_path": "AEPluginBuild\\ScatterMap.aex",
                "file_name": "ScatterMap.aex",
                "review_bucket": "primary_fixture_candidate",
                "compatibility_class": "classic_pf_effect_candidate",
                "fixture_candidate_score": 95,
                "risk_flags": [],
            },
            {
                "relative_path": "AEPluginBuild\\Hosty.aex",
                "file_name": "Hosty.aex",
                "review_bucket": "hold_for_host_contract_review",
                "compatibility_class": "classic_pf_effect_with_aegp_markers",
                "fixture_candidate_score": 70,
                "risk_flags": ["aegp_markers_present"],
            },
        ],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_render_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_render_validation_contract",
        "contract_state": "render_validation_contract_ready_render_closed",
        "real_render_open": False,
        "no_load_validation_ready": True,
        "blockers": ["load_gate_closed", "no_aex_parameter_schema_mapping", "ofx_route_closed"],
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
    }


def build_plan(**overrides):
    values = {
        "pipl_catalog": make_pipl_catalog(),
        "candidate_matrix": make_candidate_matrix(),
        "render_contract": make_render_contract(),
    }
    values.update(overrides)
    return aex_parameter_schema_plan.build_parameter_schema_plan(
        pipl_catalog=values["pipl_catalog"],
        pipl_catalog_path=Path("catalog.json"),
        candidate_matrix=values["candidate_matrix"],
        candidate_matrix_path=Path("matrix.json"),
        render_contract=values["render_contract"],
        render_contract_path=Path("render.json"),
    )


class AexParameterSchemaPlanTests(unittest.TestCase):
    def test_plan_ready_without_payload_or_real_schema(self):
        plan = build_plan()
        self.assertEqual(plan["report_kind"], "aex_parameter_schema_plan")
        self.assertEqual(plan["plan_state"], "parameter_schema_plan_ready_no_payload")
        self.assertTrue(plan["schema_plan_ready"])
        self.assertFalse(plan["real_parameter_schema_available"])
        self.assertFalse(plan["payload_parser_enabled"])
        self.assertFalse(plan["pipl_payload_parsed"])
        self.assertFalse(plan["parameter_schema_emitted"])
        self.assertFalse(plan["ofx_describe_performed"])
        self.assertFalse(plan["render_performed"])
        self.assertEqual(plan["summary"]["candidate_count"], 2)
        self.assertEqual(plan["summary"]["primary_mapping_candidate_count"], 1)
        self.assertEqual(plan["summary"]["host_contract_review_count"], 1)
        primary = plan["candidate_schema_rows"][0]
        self.assertEqual(primary["mapping_state"], "primary_schema_mapping_candidate_pending_payload_parser")
        self.assertEqual(primary["payload_policy"], "do_not_parse_or_copy_pipl_payload")
        self.assertEqual(primary["schema_output_state"], "not_emitted_no_payload_parser")
        self.assertNotIn("payload", primary["pipl_resource_entries"][0])
        self.assertFalse(plan["render_contract_link"]["this_plan_resolves_blocker"])

    def test_invalid_sources_are_rejected(self):
        catalog = make_pipl_catalog()
        catalog["resource_payload_extracted"] = True
        with self.assertRaises(ValueError):
            build_plan(pipl_catalog=catalog)

        matrix = make_candidate_matrix()
        matrix["matrix_state"] = "stale"
        with self.assertRaises(ValueError):
            build_plan(candidate_matrix=matrix)

        render = make_render_contract()
        render["blockers"] = ["load_gate_closed"]
        with self.assertRaises(ValueError):
            build_plan(render_contract=render)

    def test_paths_are_confined_and_report_is_create_new(self):
        roots = {
            "catalog": LAB_ROOT / "target" / "pipl-resource-catalog",
            "matrix": LAB_ROOT / "target" / "candidate-matrix",
            "render": LAB_ROOT / "target" / "render-validation-contract",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        paths = {
            "catalog": roots["catalog"] / f"{stamp}-schema-catalog.local.json",
            "matrix": roots["matrix"] / f"{stamp}-schema-matrix.local.json",
            "render": roots["render"] / f"{stamp}-schema-render.local.json",
        }
        payloads = {
            "catalog": make_pipl_catalog(),
            "matrix": make_candidate_matrix(),
            "render": make_render_contract(),
        }
        for label, path in paths.items():
            path.write_text(json.dumps(payloads[label]), encoding="utf-8")

        catalog, catalog_path = aex_parameter_schema_plan.load_pipl_catalog(paths["catalog"])
        matrix, matrix_path = aex_parameter_schema_plan.load_candidate_matrix(paths["matrix"])
        render, render_path = aex_parameter_schema_plan.load_render_contract(paths["render"])
        self.assertEqual(catalog_path, paths["catalog"].resolve())
        self.assertEqual(matrix_path, paths["matrix"].resolve())
        self.assertEqual(render_path, paths["render"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-schema.json"
        outside.write_text(json.dumps(make_pipl_catalog()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_parameter_schema_plan.load_pipl_catalog(outside)

        plan = aex_parameter_schema_plan.build_parameter_schema_plan(
            pipl_catalog=catalog,
            pipl_catalog_path=catalog_path,
            candidate_matrix=matrix,
            candidate_matrix_path=matrix_path,
            render_contract=render,
            render_contract_path=render_path,
        )
        out = LAB_ROOT / "target" / "parameter-schema-plan" / f"{time.time_ns()}-{os.getpid()}-schema-plan.local.json"
        written = aex_parameter_schema_plan.write_json_create_new(out, plan)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_parameter_schema_plan.write_json_create_new(out, plan)
        with self.assertRaises(ValueError):
            aex_parameter_schema_plan.write_json_create_new(LAB_ROOT / "target" / "outside-schema.json", plan)


if __name__ == "__main__":
    unittest.main()
