import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "broker/crates/broker/src/host_core"
FIXTURES = ROOT / "broker/crates/broker/src/fixture_profiles"


class HostCoreBoundaryTests(unittest.TestCase):
    def test_generic_core_contains_no_scattermap_identity_or_oracle(self):
        files = sorted(CORE.rglob("*.rs"))
        self.assertTrue(files)
        text = "\n".join(path.read_text(encoding="utf-8") for path in files).lower()
        for forbidden in (
            "scattermap",
            "scatter amount",
            "random seed",
            "mix with original",
            "223ff5ec",
            "hash_pixel",
        ):
            self.assertNotIn(forbidden, text)
        approval = (CORE / "approved_artifact.rs").read_text(encoding="utf-8")
        self.assertIn("entry.sha256.len() != 64", approval)
        self.assertIn("is_ascii_hexdigit", approval)

    def test_fixture_manifest_owns_target_descriptors_and_adapter_owns_oracle(self):
        source = (FIXTURES / "scattermap.rs").read_text(encoding="utf-8")
        for marker in ('PROFILE_ID: &str = "scattermap"', "pub fn argb8_hash"):
            self.assertIn(marker, source)
        self.assertNotIn('display_name: "Scatter Amount"', source)
        manifest = (ROOT / "profiles/scattermap/parameter_descriptors.json").read_text(encoding="utf-8")
        for marker in ('"display_name": "Scatter Amount"', '"display_name": "Mix with Original"',
                       '"plugin_sha256": "223FF5EC'):
            self.assertIn(marker, manifest)

    def test_registry_rejects_unknown_profiles_explicitly(self):
        source = (FIXTURES / "mod.rs").read_text(encoding="utf-8")
        self.assertIn("pub fn find", source)
        self.assertIn("ParameterizedRenderAdapter", source)
        self.assertIn("descriptor_manifest: ManifestPolicy", source)
        self.assertIn("_ => None", source)
        self.assertNotIn("unwrap_or", source)

    def test_design_does_not_claim_one_fixture_proves_general_support(self):
        text = (ROOT / "docs/HOST_CORE_BOUNDARY_2026-07-13.md").read_text(encoding="utf-8")
        for marker in ("not the product architecture", "second owner-authored AEX",
                       "cannot be called general solely because ScatterMap passes"):
            self.assertIn(marker, text)


if __name__ == "__main__":
    unittest.main()
