#[test]
fn iterate16_aliases_null_source_to_the_destination_pixel() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; mov rdx,[r9]; mov [rax],rdx; xor eax,eax; ret
    let mut engine = test_engine(&[
        0x48, 0x8b, 0x44, 0x24, 0x28, 0x49, 0x8b, 0x11, 0x48, 0x89, 0x10, 0x31, 0xc0, 0xc3,
    ]);
    let destination_pixels = engine.allocate(8, 8).unwrap();
    let initial = [1, 2, 3, 4, 5, 6, 7, 8];
    engine.write(destination_pixels, &initial).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&destination_pixels.to_le_bytes());
    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE16,
                &[0, 0, 1, 0, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut output = [0u8; 8];
    engine.read(destination_pixels, &mut output).unwrap();
    assert_eq!(output, initial);
}

#[test]
fn iterate16_rejects_out_of_bounds_areas_without_clamping() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0x31, 0xc0, 0xc3]);
    let destination_pixels = engine.allocate(8, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&destination_pixels.to_le_bytes());
    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    let area = engine.allocate(16, 4).unwrap();
    for bounds in [[-1i32, 0, 1, 1], [0, 0, 2, 1], [1, 0, 0, 1]] {
        engine
            .write(
                area,
                &bounds
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    HOST_ITERATE16,
                    &[0, 0, 1, 0, area, 0, CODE, destination_world],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            4
        );
        assert!(engine.unicorn.get_data().pending_iterate.is_none());
    }
    engine.write(area, &[0; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE16,
                &[0, 0, 1, 0, area, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
}

#[test]
fn iterate16_reports_progress_checks_abort_and_propagates_their_errors() {
    const CODE: u64 = 0x1000_0000;
    const PROGRESS: u64 = CODE + 0x40;
    const ABORT: u64 = CODE + 0x80;
    // Pixel: inc [refcon+12]; copy one ARGB16 pixel; return 0.
    let pixel = [
        0xff, 0x41, 0x0c, 0x48, 0x8b, 0x44, 0x24, 0x28, 0x49, 0x8b, 0x11, 0x48, 0x89, 0x10, 0x31,
        0xc0, 0xc3,
    ];
    // Progress: store current/total; increment [effect_ref+16]; return
    // the test-controlled value at effect_ref+20.
    let progress = [
        0x89, 0x11, 0x44, 0x89, 0x41, 0x04, 0xff, 0x41, 0x10, 0x8b, 0x41, 0x14, 0xc3,
    ];
    // Abort: increment [effect_ref+8]; return the test-controlled value at +24.
    let abort = [0xff, 0x41, 0x08, 0x8b, 0x41, 0x18, 0xc3];
    let mut code = vec![0x90; 0x90];
    code[..pixel.len()].copy_from_slice(&pixel);
    code[0x40..0x40 + progress.len()].copy_from_slice(&progress);
    code[0x80..0x80 + abort.len()].copy_from_slice(&abort);
    let mut engine = test_engine(&code);
    let destination_pixels = engine.allocate(16, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&destination_pixels.to_le_bytes());
    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    let counters = engine.allocate(28, 4).unwrap();
    let in_data = engine.allocate(abi::PF_IN_DATA_SIZE, 8).unwrap();
    let mut input = vec![0u8; abi::PF_IN_DATA_SIZE];
    input[abi::INTER_ABORT_OFFSET..abi::INTER_ABORT_OFFSET + 8]
        .copy_from_slice(&ABORT.to_le_bytes());
    input[abi::INTER_PROGRESS_OFFSET..abi::INTER_PROGRESS_OFFSET + 8]
        .copy_from_slice(&PROGRESS.to_le_bytes());
    input[abi::IN_EFFECT_REF_OFFSET..abi::IN_EFFECT_REF_OFFSET + 8]
        .copy_from_slice(&counters.to_le_bytes());
    engine.write(in_data, &input).unwrap();

    let call = |engine: &mut GuestEngine<'static>, base: i32, final_value: i32| {
        engine.call_win64_with_timeout(
            HOST_ITERATE16,
            &[
                in_data,
                base as u32 as u64,
                final_value as u32 as u64,
                0,
                0,
                counters,
                CODE,
                destination_world,
            ],
            TIMEOUT_MICROSECONDS,
        )
    };
    assert_eq!(call(&mut engine, 10, 14).unwrap(), 0);
    let mut observed = [0u8; 20];
    engine.read(counters, &mut observed).unwrap();
    let value =
        |bytes: &[u8], offset| i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!(
        [
            value(&observed, 0),
            value(&observed, 4),
            value(&observed, 8),
            value(&observed, 12),
            value(&observed, 16),
        ],
        [14, 14, 1, 2, 2]
    );

    engine.write(counters, &[0; 28]).unwrap();
    assert_eq!(call(&mut engine, 14, 10).unwrap(), 0);
    engine.read(counters, &mut observed).unwrap();
    assert_eq!(
        [
            value(&observed, 0),
            value(&observed, 4),
            value(&observed, 8),
            value(&observed, 12),
            value(&observed, 16),
        ],
        [4, 4, 1, 2, 2]
    );

    engine.write(counters, &[0; 28]).unwrap();
    engine.write(counters + 20, &23i32.to_le_bytes()).unwrap();
    assert_eq!(call(&mut engine, 10, 14).unwrap(), 23);

    engine.write(counters, &[0; 28]).unwrap();
    engine.write(counters + 24, &29i32.to_le_bytes()).unwrap();
    assert_eq!(call(&mut engine, 10, 14).unwrap(), 29);
}

#[test]
fn iterate16_fails_closed_for_short_rows_and_oversized_walks() {
    const CODE: u64 = 0x1000_0000;
    // mov eax,17; ret
    let mut engine = test_engine(&[0xb8, 17, 0, 0, 0, 0xc3]);
    let destination_pixels = engine.allocate(8, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&destination_pixels.to_le_bytes());
    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&4i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE16,
                &[0, 0, 1, 0, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        4
    );

    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&(4097i32 * 8).to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&4097i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&4097i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE16,
                &[0, 0, 1, 0, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        4
    );
    assert!(engine.unicorn.get_data().pending_iterate.is_none());

    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    engine.write(destination_world, &world).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE16,
                &[0, 0, 1, 0, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        17
    );
}

#[test]
fn iterate8_origin_offsets_callback_coordinates() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; mov [rax+1],dl; mov [rax+2],r8b; xor eax,eax; ret
    let mut engine = test_engine(&[
        0x48, 0x8b, 0x44, 0x24, 0x28, 0x88, 0x50, 0x01, 0x44, 0x88, 0x40, 0x02, 0x31, 0xc0, 0xc3,
    ]);
    let source_pixels = engine.allocate(8, 4).unwrap();
    let destination_pixels = engine.allocate(8, 4).unwrap();
    let source_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    for (world, pixels) in [
        (source_world, source_pixels),
        (destination_world, destination_pixels),
    ] {
        let mut bytes = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        bytes[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&pixels.to_le_bytes());
        bytes[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&8i32.to_le_bytes());
        bytes[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&2i32.to_le_bytes());
        bytes[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        engine.write(world, &bytes).unwrap();
    }
    let origin = engine.allocate(8, 4).unwrap();
    engine
        .write(
            origin,
            &[10i32.to_le_bytes(), (-3i32).to_le_bytes()].concat(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE8_ORIGIN,
                &[0, 0, 1, source_world, 0, origin, 0, CODE, destination_world,],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut output = [0u8; 8];
    engine.read(destination_pixels, &mut output).unwrap();
    assert_eq!(output, [0, 10, 253, 0, 0, 11, 253, 0]);
}

#[test]
fn iterate8_origin_walks_the_destination_and_zeros_outside_the_source() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; mov ecx,[r9]; mov [rax],ecx; xor eax,eax; ret
    let mut engine = test_engine(&[
        0x48, 0x8b, 0x44, 0x24, 0x28, 0x41, 0x8b, 0x09, 0x89, 0x08, 0x31, 0xc0, 0xc3,
    ]);
    let source_pixels = engine.allocate(4, 4).unwrap();
    engine.write(source_pixels, &[1, 2, 3, 4]).unwrap();
    let destination_pixels = engine.allocate(16, 4).unwrap();
    engine.write(destination_pixels, &[0xff; 16]).unwrap();
    let source_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    for (world, pixels, width, height) in [
        (source_world, source_pixels, 1i32, 1i32),
        (destination_world, destination_pixels, 2i32, 2i32),
    ] {
        let mut bytes = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        bytes[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&pixels.to_le_bytes());
        bytes[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&(width * 4).to_le_bytes());
        bytes[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&width.to_le_bytes());
        bytes[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&height.to_le_bytes());
        engine.write(world, &bytes).unwrap();
    }
    let origin = engine.allocate(8, 4).unwrap();
    engine.write(origin, &[0; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE8_ORIGIN,
                &[0, 0, 1, source_world, 0, origin, 0, CODE, destination_world,],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut output = [0u8; 16];
    engine.read(destination_pixels, &mut output).unwrap();
    assert_eq!(output, [1, 2, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn fill8_writes_only_the_requested_argb8_area() {
    let mut engine = test_engine(&[]);
    let pixels = engine.allocate(24, 4).unwrap();
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&pixels.to_le_bytes());
    definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&12i32.to_le_bytes());
    definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&3i32.to_le_bytes());
    definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    engine.write(world, &definition).unwrap();
    let color = engine.allocate(4, 1).unwrap();
    engine.write(color, &[255, 1, 2, 3]).unwrap();
    let area = engine.allocate(16, 4).unwrap();
    engine
        .write(
            area,
            &[
                1i32.to_le_bytes(),
                0i32.to_le_bytes(),
                3i32.to_le_bytes(),
                1i32.to_le_bytes(),
            ]
            .concat(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_FILL8, [0, color, area, world, 0, 0])
            .unwrap(),
        0
    );
    let mut output = [0u8; 24];
    engine.read(pixels, &mut output).unwrap();
    assert_eq!(
        output,
        [
            0, 0, 0, 0, 255, 1, 2, 3, 255, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
}

#[test]
fn legacy_new_world_forces_argb8_and_disposes_through_shared_registry() {
    let mut engine = test_engine(&[]);
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_NEW_WORLD8, [1, 3, 2, 1, world, 0])
            .unwrap(),
        0
    );
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    engine.read(world, &mut definition).unwrap();
    assert_eq!(
        i32::from_le_bytes(
            definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                .try_into()
                .unwrap()
        ),
        12
    );
    assert_eq!(
        engine
            .call_win64(HOST_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(engine.unicorn.get_data().worlds.is_empty());
}

#[test]
fn get_callback_addr_resolves_copy_and_fails_closed_for_unknown_ids() {
    let mut engine = test_engine(&[]);
    let output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_GET_CALLBACK_ADDR, [1, 0, 0, 9, output, 0])
            .unwrap(),
        0
    );
    let mut callback = [0u8; 8];
    engine.read(output, &mut callback).unwrap();
    assert_eq!(u64::from_le_bytes(callback), HOST_COPY);
    assert_eq!(
        engine
            .call_win64(HOST_GET_CALLBACK_ADDR, [1, 0, 0, 999, output, 0])
            .unwrap(),
        4
    );
    engine.read(output, &mut callback).unwrap();
    assert_eq!(u64::from_le_bytes(callback), 0);
}

#[test]
fn iterate8_unsupported_slots_fail_closed_with_suite_diagnostics() {
    let mut engine = test_engine(&[0xc3]);
    let mut callback = [0u8; 8];
    engine.read(HOST_ITERATE8_SUITE + 8, &mut callback).unwrap();
    assert_eq!(
        engine
            .call_win64(u64::from_le_bytes(callback), [0; 6])
            .unwrap(),
        4
    );
    assert_eq!(
        engine.unsupported_suite_calls(),
        [UnsupportedSuiteCall {
            name: "PF Iterate8 Suite",
            version: 1,
            slot: 1,
            call_count: 1,
        }]
    );
}

#[test]
fn pf_ansi_suite_v2_matches_the_windows_slot_layout() {
    let mut engine = test_engine(&[0xc3]);
    let name = DATA_BASE + 0x100;
    let output = DATA_BASE + 0x200;
    engine.unicorn.mem_write(name, b"PF ANSI Suite\0").unwrap();

    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [name, 2, output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut suite_pointer = [0u8; 8];
    engine.unicorn.mem_read(output, &mut suite_pointer).unwrap();
    assert_eq!(u64::from_le_bytes(suite_pointer), HOST_PF_ANSI_SUITE_V2);

    let mut table = [0u8; 21 * 8];
    engine
        .unicorn
        .mem_read(HOST_PF_ANSI_SUITE_V2, &mut table)
        .unwrap();
    let callbacks = table
        .chunks_exact(8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(
        callbacks,
        [
            HOST_PF_ANSI_ATAN,
            HOST_PF_ANSI_ATAN2,
            HOST_PF_ANSI_CEIL,
            HOST_PF_ANSI_COS,
            HOST_PF_ANSI_EXP,
            HOST_PF_ANSI_FABS,
            HOST_PF_ANSI_FLOOR,
            HOST_PF_ANSI_FMOD,
            HOST_PF_ANSI_HYPOT,
            HOST_PF_ANSI_LOG,
            HOST_PF_ANSI_LOG10,
            HOST_PF_ANSI_POW,
            HOST_PF_ANSI_SIN,
            HOST_PF_ANSI_SQRT,
            HOST_PF_ANSI_TAN,
            HOST_PF_ANSI_SPRINTF,
            HOST_PF_ANSI_STRCPY,
            HOST_PF_ANSI_ASIN,
            HOST_PF_ANSI_ACOS,
            0,
            HOST_PF_ANSI_STRCPY_BOUNDED,
        ]
    );
    assert_eq!(engine.suite_requests(), ["PF ANSI Suite v2"]);
}

#[test]
fn pf_ansi_suite_v2_executes_double_and_bounded_string_callbacks() {
    let mut engine = test_engine(&[0xc3]);
    let name = DATA_BASE + 0x20;
    let output = DATA_BASE + 0x40;
    engine.unicorn.mem_write(name, b"PF ANSI Suite\0").unwrap();
    engine
        .call_win64(HOST_ACQUIRE_SUITE, [name, 2, output, 0, 0, 0])
        .unwrap();
    let mut suite_bytes = [0u8; 8];
    engine.unicorn.mem_read(output, &mut suite_bytes).unwrap();
    let suite = u64::from_le_bytes(suite_bytes);
    let callback = |engine: &GuestEngine<'_>, slot: u64| {
        let mut bytes = [0u8; 8];
        engine
            .unicorn
            .mem_read(suite + slot * 8, &mut bytes)
            .unwrap();
        u64::from_le_bytes(bytes)
    };

    let mut xmm0 = [0u8; 16];
    xmm0[..8].copy_from_slice(&(0.5f64).to_le_bytes());
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM0, &xmm0)
        .unwrap();
    engine.call_win64(callback(&engine, 12), [0; 6]).unwrap();
    let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
    let sine = f64::from_le_bytes(xmm0[..8].try_into().unwrap());
    assert!((sine - 0.5f64.sin()).abs() < f64::EPSILON);

    let source = DATA_BASE + 0x100;
    let destination = DATA_BASE + 0x200;
    engine
        .unicorn
        .mem_write(source, b"bounded metadata\0")
        .unwrap();
    engine.unicorn.mem_write(destination, b"XXXXXXXX").unwrap();
    assert_eq!(
        engine
            .call_win64(callback(&engine, 16), [destination, source, 0, 0, 0, 0])
            .unwrap(),
        destination
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination, b"bounded metadata\0".len())
            .unwrap(),
        b"bounded metadata\0"
    );
    assert_eq!(
        engine
            .call_win64(callback(&engine, 20), [destination, 8, source, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
        b"bounded\0"
    );

    let literal_format = DATA_BASE + 0x300;
    engine
        .unicorn
        .mem_write(literal_format, b"Gaussian Blur %% fallback\0")
        .unwrap();
    assert_eq!(
        engine
            .call_win64(
                callback(&engine, 15),
                [destination, literal_format, 0, 0, 0, 0],
            )
            .unwrap(),
        24
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 25).unwrap(),
        b"Gaussian Blur % fallback\0"
    );
    let unsupported_format = DATA_BASE + 0x380;
    engine
        .unicorn
        .mem_write(unsupported_format, b"value=%d\0")
        .unwrap();
    let before = engine.unicorn.mem_read_as_vec(destination, 25).unwrap();
    assert_eq!(
        engine
            .call_win64(
                callback(&engine, 15),
                [destination, unsupported_format, 7, 0, 0, 0],
            )
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 25).unwrap(),
        before
    );
}

#[test]
fn pf_ansi_numeric_policy_matches_the_windows_finite_contract() {
    assert_eq!(ansi_sqrt(-1.0), 0.0);
    assert_eq!(ansi_log(0.0), 0.0);
    assert_eq!(ansi_asin(2.0), 0.0);
    assert_eq!(ansi_fmod(4.0, 0.0), 0.0);
    assert_eq!(ansi_pow(f64::NAN, 2.0), 0.0);
    assert_eq!(ansi_hypot(f64::INFINITY, 1.0), 0.0);
    assert_eq!(ansi_hypot(1.0, f64::NAN), 0.0);
    assert_eq!(ansi_exp(1000.0), 0.0);
    assert_eq!(ansi_pow(2.0, 3.0), 8.0);
    assert_eq!(ansi_hypot(3.0, 4.0), 5.0);
}

#[test]
fn pf_util_hypot_callback_uses_win64_xmm_arguments_and_fails_closed() {
    fn call_hypot(engine: &mut GuestEngine<'static>, left: f64, right: f64) -> f64 {
        for (register, value) in [(RegisterX86::XMM0, left), (RegisterX86::XMM1, right)] {
            let mut xmm = [0u8; 16];
            xmm[..8].copy_from_slice(&value.to_le_bytes());
            engine.unicorn.reg_write_long(register, &xmm).unwrap();
        }
        engine.call_win64(HOST_PF_ANSI_HYPOT, [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        f64::from_le_bytes(xmm0[..8].try_into().unwrap())
    }

    assert_eq!(abi::UTILS_ANSI_HYPOT_OFFSET, 0x110);
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(engine.ansi_hypot_callback_address(), HOST_PF_ANSI_HYPOT);
    assert_eq!(call_hypot(&mut engine, 5.0, 12.0), 13.0);
    assert_eq!(call_hypot(&mut engine, f64::INFINITY, 12.0), 0.0);
    assert_eq!(call_hypot(&mut engine, 5.0, f64::NAN), 0.0);
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn pf_ansi_bounded_copy_rejects_malformed_calls_and_unmapped_memory() {
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        engine
            .call_win64(HOST_PF_ANSI_STRCPY, [0, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(HOST_PF_ANSI_STRCPY_BOUNDED, [0, 0, 0, 0, 0, 0])
            .unwrap(),
        4
    );
    let error = engine
        .call_win64(
            HOST_PF_ANSI_STRCPY_BOUNDED,
            [DATA_BASE, 32, 0xdead_beef, 0, 0, 0],
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("PF ANSI bounded strcpy source read"),
        "{error}"
    );

    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_map(DATA_BASE + PAGE_SIZE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine
        .unicorn
        .mem_write(DATA_BASE, &vec![b'x'; 4096])
        .unwrap();
    let destination = DATA_BASE + PAGE_SIZE;
    engine
        .unicorn
        .mem_write(destination, b"unchanged\0")
        .unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_PF_ANSI_STRCPY, [destination, DATA_BASE, 0, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination, b"unchanged\0".len())
            .unwrap(),
        b"unchanged\0"
    );
}

#[test]
fn aegp_utility_v7_v13_supported_and_unsupported_slots_execute() {
    let mut engine = test_engine(&[0xc3]);
    let suite_name = engine.allocate(19, 1).unwrap();
    engine.write(suite_name, b"AEGP Utility Suite\0").unwrap();

    for version in [7u64, 13] {
        let output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [suite_name, version, output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut table_bytes = [0u8; 8];
        engine.read(output, &mut table_bytes).unwrap();
        let table = u64::from_le_bytes(table_bytes);
        let (_, register_slot, window_slot) = utility_suite_layout(version as u32).unwrap();

        let mut callback_bytes = [0u8; 8];
        engine
            .read(table + (register_slot * 8) as u64, &mut callback_bytes)
            .unwrap();
        let register = u64::from_le_bytes(callback_bytes);
        let plugin_id = engine.allocate(4, 4).unwrap();
        assert_eq!(
            engine
                .call_win64(register, [0, suite_name, plugin_id, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut plugin_id_bytes = [0u8; 4];
        engine.read(plugin_id, &mut plugin_id_bytes).unwrap();
        assert_eq!(i32::from_le_bytes(plugin_id_bytes), 1);

        engine
            .read(table + (window_slot * 8) as u64, &mut callback_bytes)
            .unwrap();
        let get_window = u64::from_le_bytes(callback_bytes);
        let window = engine.allocate(8, 8).unwrap();
        engine.write(window, &u64::MAX.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(get_window, [window, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut window_bytes = [0u8; 8];
        engine.read(window, &mut window_bytes).unwrap();
        assert_eq!(u64::from_le_bytes(window_bytes), 0);

        engine.read(table, &mut callback_bytes).unwrap();
        let unsupported = u64::from_le_bytes(callback_bytes);
        assert_eq!(engine.call_win64(unsupported, [0; 6]).unwrap(), 4);
        assert_eq!(engine.call_win64(unsupported, [0; 6]).unwrap(), 4);
    }

    let unsupported_output = engine.allocate(8, 8).unwrap();
    engine
        .write(unsupported_output, &u64::MAX.to_le_bytes())
        .unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_ACQUIRE_SUITE,
                [suite_name, 12, unsupported_output, 0, 0, 0],
            )
            .unwrap(),
        u32::MAX as u64
    );
    let mut unsupported_output_bytes = [0u8; 8];
    engine
        .read(unsupported_output, &mut unsupported_output_bytes)
        .unwrap();
    assert_eq!(u64::from_le_bytes(unsupported_output_bytes), 0);

    assert_eq!(
        engine.unsupported_suite_calls(),
        [
            UnsupportedSuiteCall {
                name: "AEGP Utility Suite",
                version: 7,
                slot: 0,
                call_count: 2,
            },
            UnsupportedSuiteCall {
                name: "AEGP Utility Suite",
                version: 13,
                slot: 0,
                call_count: 2,
            },
        ]
    );
    assert_eq!(engine.dropped_unsupported_suite_calls(), 0);
    assert_eq!(
        engine.suite_requests(),
        [
            "AEGP Utility Suite v7",
            "AEGP Utility Suite v13",
            "AEGP Utility Suite v12",
        ]
    );
}

#[test]
fn aegp_compute_cache_v1_registers_bounded_executable_callbacks() {
    let mut engine = test_engine(&[0xc3]);
    let suite_name = engine.allocate(19, 1).unwrap();
    engine.write(suite_name, b"AEGP Compute Cache\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(suite_output, &mut pointer).unwrap();
    assert_eq!(
        u64::from_le_bytes(pointer),
        HOST_AEGP_COMPUTE_CACHE_SUITE_V1
    );
    let mut slots = [0u64; 6];
    for (slot, callback) in slots.iter_mut().enumerate() {
        engine
            .read(
                HOST_AEGP_COMPUTE_CACHE_SUITE_V1 + (slot * 8) as u64,
                &mut pointer,
            )
            .unwrap();
        *callback = u64::from_le_bytes(pointer);
    }
    assert_eq!(slots, HOST_AEGP_COMPUTE_CACHE_CALLBACKS);

    let key = engine.allocate(4, 1).unwrap();
    engine.write(key, b"olm\0").unwrap();
    let callbacks = engine.allocate(32, 8).unwrap();
    let mut callback_record = [0u8; 32];
    for chunk in callback_record.chunks_exact_mut(8) {
        chunk.copy_from_slice(&TEST_CODE.to_le_bytes());
    }
    engine.write(callbacks, &callback_record).unwrap();
    assert_eq!(
        engine
            .call_win64(slots[0], [key, callbacks, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(slots[0], [key, callbacks, 0, 0, 0, 0])
            .unwrap(),
        2
    );
    assert_eq!(
        engine.call_win64(slots[1], [key, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        engine.unsupported_suite_calls().last(),
        Some(&UnsupportedSuiteCall {
            name: "AEGP Compute Cache".into(),
            version: 1,
            slot: 1,
            call_count: 1,
        })
    );
    assert_eq!(
        engine.suite_requests().last().map(String::as_str),
        Some("AEGP Compute Cache v1")
    );
}

#[test]
fn aegp_compute_cache_v1_rejects_malformed_registration_boundaries() {
    let mut engine = test_engine(&[0xc3]);
    let unterminated = engine.allocate(256, 1).unwrap();
    engine.write(unterminated, &[b'x'; 256]).unwrap();
    let callbacks = engine.allocate(32, 8).unwrap();
    engine.write(callbacks, &[0; 32]).unwrap();
    let class_id = engine.allocate(8, 1).unwrap();
    engine.write(class_id, b"foreign\0").unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_AEGP_COMPUTE_CACHE_CALLBACKS[0],
                [class_id, callbacks, 0, 0, 0, 0]
            )
            .unwrap(),
        2
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_AEGP_COMPUTE_CACHE_CALLBACKS[0],
                [0, callbacks, 0, 0, 0, 0]
            )
            .unwrap(),
        3
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_AEGP_COMPUTE_CACHE_CALLBACKS[0],
                [unterminated, callbacks, 0, 0, 0, 0]
            )
            .unwrap(),
        3
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_AEGP_COMPUTE_CACHE_CALLBACKS[0],
                [unterminated, 0, 0, 0, 0, 0]
            )
            .unwrap(),
        3
    );
}

#[test]
fn aegp_memory_v1_slots_zero_through_five_are_bounded_and_fail_closed() {
    let mut engine = test_engine(&[0xc3]);
    let suite_name = engine.allocate(18, 1).unwrap();
    engine.write(suite_name, b"AEGP Memory Suite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut table_bytes = [0u8; 8];
    engine.read(suite_output, &mut table_bytes).unwrap();
    let table = u64::from_le_bytes(table_bytes);
    let mut callbacks = [0u64; 8];
    for (slot, callback) in callbacks.iter_mut().enumerate() {
        engine
            .read(table + (slot * 8) as u64, &mut table_bytes)
            .unwrap();
        *callback = u64::from_le_bytes(table_bytes);
    }

    let label = engine.allocate(4, 1).unwrap();
    engine.write(label, b"olm\0").unwrap();
    let handle_output = engine.allocate(8, 8).unwrap();
    engine
        .write(handle_output, &u64::MAX.to_le_bytes())
        .unwrap();
    assert_eq!(
        engine
            .call_win64(
                callbacks[0],
                [1, label, i32::MAX as u64 + 1, 0, handle_output, 0]
            )
            .unwrap(),
        4
    );
    engine.read(handle_output, &mut table_bytes).unwrap();
    assert_eq!(u64::from_le_bytes(table_bytes), 0);

    assert_eq!(
        engine
            .call_win64(callbacks[0], [1, label, 32, 1, handle_output, 0])
            .unwrap(),
        0
    );
    engine.read(handle_output, &mut table_bytes).unwrap();
    let handle = u64::from_le_bytes(table_bytes);
    assert_ne!(handle, 0);
    assert_eq!(handle % 8, 0);

    let data_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.read(data_output, &mut table_bytes).unwrap();
    let data = u64::from_le_bytes(table_bytes);
    assert_eq!(data % 16, 0);
    let mut bytes = [0xffu8; 32];
    engine.read(data, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 32]);
    engine.write(data, &[0x11, 0x22, 0x33, 0x44]).unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(callbacks[5], [label, 64, handle, 0, 0, 0])
            .unwrap(),
        4
    );
    assert_eq!(
        engine
            .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(callbacks[5], [label, 64, handle, 0, 0, 0])
            .unwrap(),
        0
    );

    let size_output = engine.allocate(4, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[4], [handle, size_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut size_bytes = [0u8; 4];
    engine.read(size_output, &mut size_bytes).unwrap();
    assert_eq!(u32::from_le_bytes(size_bytes), 64);
    assert_eq!(
        engine
            .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.read(data_output, &mut table_bytes).unwrap();
    let resized_data = u64::from_le_bytes(table_bytes);
    let mut resized = [0xffu8; 64];
    engine.read(resized_data, &mut resized).unwrap();
    assert_eq!(&resized[..4], &[0x11, 0x22, 0x33, 0x44]);
    assert_eq!(&resized[4..], &[0; 60]);
    assert_eq!(
        engine
            .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        4
    );
    assert_eq!(
        engine
            .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
            .unwrap(),
        4
    );
    assert_eq!(engine.call_win64(callbacks[6], [0; 6]).unwrap(), 4);
    assert_eq!(engine.call_win64(callbacks[7], [0; 6]).unwrap(), 4);
}

#[test]
fn aegp_memory_v1_callbacks_enforce_aggregate_live_byte_budget_atomically() {
    const MIB: u64 = 1024 * 1024;

    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_unmap(HANDLE_DATA_BASE, PAGE_SIZE)
        .unwrap();
    engine
        .unicorn
        .mem_map(
            HANDLE_DATA_BASE,
            MAX_AEGP_MEMORY_BYTES * 2,
            Prot::READ | Prot::WRITE,
        )
        .unwrap();

    let suite_name = engine.allocate(18, 1).unwrap();
    engine.write(suite_name, b"AEGP Memory Suite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let table = read_u64(&engine, suite_output);
    let mut callbacks = [0u64; 6];
    for (slot, callback) in callbacks.iter_mut().enumerate() {
        *callback = read_u64(&engine, table + (slot * 8) as u64);
    }
    let label = engine.allocate(7, 1).unwrap();
    engine.write(label, b"budget\0").unwrap();
    let handle_output = engine.allocate(8, 8).unwrap();
    let data_output = engine.allocate(8, 8).unwrap();

    let (result, nine_mib_handle) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 9 * MIB);
    assert_eq!(result, 0);
    assert_ne!(nine_mib_handle, 0);
    assert_eq!(
        engine
            .call_win64(callbacks[2], [nine_mib_handle, data_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let nine_mib_data = read_u64(&engine, data_output);
    engine
        .write(nine_mib_data, &[0x51, 0x42, 0x33, 0x24])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[3], [nine_mib_handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let before_rejected_new = aegp_memory_state_snapshot(&engine);
    let (result, rejected_handle) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 9 * MIB);
    assert_eq!(result, 4);
    assert_eq!(rejected_handle, 0);
    assert_eq!(aegp_memory_state_snapshot(&engine), before_rejected_new);
    let mut marker = [0u8; 4];
    engine.read(nine_mib_data, &mut marker).unwrap();
    assert_eq!(marker, [0x51, 0x42, 0x33, 0x24]);
    assert_eq!(
        engine
            .call_win64(callbacks[1], [nine_mib_handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );

    let (result, eight_mib_a) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 8 * MIB);
    assert_eq!(result, 0);
    let (result, eight_mib_b) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 8 * MIB);
    assert_eq!(result, 0);
    assert_eq!(
        engine
            .call_win64(callbacks[2], [eight_mib_a, data_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let eight_mib_a_data = read_u64(&engine, data_output);
    engine
        .write(eight_mib_a_data, &[0xa1, 0xb2, 0xc3, 0xd4])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[3], [eight_mib_a, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );

    let before_one_byte_rejection = aegp_memory_state_snapshot(&engine);
    let (result, rejected_handle) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 1);
    assert_eq!(result, 4);
    assert_eq!(rejected_handle, 0);
    assert_eq!(
        aegp_memory_state_snapshot(&engine),
        before_one_byte_rejection
    );

    assert_eq!(
        engine
            .call_win64(callbacks[1], [eight_mib_b, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let (result, replacement_eight_mib) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, 8 * MIB);
    assert_eq!(result, 0);
    assert_eq!(
        engine
            .call_win64(callbacks[5], [label, 8 * MIB, eight_mib_a, 0, 0, 0])
            .unwrap(),
        0
    );

    let before_rejected_resize = aegp_memory_state_snapshot(&engine);
    assert_eq!(
        engine
            .call_win64(callbacks[5], [label, 9 * MIB, eight_mib_a, 0, 0, 0])
            .unwrap(),
        4
    );
    assert_eq!(aegp_memory_state_snapshot(&engine), before_rejected_resize);
    assert_eq!(
        engine
            .call_win64(callbacks[2], [eight_mib_a, data_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(read_u64(&engine, data_output), eight_mib_a_data);
    engine.read(eight_mib_a_data, &mut marker).unwrap();
    assert_eq!(marker, [0xa1, 0xb2, 0xc3, 0xd4]);
    assert_eq!(
        engine
            .call_win64(callbacks[3], [eight_mib_a, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );

    assert_eq!(
        engine
            .call_win64(callbacks[5], [label, 7 * MIB, eight_mib_a, 0, 0, 0])
            .unwrap(),
        0
    );
    let (result, recovered_one_mib) =
        call_aegp_new_mem_handle(&mut engine, callbacks[0], label, handle_output, MIB);
    assert_eq!(result, 0);

    for handle in [eight_mib_a, replacement_eight_mib, recovered_one_mib] {
        assert_eq!(
            engine
                .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }
    assert!(engine.unicorn.get_data().aegp_memory_handles.is_empty());
}

#[test]
fn aegp_memory_v1_reclaims_backing_storage_for_alloc_free_and_resize_cycles() {
    let mut engine = test_engine(&[0xc3]);
    let suite_name = engine.allocate(18, 1).unwrap();
    engine.write(suite_name, b"AEGP Memory Suite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut bytes = [0u8; 8];
    engine.read(suite_output, &mut bytes).unwrap();
    let table = u64::from_le_bytes(bytes);
    let mut callbacks = [0u64; 6];
    for (slot, callback) in callbacks.iter_mut().enumerate() {
        engine.read(table + (slot * 8) as u64, &mut bytes).unwrap();
        *callback = u64::from_le_bytes(bytes);
    }
    let label = engine.allocate(4, 1).unwrap();
    engine.write(label, b"olm\0").unwrap();
    let handle_output = engine.allocate(8, 8).unwrap();
    let data_output = engine.allocate(8, 8).unwrap();

    let mut first_handle = 0;
    let mut first_data = 0;
    for cycle in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
        assert_eq!(
            engine
                .call_win64(callbacks[0], [1, label, 32, 0, handle_output, 0])
                .unwrap(),
            0
        );
        engine.read(handle_output, &mut bytes).unwrap();
        let handle = u64::from_le_bytes(bytes);
        assert_ne!(handle, first_handle);
        assert_eq!(
            engine
                .call_win64(callbacks[2], [handle, data_output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        engine.read(data_output, &mut bytes).unwrap();
        let data = u64::from_le_bytes(bytes);
        if cycle == 0 {
            first_handle = handle;
            first_data = data;
        } else {
            assert_eq!(data, first_data);
        }
        assert_eq!(
            engine
                .call_win64(callbacks[3], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }

    assert_eq!(
        engine
            .call_win64(callbacks[0], [1, label, 32, 0, handle_output, 0])
            .unwrap(),
        0
    );
    engine.read(handle_output, &mut bytes).unwrap();
    let handle = u64::from_le_bytes(bytes);
    for _ in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 128, handle, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine
                .call_win64(callbacks[5], [label, 32, handle, 0, 0, 0])
                .unwrap(),
            0
        );
    }
    assert_eq!(
        engine
            .call_win64(callbacks[1], [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(callbacks[2], [first_handle, data_output, 0, 0, 0, 0])
            .unwrap(),
        4
    );
}

#[test]
fn execution_trace_records_nested_calls_and_returns_with_rvas() {
    const CODE: u64 = 0x1000_0000;
    // call +1; ret; call +1; ret; ret
    let mut engine = test_engine(&[
        0xe8, 0x01, 0x00, 0x00, 0x00, 0xc3, 0xe8, 0x01, 0x00, 0x00, 0x00, 0xc3, 0xc3,
    ]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(trace.schema, "aexcompat.aex-execution-trace");
    assert_eq!(trace.events.first().unwrap().kind, "selector_enter");
    assert_eq!(trace.events.last().unwrap().kind, "selector_exit");
    let calls = trace
        .events
        .iter()
        .filter(|event| event.kind == "guest_call")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].pc_rva, Some(0));
    assert_eq!(calls[0].target_rva, Some(6));
    assert_eq!(calls[1].depth, 1);
    assert!(
        trace
            .events
            .iter()
            .filter(|event| event.kind == "guest_return")
            .count()
            >= 3
    );
    assert!(
        trace
            .events
            .iter()
            .enumerate()
            .all(|(index, event)| event.sequence == index)
    );
}

#[test]
fn execution_trace_labels_indirect_import_stub_calls() {
    const CODE: u64 = 0x1000_0000;
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&STUB_BASE.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0xc3]);
    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_write(STUB_BASE, &[0x31, 0xc0, 0xc3])
        .unwrap();
    engine.unicorn.get_data_mut().trace_labels.insert(
        STUB_BASE,
        TraceLabel {
            kind: TraceLabelKind::Import,
            name: "fixture.dll!fixture_import".into(),
        },
    );
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let import = trace
        .events
        .iter()
        .find(|event| event.kind == "import_call")
        .unwrap();
    assert_eq!(import.name.as_deref(), Some("fixture.dll!fixture_import"));
    assert!(!trace.timeline.is_empty());
}

#[test]
fn execution_trace_resolves_register_indirect_guest_target() {
    const CODE: u64 = 0x1000_0000;
    let target = CODE + 13;
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&target.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0xc3, 0xc3]);
    let mut engine = test_engine(&code);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let call = trace
        .events
        .iter()
        .find(|event| event.kind == "guest_call")
        .unwrap();
    assert_eq!(call.call_kind, Some("indirect"));
    assert_eq!(call.target_rva, Some(13));
}

#[test]
fn execution_trace_resolves_rip_relative_indirect_guest_target() {
    const CODE: u64 = 0x1000_0000;
    let target = CODE + 16;
    // call qword ptr [rip+2]; ret; nop; dq target; ret
    let mut code = vec![0xff, 0x15, 0x02, 0, 0, 0, 0xc3, 0x90];
    code.extend_from_slice(&target.to_le_bytes());
    code.push(0xc3);
    let mut engine = test_engine(&code);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let call = trace
        .events
        .iter()
        .find(|event| event.kind == "guest_call")
        .unwrap();
    assert_eq!(call.call_kind, Some("indirect"));
    assert_eq!(call.target_rva, Some(16));
}

#[test]
fn execution_trace_keeps_ordinary_jump_as_taken_block_edge() {
    const CODE: u64 = 0x1000_0000;
    // jmp +1; int3; mov eax, 42; ret
    let mut engine = test_engine(&[0xeb, 0x01, 0xcc, 0xb8, 42, 0, 0, 0, 0xc3]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(result, 42);
    assert!(!trace.events.iter().any(|event| event.kind == "tail_call"));
    assert!(
        trace
            .branch_edges
            .iter()
            .any(|edge| edge.from_rva == 0 && edge.to_rva == 3)
    );
    assert!(trace.basic_blocks.iter().any(|block| block.rva == 3));
}

#[test]
fn execution_trace_records_jump_to_known_function_as_tail_call() {
    const CODE: u64 = 0x1000_0000;
    // call target; jmp target; padding; target: inc byte ptr [rcx]; mov eax,42; ret
    let mut engine = test_engine(&[
        0xe8, 0x06, 0, 0, 0, 0xeb, 0x04, 0x90, 0x90, 0x90, 0x90, 0xfe, 0x01, 0xb8, 42, 0, 0, 0,
        0xc3,
    ]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "tail-target".into(),
        function_rva: Some(11),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 1,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(result, 42);
    let tail = trace
        .events
        .iter()
        .find(|event| event.kind == "tail_call")
        .unwrap();
    assert_eq!(tail.pc_rva, Some(5));
    assert_eq!(tail.target_rva, Some(11));
    assert_eq!(tail.call_kind, Some("runtime_jmp"));
    assert!(trace.events.iter().any(|event| {
        event.kind == "guest_return"
            && event.pc_rva == Some(18)
            && event.target_rva.is_none()
            && event.function_rva == Some(11)
    }));
    assert!(!trace.events.iter().any(|event| {
        event.kind == "guest_return" && event.pc_rva == Some(18) && event.function_rva == Some(0)
    }));
    let entry_function = trace
        .functions
        .iter()
        .find(|function| function.entry_rva == 0)
        .unwrap();
    assert_eq!(entry_function.observed_calls, 2);
    assert_eq!(entry_function.callees, vec![11]);
    assert_eq!(trace.memory_witnesses.len(), 2);
    assert_eq!(trace.memory_witnesses[1].watch_id, "tail-target");
    assert_eq!(trace.memory_witnesses[1].before.u8_values, [2]);
    assert_eq!(trace.memory_witnesses[1].after.u8_values, [3]);
}

#[test]
fn execution_trace_watch_can_dereference_pointer_field() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rcx]; inc byte ptr [rax]; ret
    let mut engine = test_engine(&[0x48, 0x8b, 0x01, 0xfe, 0x00, 0xc3]);
    let target = engine.allocate(1, 1).unwrap();
    engine.write(target, &[41]).unwrap();
    let holder = engine.allocate(8, 8).unwrap();
    engine.write(holder, &target.to_le_bytes()).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "dereferenced-target".into(),
        function_rva: Some(0),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: Some(0),
        size: 1,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    engine.call_win64(CODE, [holder, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(0).unwrap();
    let witness = trace.memory_witnesses.first().unwrap();
    assert_eq!(witness.before.u8_values, [41]);
    assert_eq!(witness.after.u8_values, [42]);
}

#[test]
fn checkpoint_trace_skips_hot_basic_block_collection() {
    const CODE: u64 = 0x1000_0000;
    // entry calls a target which increments the watched byte.
    let mut engine = test_engine(&[0xe8, 1, 0, 0, 0, 0xc3, 0xfe, 0x02, 0xc3]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "checkpoint".into(),
        function_rva: Some(6),
        instruction_rva: None,
        absolute_address: None,
        register: "rdx",
        dereference_offset: None,
        size: 1,
        occurrence: Some(1),
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.configure_trace_checkpoint_only(true);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    engine.call_win64(CODE, [0, buffer, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(0).unwrap();
    assert_eq!(trace.trace_configuration.capture_mode, "checkpoint");
    assert!(trace.trace_configuration.unhookable_watches.is_empty());
    assert!(trace.basic_blocks.is_empty());
    assert!(trace.branch_edges.is_empty());
    assert_eq!(trace.memory_witnesses[0].before.u8_values, [1]);
    assert_eq!(trace.memory_witnesses[0].after.u8_values, [2]);
}

#[test]
fn checkpoint_trace_activates_function_watch_at_selector_entry() {
    const CODE: u64 = 0x1000_0000;
    // inc byte ptr [rcx]; ret
    let mut engine = test_engine(&[0xfe, 0x01, 0xc3]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "selector-entry".into(),
        function_rva: Some(0),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 1,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.configure_trace_checkpoint_only(true);
    engine.begin_execution_trace("SMART_RENDER", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(trace.trace_configuration.capture_mode, "checkpoint");
    assert!(trace.trace_configuration.unhookable_watches.is_empty());
    let witness = trace.memory_witnesses.first().unwrap();
    assert_eq!(witness.watch_id, "selector-entry");
    assert_eq!(witness.function_rva, Some(0));
    assert_eq!(witness.before.u8_values, [1]);
    assert_eq!(witness.after.u8_values, [2]);
}

#[test]
fn checkpoint_trace_lists_watches_it_cannot_hook() {
    const CODE: u64 = 0x1000_0000;
    // 0: jmp +1 (tail-call to 6); 5: ret; 6: inc byte ptr [rdx]; 8: ret;
    // 9: call rax (indirect, never executed)
    let mut engine = test_engine(&[
        0xe9, 0x01, 0x00, 0x00, 0x00, 0xc3, 0xfe, 0x02, 0xc3, 0xff, 0xd0,
    ]);
    let entry_buffer = engine.allocate(1, 1).unwrap();
    engine.write(entry_buffer, &[10]).unwrap();
    let tail_buffer = engine.allocate(1, 1).unwrap();
    engine.write(tail_buffer, &[1]).unwrap();
    let watch =
        |id: &str, function_rva: Option<u64>, instruction_rva: Option<u64>| TraceWatchSpec {
            id: id.into(),
            function_rva,
            instruction_rva,
            absolute_address: None,
            register: "rdx",
            dereference_offset: None,
            size: 1,
            occurrence: None,
            image_coordinate: None,
            image_row_offset: None,
            image_format: None,
        };
    engine.configure_trace_watches(vec![
        TraceWatchSpec {
            register: "rcx",
            ..watch("entry", Some(0), None)
        },
        watch("tail-called-function", Some(6), None),
        watch("jump-site", None, Some(0)),
        watch("indirect-call-site", None, Some(9)),
        watch("not-a-call", None, Some(7)),
        watch("uncalled-function", Some(8), None),
    ]);
    engine.configure_trace_checkpoint_only(true);
    engine.begin_execution_trace("SMART_RENDER", CODE).unwrap();
    engine
        .call_win64(CODE, [entry_buffer, tail_buffer, 0, 0, 0, 0])
        .unwrap();
    let trace = engine.finish_execution_trace(0).unwrap();

    assert_eq!(trace.trace_configuration.capture_mode, "checkpoint");
    let unhookable = &trace.trace_configuration.unhookable_watches;
    let ids = unhookable
        .iter()
        .map(|watch| watch.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "tail-called-function",
            "jump-site",
            "indirect-call-site",
            "not-a-call",
            "uncalled-function",
        ]
    );
    let reason = |id: &str| {
        unhookable
            .iter()
            .find(|watch| watch.id == id)
            .map(|watch| watch.reason)
            .unwrap()
    };
    assert!(reason("tail-called-function").contains("tail-call jump"));
    assert!(reason("jump-site").contains("tail-call jump site"));
    assert!(reason("indirect-call-site").contains("indirect call site"));
    assert!(reason("not-a-call").contains("not a call or jump"));
    assert!(reason("uncalled-function").contains("no direct call site"));
    // The tail-called function did run and mutate its buffer, but only the
    // entry watch could produce a witness; the others are absent by design.
    let mut mutated = [0u8; 1];
    engine.read(tail_buffer, &mut mutated).unwrap();
    assert_eq!(mutated, [2]);
    assert_eq!(trace.memory_witnesses.len(), 1);
    assert_eq!(trace.memory_witnesses[0].watch_id, "entry");
    assert_eq!(trace.memory_witnesses[0].before.u8_values, [10]);
    assert_eq!(trace.memory_witnesses[0].after.u8_values, [10]);
}

#[test]
fn execution_trace_treats_explicitly_watched_first_jump_as_tail_call() {
    const CODE: u64 = 0x1000_0000;
    // jmp target; padding; target: mov rax,[rsp+0x28]; inc byte ptr [rax]; ret
    let mut engine = test_engine(&[
        0xeb, 0x02, 0x90, 0x90, 0x48, 0x8b, 0x44, 0x24, 0x28, 0xfe, 0x00, 0xc3,
    ]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "first-tail-stack5".into(),
        function_rva: Some(4),
        instruction_rva: None,
        absolute_address: None,
        register: "stack5",
        dereference_offset: None,
        size: 1,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0, 0, 0, 0, buffer, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let tail = trace
        .events
        .iter()
        .find(|event| event.kind == "tail_call")
        .unwrap();
    assert_eq!(tail.target_rva, Some(4));
    assert_eq!(tail.stack_arguments[0].value.raw, buffer);
    let witness = trace.memory_witnesses.first().unwrap();
    assert_eq!(witness.watch_id, "first-tail-stack5");
    assert_eq!(witness.before.u8_values, [1]);
    assert_eq!(witness.after.u8_values, [2]);
}

#[test]
fn execution_trace_applies_watch_occurrence_to_tail_calls() {
    const CODE: u64 = 0x1000_0000;
    // Call target once, then tail-call it. The second match must be the tail call.
    let mut engine = test_engine(&[
        0xe8, 0x06, 0, 0, 0, 0xeb, 0x04, 0x90, 0x90, 0x90, 0x90, 0xfe, 0x01, 0xb8, 42, 0, 0, 0,
        0xc3,
    ]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "second-tail-target".into(),
        function_rva: Some(11),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 1,
        occurrence: Some(2),
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(result, 42);
    assert_eq!(trace.memory_witnesses.len(), 1);
    let witness = &trace.memory_witnesses[0];
    assert_eq!(witness.watch_id, "second-tail-target");
    assert_eq!(witness.before.u8_values, [2]);
    assert_eq!(witness.after.u8_values, [3]);
    assert!(
        trace
            .events
            .iter()
            .any(|event| { event.kind == "tail_call" && event.target_rva == Some(11) })
    );
}

#[test]
fn execution_trace_activates_function_watch_at_selector_entry() {
    const CODE: u64 = 0x1000_0000;
    // inc byte ptr [rcx]; ret
    let mut engine = test_engine(&[0xfe, 0x01, 0xc3]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "selector-entry".into(),
        function_rva: Some(0),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 1,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let witness = trace.memory_witnesses.first().unwrap();
    assert_eq!(witness.watch_id, "selector-entry");
    assert_eq!(witness.function_rva, Some(0));
    assert_eq!(witness.before.u8_values, [1]);
    assert_eq!(witness.after.u8_values, [2]);
}

#[test]
fn execution_trace_records_rip_relative_constant_access() {
    const CODE: u64 = 0x1000_0000;
    let value = 0x1122_3344_5566_7788u64;
    // mov rax,[rip+1]; ret; dq value
    let mut code = vec![0x48, 0x8b, 0x05, 0x01, 0, 0, 0, 0xc3];
    code.extend_from_slice(&value.to_le_bytes());
    let mut engine = test_engine(&code);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(result, value);
    let access = trace
        .events
        .iter()
        .find(|event| event.kind == "rip_constant")
        .unwrap();
    assert_eq!(access.pc_rva, Some(0));
    assert_eq!(access.target_rva, Some(8));
    assert!(access.name.as_deref().unwrap().contains("1122334455667788"));
}

#[test]
fn execution_trace_folds_repeated_call_sites_without_losing_count() {
    const CODE: u64 = 0x1000_0000;
    // mov ecx,2; loop: call return; dec ecx; jnz loop; return: ret
    let mut engine = test_engine(&[
        0xb9, 0x02, 0x00, 0x00, 0x00, 0xe8, 0x04, 0x00, 0x00, 0x00, 0xff, 0xc9, 0x75, 0xf7, 0xc3,
    ]);
    engine.begin_execution_trace("GLOBAL_SETUP", CODE).unwrap();
    let result = engine.call_win64(CODE, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let call = trace
        .events
        .iter()
        .find(|event| event.kind == "guest_call")
        .unwrap();
    assert_eq!(call.pc_rva, Some(5));
    assert_eq!(call.observed_count, 2);
    assert_eq!(call.exemplars.distinct_fingerprints, 2);
    assert_eq!(call.exemplars.first.len(), 2);
    assert_eq!(call.exemplars.last.len(), 2);
    let rcx = call
        .exemplars
        .numeric_ranges
        .iter()
        .find(|range| range.field == "rcx")
        .unwrap();
    assert_eq!((rcx.minimum, rcx.maximum), (1.0, 2.0));
    assert!(trace.timeline.iter().any(|line| line.contains("×2")));
}

#[test]
fn exemplar_fingerprint_tracking_is_bounded_and_explicitly_truncated() {
    let mut exemplars = TraceExemplars::default();
    let mut fingerprints = HashSet::new();
    for index in 0..TRACE_DISTINCT_FINGERPRINTS + 3 {
        update_exemplars(
            &mut exemplars,
            &mut fingerprints,
            TraceObservation {
                observation: index as u64 + 1,
                fingerprint: format!("{index:016x}"),
                call_id: None,
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
            },
        );
    }

    assert_eq!(fingerprints.len(), TRACE_DISTINCT_FINGERPRINTS);
    assert_eq!(
        exemplars.distinct_fingerprints,
        TRACE_DISTINCT_FINGERPRINTS as u64
    );
    assert!(exemplars.fingerprint_tracking_truncated);
    assert_eq!(exemplars.untracked_fingerprint_observations, 3);
}

#[test]
fn execution_trace_reports_fingerprint_budget_truncation() {
    const CODE: u64 = 0x1000_0000;
    let observations = TRACE_DISTINCT_FINGERPRINTS + 4;
    let mut engine = test_engine(&[0xc3]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let capture = engine.unicorn.get_data_mut().trace.as_mut().unwrap();
    for observation in 1..=observations {
        push_trace_event(
            capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 1,
                kind: "guest_call",
                call_id: Some(observation as u64),
                function_rva: Some(0),
                pc_rva: Some(5),
                target_rva: Some(10),
                name: None,
                arguments: vec![TraceArgument {
                    register: "rcx",
                    value: TraceValue {
                        raw: observation as u64,
                        classification: "integer",
                        offset: None,
                    },
                }],
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: Some("direct"),
                instruction_bytes: Some("e800000000".into()),
            },
        );
    }
    let trace = engine.finish_execution_trace(0).unwrap();

    let call = trace
        .events
        .iter()
        .find(|event| event.kind == "guest_call")
        .unwrap();
    assert_eq!(call.observed_count, observations as u64);
    assert!(call.exemplars.fingerprint_tracking_truncated);
    assert_eq!(call.exemplars.untracked_fingerprint_observations, 4);
    assert!(trace.truncation.iter().any(|item| {
        item.category == "exemplar_fingerprints"
            && item.reason == "fingerprint_budget"
            && item.dropped == 4
    }));
    assert!(trace.truncated);
}

#[test]
fn execution_trace_witnesses_memory_before_and_after_a_call() {
    const CODE: u64 = 0x1000_0000;
    // call +1; ret; mov byte ptr [rcx], 0x2a; ret
    let mut engine = test_engine(&[0xe8, 0x01, 0, 0, 0, 0xc3, 0xc6, 0x01, 0x2a, 0xc3]);
    let buffer = engine.allocate(4, 1).unwrap();
    engine.write(buffer, &[1, 2, 3, 4]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "target-buffer".into(),
        function_rva: Some(6),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 4,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let call = trace
        .events
        .iter()
        .find(|event| event.kind == "guest_call")
        .unwrap();
    let returned = trace
        .events
        .iter()
        .find(|event| event.kind == "guest_return" && event.function_rva == Some(6))
        .unwrap();
    assert_eq!(call.call_id, returned.call_id);
    let witness = trace.memory_witnesses.first().unwrap();
    assert_eq!(witness.call_id, call.call_id);
    assert_eq!(witness.before.u8_values, [1, 2, 3, 4]);
    assert_eq!(witness.after.u8_values, [42, 2, 3, 4]);
    assert_eq!(witness.changed_ranges.len(), 1);
    assert_eq!(witness.changed_ranges[0].offset, 0);
    assert_eq!(witness.changed_ranges[0].size, 1);
}

#[test]
fn execution_trace_selects_one_based_watch_occurrence_without_spending_witness_budget() {
    const CODE: u64 = 0x1000_0000;
    // Call the same target three times, then return 42. The target increments [rcx].
    let mut engine = test_engine(&[
        0xe8, 0x10, 0, 0, 0, 0xe8, 0x0b, 0, 0, 0, 0xe8, 0x06, 0, 0, 0, 0xb8, 42, 0, 0, 0, 0xc3,
        0xfe, 0x01, 0xc3,
    ]);
    let buffer = engine.allocate(1, 1).unwrap();
    engine.write(buffer, &[1]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "second-target-call".into(),
        function_rva: Some(21),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 1,
        occurrence: Some(2),
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    assert_eq!(result, 42);
    let mut final_value = [0u8; 1];
    engine.read(buffer, &mut final_value).unwrap();
    assert_eq!(final_value, [4]);
    assert_eq!(trace.memory_witnesses.len(), 1);
    assert_eq!(trace.dropped_memory_witnesses, 0);
    let witness = &trace.memory_witnesses[0];
    assert_eq!(witness.watch_id, "second-target-call");
    assert_eq!(witness.before.u8_values, [2]);
    assert_eq!(witness.after.u8_values, [3]);
}

#[test]
fn inferred_return_path_completes_pending_memory_witness() {
    const CODE: u64 = 0x1000_0000;
    // call target_a; call target_b; ret; nop;
    // target_a: mov byte ptr [rcx],0x2a; ret; target_b: ret
    let mut engine = test_engine(&[
        0xe8, 0x07, 0, 0, 0, 0xe8, 0x06, 0, 0, 0, 0xc3, 0x90, 0xc6, 0x01, 0x2a, 0xc3, 0xc3,
    ]);
    engine.trace_points = vec![CODE, CODE + 5];
    let buffer = engine.allocate(4, 1).unwrap();
    engine.write(buffer, &[1, 2, 3, 4]).unwrap();
    engine.configure_trace_watches(vec![TraceWatchSpec {
        id: "inferred-return".into(),
        function_rva: Some(12),
        instruction_rva: None,
        absolute_address: None,
        register: "rcx",
        dereference_offset: None,
        size: 4,
        occurrence: None,
        image_coordinate: None,
        image_row_offset: None,
        image_format: None,
    }]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let result = engine.call_win64(CODE, [buffer, 0, 0, 0, 0, 0]).unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();

    let witness = trace
        .memory_witnesses
        .iter()
        .find(|witness| witness.watch_id == "inferred-return")
        .unwrap();
    assert_eq!(witness.before.u8_values, [1, 2, 3, 4]);
    assert_eq!(witness.after.u8_values, [42, 2, 3, 4]);
    assert!(trace.events.iter().any(|event| {
        event.kind == "guest_return" && event.name.as_deref() == Some("inferred_from_stack")
    }));
}

#[test]
fn unicorn_rejects_vex_l256_without_the_bounded_fallback() {
    const CODE: u64 = 0x1000_0000;
    let mut unicorn =
        Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
    unicorn.mem_map(CODE, PAGE_SIZE, Prot::ALL).unwrap();
    unicorn
        .mem_write(CODE, &[0xc5, 0xfc, 0x10, 0x00]) // vmovups ymm0,[rax]
        .unwrap();
    unicorn.reg_write(RegisterX86::RAX, CODE).unwrap();

    let error = unicorn.emu_start(CODE, CODE + 4, 0, 1).unwrap_err();

    assert_eq!(error, unicorn_engine::unicorn_const::uc_error::INSN_INVALID);
}

#[test]
fn avx_fallback_moves_all_256_bits_between_memory_and_ymm() {
    const CODE: u64 = 0x1000_0000;
    let source = DATA_BASE;
    let destination = DATA_BASE + 32;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x10, 0x09, // vmovups ymm1,[rcx]
        0xc5, 0xfc, 0x11, 0x0a, // vmovups [rdx],ymm1
        0xc3,
    ]);
    let expected = std::array::from_fn::<_, 32, _>(|index| (index as u8) ^ 0xa5);
    engine.write(source, &expected).unwrap();

    engine
        .call_win64(CODE, [source, destination, 0, 0, 0, 0])
        .unwrap();

    let mut actual = [0u8; 32];
    engine.unicorn.mem_read(destination, &mut actual).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 2);
}

#[test]
fn avx_fallback_decodes_a_complete_instruction_at_the_mapped_image_end() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xc3]);
    let instruction = CODE + PAGE_SIZE - 4;
    let expected = [0x5a; 32];
    engine.write(DATA_BASE, &expected).unwrap();
    engine
        .unicorn
        .mem_write(instruction, &[0xc5, 0xfc, 0x10, 0x00]) // vmovups ymm0,[rax]
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RAX, DATA_BASE)
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RIP, instruction)
        .unwrap();

    assert!(emulate_avx_invalid_instruction(&mut engine.unicorn));

    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RIP).unwrap(),
        CODE + PAGE_SIZE
    );
    let actual = engine.unicorn.reg_read_long(RegisterX86::YMM0).unwrap();
    assert_eq!(&*actual, &expected);
    assert!(engine.unicorn.get_data().avx_defined_ymm[0]);
}

#[test]
fn avx_fallback_blends_xmm_f32_lanes_from_the_mask_sign_bits() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xc4, 0xe3, 0x51, 0x4a, 0xc6, 0x00, // vblendvps xmm0,xmm5,xmm6,xmm0
        0xc3,
    ]);
    let lanes = |values: [f32; 4]| {
        values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>()
    };
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM5, &lanes([1.0, 2.0, 3.0, 4.0]))
        .unwrap();
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM6, &lanes([10.0, 20.0, 30.0, 40.0]))
        .unwrap();
    engine
        .unicorn
        .reg_write_long(
            RegisterX86::XMM0,
            &[
                0u32.to_le_bytes(),
                0x8000_0000u32.to_le_bytes(),
                0u32.to_le_bytes(),
                0x8000_0000u32.to_le_bytes(),
            ]
            .concat(),
        )
        .unwrap();

    engine.call_win64(CODE, [0; 6]).unwrap();

    let actual = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
    assert_eq!(&actual[..16], lanes([1.0, 20.0, 3.0, 40.0]));
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 1);
}

#[test]
fn avx_fallback_blends_xmm_f64_lanes_and_zeroes_the_upper_half() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xc4, 0xe3, 0x79, 0x4b, 0xc2, 0x30, // vblendvpd xmm0,xmm0,xmm2,xmm3
        0xc3,
    ]);
    let lanes = |values: [f64; 2]| {
        values
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>()
    };
    engine
        .unicorn
        .reg_write_long(RegisterX86::YMM0, &[0x5a; 32])
        .unwrap();
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM0, &lanes([1.0, 2.0]))
        .unwrap();
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM2, &lanes([10.0, 20.0]))
        .unwrap();
    engine
        .unicorn
        .reg_write_long(
            RegisterX86::XMM3,
            &[0u64.to_le_bytes(), 0x8000_0000_0000_0000u64.to_le_bytes()].concat(),
        )
        .unwrap();

    engine.call_win64(CODE, [0; 6]).unwrap();

    let actual = engine.unicorn.reg_read_long(RegisterX86::YMM0).unwrap();
    assert_eq!(&actual[..16], lanes([1.0, 20.0]));
    assert_eq!(&actual[16..], [0; 16]);
    assert!(engine.unicorn.get_data().avx_defined_ymm[0]);
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 1);
}

#[test]
fn avx_fallback_extracts_either_128_bit_half_to_register_or_memory() {
    const CODE: u64 = 0x1000_0000;
    let source = DATA_BASE;
    let destination = DATA_BASE + 64;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x10, 0x09, // vmovups ymm1,[rcx]
        0xc4, 0xe3, 0x7d, 0x19, 0xc8, 0x01, // vextractf128 xmm0,ymm1,1
        0xc4, 0xe3, 0x7d, 0x19, 0x0a, 0x00, // vextractf128 [rdx],ymm1,0
        0xc3,
    ]);
    let expected = std::array::from_fn::<_, 32, _>(|index| index as u8);
    engine.write(source, &expected).unwrap();

    engine
        .call_win64(CODE, [source, destination, 0, 0, 0, 0])
        .unwrap();

    let register = engine.unicorn.reg_read_long(RegisterX86::YMM0).unwrap();
    assert_eq!(&register[..16], &expected[16..]);
    assert_eq!(&register[16..], [0; 16]);
    let mut memory = [0u8; 16];
    engine.unicorn.mem_read(destination, &mut memory).unwrap();
    assert_eq!(memory, expected[..16]);
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 3);
}

#[test]
fn avx_fallback_moves_unaligned_integer_vectors_used_by_fractal_noise() {
    const CODE: u64 = 0x1000_0000;
    let source = DATA_BASE;
    let destination = DATA_BASE + 65;
    let mut engine = test_engine(&[
        0xc5, 0xfe, 0x6f, 0x09, // vmovdqu ymm1,[rcx]
        0x48, 0x8d, 0x6a, 0x29, // lea rbp,[rdx+0x29]
        0xc5, 0xfe, 0x7f, 0x4d, 0xd7, // vmovdqu [rbp-0x29],ymm1
        0xc3,
    ]);
    let expected =
        std::array::from_fn::<_, 32, _>(|index| (index as u8).wrapping_mul(13).wrapping_add(7));
    engine.write(source, &expected).unwrap();

    engine
        .call_win64(CODE, [source, destination, 0, 0, 0, 0])
        .unwrap();

    let mut actual = [0u8; 32];
    engine.unicorn.mem_read(destination, &mut actual).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 2);
}

#[test]
fn avx_fallback_preserves_an_aliased_extract_source_until_emulation() {
    const CODE: u64 = 0x1000_0000;
    let source = DATA_BASE;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x10, 0x09, // vmovups ymm1,[rcx]
        0xc4, 0xe3, 0x7d, 0x19, 0xc9, 0x01, // vextractf128 xmm1,ymm1,1
        0xc3,
    ]);
    let expected = std::array::from_fn::<_, 32, _>(|index| index as u8);
    engine.write(source, &expected).unwrap();

    engine.call_win64(CODE, [source, 0, 0, 0, 0, 0]).unwrap();

    let register = engine.unicorn.reg_read_long(RegisterX86::YMM1).unwrap();
    assert_eq!(&register[..16], &expected[16..]);
    assert_eq!(&register[16..], [0; 16]);
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 2);
}

#[test]
fn avx_fallback_keeps_unimplemented_instructions_fail_closed() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x58, 0xc0, // vaddps ymm0,ymm0,ymm0
        0xc3,
    ]);

    let error = engine.call_win64(CODE, [0; 6]).unwrap_err();

    assert!(
        error
            .to_string()
            .to_ascii_lowercase()
            .contains("invalid instruction"),
        "{error}"
    );
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 0);
}

#[test]
fn avx_fallback_limit_stops_before_mutating_the_destination() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x10, 0x00, // vmovups ymm0,[rax]
        0xc3,
    ]);
    engine
        .unicorn
        .reg_write(RegisterX86::RAX, DATA_BASE)
        .unwrap();
    engine.unicorn.get_data_mut().avx_fallback_instructions = MAX_AVX_FALLBACK_INSTRUCTIONS;
    let before = engine.unicorn.reg_read_long(RegisterX86::YMM0).unwrap();

    engine.unicorn.emu_start(CODE, CODE + 4, 0, 1).unwrap();

    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .unwrap()
            .contains("AVX fallback instruction limit exceeded"),
    );
    assert_eq!(
        engine
            .unicorn
            .reg_read_long(RegisterX86::YMM0)
            .unwrap()
            .as_ref(),
        before.as_ref()
    );
}

#[test]
fn avx_fallback_budget_and_defined_registers_reset_per_dispatch() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.get_data_mut().avx_fallback_instructions = MAX_AVX_FALLBACK_INSTRUCTIONS;
    engine.unicorn.get_data_mut().avx_defined_ymm[0] = true;

    engine.call_win64(CODE, [0; 6]).unwrap();

    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 0);
    assert_eq!(engine.unicorn.get_data().avx_defined_ymm, [false; 16]);
}

#[test]
fn avx_fallback_synchronizes_native_vex_state_across_scalar_instructions() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xc5, 0xf9, 0xef, 0xc0, // vpxor xmm0,xmm0,xmm0
        0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax,1
        0xc5, 0xfc, 0x11, 0x01, // vmovups [rcx],ymm0
        0xc3,
    ]);
    engine
        .unicorn
        .reg_write_long(RegisterX86::YMM0, &[0x5a; 32])
        .unwrap();

    engine.call_win64(CODE, [DATA_BASE, 0, 0, 0, 0, 0]).unwrap();

    let mut destination = [0u8; 32];
    engine
        .unicorn
        .mem_read(DATA_BASE, &mut destination)
        .unwrap();
    assert_eq!(destination, [0; 32]);
    assert!(engine.unicorn.get_data().avx_defined_ymm[0]);
}

