import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class SdkCcuCustomUiDragResultTests(unittest.TestCase):
    def test_ccu_drag_preserves_continuation_and_terminates(self):
        result = json.loads(
            (ROOT / "analysis" / "SDK_CCU_CUSTOM_UI_DRAG_RESULT_2026-07-15.json")
            .read_text(encoding="utf-8")
        )
        self.assertEqual(result["status"], "event_completed")
        self.assertEqual(result["event_sequence"][0], "do_click")
        self.assertEqual(result["event_sequence"].count("drag"), 4)
        self.assertTrue(result["drag_requested"])
        self.assertTrue(result["drag_terminated"])
        self.assertTrue(result["final_last_time"])
        self.assertTrue(result["continue_refcon_preserved"])
        self.assertGreater(result["coordinate_transform_calls"], 0)
        self.assertTrue(result["changed_value"])
        self.assertTrue(result["handle_lifetimes_balanced"])
        self.assertTrue(result["suite_leases_balanced"])
        self.assertLessEqual(result["drag_calls"], result["bounds"]["maximum_steps"])
        draw = result["custom_ui_draw"]
        self.assertEqual(draw["status"], "event_completed")
        self.assertEqual(draw["event_target"], "layer")
        self.assertEqual(draw["paint_rect_calls"], 4)
        self.assertEqual(draw["overlay_stroke_path_calls"], 1)
        self.assertEqual(draw["path_objects_created"], draw["path_objects_released"])
        self.assertEqual(draw["invalid_drawbot_operations"], 0)
        self.assertTrue(draw["suite_leases_balanced"])


if __name__ == "__main__":
    unittest.main()
