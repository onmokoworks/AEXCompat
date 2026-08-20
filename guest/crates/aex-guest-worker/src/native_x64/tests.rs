use super::*;

thread_local! {
    static ITERATE_PROGRESS_CALLS: std::cell::RefCell<Vec<(i32, i32)>> = const { std::cell::RefCell::new(Vec::new()) };
    static ITERATE_EVENTS: std::cell::RefCell<Vec<(i32, i32)>> = const { std::cell::RefCell::new(Vec::new()) };
    static ITERATE_PROGRESS_ERROR: Cell<i32> = const { Cell::new(0) };
    static ITERATE_ABORT_CALLS: Cell<u32> = const { Cell::new(0) };
    static ITERATE_ABORT_ERROR: Cell<i32> = const { Cell::new(0) };
    static TYPED_ITERATE_PIXEL_BYTES: Cell<usize> = const { Cell::new(0) };
    static TYPED_ITERATE_PIXEL_ERROR: Cell<i32> = const { Cell::new(0) };
    static TYPED_ITERATE_PIXEL_CALLS: std::cell::RefCell<Vec<(i32, i32, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

unsafe extern "win64" fn test_iterate_pixel(
    _refcon: u64,
    _x: i32,
    _y: i32,
    _input: u64,
    output: u64,
) -> i32 {
    unsafe { ptr::write(output as *mut [u8; 4], [1, 2, 3, 4]) };
    0
}

unsafe extern "win64" fn test_iterate_progress(_effect_ref: u64, current: i32, total: i32) -> i32 {
    ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().push((current, total)));
    ITERATE_EVENTS.with(|events| events.borrow_mut().push((current, total)));
    ITERATE_PROGRESS_ERROR.with(Cell::get)
}

unsafe extern "win64" fn test_iterate_abort(_effect_ref: u64) -> i32 {
    ITERATE_ABORT_CALLS.with(|calls| calls.set(calls.get() + 1));
    ITERATE_EVENTS.with(|events| events.borrow_mut().push((-1, -1)));
    ITERATE_ABORT_ERROR.with(Cell::get)
}

unsafe extern "win64" fn test_typed_iterate_pixel(
    _refcon: u64,
    x: i32,
    y: i32,
    input: u64,
    output: u64,
) -> i32 {
    let pixel_bytes = TYPED_ITERATE_PIXEL_BYTES.with(Cell::get);
    TYPED_ITERATE_PIXEL_CALLS.with(|calls| {
        calls.borrow_mut().push((x, y, input == 0));
    });
    if input == 0 {
        unsafe { ptr::write_bytes(output as *mut u8, 0xa5, pixel_bytes) };
    } else {
        unsafe {
            ptr::copy_nonoverlapping(input as *const u8, output as *mut u8, pixel_bytes);
        }
    }
    TYPED_ITERATE_PIXEL_ERROR.with(Cell::get)
}

fn typed_iterate_test_engine() -> GuestEngine<'static> {
    let image = Mapping::anonymous(ptr::null_mut(), PAGE_SIZE as usize, false).unwrap();
    let arena = Mapping::anonymous(ptr::null_mut(), ARENA_SIZE, false).unwrap();
    let arena_base = arena.pointer as u64;
    let mut engine = GuestEngine {
        image,
        arena,
        state: NativeState {
            arena_next: arena_base,
            arena_end: arena_base + ARENA_SIZE as u64,
            ..NativeState::default()
        },
        loaded_images: loaded_image_snapshot(),
        dllmain_attached: true,
        lifetime: PhantomData,
    };
    engine.install_typed_iterate_suites().unwrap();
    engine
}

fn allocate_typed_world(
    engine: &mut GuestEngine<'static>,
    data: u64,
    rowbytes: usize,
    width: i32,
    height: i32,
) -> u64 {
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut bytes = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    bytes[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8].copy_from_slice(&data.to_le_bytes());
    bytes[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&i32::try_from(rowbytes).unwrap().to_le_bytes());
    bytes[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&width.to_le_bytes());
    bytes[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&height.to_le_bytes());
    engine.write(world, &bytes).unwrap();
    world
}

