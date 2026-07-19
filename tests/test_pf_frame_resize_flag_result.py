import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RENDER = ROOT / "minihost" / "src" / "render_subsystem.cpp"
WORKER = ROOT / "minihost" / "src" / "l2_main.cpp"


class PfFrameResizeFlagResultTest(unittest.TestCase):
    def test_evidence_covers_allowed_and_denied_resize_contracts(self):
        data = json.loads((ROOT / "analysis" / "PF_FRAME_RESIZE_FLAG_RESULT_2026-07-15.json").read_text())
        cases = {case["fixture"]: case for case in data["cases"]}
        self.assertEqual(data["sdk_constants"]["PF_OutFlag_I_EXPAND_BUFFER"], 512)
        self.assertEqual(data["sdk_constants"]["PF_OutFlag_I_SHRINK_BUFFER"], 4096)
        self.assertEqual(cases["pf_expand_allowed_probe"]["output_dimensions"], [20, 16])
        self.assertEqual(cases["pf_shrink_allowed_probe"]["output_dimensions"], [12, 8])
        for name in ("pf_expand_denied_probe", "pf_shrink_denied_probe"):
            self.assertEqual(cases[name]["render_error"], 4)
            self.assertFalse(cases[name]["render_selector_dispatched"])
            self.assertTrue(cases[name]["guard_bytes_intact"])

    def test_worker_enforces_flags_before_render(self):
        render = RENDER.read_text(encoding="utf-8")
        worker = WORKER.read_text(encoding="utf-8")
        # Request-shaping is shared by Classic and SmartFX; L2 only invokes
        # the extracted validator before replacing the guarded output world.
        validator = render[render.index("bool validate_output_extent("):
                           render.index("SmartOutputBounds prepare_smart_output_bounds")]
        self.assertIn("const bool expands", validator)
        self.assertIn("const bool shrinks", validator)
        self.assertIn("(output_flags & kExpandBuffer) != 0", validator)
        self.assertIn("(output_flags & kShrinkBuffer) != 0", validator)
        validate = worker.index("aexcompat::render::validate_output_extent(")
        resize = worker.index("width = requested_width;", validate)
        self.assertLess(validate, resize)
        self.assertIn("g_classic_render_selector_dispatched = true", worker)
        fixture = (ROOT / "instruments" / "pf-frame-resize-probe" / "pf_frame_resize_probe.cpp").read_text()
        self.assertIn("PF_Cmd_FRAME_SETUP", fixture)
        self.assertIn("PF_Err_INTERNAL_STRUCT_DAMAGED", fixture)

    def test_broker_and_harness_expose_isolated_resize_probes(self):
        broker = (ROOT / "broker" / "crates" / "broker" / "src" / "image_render.rs").read_text()
        harness = (ROOT / "broker" / "crates" / "harness" / "src" / "main.rs").read_text()
        for direction in ("expand", "shrink"):
            self.assertIn(f"probe_experimental_{direction}_buffer", broker)
            self.assertIn(f"--probe-experimental-{direction}-buffer", harness)
        self.assertIn("FRAME_SETUP resize probe failed safely", broker)
        self.assertIn("render_selector_dispatched", broker)
        self.assertIn("Probe FRAME_SETUP expansion", harness)
        self.assertIn("Probe FRAME_SETUP shrink", harness)


if __name__ == "__main__":
    unittest.main()
