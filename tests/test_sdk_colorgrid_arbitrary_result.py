import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SdkColorGridArbitraryResultTests(unittest.TestCase):
    def test_colorgrid_renders_with_balanced_arbitrary_ownership(self):
        result = json.loads((ROOT / "analysis" / "SDK_COLORGRID_ARBITRARY_RESULT_2026-07-15.json").read_text(encoding="utf-8"))
        self.assertEqual(result["status"], "render_completed")
        self.assertEqual(result["arbitrary_parameter"]["kind"], "arbitrary_data")
        self.assertEqual(result["ownership"]["copy_calls"], 1)
        self.assertEqual(result["ownership"]["dispose_calls"], 5)
        self.assertTrue(result["ownership"]["balanced"])
        self.assertTrue(result["render"]["guard_bytes_intact"])
        self.assertEqual(len(result["render"]["output_sha256"]), 64)
        self.assertEqual(result["debug_observation"]["print_callback"], "completed")
        self.assertEqual(result["debug_observation"]["cells_observed"], 9)
        self.assertTrue(result["serialization_roundtrip"]["reflattened_bytes_equal"])
        self.assertTrue(result["serialization_roundtrip"]["restored_value_used_for_render"])
        self.assertEqual(result["serialization_roundtrip"]["roundtrip_failures"], 0)
        self.assertEqual(result["serialization_roundtrip"]["compare_disagreements"], 0)
        self.assertIn("byte identity", result["serialization_roundtrip"]["compare_caveat"])
        self.assertEqual(result["temporal_interpolation"]["normalized_amount"], 0.5)
        self.assertEqual(result["temporal_interpolation"]["interpolation_failures"], 0)
        self.assertEqual(result["temporal_interpolation"]["output_bytes"], 64)
        self.assertEqual(result["custom_ui_registration"]["events"], 4)
        self.assertEqual(result["custom_ui_registration"]["parameter_control_size"], [203, 203])
        self.assertEqual(result["custom_ui_registration"]["invalid_registrations"], 0)
        self.assertEqual(result["custom_ui_adjust_cursor"]["status"], "event_completed")
        self.assertEqual(result["custom_ui_adjust_cursor"]["cursor"], 13)
        self.assertTrue(result["custom_ui_adjust_cursor"]["suite_leases_balanced"])
        draw = result["custom_ui_draw"]
        self.assertEqual(draw["status"], "event_completed")
        self.assertTrue(draw["event_handled"])
        self.assertEqual(draw["fill_path_calls"], 9)
        self.assertEqual(draw["stroke_path_calls"], 9)
        self.assertEqual(draw["fill_color_count"], 9)
        self.assertEqual(draw["first_fill_color_rgba"], [1.0, 0.5, 0.0, 1.0])
        self.assertEqual(draw["objects_created"], draw["objects_released"])
        self.assertEqual(draw["invalid_operations"], 0)
        self.assertTrue(draw["suite_leases_balanced"])
        click = result["custom_ui_click"]
        self.assertEqual(click["status"], "event_completed")
        self.assertEqual(click["event_out_flags"], 9)
        self.assertTrue(click["changed_value"])
        self.assertEqual(click["picker_color_rgba"], [0.125, 0.25, 0.75, 1.0])
        self.assertEqual(click["color_picker_calls"], 1)
        self.assertEqual(click["invalidate_rect_calls"], 1)
        self.assertEqual(click["invalidated_rect"], [0, 0, 203, 203])
        self.assertTrue(click["handle_lifetimes_balanced"])
        self.assertTrue(click["suite_leases_balanced"])
        click_render = result["custom_ui_click_render"]
        self.assertEqual(click_render["status"], "render_completed")
        self.assertTrue(click_render["changed_value"])
        self.assertEqual(
            click_render["ui_lifecycle"],
            ["new_context", "activate", "deactivate", "close_context"],
        )
        self.assertEqual(click_render["ui_lifecycle_errors"], [0, 0, 0, 0])
        self.assertTrue(click_render["ui_context_closed"])
        self.assertTrue(click_render["output_changed"])
        self.assertNotEqual(
            click_render["normal_internal_output_sha256"],
            click_render["clicked_internal_output_sha256"],
        )
        self.assertEqual(click_render["arbitrary_roundtrip_failures"], 0)
        self.assertEqual(click_render["invalid_arbitrary_operations"], 0)
        self.assertTrue(click_render["guard_bytes_intact"])
        self.assertTrue(click_render["handle_lifetimes_balanced"])
        self.assertTrue(click_render["suite_leases_balanced"])


if __name__ == "__main__":
    unittest.main()
