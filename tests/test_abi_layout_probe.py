import unittest
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class AbiLayoutProbeTests(unittest.TestCase):
    def test_probe_is_confined_to_instruments(self):
        source = ROOT / "instruments" / "abi-layout-probe" / "main.cpp"
        text = source.read_text(encoding="utf-8")
        self.assertIn('#include "AE_Effect.h"', text)
        self.assertIn("offsetof(PF_InData", text)
        self.assertIn("offsetof(PF_OutData", text)
        for path in (ROOT / "minihost").rglob("*"):
            if path.suffix in {".cpp", ".h", ".hpp"}:
                self.assertNotIn("AE_Effect.h", path.read_text(encoding="utf-8"))

    def test_probe_covers_l2_layout_and_selector_inputs(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        for marker in (
            "pf_in_data_size", "pf_out_data_size", "pf_param_def_size",
            "in.pica_basicP", "out.my_version", "out.num_params",
            "inter.add_param", "param.param_type", "param.name",
            "PF_Cmd_GLOBAL_SETUP", "PF_Cmd_PARAMS_SETUP",
            "PF_Cmd_RENDER", "layer.rowbytes", "layer.data", "in.time_scale", "in.extent_hint",
            "in.output_origin_x", "in.output_origin_y",
            "in.downsample_x", "in.downsample_y", "in.pixel_aspect_ratio",
            "out.width", "out.height", "out.origin",
            "utils.blend", "utils.convolve", "utils.copy", "utils.fill",
            "utils.new_world", "utils.dispose_world", "utils.get_platform_data",
            "utils.get_pixel_data8", "utils.get_pixel_data16",
            "pf_pixel16_size", "pf_pixel_float_size", "layer.world_flags",
            "PF_Cmd_SMART_RENDER_GPU", "PF_Cmd_GPU_DEVICE_SETUP",
            "pf_gpu_device_setup_extra_size", "gpu_setup_input.what_gpu",
            "gpu_setdown_input.gpu_data", "smart_input.device_index",
            "in.sequence_data", "out.frame_data", "PF_Cmd_SEQUENCE_SETUP",
            "PF_Cmd_FRAME_SETDOWN",
            "PF_Cmd_USER_CHANGED_PARAM", "PF_UserChangedParamExtra",
            "PF_ArbitraryDef", "PF_ArbParamsExtra", "PF_Cmd_ARBITRARY_CALLBACK",
            "PF_CustomUIInfo", "custom_ui.events",
            "PF_DoClickEventInfo", "do_click.continue_refcon",
            "PF_KeyDownEvent", "key_down.keycode",
            "PF_Context", "context.plugin_state",
            "AEGP_StreamSuite6::AEGP_SetStreamValue", "aegp_stream.set_value",
            "DRAWBOT_DrawbotSuite1", "DRAWBOT_SupplierSuite1", "DRAWBOT_SurfaceSuite2",
            "DRAWBOT_PathSuite1", "PF_EffectCustomUISuite1", "PFAppSuite4",
            "PF_EffectCustomUIOverlayThemeSuite1", "overlay_theme.stroke_path",
            "AEGP_CommandSuite1", "AEGP_RegisterSuite5", "AEGP_ItemSuite9",
            "AEGP_CompSuite11", "AEGP_LayerSuite5", "AEGP_LayerSuite9",
            "AEGP_CompSuite12", "AEGP_LayerSuite8", "AEGP_EffectSuite4",
            "AEGP_StreamSuite6",
            "AEGP_KeyframeSuite5", "AEGP_StreamValue2",
            "PF_BatchSamplingSuite1", "batch_sampling.get_func16",
        ):
            self.assertIn(marker, text)

    def test_batch_sampling_suite1_has_official_four_slot_abi(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        self.assertIn("sizeof(PF_BatchSamplingSuite1) == 4 * sizeof(void*)", text)
        for name, slot in (("begin_sampling", 0), ("end_sampling", 1),
                           ("get_batch_func", 2), ("get_batch_func16", 3)):
            self.assertIn(f"offsetof(PF_BatchSamplingSuite1, {name})", text)
            self.assertIn(f"{slot} * sizeof(void*)", text)

    def test_effect_param_union_is_observed_at_x64_slot_3(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        self.assertIn('"aegp_effect.get_param_union_by_index"', text)
        self.assertIn("offsetof(AEGP_EffectSuite4, AEGP_GetEffectParamUnionByIndex)", text)
        self.assertIn("3 * sizeof(void*)", text)
        self.assertIn("slot 3 (offset 24)", text)

    def test_layer_source_item_is_observed_at_x64_slot_4(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        self.assertIn('"aegp_layer5.get_source_item"', text)
        self.assertIn("offsetof(AEGP_LayerSuite5, AEGP_GetLayerSourceItem)", text)
        self.assertIn("4 * sizeof(void*)", text)
        self.assertIn("slot 4 (offset 32)", text)

    def test_item_suite9_get_item_type_is_observed_at_x64_slot_5(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        self.assertIn('"aegp_item.get_type"', text)
        self.assertIn("offsetof(AEGP_ItemSuite9, AEGP_GetItemType)", text)
        self.assertIn("5 * sizeof(void*)", text)
        self.assertIn("slot 5 (offset 40)", text)

    def test_effect_suite4_installed_catalog_abi_is_observed(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        self.assertIn("AEGP_InstalledEffectKey_NONE == 0", text)
        self.assertIn("sizeof(AEGP_EffectSuite4) == 22 * sizeof(void*)", text)
        for name, slot in (
            ("AEGP_GetNumInstalledEffects", 11),
            ("AEGP_GetNextInstalledEffect", 12),
            ("AEGP_GetEffectCategory", 15),
        ):
            self.assertIn(f"offsetof(AEGP_EffectSuite4, {name})", text)
            self.assertIn(f"{slot} * sizeof(void*)", text)
        for marker in (
            '"aegp_effect.get_installed_count"',
            '"aegp_effect.get_next_installed"',
            '"aegp_effect.get_category"',
        ):
            self.assertIn(marker, text)

    def test_keyframe_suite5_records_all_mutation_boundaries(self):
        text = (ROOT / "instruments" / "abi-layout-probe" / "main.cpp").read_text(encoding="utf-8")
        self.assertIn("sizeof(AEGP_KeyframeSuite5) == 22 * sizeof(void*)", text)
        for name, slot in (
            ("AEGP_InsertKeyframe", 2), ("AEGP_SetKeyframeValue", 5),
            ("AEGP_GetNewKeyframeSpatialTangents", 8),
            ("AEGP_SetKeyframeSpatialTangents", 9),
            ("AEGP_GetKeyframeTemporalEase", 10),
            ("AEGP_SetKeyframeTemporalEase", 11),
            ("AEGP_SetKeyframeFlag", 13), ("AEGP_SetKeyframeInterpolation", 15),
            ("AEGP_StartAddKeyframes", 16), ("AEGP_SetKeyframeLabelColorIndex", 21),
        ):
            self.assertIn(f"offsetof(AEGP_KeyframeSuite5, {name})", text)
            self.assertIn(f"{slot} * sizeof(void*)", text)
        for marker in (
            '"aegp_keyframe.insert"', '"aegp_keyframe.delete"',
            '"aegp_keyframe.set_value"', '"aegp_keyframe.set_flag"',
            '"aegp_keyframe.get_spatial_tangents"',
            '"aegp_keyframe.set_spatial_tangents"',
            '"aegp_keyframe.get_temporal_ease"',
            '"aegp_keyframe.set_temporal_ease"',
            '"aegp_keyframe.set_interpolation"', '"aegp_keyframe.start_add"',
            '"aegp_keyframe.end_add"', '"aegp_keyframe.set_label"',
        ):
            self.assertIn(marker, text)

    def test_recorded_observation_is_no_load_and_x64(self):
        data = json.loads((ROOT / "analysis" / "AE_ABI_LAYOUT_OBSERVATION_2026-07-13.json").read_text(encoding="utf-8"))
        self.assertEqual(data["pointer_size"], 8)
        self.assertEqual(data["pf_in_data_size"], 408)
        self.assertEqual(data["pf_out_data_size"], 408)
        self.assertEqual(data["pf_interact_callbacks_size"], 176)
        self.assertEqual(data["fields"]["inter.add_param"]["offset"], 16)
        self.assertEqual(data["fields"]["param.u"]["offset"], 56)
        self.assertEqual(data["pf_arbitrary_def_size"], 32)
        self.assertEqual(data["pf_arb_params_extra_size"], 48)
        self.assertEqual(data["fields"]["arbitrary.default"]["offset"], 8)
        self.assertEqual(data["fields"]["arbitrary.value"]["offset"], 16)
        self.assertEqual(data["fields"]["arb_copy.destination"]["offset"], 24)
        self.assertEqual(data["fields"]["arb_dispose.value"]["offset"], 16)
        self.assertEqual(data["fields"]["arb_print_size.output"]["offset"], 24)
        self.assertEqual(data["fields"]["arb_print.value"]["offset"], 24)
        self.assertEqual(data["fields"]["arb_print.buffer"]["offset"], 40)
        self.assertEqual(data["fields"]["arb_flat_size.output"]["offset"], 24)
        self.assertEqual(data["fields"]["arb_flatten.buffer"]["offset"], 32)
        self.assertEqual(data["fields"]["arb_unflatten.output"]["offset"], 32)
        self.assertEqual(data["fields"]["arb_compare.output"]["offset"], 32)
        self.assertEqual(data["fields"]["arb_new.output"]["offset"], 16)
        self.assertEqual(data["fields"]["arb_interp.amount"]["offset"], 32)
        self.assertEqual(data["fields"]["arb_interp.output"]["offset"], 40)
        self.assertEqual(data["pf_custom_ui_info_size"], 44)
        self.assertEqual(data["fields"]["custom_ui.events"]["offset"], 4)
        self.assertEqual(data["fields"]["custom_ui.layer_width"]["offset"], 20)
        self.assertEqual(data["fields"]["custom_ui.preview_alignment"]["offset"], 40)
        self.assertEqual(data["pf_event_extra_size"], 208)
        self.assertEqual(data["fields"]["event.union"]["offset"], 16)
        self.assertEqual(data["fields"]["event.effect_window"]["offset"], 80)
        self.assertEqual(data["fields"]["event.out_flags"]["offset"], 204)
        self.assertEqual(data["pf_context_size"], 72)
        self.assertEqual(data["fields"]["context.magic"]["offset"], 0)
        self.assertEqual(data["fields"]["context.window_type"]["offset"], 4)
        self.assertEqual(data["fields"]["context.plugin_state"]["offset"], 16)
        self.assertEqual(data["fields"]["context.draw_ref"]["offset"], 48)
        self.assertEqual(data["fields"]["adjust_cursor.set_cursor"]["offset"], 12)
        self.assertEqual(data["pf_do_click_event_info_size"], 64)
        self.assertEqual(data["fields"]["do_click.screen_point"]["offset"], 4)
        self.assertEqual(data["fields"]["do_click.continue_refcon"]["offset"], 24)
        self.assertEqual(data["fields"]["do_click.send_drag"]["offset"], 56)
        self.assertEqual(data["fields"]["do_click.last_time"]["offset"], 57)
        self.assertEqual(data["pf_key_down_event_size"], 20)
        self.assertEqual(data["fields"]["key_down.when"]["offset"], 0)
        self.assertEqual(data["fields"]["key_down.screen_point"]["offset"], 4)
        self.assertEqual(data["fields"]["key_down.keycode"]["offset"], 12)
        self.assertEqual(data["fields"]["key_down.modifiers"]["offset"], 16)
        self.assertEqual(data["fields"]["adv_app.info_text3"]["offset"], 64)
        self.assertEqual(data["drawbot_draw_suite_size"], 16)
        self.assertEqual(data["drawbot_supplier_suite_size"], 104)
        self.assertEqual(data["drawbot_surface_suite_size"], 136)
        self.assertEqual(data["drawbot_path_suite_size"], 48)
        self.assertEqual(data["pf_effect_custom_ui_suite1_size"], 8)
        self.assertEqual(data["pf_effect_overlay_theme_suite1_size"], 64)
        self.assertEqual(data["pf_app_suite4_size"], 88)
        self.assertEqual(data["fields"]["drawbot_supplier.release"]["offset"], 96)
        self.assertEqual(data["fields"]["drawbot_surface.fill_path"]["offset"], 24)
        self.assertEqual(data["fields"]["drawbot_path.add_rect"]["offset"], 24)
        self.assertEqual(data["fields"]["effect_custom_ui.get_drawing_ref"]["offset"], 0)
        self.assertEqual(data["fields"]["overlay_theme.foreground"]["offset"], 0)
        self.assertEqual(data["fields"]["overlay_theme.stroke_path"]["offset"], 40)
        self.assertEqual(data["fields"]["app4.invalidate_rect"]["offset"], 64)
        self.assertEqual(data["pf_pixel_size"], 4)
        self.assertEqual(data["pf_pixel16_size"], 8)
        self.assertEqual(data["pf_pixel_float_size"], 16)
        self.assertEqual(data["pf_world_flag_deep"], 1)
        self.assertEqual(data["fields"]["layer.data"]["offset"], 24)
        self.assertEqual(data["fields"]["layer.world_flags"]["offset"], 16)
        self.assertEqual(data["fields"]["utils.blend"]["offset"], 48)
        self.assertEqual(data["fields"]["utils.convolve"]["offset"], 56)
        self.assertEqual(data["fields"]["utils.copy"]["offset"], 64)
        self.assertEqual(data["fields"]["utils.fill"]["offset"], 72)
        self.assertEqual(data["fields"]["utils.new_world"]["offset"], 112)
        self.assertEqual(data["fields"]["utils.dispose_world"]["offset"], 120)
        self.assertEqual(data["fields"]["utils.get_platform_data"]["offset"], 432)
        self.assertEqual(data["fields"]["utils.get_pixel_data8"]["offset"], 528)
        self.assertEqual(data["fields"]["utils.get_pixel_data16"]["offset"], 536)
        self.assertEqual(data["fields"]["pixel.alpha"]["offset"], 0)
        self.assertEqual(data["selectors"]["render"], 11)
        self.assertEqual(data["selectors"]["user_changed_param"], 13)
        self.assertEqual(data["pf_user_changed_param_extra_size"], 4)
        self.assertEqual(data["fields"]["user_changed.param_index"]["offset"], 0)
        self.assertEqual(data["aegp_command_suite1_size"], 64)
        self.assertEqual(data["aegp_register_suite5_size"], 96)
        self.assertEqual(data["fields"]["aegp_command.do_command"]["offset"], 56)
        self.assertEqual(data["fields"]["aegp_register.idle_hook"]["offset"], 64)
        self.assertEqual(data["fields"]["aegp_item.get_active_item"]["offset"], 16)
        self.assertEqual(data["fields"]["aegp_item.get_name"]["offset"], 56)
        self.assertEqual(data["fields"]["aegp_item.get_duration"]["offset"], 112)
        self.assertEqual(data["aegp_comp_suite11_size"], 352)
        self.assertEqual(data["aegp_comp_suite12_size"], 352)
        self.assertEqual(data["aegp_layer_suite5_size"], 368)
        self.assertEqual(data["aegp_layer_suite8_size"], 400)
        self.assertEqual(data["aegp_layer_suite9_size"], 424)
        self.assertEqual(data["aegp_collection_suite2_size"], 48)
        self.assertEqual(data["aegp_collection_item_v2_size"], 56)
        self.assertEqual(data["aegp_effect_suite4_size"], 176)
        self.assertEqual(data["aegp_stream_suite6_size"], 184)
        self.assertEqual(data["aegp_keyframe_suite5_size"], 176)
        self.assertEqual(data["aegp_stream_value2_size"], 40)
        self.assertEqual(data["fields"]["aegp_item.get_current_time"]["offset"], 120)
        self.assertEqual(data["fields"]["aegp_item.set_current_time"]["offset"], 160)
        self.assertEqual(data["fields"]["aegp_layer.get_in_point"]["offset"], 120)
        self.assertEqual(data["fields"]["aegp_layer.get_duration"]["offset"], 128)
        self.assertEqual(data["fields"]["aegp_layer.set_in_point_and_duration"]["offset"], 136)
        self.assertEqual(data["fields"]["aegp_layer.get_flags"]["offset"], 80)
        self.assertEqual(data["fields"]["aegp_layer.set_flag"]["offset"], 88)
        self.assertEqual(data["fields"]["aegp_comp.get_framerate"]["offset"], 88)
        self.assertEqual(data["fields"]["aegp_layer9.get_id"]["offset"], 296)
        self.assertEqual(data["fields"]["aegp_layer9.get_active"]["offset"], 16)
        self.assertEqual(data["fields"]["aegp_layer9.get_index"]["offset"], 24)
        self.assertEqual(data["fields"]["aegp_layer9.get_parent_comp"]["offset"], 48)
        self.assertEqual(data["fields"]["aegp_layer9.get_name"]["offset"], 56)
        self.assertEqual(data["fields"]["aegp_layer9.get_parent"]["offset"], 328)
        self.assertEqual(data["fields"]["aegp_layer9.get_from_id"]["offset"], 360)
        self.assertEqual(data["fields"]["aegp_comp.get_selection"]["offset"], 208)
        self.assertEqual(data["fields"]["aegp_collection.dispose"]["offset"], 8)
        self.assertEqual(data["fields"]["aegp_collection.get_count"]["offset"], 16)
        self.assertEqual(data["fields"]["aegp_collection.get_by_index"]["offset"], 24)
        self.assertEqual(data["fields"]["aegp_collection_item.type"]["offset"], 0)
        self.assertEqual(data["fields"]["aegp_collection_item.union"]["offset"], 8)
        self.assertEqual(data["fields"]["aegp_collection_item.stream_ref"]["offset"], 48)
        self.assertEqual(data["fields"]["aegp_keyframe.get_time"]["offset"], 8)
        self.assertEqual(data["fields"]["aegp_keyframe.get_value"]["offset"], 32)
        self.assertEqual(data["fields"]["aegp_keyframe.get_interpolation"]["offset"], 112)
        self.assertEqual(data["fields"]["aegp_layer8.get_flags"]["offset"], 80)
        self.assertEqual(data["fields"]["aegp_layer8.get_transfer_mode"]["offset"], 176)
        self.assertEqual(data["fields"]["aegp_layer9.get_object_type"]["offset"], 224)
        self.assertEqual(data["fields"]["aegp_layer5.get_in_point"]["offset"], 120)
        self.assertEqual(data["fields"]["aegp_layer5.get_duration"]["offset"], 128)
        self.assertEqual(data["fields"]["aegp_effect.get_by_index"]["offset"], 8)
        self.assertEqual(data["fields"]["aegp_effect.dispose"]["offset"], 64)
        self.assertEqual(data["fields"]["aegp_effect.get_match_name"]["offset"], 112)
        self.assertEqual(data["fields"]["aegp_stream.get_effect_param_count"]["offset"], 32)
        self.assertEqual(data["fields"]["aegp_stream.get_new_effect"]["offset"], 40)
        self.assertEqual(data["fields"]["aegp_stream.get_name"]["offset"], 64)
        self.assertEqual(data["fields"]["aegp_stream.get_new_layer"]["offset"], 24)
        self.assertEqual(data["fields"]["aegp_stream.dispose_value"]["offset"], 112)
        self.assertEqual(data["fields"]["aegp_stream_value.val"]["offset"], 8)
        self.assertEqual(data["selectors"]["update_params_ui"], 14)
        self.assertEqual(data["selectors"]["query_dynamic_flags"], 18)
        self.assertEqual(data["selectors"]["event"], 15)
        self.assertEqual(data["selectors"]["arbitrary_callback"], 22)
        self.assertEqual(data["selectors"]["smart_pre_render"], 23)
        self.assertEqual(data["selectors"]["smart_render"], 24)
        self.assertEqual(data["selectors"]["smart_render_gpu"], 31)
        self.assertEqual(data["selectors"]["gpu_device_setup"], 32)
        self.assertEqual(data["selectors"]["gpu_device_setdown"], 33)
        self.assertEqual(data["pf_smart_render_callbacks_size"], 24)
        self.assertEqual(data["pf_gpu_device_setup_extra_size"], 16)
        self.assertEqual(data["pf_gpu_device_setdown_input_size"], 16)
        self.assertEqual(data["fields"]["smart_input.device_index"]["offset"], 68)
        self.assertEqual(data["fields"]["gpu_setdown_input.device_index"]["offset"], 12)
        self.assertEqual(data["fields"]["in.sequence_data"]["offset"], 320)
        self.assertEqual(data["fields"]["in.extent_hint"]["offset"], 260)
        self.assertEqual(data["fields"]["in.output_origin_x"]["offset"], 276)
        self.assertEqual(data["fields"]["in.output_origin_y"]["offset"], 280)
        self.assertEqual(data["fields"]["in.quality"]["offset"], 192)
        self.assertEqual(data["fields"]["in.local_time_step"]["offset"], 236)
        self.assertEqual(data["fields"]["in.field"]["offset"], 244)
        self.assertEqual(data["fields"]["in.shutter_angle"]["offset"], 248)
        self.assertEqual(data["fields"]["in.pre_effect_source_origin_x"]["offset"], 392)
        self.assertEqual(data["fields"]["in.pre_effect_source_origin_y"]["offset"], 396)
        self.assertEqual(data["fields"]["in.shutter_phase"]["offset"], 400)
        self.assertEqual(data["fields"]["in.downsample_x"]["offset"], 284)
        self.assertEqual(data["fields"]["in.downsample_y"]["offset"], 292)
        self.assertEqual(data["fields"]["in.pixel_aspect_ratio"]["offset"], 300)
        self.assertEqual(data["fields"]["out.frame_data"]["offset"], 72)
        self.assertEqual(data["fields"]["out.width"]["offset"], 80)
        self.assertEqual(data["fields"]["out.height"]["offset"], 84)
        self.assertEqual(data["fields"]["out.origin"]["offset"], 88)
        self.assertEqual(data["selectors"]["sequence_setup"], 5)
        self.assertEqual(data["selectors"]["do_dialog"], 9)
        self.assertEqual(data["out_flags"]["i_do_dialog"], 32)
        self.assertEqual(data["out_flags"]["wide_time_input"], 2)
        self.assertEqual(data["out_flags"]["send_do_dialog"], 128)
        self.assertEqual(data["out_flags"]["i_expand_buffer"], 512)
        self.assertEqual(data["out_flags"]["i_shrink_buffer"], 4096)
        self.assertEqual(data["out_flags"]["i_use_shutter_angle"], 524288)
        self.assertEqual(data["out_flags"]["i_use_audio"], 1048576)
        self.assertEqual(data["out_flags"]["nop_render"], 262144)
        self.assertEqual(data["out_flags"]["i_write_input_buffer"], 2048)
        self.assertEqual(data["out_flags"]["display_error_message"], 256)
        self.assertEqual(data["out_flags2"]["automatic_wide_time_input"], 131072)
        self.assertEqual(data["selectors"]["frame_setdown"], 12)
        self.assertFalse(data["native_aex_loaded"])
        self.assertFalse(data["selector_dispatched"])


if __name__ == "__main__":
    unittest.main()
