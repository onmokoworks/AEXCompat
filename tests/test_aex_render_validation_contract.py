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


aex_render_validation_contract = load_tool("aex_render_validation_contract")


def make_image_validation() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_validation",
        "validation_state": "image_fixture_validation_passed_no_load",
        "validation_passed": True,
        "fixture_results": [{"case_id": "gradient"}],
        "summary": {
            "fixture_count": 1,
            "passed_count": 1,
            "failed_count": 0,
            "total_pixel_bytes": 576,
            "pattern_counts": {"gradient": 1},
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_image_smoke() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_input_smoke_tool",
        "smoke_state": "image_input_smoke_passed_route_closed",
        "input_summary": {"width": 16, "height": 12, "pixel_bytes": 576},
        "route_contract_summary": {
            "contract_state": "ofx_route_contract_ready_route_closed",
            "real_route_open": False,
            "mock_route_ready": True,
            "allowed_route": "no_op_identity_only",
        },
        "worker_identity_check": {"width": 16, "height": 12, "bytes": 576, "pixel_match": True, "dimension_match": True},
        "worker_identity_passed": True,
        "ofx_identity_check": {"width": 16, "height": 12, "bytes": 576, "pixel_match": True, "dimension_match": True},
        "ofx_identity_passed": True,
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
        "ofx_runtime_invoked": False,
    }


def make_load_gate() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_load_gate_check",
        "gate_state": "closed_dependency_review_or_invalid_approval",
        "dependency_native_load_recommendation": "do_not_open_native_load_gate",
        "blocked_actions": ["load_aex_dll", "call_EffectMain", "render_with_aex", "route_through_ofx"],
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


def make_ofx_route_contract() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_ofx_route_contract_probe",
        "contract_state": "ofx_route_contract_ready_route_closed",
        "real_route_open": False,
        "mock_route_ready": True,
        "route_contract": {
            "real_route_open": False,
            "mock_route_ready": True,
            "ofx_runtime_invoked": False,
            "aex_runtime_invoked": False,
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
    }


def build_report(**overrides):
    values = {
        "image_validation": make_image_validation(),
        "image_smoke": make_image_smoke(),
        "load_gate": make_load_gate(),
        "ofx_route_contract": make_ofx_route_contract(),
    }
    values.update(overrides)
    return aex_render_validation_contract.build_render_validation_contract(
        image_validation=values["image_validation"],
        image_validation_path=Path("validation.json"),
        image_smoke=values["image_smoke"],
        image_smoke_path=Path("smoke.json"),
        load_gate=values["load_gate"],
        load_gate_path=Path("gate.json"),
        ofx_route_contract=values["ofx_route_contract"],
        ofx_route_contract_path=Path("ofx-contract.json"),
    )


class AexRenderValidationContractTests(unittest.TestCase):
    def test_contract_ready_but_render_closed(self):
        report = build_report()
        self.assertEqual(report["report_kind"], "aex_render_validation_contract")
        self.assertEqual(report["contract_state"], "render_validation_contract_ready_render_closed")
        self.assertFalse(report["real_render_open"])
        self.assertTrue(report["no_load_validation_ready"])
        self.assertEqual(report["render_contract"]["state"], "blocked_pending_fixture_approval_native_loader_and_render_harness")
        self.assertEqual(report["image_validation_contract"]["fixture_count"], 1)
        self.assertTrue(report["image_validation_contract"]["smoke_worker_identity_passed"])
        self.assertTrue(report["image_validation_contract"]["smoke_ofx_identity_passed"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["render_performed"])
        self.assertFalse(report["ae_invoked"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["aex_render_performed"])
        self.assertFalse(report["render_validation_performed"])
        self.assertIn("load_gate_closed", report["blockers"])

    def test_invalid_sources_are_rejected(self):
        smoke = make_image_smoke()
        smoke["ofx_identity_check"]["pixel_match"] = False
        with self.assertRaises(ValueError):
            build_report(image_smoke=smoke)

        gate = make_load_gate()
        gate["gate_state"] = "preconditions_satisfied_no_load_performed"
        with self.assertRaises(ValueError):
            build_report(load_gate=gate)

        ofx = make_ofx_route_contract()
        ofx["real_route_open"] = True
        with self.assertRaises(ValueError):
            build_report(ofx_route_contract=ofx)

    def test_paths_are_confined_and_report_is_create_new(self):
        roots = {
            "validation": LAB_ROOT / "target" / "image-fixture-validation",
            "smoke": LAB_ROOT / "target" / "image-input-smoke",
            "gate": LAB_ROOT / "target" / "load-gate",
            "ofx": LAB_ROOT / "target" / "ofx-route-contract",
        }
        for root in roots.values():
            root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        paths = {
            "validation": roots["validation"] / f"{stamp}-render-validation.local.json",
            "smoke": roots["smoke"] / f"{stamp}-render-smoke.local.json",
            "gate": roots["gate"] / f"{stamp}-render-gate.local.json",
            "ofx": roots["ofx"] / f"{stamp}-render-ofx.local.json",
        }
        payloads = {
            "validation": make_image_validation(),
            "smoke": make_image_smoke(),
            "gate": make_load_gate(),
            "ofx": make_ofx_route_contract(),
        }
        for label, path in paths.items():
            path.write_text(json.dumps(payloads[label]), encoding="utf-8")

        validation, validation_path = aex_render_validation_contract.load_image_validation(paths["validation"])
        smoke, smoke_path = aex_render_validation_contract.load_image_smoke(paths["smoke"])
        gate, gate_path = aex_render_validation_contract.load_load_gate(paths["gate"])
        ofx, ofx_path = aex_render_validation_contract.load_ofx_route_contract(paths["ofx"])
        self.assertEqual(validation_path, paths["validation"].resolve())
        self.assertEqual(smoke_path, paths["smoke"].resolve())
        self.assertEqual(gate_path, paths["gate"].resolve())
        self.assertEqual(ofx_path, paths["ofx"].resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-render.json"
        outside.write_text(json.dumps(make_image_validation()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_render_validation_contract.load_image_validation(outside)

        report = aex_render_validation_contract.build_render_validation_contract(
            image_validation=validation,
            image_validation_path=validation_path,
            image_smoke=smoke,
            image_smoke_path=smoke_path,
            load_gate=gate,
            load_gate_path=gate_path,
            ofx_route_contract=ofx,
            ofx_route_contract_path=ofx_path,
        )
        out = LAB_ROOT / "target" / "render-validation-contract" / f"{time.time_ns()}-{os.getpid()}-render-contract.local.json"
        written = aex_render_validation_contract.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_render_validation_contract.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_render_validation_contract.write_json_create_new(LAB_ROOT / "target" / "outside-render.json", report)


if __name__ == "__main__":
    unittest.main()
