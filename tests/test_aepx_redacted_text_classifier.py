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


aepx_redacted_text_classifier = load_tool("aepx_redacted_text_classifier")


def make_roundtrip() -> dict:
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_roundtrip_validator",
        "source_aepx": str((LAB_ROOT.parent / "fixture.aepx").resolve()),
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


def row(row_id: str, *, tag: str, parent_tag: str, length: int, bucket: str, **overrides) -> dict:
    payload = {
        "xml_path": f"/AfterEffectsProject/{parent_tag}/{tag}",
        "parent_tag": parent_tag,
        "tag": tag,
        "namespace_present": False,
        "namespace_id": "none",
        "sibling_index": 1,
        "depth": 2,
        "child_count": 0,
        "attribute_key_count": 0,
        "attribute_keys": [],
        "contains_bdata_attribute": False,
        "text_presence": "nonempty",
        "text_length_chars": length,
        "text_length_bucket": bucket,
        "trim_delta_bucket": "0",
        "newline_count_bucket": "0",
        "character_class_flags": {
            "has_ascii": True,
            "has_non_ascii": False,
            "has_digits": False,
            "has_symbol": False,
            "has_linebreak": False,
        },
        "sensitivity_flags": {
            "looks_like_path": False,
            "looks_like_url": False,
            "looks_like_email": False,
            "looks_like_guid": False,
            "looks_like_numeric_id": False,
        },
        "candidate_state": "text_label_candidate_pending_schema",
        "leading_or_trailing_whitespace": False,
        "payload_value_exported": False,
        "payload_hash_exported": False,
        "write_risk": "high_pending_schema_review",
        "inventory_index": int(row_id.split("_")[1]),
        "row_id": row_id,
        "element_ordinal": int(row_id.split("_")[1]),
    }
    payload.update(overrides)
    return payload


