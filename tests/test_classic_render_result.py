import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ClassicRenderResultTests(unittest.TestCase):
    def test_result_binds_measured_and_oracle_hashes(self):
        text = (ROOT / "analysis" / "SCATTERMAP_CLASSIC_RENDER_RESULT_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7", text)
        self.assertIn("19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9", text)
        self.assertIn("guard bytes intact: true", text)
        self.assertIn("independently implements", text)

    def test_result_does_not_overclaim_scope(self):
        text = (ROOT / "analysis" / "SCATTERMAP_CLASSIC_RENDER_RESULT_2026-07-13.md").read_text(encoding="utf-8")
        for gap in ("non-default parameters", "connected map layers", "SmartFX", "16-bpc/32-bpc"):
            self.assertIn(gap, text)


if __name__ == "__main__":
    unittest.main()
