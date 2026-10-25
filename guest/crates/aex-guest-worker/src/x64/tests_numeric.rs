// Execute instructions, rather than testing the implementation's source text.
#[test]
fn sse_nan_operand_order_survives_mxcsr_write() {
    let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
    let code = 0x1000_0000;
    uc.mem_map(code, PAGE_SIZE, Prot::ALL).unwrap();
    // ADDSS, SUBSS, MULSS, DIVSS; quiet/signaling payloads in both orders.
    for opcode in [0x58, 0x5c, 0x59, 0x5e] {
        let code = code + (opcode as u64) * 16;
        uc.mem_write(code, &[0xf3, 0x0f, opcode, 0xc1]).unwrap();
        for mxcsr in [None, Some(0x1f80)] {
            if let Some(value) = mxcsr {
                uc.reg_write(RegisterX86::MXCSR, value).unwrap();
            }
            for (a, b) in [
                (0x7fc12345u32, 0xffc12346u32),
                (0xffc12346, 0x7fc12345),
                (0x7f812345, 0xffc12346),
                (0x3f800000, 0xff812346),
            ] {
                uc.reg_write(RegisterX86::XMM0, a as u64).unwrap();
                uc.reg_write(RegisterX86::XMM1, b as u64).unwrap();
                uc.emu_start(code, code + 4, 0, 1).unwrap();
                let first_nan = if a & 0x7f800000 == 0x7f800000 { a } else { b };
                assert_eq!(
                    uc.reg_read(RegisterX86::XMM0).unwrap() as u32,
                    first_nan | 0x00400000
                );
            }
        }
    }
}

#[test]
fn x87_nan_still_selects_larger_significand() {
    let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
    let code = 0x1000_0000;
    uc.mem_map(code, PAGE_SIZE, Prot::ALL).unwrap();
    // FNINIT; FLD tbyte [rdi]; FLD tbyte [rsi]; FADDP; FSTP tbyte [rdx].
    let instructions = [0xdb, 0xe3, 0xdb, 0x2f, 0xdb, 0x2e, 0xde, 0xc1, 0xdb, 0x3a];
    uc.mem_write(code, &instructions).unwrap();
    let a = [0xbc, 0x9a, 0x78, 0x56, 0x34, 0x12, 0, 0xc0, 0xff, 0x7f];
    let b = [0xbd, 0x9a, 0x78, 0x56, 0x34, 0x12, 0, 0xc0, 0xff, 0xff];
    uc.mem_write(code + 128, &a).unwrap();
    uc.mem_write(code + 144, &b).unwrap();
    uc.reg_write(RegisterX86::RDI, code + 128).unwrap();
    uc.reg_write(RegisterX86::RSI, code + 144).unwrap();
    uc.reg_write(RegisterX86::RDX, code + 160).unwrap();
    uc.emu_start(code, code + instructions.len() as u64, 0, 5)
        .unwrap();
    assert_eq!(uc.mem_read_as_vec(code + 160, 10).unwrap(), b);
}

#[test]
fn legacy_deep_world_allocates_clears_and_disposes_pf16() {
    for flags in [2, 3] {
        let mut engine = test_engine(&[]);
        let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_NEW_WORLD8, [1, 3, 2, flags, world, 0])
                .unwrap(),
            0
        );
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        engine.read(world, &mut definition).unwrap();
        let rowbytes = i32::from_le_bytes(
            definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(rowbytes, 24);
        let record = &engine.unicorn.get_data().worlds[&world];
        assert_eq!(record.pixel_format as u32, 0x3631_6561);
        assert_eq!(record.size, 48);
        let data = u64::from_le_bytes(
            definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(data, 48).unwrap(),
            vec![if flags & 1 != 0 { 0 } else { 0xcd }; 48]
        );
        assert_eq!(
            engine
                .call_win64(HOST_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert!(engine.unicorn.get_data().worlds.is_empty());
        assert!(engine.unicorn.mem_read_as_vec(data, 1).is_err());
    }
}

#[test]
fn zero_wall_timeout_still_executes_and_explicit_timeout_still_bounds() {
    let mut engine = test_engine(&[0xb8, 42, 0, 0, 0, 0xc3]);
    assert_eq!(
        engine.call_win64_with_timeout(TEST_CODE, &[], 0).unwrap(),
        42
    );
    let mut engine = test_engine(&[0xeb, 0xfe]); // infinite loop
    assert!(
        engine
            .call_win64_with_timeout(TEST_CODE, &[], 1000)
            .is_err()
    );
}

#[test]
fn ucrt_imports_execute_through_production_library_dispatch() {
    for library in ["ucrtbase.dll", "api-ms-win-crt-math-l1-1-0.dll"] {
        for name in [
            "expf", "atan2f", "powf", "sinf", "cosf", "log2f", "sin", "cos", "exp", "pow",
        ] {
            let mut engine = test_engine(&[]);
            let stub = TEST_CODE + 0x800;
            engine.write(stub, &[0xc3]).unwrap();
            install_win64_import(&mut engine.unicorn, stub, library, name).unwrap();
            for (a, b) in [(0.37f64, 2.4f64), (-0.125, 3.0), (1.25, 0.75)] {
                let double = matches!(name, "sin" | "cos" | "exp" | "pow");
                let expected = match name {
                    "expf" => olm_ucrtmath::expf(a as f32).to_bits() as u64,
                    "atan2f" => olm_ucrtmath::atan2f(a as f32, b as f32).to_bits() as u64,
                    "powf" => olm_ucrtmath::powf(a as f32, b as f32).to_bits() as u64,
                    "sinf" => olm_ucrtmath::sinf(a as f32).to_bits() as u64,
                    "cosf" => olm_ucrtmath::cosf(a as f32).to_bits() as u64,
                    "log2f" => olm_ucrtmath::log2f(a as f32).to_bits() as u64,
                    "sin" => olm_ucrtmath::sin(a).to_bits(),
                    "cos" => olm_ucrtmath::cos(a).to_bits(),
                    "exp" => olm_ucrtmath::exp(a).to_bits(),
                    "pow" => olm_ucrtmath::pow(a, b).to_bits(),
                    _ => unreachable!(),
                };
                engine
                    .unicorn
                    .reg_write(
                        RegisterX86::XMM0,
                        if double {
                            a.to_bits()
                        } else {
                            (a as f32).to_bits() as u64
                        },
                    )
                    .unwrap();
                engine
                    .unicorn
                    .reg_write(
                        RegisterX86::XMM1,
                        if double {
                            b.to_bits()
                        } else {
                            (b as f32).to_bits() as u64
                        },
                    )
                    .unwrap();
                engine.call_win64(stub, [0; 6]).unwrap();
                let actual = engine.unicorn.reg_read(RegisterX86::XMM0).unwrap();
                assert_eq!(
                    if double { actual } else { actual as u32 as u64 },
                    expected,
                    "{library} {name}"
                );
            }
            assert_eq!(
                dispatch_win64_import("fixture.dll", name),
                Win64ImportDispatch::UnsupportedLegacyImport
            );
        }
    }
}
