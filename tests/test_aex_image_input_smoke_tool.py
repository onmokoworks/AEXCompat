import importlib.util
import json
import sys
import tempfile
import time
import unittest
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


aex_image_input_smoke_tool = load_tool("aex_image_input_smoke_tool")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


def create_ppm(name: str, width: int = 6, height: int = 5, pattern: str = "gradient") -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{name}.ppm"
    ppm_fixture_tool.write_ppm_create_new(path, ppm_fixture_tool.generate_image(width, height, pattern))
    return path


def make_route_contract() -> dict:
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
            "allowed_route": "no_op_identity_only",
        },
        "blockers": ["load_gate_closed", "dependency_review_blocks_native_load"],
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


def make_ofx_facade() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_ofx_facade_deferred_packet",
        "facade_state": "deferred_loader_not_ready",
        "ofx_route_action": "no_op",
        "blocked_actions": [
            "ofx_describe_from_aex",
            "ofx_render_with_aex",
            "route_through_ofx",
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "ofx_plugin_built": False,
        "ofx_describe_performed": False,
        "ofx_render_performed": False,
    }


class AexImageInputSmokeToolTests(unittest.TestCase):
    def test_single_image_smoke_passes_worker_and_ofx_noop_closed(self):
        report = aex_image_input_smoke_tool.build_smoke_report(
            input_ppm=create_ppm("smoke-input"),
            route_contract=make_route_contract(),
            route_contract_path=Path("contract.json"),
            ofx_facade=make_ofx_facade(),
            ofx_facade_path=Path("facade.json"),
            worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
            output_prefix=f"{time.time_ns()}-smoke",
        )
        self.assertEqual(report["report_kind"], "aex_image_input_smoke_tool")
        self.assertEqual(report["smoke_state"], "image_input_smoke_passed_route_closed")
        self.assertTrue(report["worker_identity_passed"])
        self.assertTrue(report["ofx_identity_passed"])
        self.assertTrue(Path(report["worker_output_ppm"]).exists())
        self.assertTrue(Path(report["ofx_output_ppm"]).exists())
        self.assertTrue(report["worker_identity_check"]["pixel_match"])
        self.assertTrue(report["ofx_identity_check"]["dimension_match"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["dll_load_performed"])
        self.assertFalse(report["render_performed"])
        self.assertFalse(report["ae_invoked"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["ofx_plugin_built"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertFalse(report["ofx_render_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertTrue(report["no_load_worker_invoked"])
        self.assertFalse(report["ofx_runtime_invoked"])
        self.assertEqual(report["route_contract_summary"]["allowed_route"], "no_op_identity_only")

    def test_invalid_route_contract_or_facade_is_rejected(self):
        contract = make_route_contract()
        contract["real_route_open"] = True
        with self.assertRaises(ValueError):
            aex_image_input_smoke_tool.build_smoke_report(
                input_ppm=create_ppm("bad-contract"),
                route_contract=contract,
                route_contract_path=Path("contract.json"),
                ofx_facade=make_ofx_facade(),
                ofx_facade_path=Path("facade.json"),
                worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                output_prefix=f"{time.time_ns()}-bad",
            )

        facade = make_ofx_facade()
        facade["ofx_route_action"] = "real_route"
        with self.assertRaises(ValueError):
            aex_image_input_smoke_tool.build_smoke_report(
                input_ppm=create_ppm("bad-facade"),
                route_contract=make_route_contract(),
                route_contract_path=Path("contract.json"),
                ofx_facade=facade,
                ofx_facade_path=Path("facade.json"),
                worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                output_prefix=f"{time.time_ns()}-bad",
            )

    def test_ofx_identity_failure_cleans_up_worker_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            ppm_root = root / "ppm"
            worker_root = root / "worker"
            ofx_root = root / "ofx"
            ppm_root.mkdir()
            input_ppm = ppm_root / "input.ppm"
            input_ppm.write_bytes(b"fixture")
            worker_output = worker_root / "race-worker-identity.ppm"

            def write_worker_output(**kwargs):
                output = kwargs["output_ppm"]
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_bytes(b"worker-output")
                return {"pixel_match": True, "dimension_match": True}, []

            with (
                mock.patch.object(aex_image_input_smoke_tool, "PPM_FIXTURE_ROOT", ppm_root),
                mock.patch.object(aex_image_input_smoke_tool, "WORKER_SELFTEST_ROOT", worker_root),
                mock.patch.object(aex_image_input_smoke_tool, "OFX_NOOP_ROOT", ofx_root),
                mock.patch.object(aex_image_input_smoke_tool, "run_worker_identity", side_effect=write_worker_output),
                mock.patch.object(
                    aex_image_input_smoke_tool.aex_ofx_noop_mock,
                    "build_mock_report",
                    side_effect=FileExistsError("injected OFX output collision"),
                ),
            ):
                with self.assertRaisesRegex(FileExistsError, "injected OFX output collision"):
                    aex_image_input_smoke_tool.build_smoke_report(
                        input_ppm=input_ppm,
                        route_contract=make_route_contract(),
                        route_contract_path=root / "route.json",
                        ofx_facade=make_ofx_facade(),
                        ofx_facade_path=root / "facade.json",
                        worker_path=LAB_ROOT / "tools" / "aex_no_load_worker.py",
                        output_prefix="race",
                    )
            self.assertFalse(worker_output.exists())

    def test_paths_are_confined_and_report_is_create_new(self):
        contract_root = LAB_ROOT / "target" / "ofx-route-contract"
        facade_root = LAB_ROOT / "target" / "ofx-facade"
        contract_root.mkdir(parents=True, exist_ok=True)
        facade_root.mkdir(parents=True, exist_ok=True)
        contract_path = contract_root / f"{time.time_ns()}-smoke-contract.local.json"
        facade_path = facade_root / f"{time.time_ns()}-smoke-facade.local.json"
        contract_path.write_text(json.dumps(make_route_contract()), encoding="utf-8")
        facade_path.write_text(json.dumps(make_ofx_facade()), encoding="utf-8")

        contract, resolved_contract = aex_image_input_smoke_tool.load_route_contract(contract_path)
        facade, resolved_facade = aex_image_input_smoke_tool.load_ofx_facade(facade_path)
        self.assertEqual(contract["report_kind"], "aex_ofx_route_contract_probe")
        self.assertEqual(facade["packet_kind"], "aex_ofx_facade_deferred_packet")
        self.assertEqual(resolved_contract, contract_path.resolve())
        self.assertEqual(resolved_facade, facade_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-smoke.json"
        outside.write_text(json.dumps(make_route_contract()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_image_input_smoke_tool.load_route_contract(outside)

        report = {
            "schema_version": 1,
            "report_kind": "aex_image_input_smoke_tool",
            "native_load_performed": False,
        }
        out = LAB_ROOT / "target" / "image-input-smoke" / f"{time.time_ns()}-smoke.local.json"
        written = aex_image_input_smoke_tool.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_image_input_smoke_tool.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_image_input_smoke_tool.write_json_create_new(LAB_ROOT / "target" / "outside-smoke.json", report)


if __name__ == "__main__":
    unittest.main()
