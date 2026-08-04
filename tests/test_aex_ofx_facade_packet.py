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


aex_ofx_facade_packet = load_tool("aex_ofx_facade_packet")


def make_stub(stub_state: str = "refused_gate_closed") -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aex_native_loader_stub_report",
        "stub_state": stub_state,
        "loader_action": "no_op",
        "refusal_reasons": ["load gate state is closed_missing_or_invalid_approval"]
        if stub_state != "stub_ready_no_load_performed"
        else [],
        "primary_review_candidate": {"relative_path": "AEPluginBuild\\ScatterMap.aex"},
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "accepted_aex_path": None,
    }


class AexOfxFacadePacketTests(unittest.TestCase):
    def test_closed_loader_defers_ofx_route(self):
        packet = aex_ofx_facade_packet.build_packet(make_stub(), Path("stub.json"))
        self.assertEqual(packet["packet_kind"], "aex_ofx_facade_deferred_packet")
        self.assertEqual(packet["facade_state"], "deferred_loader_not_ready")
        self.assertEqual(packet["ofx_route_action"], "no_op")
        self.assertFalse(packet["ofx_route_invoked"])
        self.assertFalse(packet["ofx_plugin_built"])
        self.assertFalse(packet["ofx_render_performed"])
        self.assertIn("route_through_ofx", packet["blocked_actions"])

    def test_ready_loader_stub_still_keeps_ofx_deferred(self):
        packet = aex_ofx_facade_packet.build_packet(make_stub("stub_ready_no_load_performed"), Path("stub.json"))
        self.assertEqual(packet["facade_state"], "deferred_pending_ofx_facade_review")
        self.assertEqual(packet["ofx_route_action"], "no_op")
        self.assertFalse(packet["native_load_performed"])
        self.assertFalse(packet["ofx_route_invoked"])
        self.assertTrue(any("separate reviewed facade" in reason for reason in packet["refusal_reasons"]))

    def test_invalid_loader_evidence_is_deferred(self):
        stub = make_stub()
        stub["ofx_route_invoked"] = True
        packet = aex_ofx_facade_packet.build_packet(stub, Path("stub.json"))
        self.assertEqual(packet["facade_state"], "invalid_loader_evidence_deferred")
        self.assertIn("loader stub ofx_route_invoked must be false", packet["refusal_reasons"])
        self.assertFalse(packet["ofx_route_invoked"])

    def test_paths_are_confined_and_output_is_create_new(self):
        stub_root = LAB_ROOT / "target" / "native-loader-stub"
        stub_root.mkdir(parents=True, exist_ok=True)
        source = stub_root / f"{time.time_ns()}-{os.getpid()}-ofx-stub.json"
        source.write_text(json.dumps(make_stub()), encoding="utf-8")
        loaded, resolved = aex_ofx_facade_packet.load_loader_stub(source)
        self.assertEqual(loaded["report_kind"], "aex_native_loader_stub_report")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-stub.json"
        outside.write_text(json.dumps(make_stub()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aex_ofx_facade_packet.load_loader_stub(outside)

        packet = aex_ofx_facade_packet.build_packet(loaded, resolved)
        out = LAB_ROOT / "target" / "ofx-facade" / f"{time.time_ns()}-{os.getpid()}-ofx.local.json"
        written = aex_ofx_facade_packet.write_json_create_new(out, packet)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aex_ofx_facade_packet.write_json_create_new(out, packet)
        with self.assertRaises(ValueError):
            aex_ofx_facade_packet.write_json_create_new(LAB_ROOT / "target" / "outside-ofx.json", packet)


if __name__ == "__main__":
    unittest.main()
