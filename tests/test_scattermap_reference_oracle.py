import unittest
import hashlib
import json
from pathlib import Path

from tools.scattermap_reference_oracle import generated_luma_map, gradient, hashes, render_case, render_default


class ScatterMapReferenceOracleTests(unittest.TestCase):
    def test_default_reference_hashes_match_observed_render(self):
        input_hash, output_hash = hashes()
        self.assertEqual(input_hash, "863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7")
        self.assertEqual(output_hash, "19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9")

    def test_reference_output_has_expected_size(self):
        self.assertEqual(len(render_default()), 16 * 12 * 4)

    def test_extended_cases_exercise_distinct_behavior(self):
        cases = {
            "identity": render_case(amount=0),
            "horizontal": render_case(amount=9, direction=1, seed=17),
            "vertical_no_repeat": render_case(amount=7, direction=2, repeat_edge=False),
            "mixed": render_case(amount=12, direction=3, seed=991, mix=37.5),
            "odd_dimensions": render_case(13, 9, amount=4, seed=3),
        }
        self.assertEqual(len({value for value in cases.values()}), len(cases))
        self.assertEqual(cases["identity"], gradient(16, 12))

    def test_recorded_extended_hashes_are_reproducible(self):
        root = Path(__file__).resolve().parents[1]
        record = json.loads((root / "analysis" / "SCATTERMAP_EXTENDED_ORACLE_HASHES_2026-07-13.json").read_text(encoding="utf-8"))
        self.assertTrue(record["native_aex_loaded"])
        for case in record["cases"]:
            params = dict(case["parameters"])
            params["repeat_edge"] = params.pop("repeat_edge")
            if "map" in case:
                map_spec = case["map"]
                params["luma_map"] = generated_luma_map(
                    case["width"], case["height"], map_spec["width"], map_spec["height"], map_spec["invert"])
            output = render_case(case["width"], case["height"], **params)
            self.assertEqual(hashlib.sha256(output).hexdigest().upper(), case["expected_output_sha256"])

    def test_connected_map_oracles_are_distinct(self):
        connected = render_case(11, 7, luma_map=generated_luma_map(11, 7, 5, 3))
        inverted = render_case(11, 7, luma_map=generated_luma_map(11, 7, 11, 7, True))
        self.assertNotEqual(connected, inverted)


if __name__ == "__main__":
    unittest.main()