#[test]
fn avx_fallback_synchronizes_every_executed_vex128_register_write() {
    const CODE: u64 = 0x1000_0000;
    let source = DATA_BASE;
    let destination = DATA_BASE + 64;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x10, 0x01, // vmovups ymm0,[rcx]
        0xc5, 0xf8, 0x58, 0xc0, // vaddps xmm0,xmm0,xmm0
        0xc5, 0xfc, 0x11, 0x02, // vmovups [rdx],ymm0
        0xc3,
    ]);
    let mut input = [0x5a; 32];
    for (lane, value) in [1.0f32, 2.0, 3.0, 4.0].into_iter().enumerate() {
        input[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    engine.write(source, &input).unwrap();

    engine
        .call_win64(CODE, [source, destination, 0, 0, 0, 0])
        .unwrap();

    let mut output = [0u8; 32];
    engine.unicorn.mem_read(destination, &mut output).unwrap();
    for (lane, expected) in [2.0f32, 4.0, 6.0, 8.0].into_iter().enumerate() {
        assert_eq!(&output[lane * 4..lane * 4 + 4], &expected.to_le_bytes());
    }
    assert_eq!(&output[16..], [0; 16]);
    assert!(engine.unicorn.get_data().avx_defined_ymm[0]);
}

#[test]
fn avx_state_sync_discovers_vex_at_a_non_linear_entry_boundary() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xeb, 0x01, // jmp over inline byte
        0x04, // inline data that changes a linear decoder's boundary
        0xc5, 0xf9, 0xef, 0xc0, // vpxor xmm0,xmm0,xmm0
        0xc5, 0xfc, 0x11, 0x01, // vmovups [rcx],ymm0
        0xc3,
    ]);
    engine
        .unicorn
        .reg_write_long(RegisterX86::YMM0, &[0x5a; 32])
        .unwrap();

    engine.call_win64(CODE, [DATA_BASE, 0, 0, 0, 0, 0]).unwrap();

    let mut output = [0u8; 32];
    engine.unicorn.mem_read(DATA_BASE, &mut output).unwrap();
    assert_eq!(output, [0; 32]);
}

