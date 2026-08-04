import importlib.util
import sys
import os
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


aex_no_load_worker = load_tool("aex_no_load_worker")
ppm_fixture_tool = load_tool("ppm_fixture_tool")


def create_ppm_fixture() -> Path:
    root = LAB_ROOT / "target" / "ppm-fixtures"
    root.mkdir(parents=True, exist_ok=True)
    path = root / f"{time.time_ns()}-{os.getpid()}-worker-input.ppm"
    image = ppm_fixture_tool.generate_image(4, 3, "gradient")
    ppm_fixture_tool.write_ppm_create_new(path, image)
    return path


class AexNoLoadWorkerTests(unittest.TestCase):
    def test_hello_and_environment_report_no_load_safety(self):
        hello = aex_no_load_worker.handle_message({"id": "h", "type": "hello"})
        self.assertEqual(hello["type"], "hello_ack")
        self.assertFalse(hello["safety_state"]["native_load_enabled"])
        self.assertFalse(hello["safety_state"]["aex_file_opened"])
        self.assertIn("transform_ppm_identity", hello["allowed_messages"])

        environment = aex_no_load_worker.handle_message({"type": "inspect_environment"})
        self.assertEqual(environment["type"], "environment_report")
        self.assertFalse(environment["native_load_enabled"])
        self.assertIn(environment["process_bitness"], (32, 64))

    def test_inspect_and_identity_transform_ppm(self):
        ppm = create_ppm_fixture()
        inspect = aex_no_load_worker.handle_message({"type": "inspect_ppm", "input": str(ppm)})
        self.assertEqual(inspect["type"], "ppm_summary")
        self.assertEqual(inspect["width"], 4)
        self.assertEqual(inspect["height"], 3)
        self.assertEqual(inspect["bytes"], 36)

        output = LAB_ROOT / "target" / "worker-selftest" / f"{time.time_ns()}-{os.getpid()}-identity.ppm"
        transformed = aex_no_load_worker.handle_message(
            {"type": "transform_ppm_identity", "input": str(ppm), "out": str(output)}
        )
        self.assertEqual(transformed["type"], "created_output")
        self.assertEqual(transformed["operation"], "identity")
        self.assertTrue(output.exists())
        self.assertEqual(ppm_fixture_tool.read_ppm(ppm).pixels, ppm_fixture_tool.read_ppm(output).pixels)

    def test_blocks_aex_and_rejects_paths_outside_allowed_roots(self):
        blocked = aex_no_load_worker.handle_message({"type": "load_aex", "path": "x.aex"})
        self.assertEqual(blocked["type"], "error")
        self.assertEqual(blocked["code"], "blocked_action")
        self.assertFalse(blocked["safety_state"]["aex_file_opened"])

        outside = aex_no_load_worker.handle_message(
            {"type": "inspect_ppm", "input": str(LAB_ROOT / "target" / "not-a-fixture.ppm")}
        )
        self.assertEqual(outside["type"], "error")
        self.assertEqual(outside["code"], "ppm_inspect_failed")

        ppm = create_ppm_fixture()
        bad_output = aex_no_load_worker.handle_message(
            {"type": "transform_ppm_identity", "input": str(ppm), "out": str(LAB_ROOT / "target" / "bad.ppm")}
        )
        self.assertEqual(bad_output["type"], "error")
        self.assertEqual(bad_output["code"], "ppm_transform_failed")


if __name__ == "__main__":
    unittest.main()
