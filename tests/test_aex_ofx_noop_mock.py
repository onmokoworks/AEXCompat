import importlib.util
import json
import sys
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_ofx_noop_mock = load_tool("aex_ofx_noop_mock")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


def make_packet() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aex_ofx_facade_deferred_packet",
        "facade_state": "deferred_loader_not_ready",
        "source_stub_state": "refused_gate_closed",
        "ofx_route_action": "no_op",
        "primary_review_candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
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
        "blocked_actions": ["ofx_describe_from_aex", "ofx_render_with_aex", "route_through_ofx"],
    }


def create_input_ppm() -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-ofx-input.ppm"
    image = ppm_fixture_tool.generate_image(5, 4, "gradient")
    ppm_fixture_tool.write_ppm_create_new(path, image)
    return path


class AexOfxNoopMockTests(unittest.TestCase):
    def test_mock_identity_keeps_real_ofx_and_aex_routes_closed(self):
        input_ppm = create_input_ppm()
        output_ppm = LAB_ROOT / "target" / "ofx-noop-mock" / f"{time.time_ns()}-identity.ppm"
        report = aex_ofx_noop_mock.build_mock_report(
            packet=make_packet(),
            packet_path=Path("ofx-packet.json"),
            input_ppm=input_ppm,
            output_ppm=output_ppm,
        )
        self.assertEqual(report["report_kind"], "aex_ofx_noop_mock_selftest")
        self.assertEqual(report["mock_state"], "mock_identity_completed_route_closed")
        self.assertTrue(report["mock_describe_performed"])
        self.assertTrue(report["mock_identity_transform_performed"])
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["ofx_plugin_built"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertFalse(report["ofx_render_performed"])
        self.assertTrue(Path(report["output_ppm"]).exists())
        self.assertTrue(report["identity_check"]["pixel_match"])

    def test_invalid_packet_refuses_without_writing_output_ppm(self):
        input_ppm = create_input_ppm()
        output_ppm = LAB_ROOT / "target" / "ofx-noop-mock" / f"{time.time_ns()}-invalid.ppm"
        packet = make_packet()
        packet["ofx_route_invoked"] = True
        report = aex_ofx_noop_mock.build_mock_report(
            packet=packet,
            packet_path=Path("ofx-packet.json"),
            input_ppm=input_ppm,
            output_ppm=output_ppm,
        )
        self.assertEqual(report["mock_state"], "invalid_ofx_packet_refused")
        self.assertIsNone(report["output_ppm"])
        self.assertFalse(output_ppm.exists())
        self.assertFalse(report["mock_describe_performed"])
        self.assertIn("OFX facade packet ofx_route_invoked must be false", report["packet_errors"])

    def test_cli_exit_follows_invalid_packet_state(self):
        args = SimpleNamespace(
            ofx_packet="packet.json",
            input_ppm="input.ppm",
            output_ppm="output.ppm",
            out="report.json",
        )
        for report, expected_exit in (
            ({"mock_state": "mock_identity_completed_route_closed", "packet_errors": []}, 0),
            ({"mock_state": "invalid_ofx_packet_refused", "packet_errors": ["invalid packet"]}, 1),
        ):
            with (
                mock.patch.object(aex_ofx_noop_mock, "parse_args", return_value=args),
                mock.patch.object(
                    aex_ofx_noop_mock,
                    "load_packet",
                    return_value=({}, Path("packet.json")),
                ),
                mock.patch.object(aex_ofx_noop_mock, "build_mock_report", return_value=report),
                mock.patch.object(
                    aex_ofx_noop_mock,
                    "write_json_create_new",
                    return_value=Path("report.json"),
                ) as write_report,
            ):
                self.assertEqual(aex_ofx_noop_mock.main(), expected_exit)
                write_report.assert_called_once_with(Path("report.json"), report)

    def test_paths_are_confined_and_outputs_are_create_new(self):
        packet_root = LAB_ROOT / "target" / "ofx-facade"
        packet_root.mkdir(parents=True, exist_ok=True)
        packet_path = packet_root / f"{time.time_ns()}-ofx-packet.json"
        packet_path.write_text(json.dumps(make_packet()), encoding="utf-8")
        loaded, resolved = aex_ofx_noop_mock.load_packet(packet_path)
        self.assertEqual(loaded["packet_kind"], "aex_ofx_facade_deferred_packet")
        self.assertEqual(resolved, packet_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-outside-ofx-packet.json"
        outside.write_text(json.dumps(make_packet()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_ofx_noop_mock.load_packet(outside)

        payload = {
            "schema_version": 1,
            "report_kind": "aex_ofx_noop_mock_selftest",
            "native_load_performed": False,
        }
        out = LAB_ROOT / "target" / "ofx-noop-mock" / f"{time.time_ns()}-report.local.json"
        written = aex_ofx_noop_mock.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_ofx_noop_mock.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_ofx_noop_mock.write_json_create_new(LAB_ROOT / "target" / "outside-ofx-mock.json", payload)

        with self.assertRaises(ValueError):
            aex_ofx_noop_mock.validate_input_ppm(LAB_ROOT / "target" / "bad.ppm")
        with self.assertRaises(ValueError):
            aex_ofx_noop_mock.validate_output_ppm(LAB_ROOT / "target" / "bad.ppm")


if __name__ == "__main__":
    unittest.main()
