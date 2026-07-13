import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SCATTERMAP_CLASSIC_EXTENT_RESULT_2026-07-13.md"


class ClassicExtentResultTests(unittest.TestCase):
    def test_result_records_extent_contract_and_production_boundary(self):
        text = RESULT.read_text(encoding="utf-8")
        for expected in (
            "offset 260",
            "left=3, top=2",
            "right=11, bottom=8",
            "full-world default oracle",
            "guard_bytes_intact: true",
            "does not claim a",
            "production AE cropped-world observation",
            "No diagnostic AEX remains installed",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
