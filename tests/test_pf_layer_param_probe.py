import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROBE = ROOT / "instruments" / "pf-layer-param-probe"


class PfLayerParamProbeSourceTest(unittest.TestCase):
    """The pf-layer-param-probe is the fixture the render_session_wrapper A/B
    (issue #195) needs: it declares a PF_Param_LAYER secondary layer plus a
    float slider and composites both on the CLASSIC render path. These
    machine-portable source assertions pin the invariants a silent edit could
    break without failing the gated A/B on a machine that never builds it."""

    def _source(self):
        return (PROBE / "pf_layer_param_probe.cpp").read_text(encoding="utf-8")

    def _code(self):
        # Strip line comments so a smart-render token named in the rationale
        # comment does not satisfy the "not present in code" assertions.
        return "\n".join(
            line
            for line in self._source().splitlines()
            if not line.lstrip().startswith("//")
        )

    def test_classic_render_path_only(self):
        source = self._source()
        code = self._code()
        self.assertIn("case PF_Cmd_RENDER:", source)
        # A smart-render flag would route the worker to the checkout path
        # instead of the classic params[] fill the layer slot is delivered
        # through, silently defeating the A/B.
        self.assertNotIn("PF_Cmd_SMART_RENDER", code)
        self.assertNotIn("PF_OutFlag2_SUPPORTS_SMART_RENDER", code)

    def test_declares_layer_then_slider_as_three_params(self):
        source = self._source()
        # params[0] is the implicit input; the layer must be the first declared
        # param (slot 1) and the slider the second (slot 2), matching the
        # InteractiveParameter slots the A/B binds.
        self.assertIn("kInput = 0, kLayer = 1, kSlider = 2, kNumParams = 3", source)
        self.assertIn("PF_ADD_LAYER(", source)
        self.assertIn("PF_ADD_FLOAT_SLIDERX(", source)
        self.assertIn("out_data->num_params = kNumParams;", source)

    def test_render_consumes_input_layer_and_slider(self):
        source = self._source()
        self.assertIn("params[kInput]->u.ld", source)
        self.assertIn("params[kLayer]->u.ld", source)
        self.assertIn("params[kSlider]->u.fs_d.value", source)



if __name__ == "__main__":
    unittest.main()
