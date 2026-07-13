import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "tools" / "ae_scattermap_param_bounds_probe.jsx"


class AeScatterMapParamBoundsProbeTests(unittest.TestCase):
    def test_probe_uses_new_unsaved_project_and_fixed_boundary_cases(self):
        text = PROBE.read_text(encoding="utf-8")
        for expected in (
            "app.newProject()",
            "report already exists",
            '{name:"Scatter Amount", values:[-1, 501]}',
            '{name:"Direction", values:[0, 4]}',
            '{name:"Random Seed", values:[-1, 10001]}',
            '{name:"Mix with Original", values:[-0.1, 100.1]}',
            '{name:"Invert Map", values:[-1, 2]}',
            "property.setValue(value)",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
