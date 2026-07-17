import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = ROOT / "analysis" / "SDK_NOISE_DEEP_RENDER_RESULT_2026-07-15.json"


class SdkNoiseDeepRenderResultTests(unittest.TestCase):
    def test_float_slider_uses_host_slot_and_sdk_disk_id(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        parameter = result["inspection"]["parameter"]
        self.assertEqual(result["inspection"]["reported_num_params"], 2)
        self.assertEqual([parameter["slot"], parameter["disk_id"], parameter["type"]], [1, 1, 10])
        self.assertEqual([parameter["valid_min"], parameter["valid_max"], parameter["default"]], [0.0, 1000.0, 10.0])

    def test_classic_and_all_smart_depths_render_safely(self):
        result = json.loads(RESULT.read_text(encoding="utf-8"))
        classic = result["classic_argb8"]
        self.assertEqual(classic["status"], "render_completed")
        self.assertNotEqual(classic["input_sha256"], classic["output_sha256"])
        self.assertTrue(classic["guard_bytes_intact"])
        self.assertEqual([entry["pixel_format"] for entry in result["smart"]], ["argb8", "argb16", "argb32f"])
        for entry in result["smart"]:
            self.assertEqual([entry["pre_render_error"], entry["render_error"]], [0, 0])
            self.assertTrue(entry["guard_bytes_intact"])
            self.assertTrue(entry["suite_leases_balanced"])

    def test_fixture_handle_ownership_anomaly_remains_observable(self):
        observation = json.loads(RESULT.read_text(encoding="utf-8"))["ownership_observation"]
        self.assertFalse(observation["handle_lifetimes_balanced"])
        self.assertEqual([observation["live_handle_count"], observation["live_handle_bytes"]], [1, 8])


if __name__ == "__main__":
    unittest.main()