#[test]
fn avx_state_sync_applies_vzeroupper_and_vzeroall_to_all_registers() {
    const CODE: u64 = 0x1000_0000;
    for (code, zero_all) in [
        (&[0xc5, 0xf8, 0x77, 0xc3][..], false), // vzeroupper; ret
        (&[0xc5, 0xfc, 0x77, 0xc3][..], true),  // vzeroall; ret
    ] {
        let mut engine = test_engine(code);
        for index in 0..16 {
            engine
                .unicorn
                .reg_write_long(unicorn_ymm_register(index).unwrap(), &[index as u8 + 1; 32])
                .unwrap();
        }

        engine.call_win64(CODE, [0; 6]).unwrap();

        for index in 0..16 {
            let value = engine
                .unicorn
                .reg_read_long(unicorn_ymm_register(index).unwrap())
                .unwrap();
            if zero_all {
                assert_eq!(&value[..], [0; 32], "YMM{index}");
            } else {
                assert_eq!(&value[..16], [index as u8 + 1; 16], "YMM{index}");
                assert_eq!(&value[16..], [0; 16], "YMM{index}");
            }
        }
        assert_eq!(engine.unicorn.get_data().avx_defined_ymm, [true; 16]);
    }
}

#[test]
fn avx_fallback_rejects_undefined_ymm_register_sources() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[
        0xc5, 0xfc, 0x11, 0x01, // vmovups [rcx],ymm0
        0xc4, 0xe3, 0x7d, 0x19, 0xc8, 0x01, // vextractf128 xmm0,ymm1,1
        0xc3,
    ]);

    let error = engine
        .call_win64(CODE, [DATA_BASE, 0, 0, 0, 0, 0])
        .unwrap_err();

    assert!(
        error
            .to_string()
            .to_ascii_lowercase()
            .contains("invalid instruction"),
        "{error}"
    );
    assert_eq!(engine.unicorn.get_data().avx_fallback_instructions, 0);
}