#[test]
fn iterate_progress_matches_forward_reverse_and_degenerate_contract() {
    let mut arena = vec![0u8; 2048];
    let base = arena.as_mut_ptr() as u64;
    let destination = write_blend_world(&mut arena, 0, base + 1024, 4, 1, 2);
    let mut state = NativeState {
        arena_next: base,
        arena_end: base + arena.len() as u64,
        ..NativeState::default()
    };
    state.worlds.insert(
        destination,
        NativeWorld {
            pixel_format: crate::pixel::PF_PIXEL_FORMAT_ARGB32,
            size: 8,
            data: base + 1024,
            mapping_size: 8,
        },
    );
    let mut in_data = vec![0u8; abi::PF_IN_DATA_SIZE];
    in_data[abi::INTER_PROGRESS_OFFSET..abi::INTER_PROGRESS_OFFSET + 8]
        .copy_from_slice(&callback_address!(test_iterate_progress).to_le_bytes());
    in_data[abi::INTER_ABORT_OFFSET..abi::INTER_ABORT_OFFSET + 8]
        .copy_from_slice(&callback_address!(test_iterate_abort).to_le_bytes());
    ACTIVE_STATE.with(|slot| slot.set(&mut state));

    let run = |input: &[u8], progress_base, progress_final| unsafe {
        iterate_world8(
            input.as_ptr() as u64,
            progress_base,
            progress_final,
            0,
            0,
            0,
            callback_address!(test_iterate_pixel),
            destination,
        )
    };
    ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().clear());
    ITERATE_EVENTS.with(|events| events.borrow_mut().clear());
    ITERATE_ABORT_CALLS.with(|calls| calls.set(0));
    assert_eq!(run(&in_data, 10, 14), 0);
    assert_eq!(
        ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow().clone()),
        [(12, 14), (14, 14)]
    );
    assert_eq!(ITERATE_ABORT_CALLS.with(Cell::get), 1);
    assert_eq!(
        ITERATE_EVENTS.with(|events| events.borrow().clone()),
        [(12, 14), (-1, -1), (14, 14)]
    );

    ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().clear());
    ITERATE_EVENTS.with(|events| events.borrow_mut().clear());
    assert_eq!(run(&in_data, 14, 10), 0);
    assert_eq!(
        ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow().clone()),
        [(2, 4), (4, 4)]
    );

    ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().clear());
    ITERATE_EVENTS.with(|events| events.borrow_mut().clear());
    ITERATE_ABORT_CALLS.with(|calls| calls.set(0));
    assert_eq!(run(&in_data, 0, 0), 0);
    assert!(ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow().is_empty()));
    assert_eq!(ITERATE_ABORT_CALLS.with(Cell::get), 1);
    assert_eq!(
        ITERATE_EVENTS.with(|events| events.borrow().clone()),
        [(-1, -1)]
    );
    assert_eq!(&arena[1024..1032], &[1, 2, 3, 4, 1, 2, 3, 4]);

    ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().clear());
    ITERATE_PROGRESS_ERROR.with(|error| error.set(23));
    assert_eq!(run(&in_data, 10, 14), 23);
    ITERATE_PROGRESS_ERROR.with(|error| error.set(0));

    ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().clear());
    ITERATE_EVENTS.with(|events| events.borrow_mut().clear());
    ITERATE_ABORT_ERROR.with(|error| error.set(29));
    assert_eq!(run(&in_data, 10, 14), 29);
    assert_eq!(
        ITERATE_EVENTS.with(|events| events.borrow().clone()),
        [(12, 14), (-1, -1)]
    );
    ITERATE_ABORT_ERROR.with(|error| error.set(0));

    in_data[abi::INTER_PROGRESS_OFFSET..abi::INTER_PROGRESS_OFFSET + 8]
        .copy_from_slice(&0u64.to_le_bytes());
    ITERATE_ABORT_CALLS.with(|calls| calls.set(0));
    assert_eq!(run(&in_data, i32::MIN, i32::MAX), 0);
    assert_eq!(ITERATE_ABORT_CALLS.with(Cell::get), 1);

    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn typed_iterate_suites_use_native_acquire_abi_stride_and_failure_contracts() {
    type TypedIterate = unsafe extern "win64" fn(u64, i32, i32, u64, u64, u64, u64, u64) -> u64;

    for (name, pixel_bytes, expected_callback) in [
        (
            b"PF iterate16 Suite\0".as_slice(),
            8usize,
            callback_address!(iterate_world16),
        ),
        (
            b"PF iterateFloat Suite\0".as_slice(),
            16usize,
            callback_address!(iterate_world_float),
        ),
    ] {
        let mut engine = typed_iterate_test_engine();
        let name_address = engine.allocate(name.len(), 1).unwrap();
        engine.write(name_address, name).unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(
                    engine.acquire_suite_callback_address(),
                    [name_address, 1, suite_output, 0, 0, 0],
                )
                .unwrap(),
            0
        );
        let mut pointer = [0u8; 8];
        engine.read(suite_output, &mut pointer).unwrap();
        let table = u64::from_le_bytes(pointer);
        engine.read(table, &mut pointer).unwrap();
        let callback = u64::from_le_bytes(pointer);
        assert_eq!(callback, expected_callback);
        let iterate: TypedIterate = unsafe { std::mem::transmute(callback as usize) };

        let width = 2i32;
        let height = 2i32;
        let rowbytes = pixel_bytes * width as usize;
        let byte_count = rowbytes * height as usize;
        let source_data = engine.allocate(byte_count, 16).unwrap();
        let destination_data = engine.allocate(byte_count, 16).unwrap();
        let source_world = allocate_typed_world(&mut engine, source_data, rowbytes, width, height);
        let destination_world =
            allocate_typed_world(&mut engine, destination_data, rowbytes, width, height);
        let source = (1..=byte_count as u8).collect::<Vec<_>>();
        engine.write(source_data, &source).unwrap();

        let in_data = engine.allocate(abi::PF_IN_DATA_SIZE, 8).unwrap();
        engine
            .write_u64(
                in_data + abi::INTER_PROGRESS_OFFSET as u64,
                callback_address!(test_iterate_progress),
            )
            .unwrap();
        engine
            .write_u64(
                in_data + abi::INTER_ABORT_OFFSET as u64,
                callback_address!(test_iterate_abort),
            )
            .unwrap();
        TYPED_ITERATE_PIXEL_BYTES.with(|bytes| bytes.set(pixel_bytes));
        TYPED_ITERATE_PIXEL_ERROR.with(|error| error.set(0));
        TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow_mut().clear());
        ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow_mut().clear());
        ITERATE_ABORT_CALLS.with(|calls| calls.set(0));
        ACTIVE_STATE.with(|slot| slot.set(&mut engine.state));
        assert_eq!(
            unsafe {
                iterate(
                    in_data,
                    10,
                    14,
                    source_world,
                    0,
                    0,
                    callback_address!(test_typed_iterate_pixel),
                    destination_world,
                )
            },
            0
        );
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
        let mut output = vec![0u8; byte_count];
        engine.read(destination_data, &mut output).unwrap();
        assert_eq!(output, source);
        assert_eq!(
            TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow().clone()),
            [(0, 0, false), (1, 0, false), (0, 1, false), (1, 1, false)]
        );
        assert_eq!(
            ITERATE_PROGRESS_CALLS.with(|calls| calls.borrow().clone()),
            [(12, 14), (14, 14)]
        );
        assert_eq!(ITERATE_ABORT_CALLS.with(Cell::get), 1);

        TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow_mut().clear());
        TYPED_ITERATE_PIXEL_ERROR.with(|error| error.set(37));
        ACTIVE_STATE.with(|slot| slot.set(&mut engine.state));
        assert_eq!(
            unsafe {
                iterate(
                    0,
                    0,
                    0,
                    source_world,
                    0,
                    0,
                    callback_address!(test_typed_iterate_pixel),
                    destination_world,
                )
            },
            37
        );
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
        TYPED_ITERATE_PIXEL_ERROR.with(|error| error.set(0));
        assert_eq!(
            TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow().len()),
            1
        );

        engine
            .write(destination_data, &vec![0u8; byte_count])
            .unwrap();
        TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow_mut().clear());
        ACTIVE_STATE.with(|slot| slot.set(&mut engine.state));
        assert_eq!(
            unsafe {
                iterate(
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    callback_address!(test_typed_iterate_pixel),
                    destination_world,
                )
            },
            0
        );
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
        engine.read(destination_data, &mut output).unwrap();
        assert_eq!(output, vec![0xa5; byte_count]);
        assert!(
            TYPED_ITERATE_PIXEL_CALLS
                .with(|calls| calls.borrow().iter().all(|(_, _, input_null)| *input_null))
        );

        for short_world in [source_world, destination_world] {
            engine
                .write(
                    short_world + abi::LAYER_ROWBYTES_OFFSET as u64,
                    &i32::try_from(rowbytes - 1).unwrap().to_le_bytes(),
                )
                .unwrap();
            TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow_mut().clear());
            ACTIVE_STATE.with(|slot| slot.set(&mut engine.state));
            assert_eq!(
                unsafe {
                    iterate(
                        0,
                        0,
                        0,
                        source_world,
                        0,
                        0,
                        callback_address!(test_typed_iterate_pixel),
                        destination_world,
                    )
                },
                4
            );
            ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
            assert!(TYPED_ITERATE_PIXEL_CALLS.with(|calls| calls.borrow().is_empty()));
            engine
                .write(
                    short_world + abi::LAYER_ROWBYTES_OFFSET as u64,
                    &i32::try_from(rowbytes).unwrap().to_le_bytes(),
                )
                .unwrap();
        }
    }
}

