import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ExtendedRenderResultTests(unittest.TestCase):
    def test_result_records_all_eight_cases(self):
        text = (ROOT / "analysis" / "SCATTERMAP_EXTENDED_RENDER_RESULT_2026-07-13.md").read_text(encoding="utf-8")
        for marker in ("amount-zero", "horizontal", "vertical", "37.5%", "13x9",
                       "padded", "5x3 map", "luminance inversion", "16 executions"):
            self.assertIn(marker, text)


if __name__ == "__main__":
    unittest.main()
