import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class ExtendedRenderResultTests(unittest.TestCase):
    def test_result_records_eleven_cases_and_both_render_paths(self):
        text = (ROOT / "analysis" / "SCATTERMAP_EXTENDED_RENDER_RESULT_2026-07-13.md").read_text(encoding="utf-8")
        for marker in ("amount-zero", "horizontal", "vertical", "37.5%", "amount 500",
                       "seed 10000", "mix 0%", "13x9", "padded", "5x3 map",
                       "luminance inversion", "classic and SmartFX", "broker now enforces"):
            self.assertIn(marker, text)


if __name__ == "__main__":
    unittest.main()
