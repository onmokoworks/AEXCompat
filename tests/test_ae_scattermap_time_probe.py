import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "tools" / "ae_scattermap_time_probe.jsx"


class AeScatterMapTimeProbeTests(unittest.TestCase):
    def test_probe_renders_adjacent_frames_in_new_project(self):
        text = PROBE.read_text(encoding="utf-8")
        for expected in (
            "app.newProject()",
            "3 / 24",
            "var times = [0, 1 / 24]",
            "saveFrameToPng(times[i], output)",
            "output already exists for frame",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
