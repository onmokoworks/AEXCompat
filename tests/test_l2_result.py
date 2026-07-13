import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class L2ResultTests(unittest.TestCase):
    def test_l2_result_records_exact_parameter_order(self):
        text = (ROOT / "analysis" / "SCATTERMAP_L2_RESULT_2026-07-13.md").read_text(encoding="utf-8")
        for name in ("Scatter Amount", "Direction", "Random Seed", "Repeat Edge Pixels",
                     "Mix with Original", "Scatter Map", "Invert Map"):
            self.assertIn(name, text)
        self.assertIn("reported parameter count: 8", text)
        self.assertIn("render performed: false", text)
        for expected in ("valid 0..500", "Horizontal|Vertical|Both", "default true",
                         "default 100, precision 1", "default false"):
            self.assertIn(expected, text)
        self.assertIn("current=0", text)
        self.assertIn("default=1", text)
        self.assertIn("omitting Repeat Edge", text)

    def test_l2_receipt_excludes_render(self):
        text = (ROOT / "analysis" / "SCATTERMAP_L2_APPROVAL_RECEIPT_2026-07-13.md").read_text(encoding="utf-8")
        self.assertIn("approve_native_selector_dispatch_l2", text)
        self.assertIn("does not approve", text)
        self.assertIn("render selectors", text)


if __name__ == "__main__":
    unittest.main()
