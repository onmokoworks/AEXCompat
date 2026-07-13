import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "tools" / "ae_scattermap_roi_aegp_probe.jsx"


class AeScatterMapRoiAegpProbeTests(unittest.TestCase):
    def test_probe_uses_new_unsaved_project_and_fixed_command(self):
        text = PROBE.read_text(encoding="utf-8")
        for expected in (
            "app.newProject()",
            'addComp("AEXCompatRoiAegpProbe", 16, 12',
            'addProperty("ScatterMap")',
            'findMenuCommandId("AEXCompat ROI Probe")',
            "roi_aegp_command_id ([0-9]+)",
            "app.executeCommand(commandId)",
            "CloseOptions.DO_NOT_SAVE_CHANGES",
        ):
            self.assertIn(expected, text)


if __name__ == "__main__":
    unittest.main()