#[test]
fn avx_fallback_rejects_undefined_extract_and_unbounded_blend_width() {
    const CODE: u64 = 0x1000_0000;
    let mut undefined_extract = test_engine(&[
        0xc4, 0xe3, 0x7d, 0x19, 0xc8, 0x01, // vextractf128 xmm0,ymm1,1
        0xc3,
    ]);
    let extract_error = undefined_extract.call_win64(CODE, [0; 6]).unwrap_err();
    assert!(
        extract_error
            .to_string()
            .to_ascii_lowercase()
            .contains("invalid instruction"),
        "{extract_error}"
    );

    let mut wide_blend = test_engine(&[
        0xc4, 0xe3, 0x7d, 0x4b, 0xc2, 0x30, // vblendvpd ymm0,ymm0,ymm2,ymm3
        0xc3,
    ]);
    let blend_error = wide_blend.call_win64(CODE, [0; 6]).unwrap_err();
    assert!(
        blend_error
            .to_string()
            .to_ascii_lowercase()
            .contains("invalid instruction"),
        "{blend_error}"
    );
}

#[test]
fn avx_state_sync_hook_count_is_bounded() {
    const CODE: u64 = 0x1000_0000;
    let mut code = Vec::with_capacity((MAX_AVX_STATE_SYNC_POINTS + 1) * 4);
    for _ in 0..=MAX_AVX_STATE_SYNC_POINTS {
        code.extend_from_slice(&[0xc5, 0xf8, 0x58, 0xc0]); // vaddps xmm0,xmm0,xmm0
    }
    let discovery_error = discover_avx_state_sync_points(&code, CODE).unwrap_err();
    assert!(matches!(
        &discovery_error,
        GuestError::AvxStateCapacity {
            observed,
            limit: MAX_AVX_STATE_SYNC_POINTS
        } if *observed == MAX_AVX_STATE_SYNC_POINTS + 1
    ));
    assert!(
        discovery_error
            .to_string()
            .contains("native AVX state sync point capacity exceeded"),
        "{discovery_error}"
    );

    let mut engine = test_engine(&[0xc3]);
    let error = install_avx_state_sync_points(
        &mut engine.unicorn,
        (0..=MAX_AVX_STATE_SYNC_POINTS)
            .map(|offset| (CODE + offset as u64, AvxStateSync::RegisterUpper(0)))
            .collect(),
    )
    .unwrap_err();

    assert!(matches!(
        &error,
        GuestError::AvxStateCapacity {
            observed,
            limit: MAX_AVX_STATE_SYNC_POINTS
        } if *observed == MAX_AVX_STATE_SYNC_POINTS + 1
    ));
    assert!(
        error
            .to_string()
            .contains("native AVX state sync point capacity exceeded"),
        "{error}"
    );
}

