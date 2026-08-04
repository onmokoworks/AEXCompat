import importlib.util
import json
import os
import sys
import time
import unittest
import uuid
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


aex_ofx_suite_noop_selftest = load_tool("aex_ofx_suite_noop_selftest")
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


def create_ppm(name: str, width: int, height: int, pattern: str) -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{uuid.uuid4().hex}-{name}.ppm"
    ppm_fixture_tool.write_ppm_create_new(path, ppm_fixture_tool.generate_image(width, height, pattern))
    return path


def make_suite() -> dict:
    first = create_ppm("ofx-suite-first", 5, 4, "gradient")
    second = create_ppm("ofx-suite-second", 7, 3, "checker")
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_image_fixture_suite",
        "suite_state": "image_fixture_suite_ready",
        "target_candidate": {
            "relative_path": "AEPluginBuild\\ScatterMap.aex",
            "candidate_policy_state": "eligible_for_manual_policy_review",
            "native_load_approval": "not_granted",
        },
        "fixtures": [
            {"case_id": "first", "pattern": "gradient", "ppm_path": str(first)},
            {"case_id": "second", "pattern": "checker", "ppm_path": str(second)},
        ],
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
    }


class AexOfxSuiteNoopSelftestTests(unittest.TestCase):
    def test_suite_noop_identity_keeps_real_routes_closed(self):
        report = aex_ofx_suite_noop_selftest.build_suite_report(
            packet=make_packet(),
            packet_path=Path("packet.json"),
            suite=make_suite(),
            suite_path=Path("suite.json"),
            output_prefix=f"{time.time_ns()}-{os.getpid()}-ofx-suite",
        )
        self.assertEqual(report["report_kind"], "aex_ofx_suite_noop_selftest")
        self.assertEqual(report["ofx_suite_selftest_state"], "ofx_suite_noop_identity_passed_route_closed")
        self.assertEqual(report["fixture_count"], 2)
        self.assertFalse(report["native_load_performed"])
        self.assertFalse(report["ofx_route_invoked"])
        self.assertFalse(report["ofx_describe_performed"])
        self.assertFalse(report["ofx_render_performed"])
        self.assertFalse(report["aex_file_opened"])
        for result in report["fixture_results"]:
            self.assertTrue(Path(result["output_ppm"]).exists())
            self.assertTrue(result["identity_check"]["pixel_match"])
            self.assertTrue(result["identity_check"]["dimension_match"])

    def test_invalid_packet_or_suite_is_rejected(self):
        packet = make_packet()
        packet["ofx_route_invoked"] = True
        with self.assertRaises(ValueError):
            aex_ofx_suite_noop_selftest.build_suite_report(
                packet=packet,
                packet_path=Path("packet.json"),
                suite=make_suite(),
                suite_path=Path("suite.json"),
                output_prefix=f"{time.time_ns()}-{os.getpid()}-bad",
            )

        suite = make_suite()
        suite["suite_state"] = "incomplete"
        with self.assertRaises(ValueError):
            aex_ofx_suite_noop_selftest.build_suite_report(
                packet=make_packet(),
                packet_path=Path("packet.json"),
                suite=suite,
                suite_path=Path("suite.json"),
                output_prefix=f"{time.time_ns()}-{os.getpid()}-bad",
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        packet_root = LAB_ROOT / "target" / "ofx-facade"
        suite_root = LAB_ROOT / "target" / "image-fixture-suite"
        packet_root.mkdir(parents=True, exist_ok=True)
        suite_root.mkdir(parents=True, exist_ok=True)
        packet_path = packet_root / f"{time.time_ns()}-{os.getpid()}-ofx-suite-packet.local.json"
        suite_path = suite_root / f"{time.time_ns()}-{os.getpid()}-ofx-suite.local.json"
        packet_path.write_text(json.dumps(make_packet()), encoding="utf-8")
        suite_path.write_text(json.dumps(make_suite()), encoding="utf-8")
        packet, resolved_packet = aex_ofx_suite_noop_selftest.load_packet(packet_path)
        suite, resolved_suite = aex_ofx_suite_noop_selftest.load_image_suite(suite_path)
        self.assertEqual(packet["packet_kind"], "aex_ofx_facade_deferred_packet")
        self.assertEqual(suite["report_kind"], "aex_image_fixture_suite")
        self.assertEqual(resolved_packet, packet_path.resolve())
        self.assertEqual(resolved_suite, suite_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-ofx-suite.json"
        outside.write_text(json.dumps(make_suite()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_ofx_suite_noop_selftest.load_image_suite(outside)

        payload = {
            "schema_version": 1,
            "report_kind": "aex_ofx_suite_noop_selftest",
            "native_load_performed": False,
        }
        out = LAB_ROOT / "target" / "ofx-suite-selftest" / f"{time.time_ns()}-{os.getpid()}-ofx-suite.local.json"
        written = aex_ofx_suite_noop_selftest.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_ofx_suite_noop_selftest.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aex_ofx_suite_noop_selftest.write_json_create_new(LAB_ROOT / "target" / "outside-ofx-suite.json", payload)


if __name__ == "__main__":
    unittest.main()
