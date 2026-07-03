import importlib.util
import sys
import time
import unittest
from pathlib import Path


LAB_ROOT = Path(__file__).resolve().parents[1]


def load_tool(name: str):
    path = LAB_ROOT / "tools" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ppm_fixture_tool = load_tool("ppm_fixture_tool")


class PpmFixtureToolTests(unittest.TestCase):
    def test_generate_identity_and_invert(self):
        image = ppm_fixture_tool.generate_image(4, 3, "gradient")
        self.assertEqual(image.width, 4)
        self.assertEqual(image.height, 3)
        self.assertEqual(len(image.pixels), 4 * 3 * 3)
        self.assertEqual(ppm_fixture_tool.transform_image(image, "identity"), image)
        inverted = ppm_fixture_tool.transform_image(image, "invert")
        self.assertEqual(inverted.pixels[0], 255 - image.pixels[0])

    def test_write_create_new_and_read(self):
        path = LAB_ROOT / "target" / "ppm-fixtures" / f"{time.time_ns()}-fixture.ppm"
        image = ppm_fixture_tool.generate_image(8, 8, "checker")
        ppm_fixture_tool.write_ppm_create_new(path, image)
        loaded = ppm_fixture_tool.read_ppm(path)
        self.assertEqual(loaded, image)
        with self.assertRaises(FileExistsError):
            ppm_fixture_tool.write_ppm_create_new(path, image)
        with self.assertRaises(ValueError):
            ppm_fixture_tool.write_ppm_create_new(LAB_ROOT / "target" / "outside.ppm", image)


if __name__ == "__main__":
    unittest.main()