#[test]
fn loaded_image_snapshot_is_stable_without_guest_execution() {
    let first = loaded_image_snapshot();
    let second = loaded_image_snapshot();
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

#[test]
fn loaded_image_audit_rejects_every_image_added_after_admission() {
    let baseline = BTreeSet::from(["/usr/lib/libSystem.B.dylib".to_string()]);
    let current = BTreeSet::from([
        "/tmp/untrusted.dylib".to_string(),
        "/usr/lib/libSystem.B.dylib".to_string(),
    ]);
    assert_eq!(
        unexpected_loaded_images(&baseline, &current),
        ["/tmp/untrusted.dylib"]
    );
}

fn write_blend_world(
    arena: &mut [u8],
    descriptor_offset: usize,
    data: u64,
    rowbytes: i32,
    width: i32,
    height: i32,
) -> u64 {
    let descriptor = descriptor_offset;
    arena[descriptor + abi::LAYER_DATA_OFFSET..descriptor + abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&data.to_le_bytes());
    arena[descriptor + abi::LAYER_ROWBYTES_OFFSET..descriptor + abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&rowbytes.to_le_bytes());
    arena[descriptor + abi::LAYER_WIDTH_OFFSET..descriptor + abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&width.to_le_bytes());
    arena[descriptor + abi::LAYER_HEIGHT_OFFSET..descriptor + abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&height.to_le_bytes());
    arena.as_ptr() as u64 + descriptor_offset as u64
}

#[test]
fn blend_world_matches_windows_formats_and_is_alias_safe() {
    for (pixel_format, pixel_bytes) in [
        (crate::pixel::PF_PIXEL_FORMAT_ARGB32, 4usize),
        (crate::pixel::PF_PIXEL_FORMAT_ARGB64, 8usize),
        (crate::pixel::PF_PIXEL_FORMAT_ARGB128, 16usize),
    ] {
        let mut arena = vec![0u8; 4096];
        let base = arena.as_mut_ptr() as u64;
        let first_data = base + 1024;
        let second_data = base + 1152;
        let destination_data = base + 1280;
        let first = write_blend_world(&mut arena, 0, first_data, (2 * pixel_bytes) as i32, 2, 1);
        let second =
            write_blend_world(&mut arena, 256, second_data, (2 * pixel_bytes) as i32, 2, 1);
        let destination = write_blend_world(
            &mut arena,
            512,
            destination_data,
            (2 * pixel_bytes) as i32,
            2,
            1,
        );
        let (first_bytes, second_bytes, expected) = match pixel_format {
            crate::pixel::PF_PIXEL_FORMAT_ARGB32 => (
                vec![255, 10, 20, 30, 128, 40, 50, 60],
                vec![0, 110, 120, 130, 64, 140, 150, 160],
                vec![128, 60, 70, 80, 96, 90, 100, 110],
            ),
            crate::pixel::PF_PIXEL_FORMAT_ARGB64 => {
                let encode = |values: [u16; 8]| {
                    values
                        .into_iter()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                };
                (
                    encode([32768, 1000, 2000, 3000, 16384, 4000, 5000, 6000]),
                    encode([0, 11000, 12000, 13000, 8192, 14000, 15000, 16000]),
                    encode([16384, 6000, 7000, 8000, 12288, 9000, 10000, 11000]),
                )
            }
            crate::pixel::PF_PIXEL_FORMAT_ARGB128 => {
                let encode = |values: [f32; 8]| {
                    values
                        .into_iter()
                        .flat_map(f32::to_le_bytes)
                        .collect::<Vec<_>>()
                };
                let first = [1.0, 0.1, 0.2, 0.3, 0.5, 0.4, 0.5, 0.6];
                let second = [0.0, 1.1, 1.2, 1.3, 0.25, 1.4, 1.5, 1.6];
                let mut blended = [0.0f32; 8];
                for index in 0..blended.len() {
                    blended[index] =
                        (f64::from(first[index]) * 0.5 + f64::from(second[index]) * 0.5) as f32;
                }
                (encode(first), encode(second), encode(blended))
            }
            _ => unreachable!(),
        };
        arena[1024..1024 + first_bytes.len()].copy_from_slice(&first_bytes);
        arena[1152..1152 + second_bytes.len()].copy_from_slice(&second_bytes);
        let mut state = NativeState {
            arena_next: base,
            arena_end: base + arena.len() as u64,
            ..NativeState::default()
        };
        for (world, data) in [
            (first, first_data),
            (second, second_data),
            (destination, destination_data),
        ] {
            state.worlds.insert(
                world,
                NativeWorld {
                    pixel_format,
                    size: (2 * pixel_bytes) as u64,
                    data,
                    mapping_size: 2 * pixel_bytes,
                },
            );
        }
        ACTIVE_STATE.with(|slot| slot.set(&mut state));

        assert_eq!(
            unsafe { blend_world(1, first, second, 32_768, destination, 0) },
            0
        );
        assert_eq!(&arena[1280..1280 + expected.len()], expected.as_slice());
        arena[1024..1024 + first_bytes.len()].copy_from_slice(&first_bytes);
        assert_eq!(
            unsafe { blend_world(1, first, second, 32_768, first, 0) },
            0
        );
        assert_eq!(&arena[1024..1024 + expected.len()], expected.as_slice());
        arena[1024..1024 + first_bytes.len()].copy_from_slice(&first_bytes);
        arena[1152..1152 + second_bytes.len()].copy_from_slice(&second_bytes);
        assert_eq!(
            unsafe { blend_world(1, first, second, 32_768, second, 0) },
            0
        );
        assert_eq!(&arena[1152..1152 + expected.len()], expected.as_slice());
        ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    }
}

#[test]
fn blend_world_rejects_invalid_ratio_and_mismatch_without_writes() {
    let mut arena = vec![0u8; 2048];
    let base = arena.as_mut_ptr() as u64;
    let first = write_blend_world(&mut arena, 0, base + 1024, 4, 1, 1);
    let second = write_blend_world(&mut arena, 256, base + 1088, 4, 1, 1);
    let destination = write_blend_world(&mut arena, 512, base + 1152, 4, 1, 1);
    arena[1024..1028].copy_from_slice(&[255, 1, 2, 3]);
    arena[1088..1092].copy_from_slice(&[0, 4, 5, 6]);
    arena[1152..1156].copy_from_slice(&[0x5a; 4]);
    let mut state = NativeState {
        arena_next: base,
        arena_end: base + arena.len() as u64,
        ..NativeState::default()
    };
    ACTIVE_STATE.with(|slot| slot.set(&mut state));

    assert_eq!(
        unsafe { blend_world(1, first, second, 65_537, destination, 0) },
        4
    );
    assert_eq!(&arena[1152..1156], &[0x5a; 4]);
    arena[256 + abi::LAYER_WIDTH_OFFSET..256 + abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    assert_eq!(
        unsafe { blend_world(1, first, second, 32_768, destination, 0) },
        4
    );
    assert_eq!(&arena[1152..1156], &[0x5a; 4]);
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn ansi_sprintf_copies_bounded_literals_and_rejects_conversions() {
    let mut state = NativeState::default();
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    let literal = b"Not able to acquire AEFX Suite.\0";
    let escaped = b"progress 100%%\0";
    let conversion = b"value=%d\0";
    let mut output = [0x5au8; 64];
    state.image_start = literal
        .as_ptr()
        .min(escaped.as_ptr())
        .min(conversion.as_ptr()) as u64;
    state.image_end = (literal.as_ptr() as u64 + literal.len() as u64)
        .max(escaped.as_ptr() as u64 + escaped.len() as u64)
        .max(conversion.as_ptr() as u64 + conversion.len() as u64);

    assert_eq!(
        unsafe {
            native_ansi_sprintf(
                output.as_mut_ptr() as u64,
                literal.as_ptr() as u64,
                0,
                0,
                0,
                0,
            )
        },
        (literal.len() - 1) as u64
    );
    assert_eq!(&output[..literal.len()], literal);
    output.fill(0x5a);
    assert_eq!(
        unsafe {
            native_ansi_sprintf(
                output.as_mut_ptr() as u64,
                escaped.as_ptr() as u64,
                0,
                0,
                0,
                0,
            )
        },
        13
    );
    assert_eq!(&output[..14], b"progress 100%\0");
    output.fill(0x5a);
    assert_eq!(
        unsafe {
            native_ansi_sprintf(
                output.as_mut_ptr() as u64,
                conversion.as_ptr() as u64,
                7,
                0,
                0,
                0,
            )
        },
        u32::MAX as u64
    );
    assert_eq!(output, [0x5a; 64]);
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn transfer_rect8_applies_mask_world_and_rejects_invalid_mask_without_writes() {
    let mut arena = vec![0u8; 4096];
    let base = arena.as_mut_ptr() as u64;
    let source_world = base;
    let destination_world = base + 256;
    let mask_world = base + 512;
    let source_data = base + 1024;
    let destination_data = base + 1088;
    let mask_data = base + 1152;
    let composite = base + 1216;
    let bounds = base + 1248;
    let write_world = |arena: &mut [u8], offset: usize, data: u64, rowbytes: i32| {
        arena[offset + abi::LAYER_DATA_OFFSET..offset + abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&data.to_le_bytes());
        arena[offset + abi::LAYER_ROWBYTES_OFFSET..offset + abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&rowbytes.to_le_bytes());
        arena[offset + abi::LAYER_WIDTH_OFFSET..offset + abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&3i32.to_le_bytes());
        arena[offset + abi::LAYER_HEIGHT_OFFSET..offset + abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
    };
    write_world(&mut arena, 0, source_data, 12);
    write_world(&mut arena, 256, destination_data, 12);
    write_world(&mut arena, 512, mask_data, 12);
    arena[1024..1036].copy_from_slice(&[255, 200, 0, 0, 255, 200, 0, 0, 255, 200, 0, 0]);
    arena[1152..1164].copy_from_slice(&[0, 0, 0, 0, 128, 128, 128, 128, 255, 255, 255, 255]);
    arena[1216..1220].copy_from_slice(&2i32.to_le_bytes());
    arena[1224] = 255;
    arena[1226..1228].copy_from_slice(&32768u16.to_le_bytes());
    for (index, value) in [0i32, 0, 3, 1].into_iter().enumerate() {
        arena[1248 + index * 4..1252 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut state = NativeState {
        arena_next: base,
        arena_end: base + arena.len() as u64,
        ..NativeState::default()
    };
    ACTIVE_STATE.with(|slot| slot.set(&mut state));

    assert_eq!(
        unsafe {
            transfer_rect8(
                1,
                0,
                0,
                0,
                bounds,
                source_world,
                composite,
                mask_world,
                0,
                0,
                destination_world,
            )
        },
        0
    );
    assert_eq!(
        &arena[1088..1100],
        &[0, 0, 0, 0, 128, 100, 0, 0, 255, 200, 0, 0]
    );

    arena[1088..1100].fill(0);
    arena[512 + abi::PF_LAYER_DEF_SIZE + 8..512 + abi::PF_LAYER_DEF_SIZE + 12]
        .copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        unsafe {
            transfer_rect8(
                1,
                0,
                0,
                0,
                bounds,
                source_world,
                composite,
                mask_world,
                0,
                0,
                destination_world,
            )
        },
        0
    );
    assert_eq!(
        &arena[1088..1100],
        &[255, 200, 0, 0, 127, 100, 0, 0, 0, 0, 0, 0]
    );

    arena[1088..1100].fill(0x5a);
    arena[512 + abi::PF_LAYER_DEF_SIZE + 8..512 + abi::PF_LAYER_DEF_SIZE + 12]
        .copy_from_slice(&4u32.to_le_bytes());
    let before = arena[1088..1100].to_vec();
    assert_eq!(
        unsafe {
            transfer_rect8(
                1,
                0,
                0,
                0,
                bounds,
                source_world,
                composite,
                mask_world,
                0,
                0,
                destination_world,
            )
        },
        PF_BAD_CALLBACK_PARAM
    );
    assert_eq!(&arena[1088..1100], before);

    arena[1088..1100].fill(40);
    arena[1216..1220].copy_from_slice(&0i32.to_le_bytes());
    arena[1224] = 128;
    assert_eq!(
        unsafe {
            transfer_rect8(
                1,
                0,
                0,
                0,
                bounds,
                source_world,
                composite,
                0,
                0,
                0,
                destination_world,
            )
        },
        0
    );
    assert_eq!(arena[1089], 120);
    assert_eq!(arena[1093], 120);
    assert_eq!(arena[1097], 120);

    // Freeze mask coverage before destination writes: the mask begins one
    // pixel before the destination and therefore overlaps pixels 0 and 1.
    arena[512 + abi::LAYER_DATA_OFFSET..512 + abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&(destination_data - abi::PF_PIXEL_SIZE as u64).to_le_bytes());
    arena[512 + abi::PF_LAYER_DEF_SIZE + 8..512 + abi::PF_LAYER_DEF_SIZE + 12]
        .copy_from_slice(&2u32.to_le_bytes());
    arena[1084..1088].copy_from_slice(&[255, 255, 255, 255]);
    arena[1088..1100].copy_from_slice(&[255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
    arena[1216..1220].copy_from_slice(&0i32.to_le_bytes());
    arena[1224] = 255;
    assert_eq!(
        unsafe {
            transfer_rect8(
                1,
                0,
                0,
                0,
                bounds,
                source_world,
                composite,
                mask_world,
                0,
                0,
                destination_world,
            )
        },
        0
    );
    assert_eq!(
        &arena[1088..1100],
        &[255, 200, 0, 0, 255, 200, 0, 0, 0, 0, 0, 0]
    );

    // Extreme guest-provided offsets remain an empty mask intersection
    // instead of overflowing native coordinate arithmetic.
    arena[512 + abi::PF_LAYER_DEF_SIZE..512 + abi::PF_LAYER_DEF_SIZE + 4]
        .copy_from_slice(&i32::MIN.to_le_bytes());
    arena[512 + abi::PF_LAYER_DEF_SIZE + 8..512 + abi::PF_LAYER_DEF_SIZE + 12]
        .copy_from_slice(&0u32.to_le_bytes());
    arena[1088..1100].fill(0x5a);
    assert_eq!(
        unsafe {
            transfer_rect8(
                1,
                0,
                0,
                0,
                bounds,
                source_world,
                composite,
                mask_world,
                0,
                0,
                destination_world,
            )
        },
        0
    );
    assert_eq!(&arena[1088..1100], &[0x5a; 12]);
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn smart_checkout_rejects_other_times_and_tracks_pixel_balance() {
    let mut arena = vec![0u8; 4096];
    let arena_base = arena.as_mut_ptr() as u64;
    let world = arena_base;
    let layer_definition = arena_base + 512;
    let layer_world_offset = 512 + abi::PARAM_U_OFFSET;
    let pixels = arena_base + 1024;
    let result = arena_base + 2048;
    let checked_out_world = arena_base + 2200;
    for offset in [0, layer_world_offset] {
        arena[offset + abi::LAYER_DATA_OFFSET..offset + abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&pixels.to_le_bytes());
        arena[offset + abi::LAYER_ROWBYTES_OFFSET..offset + abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&16i32.to_le_bytes());
        arena[offset + abi::LAYER_WIDTH_OFFSET..offset + abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&4i32.to_le_bytes());
        arena[offset + abi::LAYER_HEIGHT_OFFSET..offset + abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&3i32.to_le_bytes());
    }
    let mut state = NativeState {
        smart_input_world: world,
        smart_pixel_format: crate::pixel::PF_PIXEL_FORMAT_ARGB32,
        smart_current_time: 10,
        smart_current_time_scale: 30,
        arena_next: arena_base,
        arena_end: arena_base + ARENA_SIZE as u64,
        params: vec![GuestParam {
            index: 1,
            param_type: 0,
            name: "Map".into(),
            bytes: vec![0; abi::PF_PARAM_DEF_SIZE],
        }],
        parameter_definitions: vec![layer_definition],
        ..NativeState::default()
    };
    ACTIVE_STATE.with(|slot| slot.set(&mut state));

    assert_eq!(
        unsafe { pre_checkout_layer(0, 0, 6, 0, 11, 0, 30, result) },
        0,
        "primary input permits temporal requests against the current fallback world"
    );
    state.smart_checkout_ids.clear();
    assert_eq!(
        unsafe { pre_checkout_layer(0, 1, 7, 0, 11, 0, 30, result) },
        4
    );
    assert!(
        state
            .callback_error
            .take()
            .is_some_and(|message| message.contains("unsupported temporal smart checkout"))
    );
    assert!(state.smart_checkout_ids.is_empty());

    let status = unsafe { pre_checkout_layer(0, 0, 8, 0, 10, 0, 30, result) };
    assert_eq!(status, 0, "{:?}", state.callback_error);
    assert_eq!(
        unsafe { checkout_layer_pixels(0, 8, checked_out_world, 0, 0, 0) },
        0
    );
    assert_eq!(
        u64::from_le_bytes(
            arena[2200..2208]
                .try_into()
                .expect("checked out world is eight bytes")
        ),
        world
    );
    let repeated_output = arena_base + 2216;
    assert_eq!(
        unsafe { checkout_layer_pixels(0, 8, repeated_output, 0, 0, 0) },
        0,
        "a registered token can be replayed into another output slot"
    );
    assert_eq!(
        u64::from_le_bytes(
            arena[2216..2224]
                .try_into()
                .expect("repeated world is eight bytes")
        ),
        world
    );
    assert_eq!(
        unsafe { checkout_layer_pixels(0, 8, arena_base + ARENA_SIZE as u64, 0, 0, 0) },
        4,
        "an invalid replay destination still fails closed"
    );
    assert!(
        state
            .smart_checkout_ids
            .values()
            .any(|checkout| checkout.checked_out)
    );
    assert_eq!(unsafe { checkin_layer_pixels(0, 8, 0, 0, 0, 0) }, 0);
    assert!(
        state
            .smart_checkout_ids
            .values()
            .all(|checkout| !checkout.checked_out)
    );

    assert_eq!(
        unsafe { pre_checkout_layer(0, 0, 9, 0, 10, 0, 30, result) },
        0
    );
    assert!(
        state
            .smart_checkout_ids
            .values()
            .all(|checkout| !checkout.checked_out),
        "a pre-checkout-only token is balanced"
    );

    let empty_request = arena_base + 2300;
    assert_eq!(
        unsafe { pre_checkout_layer(0, 0, 10, empty_request, 10, 0, 30, result) },
        0
    );
    assert_eq!(
        &arena[2048..2064],
        &[0; 16],
        "an empty request has an empty availability rectangle"
    );
    assert_eq!(
        unsafe { checkout_layer_pixels(0, 10, checked_out_world, 0, 0, 0) },
        0,
        "an empty availability rectangle does not invalidate the host-owned world"
    );
    assert_eq!(
        u64::from_le_bytes(
            arena[2200..2208]
                .try_into()
                .expect("checked out world is eight bytes")
        ),
        world
    );
    assert_eq!(unsafe { checkin_layer_pixels(0, 10, 0, 0, 0, 0) }, 0);
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn native_smart_checkout_inherits_only_an_exactly_empty_declared_layer() {
    let mut arena = vec![0u8; 4096];
    let arena_base = arena.as_mut_ptr() as u64;
    let input_world = arena_base;
    let layer_definition = arena_base + 512;
    let layer_world = layer_definition + abi::PARAM_U_OFFSET as u64;
    let input_pixels = arena_base + 1024;
    for (offset, bytes) in [
        (abi::LAYER_DATA_OFFSET, input_pixels.to_le_bytes().to_vec()),
        (abi::LAYER_ROWBYTES_OFFSET, 16i32.to_le_bytes().to_vec()),
        (abi::LAYER_WIDTH_OFFSET, 4i32.to_le_bytes().to_vec()),
        (abi::LAYER_HEIGHT_OFFSET, 3i32.to_le_bytes().to_vec()),
    ] {
        arena[offset..offset + bytes.len()].copy_from_slice(&bytes);
    }
    let state = NativeState {
        smart_input_world: input_world,
        smart_pixel_format: crate::pixel::PF_PIXEL_FORMAT_ARGB32,
        arena_next: arena_base,
        arena_end: arena_base + ARENA_SIZE as u64,
        params: vec![GuestParam {
            index: 1,
            param_type: 0,
            name: "Optional Map".into(),
            bytes: vec![0; abi::PF_PARAM_DEF_SIZE],
        }],
        parameter_definitions: vec![layer_definition],
        ..NativeState::default()
    };

    assert_eq!(
        native_smart_checkout_world(&state, 1),
        Some((input_world, 4, 3))
    );

    arena[512 + abi::PARAM_U_OFFSET + abi::LAYER_WORLD_FLAGS_OFFSET
        ..512 + abi::PARAM_U_OFFSET + abi::LAYER_WORLD_FLAGS_OFFSET + 4]
        .copy_from_slice(&1i32.to_le_bytes());
    assert_eq!(native_smart_checkout_world(&state, 1), None);
    arena[512 + abi::PARAM_U_OFFSET + abi::LAYER_WORLD_FLAGS_OFFSET
        ..512 + abi::PARAM_U_OFFSET + abi::LAYER_WORLD_FLAGS_OFFSET + 4]
        .fill(0);

    let secondary_pixels = arena_base + 1200;
    for (offset, bytes) in [
        (
            abi::LAYER_DATA_OFFSET,
            secondary_pixels.to_le_bytes().to_vec(),
        ),
        (abi::LAYER_ROWBYTES_OFFSET, 8i32.to_le_bytes().to_vec()),
        (abi::LAYER_WIDTH_OFFSET, 2i32.to_le_bytes().to_vec()),
        (abi::LAYER_HEIGHT_OFFSET, 2i32.to_le_bytes().to_vec()),
    ] {
        let start = 512 + abi::PARAM_U_OFFSET + offset;
        arena[start..start + bytes.len()].copy_from_slice(&bytes);
    }
    assert_eq!(
        native_smart_checkout_world(&state, 1),
        Some((layer_world, 2, 2))
    );
}

#[test]
fn crt_heap_imports_allocate_zero_and_reject_invalid_free() {
    let mut state = NativeState::default();
    ACTIVE_STATE.with(|slot| slot.set(&mut state));

    let zero = unsafe { native_crt_malloc(0, 0, 0, 0, 0, 0) };
    assert_ne!(zero, 0);
    assert_eq!(zero % crate::crt_heap::CRT_HEAP_ALIGNMENT, 0);
    let calloc_pointer = unsafe { native_crt_calloc(8, 4, 0, 0, 0, 0) };
    assert_ne!(calloc_pointer, 0);
    let bytes = unsafe { std::slice::from_raw_parts(calloc_pointer as *const u8, 32) };
    assert_eq!(bytes, &[0; 32]);

    assert_eq!(unsafe { native_crt_free(0, 0, 0, 0, 0, 0) }, 0);
    assert!(state.callback_error.is_none());
    assert_eq!(unsafe { native_crt_free(zero, 0, 0, 0, 0, 0) }, 0);
    assert_eq!(unsafe { native_crt_free(zero, 0, 0, 0, 0, 0) }, 0);
    assert!(
        state
            .callback_error
            .as_deref()
            .is_some_and(|message| message.contains("foreign or already-freed"))
    );
    state.callback_error = None;
    assert_eq!(unsafe { native_crt_free(calloc_pointer, 0, 0, 0, 0, 0) }, 0);
    assert_eq!(state.crt_heap.allocations().count(), 0);
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn crt_heap_imports_return_null_for_overflow_and_budget_failure() {
    let mut state = NativeState::default();
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    assert_eq!(unsafe { native_crt_calloc(u64::MAX, 2, 0, 0, 0, 0) }, 0);
    assert_eq!(
        unsafe { native_crt_malloc(crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1, 0, 0, 0, 0, 0) },
        0
    );
    assert!(state.callback_error.is_none());
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn color_param_suite_v1_is_stateful_and_fails_closed() {
    let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
    definition[..4].copy_from_slice(&101i32.to_le_bytes());
    definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
        .copy_from_slice(&PARAM_TYPE_COLOR.to_le_bytes());
    definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8]
        .copy_from_slice(&[255, 64, 128, 192, 255, 1, 2, 3]);
    let mut state = NativeState::default();
    state.color_param_suite = 0x1234;
    state.params.push(GuestParam {
        index: 1,
        param_type: PARAM_TYPE_COLOR,
        name: "Key Color".into(),
        bytes: definition.clone(),
    });
    state.parameter_definitions.push(definition.as_ptr() as u64);
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    let name = b"PF ColorParamSuite\0";
    let mut suite = 0u64;
    assert_eq!(
        unsafe {
            acquire_suite(
                name.as_ptr() as u64,
                1,
                (&mut suite as *mut u64) as u64,
                0,
                0,
                0,
            )
        },
        0
    );
    assert_eq!(suite, state.color_param_suite);

    let mut output = ColorParamPixelFloat {
        alpha: -1.0,
        red: -1.0,
        green: -1.0,
        blue: -1.0,
    };
    assert_eq!(
        unsafe {
            color_param_value(
                HOST_EFFECT_REF,
                definition.as_ptr() as u64,
                (&mut output as *mut ColorParamPixelFloat) as u64,
                0,
                0,
                0,
            )
        },
        0
    );
    assert_eq!(
        output,
        ColorParamPixelFloat {
            alpha: 1.0,
            red: 64.0 / 255.0,
            green: 128.0 / 255.0,
            blue: 192.0 / 255.0,
        }
    );

    let sentinel = output;
    definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4].copy_from_slice(&[9, 9, 9, 9]);
    assert_eq!(
        unsafe {
            color_param_value(
                HOST_EFFECT_REF,
                definition.as_ptr() as u64,
                (&mut output as *mut ColorParamPixelFloat) as u64,
                0,
                0,
                0,
            )
        },
        PF_BAD_CALLBACK_PARAM
    );
    assert_eq!(output, sentinel);
    definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4].copy_from_slice(&[255, 1, 2, 3]);
    assert_eq!(
        unsafe {
            color_param_value(
                HOST_EFFECT_REF,
                definition.as_ptr() as u64,
                (&mut output as *mut ColorParamPixelFloat) as u64,
                0,
                0,
                0,
            )
        },
        0
    );
    definition[..4].copy_from_slice(&999i32.to_le_bytes());
    assert_eq!(
        unsafe {
            color_param_value(
                HOST_EFFECT_REF,
                definition.as_ptr() as u64,
                (&mut output as *mut ColorParamPixelFloat) as u64,
                0,
                0,
                0,
            )
        },
        PF_INVALID_INDEX
    );
    definition[..4].copy_from_slice(&101i32.to_le_bytes());
    definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
        .copy_from_slice(&6i32.to_le_bytes());
    assert_eq!(
        unsafe {
            color_param_value(
                HOST_EFFECT_REF,
                definition.as_ptr() as u64,
                (&mut output as *mut ColorParamPixelFloat) as u64,
                0,
                0,
                0,
            )
        },
        PF_UNRECOGNIZED_PARAM_TYPE
    );
    assert_eq!(
        unsafe {
            color_param_value(
                0,
                definition.as_ptr() as u64,
                (&mut output as *mut ColorParamPixelFloat) as u64,
                0,
                0,
                0,
            )
        },
        PF_BAD_CALLBACK_PARAM
    );
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn point_param_suite_v1_returns_signed_fixed_values_and_rejects_nulls() {
    let mut state = NativeState {
        point_param_suite: 0x5678,
        ..NativeState::default()
    };
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    let name = b"PF PointParamSuite\0";
    let mut suite = 0u64;
    assert_eq!(
        unsafe {
            acquire_suite(
                name.as_ptr() as u64,
                1,
                (&mut suite as *mut u64) as u64,
                0,
                0,
                0,
            )
        },
        0
    );
    assert_eq!(suite, state.point_param_suite);

    let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
    definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
        .copy_from_slice(&PARAM_TYPE_POINT.to_le_bytes());
    definition[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 4]
        .copy_from_slice(&98304i32.to_le_bytes());
    definition[abi::PARAM_U_OFFSET + 4..abi::PARAM_U_OFFSET + 8]
        .copy_from_slice(&(-147456i32).to_le_bytes());
    let mut output = [0.0f64; 2];
    assert_eq!(
        unsafe {
            point_param_value(
                HOST_EFFECT_REF,
                definition.as_ptr() as u64,
                output.as_mut_ptr() as u64,
                0,
                0,
                0,
            )
        },
        0
    );
    assert_eq!(output, [1.5, -2.25]);
    assert_eq!(
        unsafe { point_param_value(HOST_EFFECT_REF, 0, output.as_mut_ptr() as u64, 0, 0, 0) },
        4
    );
    assert_eq!(
        unsafe { point_param_value(HOST_EFFECT_REF, definition.as_ptr() as u64, 0, 0, 0, 0,) },
        4
    );
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn pf_handles_map_large_storage_and_preserve_resize_bytes() {
    let mut state = NativeState::default();
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    let size = 333_294_848;
    let handle = unsafe { new_handle(size, 0, 0, 0, 0, 0) };
    assert_ne!(handle, 0);
    let data = unsafe { lock_handle(handle, 0, 0, 0, 0, 0) };
    assert_ne!(data, 0);
    unsafe {
        *((data + size - 1) as *mut u8) = 0x5a;
    }
    assert_eq!(unsafe { handle_size(handle, 0, 0, 0, 0, 0) }, size);
    unsafe {
        unlock_handle(handle, 0, 0, 0, 0, 0);
        dispose_handle(handle, 0, 0, 0, 0, 0);
    }

    let handle = unsafe { new_handle(16, 0, 0, 0, 0, 0) };
    let data = unsafe { lock_handle(handle, 0, 0, 0, 0, 0) };
    unsafe {
        *(data as *mut u32) = 0x0403_0201;
        unlock_handle(handle, 0, 0, 0, 0, 0);
    }
    let mut handle_pointer = handle;
    assert_eq!(
        unsafe { resize_handle(8192, (&mut handle_pointer as *mut u64) as u64, 0, 0, 0, 0,) },
        0
    );
    assert_eq!(handle_pointer, handle);
    let resized = unsafe { lock_handle(handle, 0, 0, 0, 0, 0) };
    assert_eq!(unsafe { *(resized as *const u32) }, 0x0403_0201);
    unsafe {
        unlock_handle(handle, 0, 0, 0, 0, 0);
        dispose_handle(handle, 0, 0, 0, 0, 0);
    }
    assert!(state.handles.is_empty());
    assert!(state.callback_error.is_none());

    let budget_handle = unsafe { new_handle(MAX_PF_HANDLE_SIZE, 0, 0, 0, 0, 0) };
    assert_ne!(budget_handle, 0);
    assert_eq!(unsafe { new_handle(1, 0, 0, 0, 0, 0) }, 0);
    assert!(
        state
            .callback_error
            .as_deref()
            .is_some_and(|message| message.contains("live budget"))
    );
    state.callback_error = None;
    unsafe {
        dispose_handle(budget_handle, 0, 0, 0, 0, 0);
    }
    let released_budget_handle = unsafe { new_handle(1, 0, 0, 0, 0, 0) };
    assert_ne!(released_budget_handle, 0);
    unsafe {
        dispose_handle(released_budget_handle, 0, 0, 0, 0, 0);
    }

    let mut regression_handles = Vec::with_capacity(1025);
    for _ in 0..1025 {
        let handle = unsafe { new_handle(0, 0, 0, 0, 0, 0) };
        assert_ne!(
            handle, 0,
            "real AEX workloads must be allowed to exceed the old 1024-handle cap"
        );
        regression_handles.push(handle);
    }
    for handle in regression_handles {
        unsafe {
            dispose_handle(handle, 0, 0, 0, 0, 0);
        }
    }
    assert!(state.handles.is_empty());
    assert!(state.callback_error.is_none());

    assert_eq!(unsafe { lock_handle(0xdead_beef, 0, 0, 0, 0, 0) }, 0);
    assert!(
        state
            .callback_error
            .as_deref()
            .is_some_and(|message| message.contains("unknown handle"))
    );
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn pf_handle_dispose_consumes_outstanding_locks_and_rejects_stale_handles() {
    let mut state = NativeState::default();
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    let handle = unsafe { new_handle(4096, 0, 0, 0, 0, 0) };
    assert_ne!(handle, 0);
    assert_ne!(unsafe { lock_handle(handle, 0, 0, 0, 0, 0) }, 0);
    assert_eq!(state.handles[&handle].locks, 1);

    assert_eq!(unsafe { dispose_handle(handle, 0, 0, 0, 0, 0) }, 0);
    assert!(state.handles.is_empty());
    assert!(state.callback_error.is_none());

    assert_eq!(unsafe { dispose_handle(handle, 0, 0, 0, 0, 0) }, 0);
    assert!(
        state
            .callback_error
            .as_deref()
            .is_some_and(|message| message.contains("stale or foreign handle"))
    );
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn openmp_thread_count_is_positive_and_deterministic() {
    assert_eq!(unsafe { native_omp_get_max_threads(0, 0, 0, 0, 0, 0) }, 1);
    let omp_callback: unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64 =
        unsafe { std::mem::transmute(native_import_callback("omp_get_max_threads")) };
    assert_eq!(unsafe { omp_callback(0, 0, 0, 0, 0, 0) }, 1);
    assert_eq!(
        native_import_callback("unknown_import"),
        callback_address!(poison_callback)
    );
}

#[test]
fn native_imports_reject_cxx_exception_dispatch_but_preserve_ordinary_callbacks() {
    assert!(matches!(
        validate_native_import("_CxxThrowException"),
        Err(GuestError::UnsupportedImport { name }) if name == "_CxxThrowException"
    ));
    // These are object-file aliases/decorations rather than names emitted
    // by the AMD64 PE import directory and must not broaden the match.
    assert!(matches!(
        validate_native_import("__imp__CxxThrowException"),
        Err(GuestError::UnsupportedImport { .. })
    ));
    assert!(matches!(
        validate_native_import("__CxxThrowException@8"),
        Err(GuestError::UnsupportedImport { .. })
    ));

    assert!(validate_native_import("malloc").is_ok());
    assert_eq!(
        native_import_callback("malloc"),
        callback_address!(native_crt_malloc)
    );
    assert!(validate_native_import("omp_get_max_threads").is_ok());
    assert!(validate_native_import("lround").is_ok());
    let omp_callback: unsafe extern "win64" fn(u64, u64, u64, u64, u64, u64) -> u64 =
        unsafe { std::mem::transmute(native_import_callback("omp_get_max_threads")) };
    assert_eq!(unsafe { omp_callback(0, 0, 0, 0, 0, 0) }, 1);

    for name in [
        "?GetEntry@DebugDatabase@debug@dvacore@@QEBA?AVstring@std@@XZ",
        "_vcomp_for_dynamic_init",
        "_vcomp_future_runtime_entry",
    ] {
        assert!(matches!(
            validate_native_import(name),
            Err(GuestError::UnsupportedImport { name: rejected }) if rejected == name
        ));
    }

    // Every decorated or otherwise unknown import is rejected unless it has
    // an explicit typed implementation above.
    for name in [
        "?GetValue@Thing@@QEBAHAEBVOther@@@Z",
        "?GetValue@Thing@@QEBAHPEBVOther@@@Z",
        "?Consume@Thing@@QEBAHVPayload@@@Z",
        "?Consume@@YAHUPayload@@@Z",
        "?GetValue@Thing@@QEBAHXZ",
        "my_vcomp_helper",
    ] {
        assert!(matches!(
            validate_native_import(name),
            Err(GuestError::UnsupportedImport { name: rejected }) if rejected == name
        ));
    }
}

#[test]
fn utility_v7_v13_callbacks_match_unicorn_contract() {
    let mut state = NativeState::default();
    ACTIVE_STATE.with(|slot| slot.set(&mut state));

    for version in [7u32, 13] {
        let callbacks = native_utility_callbacks(version).unwrap();
        let (slot_count, register_slot, window_slot) = utility_suite_layout(version).unwrap();
        assert_eq!(callbacks.len(), slot_count);

        let register: Win64Function =
            unsafe { std::mem::transmute(callbacks[register_slot] as usize) };
        let mut plugin_id = 0i32;
        assert_eq!(
            unsafe { register(0, 0, (&mut plugin_id as *mut i32) as u64, 0, 0, 0,) },
            0
        );
        assert_eq!(plugin_id, 1);

        let get_window: Win64Function =
            unsafe { std::mem::transmute(callbacks[window_slot] as usize) };
        let mut window = u64::MAX;
        assert_eq!(
            unsafe { get_window((&mut window as *mut u64) as u64, 0, 0, 0, 0, 0) },
            0
        );
        assert_eq!(window, 0);

        let unsupported: Win64Function = unsafe { std::mem::transmute(callbacks[0] as usize) };
        assert_eq!(unsafe { unsupported(0, 0, 0, 0, 0, 0) }, 4);
        assert_eq!(unsafe { unsupported(0, 0, 0, 0, 0, 0) }, 4);
    }

    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
    assert_eq!(
        state.unsupported_suite_calls,
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
    assert_eq!(state.dropped_unsupported_suite_calls, 0);
}

#[test]
fn pf_world_suite_v2_native_lifecycle_matches_unicorn() {
    let mut arena = vec![0u8; 0x10000];
    let arena_base = arena.as_mut_ptr() as u64;
    let mut state = NativeState {
        arena_next: arena_base,
        arena_end: arena_base + arena.len() as u64,
        world_suite: 0x1234,
        ..NativeState::default()
    };
    ACTIVE_STATE.with(|slot| slot.set(&mut state));
    let mut world_storage = [0u8; abi::PF_LAYER_DEF_SIZE];
    let world = world_storage.as_mut_ptr() as u64;
    let suite_name = std::ffi::CString::new("PF World Suite").unwrap();
    let mut suite = 0u64;
    assert_eq!(
        unsafe {
            acquire_suite(
                suite_name.as_ptr() as u64,
                2,
                (&mut suite as *mut u64) as u64,
                0,
                0,
                0,
            )
        },
        0
    );
    assert_eq!(suite, state.world_suite);

    assert_eq!(unsafe { new_world(1, 3, 2, 1, 0xdead_beef, world) }, 4);
    assert!(state.worlds.is_empty());
    assert_eq!(unsafe { new_world(1, 3, 2, 1, 0x3631_6561, world) }, 0);
    let data =
        unsafe { ptr::read_unaligned((world + abi::LAYER_DATA_OFFSET as u64) as *const u64) };
    assert_ne!(data, 0);
    assert_eq!(
        unsafe { ptr::read_unaligned((world + abi::LAYER_ROWBYTES_OFFSET as u64) as *const i32) },
        24
    );
    assert_eq!(
        unsafe { std::slice::from_raw_parts(data as *const u8, 48) },
        &[0; 48]
    );
    assert_eq!(unsafe { new_world(1, 3, 2, 1, 0x3631_6561, world) }, 4);
    assert_eq!(state.worlds.len(), 1);
    let mut format = 0u32;
    let format_output = (&mut format as *mut u32) as u64;
    format = 0xfeed_beef;
    assert_eq!(
        unsafe { get_world_pixel_format(0, format_output, 0, 0, 0, 0) },
        4
    );
    assert_eq!(format, 0xfeed_beef);
    assert_eq!(
        unsafe { get_world_pixel_format(world, format_output, 0, 0, 0, 0) },
        0
    );
    assert_eq!(format, 0x3631_6561);
    let mut smart_input_storage = [0u8; abi::PF_LAYER_DEF_SIZE];
    let mut smart_output_storage = [0u8; abi::PF_LAYER_DEF_SIZE];
    state.smart_input_world = smart_input_storage.as_mut_ptr() as u64;
    state.smart_output_world = smart_output_storage.as_mut_ptr() as u64;
    state.smart_pixel_format = crate::pixel::PF_PIXEL_FORMAT_ARGB128;
    for smart_world in [state.smart_input_world, state.smart_output_world] {
        assert_eq!(
            unsafe { get_world_pixel_format(smart_world, format_output, 0, 0, 0, 0) },
            0
        );
        assert_eq!(format as i32, crate::pixel::PF_PIXEL_FORMAT_ARGB128);
    }
    assert_eq!(unsafe { get_world_pixel_format(world, 0, 0, 0, 0, 0) }, 4);
    assert_eq!(unsafe { dispose_world(1, world, 0, 0, 0, 0) }, 0);
    assert!(state.worlds.is_empty());
    assert_eq!(
        unsafe { std::slice::from_raw_parts(world as *const u8, abi::PF_LAYER_DEF_SIZE) },
        vec![0; abi::PF_LAYER_DEF_SIZE]
    );
    assert_eq!(unsafe { dispose_world(1, world, 0, 0, 0, 0) }, 4);
    world_storage[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    world_storage[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    assert_eq!(
        unsafe { get_world_pixel_format(world, format_output, 0, 0, 0, 0) },
        0
    );
    assert_eq!(format, 0x6267_7261);
    assert_eq!(unsafe { new_world(1, 1, 1, 0x100, 0x3233_6561, world) }, 0);
    let float_data =
        unsafe { ptr::read_unaligned((world + abi::LAYER_DATA_OFFSET as u64) as *const u64) };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(float_data as *const u8, 16) },
        &[0xcd; 16]
    );
    assert_eq!(unsafe { dispose_world(1, world, 0, 0, 0, 0) }, 0);
    assert_eq!(
        unsafe { new_world(1, i32::MAX as u64, i32::MAX as u64, 1, 0x6267_7261, world,) },
        4
    );
    assert!(state.worlds.is_empty());
    ACTIVE_STATE.with(|slot| slot.set(ptr::null_mut()));
}

#[test]
fn aegp_memory_v1_slots_zero_through_five_match_unicorn_lifecycle() {
    let mut arena = vec![0u8; 0x10000];
    let arena_end = arena.as_ptr() as u64 + arena.len() as u64;
    let mut state = NativeState {
        arena_next: arena.as_mut_ptr() as u64,
        arena_end,
        ..NativeState::default()
    };
    with_native_aegp_memory_context(
        &mut state.aegp_memory,
        &mut state.arena_next,
        arena_end,
        || {
            let label = std::ffi::CString::new("olm_memory").unwrap();
            let mut handle = u64::MAX;
            assert_eq!(
                unsafe {
                    new_aegp_mem_handle(
                        1,
                        label.as_ptr() as u64,
                        i32::MAX as u64 + 1,
                        0,
                        (&mut handle as *mut u64) as u64,
                        0,
                    )
                },
                4
            );
            assert_eq!(handle, 0);
            assert_eq!(
                unsafe {
                    new_aegp_mem_handle(
                        1,
                        label.as_ptr() as u64,
                        4,
                        1,
                        (&mut handle as *mut u64) as u64,
                        0,
                    )
                },
                0
            );
            assert_ne!(handle, 0);
            assert_eq!(handle % 8, 0);

            let mut data = 0u64;
            assert_eq!(
                unsafe { lock_aegp_mem_handle(handle, (&mut data as *mut u64) as u64, 0, 0, 0, 0) },
                0
            );
            assert_eq!(data % 16, 0);
            assert_eq!(unsafe { *(data as *const u32) }, 0);
            unsafe {
                *(data as *mut u32) = 0x1122_3344;
            }
            assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 4);
            assert_eq!(
                unsafe { resize_aegp_mem_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
                4
            );
            assert_eq!(unsafe { unlock_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);

            let mut size = 0u32;
            assert_eq!(
                unsafe {
                    get_aegp_mem_handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0)
                },
                0
            );
            assert_eq!(size, 4);
            assert_eq!(
                unsafe { resize_aegp_mem_handle(label.as_ptr() as u64, 8, handle, 0, 0, 0) },
                0
            );
            let mut resized_data = 0u64;
            assert_eq!(
                unsafe {
                    lock_aegp_mem_handle(handle, (&mut resized_data as *mut u64) as u64, 0, 0, 0, 0)
                },
                0
            );
            assert_eq!(unsafe { *(resized_data as *const u32) }, 0x1122_3344);
            assert_eq!(unsafe { *((resized_data + 4) as *const u32) }, 0);
            assert_eq!(unsafe { unlock_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);
            assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 0);
            assert_eq!(unsafe { free_aegp_mem_handle(handle, 0, 0, 0, 0, 0) }, 4);
            assert_eq!(
                unsafe {
                    get_aegp_mem_handle_size(handle, (&mut size as *mut u32) as u64, 0, 0, 0, 0)
                },
                4
            );
            let callbacks = native_aegp_memory_callbacks();
            assert_eq!(
                callbacks[6],
                callback_address!(unsupported_aegp_memory_slot)
            );
            assert_eq!(
                callbacks[7],
                callback_address!(unsupported_aegp_memory_slot)
            );
            assert_eq!(unsafe { unsupported_aegp_memory_slot(0, 0, 0, 0, 0, 0) }, 4);

            let mut reuse_high_water = 0u64;
            for cycle in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
                let mut recycled_handle = 0u64;
                assert_eq!(
                    unsafe {
                        new_aegp_mem_handle(
                            1,
                            label.as_ptr() as u64,
                            32,
                            0,
                            (&mut recycled_handle as *mut u64) as u64,
                            0,
                        )
                    },
                    0
                );
                let mut recycled_data = 0u64;
                assert_eq!(
                    unsafe {
                        lock_aegp_mem_handle(
                            recycled_handle,
                            (&mut recycled_data as *mut u64) as u64,
                            0,
                            0,
                            0,
                            0,
                        )
                    },
                    0
                );
                assert!(recycled_data >= arena.as_ptr() as u64);
                assert!(recycled_data + 32 <= arena_end);
                assert_eq!(
                    unsafe { unlock_aegp_mem_handle(recycled_handle, 0, 0, 0, 0, 0) },
                    0
                );
                assert_eq!(
                    unsafe { free_aegp_mem_handle(recycled_handle, 0, 0, 0, 0, 0) },
                    0
                );
                if cycle == 0 {
                    reuse_high_water = active_arena_next().unwrap();
                } else {
                    assert_eq!(active_arena_next().unwrap(), reuse_high_water);
                }
            }
            let mut resize_handle = 0u64;
            assert_eq!(
                unsafe {
                    new_aegp_mem_handle(
                        1,
                        label.as_ptr() as u64,
                        32,
                        0,
                        (&mut resize_handle as *mut u64) as u64,
                        0,
                    )
                },
                0
            );
            for _ in 0..(MAX_AEGP_MEMORY_HANDLES * 2) {
                assert_eq!(
                    unsafe {
                        resize_aegp_mem_handle(label.as_ptr() as u64, 128, resize_handle, 0, 0, 0)
                    },
                    0
                );
                assert_eq!(
                    unsafe {
                        resize_aegp_mem_handle(label.as_ptr() as u64, 32, resize_handle, 0, 0, 0)
                    },
                    0
                );
            }
            assert_eq!(
                unsafe { free_aegp_mem_handle(resize_handle, 0, 0, 0, 0, 0) },
                0
            );
        },
    );
}
