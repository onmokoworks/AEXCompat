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