def make_inventory(roundtrip_path: Path) -> dict:
    rows = [
        row(
            "text_0000",
            tag="ProjectXMPMetadata",
            parent_tag="AfterEffectsProject",
            length=512,
            bucket="65_plus",
            sensitivity_flags={
                "looks_like_path": True,
                "looks_like_url": True,
                "looks_like_email": False,
                "looks_like_guid": True,
                "looks_like_numeric_id": False,
            },
        ),
        row(
            "text_0001",
            tag="string",
            parent_tag="tdsn",
            length=6,
            bucket="5_16",
            sensitivity_flags={
                "looks_like_path": True,
                "looks_like_url": False,
                "looks_like_email": False,
                "looks_like_guid": False,
                "looks_like_numeric_id": False,
            },
        ),
        row("text_0002", tag="string", parent_tag="SLay", length=4, bucket="1_4"),
    ]
    return {
        "schema_version": 1,
        "publication_status": "local-only",
        "report_kind": "aepx_redacted_text_inventory",
        "source_aepx_roundtrip_validator": str(roundtrip_path.resolve()),
        "source_aepx": str((LAB_ROOT.parent / "fixture.aepx").resolve()),
        "inventory_state": "aepx_redacted_text_inventory_ready_no_write",
        "inventory_ready": True,
        "source_roundtrip_state": "aepx_roundtrip_validator_ready_no_write",
        "source_validator_ready": True,
        "source_structure_match": True,
        "roundtrip_structure_match": True,
        "aepx_xml_parsed": True,
        "text_payload_exported": False,
        "text_payload_hash_exported": False,
        "bdata_payload_exported": False,
        "raw_text_fields_present": False,
        "value_hashes_emitted": False,
        "absolute_source_paths_in_inventory_rows": False,
        "raw_payload_serialized": False,
        "roundtrip_xml_serialized_to_disk": False,
        "summary": {
            "text_node_count": len(rows),
            "unique_text_tag_count": 2,
            "total_text_length_chars": 522,
            "max_text_length_chars": 512,
            "max_depth": 2,
        },
        "text_nodes": rows,
        "redaction_policy": {"state": "text_inventory_redacted_metadata_only"},
        "inventory_contract": {"state": "redacted_text_inventory_ready_no_write"},
        "blockers": ["project_write_not_approved"],
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


class AepxRedactedTextClassifierTests(unittest.TestCase):
    def test_classifier_builds_metadata_only_no_write_rows(self):
        roundtrip_path = Path("roundtrip.json").resolve()
        report = aepx_redacted_text_classifier.build_redacted_text_classifier(
            inventory=make_inventory(roundtrip_path),
            inventory_path=Path("inventory.json").resolve(),
            roundtrip_validator=make_roundtrip(),
            roundtrip_validator_path=roundtrip_path,
        )

        self.assertEqual(report["report_kind"], "aepx_redacted_text_classifier")
        self.assertEqual(report["classifier_state"], "aepx_redacted_text_classifier_ready_no_write")
        self.assertTrue(report["classifier_ready"])
        self.assertTrue(report["source_chain_valid"])
        self.assertTrue(report["inventory_rows_classified"])
        self.assertFalse(report["project_write_ready"])
        self.assertFalse(report["project_write_allowed_now"])
        self.assertFalse(report["classifier_approves_project_write"])
        self.assertEqual(report["approved_write_candidate_count"], 0)
        self.assertEqual(report["classification_row_count"], 3)
        self.assertEqual(report["no_write_row_count"], 3)
        self.assertEqual(report["unknown_row_count"], 0)
        self.assertTrue(report["row_count_matches_inventory_summary"])
        self.assertFalse(report["text_payload_exported"])
        self.assertFalse(report["text_payload_hash_exported"])
        self.assertFalse(report["bdata_payload_exported"])
        self.assertFalse(report["raw_text_fields_present"])
        self.assertFalse(report["absolute_source_paths_in_classifier_rows"])
        self.assertFalse(report["aepx_file_modified"])
        self.assertFalse(report["ae_project_write_performed"])

        classifications = {row["row_id"]: row["classification"] for row in report["classification_rows"]}
        self.assertEqual(
            classifications["text_0000"],
            "hold_structural_or_large_metadata_payload_no_write",
        )
        self.assertEqual(
            classifications["text_0001"],
            "hold_sensitive_reference_or_identifier_no_write",
        )
        self.assertEqual(
            classifications["text_0002"],
            "review_label_like_string_candidate_no_write",
        )
        self.assertTrue(all(row["write_allowed_now"] is False for row in report["classification_rows"]))

        for classified_row in report["classification_rows"]:
            self.assertFalse(
                {
                    "raw_text",
                    "trimmed_text",
                    "text_hash",
                    "bdata_value",
                    "attribute_values",
                    "replacement_text",
                }
                & set(classified_row)
            )

    def test_invalid_or_unsafe_inventory_is_rejected(self):
        roundtrip_path = Path("roundtrip.json").resolve()
        inventory = make_inventory(roundtrip_path)
        inventory["text_payload_exported"] = True
        with self.assertRaises(ValueError):
            aepx_redacted_text_classifier.build_redacted_text_classifier(
                inventory=inventory,
                inventory_path=Path("inventory.json").resolve(),
                roundtrip_validator=make_roundtrip(),
                roundtrip_validator_path=roundtrip_path,
            )

        inventory = make_inventory(roundtrip_path)
        inventory["text_nodes"][0]["raw_text"] = "do not export"
        with self.assertRaises(ValueError):
            aepx_redacted_text_classifier.build_redacted_text_classifier(
                inventory=inventory,
                inventory_path=Path("inventory.json").resolve(),
                roundtrip_validator=make_roundtrip(),
                roundtrip_validator_path=roundtrip_path,
            )

        inventory = make_inventory(roundtrip_path)
        inventory["source_aepx_roundtrip_validator"] = str(Path("other-roundtrip.json").resolve())
        with self.assertRaises(ValueError):
            aepx_redacted_text_classifier.build_redacted_text_classifier(
                inventory=inventory,
                inventory_path=Path("inventory.json").resolve(),
                roundtrip_validator=make_roundtrip(),
                roundtrip_validator_path=roundtrip_path,
            )

    def test_paths_are_confined_and_report_is_create_new(self):
        roundtrip_root = LAB_ROOT / "target" / "aepx-roundtrip-validator"
        inventory_root = LAB_ROOT / "target" / "aepx-redacted-text-inventory"
        roundtrip_root.mkdir(parents=True, exist_ok=True)
        inventory_root.mkdir(parents=True, exist_ok=True)
        stamp = f"{time.time_ns()}-{os.getpid()}"
        roundtrip_path = roundtrip_root / f"{stamp}-roundtrip.local.json"
        inventory_path = inventory_root / f"{stamp}-inventory.local.json"
        roundtrip_path.write_text(json.dumps(make_roundtrip()), encoding="utf-8")
        inventory_path.write_text(json.dumps(make_inventory(roundtrip_path)), encoding="utf-8")

        inventory, resolved_inventory = aepx_redacted_text_classifier.load_inventory(inventory_path)
        roundtrip, resolved_roundtrip = aepx_redacted_text_classifier.load_roundtrip_validator(roundtrip_path)
        self.assertEqual(resolved_inventory, inventory_path.resolve())
        self.assertEqual(resolved_roundtrip, roundtrip_path.resolve())

        outside = LAB_ROOT / "target" / f"{time.time_ns()}-{os.getpid()}-outside-inventory.json"
        outside.write_text(json.dumps(make_inventory(roundtrip_path)), encoding="utf-8")
        with self.assertRaises(ValueError):
            aepx_redacted_text_classifier.load_inventory(outside)

        report = aepx_redacted_text_classifier.build_redacted_text_classifier(
            inventory=inventory,
            inventory_path=resolved_inventory,
            roundtrip_validator=roundtrip,
            roundtrip_validator_path=resolved_roundtrip,
        )
        out = LAB_ROOT / "target" / "aepx-redacted-text-classifier" / f"{time.time_ns()}-{os.getpid()}-classifier.local.json"
        written = aepx_redacted_text_classifier.write_json_create_new(out, report)
        self.assertEqual(written, out.resolve())
        with self.assertRaises(FileExistsError):
            aepx_redacted_text_classifier.write_json_create_new(out, report)
        with self.assertRaises(ValueError):
            aepx_redacted_text_classifier.write_json_create_new(
                LAB_ROOT / "target" / "outside-redacted-text-classifier.json",
                report,
            )


if __name__ == "__main__":
    unittest.main()
