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

    def test_request_v4_is_bounded_and_revalidated_by_worker(self):
        request_schema = json.loads(
            (ROOT / "contracts/aex/render_parameter_request.schema.json").read_text(
                encoding="utf-8"
            )
        )
        worker = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(
            encoding="utf-8"
        )
        self.assertEqual(request_schema["schema_version"], 4)
        self.assertEqual(
            request_schema["properties"]["host_context"]["properties"]["mask_scene"]
            ["properties"]["masks"]["maxItems"],
            8,
        )
        for marker in (
            "host mask total vertex count exceeds 128",
            "host mask transport exceeds 8192 bytes",
            '"--smart-mask-context-request"',
            "bezier_mask_argb8_hash",
            "host_context_tangent_vertex_count",
        ):
            self.assertIn(marker, route)
        for marker in (
            "parse_mask_context_payload",
            "masks.size() > 8",
            "total_vertices > 128",
            "encoded.size() > 8192",
            'encoded.compare(0, 3, L"v2|")',
            "mask.open = item[0] == L'1'",
            "mask_tangent_vertex_count",
            'g_mask_scene_id = "request_v4"',
        ):
            self.assertIn(marker, worker)

    def test_mask_handle_lifetimes_are_single_owner_and_broker_verified(self):
        worker = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(
            encoding="utf-8"
        )
        for marker in (
            "bool mask_live{}",
            "bool stream_live{}",
            "bool value_live{}",
            "!record->mask_live",
            "record->stream_live || !stream",
            "record->value_live || !value",
            "!record->stream_live || record->value_live",
            "!stream_record->value_live",
            "mask_lifetimes_balanced()",
        ):
            self.assertIn(marker, worker)
        for marker in (
            'report.get("mask_lifetimes_balanced")',
            '"mask_handles_acquired"',
            '"stream_handles_disposed"',
            '"stream_values_disposed"',
            "expected_mask_lifetime_count",
        ):
            self.assertIn(marker, route)

        report_schema = json.loads(
            (ROOT / "contracts/aex/parameterized_smartfx_render_report.schema.json")
            .read_text(encoding="utf-8")
        )
        run_required = set(report_schema["$defs"]["run"]["required"])
        for field in (
            "mask_lifetimes_balanced",
            "mask_handles_acquired",
            "stream_handles_disposed",
            "stream_values_disposed",
            "suite_leases_balanced",
            "suite_acquires",
            "suite_releases",
            "live_suite_lease_count",
            "live_suite_reference_count",
        ):
            self.assertIn(field, run_required)


if __name__ == "__main__":
    unittest.main()
