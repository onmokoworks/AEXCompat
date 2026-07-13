import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "tools" / "ae_scattermap_roundtrip_probe.jsx"


class AeScatterMapRoundtripProbeTests(unittest.TestCase):
    def test_probe_uses_create_new_project_and_nondefault_seed(self):
        text = PROBE.read_text(encoding="utf-8")
        for expected in (
            "app.newProject()",
            "project output already exists",
            "setValue(10000)",
            "app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES)",
            "app.open(projectFile)",
            "seed did not persist",
            "saveFrameToPng",
        ):
            self.assertIn(expected, text)

    def test_probe_requires_supplied_paths(self):
        text = PROBE.read_text(encoding="utf-8")
        for variable in (
            "AEXCOMPAT_AE_INPUT",
            "AEXCOMPAT_AE_PROJECT",
            "AEXCOMPAT_AE_OUTPUT",
            "AEXCOMPAT_AE_REPORT",
        ):
            self.assertIn(variable, text)


if __name__ == "__main__":
    unittest.main()
