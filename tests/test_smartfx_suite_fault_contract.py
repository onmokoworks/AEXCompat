import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SmartFxSuiteFaultContractTests(unittest.TestCase):
    def test_report_requires_double_run_and_broker_survival(self):
        schema = json.loads(
            (ROOT / "contracts/aex/smartfx_suite_fault_report.schema.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertFalse(schema["additionalProperties"])
        required = set(schema["required"])
        for field in (
            "fault_id",
            "expected_outcome",
            "run_1",
            "run_2",
            "broker_survived",
            "passed",
        ):
            self.assertIn(field, required)
        self.assertTrue(schema["properties"]["broker_survived"]["const"])

    def test_fault_modes_are_fixed_and_profile_gated(self):
        worker = (ROOT / "minihost/src/l2_main.cpp").read_text(encoding="utf-8")
        route = (ROOT / "broker/crates/broker/src/render_request.rs").read_text(
            encoding="utf-8"
        )
        main = (ROOT / "broker/crates/broker/src/main.rs").read_text(encoding="utf-8")
        for marker in (
            'L"--smart-mask-count-error-request"',
            'L"--smart-mask-count-crash-request"',
            'L"--smart-mask-double-dispose-request"',
            'L"--smart-stream-live-value-dispose-request"',
            'L"--smart-stream-metadata-ownership-request"',
            'L"--smart-keyframe-ownership-request"',
            'L"--smart-dynamic-stream-tree-request"',
            'L"--smart-suite-release-without-acquire-request"',
            'L"--smart-handle-resize-while-locked-request"',
            "MaskFault::CountError",
            "MaskFault::CountCrash",
            "RaiseException(EXCEPTION_ACCESS_VIOLATION",
            "SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX)",
            "live_suite_reference_count()",
        ):
            self.assertIn(marker, worker)
        self.assertIn('"mask_count_error"', route)
        self.assertIn('"mask_count_crash"', route)
        self.assertIn('"mask_double_dispose"', route)
        self.assertIn('"stream_dispose_with_live_value"', route)
        self.assertIn('"stream_metadata_ownership"', route)
        self.assertIn('report.get("stream_metadata_fault_observed")', route)
        self.assertIn('"keyframe_ownership"', route)
        self.assertIn('report.get("keyframe_fault_observed")', route)
        self.assertIn('"dynamic_stream_tree"', route)
        self.assertIn('report.get("dynamic_stream_fault_observed")', route)
        self.assertIn('"callback_error_rejected"', route)
        self.assertIn('"suite_release_without_acquire"', route)
        self.assertIn('report.get("suite_fault_observed")', route)
        self.assertIn('"handle_resize_while_locked"', route)
        self.assertIn('"world_double_dispose"', route)
        self.assertIn('"world_allocation_limit"', route)
        self.assertIn('"pixel_format_registry"', route)
        self.assertIn('report.get("pixel_format_fault_observed")', route)
        self.assertIn('"outline_mutation"', route)
        self.assertIn('report.get("outline_fault_observed")', route)
        self.assertIn('"mask_attribute_ownership"', route)
        self.assertIn('report.get("mask_attribute_fault_observed")', route)
        self.assertIn('report.get("world_fault_observed")', route)
        self.assertIn('report.get("handle_fault_observed")', route)
        self.assertIn('invalid("unknown fixed suite fault")', route)
        self.assertIn('spec.request_mode == "--smart-mask-request"', route)
        self.assertIn('args[1] == "smart-suite-fault"', main)


if __name__ == "__main__":
    unittest.main()
