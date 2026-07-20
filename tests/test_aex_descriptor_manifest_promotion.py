import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def load_tool():
    path = ROOT / "tools/aex_descriptor_manifest_promotion.py"
    spec = importlib.util.spec_from_file_location("aex_descriptor_manifest_promotion", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


promotion = load_tool()


def promoted_manifest():
    return json.loads((ROOT / "profiles/scattermap/parameter_descriptors.json").read_text(encoding="utf-8"))


def l2_report(manifest):
    parameters = []
    for descriptor in manifest["descriptors"]:
        parameter = {
            "index": -1,
            "type": descriptor["observed_type"],
            "flags": 0,
            "name": descriptor["display_name"],
        }
        if descriptor["assignable"]:
            parameter.update(
                valid_min=descriptor["minimum"],
                valid_max=descriptor["maximum"],
                default=descriptor["default"],
            )
        parameters.append(parameter)
    return {
        "schema_version": 1,
        "stage": "L2",
        "plugin_id": manifest["plugin_id"],
        "expected_sha256": manifest["source"]["plugin_sha256"],
        "receipt_id": manifest["source"]["receipt_id"],
        "passed": True,
        "worker_exit": "ok",
        "worker_report": {
            "status": "selectors_completed",
            "params_setup_error": 0,
            "render_performed": False,
            "reported_num_params": len(parameters) + 1,
            "parameters": parameters,
        },
    }


class DescriptorManifestPromotionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        self.old_roots = promotion.LAB_ROOT, promotion.L2_ROOT, promotion.PROFILE_ROOT, promotion.OUTPUT_ROOT
        promotion.LAB_ROOT = root
        promotion.L2_ROOT = root / "target/l2-results"
        promotion.PROFILE_ROOT = root / "profiles"
        promotion.OUTPUT_ROOT = root / "target/descriptor-manifest-promotion"
        promotion.L2_ROOT.mkdir(parents=True)
        (promotion.PROFILE_ROOT / "scattermap").mkdir(parents=True)
        self.manifest = promoted_manifest()
        (promotion.PROFILE_ROOT / "scattermap/parameter_descriptors.json").write_text(
            json.dumps(self.manifest, indent=2) + "\n", encoding="utf-8"
        )

    def tearDown(self):
        promotion.LAB_ROOT, promotion.L2_ROOT, promotion.PROFILE_ROOT, promotion.OUTPUT_ROOT = self.old_roots
        self.temp.cleanup()

    def write_report(self, report):
        path = promotion.L2_ROOT / "source.json"
        path.write_text(json.dumps(report), encoding="utf-8")
        return path

    def test_exact_observation_regenerates_promoted_manifest(self):
        source = self.write_report(l2_report(self.manifest))
        candidate = promotion.OUTPUT_ROOT / "candidate.json"
        report = promotion.OUTPUT_ROOT / "report.json"
        self.assertTrue(
            promotion.run(
                source,
                promotion.PROFILE_ROOT / "scattermap/parameter_descriptors.json",
                candidate,
                report,
            )
        )
        self.assertEqual(json.loads(candidate.read_text(encoding="utf-8")), self.manifest)
        result = json.loads(report.read_text(encoding="utf-8"))
        self.assertTrue(result["matches_promoted"])
        self.assertEqual(result["candidate_canonical_sha256"], "C797EC7C45A603D2C86FB980DC5279E8B075D0E27D466315A2ABFA15BE608C37")
        self.assertFalse(result["native_process_started"])
        with self.assertRaises(FileExistsError):
            promotion.run(source, promotion.PROFILE_ROOT / "scattermap/parameter_descriptors.json", candidate, report)
        candidate.unlink()
        with self.assertRaises(FileExistsError):
            promotion.run(source, promotion.PROFILE_ROOT / "scattermap/parameter_descriptors.json", candidate, report)
        self.assertFalse(candidate.exists())

    def test_observed_range_change_requires_review(self):
        source_report = l2_report(self.manifest)
        source_report["worker_report"]["parameters"][0]["valid_max"] = 501
        source = self.write_report(source_report)
        candidate = promotion.OUTPUT_ROOT / "changed-candidate.json"
        report = promotion.OUTPUT_ROOT / "changed-report.json"
        self.assertFalse(
            promotion.run(source, promotion.PROFILE_ROOT / "scattermap/parameter_descriptors.json", candidate, report)
        )
        result = json.loads(report.read_text(encoding="utf-8"))
        self.assertFalse(result["matches_promoted"])
        self.assertNotEqual(result["candidate_canonical_sha256"], result["promoted_canonical_sha256"])

    def test_failed_l2_and_output_escape_are_rejected_without_outputs(self):
        failed = l2_report(self.manifest)
        failed["passed"] = False
        source = self.write_report(failed)
        candidate = promotion.OUTPUT_ROOT / "rejected-candidate.json"
        report = promotion.OUTPUT_ROOT / "rejected-report.json"
        with self.assertRaises(ValueError):
            promotion.run(source, promotion.PROFILE_ROOT / "scattermap/parameter_descriptors.json", candidate, report)
        self.assertFalse(candidate.exists())
        self.assertFalse(report.exists())
        with self.assertRaises(ValueError):
            promotion.resolve_json(Path("outside.json"), promotion.OUTPUT_ROOT, must_exist=False)


if __name__ == "__main__":
    unittest.main()
