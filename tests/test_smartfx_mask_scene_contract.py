import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SmartFxMaskSceneContractTests(unittest.TestCase):
    def test_report_requires_scene_echo_oracle_and_double_run(self):
        schema = json.loads(
            (ROOT / "contracts/aex/smartfx_mask_scene_report.schema.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertFalse(schema["additionalProperties"])
        required = set(schema["required"])
        for field in (
            "scene_case_id",
            "host_scene_id",
            "expected_mask_count",
            "expected_oracle_sha256",
            "run_1",
            "run_2",
            "broker_survived",
        ):
            self.assertIn(field, required)

    def test_worker_uses_host_owned_records_and_fixed_scene_gate(self):
        worker = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(
            encoding="utf-8"
        )
        for marker in (
            "struct HostMask",
            "std::vector<HostMask> g_mask_scene",
            'L"--smart-mask-scene-request"',
            'scene_id == "two_rectangles"',
            "mask_scene_id",
        ):
            self.assertIn(marker, worker)
        self.assertIn('invalid("unknown fixed mask scene")', route)
        self.assertIn('"two_rectangles_second"', route)
        self.assertIn("mask_scene_argb8_hash", route)


if __name__ == "__main__":
    unittest.main()
