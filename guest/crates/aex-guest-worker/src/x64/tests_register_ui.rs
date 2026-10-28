fn custom_ui_bytes(values: [i32; 11]) -> [u8; abi::PF_CUSTOM_UI_INFO_SIZE] {
    let mut bytes = [0u8; abi::PF_CUSTOM_UI_INFO_SIZE];
    for (index, value) in values.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn call_register_ui(engine: &mut GuestEngine<'static>, effect_ref: u64, info: u64) -> u64 {
    engine
        .call_win64_with_timeout(
            HOST_REGISTER_UI,
            &[effect_ref, info, 0, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap()
}

fn setup_register_ui_code() -> Vec<u8> {
    let mut code = vec![
        0x83, 0xf9, 0x04, // cmp ecx, PF_CMD_PARAMS_SETUP
        0x74, 0x03, // je params_setup
        0x31, 0xc0, // xor eax, eax
        0xc3, // ret
        0x49, 0x89, 0xd2, // mov r10, rdx (PF_InData)
        0x48, 0x83, 0xec, 0x58, // sub rsp, 0x58
    ];
    for (offset, value) in [0i32, 4, 640, 360, -7, 320, 180, 2, 160, 90, 3]
        .into_iter()
        .enumerate()
    {
        code.extend_from_slice(&[0xc7, 0x44, 0x24, 0x20 + (offset as u8 * 4)]);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[
        0x49, 0x8b, 0x8a, 0xb8, 0x00, 0x00, 0x00, // mov rcx, [r10 + effect_ref]
        0x48, 0x8d, 0x54, 0x24, 0x20, // lea rdx, [rsp + custom_ui]
        0x41, 0xff, 0x52, 0x28, // call qword ptr [r10 + register_ui]
        0x48, 0x83, 0xc4, 0x58, // add rsp, 0x58
        0xc3, // ret
    ]);
    code
}

#[test]
fn guest_register_ui_retains_the_valid_bounded_contract() {
    let mut engine = test_engine(&[0xc3]);
    let callbacks = crate::classic::build_interact_callbacks(&engine);
    assert_eq!(
        u64::from_le_bytes(
            callbacks[abi::INTER_REGISTER_UI_OFFSET..abi::INTER_REGISTER_UI_OFFSET + 8]
                .try_into()
                .unwrap()
        ),
        HOST_REGISTER_UI,
        "the production PF_InData interaction table must not leave register_ui poisoned"
    );
    let info = engine.allocate(abi::PF_CUSTOM_UI_INFO_SIZE, 4).unwrap();
    engine
        .write(
            info,
            &custom_ui_bytes([99, 15, 640, 360, -7, 320, 180, 2, 160, 90, 3]),
        )
        .unwrap();

    assert_eq!(abi::INTER_REGISTER_UI_OFFSET, 0x28);
    assert_eq!(engine.register_ui_callback_address(), HOST_REGISTER_UI);
    assert_eq!(call_register_ui(&mut engine, 1, info), 0);
    assert_eq!(
        engine.custom_ui_registration(),
        Some(CustomUiRegistration {
            events: 15,
            comp_width: 640,
            comp_height: 360,
            comp_alignment: -7,
            layer_width: 320,
            layer_height: 180,
            layer_alignment: 2,
            preview_width: 160,
            preview_height: 90,
            preview_alignment: 3,
        })
    );
}

#[test]
fn guest_register_ui_rejects_bad_or_incomplete_input_without_replacing_state() {
    let mut engine = test_engine(&[0xc3]);
    let info = engine.allocate(abi::PF_CUSTOM_UI_INFO_SIZE, 4).unwrap();
    engine
        .write(info, &custom_ui_bytes([0, 1, 1, 2, 0, 3, 4, 0, 5, 6, 0]))
        .unwrap();
    assert_eq!(call_register_ui(&mut engine, 1, info), 0);
    let retained = engine.custom_ui_registration();

    for invalid in [
        [0, 16, 1, 2, 0, 3, 4, 0, 5, 6, 0],
        [0, 1, -1, 2, 0, 3, 4, 0, 5, 6, 0],
        [0, 1, 1, 2, 0, 3, 4, 0, 8193, 6, 0],
    ] {
        engine.write(info, &custom_ui_bytes(invalid)).unwrap();
        assert_eq!(call_register_ui(&mut engine, 1, info), 4);
        assert_eq!(engine.custom_ui_registration(), retained);
    }
    for (effect_ref, pointer) in [
        (0, info),
        (2, info),
        (1, 0),
        (1, 0x5000_0000),
        (1, DATA_BASE + PAGE_SIZE - 4),
    ] {
        assert_eq!(call_register_ui(&mut engine, effect_ref, pointer), 4);
        assert_eq!(engine.custom_ui_registration(), retained);
    }
}

#[test]
fn classic_setup_reports_the_custom_ui_registered_through_pf_in_data() {
    let code = setup_register_ui_code();
    let engine = test_engine(&code);
    let mut host = crate::classic::ClassicHost::from_test_engine(engine, TEST_CODE).unwrap();

    let report = host.setup().unwrap();
    let expected = CustomUiRegistration {
        events: 4,
        comp_width: 640,
        comp_height: 360,
        comp_alignment: -7,
        layer_width: 320,
        layer_height: 180,
        layer_alignment: 2,
        preview_width: 160,
        preview_height: 90,
        preview_alignment: 3,
    };
    assert_eq!(report.custom_ui, Some(expected));
    assert_eq!(
        serde_json::to_value(&report).unwrap()["custom_ui"],
        serde_json::json!({
            "events": 4,
            "comp_width": 640,
            "comp_height": 360,
            "comp_alignment": -7,
            "layer_width": 320,
            "layer_height": 180,
            "layer_alignment": 2,
            "preview_width": 160,
            "preview_height": 90,
            "preview_alignment": 3,
        })
    );
}

#[test]
fn classic_user_changed_param_uses_the_real_selector_slot_and_returns_dynamic_flags() {
    let mut code = vec![
        0x83,
        0xf9,
        0x0d, // cmp ecx, PF_CMD_USER_CHANGED_PARAM
        0x75,
        0x00, // jne failure (patched below)
        0x49,
        0x8b,
        0x41,
        0x08, // mov rax, [r9 + sizeof(void*)]
        0x83,
        0x78,
        u8::try_from(abi::PARAM_U_OFFSET).unwrap(),
        0x25, // cmp current value, 37
        0x75,
        0x00, // jne failure (patched below)
        0xc7,
        0x40,
        0x04,
        0x20,
        0x00,
        0x00,
        0x00, // ui_flags = DISABLED
        0xc7,
        0x40,
        0x30,
        0x40,
        0x00,
        0x00,
        0x00, // flags = SUPERVISE
        0x48,
        0x8b,
        0x54,
        0x24,
        0x30, // mov rdx, [rsp + sixth argument]
        0x83,
        0x3a,
        0x01, // cmp dword ptr [rdx], 1
        0x75,
        0x00, // jne failure (patched below)
        0x31,
        0xc0, // xor eax, eax
        0xc3, // ret
    ];
    let failure = code.len();
    code.extend_from_slice(&[0xb8, 0x11, 0x00, 0x00, 0x00, 0xc3]);
    code[4] = u8::try_from(failure - 5).unwrap();
    code[14] = u8::try_from(failure - 15).unwrap();
    code[failure - 4] = 3;

    let engine = test_engine(&code);
    let mut host = crate::classic::ClassicHost::from_test_engine(engine, TEST_CODE).unwrap();
    let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
    definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    host.prepare_test_user_changed_parameters(vec![definition])
        .unwrap();
    host.apply_resident_parameter_values(&[crate::classic::ParameterValue {
        slot: Some(1),
        name: "Amount".into(),
        value: Some(37.0),
        color: None,
        point: None,
        angle: None,
        point3d: None,
    }])
    .unwrap();

    let report = host.user_changed_parameter(1).unwrap();
    let extra = host.test_user_changed_extra().unwrap();
    assert_eq!(report.slot, 1);
    assert_eq!(report.selector_error, 0);
    assert_eq!(report.parameters.len(), 1);
    assert_eq!(report.parameters[0].slot, 1);
    assert_eq!(report.parameters[0].ui_flags, 1 << 5);
    assert_eq!(report.parameters[0].flags, 1 << 6);
    assert_eq!(host.user_changed_parameter(1).unwrap().slot, 1);
    assert_eq!(host.test_user_changed_extra(), Some(extra));
    assert!(host.user_changed_parameter(0).is_err());
    assert!(host.user_changed_parameter(2).is_err());
}

#[test]
fn ui_update_records_cosmetics_preserves_values_and_rejects_wrong_scope() {
    let mut engine = test_engine(&[0xc3]);
    engine.enable_ui_session().unwrap();
    let target = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let source = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let mut original = vec![0; abi::PF_PARAM_DEF_SIZE];
    original[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4].copy_from_slice(&123i32.to_le_bytes());
    engine.write(target, &original).unwrap();
    let mut changed = original.clone();
    changed[abi::PARAM_UI_FLAGS_OFFSET..abi::PARAM_UI_FLAGS_OFFSET + 4]
        .copy_from_slice(&32u32.to_le_bytes());
    changed[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + 4].copy_from_slice(b"new\0");
    changed[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4].copy_from_slice(&456i32.to_le_bytes());
    engine.write(source, &changed).unwrap();
    engine.unicorn.get_data_mut().parameter_definitions = vec![target];
    engine.begin_ui_dispatch();
    assert_eq!(
        engine
            .call_win64_with_timeout(UI_CALLBACK_BASE, &[1, 1, source], TIMEOUT_MICROSECONDS)
            .unwrap(),
        0
    );
    let calls = engine.end_ui_dispatch();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["slot"], 1);
    assert_eq!(calls[0]["ui_flags"], 32);
    let mut actual = vec![0; abi::PF_PARAM_DEF_SIZE];
    engine.read(target, &mut actual).unwrap();
    assert_eq!(
        &actual[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4],
        &123i32.to_le_bytes()
    );
    assert_eq!(
        &actual[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + 4],
        b"new\0"
    );
    assert!(
        engine
            .call_win64_with_timeout(UI_CALLBACK_BASE, &[1, 1, source], TIMEOUT_MICROSECONDS)
            .is_err()
    );
}

#[test]
fn ui_update_rejects_invalid_slot_and_unsupported_suite_slot() {
    for (callback, param_slot) in [
        (UI_CALLBACK_BASE, 0),
        (UI_CALLBACK_BASE, 2),
        (UI_CALLBACK_BASE + STUB_STRIDE, 1),
    ] {
        let mut engine = test_engine(&[0xc3]);
        engine.enable_ui_session().unwrap();
        engine.begin_ui_dispatch();
        assert!(
            engine
                .call_win64_with_timeout(callback, &[1, param_slot, 0], TIMEOUT_MICROSECONDS)
                .is_err()
        );
        assert!(engine.end_ui_dispatch().is_empty());
    }
}

#[test]
fn ui_drag_preserves_continuation_across_draw_and_checks_context_and_slot() {
    // EffectMain: click returns continuation, drag copies it into param 1.
    let code = [
        0x48, 0x8b, 0x44, 0x24, 0x30, // rax = sixth arg
        0x83, 0x78, 0x08, 0x02, 0x75, 0x0e, 0xc7, 0x40, 0x28, 0x78, 0x56, 0x34, 0x12, 0xc6, 0x40,
        0x48, 0x01, 0x31, 0xc0, 0xc3, 0x83, 0x78, 0x08, 0x03, 0x75, 0x0c, 0x4c, 0x8b, 0x50, 0x28,
        0x4d, 0x8b, 0x59, 0x08, 0x4d, 0x89, 0x53, 0x38, 0x31, 0xc0, 0xc3,
    ];
    let mut engine = test_engine(&code);
    engine.enable_ui_session().unwrap();
    let mut host = crate::classic::ClassicHost::from_test_engine(engine, TEST_CODE).unwrap();
    let mut definition = vec![0; abi::PF_PARAM_DEF_SIZE];
    definition[12..16].copy_from_slice(&1i32.to_le_bytes());
    host.prepare_test_user_changed_parameters(vec![definition])
        .unwrap();
    let mut event = crate::classic::UiEvent {
        event: 2,
        slot: 1,
        area: 2,
        frame: [30, 40, 340, 210],
        x: 121,
        y: -37,
        clicks: 1,
        last: false,
        context: 1,
        window_type: 2,
        theme: BTreeMap::new(),
        picker_argb_bits: None,
        key_code: None,
        modifiers: None,
    };
    assert_eq!(host.ui_event(&event).unwrap()["send_drag"], 1);
    event.event = 4;
    host.ui_event(&event).unwrap();
    event.event = 3;
    event.context = 2;
    assert!(host.ui_event(&event).is_err());
    event.context = 1;
    event.last = true;
    let report = host.ui_event(&event).unwrap();
    assert_eq!(
        report["after"][0]["value"]["bytes"],
        serde_json::json!([0x78, 0x56, 0x34, 0x12])
    );
    assert!(host.ui_event(&event).is_err());
}

#[test]
fn ui_layer_high_slot_cannot_alias_effect_disposal() {
    let mut engine = test_engine(&[0xc3]);
    engine.enable_ui_session().unwrap();
    engine.begin_ui_dispatch();
    engine.unicorn.get_data_mut().ui_session.effects.insert(123);
    let callback = UI_CALLBACK_BASE + (2 * UI_SLOTS + 40) * STUB_STRIDE;
    assert!(
        engine
            .call_win64_with_timeout(callback, &[123], TIMEOUT_MICROSECONDS)
            .is_err()
    );
    assert!(engine.unicorn.get_data().ui_session.effects.contains(&123));
}

#[test]
fn ui_draw_records_all_rect_words_and_rejects_released_resources() {
    let mut engine = test_engine(&[0xc3]);
    engine.enable_ui_session().unwrap();
    engine.begin_ui_dispatch();
    engine.configure_ui_drawing(55, BTreeMap::new(), None);
    let buffer = engine.allocate(16, 8).unwrap();
    let bytes: Vec<_> = [1u32, 2, 3, 4]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    engine.write(buffer, &bytes).unwrap();
    let callback = UI_CALLBACK_BASE + (11 * UI_SLOTS + 9) * STUB_STRIDE;
    assert_eq!(
        engine
            .call_win64_with_timeout(callback, &[55, buffer], TIMEOUT_MICROSECONDS)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.end_ui_dispatch()[0]["rect_bytes_u32"],
        serde_json::json!([1, 2, 3, 4])
    );
    engine.begin_ui_dispatch();
    let output = engine.allocate(8, 8).unwrap();
    let new_brush = UI_CALLBACK_BASE + (8 * UI_SLOTS + 1) * STUB_STRIDE;
    engine
        .call_win64_with_timeout(new_brush, &[2, buffer, output], TIMEOUT_MICROSECONDS)
        .unwrap();
    let mut id = [0u8; 8];
    engine.read(output, &mut id).unwrap();
    let id = u64::from_le_bytes(id);
    let release = UI_CALLBACK_BASE + (8 * UI_SLOTS + 12) * STUB_STRIDE;
    engine
        .call_win64_with_timeout(release, &[id], TIMEOUT_MICROSECONDS)
        .unwrap();
    assert!(
        engine
            .call_win64_with_timeout(release, &[id], TIMEOUT_MICROSECONDS)
            .is_err()
    );
}
