import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ClassicRenderContractTests(unittest.TestCase):
    def test_production_path_requires_v2_secure_launch_without_fallback(self):
        source = (ROOT / "broker" / "crates" / "broker" / "src" / "render.rs").read_text(encoding="utf-8")
        self.assertIn("load_v2_load_tree", source)
        self.assertIn("SealedLoadTree::create", source)
        self.assertIn("secure_launch(tree", source)
        self.assertNotIn("run_isolated", source)

    def test_registered_image_render_uses_the_v2_allowlist_and_load_tree(self):
        render = (ROOT / "broker" / "crates" / "broker" / "src" / "render.rs").read_text(encoding="utf-8")
        image = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text(encoding="utf-8")
        self.assertNotIn("ApprovedArtifact, load", render)
        self.assertNotIn("pub(crate) fn entry", render)
        self.assertIn("pub(crate) fn secure_entry", render)
        self.assertIn("crate::render::secure_entry(repository, plugin_id)?", image)
        self.assertNotIn("crate::render::entry", image)
        self.assertIn("approved\n        .dependencies", image)

    def test_each_determinism_run_reloads_receipt_and_builds_a_fresh_tree(self):
        source = (ROOT / "broker" / "crates" / "broker" / "src" / "render.rs").read_text(encoding="utf-8")
        loop = source.index("for _ in 0..2")
        reload = source.index("let entry = secure_entry(repository, id)?;", loop)
        create = source.index("SealedLoadTree::create", reload)
        launch = source.index("secure_launch(tree", create)
        self.assertLess(loop, reload)
        self.assertLess(reload, create)
        self.assertLess(create, launch)

    def test_contract_requires_two_hash_bound_runs_and_guards(self):
        schema = json.loads((ROOT / "contracts" / "aex" / "classic_render_report.schema.json").read_text(encoding="utf-8"))
        required = set(schema["required"])
        for key in ("input_sha256", "run_1", "run_2", "deterministic",
                    "guard_bytes_intact", "broker_survived"):
            self.assertIn(key, required)
        self.assertEqual(schema["properties"]["pixel_format"]["const"], "argb8")

    def test_contract_bounds_dimensions_and_runtime(self):
        schema = json.loads((ROOT / "contracts" / "aex" / "classic_render_report.schema.json").read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["width"]["maximum"], 1024)
        self.assertEqual(schema["properties"]["height"]["maximum"], 1024)
        self.assertEqual(schema["$defs"]["run"]["properties"]["elapsed_ms"]["maximum"], 30000)


if __name__ == "__main__":
    unittest.main()
