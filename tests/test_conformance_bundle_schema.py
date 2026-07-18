import copy
import json
import unittest
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

from tools.conformance_bundle_validator import BundleValidationError, validate_bundle


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

    def test_example_manifest_and_schema_documents_are_valid(self):
        self.manifest_validator.validate(self.manifest)
        for path in SCHEMAS.glob("conformance-*.schema.json"):
            Draft202012Validator.check_schema(load_json(path))

    def test_manifest_rejects_unknown_absolute_parent_bad_sha_and_missing_execution(self):
        for mutate in (
            lambda item: item.update({"unknown": True}),
            lambda item: item["input"].update({"path": "C:/private/input.png"}),
            lambda item: item["input"].update({"path": "inputs/../private.png"}),
            lambda item: item["runner"].update({"sha256": "not-a-sha"}),
            lambda item: item.pop("execution"),
            lambda item: item["execution"].pop("time"),
        ):
            candidate = copy.deepcopy(self.manifest)
            mutate(candidate)
            self.assert_invalid(self.manifest_validator, candidate)

    def test_manifest_rejects_windows_unsafe_paths(self):
        for path in (
            "inputs/./control.png", "inputs/CON", "inputs/con.txt",
            "inputs/Lpt1.bin", "inputs/trailing.", "inputs/trailing ",
        ):
            candidate = copy.deepcopy(self.manifest)
            candidate["input"]["path"] = path
            self.assert_invalid(self.manifest_validator, candidate)

    def test_manifest_requires_captured_oracle_artifact(self):
        candidate = copy.deepcopy(self.manifest)
        candidate["oracle"] = {"state": "not_captured", "identity_match": False}
        self.manifest_validator.validate(candidate)
        candidate["oracle"]["artifact"] = self.manifest["oracle"]["artifact"]
        self.assert_invalid(self.manifest_validator, candidate)

    def valid_report(self):
        artifact = lambda path, sha: {"path": path, "sha256": sha * 64, "size_bytes": 64}
        world = {"width": 2, "height": 2, "row_bytes": 8, "pixel_format": "argb8", "premultiplication": "premultiplied", "extent_hint": {"left": 0, "top": 0, "right": 2, "bottom": 2}}
        results = []
        for depth in self.manifest["requested_depths"]:
            depth_world = copy.deepcopy(world)
            depth_world["pixel_format"] = depth
            results.append({
                "depth": depth, "classification": "ok",
                "selector": {"render_path": "classic", "completed": True, "error_code": 0},
                "input_world": depth_world, "world": depth_world,
                "raw_input": artifact(f"raw/{depth}-input.bin", "1"),
                "raw_output": artifact(f"raw/{depth}-output.bin", "2"),
                "output_sha256": "2" * 64,
                "suite_timeline": [{"sequence": 0, "action": "acquire", "name": "PF World Suite", "version": 2, "selector": "PF_Cmd_RENDER", "result": 0}],
                "oracle": {"state": "captured", "identity_match": True, "exact": True, "expected_sha256": "2" * 64, "actual_sha256": "2" * 64, "mismatched_pixels": 0},
            })
        return {
            "schema_version": 1, "fixture_id": self.manifest["fixture_id"],
            "identities": copy.deepcopy({"aex": self.manifest["plugin"]["aex"], "dependencies": self.manifest["plugin"]["dependencies"], "input": self.manifest["input"], "runner": self.manifest["runner"]}),
            "parameters": [{"index": 1, "type": "slider", "initial_value": 50, "host_range": {"minimum": 0, "maximum": 100}, "user_range": {"minimum": 0, "maximum": 100}}],
            "results": results,
        }

    def test_valid_bundle_captures_required_evidence(self):
        report = self.valid_report()
        self.report_validator.validate(report)
        validate_bundle(self.manifest, report)

    def test_report_rejects_unknown_absolute_identity_and_missing_evidence(self):
        for mutate in (
            lambda item: item["results"][0].update({"unknown": True}),
            lambda item: item["identities"]["aex"].update({"path": "/tmp/plugin.aex"}),
            lambda item: item["results"][0].pop("input_world"),
            lambda item: item["results"][0].pop("raw_input"),
            lambda item: item["results"][0].pop("suite_timeline"),
            lambda item: item.pop("parameters"),
        ):
            report = self.valid_report()
            mutate(report)
            self.assert_invalid(self.report_validator, report)

    def test_semantic_validator_rejects_identity_fixture_and_depth_mismatch(self):
        mutations = (
            lambda report: report["identities"]["aex"].update({"sha256": "9" * 64}),
            lambda report: report.update({"fixture_id": "other"}),
            lambda report: report["results"].pop(),
            lambda report: report["results"].__setitem__(1, copy.deepcopy(report["results"][0])),
        )
        for mutate in mutations:
            report = self.valid_report()
            mutate(report)
            with self.assertRaises(BundleValidationError):
                validate_bundle(self.manifest, report)

    def test_semantic_validator_rejects_exact_with_different_hashes(self):
        report = self.valid_report()
        report["results"][0]["oracle"]["actual_sha256"] = "3" * 64
        self.report_validator.validate(report)
        with self.assertRaisesRegex(BundleValidationError, "exact oracle hashes differ"):
            validate_bundle(self.manifest, report)

    def test_exact_rejected_without_capture_or_identity_match(self):
        for state, identity_match in (("not_captured", False), ("captured", False)):
            report = self.valid_report()
            oracle = report["results"][0]["oracle"]
            oracle.update({"state": state, "identity_match": identity_match, "exact": True})
            if state != "captured":
                oracle.pop("expected_sha256")
                oracle.pop("actual_sha256")
                oracle.pop("mismatched_pixels")
            self.assert_invalid(self.report_validator, report)


if __name__ == "__main__":
    unittest.main()
