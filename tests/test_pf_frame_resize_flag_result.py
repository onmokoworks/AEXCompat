import json
import unittest
from pathlib import Path
import source_owners

ROOT = Path(__file__).resolve().parents[1]
RENDER = ROOT / "minihost" / "src" / "render_subsystem.cpp"
CLASSIC_RUNTIME = ROOT / "minihost" / "src" / "worker_classic_runtime.cpp"

class PfFrameResizeFlagResultTest(unittest.TestCase):
    def test_evidence_covers_allowed_and_denied_resize_contracts(self):
        data = json.loads((ROOT / "analysis" / "PF_FRAME_RESIZE_FLAG_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
        cases = {case["fixture"]: case for case in data["cases"]}
        self.assertEqual(data["sdk_constants"]["PF_OutFlag_I_EXPAND_BUFFER"], 512)
        self.assertEqual(data["sdk_constants"]["PF_OutFlag_I_SHRINK_BUFFER"], 4096)
        self.assertEqual(cases["pf_expand_allowed_probe"]["output_dimensions"], [20, 16])
        self.assertEqual(cases["pf_shrink_allowed_probe"]["output_dimensions"], [12, 8])
        for name in ("pf_expand_denied_probe", "pf_shrink_denied_probe"):
            self.assertEqual(cases[name]["render_error"], 4)
            self.assertFalse(cases[name]["render_selector_dispatched"])
            self.assertTrue(cases[name]["guard_bytes_intact"])

    def test_broker_and_harness_expose_isolated_resize_probes(self):
        broker = source_owners.IMAGE_RENDER_SOURCE.read_text(encoding="utf-8")
        harness = source_owners.harness_windows_text()
        for direction in ("expand", "shrink"):
            self.assertIn(f"probe_experimental_{direction}_buffer", broker)
            self.assertIn(f"--probe-experimental-{direction}-buffer", harness)
        self.assertIn("FRAME_SETUP resize probe failed safely", broker)
        self.assertIn("render_selector_dispatched", broker)
        self.assertIn("Probe FRAME_SETUP expansion", harness)
        self.assertIn("Probe FRAME_SETUP shrink", harness)

if __name__ == "__main__":
    unittest.main()