#[test]
fn avx_state_sync_accepts_the_exact_hard_limit_with_one_dense_hook() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xc3]);
    let points = (0..MAX_AVX_STATE_SYNC_POINTS)
        .map(|offset| (CODE + offset as u64, AvxStateSync::RegisterUpper(0)))
        .collect();

    install_avx_state_sync_points(&mut engine.unicorn, points).unwrap();

    assert_eq!(
        engine.unicorn.get_data().avx_state_sync_points.len(),
        MAX_AVX_STATE_SYNC_POINTS
    );
}

#[test]
fn duplicate_avx_sync_points_do_not_consume_the_dense_budget() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xc3]);

    install_avx_state_sync_points(
        &mut engine.unicorn,
        std::iter::repeat_n(
            (CODE, AvxStateSync::RegisterUpper(0)),
            MAX_SPARSE_AVX_STATE_SYNC_HOOKS + 1,
        )
        .collect(),
    )
    .unwrap();

    assert!(engine.unicorn.get_data().avx_state_sync_points.is_empty());
}

#[test]
fn dense_avx_sync_map_preserves_vex128_upper_zeroing() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0x90, 0xc3]);
    engine
        .unicorn
        .mem_write(
            CODE,
            &[
                0xc5, 0xf9, 0xef, 0xc0, // vpxor xmm0,xmm0,xmm0
                0xc5, 0xfc, 0x11, 0x01, // vmovups [rcx],ymm0
                0xc3,
            ],
        )
        .unwrap();
    let points = (0..=MAX_SPARSE_AVX_STATE_SYNC_HOOKS)
        .map(|offset| (CODE + offset as u64, AvxStateSync::RegisterUpper(0)))
        .collect::<Vec<_>>();
    install_avx_state_sync_points(&mut engine.unicorn, points).unwrap();
    engine
        .unicorn
        .reg_write_long(RegisterX86::YMM0, &[0x5a; 32])
        .unwrap();

    engine.call_win64(CODE, [DATA_BASE, 0, 0, 0, 0, 0]).unwrap();

    let mut output = [0xff; 32];
    engine.unicorn.mem_read(DATA_BASE, &mut output).unwrap();
    assert_eq!(output, [0; 32]);
    assert_eq!(
        engine.unicorn.get_data().avx_state_sync_points.len(),
        MAX_SPARSE_AVX_STATE_SYNC_HOOKS + 1
    );
}

