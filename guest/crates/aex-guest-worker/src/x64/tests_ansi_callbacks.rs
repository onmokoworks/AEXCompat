#[test]
fn classic_utils_wires_all_ansi_numeric_callbacks_to_working_trampolines() {
    fn call_numeric(engine: &mut GuestEngine<'static>, address: u64, left: f64, right: f64) -> f64 {
        for (register, value) in [(RegisterX86::XMM0, left), (RegisterX86::XMM1, right)] {
            let mut xmm = [0u8; 16];
            xmm[..8].copy_from_slice(&value.to_le_bytes());
            engine.unicorn.reg_write_long(register, &xmm).unwrap();
        }
        engine.call_win64(address, [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        f64::from_le_bytes(xmm0[..8].try_into().unwrap())
    }

    let mut engine = test_engine(&[0xc3]);
    let utility_bytes = crate::classic::build_utility_callbacks(&engine);
    for (offset, address) in [
        (abi::UTILS_ANSI_ATAN_OFFSET, HOST_PF_ANSI_ATAN),
        (abi::UTILS_ANSI_ATAN2_OFFSET, HOST_PF_ANSI_ATAN2),
        (abi::UTILS_ANSI_CEIL_OFFSET, HOST_PF_ANSI_CEIL),
        (abi::UTILS_ANSI_COS_OFFSET, HOST_PF_ANSI_COS),
        (abi::UTILS_ANSI_EXP_OFFSET, HOST_PF_ANSI_EXP),
        (abi::UTILS_ANSI_FABS_OFFSET, HOST_PF_ANSI_FABS),
        (abi::UTILS_ANSI_FLOOR_OFFSET, HOST_PF_ANSI_FLOOR),
        (abi::UTILS_ANSI_FMOD_OFFSET, HOST_PF_ANSI_FMOD),
        (abi::UTILS_ANSI_HYPOT_OFFSET, HOST_PF_ANSI_HYPOT),
        (abi::UTILS_ANSI_LOG_OFFSET, HOST_PF_ANSI_LOG),
        (abi::UTILS_ANSI_LOG10_OFFSET, HOST_PF_ANSI_LOG10),
        (abi::UTILS_ANSI_POW_OFFSET, HOST_PF_ANSI_POW),
        (abi::UTILS_ANSI_SIN_OFFSET, HOST_PF_ANSI_SIN),
        (abi::UTILS_ANSI_SQRT_OFFSET, HOST_PF_ANSI_SQRT),
        (abi::UTILS_ANSI_TAN_OFFSET, HOST_PF_ANSI_TAN),
        (
            abi::UTILS_ANSI_SPRINTF_OFFSET,
            engine.ansi_sprintf_callback_address(),
        ),
        (
            abi::UTILS_ANSI_STRCPY_OFFSET,
            engine.ansi_strcpy_callback_address(),
        ),
        (abi::UTILS_ANSI_ASIN_OFFSET, HOST_PF_ANSI_ASIN),
        (abi::UTILS_ANSI_ACOS_OFFSET, HOST_PF_ANSI_ACOS),
    ] {
        assert_eq!(
            u64::from_le_bytes(utility_bytes[offset..offset + 8].try_into().unwrap()),
            address
        );
    }

    let cases = [
        (HOST_PF_ANSI_ATAN, 1.0, 0.0, std::f64::consts::FRAC_PI_4),
        (HOST_PF_ANSI_ATAN2, 1.0, 1.0, std::f64::consts::FRAC_PI_4),
        (HOST_PF_ANSI_EXP, 1.0, 0.0, std::f64::consts::E),
        (HOST_PF_ANSI_FLOOR, 3.75, 0.0, 3.0),
        (HOST_PF_ANSI_FMOD, 7.5, 2.0, 1.5),
        (HOST_PF_ANSI_LOG, std::f64::consts::E, 0.0, 1.0),
        (HOST_PF_ANSI_LOG10, 100.0, 0.0, 2.0),
        (HOST_PF_ANSI_TAN, std::f64::consts::FRAC_PI_4, 0.0, 1.0),
    ];
    for (address, left, right, expected) in cases {
        let actual = call_numeric(&mut engine, address, left, right);
        assert!((actual - expected).abs() < 1e-12, "address={address:#x}");
    }
    assert!(engine.unicorn.get_data().callback_error.is_none());
}
