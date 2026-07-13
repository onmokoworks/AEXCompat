import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "tools" / "ae_scattermap_alpha_probe.jsx"


class AeScatterMapAlphaProbeTests(unittest.TestCase):
    def test_probe_captures_identity_and_default_without_existing_project(self):
        text = PROBE.read_text(encoding="utf-8")
        for expected in (
            "app.newProject()",
            'input_kind":"variable_alpha_rgba8',
            '{id:"identity", value:0}',
            '{id:"default", value:5}',
            "output already exists",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
