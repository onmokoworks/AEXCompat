import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_AE_PARAM_BOUNDS_RESULT_2026-07-13.md"


class AeParamBoundsResultTests(unittest.TestCase):
    def test_result_records_rejection_not_clamping(self):
        text = RESULT.read_text(encoding="utf-8")
        for expected in (
            "AE 25.2",
            "Scatter Amount | -1, 501",
            "Direction | 0, 4",
            "Random Seed | -1, 10001",
            "Mix with Original | -0.1, 100.1",
            "Invert Map | -1, 2",
            "did not clamp",
            "rejected_count: 10",
            "must reject these",
            "out-of-range values before dispatch",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