#[test]
fn memory_witness_reports_unmapped_and_oversized_reads() {
    const CODE: u64 = 0x1000_0000;
    let engine = test_engine(&[0xc3]);
    let unmapped = trace_memory_snapshot(&engine.unicorn, 0xdead_beef, 16, CODE, CODE + PAGE_SIZE);
    assert_eq!(unmapped.status, "unmapped");
    assert!(unmapped.sha256.is_none());
    let oversized = trace_memory_snapshot(
        &engine.unicorn,
        DATA_BASE,
        MAX_TRACE_WATCH_BYTES + 1,
        CODE,
        CODE + PAGE_SIZE,
    );
    assert_eq!(oversized.status, "oversized");
    assert!(oversized.hex.is_none());
}

#[test]
fn call_envelope_decodes_xmm_stack_and_return_values() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xc3]);
    let xmm_bytes = [
        1.5f32.to_le_bytes(),
        (-2.25f32).to_le_bytes(),
        3.0f32.to_le_bytes(),
        4.5f32.to_le_bytes(),
    ]
    .concat();
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM0, &xmm_bytes)
        .unwrap();
    let rsp = STACK_BASE + 0x1000;
    engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x20, &0x1122_3344_5566_7788u64.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RAX, 0xaabb_ccdd)
        .unwrap();

    let xmm = trace_xmm_arguments(&engine.unicorn);
    assert_eq!(xmm[0].register, "xmm0");
    assert_eq!(xmm[0].f32_lanes[0], Some(1.5));
    assert_eq!(xmm[0].f32_lanes[1], Some(-2.25));
    let stack = trace_stack_arguments(&engine.unicorn, rsp, 0x20, CODE, CODE + PAGE_SIZE);
    assert_eq!(stack[0].index, 5);
    assert_eq!(stack[0].value.raw, 0x1122_3344_5566_7788);
    let returned = trace_return_value(&engine.unicorn, CODE, CODE + PAGE_SIZE);
    assert_eq!(returned.rax.raw, 0xaabb_ccdd);
    assert_eq!(returned.xmm0.f32_lanes[2], Some(3.0));
}

