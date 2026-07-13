import unittest

from tools.scattermap_reference_oracle import hashes, render_default


class ScatterMapReferenceOracleTests(unittest.TestCase):
    def test_default_reference_hashes_match_observed_render(self):
        input_hash, output_hash = hashes()
        self.assertEqual(input_hash, "863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7")
        self.assertEqual(output_hash, "19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9")

    def test_reference_output_has_expected_size(self):
        self.assertEqual(len(render_default()), 16 * 12 * 4)


if __name__ == "__main__":
    unittest.main()
