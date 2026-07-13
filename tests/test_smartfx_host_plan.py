import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SmartFxHostPlanTests(unittest.TestCase):
    def test_plan_binds_selectors_callbacks_and_classic_oracle(self):
        text = (ROOT / "analysis" / "SCATTERMAP_SMARTFX_HOST_PLAN_2026-07-13.md").read_text(encoding="utf-8")
        for marker in ("selector 23", "selector 24", "24-byte callback", "19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9", "GPU callbacks"):
            self.assertIn(marker, text)


if __name__ == "__main__":
    unittest.main()