#[test]
fn numeric_ranges_include_f64_xmm_lanes() {
    let xmm = TraceXmmValue {
        register: "xmm1",
        raw_hex: String::new(),
        f32_lanes: Vec::new(),
        f64_lanes: vec![Some(1.25), Some(-3.5)],
    };
    let returned = TraceReturnValue {
        rax: TraceValue {
            raw: 0,
            classification: "integer",
            offset: None,
        },
        xmm0: TraceXmmValue {
            register: "xmm0",
            raw_hex: String::new(),
            f32_lanes: Vec::new(),
            f64_lanes: vec![Some(9.75)],
        },
    };
    let observation = TraceObservation {
        observation: 1,
        fingerprint: "0000000000000001".into(),
        call_id: None,
        arguments: Vec::new(),
        xmm_arguments: vec![xmm],
        stack_arguments: Vec::new(),
        return_value: Some(returned),
    };
    let mut exemplars = TraceExemplars::default();

    update_numeric_ranges(&mut exemplars, &observation);

    assert!(exemplars.numeric_ranges.iter().any(|range| {
        range.field == "xmm1.f64[1]" && range.minimum == -3.5 && range.maximum == -3.5
    }));
    assert!(exemplars.numeric_ranges.iter().any(|range| {
        range.field == "xmm0.f64[0]" && range.minimum == 9.75 && range.maximum == 9.75
    }));
}

