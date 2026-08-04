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


aepx_edit_plan_packet = load_tool("aepx_edit_plan_packet")


def make_probe() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_static_probe",
        "probe_state": "aepx_static_probe_ready_no_write",
        "xml_parse_state": "parsed",
        "edit_readiness_state": "xml_static_edit_candidate_pending_schema_review",
        "root": {
            "tag": "AfterEffectsProject",
            "namespace": "http://www.adobe.com/products/aftereffects",
            "attributes": {"majorVersion": "1", "minorVersion": "0"},
        },
        "summary": {
            "element_count": 12,
            "unique_tag_count": 5,
            "max_depth": 3,
            "bdata_attribute_count": 4,
            "bdata_total_decoded_bytes_if_hex": 12,
            "bdata_invalid_hex_count": 0,
            "nonempty_text_node_count": 2,
        },
        "top_tags": [
            {"tag": "string", "count": 2, "nonempty_text_count": 2},
            {"tag": "cdat", "count": 1, "bdata_attribute_count": 1},
        ],
        "edit_surface": {
            "aepx_xml_parseable": True,
            "aep_binary_editable_by_this_tool": False,
            "aepx_write_approved": False,
            "text_payload_exported": False,
        },
        "native_load_enabled": False,
        "native_load_performed": False,
        "render_performed": False,
        "ae_invoked": False,
        "ofx_route_invoked": False,
        "private_payload_copied": False,
        "aex_file_opened": False,
        "aepx_file_modified": False,
        "aep_binary_modified": False,
        "ae_project_write_performed": False,
    }


class AepxEditPlanPacketTests(unittest.TestCase):
    def test_edit_plan_keeps_project_writes_blocked(self):
        packet = aepx_edit_plan_packet.build_edit_plan_packet(make_probe(), Path("probe.json"))
        self.assertEqual(packet["packet_kind"], "aepx_edit_plan_packet")
        self.assertEqual(packet["edit_plan_state"], "aepx_edit_plan_ready_no_write")
        self.assertEqual(packet["write_recommendation"], "do_not_write_project_files")
        self.assertFalse(packet["aepx_file_modified"])
        self.assertFalse(packet["ae_project_write_performed"])
        self.assertIn("modify_aepx", packet["blocked_actions"])
        self.assertEqual(packet["summary"]["surface_count"], 4)
        blockers = {item["blocker_id"]: item for item in packet["review_blockers"]}
        self.assertIn("round_trip_validator_missing", blockers)
        self.assertIn("binary_bdata_schema_unknown", blockers)
        surfaces = {item["surface_id"]: item for item in packet["edit_surfaces"]}
        self.assertEqual(surfaces["bdata_binary_attributes"]["edit_candidate_state"], "do_not_edit_binary_payloads")

    def test_invalid_or_unsafe_probe_is_rejected(self):
        probe = make_probe()
        probe["aepx_file_modified"] = True
        with self.assertRaises(ValueError):
            aepx_edit_plan_packet.build_edit_plan_packet(probe, Path("probe.json"))

        probe = make_probe()
        probe["edit_surface"]["aepx_write_approved"] = True
        with self.assertRaises(ValueError):
            aepx_edit_plan_packet.build_edit_plan_packet(probe, Path("probe.json"))

    def test_paths_are_confined_and_output_is_create_new(self):
        probe_root = LAB_ROOT / "target" / "aepx-static-probe"
        probe_root.mkdir(parents=True, exist_ok=True)
        source = probe_root / f"{time.time_ns()}-{os.getpid()}-aepx-probe.local.json"
        source.write_text(json.dumps(make_probe()), encoding="utf-8")
        loaded, resolved = aepx_edit_plan_packet.load_probe(source)
        self.assertEqual(loaded["report_kind"], "aepx_static_probe")
        self.assertEqual(resolved, source.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-aepx-probe.json"
        outside.write_text(json.dumps(make_probe()), encoding="utf-8")
        with self.assertRaises(ValueError):
            aepx_edit_plan_packet.load_probe(outside)

        payload = aepx_edit_plan_packet.build_edit_plan_packet(loaded, resolved)
        out = LAB_ROOT / "target" / "aepx-edit-plan" / f"{time.time_ns()}-{os.getpid()}-edit-plan.local.json"
        written = aepx_edit_plan_packet.write_json_create_new(out, payload)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aepx_edit_plan_packet.write_json_create_new(out, payload)
        with self.assertRaises(ValueError):
            aepx_edit_plan_packet.write_json_create_new(LAB_ROOT / "target" / "outside-edit-plan.json", payload)


if __name__ == "__main__":
    unittest.main()
