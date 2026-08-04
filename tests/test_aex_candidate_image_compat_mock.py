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


aex_candidate_image_compat_mock = load_tool("aex_candidate_image_compat_mock")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


def compat_card_payload() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_candidate_compatibility_card",
        "compatibility_card_state": "candidate_compatibility_card_ready_no_load",
        "compatibility_card_ready": True,
        "candidate_relative_path": "AEPluginBuild\\ScatterMap.aex",
        "unsafe_exports_present": False,
        "absolute_ppm_paths_exported": False,
        "absolute_aex_paths_exported": False,
        "approval_can_be_issued_now": False,
        "approval_manifest_created": False,
        "current_fixture_approval_valid": False,
        "fixture_approval_satisfied": False,
        "path_acceptance_ready": False,
        "aex_path_acceptance_enabled": False,
        "path_payload_supplied": False,
        "real_render_open": False,
        "real_route_open": False,
        "aex_file_hashed": False,
        "aex_file_copied": False,
        "native_load_gate": "closed",
        "native_load_gate_stays_closed": True,
        "approval_gate_stays_closed": True,
        "accepted_aex_path": None,
        "no_load_test_card": {
            "worker_identity_passed": True,
            "ofx_noop_identity_passed": True,
            "blocked_load_aex_verified": True,
            "ppm_paths_exported": False,
            "real_render_open": False,
            "real_route_open": False,
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
        "real_pipl_payload_parser_enabled": False,
        "real_pipl_payload_parsed": False,
        "resource_payload_opened": False,
        "resource_payload_extracted": False,
        "raw_payload_serialized": False,
    }


def write_card(name: str, payload: dict | None = None) -> Path:
    root = LAB_ROOT / "target" / "candidate-compat-card"
    root.mkdir(parents=True, exist_ok=True)
    path = root / name
    path.write_text(json.dumps(payload or compat_card_payload()), encoding="utf-8")
    return path


def write_input_ppm(name: str) -> Path:
    path = LAB_ROOT / "target" / "ppm-fixtures" / name
    image = ppm_fixture_tool.generate_image(3, 2, "gradient")
    ppm_fixture_tool.write_ppm_create_new(path, image)
    return path


class AexCandidateImageCompatMockTests(unittest.TestCase):
    def test_builds_no_load_invert_mock_without_absolute_path_exports(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        card_path = write_card(f"ae-candidate-compat-card-{stamp}.local.json")
        input_ppm = write_input_ppm(f"candidate-image-compat-mock-{stamp}.ppm")
        output_ppm = LAB_ROOT / "target" / "candidate-image-compat-mock" / f"mock-{stamp}.ppm"
        report = aex_candidate_image_compat_mock.build_mock_report(
            card=compat_card_payload(),
            card_path=card_path,
            input_ppm=input_ppm,
            output_ppm=output_ppm,
            operation="invert",
        )

        self.assertEqual(report["report_kind"], "aex_candidate_image_compat_mock")
        self.assertEqual(report["mock_state"], "candidate_image_compat_mock_passed_no_load")
        self.assertTrue(report["mock_ready"])
        self.assertEqual(report["operation"], "invert")
        self.assertEqual(report["candidate_relative_path"], "AEPluginBuild\\ScatterMap.aex")
        self.assertFalse(report["input_ppm_absolute_path_exported"])
        self.assertFalse(report["output_ppm_absolute_path_exported"])
        self.assertFalse(Path(report["input_ppm_relative"]).is_absolute())
        self.assertFalse(Path(report["output_ppm_relative"]).is_absolute())
        self.assertTrue(report["transform_check"]["pixel_match_expected"])
        self.assertTrue(report["transform_check"]["dimension_match_expected"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["aex_file_opened"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["pipl_payload_parsed"])

        input_image = ppm_fixture_tool.read_ppm(input_ppm)
        expected = ppm_fixture_tool.transform_image(input_image, "invert")
        output_image = ppm_fixture_tool.read_ppm(output_ppm)
        self.assertEqual(output_image.pixels, expected.pixels)

    def test_rejects_unsafe_or_open_compatibility_card(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        card = compat_card_payload()
        card["real_route_open"] = True
        input_ppm = write_input_ppm(f"candidate-image-compat-mock-unsafe-{stamp}.ppm")
        output_ppm = LAB_ROOT / "target" / "candidate-image-compat-mock" / f"unsafe-{stamp}.ppm"
        with self.assertRaises(ValueError):
            aex_candidate_image_compat_mock.build_mock_report(
                card=card,
                card_path=LAB_ROOT / "target" / "candidate-compat-card" / f"ae-candidate-compat-card-{stamp}.local.json",
                input_ppm=input_ppm,
                output_ppm=output_ppm,
                operation="identity",
            )
        self.assertFalse(output_ppm.exists())

    def test_confines_paths_and_writes_json_create_new(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        card_path = write_card(f"ae-candidate-compat-card-{stamp}.local.json")
        loaded, resolved = aex_candidate_image_compat_mock.load_compat_card(
            Path("target") / "candidate-compat-card" / card_path.name
        )
        input_ppm = write_input_ppm(f"candidate-image-compat-mock-write-{stamp}.ppm")
        output_ppm = LAB_ROOT / "target" / "candidate-image-compat-mock" / f"write-{stamp}.ppm"
        report = aex_candidate_image_compat_mock.build_mock_report(
            card=loaded,
            card_path=resolved,
            input_ppm=input_ppm,
            output_ppm=output_ppm,
            operation="identity",
        )
        out = LAB_ROOT / "target" / "candidate-image-compat-mock" / f"report-{stamp}.json"
        written = aex_candidate_image_compat_mock.write_json_create_new(out, report)
        self.assertEqual(written, out)
        with self.assertRaises(FileExistsError):
            aex_candidate_image_compat_mock.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aex_candidate_image_compat_mock.write_json_create_new(
                LAB_ROOT / "target" / "outside-candidate-image-compat-mock.json", report
            )
        with self.assertRaises(ValueError):
            aex_candidate_image_compat_mock.build_mock_report(
                card=loaded,
                card_path=resolved,
                input_ppm=LAB_ROOT / "target" / "outside.ppm",
                output_ppm=LAB_ROOT / "target" / "candidate-image-compat-mock" / f"outside-{stamp}.ppm",
                operation="identity",
            )

    def test_rejects_unsupported_operation(self):
        stamp = f"{time.time_ns()}-{os.getpid()}"
        input_ppm = write_input_ppm(f"candidate-image-compat-mock-operation-{stamp}.ppm")
        output_ppm = LAB_ROOT / "target" / "candidate-image-compat-mock" / f"operation-{stamp}.ppm"
        with self.assertRaises(ValueError):
            aex_candidate_image_compat_mock.build_mock_report(
                card=compat_card_payload(),
                card_path=LAB_ROOT / "target" / "candidate-compat-card" / f"ae-candidate-compat-card-{stamp}.local.json",
                input_ppm=input_ppm,
                output_ppm=output_ppm,
                operation="aex_render",
            )


if __name__ == "__main__":
    unittest.main()