#[test]
fn trace_event_limit_marks_capture_as_truncated() {
    let mut capture = TraceCapture {
        selector: "GLOBAL_SETUP".into(),
        entry_rva: 0,
        events: Vec::new(),
        return_stack: Vec::new(),
        function_stack: Vec::new(),
        call_rsp_stack: Vec::new(),
        call_id_stack: Vec::new(),
        next_call_id: 1,
        watch_specs: Vec::new(),
        watch_occurrence_counts: HashMap::new(),
        watch_stack: Vec::new(),
        selector_watches: Vec::new(),
        checkpoint_returns: HashMap::new(),
        unhookable_watches: Vec::new(),
        witnesses: Vec::new(),
        dropped_witnesses: 0,
        basic_blocks: HashMap::new(),
        branch_edges: HashMap::new(),
        dropped_basic_blocks: 0,
        dropped_branch_edges: 0,
        previous_block: None,
        event_index: HashMap::new(),
        event_fingerprints: HashMap::new(),
        known_function_entries: HashSet::new(),
        truncated: false,
        dropped_events: 0,
        checkpoint_only: false,
    };
    for index in 0..=MAX_TRACE_EVENTS {
        push_trace_event(
            &mut capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "guest_call",
                call_id: Some(index as u64 + 1),
                function_rva: None,
                pc_rva: Some(index as u64),
                target_rva: None,
                name: None,
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            },
        );
    }
    assert_eq!(capture.events.len(), MAX_TRACE_EVENTS);
    assert!(capture.truncated);
}

#[test]
fn win64_call_rejects_execution_that_does_not_reach_return_sentinel() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xf4]); // hlt
    let error = engine.call_win64(CODE, [0; 6]).unwrap_err().to_string();
    assert!(error.contains("before the guest returned"), "{error}");
}

#[test]
fn win64_call_timeout_still_fails_closed() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xeb, 0xfe]); // jmp $
    let error = engine
        .call_win64_with_timeout(CODE, &[0; 6], 1_000)
        .unwrap_err();
    assert!(
        error.to_string().contains("before the guest returned"),
        "{error}"
    );
    let diagnostic_snapshot = error
        .crash_snapshot()
        .expect("resident diagnostics retain the crash snapshot");
    assert_eq!(diagnostic_snapshot["instruction_rva"], 0);
    assert_eq!(
        diagnostic_snapshot["registers"].as_object().unwrap().len(),
        18
    );
    let GuestError::ExecutionCrash { snapshot, .. } = error else {
        panic!("expected structured crash snapshot");
    };
    assert_eq!(snapshot.registers.len(), 18);
    assert_eq!(snapshot.xmm_registers.len(), 16);
    assert_eq!(snapshot.instruction_rva, Some(0));
    assert!(!snapshot.instruction_bytes.is_empty());
}

#[test]
fn crash_snapshot_captures_null_register_call_provenance() {
    const CODE: u64 = 0x1000_0000;
    // xor eax,eax; call rax; ret
    let mut engine = test_engine(&[0x31, 0xc0, 0xff, 0xd0, 0xc3]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let error = engine.call_win64(CODE, [0; 6]).unwrap_err();
    let GuestError::ExecutionCrash { snapshot, .. } = error else {
        panic!("expected structured crash snapshot");
    };
    let target = snapshot.runtime_target.expect("runtime target provenance");
    assert_eq!(target.source_rva, Some(2));
    assert_eq!(target.transfer_kind, "call");
    assert_eq!(target.operand_kind, "register");
    assert_eq!(target.effective_target, Some(0));
    assert_eq!(target.target.kind, "null");
    let register = target.register.expect("register provenance");
    assert_eq!(register.name, "rax");
    assert_eq!(register.value, 0);
    assert!(target.memory.is_none());
}

#[test]
fn crash_snapshot_captures_memory_indirect_nonexec_target_provenance() {
    const CODE: u64 = 0x1000_0000;
    // call qword ptr [rip+2]; ret; pad; qword target
    let mut code = vec![0xff, 0x15, 0x02, 0, 0, 0, 0xc3, 0x90];
    code.extend_from_slice(&DATA_BASE.to_le_bytes());
    let mut engine = test_engine(&code);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let error = engine.call_win64(CODE, [0; 6]).unwrap_err();
    let GuestError::ExecutionCrash { snapshot, .. } = error else {
        panic!("expected structured crash snapshot");
    };
    let target = snapshot.runtime_target.expect("runtime target provenance");
    assert_eq!(target.source_rva, Some(0));
    assert_eq!(target.operand_kind, "memory");
    assert_eq!(target.effective_target, Some(DATA_BASE));
    assert_eq!(target.target.kind, "other_mapped");
    let memory = target.memory.expect("memory provenance");
    assert_eq!(memory.address, Some(CODE + 8));
    assert_eq!(memory.base_register.as_deref(), Some("rip"));
    assert_eq!(memory.dereferenced_target, Some(DATA_BASE));
}

#[test]
fn crash_snapshot_marks_external_register_jump_as_tail_call() {
    const CODE: u64 = 0x1000_0000;
    // xor eax,eax; jmp rax
    let mut engine = test_engine(&[0x31, 0xc0, 0xff, 0xe0]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let error = engine.call_win64(CODE, [0; 6]).unwrap_err();
    let GuestError::ExecutionCrash { snapshot, .. } = error else {
        panic!("expected structured crash snapshot");
    };
    let target = snapshot.runtime_target.expect("runtime target provenance");
    assert_eq!(target.source_rva, Some(2));
    assert_eq!(target.transfer_kind, "tail_call");
    assert_eq!(target.operand_kind, "register");
    assert_eq!(target.target.kind, "null");
}

#[test]
fn crash_snapshot_omits_stale_runtime_target_after_completed_call() {
    const CODE: u64 = 0x1000_0000;
    // call the ret at +7; then fail later on ud2 at +5.
    let mut engine = test_engine(&[0xe8, 2, 0, 0, 0, 0x0f, 0x0b, 0xc3]);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let error = engine.call_win64(CODE, [0; 6]).unwrap_err();
    let GuestError::ExecutionCrash { snapshot, .. } = error else {
        panic!("expected structured crash snapshot");
    };
    assert_eq!(snapshot.instruction_rva, Some(5));
    assert!(
        snapshot.runtime_target.is_none(),
        "completed call must not be reported as the cause of a later fault"
    );
}

#[test]
fn crash_snapshot_does_not_reuse_call_provenance_after_conditional_reentry() {
    const CODE: u64 = 0x1000_0000;
    // call T with rcx=0; set rcx=1; conditionally re-enter T. The first
    // visit returns, while the second reaches an unrelated ud2.
    let code = [
        0xe8, 0x0b, 0, 0, 0, // call T (+16)
        0xb9, 1, 0, 0, 0, // mov ecx,1
        0x85, 0xc9, // test ecx,ecx
        0x75, 0x02, // jne T
        0x0f, 0x0b, // unreachable ud2
        0x85, 0xc9, // T: test ecx,ecx
        0x75, 0x01, // jne fault
        0xc3, // ret
        0x0f, 0x0b, // fault: ud2
    ];
    let mut engine = test_engine(&code);
    engine.begin_execution_trace("RENDER", CODE).unwrap();
    let error = engine.call_win64(CODE, [0; 6]).unwrap_err();
    let GuestError::ExecutionCrash { snapshot, .. } = error else {
        panic!("expected structured crash snapshot");
    };
    assert_eq!(snapshot.instruction_rva, Some(21));
    assert!(
        snapshot.runtime_target.is_none(),
        "the completed call to T must expire before later re-entry"
    );
}

#[test]
fn runtime_target_classification_distinguishes_image_permissions_and_labels() {
    const CODE: u64 = 0x1000_0000;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        classify_runtime_target(&engine.unicorn, Some(CODE)).kind,
        "image_executable"
    );
    assert_eq!(
        classify_runtime_target(&engine.unicorn, Some(CODE + 0x100)).kind,
        "image_nonexec"
    );
    engine.unicorn.get_data_mut().trace_labels.insert(
        STUB_BASE,
        TraceLabel {
            kind: TraceLabelKind::Import,
            name: "fixture!entry".into(),
        },
    );
    let import = classify_runtime_target(&engine.unicorn, Some(STUB_BASE));
    assert_eq!(import.kind, "import");
    assert_eq!(import.name.as_deref(), Some("fixture!entry"));
    engine.unicorn.get_data_mut().trace_labels.insert(
        HOST_NOOP,
        TraceLabel {
            kind: TraceLabelKind::HostCallback,
            name: "noop".into(),
        },
    );
    assert_eq!(
        classify_runtime_target(&engine.unicorn, Some(HOST_NOOP)).kind,
        "callback"
    );
    assert_eq!(
        classify_runtime_target(&engine.unicorn, Some(HOST_AEGP_UTILITY_TABLES)).kind,
        "suite"
    );
    assert_eq!(
        classify_runtime_target(&engine.unicorn, Some(0xdead_beef)).kind,
        "unmapped"
    );
}

#[test]
fn block_census_is_opt_in_and_counts_repeated_guest_work() {
    const CODE: u64 = 0x1000_0000;
    // mov ecx,10; dec ecx; jne -4; ret
    let mut engine = test_engine(&[0xb9, 10, 0, 0, 0, 0xff, 0xc9, 0x75, 0xfc, 0xc3]);
    engine.begin_block_census().unwrap();
    engine.call_win64(CODE, [0; 6]).unwrap();
    let census = engine.finish_block_census(1).unwrap();
    assert!(census.total_block_executions >= 10);
    assert!(census.estimated_dynamic_instructions >= 20);
    assert_eq!(census.output_pixels, 1);
    assert_eq!(
        census.estimated_dynamic_instructions_per_pixel,
        census.estimated_dynamic_instructions as f64
    );
    assert!(!census.extents.is_empty());
    assert!(census.top_1_extent_dynamic_instruction_fraction > 0.0);
}

#[test]
fn census_extents_merge_overlapping_translation_block_variants() {
    let block = |address, size_bytes, dynamic_instructions| CensusBlock {
        address,
        rva: address - 0x1000,
        size_bytes,
        executions: 1,
        instructions: dynamic_instructions as u32,
        dynamic_instructions,
        scalar_sse_fp_instructions: 0,
        dynamic_scalar_sse_fp_instructions: 0,
    };
    let extents = coalesce_census_extents(
        &[
            block(0x1010, 8, 20),
            block(0x1014, 8, 30),
            block(0x1020, 4, 60),
        ],
        0x1000,
        110,
    );
    assert_eq!(extents.len(), 2);
    assert_eq!(extents[0].start_rva, 0x20);
    assert_eq!(extents[0].dynamic_instruction_fraction, 60.0 / 110.0);
    assert_eq!(extents[1].start_rva, 0x10);
    assert_eq!(extents[1].end_rva, 0x1c);
    assert_eq!(extents[1].block_variants, 2);
}
