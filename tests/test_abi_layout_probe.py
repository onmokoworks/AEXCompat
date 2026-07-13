import unittest
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class AbiLayoutProbeTests(unittest.TestCase):
    def test_probe_is_confined_to_instruments(self):
        source = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"
        text = source.read_text(encoding="utf-8")
        self.assertIn('#include "AE_Effect.h"', text)
        self.assertIn("offsetof(PF_InData", text)
        self.assertIn("offsetof(PF_OutData", text)
        for path in (ROOT / "minihost").rglob("*"):
            if path.suffix in {".cpp", ".h", ".hpp"}:
                self.assertNotIn("AE_Effect.h", path.read_text(encoding="utf-8"))

    def test_probe_covers_l2_layout_and_selector_inputs(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        for marker in (
            "pf_in_data_size", "pf_out_data_size", "pf_param_def_size",
            "in.pica_basicP", "out.my_version", "out.num_params",
            "inter.add_param", "param.param_type", "param.name",
            "PF_Cmd_GLOBAL_SETUP", "PF_Cmd_PARAMS_SETUP",
            "PF_Cmd_RENDER", "layer.rowbytes", "layer.data", "in.time_scale",
        ):
            self.assertIn(marker, text)

    def test_recorded_observation_is_no_load_and_x64(self):
        data = json.loads((ROOT / "analysis" / "AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json").read_text(encoding="utf-8"))
        self.assertEqual(data["pointer_size"], 8)
        self.assertEqual(data["pf_in_data_size"], 408)
        self.assertEqual(data["pf_out_data_size"], 408)
        self.assertEqual(data["pf_interact_callbacks_size"], 176)
        self.assertEqual(data["fields"]["inter.add_param"]["offset"], 16)
        self.assertEqual(data["fields"]["param.u"]["offset"], 56)
        self.assertEqual(data["pf_pixel_size"], 4)
        self.assertEqual(data["fields"]["layer.data"]["offset"], 24)
        self.assertEqual(data["fields"]["pixel.alpha"]["offset"], 0)
        self.assertEqual(data["selectors"]["render"], 11)
        self.assertEqual(data["selectors"]["smart_pre_render"], 23)
        self.assertEqual(data["selectors"]["smart_render"], 24)
        self.assertEqual(data["pf_smart_render_callbacks_size"], 24)
        self.assertFalse(data["native_aex_loaded"])
        self.assertFalse(data["selector_dispatched"])


if __name__ == "__main__":
    unittest.main()
