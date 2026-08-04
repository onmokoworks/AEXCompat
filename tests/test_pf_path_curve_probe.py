import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments" / "pf-path-curve-probe"


class PfPathCurveProbeSourceTest(unittest.TestCase):







    def test_dedicated_ae_runner_uses_large_opaque_mask_carrier(self):
        runner = (ROOT / "tools" / "ae-path-curve-oracle-run.jsx").read_text(
            encoding="utf-8"
        )
        for marker in (
            'addComp("AEXCompat Path Curve Oracle", 128, 64',
            'addProperty("ADBE Mask Atom")',
            "shape.inTangents",
            "shape.outTangents",
            "pathParameter.setValue(1)",
            "comp.saveFrameToPng(0, output)",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(marker, runner)


if __name__ == "__main__":
    unittest.main()
