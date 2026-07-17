import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "minihost" / "src" / "l2_main.cpp"


class MinihostL2SourceTests(unittest.TestCase):
    def test_batch_sampling_suite_is_typed_and_fail_closed(self):
        text = (ROOT / "minihost" / "src" / "l2_main.cpp").read_text(encoding="utf-8")
        for marker in (
            "struct PfBatchSamplingSuite1", "4 * sizeof(void*)",
            'strcmp(name, "PF Batch Sampling Suite") == 0',
            "&begin_sampling8, &end_sampling8", "unsupported_batch_sample_func",
            "*batch = nullptr", "verify_pf_batch_sampling_suite",
            "--self-test-pf-batch-sampling-suite",
            '"opaque_callable_exposed\\\":false}',
        ):
            self.assertIn(marker, text)

    def test_layout_matches_observed_contract(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("kInSize = 408", "kOutSize = 408", "kParamSize = 176",
                       "kInAddParam = 16", "kInGlobalData = 312", "kOutGlobalData = 40"):
            self.assertIn(marker, text)

    def test_host_advertises_the_current_sdk_spec_version(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("kHostSpecMajor = 13", text)
        self.assertIn("kHostSpecMinor = 28", text)
        self.assertIn("write<int16_t>(input, kInVersion, kHostSpecMajor)", text)
        self.assertIn("write<int16_t>(input, kInVersion + sizeof(int16_t), kHostSpecMinor)", text)
        self.assertNotIn("write<uint32_t>(input, kInVersion, 0)", text)

    def test_l2_is_non_rendering_and_bounded(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("kMaxParams = 1024", text)
        self.assertIn('render_performed\\\":false', text)
        self.assertNotIn("PF_Cmd_RENDER", text)
        self.assertNotIn("AE_Effect.h", text)

    def test_render_code_is_compile_time_separated(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("#ifdef AEXCOMPAT_RENDER_WORKER", text)
        self.assertIn('L"--render"', text)
        self.assertIn('L"--l2"', text)
        self.assertIn("guard_bytes_intact", text)

    def test_l2_provides_bounded_movable_handle_callbacks(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("kUtilsSize = 552", "kUtilsNewHandle = 160", "new_handle(uint64_t size)",
                       "64 * 1024 * 1024", "g_handles.count", "dispose_handle"):
            self.assertIn(marker, text)
        for marker in (
            "uint32_t lock_count{}",
            "handle_lifetimes_balanced()",
            "record->lock_count != 0",
            "verify_handle_resize_while_locked_rejected()",
            "verify_world_double_dispose_rejected()",
            "verify_world_allocation_limit_rejected()",
            "verify_pixel_format_registry_rejection()",
            "verify_outline_mutation_rejection()",
            "verify_mask_attribute_and_ownership_rejection()",
            "verify_stream_metadata_and_ownership_rejection()",
            "static_assert(sizeof(StreamSuite) == 23 * sizeof(void*))",
            "valid_outline_snapshot(const OutlineData& outline)",
            "decltype(&set_stream_value) set_stream_value",
            "first.value != second.value",
            "set_stream_value(1, stream, &first) == 0",
            "std::numeric_limits<double>::quiet_NaN()",
            "host_mask->keyframes.push_back(snapshot_keyframe",
            "dispose_stream_value(&second) != 0",
            "static_assert(sizeof(KeyframeSuite) == 22 * sizeof(void*))",
            "verify_keyframe_ownership_rejection()",
            "static_assert(sizeof(DynamicStreamSuite) == 26 * sizeof(void*))",
            "verify_dynamic_stream_tree_rejection()",
            "static_assert(sizeof(AegpMemorySuite) == 8 * sizeof(void*))",
            "verify_aegp_memory_and_strings_rejection()",
            "kMaxHandleCount = 1024",
            "kMaxHandleBytes = 64 * 1024 * 1024",
        ):
            self.assertIn(marker, text)

    def test_l2_pica_is_default_deny_except_bounded_parameter_suites(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn('std::strcmp(name, "PF Handle Suite")', text)
        self.assertIn('std::strcmp(name, "PF PointParamSuite")', text)
        self.assertIn('std::strcmp(name, "PF AngleParamSuite")', text)
        self.assertIn("floating_point_from_point", text)
        self.assertIn("floating_point_from_angle", text)
        self.assertIn("version == 2", text)
        self.assertIn("*suite = nullptr", text)
        self.assertIn("return 1", text)

    def test_aegp_keyframe_suite5_wires_all_mutations(self):
        text = SOURCE.read_text(encoding="utf-8")
        for slot, function in (
            (0, "aegp_get_stream_num_keyframes"), (1, "aegp_get_keyframe_time"),
            (2, "insert_keyframe"), (3, "delete_keyframe"),
            (4, "aegp_get_new_keyframe_value"),
            (5, "set_keyframe_value"), (6, "get_stream_value_dimensionality"),
            (7, "get_stream_temporal_dimensionality"),
            (8, "get_new_keyframe_spatial_tangents"),
            (9, "set_keyframe_spatial_tangents"),
            (10, "get_keyframe_temporal_ease"),
            (11, "set_keyframe_temporal_ease"),
            (12, "get_keyframe_flags"), (13, "set_keyframe_flag"),
            (14, "aegp_get_keyframe_interpolation"),
            (15, "set_keyframe_interpolation"), (16, "start_add_keyframes"),
            (17, "add_keyframes"), (18, "set_add_keyframe"),
            (19, "end_add_keyframes"), (20, "get_keyframe_label"),
            (21, "set_keyframe_label"),
        ):
            self.assertIn(
                f"g_aegp_keyframe_suite5[{slot}] = reinterpret_cast<void*>(&{function})",
                text,
            )
        for marker in (
            'L"--self-test-aegp-keyframe-mutations"',
            "verify_aegp_keyframe_suite5_mutations()",
            "verify_keyframe_ownership_rejection()",
            "g_stream_refs.empty()", "g_stream_values.empty()",
            "g_add_keyframe_transactions.empty()", "mask_lifetimes_balanced()",
            "kHostTemporalDimensions", "register_keyframe_stream_value",
            "unchanged_in.stream == unchanged_before.stream",
            "unchanged_ease.speed == 41.0", "stale_spatial = spatial_in",
            "value->value == &found->second.outline->outline",
        ):
            self.assertIn(marker, text)

    def test_aegp_item_suite9_wires_get_item_type_at_slot_5(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "offsetof(AegpItemSuite, get_item_type) == 40",
            "g_aegp_item_suite.get_item_type = &aegp_get_item_type",
            "g_aegp_item_type_calls == item_type_calls_before + 1",
            "item_type == 0x1234",
            "item_type_calls",
        ):
            self.assertIn(marker, text)

    def test_l2_decodes_supported_parameter_descriptors(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("record.type == 1", "record.type == 2", "record.type == 3", "record.type == 6",
                       "record.type == 7", "record.type == 4", "record.type == 10",
                       "record.type == 18", "valid_min", "default_value", "current_value",
                       "default_components", "current_components",
                       "current_default_mismatch", "choices", "bytes_written_per_row",
                       "undefined_tail_bytes_per_row"):
            self.assertIn(marker, text)

    def test_fixed_slider_uses_sdk_16_16_layout_and_float_transport(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "record.type == 2",
            "read<int32_t>(bytes, u + 68) / 65536.0",
            "read<int32_t>(bytes, u + 84) / 65536.0",
            "read<int16_t>(bytes, u + 88)",
            "descriptor.type == 2 || descriptor.type == 10",
            "assignment.value * 65536.0",
            "fixed < INT32_MIN || fixed > INT32_MAX",
        ):
            self.assertIn(marker, text)

    def test_legacy_copy_and_iterate_callbacks_are_bounded_argb8(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "kUtilsCopy = 64",
            "kUtilsIterate = 88",
            "kUtilsAnsiPow = 296",
            "kUtilsAnsiStrcpy = 336",
            "copy_world8",
            "iterate_world8",
            "bounded_argb8_world",
            "rowbytes >= width * pixel_bytes",
            "static_cast<int64_t>(width) * height <= 16'777'216",
            "(flags & 1) == 0",
            "progress_callback(effect_ref, current, progress_final)",
            "completed_rows < rows && abort_callback",
            "if (error != 0) return error",
            "write(utils, kUtilsCopy, &copy_world8)",
            "write(utils, kUtilsIterate, &iterate_world8)",
            "write(utils, kUtilsAnsiPow, &ansi_pow)",
            "strnlen_s(source, 4096)",
            "write(utils, kUtilsAnsiStrcpy, &ansi_strcpy)",
        ):
            self.assertIn(marker, text)

    def test_render_exposes_bounded_suite_adapters_for_path_effects(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "struct PfMaskSuite1",
            "struct Iterate8Suite2",
            "struct WorldTransformSuite1",
            'std::strcmp(name, "AEGP Mask Suite") == 0',
            'std::strcmp(name, "PF Iterate8 Suite") == 0',
            'std::strcmp(name, "PF World Transform Suite") == 0',
            "g_iterate8_suite2.iterate = reinterpret_cast<void*>(&iterate_world8)",
            "g_world_transform_suite1.composite_rect = &composite_rect8",
            "g_world_transform_suite1.copy = &copy_world8",
            'std::strcmp(name, "PF ANSI Suite") == 0 && version == 1',
            "g_ansi_suite1[16] = reinterpret_cast<void*>(&ansi_strcpy)",
            "g_mask_model_enabled = true",
        ):
            self.assertIn(marker, text)

    def test_smart_render_exposes_bounded_deep_iterate_suites(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'std::strcmp(name, "PF iterate16 Suite") == 0',
            'std::strcmp(name, "PF iterateFloat Suite") == 0',
            "progress_final, 8, source_world",
            "progress_final, 16, source_world",
            "rowbytes >= width * pixel_bytes",
            "g_checkout_layer_definitions.emplace(static_cast<int32_t>(slot), definitions[slot])",
            "index < 0 ? static_cast<int32_t>(g_params.size() + 1) : index",
        ):
            self.assertIn(marker, text)

    def test_classic_render_honors_frame_setup_resize_and_fill(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "constexpr std::size_t kOutWidth = 80",
            "constexpr std::size_t kOutOrigin = 88",
            "struct LegacyRect { int32_t left, top, right, bottom; }",
            'std::strcmp(name, "PF Fill Matte Suite") == 0',
            "fill_world_typed(16, color, area, world)",
            "const int32_t requested_width = read<int32_t>(command_output, kOutWidth)",
            "static_cast<int64_t>(requested_width) * requested_height > 16'777'216",
            "write<int32_t>(input, 284, g_downsample_x.numerator)",
            'encoded.compare(0, 11, L"spatial:v1|")',
            'encoded.compare(0, 11, L"spatial:v2|")',
            'encoded.compare(0, 11, L"spatial:v3|")',
        ):
            self.assertIn(marker, text)

    def test_classic_world_utilities_are_bounded_and_ownership_tracked(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "constexpr std::size_t kUtilsBlend = 48",
            "constexpr std::size_t kUtilsConvolve = 56",
            "constexpr std::size_t kUtilsNewWorld = 112",
            "g_world_transform_suite1.convolve = &convolve_world",
            "kernel_size > 15",
            "constexpr uint32_t kReplicateBorders = 1u << 6",
            "snapshot.resize(static_cast<std::size_t>(source_bytes))",
            "ratio < 0 || ratio > 65536",
            "write(utils, kUtilsNewWorld, &legacy_new_world)",
            "world_lifetimes_balanced()",
        ):
            self.assertIn(marker, text)

    def test_component_parameter_transport_is_slot_bound_and_versioned(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"v4|"', "RequestedKind::Angle", "RequestedKind::Point",
                       "RequestedKind::Point3D", 'kind_text == L"angle"',
                       'kind_text == L"point"', 'kind_text == L"point3d"'):
            self.assertIn(marker, text)

    def test_secondary_layer_transport_is_multi_slot_bound(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--render-image-layer"', 'L"--smart-image-layer"',
                       "g_checkout_layer_definitions", "g_smart_hosted_layers",
                       "existing.slot != layer.slot", "g_params[layer.slot - 1].type != 0",
                       "(image_argc - 13) / 4 <= 64",
                       "(smart_image_argc - 13) / 4 <= 64",
                       'L"v1|%d|%d|%u%n"', "same_rational_time",
                       "g_timed_classic_layers", "timed_slot"):
            self.assertIn(marker, text)

    def test_large_parameter_sets_use_bounded_dynamic_storage(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("kMaxParams = 1024", text)
        self.assertIn("lifecycle_definitions(g_params.size() + 1)", text)
        self.assertIn("std::vector<void*> lifecycle_params", text)
        self.assertNotIn("lifecycle_definitions{}", text)

    def test_smartfx_expanded_extent_is_bounded_and_origin_aware(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("rect_width <= 4096", "rect_height <= 4096",
                       "rect_width * rect_height <= 16'777'216",
                       "write<int32_t>(input, 276", "write<int32_t>(input, 280",
                       "result.output_width", "result.output_height"):
            self.assertIn(marker, text)

    def test_classic_render_defaults_extent_hint_to_the_full_input_world(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("const int32_t full_extent[4] = {0, 0, width, height}", text)
        self.assertIn("std::memcpy(input.data() + 260, full_extent", text)
        self.assertLess(
            text.index("std::memcpy(input.data() + 260, full_extent"),
            text.index("if (partial_extent_hint)"),
        )

    def test_interactive_image_time_context_is_validated_and_forwarded(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("external_current_time", "external_time_step",
                       "external_total_time", "external_time_scale",
                       "external_total_time < external_current_time",
                       "write<int32_t>(input, 224, external_current_time)"):
            self.assertIn(marker, text)

    def test_effect_render_defaults_to_high_quality_and_complete_frame_context(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "constexpr std::size_t kInQuality = 192",
            "constexpr std::size_t kInLocalTimeStep = 236",
            "write<int32_t>(input, kInQuality, g_render_quality)",
            "write<int32_t>(input, 236, external_time_step)",
            "write<int32_t>(input, 236, time_step)",
            "g_full_resolution_width > 0 ? g_full_resolution_width : width",
            "parse_render_environment_payload",
            'L"render:v1|"',
        ):
            self.assertIn(marker, text)

    def test_frame_setup_origin_uses_two_signed_32_bit_components(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("read<int32_t>(command_output, kOutOrigin)", text)
        self.assertIn("read<int32_t>(command_output, kOutOrigin + 4)", text)
        self.assertNotIn("read<int16_t>(command_output, kOutOrigin)", text)

    def test_params_setup_count_is_forwarded_and_sanity_checked(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "expected_num_params = static_cast<int32_t>(g_params.size() + 1)",
            "parameter_count_contract_valid",
            "read<int32_t>(output, kOutNumParams) == expected_num_params",
            "write<int32_t>(input, kInNumParams, expected_num_params)",
            "in_data_num_params",
        ):
            self.assertIn(marker, text)

    def test_interactive_render_uses_balanced_sequence_and_frame_lifecycle(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "RenderLifecycle begin_render_lifecycle",
            "invoke_sequence_selector(\n      entry, kSequenceSetup",
            "write<void*>(input, kInSequenceData, read<void*>(output, kOutSequenceData))",
            "entry(kFrameSetup",
            "write<void*>(input, kInFrameData, read<void*>(output, kOutFrameData))",
            "g_classic_render_selector_dispatched = true;\n      error = entry(kRender",
            "entry(kFrameSetdown",
            "invoke_sequence_selector(\n        entry, kSequenceSetdown",
            "end_render_lifecycle(entry, input, command_output, params.data()",
        ):
            self.assertIn(marker, text)
        self.assertLess(text.index("entry(kFrameSetdown"),
                        text.index("entry, kSequenceSetdown"))

    def test_interactive_render_supports_bounded_deep_pixel_worlds(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"--render-image16"',
            'L"--render-image32"',
            'L"--smart-image16"',
            'L"--smart-image32"',
            "rgba8_to_argb",
            "argb_to_rgba8",
            "rgba[3] / 255.0f",
            "* 32768u + 127u",
            "std::clamp(value, 0.0f, 1.0f)",
            "pixel_bytes != 4 && pixel_bytes != 8 && pixel_bytes != 16",
            "write<int32_t>(world, 16, pixel_bytes == 4 ? 0 : 1)",
        ):
            self.assertIn(marker, text)

    def test_supervised_parameter_dispatch_is_slot_bound_typed_and_explicit(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--user-changed"', "kUserChangedParam = 13",
                       "g_params[offset].flags & (1u << 6)",
                       "parse_parameter_payload(argv[5], g_user_changed_parameters)",
                       "apply_requested_assignments(lifecycle_definitions, g_user_changed_parameters)",
                       "write<int32_t>(changed_extra, 0, g_user_changed_param_slot)",
                       "g_user_changed_param_error = entry(kUserChangedParam"):
            self.assertIn(marker, text)
        self.assertNotIn("g_params[offset].type != 15", text)

    def test_ui_flags_use_the_observed_paramdef_offset(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("kParamUiFlags = 4", text)
        self.assertIn('read<uint32_t>(param.raw, kParamUiFlags)', text)
        self.assertNotIn('read<uint32_t>(param.raw, 0)', text)

    def test_aegp_initialization_has_a_distinct_default_deny_abi(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-init"', 'GetProcAddress(module, "EntryPointFunc")',
                       'std::strcmp(name, "AEGP Command Suite") == 0 && version == 1',
                       'std::strcmp(name, "AEGP Register Suite") == 0 && version == 6',
                       '"hooks_invoked\\\":"',
                       'live_suite_references == 0 || isolated_item_cache',
                       'live_suite_summary == "AEGP Item Suite@14=1"'):
            self.assertIn(marker, text)
        self.assertNotIn('reinterpret_cast<EffectEntry>(GetProcAddress(module, "EntryPointFunc"))', text)

    def test_parameter_inspection_can_isolate_optional_about_crashes(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn('L"--l2-no-about"', text)
        self.assertIn('about_selector_dispatched', text)
        self.assertIn('about_error = g_skip_about ? 0', text)

    def test_parameter_inspection_does_not_require_render_lifecycle(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"--l2-params-only"',
            '"parameters_inspected"',
            "const std::array<int32_t, 5> skipped_lifecycle{-1, -1, -1, -1, -1}",
            "params_setdown_error == 0 ? 0 : 20",
        ):
            self.assertIn(marker, text)
        params_only = text.index("if (params_only_mode)")
        lifecycle = text.index("stage:sequence_setup_begin", params_only)
        self.assertLess(params_only, lifecycle)

    def test_aegp_update_menu_event_owns_and_invokes_registered_hooks(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-update-menu"', "AegpUpdateMenuRegistration",
                       "g_aegp_update_menu_registrations.size() >= 64",
                       "registration.hook(", "global_refcon, registration.refcon, 0)",
                       '"event_requested\\\":\\\""', '"update_menu"'):
            self.assertIn(marker, text)

    def test_aegp_idle_event_is_single_tick_bounded_and_owned(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-idle"', "AegpIdleRegistration",
                       "g_aegp_idle_registrations.size() >= 64",
                       "requested_sleep < 0 || requested_sleep > 3600",
                       "registration.refcon, &requested_sleep"):
            self.assertIn(marker, text)

    def test_aegp_death_hooks_are_owned_before_module_unload(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("AegpDeathRegistration", "g_aegp_death_registrations.size() >= 64",
                       "registration.hook(global_refcon, registration.refcon)",
                       '"death_hooks_invoked\\\":"', '"death_error\\\":"'):
            self.assertIn(marker, text)
        death_call = text.index("registration.hook(global_refcon, registration.refcon)")
        self.assertLess(death_call, text.index("FreeLibrary(module);", death_call))

    def test_aegp_command_roundtrip_tracks_filter_priority_and_handled(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-command-roundtrip"', "AegpCommandRegistration",
                       "priority != 1 && priority != 2", "registration.command != 0",
                       "registration.command != command", "already_handled, &handled",
                       "handled > 1", "command_handled_count"):
            self.assertIn(marker, text)

    def test_active_aegp_idle_roundtrip_hosts_an_observed_empty_item_scene(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-active-idle-roundtrip"', '"AEGP Item Suite"',
                       "version == 14", "static_assert(sizeof(AegpItemSuite) == 208)",
                       "*item = (g_aegp_update_menu_mode || g_aegp_command_roundtrip_mode ||",
                       "g_aegp_comp_idle_roundtrip_mode)",
                       "? &g_aegp_comp_item : nullptr",
                       "Always toggle OFF before unload"):
            self.assertIn(marker, text)

    def test_comp_aegp_idle_roundtrip_hosts_typed_item_comp_and_layer_handles(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-comp-idle-roundtrip"', '"AEGP Comp Suite"',
                       '"AEGP Layer Suite"', "version == 25", "version == 11",
                       "version == 15", "sizeof(g_aegp_comp_suite11) == 352",
                       "sizeof(g_aegp_layer_suite5) == 368",
                       "sizeof(g_aegp_layer_suite9) == 424",
                       "sizeof(g_aegp_comp_suite12) == 352",
                       "sizeof(g_aegp_layer_suite8) == 400",
                       "sizeof(g_aegp_effect_suite4) == 176",
                       "sizeof(g_aegp_stream_suite6) == 184",
                       "item != &g_aegp_comp_item", "comp != &g_aegp_comp",
                       "g_aegp_layers.size()", "aegp_layer_index(layer)",
                       "aegp_get_active_layer", "aegp_get_layer_parent_comp",
                       "aegp_get_layer_parent", "aegp_get_layer_from_id",
                       "g_aegp_layer_suite5[4] =",
                       "g_aegp_layer_suite8[4] =",
                       "g_aegp_layer_suite9[4] =",
                       "aegp_get_layer_source_item",
                       '"layer_source_item_calls\\\":"',
                       "isolated_aegp_read_cache_is_bounded",
                       "count > 16", "total <= 32", '"comp_from_item_calls\\\":"',
                       '"layer_by_index_calls\\\":"', "g_aegp_effect_live",
                       "g_aegp_effect_acquires == g_aegp_effect_disposes",
                       '"effect_lifetimes_balanced\\\":"',
                       "sizeof(AegpStreamValue) == 40",
                       "g_aegp_stream_acquires == g_aegp_stream_disposes",
                       "g_aegp_stream_value_acquires == g_aegp_stream_value_disposes",
                       '"stream_sampled_selector_mask\\\":"',
                       '"effect_param_value_calls\\\":"',
                       "g_aegp_effect_suite4[3] =",
                       "g_aegp_effect_suite4[11] =",
                       "g_aegp_effect_suite4[12] =",
                       "g_aegp_effect_suite4[15] =",
                       "kAegpInstalledEffectKeyNone = 0",
                       "kAegpMaxEffectCategoryNameSize = 128",
                       "kAegpInstalledEffects",
                       "aegp_get_num_installed_effects",
                       "aegp_get_next_installed_effect",
                       "aegp_get_effect_category",
                       "verify_aegp_installed_effect_catalog_suite4",
                       'L"--self-test-aegp-installed-effect-catalog"',
                       "category.fill('Z')",
                       "std::all_of(category.begin(), category.end()",
                       "aegp_get_effect_param_union_by_index_v3",
                       "index < 0 || index >= 5", "kParamSize - 56",
                       '"effect_param_union_calls\\\":"',
                       "dispatch_update_menu();", '"menu_hooks_invoked\\\":"',
                       '"command_checked_true_calls\\\":"',
                       '"command_checked_false_calls\\\":"',
                       '"scene_layer_count\\\":"',
                       "AegpSelectionCollection", "sizeof(AegpCollectionItem) == 56",
                       '"AEGP Collection Suite"', "version == 2",
                       "g_aegp_collection_creates == g_aegp_collection_disposes",
                       '"collection_lifetimes_balanced\\\":"',
                       '"aegp_memory_lifetimes_balanced\\\":"',
                       "aegp_get_layer_name", "make_utf16_handle(u\"Layer \"",
                       "make_utf16_handle(u\"Source \"", '"layer_name_calls\\\":"',
                       "make_utf16_handle(u\"AEXCompat Composition\"",
                       '"item_name_calls\\\":"', '"item_duration_calls\\\":"',
                       'name = u"Amount"', 'name = u"Center"',
                       'name = u"Vector"', 'name = u"Tint"',
                       '"keyframe_count_calls\\\":"',
                       '"keyframed_stream_reports\\\":"',
                       "aegp_get_keyframe_time", "aegp_get_new_keyframe_value",
                       "aegp_get_keyframe_interpolation",
                       '"keyframe_time_calls\\\":"', '"keyframe_value_calls\\\":"',
                       "g_aegp_layer_flags{{0x00000005u, 0x00000005u, 0x00000005u}}",
                       "*transfer = {0, 0, 0}",
                       "g_aegp_layer_durations{{{300, 30}, {300, 30}, {300, 30}}}",
                       '"layer_attribute_calls\\\":"',
                       "g_aegp_scene_frame = tick + 1",
                       '"scene_first_observed_frame\\\":"',
                       '"scene_last_observed_frame\\\":"'):
            self.assertIn(marker, text)

    def test_keyframe_roundtrip_owns_a_bounded_explicit_pipe(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-keyframe-roundtrip"',
                       "static_assert(sizeof(TimelineKeyframeRequest) == 32)",
                       "static_assert(sizeof(TimelineKeyframesSnapshotHeader) == 40)",
                       "static_assert(sizeof(TimelineKeyframedPropHeader) == 104)",
                       "static_assert(sizeof(TimelineKeyframeEntry) == 24)",
                       'CreateNamedPipeW(L"\\\\\\\\.\\\\pipe\\\\ae-timeline-sync"',
                       "g_aegp_keyframe_roundtrip_mode && !keyframe_probe.start()",
                       '"keyframe_pipe_connected\\\":"',
                       '"keyframe_pipe_request_sent\\\":"',
                       '"keyframe_pipe_response_received\\\":"',
                       '"keyframe_pipe_response_valid\\\":"',
                       '"keyframe_pipe_response_bytes\\\":"'):
            self.assertIn(marker, text)

    def test_seek_roundtrip_applies_item_time_and_validates_ack(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-seek-roundtrip"',
                       "static_assert(sizeof(TimelineHostSeekRequest) == 48)",
                       "static_assert(sizeof(TimelineHostSeekAck) == 44)",
                       "aegp_set_item_current_time",
                       "after_get_item_type[14]",
                       "g_aegp_item_set_current_time_calls != 1",
                       "g_aegp_scene_frame != 75",
                       '"scene_current_frame\\\":"',
                       '"seek_pipe_ack_received\\\":"',
                       '"seek_pipe_ack_valid\\\":"'):
            self.assertIn(marker, text)

    def test_trim_roundtrip_mutates_one_layer_and_validates_ack(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-trim-roundtrip"',
                       "static_assert(sizeof(TimelineHostTrimRequest) == 48)",
                       "static_assert(sizeof(TimelineHostTrimAck) == 40)",
                       "aegp_set_layer_in_point_and_duration",
                       "g_aegp_layer_suite9[17]",
                       "g_aegp_layer_trim_set_calls != 1",
                       '"layer_1_duration_value\\\":"',
                       '"trim_pipe_ack_received\\\":"',
                       '"trim_pipe_ack_valid\\\":"'):
            self.assertIn(marker, text)

    def test_switch_roundtrip_handles_inverted_active_flags_and_ack(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('L"--aegp-switch-roundtrip"',
                       "static_assert(sizeof(TimelineHostSwitchRequest) == 40)",
                       "static_assert(sizeof(TimelineHostSwitchAck) == 36)",
                       "aegp_set_layer_flag",
                       "g_aegp_layer_suite8[11]",
                       "g_aegp_layer_flag_set_calls != 4",
                       "g_aegp_layer_flags[0] != 0x00004026u",
                       '"switch_pipe_ack_received\\\":"',
                       '"switch_pipe_ack_valid\\\":"'):
            self.assertIn(marker, text)

    def test_platform_data_is_absolute_bounded_and_fail_closed(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("kUtilsGetPlatformData = 432",
                       "kExeFilePathWide = 7",
                       "kResourceFilePathWide = 8",
                       "g_plugin_file_path.size() >= kMaxPath",
                       "std::filesystem::path(g_plugin_file_path).is_absolute()",
                       "write(utils, kUtilsGetPlatformData, &get_platform_data)"):
            self.assertIn(marker, text)

    def test_typed_pixel_data_callbacks_validate_depth_and_world_bounds(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ("kUtilsGetPixelData8 = 528",
                       "kUtilsGetPixelData16 = 536",
                       "if (format != required_format) return 0",
                       "std::abs(rowbytes) < width * pixel_bytes",
                       "get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb32, 4)",
                       "get_typed_pixel_data(world, pixels0, output, kPixelFormatArgb64, 8)"):
            self.assertIn(marker, text)

    def test_native_stdout_cannot_corrupt_the_worker_json_protocol(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in ('_open("NUL", _O_WRONLY)',
                       "_dup2(g_native_stdout_sink_fd, _fileno(stdout))",
                       "if (!redirect_native_stdout()) { FreeLibrary(module); return 13; }",
                       "void restore_native_stdout()",
                       "restore_native_stdout();"):
            self.assertIn(marker, text)

    def test_arbitrary_debug_print_is_bounded_and_crash_isolated(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "kArbitraryCallback = 22",
            "kMaxArbitraryPrintBytes = 64 * 1024",
            "kMaxSummaryBytes = 4096",
            "invoke_entry_seh(entry, kArbitraryCallback",
            "arbitrary_summary",
            "g_arbitrary_print_failures",
        ):
            self.assertIn(marker, text)

    def test_arbitrary_serialization_roundtrip_is_guarded_and_handle_checked(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "kMaxFlatBytes = 16 * 1024 * 1024",
            "roundtrip_arbitrary_values",
            "host_handle_is_live(restored)",
            "original_flat.begin() + kGuardBytes",
            "g_arbitrary_roundtrip_failures",
            "g_arbitrary_compare_disagreements",
        ):
            self.assertIn(marker, text)

    def test_arbitrary_temporal_interpolation_is_time_bound_and_owned(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "interpolate_arbitrary_values",
            "static_cast<double>(current_time) / total_time",
            "host_handle_is_live(interpolated)",
            "g_arbitrary_new_calls",
            "g_last_arbitrary_interpolation_amount",
        ):
            self.assertIn(marker, text)

    def test_custom_ui_registration_is_bounded_and_reported(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "struct CustomUiRegistration",
            "std::array<std::byte, 44> bytes",
            "(registration.events & ~15u) != 0",
            "value >= 0 && value <= 8192",
            '"custom_ui\\\":{\\\"events\\\":"',
            '"ui_width\\\":"',
        ):
            self.assertIn(marker, text)

    def test_custom_ui_adjust_cursor_is_isolated_and_owned(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "kEvent = 15",
            'L"--l2-adjust-cursor"',
            "std::array<std::byte, 208> extra",
            "invoke_entry_seh(entry, kEvent",
            '"PF AE Adv App Suite"',
            '"event_completed"',
        ):
            self.assertIn(marker, text)

    def test_custom_ui_drawbot_commands_are_bounded_and_owned(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"--l2-draw-event"',
            '"DRAWBOT Draw Suite"',
            '"PF Effect Custom UI Suite"',
            '"PF AE App Suite"',
            "g_drawbot_objects_created",
            "g_drawbot_objects_released",
            "g_drawbot_invalid_operations",
            "g_drawbot_fill_colors.size() == g_drawbot_fill_path_calls",
            "drawbot_fill_color_count",
        ):
            self.assertIn(marker, text)

    def test_custom_ui_click_is_bounded_changed_and_invalidated(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"--l2-click-event"',
            'L"%d,%d,%f,%f,%f,%f"',
            "app_color_picker",
            "app_invalidate_rect",
            "(event_out_flags & 9) == 9",
            "g_app_color_picker_calls == 1",
            "g_app_invalidate_rect_calls == 1",
            "changed_value",
        ):
            self.assertIn(marker, text)

    def test_custom_ui_click_can_mutate_the_same_state_used_for_classic_and_smart_render(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"click:v1|"',
            "smart_image_click_context",
            "smart_image_click_argc",
            "g_render_click_enabled",
            "dispatch_render_click(entry, input, command_output, definitions)",
            "struct SmartRenderUiContextScope",
            "custom_ui_click_dispatched",
            "custom_ui_click_changed_value",
            "(!g_render_click_enabled && !g_render_draw_enabled)",
            "app_color_picker_calls",
        ):
            self.assertIn(marker, text)

    def test_custom_ui_draw_can_share_the_classic_or_smart_render_lifecycle(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"draw:v1"',
            "g_render_draw_enabled",
            "dispatch_render_draw(entry, input, command_output, definitions)",
            "custom_ui_draw_dispatched",
            "custom_ui_draw_error",
            "custom_ui_draw_out_flags",
            "g_render_ui_context_closed",
        ):
            self.assertIn(marker, text)

    def test_custom_ui_context_is_a_valid_effect_window_handle(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            "struct HostUiContext",
            "uint32_t magic{0x05ea771e}",
            "int32_t window_type{2}",
            "std::array<intptr_t, 4> plugin_state",
            "HostUiContext* g_ui_context_pointer = &g_ui_context",
            "write<void*>(extra, 0, &g_ui_context_pointer)",
            "context != &g_ui_context_pointer",
        ):
            self.assertIn(marker, text)

    def test_custom_ui_drag_is_continuation_bound_and_finalized(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            'L"--l2-drag-event"',
            "drag_steps < 1 || drag_steps > 32",
            "write<int32_t>(extra, 8, 3)",
            "write<uint8_t>(extra, 73, step == drag_steps ? 1 : 0)",
            "g_ui_drag_requested",
            "g_ui_drag_terminated",
            "ui_transform_point_simple",
        ):
            self.assertIn(marker, text)

    def test_comp_layer_draw_uses_overlay_theme_and_registered_target(self):
        text = SOURCE.read_text(encoding="utf-8")
        for marker in (
            '"PF Effect Custom UI Overlay Theme Suite"',
            "overlay_foreground",
            "overlay_stroke_path",
            "drawbot_path_point",
            "registered_layer_ui",
            "event_target = drag_event_mode || ui_mouse_exited_mode ||",
            "g_ui_context.window_type != 2",
        ):
            self.assertIn(marker, text)

    def test_suite_lease_leaks_are_reported_without_discarding_valid_images(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn('"suite_lease_warning\\\":"', text)
        self.assertIn("smart.guards_intact && handle_lifetimes_balanced()", text)
        self.assertNotIn(
            "smart.guards_intact && suite_leases_balanced() && handle_lifetimes_balanced()",
            text,
        )

    def test_effect_initialization_exposes_the_frozen_utility_suite3_layout(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("struct UtilitySuite3", text)
        self.assertIn("void* unsupported[7]{}", text)
        self.assertIn("UtilitySuite3 g_utility_suite3", text)
        self.assertIn(
            'std::strcmp(name, "AEGP Utility Suite") == 0 && version == 7', text
        )

    def test_discovery_selectors_share_the_seh_boundary_and_report_selector(self):
        text = SOURCE.read_text(encoding="utf-8")
        for selector in ("kGlobalSetup", "kAbout", "kParamsSetup", "kGlobalSetdown"):
            self.assertRegex(text, rf"invoke_entry_seh\s*\(\s*entry,\s*{selector}")
        for name in ("ABOUT", "GLOBAL_SETUP", "GLOBAL_SETDOWN", "PARAMS_SETUP"):
            self.assertIn(f'return "{name}"', text)
        self.assertIn('"last_seh_selector\\\":\\\""', text)
        self.assertIn('"last_seh_error\\\":"', text)

    def test_all_macro_effect_calls_share_the_audited_seh_boundary(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("int32_t guarded_effect_call(EffectEntry entry", text)
        self.assertIn("return invoke_entry_seh(entry, command, input, output, params, world, extra,", text)
        self.assertIn("#define entry(...) guarded_effect_call(entry, __VA_ARGS__)", text)
        self.assertNotIn("#define entry(...) audited_effect_call(entry, __VA_ARGS__)", text)

    def test_effect_selector_diagnostics_cover_hosted_selector_families(self):
        text = SOURCE.read_text(encoding="utf-8")
        for name in (
            "SEQUENCE_SETUP", "SEQUENCE_RESETUP", "SEQUENCE_FLATTEN",
            "SEQUENCE_SETDOWN", "DO_DIALOG", "FRAME_SETUP", "RENDER",
            "FRAME_SETDOWN", "USER_CHANGED_PARAM", "UPDATE_PARAMS_UI", "EVENT",
            "GET_EXTERNAL_DEPENDENCIES", "QUERY_DYNAMIC_FLAGS", "AUDIO_RENDER",
            "AUDIO_SETUP", "AUDIO_SETDOWN", "ARBITRARY_CALLBACK",
            "SMART_PRE_RENDER", "SMART_RENDER", "GET_FLATTENED_SEQUENCE_DATA",
            "SMART_RENDER_GPU", "GPU_DEVICE_SETUP", "GPU_DEVICE_SETDOWN",
        ):
            self.assertIn(f'return "{name}"', text)

    def test_params_only_discovery_does_not_require_about(self):
        text = SOURCE.read_text(encoding="utf-8")
        self.assertIn("g_skip_about =", text)
        self.assertIn("params_only_mode || external_dependencies_mode", text)
        self.assertIn("about_error = g_skip_about ? 0", text)


if __name__ == "__main__":
    unittest.main()
