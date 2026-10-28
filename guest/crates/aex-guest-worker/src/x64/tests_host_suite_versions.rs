// Invoke pointers returned by AcquireSuite, not the implementation directly.
fn census_suite(engine: &mut GuestEngine<'static>, name: &[u8], version: u64) -> u64 {
    let text = engine.allocate(name.len(), 1).unwrap();
    let output = engine.allocate(8, 8).unwrap();
    engine.write(text, name).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [text, version, output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut bytes = [0; 8];
    engine.read(output, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn census_slot(engine: &GuestEngine<'static>, table: u64, slot: u64) -> u64 {
    let mut bytes = [0; 8];
    engine.read(table + slot * 8, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

macro_rules! ansi_v1_numeric_test {
    ($name:ident, $slot:expr, $left:expr, $right:expr, $expected:expr) => {
        #[test]
        fn $name() {
            let mut engine = test_engine(&[0xc3]);
            let table = census_suite(&mut engine, b"PF ANSI Suite\0", 1);
            let callback = census_slot(&engine, table, $slot);
            for (register, value) in [
                (RegisterX86::XMM0, $left as f64),
                (RegisterX86::XMM1, $right as f64),
            ] {
                let mut bytes = [0; 16];
                bytes[..8].copy_from_slice(&value.to_le_bytes());
                engine.unicorn.reg_write_long(register, &bytes).unwrap();
            }
            engine.call_win64(callback, [0; 6]).unwrap();
            let actual = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
            assert_eq!(&actual[..8], &($expected as f64).to_le_bytes());
        }
    };
}

// Exactly representable results; these test routing/ABI, not libm equivalence.
ansi_v1_numeric_test!(census_ansi_v1_slot00_atan, 0, 0.0, 0.0, 0.0);
ansi_v1_numeric_test!(census_ansi_v1_slot01_atan2, 1, 0.0, 1.0, 0.0);
ansi_v1_numeric_test!(census_ansi_v1_slot02_ceil, 2, 1.25, 0.0, 2.0);
ansi_v1_numeric_test!(census_ansi_v1_slot03_cos, 3, 0.0, 0.0, 1.0);
ansi_v1_numeric_test!(census_ansi_v1_slot04_exp, 4, 0.0, 0.0, 1.0);
ansi_v1_numeric_test!(census_ansi_v1_slot05_fabs, 5, -2.5, 0.0, 2.5);
ansi_v1_numeric_test!(census_ansi_v1_slot06_floor, 6, -1.25, 0.0, -2.0);
ansi_v1_numeric_test!(census_ansi_v1_slot07_fmod, 7, 7.5, 2.0, 1.5);
ansi_v1_numeric_test!(census_ansi_v1_slot08_hypot, 8, 3.0, 4.0, 5.0);
ansi_v1_numeric_test!(census_ansi_v1_slot09_log, 9, 1.0, 0.0, 0.0);
ansi_v1_numeric_test!(census_ansi_v1_slot10_log10, 10, 100.0, 0.0, 2.0);
ansi_v1_numeric_test!(census_ansi_v1_slot11_pow, 11, 2.0, 3.0, 8.0);
ansi_v1_numeric_test!(census_ansi_v1_slot12_sin, 12, 0.0, 0.0, 0.0);
ansi_v1_numeric_test!(census_ansi_v1_slot13_sqrt, 13, 9.0, 0.0, 3.0);
ansi_v1_numeric_test!(census_ansi_v1_slot14_tan, 14, 0.0, 0.0, 0.0);
ansi_v1_numeric_test!(census_ansi_v1_slot17_asin, 17, 0.0, 0.0, 0.0);
ansi_v1_numeric_test!(census_ansi_v1_slot18_acos, 18, 1.0, 0.0, 0.0);

#[test]
fn census_ansi_v1_slot15_sprintf() {
    let mut engine = test_engine(&[0xc3]);
    let table = census_suite(&mut engine, b"PF ANSI Suite\0", 1);
    let callback = census_slot(&engine, table, 15);
    let destination = engine.allocate(32, 1).unwrap();
    let format = engine.allocate(16, 1).unwrap();
    engine.write(format, b"value=%d/%x\0").unwrap();
    engine.write(destination, &[0x5a; 32]).unwrap();
    // Existing guest sprintf deliberately rejects varargs; preserve that boundary.
    assert_eq!(
        engine
            .call_win64(callback, [destination, format, 42, 0xab, 0, 0])
            .unwrap(),
        u32::MAX as u64
    );
    let mut untouched = [0; 32];
    engine.read(destination, &mut untouched).unwrap();
    assert_eq!(untouched, [0x5a; 32]);
    engine.write(format, b"value=100%%\0").unwrap();
    assert_eq!(
        engine
            .call_win64(callback, [destination, format, 0, 0, 0, 0])
            .unwrap(),
        10
    );
    let mut bytes = [0; 12];
    engine.read(destination, &mut bytes).unwrap();
    assert_eq!(&bytes, b"value=100%\0Z");
}

#[test]
fn census_ansi_v1_slot16_strcpy() {
    let mut engine = test_engine(&[0xc3]);
    let table = census_suite(&mut engine, b"PF ANSI Suite\0", 1);
    let callback = census_slot(&engine, table, 16);
    let source = engine.allocate(16, 1).unwrap();
    let destination = engine.allocate(16, 1).unwrap();
    engine.write(source, b"copied\0").unwrap();
    engine.write(destination, &[0x5a; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64(callback, [destination, source, 0, 0, 0, 0])
            .unwrap(),
        destination
    );
    let mut bytes = [0; 8];
    engine.read(destination, &mut bytes).unwrap();
    assert_eq!(&bytes, b"copied\0Z");
}

#[test]
fn census_utility_v5_slot07_register() {
    let mut engine = test_engine(&[0xc3]);
    let table = census_suite(&mut engine, b"AEGP Utility Suite\0", 5);
    let callback = census_slot(&engine, table, 7);
    let output = engine.allocate(8, 8).unwrap();
    engine.write(output, &[0x5a; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(callback, [0, 0, output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut bytes = [0; 8];
    engine.read(output, &mut bytes).unwrap();
    assert_eq!(bytes, [1, 0, 0, 0, 0x5a, 0x5a, 0x5a, 0x5a]);
    assert_eq!(engine.call_win64(callback, [0; 6]).unwrap(), 4);
}

#[test]
fn census_utility_v5_slot08_window() {
    let mut engine = test_engine(&[0xc3]);
    let table = census_suite(&mut engine, b"AEGP Utility Suite\0", 5);
    let callback = census_slot(&engine, table, 8);
    let output = engine.allocate(8, 8).unwrap();
    engine.write(output, &[0x5a; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(callback, [output, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut bytes = [0; 8];
    engine.read(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 8]); // Guest headless contract; not a Windows HWND.
    assert_eq!(engine.call_win64(callback, [0; 6]).unwrap(), 4);
}

#[test]
fn census_utility_all_versions_preserve_diagnostics_and_table_boundaries() {
    let mut engine = test_engine(&[0xc3]);
    // Fixed ABI extents, independent of the implementation's layout helper.
    for (version, count, register, window) in [
        (3, 9, 7, 8),
        (5, 19, 7, 8),
        (7, 25, 7, 8),
        (11, 31, 8, 9),
        (13, 33, 9, 10),
    ] {
        let table = census_suite(&mut engine, b"AEGP Utility Suite\0", version);
        for slot in 0..count {
            if slot == register || slot == window {
                continue;
            }
            let callback = census_slot(&engine, table, slot);
            assert_eq!(engine.call_win64(callback, [0; 6]).unwrap(), 4);
            assert!(
                engine
                    .unsupported_suite_calls()
                    .iter()
                    .any(|call| call.version == version as u32
                        && call.slot == slot as usize
                        && call.call_count == 1)
            );
            assert_eq!(
                classify_runtime_target(&engine.unicorn, Some(callback)).kind,
                "suite"
            );
        }
    }
    assert_eq!(engine.dropped_unsupported_suite_calls(), 0);
}
