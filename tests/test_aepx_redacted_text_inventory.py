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


aepx_redacted_text_inventory = load_tool("aepx_redacted_text_inventory")


def write_synthetic_aepx() -> Path:
    root = LAB_ROOT / "target" / "test-inputs"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{os.getpid()}-redacted-inventory.aepx"
    path.write_text(
        """<?xml version="1.0" encoding="UTF-8"?>
<AfterEffectsProject xmlns="http://www.adobe.com/products/aftereffects" majorVersion="1" minorVersion="0">
  <Folder privateAttr="do-not-export-attribute-value">
    <string>Secret Layer Name</string>
    <string>Another Secret Value</string>
    <Label bdata="0a0b">Private label text</Label>
  </Folder>
</AfterEffectsProject>
""",
        encoding="utf-8",
    )
    return path


def make_roundtrip(source: Path) -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_roundtrip_validator",
        "source_aepx": str(source.resolve()),
        "roundtrip_state": "aepx_roundtrip_validator_ready_no_write",
        "validator_ready": True,
        "source_structure_match": True,
        "roundtrip_structure_match": True,
        "roundtrip_xml_serialized_to_memory": True,
        "roundtrip_xml_serialized_to_disk": False,
        "text_payload_exported": False,
        "bdata_payload_exported": False,
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


class AepxRedactedTextInventoryTests(unittest.TestCase):
    def test_inventory_records_text_metadata_without_payload_values(self):
        source = write_synthetic_aepx()
        report = aepx_redacted_text_inventory.build_redacted_text_inventory(
            roundtrip_validator=make_roundtrip(source),
            roundtrip_validator_path=Path("roundtrip.json").resolve(),
        )

        self.assertEqual(report["report_kind"], "aepx_redacted_text_inventory")
        self.assertEqual(report["inventory_state"], "aepx_redacted_text_inventory_ready_no_write")
        self.assertTrue(report["inventory_ready"])
        self.assertTrue(report["aepx_xml_parsed"])
        self.assertFalse(report["text_payload_exported"])
        self.assertFalse(report["text_payload_hash_exported"])
        self.assertFalse(report["bdata_payload_exported"])
        self.assertFalse(report["raw_payload_serialized"])
        self.assertFalse(report["aepx_file_modified"])
        self.assertFalse(report["ae_project_write_performed"])
        self.assertEqual(report["summary"]["text_node_count"], 3)
        self.assertEqual(report["summary"]["unique_text_tag_count"], 2)
        self.assertIn("string", report["summary"]["by_tag"])
        self.assertIn("Label", report["summary"]["by_tag"])
        self.assertTrue(all(entry["payload_value_exported"] is False for entry in report["text_nodes"]))
        self.assertTrue(all(entry["payload_hash_exported"] is False for entry in report["text_nodes"]))

        serialized = json.dumps(report)
        self.assertNotIn("Secret Layer Name", serialized)
        self.assertNotIn("Another Secret Value", serialized)
        self.assertNotIn("Private label text", serialized)
        self.assertNotIn("do-not-export-attribute-value", serialized)
        self.assertNotIn("0a0b", serialized)

    def test_invalid_roundtrip_sources_are_rejected(self):
        source = write_synthetic_aepx()
        roundtrip = make_roundtrip(source)
        roundtrip["validator_ready"] = False
        with self.assertRaises(ValueError):
            aepx_redacted_text_inventory.build_redacted_text_inventory(
                roundtrip_validator=roundtrip,
                roundtrip_validator_path=Path("roundtrip.json"),
            )

        roundtrip = make_roundtrip(source)
        roundtrip["text_payload_exported"] = True
        with self.assertRaises(ValueError):
            aepx_redacted_text_inventory.build_redacted_text_inventory(
                roundtrip_validator=roundtrip,
                roundtrip_validator_path=Path("roundtrip.json"),
            )

        roundtrip = make_roundtrip(source)
        roundtrip["aepx_file_modified"] = True
        with self.assertRaises(ValueError):
            aepx_redacted_text_inventory.build_redacted_text_inventory(
                roundtrip_validator=roundtrip,
                roundtrip_validator_path=Path("roundtrip.json"),
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        source = write_synthetic_aepx()
        roundtrip_root = LAB_ROOT / "target" / "aepx-roundtrip-validator"
        roundtrip_root.mkdir(parents=True, exist_ok=True)
        roundtrip_path = roundtrip_root / f"{time.time_ns()}-{os.getpid()}-roundtrip.local.json"
        roundtrip_path.write_text(json.dumps(make_roundtrip(source)), encoding="utf-8")

        roundtrip, resolved_roundtrip = aepx_redacted_text_inventory.load_roundtrip_validator(roundtrip_path)
        self.assertEqual(resolved_roundtrip, roundtrip_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-roundtrip.json"
        outside.write_text(json.dumps(make_roundtrip(source)), encoding="utf-8")
        with self.assertRaises(ValueError):
            aepx_redacted_text_inventory.load_roundtrip_validator(outside)

        report = aepx_redacted_text_inventory.build_redacted_text_inventory(
            roundtrip_validator=roundtrip,
            roundtrip_validator_path=resolved_roundtrip,
        )
        out = LAB_ROOT / "target" / "aepx-redacted-text-inventory" / f"{time.time_ns()}-{os.getpid()}-inventory.local.json"
        written = aepx_redacted_text_inventory.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aepx_redacted_text_inventory.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aepx_redacted_text_inventory.write_json_create_new(
                LAB_ROOT / "target" / "outside-redacted-text-inventory.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
