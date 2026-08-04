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


aex_synthetic_pipl_payload_parser = load_tool("aex_synthetic_pipl_payload_parser")


SAFETY_FALSE = {
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
    "real_pipl_payload_parser_enabled": False,
    "real_pipl_payload_parsed": False,
    "raw_payload_serialized": False,
    "resource_payload_opened": False,
}


def pipl_parser_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_pipl_parser_gate",
        "gate_state": "pipl_parser_gate_closed_no_real_payload",
        "gate_ready_for_review": True,
        "metadata_budget_ready": True,
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "raw_payload_serialized": False,
        "parser_input_contract": {
            "state": "metadata_size_budget_ready",
            "real_payload_input_allowed_now": False,
            "proposed_real_parser_limit_bytes": 4096,
        },
        "parser_gate_checks": [{"check_id": "synthetic_parser_selftest_passed", "passed": True}],
        **SAFETY_FALSE,
    }


def synthetic_selftest() -> dict:
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
        "parser_contract": {
            "state": "synthetic_bounds_contract_ready",
            "raw_payload_output_allowed": False,
            "real_pipl_payload_input_allowed": False,
        },
        "case_results": [{"case_id": "valid", "passed": True, "raw_payload_serialized": False}],
        "summary": {
            "case_count": 1,
            "passed_count": 1,
            "failed_count": 0,
            "raw_payload_serialized_count": 0,
        },
        **SAFETY_FALSE,
    }


def write_json(root_name: str, name: str, payload: dict) -> Path:
    root = LAB_ROOT / "target" / root_name
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{os.getpid()}-{name}.local.json"
    path.write_text(json.dumps(payload), encoding="utf-8")
    return path


class AexSyntheticPiplPayloadParserTests(unittest.TestCase):
    def test_builds_parser_report_with_real_payload_closed(self):
        gate_path = LAB_ROOT / "target" / "pipl-parser-gate" / "gate.local.json"
        selftest_path = LAB_ROOT / "target" / "synthetic-pipl-parser-selftest" / "selftest.local.json"
        report = aex_synthetic_pipl_payload_parser.build_parser_report(
            pipl_parser_gate=pipl_parser_gate(),
            pipl_parser_gate_path=gate_path,
            synthetic_selftest=synthetic_selftest(),
            synthetic_selftest_path=selftest_path,
        )

        self.assertEqual(report["report_kind"], "aex_synthetic_pipl_payload_parser")
        self.assertEqual(
            report["synthetic_payload_parser_state"],
            "synthetic_pipl_payload_parser_ready_real_payload_closed",
        )
        self.assertTrue(report["synthetic_payload_parser_ready"])
        self.assertTrue(report["synthetic_parser_implemented"])
        self.assertTrue(report["synthetic_bounds_harness_reused"])
        self.assertTrue(report["synthetic_payload_cases_passed"])
        self.assertTrue(report["synthetic_payloads_used"])
        self.assertFalse(report["synthetic_payloads_serialized"])
        self.assertFalse(report["real_payload_input_allowed_now"])
        self.assertTrue(report["output_metadata_only"])
        self.assertFalse(report["real_pipl_payload_parser_enabled"])
        self.assertFalse(report["real_pipl_payload_parsed"])
        self.assertFalse(report["resource_payload_opened"])
        self.assertFalse(report["raw_payload_serialized"])
        self.assertFalse(report["pipl_payload_parsed"])
        self.assertFalse(report["parameter_schema_emitted"])
        self.assertEqual(report["parser_case_failed_count"], 0)
        self.assertGreater(report["parser_case_count"], 0)
        self.assertEqual(report["summary"]["parser_case_failed_count"], 0)
        self.assertGreater(report["summary"]["parser_case_count"], 0)
        self.assertIn("--payload-file", report["forbidden_cli_inputs"])
        for result in report["parser_case_results"]:
            self.assertTrue(result["passed"])
            self.assertFalse(result["raw_payload_serialized"])
            self.assertNotIn("payload", result)

    def test_parser_rejects_malformed_and_never_outputs_bytes(self):
        payload = b"SPIP\x01\x05xx"
        parsed = aex_synthetic_pipl_payload_parser.parse_synthetic_pipl_payload(payload)
        self.assertEqual(parsed["parse_state"], "rejected_malformed_synthetic_payload")
        self.assertFalse(aex_synthetic_pipl_payload_parser.contains_raw_payload_value(parsed))
        self.assertFalse(aex_synthetic_pipl_payload_parser.has_forbidden_output_keys(parsed))

    def test_rejects_open_gate_or_unsafe_selftest(self):
        gate = pipl_parser_gate()
        gate["real_pipl_payload_parser_enabled"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_synthetic_pipl_payload_parser.build_parser_report(
                pipl_parser_gate=gate,
                pipl_parser_gate_path=Path("gate.json"),
                synthetic_selftest=synthetic_selftest(),
                synthetic_selftest_path=Path("selftest.json"),
            )
        self.assertIn("real_pipl_payload_parser_enabled must be false", str(ctx.exception))

        selftest = synthetic_selftest()
        selftest["raw_payload_serialized"] = True
        with self.assertRaises(ValueError) as ctx:
            aex_synthetic_pipl_payload_parser.build_parser_report(
                pipl_parser_gate=pipl_parser_gate(),
                pipl_parser_gate_path=Path("gate.json"),
                synthetic_selftest=selftest,
                synthetic_selftest_path=Path("selftest.json"),
            )
        self.assertIn("raw payload serialized must be false", str(ctx.exception))

    def test_rejects_forbidden_cli_tokens(self):
        aex_synthetic_pipl_payload_parser.reject_forbidden_cli_inputs(["--pipl-parser-gate", "gate.json"])
        for token in ("--aex", "--pipl-payload=raw.bin", "--payload-file", "APPROVE_AEX_LOAD_GATE"):
            with self.assertRaises(ValueError):
                aex_synthetic_pipl_payload_parser.reject_forbidden_cli_inputs([token])

    def test_paths_are_confined_and_report_is_create_new(self):
        gate_path = write_json("pipl-parser-gate", "gate", pipl_parser_gate())
        selftest_path = write_json("synthetic-pipl-parser-selftest", "selftest", synthetic_selftest())
        loaded_gate, resolved_gate = aex_synthetic_pipl_payload_parser.load_pipl_parser_gate(gate_path)
        loaded_selftest, resolved_selftest = aex_synthetic_pipl_payload_parser.load_synthetic_selftest(selftest_path)
        self.assertEqual(loaded_gate["report_kind"], "aex_pipl_parser_gate")
        self.assertEqual(loaded_selftest["report_kind"], "aex_synthetic_pipl_parser_selftest")
        self.assertEqual(resolved_gate, gate_path.resolve())
        self.assertEqual(resolved_selftest, selftest_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-gate.local.json"
        outside.write_text(json.dumps(pipl_parser_gate()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_synthetic_pipl_payload_parser.load_pipl_parser_gate(outside)

        payload = {
            "schema_version": 1,
            "publication_status": "local-only",
            "report_kind": "aex_synthetic_pipl_payload_parser",
        }
        out = LAB_ROOT / "target" / "synthetic-pipl-payload-parser" / f"{time.time_ns()}-{os.getpid()}-parser.local.json"
        written = aex_synthetic_pipl_payload_parser.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_synthetic_pipl_payload_parser.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_synthetic_pipl_payload_parser.write_json_create_new(
                LAB_ROOT / "target" / "outside-parser.json",
                payload,
            )


if __name__ == "__main__":
    unittest.main()
