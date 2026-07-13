import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "tools" / "ae_scattermap_odd_probe.jsx"


class AeScatterMapOddProbeTests(unittest.TestCase):
    def test_probe_renders_odd_dimensions_without_existing_project(self):
        text = PROBE.read_text(encoding="utf-8")
        for expected in (
            "app.newProject()",
            'addComp("AEXCompatOddProbe", 13, 9',
            '{id:"identity", value:0}',
            '{id:"default", value:5}',
            '"dimensions":[13,9]',
            "output already exists",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
