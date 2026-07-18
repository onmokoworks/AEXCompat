import copy
import hashlib
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.bundle_root = Path(self.temp.name)
        for label, artifact in self.manifest_artifacts():
            oracle_sizes = {"oracle-argb8": 16, "oracle-argb16": 32, "oracle-argb32f": 64}
            size = oracle_sizes.get(label, artifact["size_bytes"])
            self.materialize(artifact, (label.encode("ascii") * (size + len(label)))[:size])

    def manifest_artifacts(self):
        yield "aex", self.manifest["plugin"]["aex"]
        for index, artifact in enumerate(self.manifest["plugin"]["dependencies"]):
            yield f"dependency-{index}", artifact
        yield "input", self.manifest["input"]
        yield "runner", self.manifest["runner"]
        for depth, artifact in self.manifest["oracle"]["artifacts"].items():
            yield f"oracle-{depth}", artifact

    def materialize(self, artifact, content):
        path = self.bundle_root.joinpath(*artifact["path"].split("/"))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        artifact["size_bytes"] = len(content)
        artifact["sha256"] = hashlib.sha256(content).hexdigest()
        return artifact

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
        candidate["oracle"]["artifacts"] = self.manifest["oracle"]["artifacts"]
        self.assert_invalid(self.manifest_validator, candidate)

    def valid_report(self):
        world = {"width": 2, "height": 2, "row_bytes": 8, "pixel_format": "argb8", "premultiplication": "premultiplied", "extent_hint": {"left": 0, "top": 0, "right": 2, "bottom": 2}}
        results = []
        for depth in self.manifest["requested_depths"]:
            depth_world = copy.deepcopy(world)
            depth_world["pixel_format"] = depth
            depth_world["row_bytes"] = 2 * {"argb8": 4, "argb16": 8, "argb32f": 16}[depth]
            raw_size = depth_world["row_bytes"] * depth_world["height"]
            raw_input = self.materialize(
                {"path": f"raw/{depth}-input.bin"}, bytes([1]) * raw_size
            )
            oracle_artifact = self.manifest["oracle"]["artifacts"][depth]
            output_content = self.bundle_root.joinpath(
                *oracle_artifact["path"].split("/")
            ).read_bytes()
            raw_output = self.materialize(
                {"path": f"raw/{depth}-output.bin"}, output_content
            )
            output_sha = raw_output["sha256"]
            oracle = {"state": "captured", "identity_match": True, "exact": True,
                      "expected_sha256": oracle_artifact["sha256"],
                      "actual_sha256": output_sha, "mismatched_pixels": 0}
            results.append({
                "depth": depth, "classification": "ok",
                "selector": {"render_path": "classic", "completed": True, "error_code": 0},
                "input_world": depth_world, "world": depth_world,
                "raw_input": raw_input,
                "raw_output": raw_output,
                "output_sha256": output_sha,
                "suite_timeline": [{"sequence": 0, "action": "acquire", "name": "PF World Suite", "version": 2, "selector": "PF_Cmd_RENDER", "result": 0}],
                "oracle": oracle,
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
        validate_bundle(self.manifest, report, self.bundle_root)

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
                validate_bundle(self.manifest, report, self.bundle_root)

    def test_semantic_validator_rejects_exact_with_different_hashes(self):
        report = self.valid_report()
        report["results"][0]["oracle"]["actual_sha256"] = "3" * 64
        self.report_validator.validate(report)
        with self.assertRaisesRegex(BundleValidationError, "exact oracle hashes differ"):
            validate_bundle(self.manifest, report, self.bundle_root)

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

    def test_validator_requires_bundle_root_and_verifies_artifact_bytes(self):
        report = self.valid_report()
        with self.assertRaises(TypeError):
            validate_bundle(self.manifest, report)

        input_path = self.bundle_root / self.manifest["input"]["path"]
        input_path.write_bytes(b"tampered")
        with self.assertRaisesRegex(BundleValidationError, "size does not match"):
            validate_bundle(self.manifest, report, self.bundle_root)
        input_path.write_bytes(b"x" * self.manifest["input"]["size_bytes"])
        with self.assertRaisesRegex(BundleValidationError, "SHA-256"):
            validate_bundle(self.manifest, report, self.bundle_root)

    def test_validator_rejects_missing_and_symlink_artifacts(self):
        report = self.valid_report()
        runner_path = self.bundle_root / self.manifest["runner"]["path"]
        runner_path.unlink()
        with self.assertRaisesRegex(BundleValidationError, "does not exist"):
            validate_bundle(self.manifest, report, self.bundle_root)

        report = self.valid_report()
        runner_path.parent.mkdir(parents=True, exist_ok=True)
        target = self.bundle_root / "real-runner.exe"
        target.write_bytes(b"runner")
        try:
            runner_path.symlink_to(target)
        except OSError as error:
            self.skipTest(f"symlink creation is unavailable: {error}")
        with self.assertRaisesRegex(BundleValidationError, "reparse point|symlink"):
            validate_bundle(self.manifest, report, self.bundle_root)

    def test_validator_rejects_disconnected_oracle_hash_chain(self):
        mutations = (
            lambda result: result["oracle"].update({"expected_sha256": "3" * 64}),
            lambda result: result.update({"output_sha256": "3" * 64}),
            lambda result: result["raw_output"].update({"sha256": "3" * 64}),
        )
        for mutate in mutations:
            report = self.valid_report()
            mutate(report["results"][0])
            with self.assertRaises(BundleValidationError):
                validate_bundle(self.manifest, report, self.bundle_root)

    def test_validator_binds_non_oracle_output_hash_to_raw_output(self):
        report = self.valid_report()
        self.manifest["oracle"] = {"state": "not_captured", "identity_match": False}
        for result in report["results"]:
            result["oracle"] = {
                "state": "not_captured",
                "identity_match": False,
                "exact": False,
            }
        report["results"][0]["output_sha256"] = "3" * 64

        with self.assertRaisesRegex(BundleValidationError, "output hash does not match raw output"):
            validate_bundle(self.manifest, report, self.bundle_root)

    def test_validator_binds_each_oracle_to_its_depth(self):
        report = self.valid_report()
        report["results"][1]["oracle"]["expected_sha256"] = self.manifest["oracle"]["artifacts"]["argb8"]["sha256"]
        with self.assertRaisesRegex(BundleValidationError, "expected oracle hash"):
            validate_bundle(self.manifest, report, self.bundle_root)

        manifest = copy.deepcopy(self.manifest)
        manifest["oracle"]["artifacts"].pop("argb32f")
        with self.assertRaisesRegex(BundleValidationError, "oracle depths"):
            validate_bundle(manifest, self.valid_report(), self.bundle_root)

    def test_validator_binds_report_oracle_identity_to_manifest(self):
        report = self.valid_report()
        self.manifest["oracle"]["identity_match"] = False
        with self.assertRaisesRegex(BundleValidationError, "identity_match does not match manifest"):
            validate_bundle(self.manifest, report, self.bundle_root)

    @unittest.skipUnless(os.name == "nt", "Windows junction regression")
    def test_artifact_open_rejects_intermediate_junction_inside_bundle(self):
        report = self.valid_report()
        target_dir = self.bundle_root / "real-runner"
        target_dir.mkdir()
        content = b"junction-artifact"
        (target_dir / "runner.exe").write_bytes(content)
        junction = self.bundle_root / "alias-runner"
        created = subprocess.run(
            ["cmd", "/c", "mklink", "/J", str(junction), str(target_dir)],
            capture_output=True,
            text=True,
        )
        if created.returncode:
            self.skipTest(f"junction creation is unavailable: {created.stderr}")

        artifact = {
            "path": "alias-runner/runner.exe",
            "sha256": hashlib.sha256(content).hexdigest(),
            "size_bytes": len(content),
        }
        self.manifest["runner"] = artifact
        report["identities"]["runner"] = copy.deepcopy(artifact)
        with self.assertRaisesRegex(BundleValidationError, "reparse point|component handle"):
            validate_bundle(self.manifest, report, self.bundle_root)

    @unittest.skipUnless(os.name == "posix", "POSIX dirfd race regression")
    def test_artifact_open_cannot_be_redirected_by_absolute_path_race(self):
        report = self.valid_report()
        artifact = report["results"][0]["raw_input"]
        candidate = self.bundle_root.joinpath(*artifact["path"].split("/"))
        outside = self.bundle_root.parent / "outside-secret.bin"
        outside.write_bytes(b"outside-secret")
        artifact["size_bytes"] = outside.stat().st_size
        artifact["sha256"] = hashlib.sha256(outside.read_bytes()).hexdigest()
        original_open = os.open

        def raced_open(path, flags, *args, **kwargs):
            if Path(path) == candidate:
                return original_open(outside, flags, *args, **kwargs)
            return original_open(path, flags, *args, **kwargs)

        with patch("tools.conformance_bundle_validator.os.open", side_effect=raced_open):
            with self.assertRaises(BundleValidationError):
                validate_bundle(self.manifest, report, self.bundle_root)

    def test_exact_requires_successful_real_output(self):
        report = self.valid_report()
        result = report["results"][0]
        result.update({"classification": "crashed", "world": None,
                       "raw_output": None, "output_sha256": None})
        result["selector"] = {"render_path": "classic", "completed": False, "error_code": None}
        with self.assertRaisesRegex(BundleValidationError, "successful real output"):
            validate_bundle(self.manifest, report, self.bundle_root)

    def test_validator_rejects_incoherent_world_layouts(self):
        mutations = (
            lambda result: result["input_world"].update({"pixel_format": "argb8"}),
            lambda result: result["world"].update({"row_bytes": 1}),
            lambda result: result["world"]["extent_hint"].update({"right": 3}),
        )
        for mutate in mutations:
            report = self.valid_report()
            mutate(report["results"][1])
            with self.assertRaises(BundleValidationError):
                validate_bundle(self.manifest, report, self.bundle_root)


if __name__ == "__main__":
    unittest.main()
