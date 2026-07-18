import copy
import json
import unittest
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[1]
SCHEMAS = ROOT / "schemas"
FIXTURE = ROOT / "tests" / "fixtures" / "conformance" / "basic-manifest.json"


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def validator(name: str) -> Draft202012Validator:
    schema = load_json(SCHEMAS / name)
    manifest_schema = load_json(SCHEMAS / "conformance-manifest.schema.json")
    registry = Registry().with_resource(
        manifest_schema["$id"], Resource.from_contents(manifest_schema)
    )
    return Draft202012Validator(schema, registry=registry)


class ConformanceBundleSchemaTests(unittest.TestCase):
    def setUp(self):
        self.manifest_validator = validator("conformance-manifest.schema.json")
        self.report_validator = validator("conformance-report.schema.json")
        self.manifest = load_json(FIXTURE)

    def assert_invalid(self, validator_instance, value):
        self.assertTrue(list(validator_instance.iter_errors(value)))

    def test_example_manifest_is_valid(self):
        self.manifest_validator.validate(self.manifest)

    def test_schema_documents_are_valid_draft_2020_12(self):
        for path in SCHEMAS.glob("conformance-*.schema.json"):
            Draft202012Validator.check_schema(load_json(path))

    def test_manifest_rejects_unknown_field_absolute_path_and_bad_sha(self):
        for mutate in (
            lambda item: item.update({"unknown": True}),
            lambda item: item["input"].update({"path": "C:/private/input.png"}),
            lambda item: item["input"].update({"path": "inputs/../private.png"}),
            lambda item: item["runner"].update({"sha256": "not-a-sha"}),
            lambda item: item["runner"].update({"sha256": "A" * 64}),
        ):
            candidate = copy.deepcopy(self.manifest)
            mutate(candidate)
            self.assert_invalid(self.manifest_validator, candidate)

    def test_manifest_requires_captured_oracle_artifact(self):
        candidate = copy.deepcopy(self.manifest)
        candidate["oracle"] = {"state": "not_captured", "identity_match": False}
        self.manifest_validator.validate(candidate)
        candidate["oracle"]["artifact"] = self.manifest["oracle"]["artifact"]
        self.assert_invalid(self.manifest_validator, candidate)

    def valid_report(self):
        return {
            "schema_version": 1,
            "fixture_id": self.manifest["fixture_id"],
            "identities": {
                "aex": self.manifest["plugin"]["aex"],
                "dependencies": self.manifest["plugin"]["dependencies"],
                "input": self.manifest["input"],
                "runner": self.manifest["runner"],
            },
            "results": [{
                "depth": "argb8",
                "classification": "ok",
                "selector": {"render_path": "classic", "completed": True, "error_code": 0},
                "world": {"width": 2, "height": 2, "row_bytes": 8, "pixel_format": "argb8", "extent_hint": {"left": 0, "top": 0, "right": 2, "bottom": 2}},
                "output_sha256": "f" * 64,
                "oracle": {"state": "captured", "identity_match": True, "exact": True, "expected_sha256": "f" * 64, "actual_sha256": "f" * 64, "mismatched_pixels": 0},
            }],
        }

    def test_valid_report_captures_selector_world_classification_and_oracle(self):
        self.report_validator.validate(self.valid_report())

    def test_exact_rejected_without_capture_or_with_identity_mismatch(self):
        for state, identity_match in (("not_captured", False), ("captured", False)):
            report = self.valid_report()
            oracle = report["results"][0]["oracle"]
            oracle.update({"state": state, "identity_match": identity_match, "exact": True})
            if state != "captured":
                oracle.pop("expected_sha256")
                oracle.pop("actual_sha256")
                oracle.pop("mismatched_pixels")
            self.assert_invalid(self.report_validator, report)

    def test_report_rejects_unknown_field_absolute_identity_path_and_bad_sha(self):
        for mutate in (
            lambda item: item["results"][0].update({"unknown": True}),
            lambda item: item["identities"]["aex"].update({"path": "/tmp/plugin.aex"}),
            lambda item: item["results"][0].update({"output_sha256": "ABC"}),
        ):
            report = self.valid_report()
            mutate(report)
            self.assert_invalid(self.report_validator, report)


if __name__ == "__main__":
    unittest.main()
