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


aepx_roundtrip_validator = load_tool("aepx_roundtrip_validator")


def write_synthetic_aepx() -> Path:
    root = LAB_ROOT / "target" / "test-inputs"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{os.getpid()}-roundtrip.aepx"
    path.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<AfterEffectsProject xmlns="http://www.adobe.com/products/aftereffects" majorVersion="1" minorVersion="0">
  <head bdata="00010203"/>
  <Folder>
    <string>Do not export this literal text</string>
    <Item bdata="0a0b"/>
  </Folder>
</AfterEffectsProject>
""",
        encoding="utf-8",
    )
    return path


def make_probe(source: Path) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_static_probe",
        "source_aepx": str(source.resolve()),
        "probe_state": "aepx_static_probe_ready_no_write",
        "xml_parse_state": "parsed",
        "edit_readiness_state": "xml_static_edit_candidate_pending_schema_review",
        "root": {
            "tag": "AfterEffectsProject",
            "namespace": "http://www.adobe.com/products/aftereffects",
            "attributes": {"majorVersion": "1", "minorVersion": "0"},
        },
        "summary": {
            "element_count": 5,
            "unique_tag_count": 5,
            "namespace_count": 1,
            "max_depth": 2,
            "max_attribute_count": 2,
            "bdata_attribute_count": 2,
            "bdata_total_decoded_bytes_if_hex": 6,
            "bdata_invalid_hex_count": 0,
            "nonempty_text_node_count": 1,
            "nonempty_text_total_chars": 31,
        },
        "top_tags": [],
        "edit_surface": {
            "aepx_xml_parseable": True,
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


def make_edit_plan(probe_path: Path) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "packet_kind": "aepx_edit_plan_packet",
        "source_aepx_probe": str(probe_path.resolve()),
        "edit_plan_state": "aepx_edit_plan_ready_no_write",
        "write_recommendation": "do_not_write_project_files",
        "review_blockers": [
            {"blocker_id": "round_trip_validator_missing", "severity": "blocker"},
            {"blocker_id": "project_write_not_approved", "severity": "blocker"},
        ],
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


class AepxRoundtripValidatorTests(unittest.TestCase):
    def test_roundtrip_validates_structure_without_writing_or_exporting_payloads(self):
        source = write_synthetic_aepx()
        probe = make_probe(source)
        edit_plan = make_edit_plan(Path("probe.json"))
        edit_plan["source_aepx_probe"] = str(Path("probe.json").resolve())
        report = aepx_roundtrip_validator.build_roundtrip_validator(
            aepx_probe=probe,
            aepx_probe_path=Path("probe.json").resolve(),
            aepx_edit_plan=edit_plan,
            aepx_edit_plan_path=Path("edit.json"),
        )
        self.assertEqual(report["report_kind"], "aepx_roundtrip_validator")
        self.assertEqual(report["roundtrip_state"], "aepx_roundtrip_validator_ready_no_write")
        self.assertTrue(report["validator_ready"])
        self.assertTrue(report["source_structure_match"])
        self.assertTrue(report["roundtrip_structure_match"])
        self.assertTrue(report["roundtrip_xml_serialized_to_memory"])
        self.assertFalse(report["roundtrip_xml_serialized_to_disk"])
        self.assertFalse(report["aepx_file_modified"])
        self.assertFalse(report["ae_project_write_performed"])
        self.assertFalse(report["text_payload_exported"])
        self.assertFalse(report["bdata_payload_exported"])
        serialized = json.dumps(report)
        self.assertNotIn("Do not export this literal text", serialized)
        self.assertNotIn("00010203", serialized)

    def test_invalid_sources_are_rejected(self):
        source = write_synthetic_aepx()
        probe = make_probe(source)
        probe["aepx_file_modified"] = True
        edit_plan = make_edit_plan(Path("probe.json"))
        with self.assertRaises(ValueError):
            aepx_roundtrip_validator.build_roundtrip_validator(
                aepx_probe=probe,
                aepx_probe_path=Path("probe.json"),
                aepx_edit_plan=edit_plan,
                aepx_edit_plan_path=Path("edit.json"),
            )

        probe = make_probe(source)
        edit_plan["write_recommendation"] = "write_project_files"
        with self.assertRaises(ValueError):
            aepx_roundtrip_validator.build_roundtrip_validator(
                aepx_probe=probe,
                aepx_probe_path=Path("probe.json"),
                aepx_edit_plan=edit_plan,
                aepx_edit_plan_path=Path("edit.json"),
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        source = write_synthetic_aepx()
        probe_root = LAB_ROOT / "target" / "aepx-static-probe"
        plan_root = LAB_ROOT / "target" / "aepx-edit-plan"
        probe_root.mkdir(parents=True, exist_ok=True)
        plan_root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        probe_path = probe_root / f"{stamp}-probe.local.json"
        plan_path = plan_root / f"{stamp}-plan.local.json"
        probe_path.write_text(json.dumps(make_probe(source)), encoding="utf-8")
        plan_path.write_text(json.dumps(make_edit_plan(probe_path)), encoding="utf-8")

        probe, resolved_probe = aepx_roundtrip_validator.load_aepx_probe(probe_path)
        plan, resolved_plan = aepx_roundtrip_validator.load_aepx_edit_plan(plan_path)
        self.assertEqual(resolved_probe, probe_path.resolve())
        self.assertEqual(resolved_plan, plan_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-roundtrip.json"
        outside.write_text(json.dumps(make_probe(source)), encoding="utf-8")
        with self.assertRaises(ValueError):
            aepx_roundtrip_validator.load_aepx_probe(outside)

        report = aepx_roundtrip_validator.build_roundtrip_validator(
            aepx_probe=probe,
            aepx_probe_path=resolved_probe,
            aepx_edit_plan=plan,
            aepx_edit_plan_path=resolved_plan,
        )
        out = LAB_ROOT / "target" / "aepx-roundtrip-validator" / f"{time.time_ns()}-{os.getpid()}-roundtrip.local.json"
        written = aepx_roundtrip_validator.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aepx_roundtrip_validator.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aepx_roundtrip_validator.write_json_create_new(LAB_ROOT / "target" / "outside-roundtrip.json", report)


if __name__ == "__main__":
    unittest.main()
