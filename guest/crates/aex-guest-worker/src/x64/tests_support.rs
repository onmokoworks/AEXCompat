use super::*;

const TEST_CODE: u64 = 0x1000_0000;
const TEST_CXX_THROW: u64 = STUB_BASE + 0x80460;
const TEST_THROW_INFO: u64 = TEST_CODE + 0x800;

/// RSP at guest entry for a `call_win64` with `argument_count` slots: the
/// frame holds the return address, the 32-byte home space, and one slot per
/// argument beyond the four register slots, padded so RSP % 16 == 8 (mirrors
/// `call_win64_with_timeout`).
fn win64_entry_rsp(argument_count: usize) -> u64 {
    let frame_bytes = 8 + 0x20 + 8 * argument_count.saturating_sub(4) as u64;
    let frame_bytes = if frame_bytes % 16 == 8 {
        frame_bytes
    } else {
        frame_bytes + 8
    };
    STACK_BASE + STACK_SIZE - frame_bytes
}

fn test_engine(code: &[u8]) -> GuestEngine<'static> {
    let mut unicorn = Unicorn::new_with_data(
        Arch::X86,
        Mode::MODE_64,
        GuestState {
            next_handle_data: HANDLE_DATA_BASE,
            next_aegp_memory_handle: AEGP_MEMORY_HANDLE_BASE,
            image_region: Some((TEST_CODE, TEST_CODE + PAGE_SIZE)),
            image_executable_ranges: vec![(TEST_CODE, TEST_CODE + code.len() as u64)],
            ..GuestState::default()
        },
    )
    .unwrap();
    install_avx_fallback(&mut unicorn).unwrap();
    unicorn.mem_map(TEST_CODE, PAGE_SIZE, Prot::ALL).unwrap();
    unicorn
        .mem_map(DATA_BASE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    unicorn
        .mem_map(HANDLE_DATA_BASE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL).unwrap();
    unicorn
        .mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    unicorn.mem_write(TEST_CODE, code).unwrap();
    install_avx_state_sync_points(
        &mut unicorn,
        discover_avx_state_sync_points(code, TEST_CODE).unwrap(),
    )
    .unwrap();
    unicorn.mem_write(RETURN_ADDRESS, &[0xcc]).unwrap();
    for address in [
        HOST_ACQUIRE_SUITE,
        HOST_REGISTER_UI,
        HOST_NEW_HANDLE,
        HOST_LOCK_HANDLE,
        HOST_UNLOCK_HANDLE,
        HOST_DISPOSE_HANDLE,
        HOST_HANDLE_SIZE,
        HOST_RESIZE_HANDLE,
        HOST_AEGP_REGISTER,
        HOST_AEGP_GET_MAIN_WINDOW,
        HOST_AEGP_NEW_MEM_HANDLE,
        HOST_AEGP_FREE_MEM_HANDLE,
        HOST_AEGP_LOCK_MEM_HANDLE,
        HOST_AEGP_UNLOCK_MEM_HANDLE,
        HOST_AEGP_MEM_HANDLE_SIZE,
        HOST_AEGP_RESIZE_MEM_HANDLE,
        HOST_NEW_WORLD,
        HOST_DISPOSE_WORLD,
        HOST_GET_WORLD_PIXEL_FORMAT,
        HOST_ITERATE8,
        HOST_ITERATE8_CONTINUE,
        HOST_COLOR_PARAM_VALUE,
        HOST_POINT_PARAM_VALUE,
        HOST_EXTENDED_ALLOC,
        HOST_EXTENDED_FREE,
        HOST_EXTENDED_LOOKUP,
        HOST_ITERATE8_ORIGIN,
        HOST_FILL8,
        HOST_NEW_WORLD8,
        HOST_GET_CALLBACK_ADDR,
        HOST_CHECKOUT_PARAM,
        HOST_TRANSFER_RECT8,
        HOST_PRE_CHECKOUT_LAYER,
        HOST_CHECKOUT_LAYER_PIXELS,
        HOST_CHECKIN_LAYER_PIXELS,
        HOST_ITERATE16,
        HOST_ITERATE16_CONTINUE,
        HOST_ITERATE_FLOAT,
        HOST_ITERATE_FLOAT_CONTINUE,
        HOST_CRT_INITTERM_CONTINUE,
        HOST_FLS_FREE_CONTINUE,
        HOST_CREATE_THREAD_CONTINUE,
    ] {
        unicorn.mem_write(address, &[0xc3]).unwrap();
    }
    unicorn
        .mem_write(HOST_POISON, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3])
        .unwrap();
    unicorn
        .mem_write(HOST_AEGP_MEMORY_UNSUPPORTED, &[0xb8, 4, 0, 0, 0, 0xc3])
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_ACQUIRE_SUITE,
            HOST_ACQUIRE_SUITE,
            emulate_acquire_suite,
        )
        .unwrap();
    unicorn
        .mem_write(HOST_CREATE_THREAD_CONTINUE, &[0x41, 0xff, 0xe3])
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_CREATE_THREAD_CONTINUE,
            HOST_CREATE_THREAD_CONTINUE,
            continue_windows_thread,
        )
        .unwrap();
    for (address, callback) in [
        (
            HOST_NEW_HANDLE,
            emulate_new_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
        ),
        (HOST_LOCK_HANDLE, emulate_lock_handle),
        (HOST_UNLOCK_HANDLE, emulate_unlock_handle),
        (HOST_DISPOSE_HANDLE, emulate_dispose_handle),
        (HOST_HANDLE_SIZE, emulate_handle_size),
        (HOST_RESIZE_HANDLE, emulate_resize_handle),
    ] {
        unicorn.add_code_hook(address, address, callback).unwrap();
    }
    unicorn
        .add_code_hook(
            HOST_AEGP_REGISTER,
            HOST_AEGP_REGISTER,
            emulate_aegp_register,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_AEGP_GET_MAIN_WINDOW,
            HOST_AEGP_GET_MAIN_WINDOW,
            emulate_aegp_get_main_window,
        )
        .unwrap();
    for (address, callback) in [
        (
            HOST_AEGP_NEW_MEM_HANDLE,
            emulate_aegp_new_mem_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
        ),
        (HOST_AEGP_FREE_MEM_HANDLE, emulate_aegp_free_mem_handle),
        (HOST_AEGP_LOCK_MEM_HANDLE, emulate_aegp_lock_mem_handle),
        (HOST_AEGP_UNLOCK_MEM_HANDLE, emulate_aegp_unlock_mem_handle),
        (HOST_AEGP_MEM_HANDLE_SIZE, emulate_aegp_mem_handle_size),
        (HOST_AEGP_RESIZE_MEM_HANDLE, emulate_aegp_resize_mem_handle),
        (HOST_NEW_WORLD, emulate_new_world),
        (HOST_DISPOSE_WORLD, emulate_dispose_world),
        (HOST_GET_WORLD_PIXEL_FORMAT, emulate_get_world_pixel_format),
    ] {
        unicorn.add_code_hook(address, address, callback).unwrap();
    }
    let mut aegp_memory_suite = [0u8; 64];
    for (slot, callback) in [
        HOST_AEGP_NEW_MEM_HANDLE,
        HOST_AEGP_FREE_MEM_HANDLE,
        HOST_AEGP_LOCK_MEM_HANDLE,
        HOST_AEGP_UNLOCK_MEM_HANDLE,
        HOST_AEGP_MEM_HANDLE_SIZE,
        HOST_AEGP_RESIZE_MEM_HANDLE,
        HOST_AEGP_MEMORY_UNSUPPORTED,
        HOST_AEGP_MEMORY_UNSUPPORTED,
    ]
    .into_iter()
    .enumerate()
    {
        aegp_memory_suite[slot * 8..slot * 8 + 8].copy_from_slice(&callback.to_le_bytes());
    }
    unicorn
        .mem_write(HOST_AEGP_MEMORY_SUITE, &aegp_memory_suite)
        .unwrap();
    let mut world_suite = [0u8; 24];
    for (slot, callback) in [
        HOST_NEW_WORLD,
        HOST_DISPOSE_WORLD,
        HOST_GET_WORLD_PIXEL_FORMAT,
    ]
    .into_iter()
    .enumerate()
    {
        world_suite[slot * 8..slot * 8 + 8].copy_from_slice(&callback.to_le_bytes());
    }
    unicorn.mem_write(HOST_WORLD_SUITE, &world_suite).unwrap();
    install_aegp_compute_cache_suite(&mut unicorn).unwrap();
    unicorn
        .add_code_hook(HOST_ITERATE8, HOST_ITERATE8, emulate_iterate8)
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_ITERATE8_CONTINUE,
            HOST_ITERATE8_CONTINUE,
            continue_iterate,
        )
        .unwrap();
    unicorn
        .add_code_hook(HOST_ITERATE16, HOST_ITERATE16, emulate_iterate16)
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_ITERATE16_CONTINUE,
            HOST_ITERATE16_CONTINUE,
            continue_iterate,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_ITERATE_FLOAT,
            HOST_ITERATE_FLOAT,
            emulate_iterate_float,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_ITERATE_FLOAT_CONTINUE,
            HOST_ITERATE_FLOAT_CONTINUE,
            continue_iterate,
        )
        .unwrap();
    unicorn
        .mem_write(HOST_CRT_INITTERM_CONTINUE, &[0x41, 0xff, 0xe3])
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_CRT_INITTERM_CONTINUE,
            HOST_CRT_INITTERM_CONTINUE,
            continue_crt_initterm,
        )
        .unwrap();
    unicorn
        .mem_write(HOST_FLS_FREE_CONTINUE, &[0x41, 0xff, 0xe3])
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_FLS_FREE_CONTINUE,
            HOST_FLS_FREE_CONTINUE,
            continue_fls_free,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_COLOR_PARAM_VALUE,
            HOST_COLOR_PARAM_VALUE,
            emulate_color_param_value,
        )
        .unwrap();
    unicorn
        .add_code_hook(HOST_REGISTER_UI, HOST_REGISTER_UI, emulate_register_ui)
        .unwrap();
    unicorn.get_data_mut().next_pf_handle_data = PF_HANDLE_DATA_BASE;
    unicorn
        .add_code_hook(
            HOST_POINT_PARAM_VALUE,
            HOST_POINT_PARAM_VALUE,
            emulate_point_param_value,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_CHECKOUT_PARAM,
            HOST_CHECKOUT_PARAM,
            emulate_checkout_param,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_TRANSFER_RECT8,
            HOST_TRANSFER_RECT8,
            emulate_transfer_rect8,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_PRE_CHECKOUT_LAYER,
            HOST_PRE_CHECKOUT_LAYER,
            emulate_pre_checkout_layer,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_CHECKOUT_LAYER_PIXELS,
            HOST_CHECKOUT_LAYER_PIXELS,
            emulate_checkout_layer_pixels,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_CHECKIN_LAYER_PIXELS,
            HOST_CHECKIN_LAYER_PIXELS,
            emulate_checkin_layer_pixels,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_EXTENDED_ALLOC,
            HOST_EXTENDED_ALLOC,
            emulate_extended_alloc,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_EXTENDED_FREE,
            HOST_EXTENDED_FREE,
            emulate_extended_free,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_EXTENDED_LOOKUP,
            HOST_EXTENDED_LOOKUP,
            emulate_extended_lookup,
        )
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_ITERATE8_ORIGIN,
            HOST_ITERATE8_ORIGIN,
            emulate_iterate8_origin,
        )
        .unwrap();
    unicorn
        .add_code_hook(HOST_FILL8, HOST_FILL8, emulate_fill8)
        .unwrap();
    unicorn.mem_write(HOST_SUBPIXEL_SAMPLE8, &[0xc3]).unwrap();
    unicorn.mem_write(HOST_AREA_SAMPLE8, &[0xc3]).unwrap();
    unicorn
        .add_code_hook(
            HOST_SUBPIXEL_SAMPLE8,
            HOST_SUBPIXEL_SAMPLE8,
            emulate_subpixel_sample8,
        )
        .unwrap();
    unicorn
        .add_code_hook(HOST_AREA_SAMPLE8, HOST_AREA_SAMPLE8, emulate_area_sample8)
        .unwrap();
    unicorn
        .add_code_hook(HOST_NEW_WORLD8, HOST_NEW_WORLD8, emulate_new_world8)
        .unwrap();
    unicorn
        .add_code_hook(
            HOST_GET_CALLBACK_ADDR,
            HOST_GET_CALLBACK_ADDR,
            emulate_get_callback_addr,
        )
        .unwrap();
    install_iterate8_suites(&mut unicorn).unwrap();
    install_typed_iterate_suites(&mut unicorn).unwrap();
    install_pf_ansi_suite_v2(&mut unicorn).unwrap();
    install_gpu_device_suite(&mut unicorn).unwrap();
    install_windows_condition_variable_callbacks(&mut unicorn).unwrap();
    install_dynamic_windows_import_callbacks(&mut unicorn).unwrap();
    unicorn.get_data_mut().next_windows_thread_id = 2;
    unicorn.get_data_mut().current_windows_thread_id = 1;
    unicorn
        .mem_write(
            HOST_COLOR_PARAM_SUITE,
            &HOST_COLOR_PARAM_VALUE.to_le_bytes(),
        )
        .unwrap();
    unicorn
        .mem_write(
            HOST_POINT_PARAM_SUITE,
            &HOST_POINT_PARAM_VALUE.to_le_bytes(),
        )
        .unwrap();
    install_aegp_utility_suites(&mut unicorn).unwrap();
    let mut trace_points = Vec::new();
    let mut decoder = Decoder::with_ip(64, code, TEST_CODE, DecoderOptions::NONE);
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if instruction.mnemonic() == Mnemonic::Call
            || instruction.mnemonic() == Mnemonic::Jmp
            || instruction.is_ip_rel_memory_operand()
            || matches!(instruction.mnemonic(), Mnemonic::Ret | Mnemonic::Retf)
        {
            trace_points.push(instruction.ip());
        }
    }
    GuestEngine {
        unicorn,
        scheduled_windows_threads: BTreeMap::new(),
        scheduler_ready: VecDeque::new(),
        scheduler_deferred_ready: VecDeque::new(),
        parked_main_context: None,
        next_data: DATA_BASE,
        next_import_stub: 0,
        image_base: TEST_CODE,
        image_end: TEST_CODE + PAGE_SIZE,
        census_hook: None,
        trace_hooks: Vec::new(),
        trace_points,
        image_sha256: "synthetic".into(),
        entry_export: "fixture_entry".into(),
        trace_modules: vec![TraceModule {
            name: "fixture".into(),
            kind: "mapped_pe",
            sha256: None,
            symbols: vec!["fixture_entry".into()],
        }],
    }
}

fn install_test_cxx_throw(engine: &mut GuestEngine<'static>) {
    engine.unicorn.mem_write(TEST_CXX_THROW, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(TEST_CXX_THROW, TEST_CXX_THROW, |unicorn, _, _| {
            emulate_cxx_throw_exception(unicorn);
        })
        .unwrap();
}

fn install_test_i32_throw_info(engine: &mut GuestEngine<'static>) {
    const CATCHABLE_ARRAY: u64 = TEST_CODE + 0x820;
    const CATCHABLE_TYPE: u64 = TEST_CODE + 0x830;
    const TYPE_DESCRIPTOR: u64 = TEST_CODE + 0x850;
    let rva = |address: u64| u32::try_from(address - TEST_CODE).unwrap();

    let mut throw_info = [0u8; 16];
    throw_info[12..16].copy_from_slice(&rva(CATCHABLE_ARRAY).to_le_bytes());
    engine.write(TEST_THROW_INFO, &throw_info).unwrap();

    let mut catchable_array = [0u8; 8];
    catchable_array[0..4].copy_from_slice(&1u32.to_le_bytes());
    catchable_array[4..8].copy_from_slice(&rva(CATCHABLE_TYPE).to_le_bytes());
    engine.write(CATCHABLE_ARRAY, &catchable_array).unwrap();

    let mut catchable_type = [0u8; 28];
    catchable_type[0..4].copy_from_slice(&1u32.to_le_bytes());
    catchable_type[4..8].copy_from_slice(&rva(TYPE_DESCRIPTOR).to_le_bytes());
    catchable_type[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    catchable_type[20..24].copy_from_slice(&4u32.to_le_bytes());
    engine.write(CATCHABLE_TYPE, &catchable_type).unwrap();

    let mut type_descriptor = [0u8; 19];
    type_descriptor[16..19].copy_from_slice(b".H\0");
    engine.write(TYPE_DESCRIPTOR, &type_descriptor).unwrap();
}

fn push_mov_imm64(code: &mut Vec<u8>, register_opcode: [u8; 2], value: u64) {
    code.extend_from_slice(&register_opcode);
    code.extend_from_slice(&value.to_le_bytes());
}

fn selector_throw_fixture(
    include_unsupported_acquire: bool,
    throw_error_pointer: u64,
    throw_info_pointer: u64,
    instructions_after_acquire: usize,
) -> Vec<u8> {
    let mut code = Vec::new();
    if include_unsupported_acquire {
        push_mov_imm64(&mut code, [0x48, 0xb9], DATA_BASE + 0x100); // mov rcx, suite name
        code.extend_from_slice(&[0xba, 1, 0, 0, 0]); // mov edx, 1
        push_mov_imm64(&mut code, [0x49, 0xb8], DATA_BASE + 0x200); // mov r8, output
        push_mov_imm64(&mut code, [0x48, 0xb8], HOST_ACQUIRE_SUITE); // mov rax, callback
        code.extend_from_slice(&[0xff, 0xd0]); // call rax
        code.resize(code.len() + instructions_after_acquire, 0x90);
    }
    push_mov_imm64(&mut code, [0x48, 0xb9], throw_error_pointer); // mov rcx, exception object
    push_mov_imm64(&mut code, [0x48, 0xba], throw_info_pointer); // mov rdx, ThrowInfo
    push_mov_imm64(&mut code, [0x48, 0xb8], TEST_CXX_THROW); // mov rax, callback
    code.extend_from_slice(&[0xff, 0xd0]); // call rax
    code.extend_from_slice(&[0xb8, 77, 0, 0, 0, 0xc3]); // fallback: mov eax, 77; ret
    code
}

fn unsupported_acquire_fallback_fixture() -> Vec<u8> {
    let mut code = Vec::new();
    push_mov_imm64(&mut code, [0x48, 0xb9], DATA_BASE + 0x100);
    code.extend_from_slice(&[0xba, 1, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x49, 0xb8], DATA_BASE + 0x200);
    push_mov_imm64(&mut code, [0x48, 0xb8], HOST_ACQUIRE_SUITE);
    code.extend_from_slice(&[0xff, 0xd0, 0x31, 0xc0, 0xc3]);
    code
}

#[test]
fn crt_heap_imports_allocate_zero_reuse_and_reject_invalid_free() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_crt_malloc(&mut engine.unicorn, false);
    let first = engine.unicorn.reg_read(RegisterX86::RAX).unwrap();
    assert_ne!(first, 0);
    assert_eq!(first % crate::crt_heap::CRT_HEAP_ALIGNMENT, 0);

    engine.unicorn.reg_write(RegisterX86::RCX, 8).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 4).unwrap();
    emulate_crt_malloc(&mut engine.unicorn, true);
    let calloc_pointer = engine.unicorn.reg_read(RegisterX86::RAX).unwrap();
    let mut bytes = [0xff; 32];
    engine.unicorn.mem_read(calloc_pointer, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 32]);

    engine.unicorn.reg_write(RegisterX86::RCX, first).unwrap();
    emulate_crt_free(&mut engine.unicorn);
    assert!(engine.unicorn.get_data().callback_error.is_none());
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_crt_malloc(&mut engine.unicorn, false);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), first);

    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_crt_free(&mut engine.unicorn);
    assert!(engine.unicorn.get_data().callback_error.is_none());
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_crt_free(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|message| message.contains("foreign or already-freed"))
    );
}

#[test]
fn crt_strdup_copies_independently_uses_crt_ownership_and_fails_closed() {
    const STRDUP: u64 = STUB_BASE + 0x530;
    let mut engine = test_engine(&[0xc3]);
    for library in ["api-ms-win-crt-string-l1-1-0.dll", "UCRTBASE.DLL"] {
        assert_eq!(
            dispatch_win64_import(library, "_strdup"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtStrdup)
        );
    }
    assert_eq!(
        dispatch_win64_import("fixture.dll", "_strdup"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            STRDUP,
            "api-ms-win-crt-string-l1-1-0.dll",
            "_strdup",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtStrdup)
    );

    assert_eq!(engine.call_win64(STRDUP, [0; 6]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);

    let source = DATA_BASE + 0x800;
    engine.write(source, b"OLM\0").unwrap();
    let first = engine.call_win64(STRDUP, [source, 0, 0, 0, 0, 0]).unwrap();
    let second = engine.call_win64(STRDUP, [source, 0, 0, 0, 0, 0]).unwrap();
    assert_ne!(first, 0);
    assert_ne!(first, second);
    assert_eq!(engine.unicorn.mem_read_as_vec(first, 4).unwrap(), b"OLM\0");
    assert_eq!(engine.unicorn.mem_read_as_vec(second, 4).unwrap(), b"OLM\0");
    engine.write(source, b"AE!\0").unwrap();
    assert_eq!(engine.unicorn.mem_read_as_vec(first, 4).unwrap(), b"OLM\0");
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 8);

    for pointer in [first, second] {
        engine.unicorn.reg_write(RegisterX86::RCX, pointer).unwrap();
        emulate_crt_free(&mut engine.unicorn);
    }
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);

    engine.write(source, b"ABCDE").unwrap();
    let error = duplicate_crt_string(&mut engine.unicorn, source, 4).unwrap_err();
    assert!(error.contains("exceeds 4 bytes"), "{error}");
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);

    let error = engine
        .call_win64(STRDUP, [0xdead_beef, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("_strdup source read"), "{error}");
}

#[test]
fn win64_crt_tolower_uses_integer_abi_and_ascii_c_locale_semantics() {
    const TOLOWER: u64 = STUB_BASE + 0x540;
    let mut engine = test_engine(&[0xc3]);
    for library in ["api-ms-win-crt-string-l1-1-0.dll", "UCRTBASE.DLL"] {
        assert_eq!(
            dispatch_win64_import(library, "tolower"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtToLower)
        );
    }
    assert_eq!(
        dispatch_win64_import("fixture.dll", "tolower"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            TOLOWER,
            "api-ms-win-crt-string-l1-1-0.dll",
            "tolower",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtToLower)
    );

    for (input, expected) in [
        (u64::from(b'A'), u64::from(b'a')),
        (u64::from(b'Z'), u64::from(b'z')),
        (u64::from(b'@'), u64::from(b'@')),
        (u64::from(b'['), u64::from(b'[')),
        (u64::from(b'a'), u64::from(b'a')),
        (0xff, 0xff),
        (u64::from(u32::MAX), u64::from(u32::MAX)),
    ] {
        assert_eq!(
            engine.call_win64(TOLOWER, [input, 0, 0, 0, 0, 0]).unwrap(),
            expected
        );
    }
}

#[test]
fn win64_crt_stricmp_is_bounded_ascii_only_and_library_scoped() {
    const STRICMP: u64 = STUB_BASE + 0x5d0;
    assert!(matches!(
        dispatch_win64_import("api-ms-win-crt-string-l1-1-0.dll", "_stricmp"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtStricmp)
    ));
    assert!(matches!(
        dispatch_win64_import("ucrtbase.dll", "_stricmp"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtStricmp)
    ));
    assert_eq!(
        dispatch_win64_import("fixture.dll", "_stricmp"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let prepare = || {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            STRICMP,
            "api-ms-win-crt-string-l1-1-0.dll",
            "_stricmp",
        )
        .unwrap();
        engine
    };
    let compare = |engine: &mut GuestEngine<'static>, left: &[u8], right: &[u8]| {
        let left_address = DATA_BASE + 0xa00;
        let right_address = DATA_BASE + 0xb00;
        engine.write(left_address, left).unwrap();
        engine.write(right_address, right).unwrap();
        engine
            .call_win64(STRICMP, [left_address, right_address, 0, 0, 0, 0])
            .unwrap() as u32 as i32
    };

    let mut ordinary = prepare();
    assert_eq!(compare(&mut ordinary, b"Depth\0", b"depth\0"), 0);
    assert!(compare(&mut ordinary, b"alpha\0", b"BETA\0") < 0);
    assert!(compare(&mut ordinary, b"gamma\0", b"Beta\0") > 0);
    assert!(compare(&mut ordinary, &[0xc0, 0], &[0xe0, 0]) < 0);
    assert!(compare(&mut ordinary, &[0x80, 0], &[0x7f, 0]) > 0);

    let mut no_read_ahead = prepare();
    let left_page = DATA_BASE + 0x30_0000;
    let right_page = DATA_BASE + 0x32_0000;
    for page in [left_page, right_page] {
        no_read_ahead
            .unicorn
            .mem_map(page, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
    }
    let left_edge = left_page + PAGE_SIZE - 1;
    let right_edge = right_page + PAGE_SIZE - 1;
    no_read_ahead.write(left_edge, &[b'a']).unwrap();
    no_read_ahead.write(right_edge, &[b'b']).unwrap();
    assert_eq!(
        no_read_ahead
            .call_win64(STRICMP, [left_edge, right_edge, 0, 0, 0, 0])
            .unwrap() as u32 as i32,
        -1
    );

    let mut nul_does_not_read_ahead = prepare();
    for page in [left_page, right_page] {
        nul_does_not_read_ahead
            .unicorn
            .mem_map(page, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
    }
    nul_does_not_read_ahead.write(left_edge, &[0]).unwrap();
    nul_does_not_read_ahead.write(right_edge, &[0]).unwrap();
    assert_eq!(
        nul_does_not_read_ahead
            .call_win64(STRICMP, [left_edge, right_edge, 0, 0, 0, 0])
            .unwrap(),
        0
    );

    let mut aliased = prepare();
    let address = DATA_BASE + 0xa00;
    aliased.write(address, b"Alias\0").unwrap();
    assert_eq!(
        aliased
            .call_win64(STRICMP, [address, address, 0, 0, 0, 0])
            .unwrap(),
        0
    );

    let mut invalid = prepare();
    assert!(
        invalid
            .call_win64(STRICMP, [0, DATA_BASE, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("null")
    );
    let mut unreadable = prepare();
    assert!(
        unreadable
            .call_win64(STRICMP, [0x10, DATA_BASE, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("not readable")
    );

    let mut unterminated = prepare();
    let left = DATA_BASE + 0x1000;
    let right = DATA_BASE + 0x1000 + MAX_CRT_STRING_BYTES;
    unterminated
        .unicorn
        .mem_map(left, MAX_CRT_STRING_BYTES * 2, Prot::READ | Prot::WRITE)
        .unwrap();
    let bytes = vec![b'x'; MAX_CRT_STRING_BYTES as usize];
    unterminated.write(left, &bytes).unwrap();
    unterminated.write(right, &bytes).unwrap();
    assert!(
        unterminated
            .call_win64(STRICMP, [left, right, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("exceed")
    );
}

#[test]
fn win64_crt_toupper_uses_integer_abi_and_ascii_c_locale_semantics() {
    const TOUPPER: u64 = STUB_BASE + 0x550;
    let mut engine = test_engine(&[0xc3]);
    for library in ["api-ms-win-crt-string-l1-1-0.dll", "UCRTBASE.DLL"] {
        assert_eq!(
            dispatch_win64_import(library, "toupper"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtToUpper)
        );
    }
    assert_eq!(
        dispatch_win64_import("fixture.dll", "toupper"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            TOUPPER,
            "api-ms-win-crt-string-l1-1-0.dll",
            "toupper",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtToUpper)
    );

    for (input, expected) in [
        (u64::from(b'a'), u64::from(b'A')),
        (u64::from(b'z'), u64::from(b'Z')),
        (u64::from(b'`'), u64::from(b'`')),
        (u64::from(b'{'), u64::from(b'{')),
        (u64::from(b'A'), u64::from(b'A')),
        (0xff, 0xff),
        (u64::from(u32::MAX), u64::from(u32::MAX)),
    ] {
        assert_eq!(
            engine.call_win64(TOUPPER, [input, 0, 0, 0, 0, 0]).unwrap(),
            expected
        );
    }
}

#[test]
fn crt_aligned_allocation_honors_alignment_reuses_and_owns_free() {
    const ALLOC: u64 = STUB_BASE + 0x570;
    const FREE: u64 = STUB_BASE + 0x580;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol, implementation) in [
        (ALLOC, "_aligned_malloc", LegacyWin64Import::AlignedMalloc),
        (FREE, "_aligned_free", LegacyWin64Import::AlignedFree),
    ] {
        assert_eq!(
            install_win64_import(
                &mut engine.unicorn,
                stub,
                "api-ms-win-crt-heap-l1-1-0.dll",
                symbol,
            )
            .unwrap(),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("UCRTBASE.DLL", symbol),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    let first = engine
        .call_win64(ALLOC, [17, 0x20_000, 0, 0, 0, 0])
        .unwrap();
    assert_ne!(first, 0);
    assert_eq!(first % 0x20_000, 0);
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 17);
    assert_eq!(engine.call_win64(FREE, [first, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);
    assert!(engine.unicorn.mem_read_as_vec(first, 1).is_err());

    let reused = engine.call_win64(ALLOC, [0, 0x20_000, 0, 0, 0, 0]).unwrap();
    assert_eq!(reused, first);
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 1);
    engine.call_win64(FREE, [reused, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(engine.call_win64(FREE, [0, 0, 0, 0, 0, 0]).unwrap(), 0);
}

#[test]
fn crt_aligned_allocation_rejects_invalid_budget_and_free_kind_mismatch() {
    const ALLOC: u64 = STUB_BASE + 0x590;
    const FREE: u64 = STUB_BASE + 0x5a0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        ALLOC,
        "api-ms-win-crt-heap-l1-1-0.dll",
        "_aligned_malloc",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        FREE,
        "api-ms-win-crt-heap-l1-1-0.dll",
        "_aligned_free",
    )
    .unwrap();
    for (size, alignment) in [
        (1, 0),
        (1, 3),
        (crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1, 16),
        (1, 1u64 << 63),
    ] {
        assert_eq!(
            engine
                .call_win64(ALLOC, [size, alignment, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);

    engine.unicorn.reg_write(RegisterX86::RCX, 8).unwrap();
    emulate_crt_malloc(&mut engine.unicorn, false);
    let regular = engine.unicorn.reg_read(RegisterX86::RAX).unwrap();
    let error = engine
        .call_win64(FREE, [regular, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("does not own"), "{error}");
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 8);

    let mut engine = test_engine(&[0xc3]);
    let aligned = allocate_aligned_crt_region(&mut engine.unicorn, 8, 64).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, aligned).unwrap();
    emulate_crt_free(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("does not own"))
    );
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 8);
}

#[test]
fn extended_inter_allocation_is_zeroed_bounded_and_owned() {
    let mut engine = test_engine(&[0xc3]);
    let output = engine.allocate(8, 8).unwrap();
    engine.write(output, &[0xff; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_EXTENDED_ALLOC, [output, 4000, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(output, &mut pointer).unwrap();
    let pointer = u64::from_le_bytes(pointer);
    assert_ne!(pointer, 0);
    let mut bytes = [0xff; 32];
    engine.read(pointer, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 32]);

    assert_eq!(
        engine
            .call_win64(HOST_EXTENDED_FREE, [output, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .crt_heap
            .allocations()
            .next()
            .is_none()
    );

    assert_eq!(
        engine
            .call_win64(HOST_EXTENDED_ALLOC, [0, 4000, 0, 0, 0, 0])
            .unwrap(),
        4
    );
}

#[test]
fn extended_inter_lookup_serves_values_empty_and_absent_tables() {
    let mut engine = test_engine(&[0xc3]);
    let value = engine.allocate(6, 1).unwrap();
    let empty = engine.allocate(1, 1).unwrap();
    engine.write(value, b"Label\0").unwrap();
    engine.write(empty, &[0]).unwrap();
    {
        let state = engine.unicorn.get_data_mut();
        state.extended_strings.insert(27, value);
        state.extended_empty_string = empty;
        state.extended_string_table_valid = true;
    }
    assert_eq!(
        engine
            .call_win64(HOST_EXTENDED_LOOKUP, [0, 27, 0, 0, 0, 0])
            .unwrap(),
        value
    );
    assert_eq!(
        engine
            .call_win64(HOST_EXTENDED_LOOKUP, [0, 999, 0, 0, 0, 0])
            .unwrap(),
        empty
    );
    engine.unicorn.get_data_mut().extended_string_table_valid = false;
    assert_eq!(
        engine
            .call_win64(HOST_EXTENDED_LOOKUP, [0, 999, 0, 0, 0, 0])
            .unwrap(),
        0
    );
}

#[test]
fn legacy_blend_is_typed_alias_safe_and_fails_closed() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(HOST_BLEND, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(HOST_BLEND, HOST_BLEND, |unicorn, _, _| {
            emulate_blend(unicorn);
        })
        .unwrap();

    fn write_world(
        engine: &mut GuestEngine<'static>,
        pixels: &[u8],
        pixel_bytes: usize,
    ) -> (u64, u64) {
        let data = engine.allocate(pixels.len(), 8).unwrap();
        let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        engine.write(data, pixels).unwrap();
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&data.to_le_bytes());
        definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&(pixel_bytes as i32).to_le_bytes());
        definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        engine.write(world, &definition).unwrap();
        (world, data)
    }

    let cases = [
        (
            0x6267_7261u32 as i32,
            vec![255, 10, 20, 30],
            vec![0, 110, 120, 130],
            vec![128, 60, 70, 80],
        ),
        (
            0x3631_6561u32 as i32,
            [32_768u16, 1024, 2048, 4096]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
            [0u16, 3072, 4096, 6144]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
            [16_384u16, 2048, 3072, 5120]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect(),
        ),
        (
            0x3233_6561u32 as i32,
            [1.0f32, 0.1, 0.2, 0.3]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
            [0.0f32, 0.5, 0.6, 0.7]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
            [0.5f32, 0.3, 0.4, 0.5]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        ),
    ];
    for (pixel_format, first_pixels, second_pixels, expected) in cases {
        engine.configure_render_pixel_format(pixel_format);
        let pixel_bytes = first_pixels.len();
        let (first, first_data) = write_world(&mut engine, &first_pixels, pixel_bytes);
        let (second, _) = write_world(&mut engine, &second_pixels, pixel_bytes);
        assert_eq!(
            engine
                .call_win64(HOST_BLEND, [1, first, second, 32_768, first, 0])
                .unwrap(),
            0
        );
        let mut output = vec![0u8; pixel_bytes];
        engine.read(first_data, &mut output).unwrap();
        assert_eq!(output, expected);
    }

    engine.configure_render_pixel_format(0x3233_6561u32 as i32);
    let first_pixels = [-1.515_396_1f32; 4]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let second_pixels = [-0.089_762_6f32; 4]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let (first, first_data) = write_world(&mut engine, &first_pixels, 16);
    let (second, _) = write_world(&mut engine, &second_pixels, 16);
    assert_eq!(
        engine
            .call_win64(HOST_BLEND, [0, first, second, 33_817, first, 0])
            .unwrap(),
        0
    );
    let expected = f32::from_bits(0xbf47_9e5a).to_le_bytes().repeat(4);
    let mut output = vec![0u8; 16];
    engine.read(first_data, &mut output).unwrap();
    assert_eq!(output, expected);

    engine.configure_render_pixel_format(0x6267_7261u32 as i32);
    let (first, first_data) = write_world(&mut engine, &[1, 2, 3, 4], 4);
    let (second, _) = write_world(&mut engine, &[5, 6, 7, 8], 4);
    let before = engine.unicorn.mem_read_as_vec(first_data, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_BLEND, [1, first, second, 65_537, first, 0])
            .unwrap(),
        4
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(first_data, 4).unwrap(),
        before
    );
}

fn argb8_sampling_fixture(engine: &mut GuestEngine<'static>) -> (u64, u64, u64) {
    let pixels = engine.allocate(16, 8).unwrap();
    engine
        .write(
            pixels,
            &[
                255, 0, 0, 0, 255, 100, 0, 0, 255, 0, 100, 0, 255, 100, 100, 0,
            ],
        )
        .unwrap();
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&pixels.to_le_bytes());
    definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    engine.write(world, &definition).unwrap();
    let params = engine.allocate(32, 8).unwrap();
    let mut sampling = [0u8; 32];
    sampling[16..24].copy_from_slice(&world.to_le_bytes());
    engine.write(params, &sampling).unwrap();
    let destination = engine.allocate(4, 4).unwrap();
    (params, destination, world)
}

fn argb8_world_fixture(
    engine: &mut GuestEngine<'static>,
    width: i32,
    height: i32,
    pixels: &[u8],
) -> (u64, u64) {
    let rowbytes = width * abi::PF_PIXEL_SIZE as i32;
    assert_eq!(pixels.len(), (rowbytes * height) as usize);
    let pixel_data = engine.allocate(pixels.len(), 8).unwrap();
    engine.write(pixel_data, pixels).unwrap();
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&pixel_data.to_le_bytes());
    definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&rowbytes.to_le_bytes());
    definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&width.to_le_bytes());
    definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&height.to_le_bytes());
    engine.write(world, &definition).unwrap();
    (world, pixel_data)
}

fn argb8_mask_world_fixture(
    engine: &mut GuestEngine<'static>,
    width: i32,
    height: i32,
    pixels: &[u8],
) -> (u64, u64) {
    let rowbytes = width * abi::PF_PIXEL_SIZE as i32;
    assert_eq!(pixels.len(), (rowbytes * height) as usize);
    let pixel_data = engine.allocate(pixels.len(), 8).unwrap();
    engine.write(pixel_data, pixels).unwrap();
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE + 12, 8).unwrap();
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE + 12];
    definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&pixel_data.to_le_bytes());
    definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&rowbytes.to_le_bytes());
    definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&width.to_le_bytes());
    definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&height.to_le_bytes());
    engine.write(world, &definition).unwrap();
    (world, pixel_data)
}

#[test]
fn legacy_subpixel_sample8_bilinearly_samples_argb8_and_transparent_edges() {
    let mut engine = test_engine(&[0xc3]);
    let (params, destination, _) = argb8_sampling_fixture(&mut engine);
    assert_eq!(
        engine
            .call_win64(
                HOST_SUBPIXEL_SAMPLE8,
                [0, 0x8000, 0x8000, params, destination, 0],
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        [255, 50, 50, 0]
    );

    assert_eq!(
        engine
            .call_win64(
                HOST_SUBPIXEL_SAMPLE8,
                [1, (-0x8000i32) as u32 as u64, 0, params, destination, 0],
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        [128, 0, 0, 0]
    );
}

#[test]
fn legacy_area_sample8_matches_common_runtime() {
    let mut engine = test_engine(&[0xc3]);
    let (params, destination, _) = argb8_sampling_fixture(&mut engine);
    engine.write(params, &0x8000i32.to_le_bytes()).unwrap();
    engine.write(params + 4, &0x8000i32.to_le_bytes()).unwrap();
    engine
        .write(params + 8, &0x1_0000i32.to_le_bytes())
        .unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_AREA_SAMPLE8,
                [0, 0x8000, 0x8000, params, destination, 0],
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        [255, 50, 50, 0]
    );
}

#[test]
fn legacy_sampling_accepts_null_effect_ref_but_rejects_dereferenced_nulls() {
    let mut engine = test_engine(&[0xc3]);
    let (params, destination, _) = argb8_sampling_fixture(&mut engine);
    engine.write(params, &0x8000i32.to_le_bytes()).unwrap();
    engine.write(params + 4, &0x8000i32.to_le_bytes()).unwrap();
    engine
        .write(params + 8, &0x1_0000i32.to_le_bytes())
        .unwrap();

    for callback in [HOST_SUBPIXEL_SAMPLE8, HOST_AREA_SAMPLE8] {
        engine.write(destination, &[7, 7, 7, 7]).unwrap();
        assert_eq!(
            engine
                .call_win64(callback, [0, 0x8000, 0x8000, params, destination, 0])
                .unwrap(),
            0,
            "null effect_ref is an accepted, unused argument"
        );

        engine.write(destination, &[7, 7, 7, 7]).unwrap();
        assert_eq!(
            engine
                .call_win64(callback, [1, 0x8000, 0x8000, 0, destination, 0])
                .unwrap(),
            4,
            "null sampling params remain rejected"
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
            [7, 7, 7, 7],
            "a rejected call must not write the destination"
        );

        assert_eq!(
            engine
                .call_win64(callback, [1, 0x8000, 0x8000, params, 0, 0])
                .unwrap(),
            4,
            "null destination remains rejected"
        );
    }
}

#[test]
fn legacy_transfer_rect8_copies_clipped_argb8_with_opacity_and_rejects_other_modes() {
    let mut engine = test_engine(&[0xc3]);
    let (source, _) = argb8_world_fixture(
        &mut engine,
        2,
        2,
        &[
            255, 100, 0, 0, 255, 200, 0, 0, 255, 0, 100, 0, 255, 0, 200, 0,
        ],
    );
    let (destination, destination_pixels) = argb8_world_fixture(&mut engine, 3, 2, &[0; 24]);
    let rect = engine.allocate(16, 4).unwrap();
    engine
        .write(
            rect,
            &[0i32, 0, 2, 2]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let composite = engine.allocate(12, 4).unwrap();
    let mut mode = [0u8; 12];
    mode[8] = 128;
    mode[10..12].copy_from_slice(&32768u16.to_le_bytes());
    engine.write(composite, &mode).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_TRANSFER_RECT8,
                &[1, 1, 0, 0, rect, source, composite, 0, 1, 0, destination],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination_pixels, 24)
            .unwrap(),
        [
            0, 0, 0, 0, 128, 50, 0, 0, 128, 100, 0, 0, 0, 0, 0, 0, 128, 0, 50, 0, 128, 0, 100, 0,
        ]
    );

    engine.write(composite, &3i32.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_TRANSFER_RECT8,
                &[1, 1, 0, 0, rect, source, composite, 0, 1, 0, destination],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        4
    );
}

#[test]
fn legacy_transfer_rect8_applies_argb8_mask_coverage_and_rejects_invalid_masks() {
    let mut engine = test_engine(&[0xc3]);
    let (source, _) = argb8_world_fixture(
        &mut engine,
        3,
        1,
        &[255, 200, 0, 0, 255, 200, 0, 0, 255, 200, 0, 0],
    );
    let (destination, destination_pixels) = argb8_world_fixture(&mut engine, 3, 1, &[0; 12]);
    let (mask, _) = argb8_mask_world_fixture(
        &mut engine,
        3,
        1,
        &[0, 0, 0, 0, 128, 128, 128, 128, 255, 255, 255, 255],
    );
    let rect = engine.allocate(16, 4).unwrap();
    engine
        .write(
            rect,
            &[0i32, 0, 3, 1]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let composite = engine.allocate(12, 4).unwrap();
    let mut mode = [0u8; 12];
    mode[..4].copy_from_slice(&2i32.to_le_bytes());
    mode[8] = 255;
    mode[10..12].copy_from_slice(&32768u16.to_le_bytes());
    engine.write(composite, &mode).unwrap();
    let call = |engine: &mut GuestEngine<'static>| {
        engine
            .call_win64_with_timeout(
                HOST_TRANSFER_RECT8,
                &[1, 0, 0, 0, rect, source, composite, mask, 0, 0, destination],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap()
    };
    assert_eq!(call(&mut engine), 0);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination_pixels, 12)
            .unwrap(),
        [0, 0, 0, 0, 128, 100, 0, 0, 255, 200, 0, 0]
    );

    engine.write(destination_pixels, &[0x5a; 12]).unwrap();
    engine
        .write(
            mask + abi::PF_LAYER_DEF_SIZE as u64 + 8,
            &4u32.to_le_bytes(),
        )
        .unwrap();
    assert_eq!(call(&mut engine), 4);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination_pixels, 12)
            .unwrap(),
        [0x5a; 12]
    );

    engine
        .write(
            mask + abi::PF_LAYER_DEF_SIZE as u64 + 8,
            &0u32.to_le_bytes(),
        )
        .unwrap();
    engine
        .write(mask + abi::LAYER_WIDTH_OFFSET as u64, &4i32.to_le_bytes())
        .unwrap();
    assert_eq!(call(&mut engine), 4);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination_pixels, 12)
            .unwrap(),
        [0x5a; 12]
    );
}

#[test]
fn legacy_transfer_rect8_accepts_calculations_64x64_mask_with_zero_opacity() {
    const SIDE: usize = 64;
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_map(DATA_BASE + PAGE_SIZE, 0x10_000, Prot::READ | Prot::WRITE)
        .unwrap();
    let source_pixels = vec![255u8; SIDE * SIDE * abi::PF_PIXEL_SIZE];
    let destination_before = vec![0x35u8; SIDE * SIDE * abi::PF_PIXEL_SIZE];
    let mask_pixels = vec![255u8; SIDE * SIDE * abi::PF_PIXEL_SIZE];
    let (source, _) = argb8_world_fixture(&mut engine, SIDE as i32, SIDE as i32, &source_pixels);
    let (destination, destination_pixels) =
        argb8_world_fixture(&mut engine, SIDE as i32, SIDE as i32, &destination_before);
    let (mask, _) = argb8_mask_world_fixture(&mut engine, SIDE as i32, SIDE as i32, &mask_pixels);
    let composite = engine.allocate(12, 4).unwrap();
    let mut mode = [0u8; 12];
    mode[..4].copy_from_slice(&2i32.to_le_bytes());
    engine.write(composite, &mode).unwrap();

    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_TRANSFER_RECT8,
                &[1, 0, 0, 0, 0, source, composite, mask, 0, 0, destination],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination_pixels, destination_before.len())
            .unwrap(),
        destination_before
    );
}

#[test]
fn crt_memory_copy_copies_bytes_and_returns_destination() {
    let mut engine = test_engine(&[0xc3]);
    let source = DATA_BASE + 0x100;
    let destination = DATA_BASE + 0x200;
    let bytes = b"generic CRT memory copy";
    engine.unicorn.mem_write(source, bytes).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, destination)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, source).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::R8, bytes.len() as u64)
        .unwrap();

    emulate_crt_memory_copy(&mut engine.unicorn);

    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination, bytes.len())
            .unwrap(),
        bytes
    );
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        destination
    );
    assert_eq!(engine.unicorn.get_data().callback_error, None);
}

#[test]
fn crt_memory_copy_preserves_overlapping_memmove_semantics() {
    let mut engine = test_engine(&[0xc3]);
    let buffer = DATA_BASE + 0x100;
    engine
        .unicorn
        .mem_write(buffer, b"0123456789abcdef")
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, buffer + 4)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, buffer).unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 12).unwrap();

    emulate_crt_memory_copy(&mut engine.unicorn);

    assert_eq!(
        engine.unicorn.mem_read_as_vec(buffer, 16).unwrap(),
        b"01230123456789ab"
    );
}

#[test]
fn zero_length_crt_memory_copy_accepts_unmapped_pointers() {
    let mut engine = test_engine(&[0xc3]);
    let destination = 0xdead_beef_dead_beef;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, destination)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 0).unwrap();

    emulate_crt_memory_copy(&mut engine.unicorn);

    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        destination
    );
    assert_eq!(engine.unicorn.get_data().callback_error, None);
}

#[test]
fn crt_memory_copy_fails_closed_without_masking_an_earlier_error() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, DATA_BASE)
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, u64::MAX)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 2).unwrap();
    emulate_crt_memory_copy(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("memory-copy source range overflow")
    );

    engine.unicorn.get_data_mut().callback_error = Some("earlier failure".into());
    engine
        .unicorn
        .reg_write(RegisterX86::R8, MAX_CRT_MEMORY_COPY_BYTES + 1)
        .unwrap();
    emulate_crt_memory_copy(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("earlier failure")
    );
}

#[test]
fn vcruntime_memchr_returns_first_unsigned_byte_match_or_null() {
    const MEMCHR: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(&mut engine.unicorn, MEMCHR, "VCRUNTIME140.DLL", "memchr",).unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::MemChr)
    );
    let source = DATA_BASE + 0x180;
    engine
        .unicorn
        .mem_write(source, &[0x41, 0xff, 0x42, 0xff])
        .unwrap();

    assert_eq!(
        engine
            .call_win64(MEMCHR, [source, u64::MAX, 4, 0, 0, 0])
            .unwrap(),
        source + 1
    );
    assert_eq!(
        engine
            .call_win64(MEMCHR, [source, 0x43, 4, 0, 0, 0])
            .unwrap(),
        0
    );
}

#[test]
fn vcruntime_memchr_zero_length_accepts_unmapped_pointer() {
    const MEMCHR: u64 = STUB_BASE + 0x1b0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MEMCHR, "vcruntime140.dll", "memchr").unwrap();

    assert_eq!(
        engine
            .call_win64(MEMCHR, [0xdead_beef_dead_beef, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
}

#[test]
fn vcruntime_memchr_is_bounded_library_qualified_and_fail_closed() {
    assert_eq!(
        dispatch_win64_import("fixture.dll", "memchr"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    const MEMCHR: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MEMCHR, "vcruntime140.dll", "memchr").unwrap();
    let error = engine
        .call_win64(
            MEMCHR,
            [DATA_BASE, 0, MAX_CRT_MEMORY_COPY_BYTES + 1, 0, 0, 0],
        )
        .unwrap_err();
    assert!(error.to_string().contains("memchr length"), "{error}");

    engine.unicorn.get_data_mut().callback_error = None;
    let error = engine
        .call_win64(MEMCHR, [u64::MAX, 0, 2, 0, 0, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("memchr source range overflow"),
        "{error}"
    );

    engine.unicorn.get_data_mut().callback_error = None;
    let error = engine
        .call_win64(MEMCHR, [0xdead_beef, 0, 1, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("memchr source read"), "{error}");
}

#[test]
fn vcruntime_memcmp_orders_unsigned_bytes_and_crosses_chunk_boundaries() {
    const MEMCMP: u64 = STUB_BASE + 0x1d0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(&mut engine.unicorn, MEMCMP, "VCRUNTIME140.DLL", "memcmp",).unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::MemCmp)
    );
    engine
        .unicorn
        .mem_map(DATA_BASE + PAGE_SIZE, 0x30_000, Prot::READ | Prot::WRITE)
        .unwrap();
    let left = DATA_BASE;
    let right = DATA_BASE + 0x20_000;
    let mut left_bytes = vec![0x41; CRT_MEMORY_COPY_CHUNK + 2];
    let mut right_bytes = left_bytes.clone();
    left_bytes[1] = 0;
    right_bytes[1] = 0;
    left_bytes[CRT_MEMORY_COPY_CHUNK] = 0xff;
    right_bytes[CRT_MEMORY_COPY_CHUNK] = 1;
    engine.unicorn.mem_write(left, &left_bytes).unwrap();
    engine.unicorn.mem_write(right, &right_bytes).unwrap();

    assert_eq!(
        engine
            .call_win64(MEMCMP, [left, right, 1, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(MEMCMP, [left, right, 2, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(
        (engine
            .call_win64(
                MEMCMP,
                [left, right, (CRT_MEMORY_COPY_CHUNK + 1) as u64, 0, 0, 0],
            )
            .unwrap() as u32 as i32)
            > 0
    );
    assert!(
        (engine
            .call_win64(
                MEMCMP,
                [right, left, (CRT_MEMORY_COPY_CHUNK + 1) as u64, 0, 0, 0],
            )
            .unwrap() as u32 as i32)
            < 0
    );
}

#[test]
fn vcruntime_memcmp_is_bounded_library_qualified_and_fail_closed() {
    assert_eq!(
        dispatch_win64_import("vcruntime140.dll", "memcmp"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::MemCmp)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "memcmp"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    const MEMCMP: u64 = STUB_BASE + 0x1e0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MEMCMP, "vcruntime140.dll", "memcmp").unwrap();
    assert_eq!(
        engine
            .call_win64(MEMCMP, [0xdead_beef, 0xfeed_face, 0, 0, 0, 0],)
            .unwrap(),
        0
    );
    let error = engine
        .call_win64(
            MEMCMP,
            [DATA_BASE, DATA_BASE, MAX_CRT_MEMORY_COPY_BYTES + 1, 0, 0, 0],
        )
        .unwrap_err();
    assert!(error.to_string().contains("memcmp length"), "{error}");

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MEMCMP, "vcruntime140.dll", "memcmp").unwrap();
    let error = engine
        .call_win64(MEMCMP, [u64::MAX, DATA_BASE, 2, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("left range overflow"), "{error}");

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MEMCMP, "vcruntime140.dll", "memcmp").unwrap();
    let error = engine
        .call_win64(MEMCMP, [DATA_BASE, 0xdead_beef, 1, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("memcmp right read"), "{error}");
}

#[test]
fn stdio_vsnprintf_s_formats_observed_opencv_string_and_signed_integers() {
    const VSNPRINTF: u64 = STUB_BASE + 0x1d0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            VSNPRINTF,
            "API-MS-WIN-CRT-STDIO-L1-1-0.DLL",
            "__stdio_common_vsnprintf_s",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::StdioVsnprintfS)
    );

    let format = DATA_BASE + 0x100;
    let va_list = DATA_BASE + 0x300;
    let output = DATA_BASE + 0x500;
    engine
        .unicorn
        .mem_write(
            format,
            b"OpenCV(%s) %s:%d: error: (%d:%s) %s in function '%s'\n\0",
        )
        .unwrap();
    let strings = [
        (DATA_BASE + 0x700, b"4.10.0".as_slice()),
        (DATA_BASE + 0x740, b"resize.cpp".as_slice()),
        (DATA_BASE + 0x780, b"Bad argument".as_slice()),
        (DATA_BASE + 0x7c0, b"width > 0".as_slice()),
        (DATA_BASE + 0x800, b"resize".as_slice()),
    ];
    for (address, value) in strings {
        let mut terminated = value.to_vec();
        terminated.push(0);
        engine.unicorn.mem_write(address, &terminated).unwrap();
    }
    let values = [
        DATA_BASE + 0x700,
        DATA_BASE + 0x740,
        1737,
        u32::MAX as u64 - 214,
        DATA_BASE + 0x780,
        DATA_BASE + 0x7c0,
        DATA_BASE + 0x800,
    ];
    let va_bytes = values
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect::<Vec<_>>();
    engine.unicorn.mem_write(va_list, &va_bytes).unwrap();

    let written = engine
        .call_win64_with_timeout(
            VSNPRINTF,
            &[0x24, output, 1024, u64::MAX, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    let expected = b"OpenCV(4.10.0) resize.cpp:1737: error: (-215:Bad argument) width > 0 in function 'resize'\n\0";
    assert_eq!(written, (expected.len() - 1) as u64);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(output, expected.len())
            .unwrap(),
        expected
    );
}

#[test]
fn stdio_vsprintf_formats_observed_olm_parameter_name_with_size_max() {
    const VSPRINTF: u64 = STUB_BASE + 0x200;
    let mut engine = test_engine(&[0xc3]);
    for library in ["api-ms-win-crt-stdio-l1-1-0.dll", "ucrtbase.dll"] {
        assert_eq!(
            dispatch_win64_import(library, "__stdio_common_vsprintf"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::StdioVsprintf)
        );
    }
    assert_eq!(
        dispatch_win64_import("fixture.dll", "__stdio_common_vsprintf"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    install_win64_import(
        &mut engine.unicorn,
        VSPRINTF,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "__stdio_common_vsprintf",
    )
    .unwrap();

    let format = DATA_BASE + 0x100;
    let label = DATA_BASE + 0x200;
    let va_list = DATA_BASE + 0x300;
    let output = DATA_BASE + 0x500;
    engine.unicorn.mem_write(format, b"%s %d\0").unwrap();
    engine.unicorn.mem_write(label, b"Threshold\0").unwrap();
    let values = [label, 7u64]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect::<Vec<_>>();
    engine.unicorn.mem_write(va_list, &values).unwrap();

    let written = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[0x25, output, u64::MAX, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_eq!(written, 11);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 12).unwrap(),
        b"Threshold 7\0"
    );

    engine.unicorn.mem_write(output, &[0xa5; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, output, 12, format, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        11
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 12).unwrap(),
        b"Threshold 7\0"
    );

    engine.unicorn.mem_write(output, &[0xa5; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, output, 11, format, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        11
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 12).unwrap(),
        [b"Threshold 7".as_slice(), &[0xa5]].concat()
    );

    engine.unicorn.mem_write(output, &[0xa5; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, output, 8, format, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 9).unwrap(),
        [b"Threshol".as_slice(), &[0xa5]].concat()
    );

    let error = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[0, output, u64::MAX, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("formatting options 0x0"),
        "{error}"
    );
}

#[test]
fn stdio_finite_vsprintf_formats_bang_path_and_validates_complete_guest_ranges() {
    const VSPRINTF: u64 = STUB_BASE + 0x1d0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        VSPRINTF,
        "ucrtbase.dll",
        "__stdio_common_vsprintf",
    )
    .unwrap();
    let format = DATA_BASE + 0x100;
    let root = DATA_BASE + 0x180;
    let suffix = DATA_BASE + 0x1c0;
    let va_list = DATA_BASE + 0x280;
    let output = DATA_BASE + 0x400;
    engine.unicorn.mem_write(format, b"%s\\%s\0").unwrap();
    engine
        .unicorn
        .mem_write(root, b"C:\\ProgramData\0")
        .unwrap();
    engine
        .unicorn
        .mem_write(suffix, b"\\Red Giant\\Common\\Libraries\\RGBranding.dll\0")
        .unwrap();
    engine
        .unicorn
        .mem_write(
            va_list,
            &[root, suffix]
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x2468;
    let expected = b"C:\\ProgramData\\\\Red Giant\\Common\\Libraries\\RGBranding.dll\0";
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, output, 260, format, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        (expected.len() - 1) as u64
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(output, expected.len())
            .unwrap(),
        expected
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x2468);

    engine.unicorn.mem_write(output, b"unchanged\0").unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, output, 0, format, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 10).unwrap(),
        b"unchanged\0"
    );
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, 0, 0, format, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        (expected.len() - 1) as u64
    );

    let boundary = DATA_BASE + PAGE_SIZE - 259;
    engine.unicorn.mem_write(boundary, &[0x5a; 16]).unwrap();
    let error = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[0x25, boundary, 260, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("not writable"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(boundary, 16).unwrap(),
        vec![0x5a; 16]
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.mem_write(format, b"%s\0").unwrap();
    engine.unicorn.mem_write(output, b"%s\0").unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSPRINTF,
                &[0x25, output, 32, output, 0, va_list],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        14
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 15).unwrap(),
        b"C:\\ProgramData\0"
    );
}

#[test]
fn stdio_finite_vsprintf_rejects_unreadable_inputs_and_readonly_destinations_atomically() {
    const VSPRINTF: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        VSPRINTF,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "__stdio_common_vsprintf",
    )
    .unwrap();
    let format = DATA_BASE + 0x100;
    let value = DATA_BASE + 0x180;
    let output = DATA_BASE + 0x300;
    let va_list = HANDLE_DATA_BASE + 0x100;
    engine.unicorn.mem_write(format, b"%s\0").unwrap();
    engine.unicorn.mem_write(value, b"guest\0").unwrap();
    engine
        .unicorn
        .mem_write(va_list, &value.to_le_bytes())
        .unwrap();
    engine.unicorn.mem_write(output, b"unchanged\0").unwrap();

    engine
        .unicorn
        .mem_protect(HANDLE_DATA_BASE, PAGE_SIZE, Prot::WRITE)
        .unwrap();
    let error = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[0x25, output, 32, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("va_list slot"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 10).unwrap(),
        b"unchanged\0"
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .mem_protect(HANDLE_DATA_BASE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
        .unwrap();
    let error = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[0x25, output, 32, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("not writable"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 10).unwrap(),
        b"unchanged\0"
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let error = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[
                0x25,
                output,
                MAX_CRT_STDIO_BUFFER_BYTES + 2,
                format,
                0,
                va_list,
            ],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("buffer count"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 10).unwrap(),
        b"unchanged\0"
    );
}

#[test]
fn stdio_vsnprintf_s_truncates_with_nul_and_negative_one() {
    const VSNPRINTF: u64 = STUB_BASE + 0x1e0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        VSNPRINTF,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "__stdio_common_vsnprintf_s",
    )
    .unwrap();
    let format = DATA_BASE + 0x100;
    let output = DATA_BASE + 0x300;
    engine.unicorn.mem_write(format, b"0123456789\0").unwrap();

    assert_eq!(
        engine
            .call_win64_with_timeout(
                VSNPRINTF,
                &[0x24, output, 8, u64::MAX, format, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        b"0123456\0"
    );
}

#[test]
fn strncpy_s_is_string_library_scoped_and_ucrtbase_compatible() {
    assert_eq!(
        dispatch_win64_import("api-ms-win-crt-string-l1-1-0.dll", "strncpy_s"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::StrncpyS)
    );
    assert_eq!(
        dispatch_win64_import("UCRTBASE.DLL", "strncpy_s"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::StrncpyS)
    );
    assert_eq!(
        dispatch_win64_import("api-ms-win-crt-stdio-l1-1-0.dll", "strncpy_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "strncpy_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn strncpy_s_copies_counted_and_terminated_strings_without_debug_fill() {
    const STRNCPY_S: u64 = STUB_BASE + 0x220;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        STRNCPY_S,
        "api-ms-win-crt-string-l1-1-0.dll",
        "strncpy_s",
    )
    .unwrap();
    let destination = DATA_BASE + 0x100;
    let source = DATA_BASE + 0x200;
    engine.unicorn.mem_write(source, b"abcdef\0").unwrap();
    engine.unicorn.get_data_mut().crt_errno = 91;
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;

    engine.unicorn.mem_write(destination, &[0xcc; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[destination, 8, source, 3],
                TIMEOUT_MICROSECONDS
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
        [b'a', b'b', b'c', 0, 0xcc, 0xcc, 0xcc, 0xcc]
    );

    engine.unicorn.mem_write(destination, &[0xcc; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[destination, 8, source, 7],
                TIMEOUT_MICROSECONDS
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
        [b'a', b'b', b'c', b'd', b'e', b'f', 0, 0xcc]
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 91);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    let guarded_source = DATA_BASE + PAGE_SIZE - 2;
    engine.unicorn.mem_write(guarded_source, b"x\0").unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[destination, 8, guarded_source, u64::MAX],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 2).unwrap(),
        b"x\0"
    );
}

#[test]
fn strncpy_s_truncate_is_nul_terminated_and_preserves_errno() {
    const STRNCPY_S: u64 = STUB_BASE + 0x220;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, STRNCPY_S, "ucrtbase.dll", "strncpy_s").unwrap();
    let destination = DATA_BASE + 0x100;
    let source = DATA_BASE + 0x200;
    engine.unicorn.mem_write(source, b"abcdef\0").unwrap();
    engine.unicorn.mem_write(destination, &[0xcc; 4]).unwrap();
    engine.unicorn.get_data_mut().crt_errno = 73;
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[destination, 4, source, u64::MAX],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        80
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        b"abc\0"
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 73);

    engine.unicorn.mem_write(source, b"abc\0").unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[destination, 4, source, u64::MAX],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        b"abc\0"
    );
}

#[test]
fn strncpy_s_range_and_invalid_parameters_clear_only_the_first_byte() {
    const STRNCPY_S: u64 = STUB_BASE + 0x220;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        STRNCPY_S,
        "api-ms-win-crt-string-l1-1-0.dll",
        "strncpy_s",
    )
    .unwrap();
    let destination = DATA_BASE + 0x100;
    let source = DATA_BASE + 0x200;
    engine.unicorn.mem_write(source, b"abcdef\0").unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x4321;

    for (arguments, expected) in [
        ([destination, 4, source, 4], 34),
        ([destination, 4, 0, 3], 22),
        ([destination, 4, source, (u64::MAX >> 1) + 1], 22),
    ] {
        engine.unicorn.mem_write(destination, &[0xaa; 4]).unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(STRNCPY_S, &arguments, TIMEOUT_MICROSECONDS)
                .unwrap(),
            expected
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
            [0, 0xaa, 0xaa, 0xaa]
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, expected as u32);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x4321);
    }

    engine.unicorn.mem_write(destination, &[0xaa; 4]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(STRNCPY_S, &[0, 4, source, 3], TIMEOUT_MICROSECONDS)
            .unwrap(),
        22
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        [0xaa; 4]
    );
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[destination, 0, source, 3],
                TIMEOUT_MICROSECONDS
            )
            .unwrap(),
        22
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        [0xaa; 4]
    );
}

#[test]
fn strncpy_s_fails_closed_before_mutation_on_bad_memory_and_rejects_overlap() {
    const STRNCPY_S: u64 = STUB_BASE + 0x220;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        STRNCPY_S,
        "api-ms-win-crt-string-l1-1-0.dll",
        "strncpy_s",
    )
    .unwrap();
    let destination = DATA_BASE + 0x100;
    engine.unicorn.mem_write(destination, b"sentinel").unwrap();
    let error = engine
        .call_win64_with_timeout(
            STRNCPY_S,
            &[destination, 8, DATA_BASE + PAGE_SIZE, 4],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("source at"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
        b"sentinel"
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        STRNCPY_S,
        "api-ms-win-crt-string-l1-1-0.dll",
        "strncpy_s",
    )
    .unwrap();
    let overlap = DATA_BASE + 0x180;
    engine.unicorn.mem_write(overlap, b"abcdef\0x").unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                STRNCPY_S,
                &[overlap, 8, overlap + 1, 4],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        22
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(overlap, 8).unwrap(),
        b"\0bcdef\0x"
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        STRNCPY_S,
        "api-ms-win-crt-string-l1-1-0.dll",
        "strncpy_s",
    )
    .unwrap();
    let tail = DATA_BASE + PAGE_SIZE - 4;
    engine.unicorn.mem_write(tail, b"keep").unwrap();
    let error = engine
        .call_win64_with_timeout(
            STRNCPY_S,
            &[tail, 8, DATA_BASE + 0x200, 3],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("not fully writable"), "{error}");
    assert_eq!(engine.unicorn.mem_read_as_vec(tail, 4).unwrap(), b"keep");
}

#[test]
fn fopen_s_is_stdio_library_scoped_and_returns_secure_guest_only_failure() {
    const FOPEN_S: u64 = STUB_BASE + 0x230;
    assert_eq!(
        dispatch_win64_import("api-ms-win-crt-stdio-l1-1-0.dll", "fopen_s"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::FopenS)
    );
    assert_eq!(
        dispatch_win64_import("UCRTBASE.DLL", "fopen_s"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::FopenS)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "fopen_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    let result_pointer = DATA_BASE + 0x100;
    let filename = DATA_BASE + 0x200;
    let mode = DATA_BASE + 0x300;
    engine
        .unicorn
        .mem_write(result_pointer, &0xdead_beef_cafe_babeu64.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(filename, b"C:\\Temp\\esc_gpu.log\0")
        .unwrap();
    engine.unicorn.mem_write(mode, b"a\0").unwrap();

    assert_eq!(
        engine
            .call_win64_with_timeout(
                FOPEN_S,
                &[result_pointer, filename, mode, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        13
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(result_pointer, 8).unwrap(),
        0u64.to_le_bytes()
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 13);
}

#[test]
fn fopen_s_validates_arguments_modes_and_result_atomicity() {
    const FOPEN_S: u64 = STUB_BASE + 0x240;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    let result_pointer = DATA_BASE + 0x100;
    let filename = DATA_BASE + 0x200;
    let mode = DATA_BASE + 0x300;
    engine
        .unicorn
        .mem_write(filename, b"diagnostic.log\0")
        .unwrap();

    for valid in [
        b"r".as_slice(),
        b"wb+",
        b"a+t",
        b"w+x",
        b"w+cNSTD",
        b"r+nRT",
        b"rD+",
        b" r D + ",
        b"wxxNN",
        b"w, ccs=UTF-16LE",
        b"rt+, ccs=UTF-8",
        b"a,ccs=UNICODE",
        b" rt + , ccs = utf-8   ",
    ] {
        engine.unicorn.mem_write(mode, valid).unwrap();
        engine
            .unicorn
            .mem_write(mode + valid.len() as u64, &[0])
            .unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    FOPEN_S,
                    &[result_pointer, filename, mode, 0],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            if valid == b"r" { 2 } else { 13 },
            "valid mode {:?}",
            String::from_utf8_lossy(valid)
        );
    }

    for invalid in [
        b"".as_slice(),
        b"q",
        b"tr",
        b"br",
        b"r++",
        b"rbt",
        b"rbb",
        b"rcc",
        b"rcn",
        b"rSR",
        b"rSS",
        b"rTT",
        b"rDD",
        b"rx",
        b"ax",
        b"r,,D",
        b"r,D",
        b"r,+",
        b"r,b",
        b"r,D,D",
        b"r,Dccs=UTF-8",
        b"r,D ccs=bogus",
        b"w+,DN ccs=UTF-8",
        b"r,ccs=UTF-8D",
        b"r,CCS=UTF-8",
        b"\tr",
        b"r\tD",
        b"r,\tccs=UTF-8",
        b"r,ccs\t=UTF-8",
    ] {
        engine.unicorn.mem_write(mode, invalid).unwrap();
        engine
            .unicorn
            .mem_write(mode + invalid.len() as u64, &[0])
            .unwrap();
        engine
            .unicorn
            .mem_write(result_pointer, &u64::MAX.to_le_bytes())
            .unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    FOPEN_S,
                    &[result_pointer, filename, mode, 0],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            22
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(result_pointer, 8).unwrap(),
            0u64.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 22);
    }

    assert_eq!(
        engine
            .call_win64_with_timeout(FOPEN_S, &[0, filename, mode, 0], TIMEOUT_MICROSECONDS)
            .unwrap(),
        22
    );
    for arguments in [
        [result_pointer, 0, mode, 0],
        [result_pointer, filename, 0, 0],
    ] {
        engine
            .unicorn
            .mem_write(result_pointer, &u64::MAX.to_le_bytes())
            .unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(FOPEN_S, &arguments, TIMEOUT_MICROSECONDS)
                .unwrap(),
            22
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(result_pointer, 8).unwrap(),
            u64::MAX.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 22);
    }

    let other = test_engine(&[0xc3]);
    assert_eq!(other.unicorn.get_data().crt_errno, 0);
}

#[test]
fn fopen_s_errno_is_thread_local_and_preserves_win32_last_error() {
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    const CREATE: u64 = STUB_BASE + 0x260;
    const FOPEN_S: u64 = STUB_BASE + 0x270;
    let result_pointer = DATA_BASE + 0x100;
    let filename = DATA_BASE + 0x200;
    let mode = DATA_BASE + 0x300;
    let mut code = Vec::new();
    push_mov_imm64(&mut code, [0x48, 0xb9], result_pointer); // mov rcx, FILE **
    push_mov_imm64(&mut code, [0x48, 0xba], filename); // mov rdx, filename
    push_mov_imm64(&mut code, [0x49, 0xb8], mode); // mov r8, mode
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]); // sub rsp, 40
    push_mov_imm64(&mut code, [0x48, 0xb8], FOPEN_S); // mov rax, fopen_s
    code.extend_from_slice(&[0xff, 0xd0]); // call rax
    let after_fopen = TEST_CODE + code.len() as u64;
    code.extend_from_slice(&[0x48, 0x83, 0xc4, 0x28, 0xc3]); // add rsp, 40; ret

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut caller_teb_stack = [0u8; 16];
    caller_teb_stack[0..8].copy_from_slice(&(STACK_BASE + STACK_SIZE).to_le_bytes());
    caller_teb_stack[8..16].copy_from_slice(&STACK_BASE.to_le_bytes());
    engine.unicorn.mem_write(0x08, &caller_teb_stack).unwrap();
    install_win64_import(&mut engine.unicorn, CREATE, "kernel32.dll", "CreateThread").unwrap();
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    engine
        .unicorn
        .mem_write(result_pointer, &u64::MAX.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(filename, b"C:\\Temp\\esc_gpu.log\0")
        .unwrap();
    engine.unicorn.mem_write(mode, b"a\0").unwrap();

    let child_entry_errno = Arc::new(AtomicU32::new(u32::MAX));
    let observed = Arc::clone(&child_entry_errno);
    engine
        .unicorn
        .add_code_hook(TEST_CODE, TEST_CODE, move |unicorn, _, _| {
            observed.store(unicorn.get_data().crt_errno, Ordering::SeqCst);
        })
        .unwrap();
    let child_after_errno = Arc::new(AtomicU32::new(u32::MAX));
    let observed = Arc::clone(&child_after_errno);
    engine
        .unicorn
        .add_code_hook(after_fopen, after_fopen, move |unicorn, _, _| {
            observed.store(unicorn.get_data().crt_errno, Ordering::SeqCst);
        })
        .unwrap();

    engine.unicorn.get_data_mut().crt_errno = 77;
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    let handle = engine
        .call_win64_with_timeout(
            CREATE,
            &[0, STACK_SIZE, TEST_CODE, 0, 0x1_0000, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_ne!(handle, 0);
    assert_eq!(child_entry_errno.load(Ordering::SeqCst), 0);
    assert_eq!(child_after_errno.load(Ordering::SeqCst), 13);
    assert_eq!(engine.unicorn.get_data().crt_errno, 77);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(result_pointer, 8).unwrap(),
        0u64.to_le_bytes()
    );
}

#[test]
fn fopen_s_fails_closed_on_unmapped_or_unterminated_guest_memory() {
    const FOPEN_S: u64 = STUB_BASE + 0x250;
    const SENTINEL: u64 = 0x0102_0304_0506_0708;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    let result_pointer = DATA_BASE + 0x100;
    let mode = DATA_BASE + 0x300;
    engine.unicorn.mem_write(mode, b"a\0").unwrap();
    engine
        .unicorn
        .mem_write(result_pointer, &SENTINEL.to_le_bytes())
        .unwrap();

    let error = engine
        .call_win64_with_timeout(
            FOPEN_S,
            &[result_pointer, DATA_BASE + PAGE_SIZE, mode, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fopen_s filename address 0x40001000 is not readable"),
        "{error}"
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(result_pointer, 8).unwrap(),
        SENTINEL.to_le_bytes()
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    engine.unicorn.mem_write(mode, b"a\0").unwrap();
    let error = engine
        .call_win64_with_timeout(
            FOPEN_S,
            &[DATA_BASE + PAGE_SIZE, DATA_BASE + 0x200, mode, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("is not writable"), "{error}");

    const LARGE_STRING: u64 = 0x3000_0000;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    engine
        .unicorn
        .mem_map(
            LARGE_STRING,
            MAX_CRT_STRING_BYTES + PAGE_SIZE,
            Prot::READ | Prot::WRITE,
        )
        .unwrap();
    engine
        .unicorn
        .mem_write(LARGE_STRING, &vec![b'X'; MAX_CRT_STRING_BYTES as usize])
        .unwrap();
    engine.unicorn.mem_write(mode, b"a\0").unwrap();
    engine
        .unicorn
        .mem_write(result_pointer, &SENTINEL.to_le_bytes())
        .unwrap();
    let error = engine
        .call_win64_with_timeout(
            FOPEN_S,
            &[result_pointer, LARGE_STRING, mode, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fopen_s filename exceeds 1048576 bytes"),
        "{error}"
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(result_pointer, 8).unwrap(),
        SENTINEL.to_le_bytes()
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FOPEN_S,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "fopen_s",
    )
    .unwrap();
    let error = engine
        .call_win64_with_timeout(
            FOPEN_S,
            &[u64::MAX, DATA_BASE + 0x200, DATA_BASE + 0x300, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("is not writable"), "{error}");
}

#[test]
fn stdio_vsnprintf_s_is_library_qualified_bounded_and_fail_closed() {
    assert_eq!(
        dispatch_win64_import("fixture.dll", "__stdio_common_vsnprintf_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    const VSNPRINTF: u64 = STUB_BASE + 0x1f0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        VSNPRINTF,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "__stdio_common_vsnprintf_s",
    )
    .unwrap();
    let format = DATA_BASE + 0x100;
    let output = DATA_BASE + 0x300;
    engine
        .unicorn
        .mem_write(format, b"unsupported=%f\0")
        .unwrap();
    engine.unicorn.mem_write(output, b"unchanged\0").unwrap();

    let error = engine
        .call_win64_with_timeout(
            VSNPRINTF,
            &[0x24, output, 32, u64::MAX, format, 0, DATA_BASE + 0x500],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("unsupported conversion '%f'"),
        "{error}"
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 10).unwrap(),
        b"unchanged\0"
    );

    engine.unicorn.get_data_mut().callback_error = None;
    let error = engine
        .call_win64_with_timeout(
            VSNPRINTF,
            &[
                0x24,
                output,
                MAX_CRT_STDIO_BUFFER_BYTES + 1,
                u64::MAX,
                format,
                0,
                DATA_BASE + 0x500,
            ],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("stdio buffer count"), "{error}");
}

#[test]
fn msvcp_mutex_lifecycle_is_recursive_serial_and_session_local() {
    const INIT: u64 = STUB_BASE + 0x210;
    const LOCK: u64 = STUB_BASE + 0x220;
    const UNLOCK: u64 = STUB_BASE + 0x230;
    const DESTROY: u64 = STUB_BASE + 0x240;
    const HARDWARE: u64 = STUB_BASE + 0x250;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol) in [
        (INIT, "_Mtx_init_in_situ"),
        (LOCK, "_Mtx_lock"),
        (UNLOCK, "_Mtx_unlock"),
        (DESTROY, "_Mtx_destroy_in_situ"),
        (HARDWARE, "_Thrd_hardware_concurrency"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, "MSVCP140.DLL", symbol).unwrap();
    }
    let object = DATA_BASE + 0x900;

    assert_eq!(
        engine
            .call_win64(INIT, [object, OBSERVED_MSVCP_MUTEX_TYPE as u64, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.call_win64(LOCK, [object, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.call_win64(LOCK, [object, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().msvcp_mutexes.get(&object),
        Some(&MsvcpMutex {
            mutex_type: OBSERVED_MSVCP_MUTEX_TYPE,
            owner_thread_id: Some(1),
            lock_count: 2,
        })
    );
    assert_eq!(
        engine.call_win64(UNLOCK, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(UNLOCK, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(DESTROY, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert!(engine.unicorn.get_data().msvcp_mutexes.is_empty());
    assert_eq!(engine.call_win64(HARDWARE, [0; 6]).unwrap(), 1);
    assert!(
        test_engine(&[0xc3])
            .unicorn
            .get_data()
            .msvcp_mutexes
            .is_empty()
    );
}

#[test]
fn msvcp_mutex_rejects_invalid_and_unbalanced_lifecycle() {
    const INIT: u64 = STUB_BASE + 0x260;
    const LOCK: u64 = STUB_BASE + 0x270;
    const UNLOCK: u64 = STUB_BASE + 0x280;
    const DESTROY: u64 = STUB_BASE + 0x290;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol) in [
        (INIT, "_Mtx_init_in_situ"),
        (LOCK, "_Mtx_lock"),
        (UNLOCK, "_Mtx_unlock"),
        (DESTROY, "_Mtx_destroy_in_situ"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, "msvcp140.dll", symbol).unwrap();
    }
    let object = DATA_BASE + 0xa00;

    let error = engine
        .call_win64(INIT, [object, 1, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("mutex type"), "{error}");
    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .call_win64(INIT, [object, OBSERVED_MSVCP_MUTEX_TYPE as u64, 0, 0, 0, 0])
        .unwrap();

    let error = engine
        .call_win64(INIT, [object, OBSERVED_MSVCP_MUTEX_TYPE as u64, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("already initialized"), "{error}");
    engine.unicorn.get_data_mut().callback_error = None;
    let error = engine
        .call_win64(UNLOCK, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("unbalanced"), "{error}");
    engine.unicorn.get_data_mut().callback_error = None;
    engine.call_win64(LOCK, [object, 0, 0, 0, 0, 0]).unwrap();
    let error = engine
        .call_win64(DESTROY, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("lock count 1"), "{error}");
    engine.unicorn.get_data_mut().callback_error = None;
    let foreign = DATA_BASE + 0xb00;
    let error = engine
        .call_win64(LOCK, [foreign, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not initialized"), "{error}");

    engine.unicorn.get_data_mut().callback_error = None;
    engine.call_win64(UNLOCK, [object, 0, 0, 0, 0, 0]).unwrap();
    engine.call_win64(DESTROY, [object, 0, 0, 0, 0, 0]).unwrap();
    let error = engine
        .call_win64(DESTROY, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not initialized"), "{error}");

    engine.unicorn.get_data_mut().callback_error = None;
    let error = engine
        .call_win64(INIT, [0, OBSERVED_MSVCP_MUTEX_TYPE as u64, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("object is null"), "{error}");
}

#[test]
fn msvcp_mutex_is_library_qualified_and_bounded() {
    for symbol in [
        "_Mtx_init_in_situ",
        "_Mtx_lock",
        "_Mtx_unlock",
        "_Mtx_destroy_in_situ",
        "_Thrd_hardware_concurrency",
    ] {
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    const INIT: u64 = STUB_BASE + 0x2a0;
    const LOCK: u64 = STUB_BASE + 0x2b0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        INIT,
        "msvcp140.dll",
        "_Mtx_init_in_situ",
    )
    .unwrap();
    install_win64_import(&mut engine.unicorn, LOCK, "msvcp140.dll", "_Mtx_lock").unwrap();
    let object = DATA_BASE + 0xc00;
    for index in 0..MAX_MSVCP_MUTEXES {
        engine.unicorn.get_data_mut().msvcp_mutexes.insert(
            0x1000 + index as u64,
            MsvcpMutex {
                mutex_type: OBSERVED_MSVCP_MUTEX_TYPE,
                owner_thread_id: None,
                lock_count: 0,
            },
        );
    }
    let error = engine
        .call_win64(INIT, [object, OBSERVED_MSVCP_MUTEX_TYPE as u64, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("mutex count"), "{error}");

    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.get_data_mut().msvcp_mutexes.clear();
    engine.unicorn.get_data_mut().msvcp_mutexes.insert(
        object,
        MsvcpMutex {
            mutex_type: OBSERVED_MSVCP_MUTEX_TYPE,
            owner_thread_id: Some(1),
            lock_count: MAX_MSVCP_MUTEX_RECURSION,
        },
    );
    let error = engine
        .call_win64(LOCK, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("recursion"), "{error}");
}

#[test]
fn msvcp_try_mutex_type_two_is_nonrecursive_and_guest_thread_owned() {
    const INIT: u64 = STUB_BASE + 0x2c0;
    const LOCK: u64 = STUB_BASE + 0x2d0;
    const UNLOCK: u64 = STUB_BASE + 0x2e0;
    const DESTROY: u64 = STUB_BASE + 0x2f0;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol) in [
        (INIT, "_Mtx_init_in_situ"),
        (LOCK, "_Mtx_lock"),
        (UNLOCK, "_Mtx_unlock"),
        (DESTROY, "_Mtx_destroy_in_situ"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, "msvcp140.dll", symbol).unwrap();
    }
    let object = DATA_BASE + 0xd00;

    assert_eq!(
        engine
            .call_win64(INIT, [object, MSVCP_MUTEX_TRY as u64, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.call_win64(LOCK, [object, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.call_win64(LOCK, [object, 0, 0, 0, 0, 0]).unwrap(),
        MSVCP_THRD_BUSY as u64
    );
    assert_eq!(
        engine.unicorn.get_data().msvcp_mutexes.get(&object),
        Some(&MsvcpMutex {
            mutex_type: MSVCP_MUTEX_TRY,
            owner_thread_id: Some(1),
            lock_count: 1,
        })
    );

    engine.unicorn.get_data_mut().current_windows_thread_id = 2;
    let error = engine
        .call_win64(LOCK, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("would block"), "{error}");
    engine.unicorn.get_data_mut().callback_error = None;
    let error = engine
        .call_win64(UNLOCK, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not owned"), "{error}");
    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.get_data_mut().current_windows_thread_id = 1;
    assert_eq!(
        engine.call_win64(UNLOCK, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(DESTROY, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    let error = engine
        .call_win64(LOCK, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not initialized"), "{error}");

    engine.unicorn.get_data_mut().callback_error = None;
    assert_eq!(
        engine
            .call_win64(INIT, [object, MSVCP_MUTEX_TRY as u64, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(DESTROY, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
}

#[test]
fn msvcp_mutex_requires_a_complete_writable_guest_object_before_state_changes() {
    const INIT: u64 = STUB_BASE + 0x300;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        INIT,
        "msvcp140.dll",
        "_Mtx_init_in_situ",
    )
    .unwrap();
    let boundary = DATA_BASE + PAGE_SIZE - (MSVCP_MUTEX_BYTES as u64 - 1);
    let error = engine
        .call_win64(INIT, [boundary, MSVCP_MUTEX_TRY as u64, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not fully writable"), "{error}");
    assert!(engine.unicorn.get_data().msvcp_mutexes.is_empty());

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
        .unwrap();
    let object = DATA_BASE + 0xe00;
    let error = engine
        .call_win64(INIT, [object, MSVCP_MUTEX_TRY as u64, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not fully writable"), "{error}");
    assert!(engine.unicorn.get_data().msvcp_mutexes.is_empty());

    assert!(
        test_engine(&[0xc3])
            .unicorn
            .get_data()
            .msvcp_mutexes
            .is_empty()
    );
}

#[test]
fn sh_get_folder_path_a_returns_deterministic_observed_guest_paths() {
    const SH_GET_FOLDER_PATH_A: u64 = STUB_BASE + 0x310;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        SH_GET_FOLDER_PATH_A,
        "SHELL32.DLL",
        "SHGetFolderPathA",
    )
    .unwrap();
    let output = DATA_BASE + 0x100;

    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine
            .call_win64(SH_GET_FOLDER_PATH_A, [0, 0x23, 0, 0, output, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 15).unwrap(),
        b"C:\\ProgramData\0"
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    engine.unicorn.mem_write(output, &[0xcc; 32]).unwrap();
    assert_eq!(
        engine
            .call_win64(SH_GET_FOLDER_PATH_A, [0, 0x26, 0, 0, output, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 17).unwrap(),
        b"C:\\Program Files\0"
    );
    let mut other = test_engine(&[0xc3]);
    install_win64_import(
        &mut other.unicorn,
        SH_GET_FOLDER_PATH_A,
        "shell32.dll",
        "SHGetFolderPathA",
    )
    .unwrap();
    assert_eq!(
        other
            .call_win64(SH_GET_FOLDER_PATH_A, [0, 0x23, 0, 0, output, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        other.unicorn.mem_read_as_vec(output, 15).unwrap(),
        b"C:\\ProgramData\0"
    );
    assert_eq!(other.unicorn.get_data().windows_last_error, 0);
}

#[test]
fn sh_get_folder_path_a_rejects_unobserved_inputs_without_mutating_output() {
    const SH_GET_FOLDER_PATH_A: u64 = STUB_BASE + 0x320;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        SH_GET_FOLDER_PATH_A,
        "shell32.dll",
        "SHGetFolderPathA",
    )
    .unwrap();
    let output = DATA_BASE + 0x300;
    for arguments in [
        [1, 0x23, 0, 0, output, 0],
        [0, 0x24, 0, 0, output, 0],
        [0, 0x23, 1, 0, output, 0],
        [0, 0x23, 0, 1, output, 0],
        [0, 0x23, 0, 0, 0, 0],
    ] {
        engine.unicorn.mem_write(output, &[0xa5; 32]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x5678;
        assert_eq!(
            engine.call_win64(SH_GET_FOLDER_PATH_A, arguments).unwrap(),
            HRESULT_E_INVALIDARG as u64
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 32).unwrap(),
            vec![0xa5; 32]
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x5678);
    }

    let boundary = DATA_BASE + PAGE_SIZE - (WINDOWS_MAX_PATH_BYTES - 1);
    engine.unicorn.mem_write(boundary, &[0x6d; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64(SH_GET_FOLDER_PATH_A, [0, 0x23, 0, 0, boundary, 0],)
            .unwrap(),
        HRESULT_E_INVALIDARG as u64
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(boundary, 16).unwrap(),
        vec![0x6d; 16]
    );

    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
        .unwrap();
    assert_eq!(
        engine
            .call_win64(SH_GET_FOLDER_PATH_A, [0, 0x26, 0, 0, output, 0],)
            .unwrap(),
        HRESULT_E_INVALIDARG as u64
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 32).unwrap(),
        vec![0xa5; 32]
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn sh_get_folder_path_a_is_shell32_only() {
    assert_eq!(
        dispatch_win64_import("shell32.dll", "SHGetFolderPathA"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::ShGetFolderPathA)
    );
    for library in ["kernel32.dll", "fixture.dll", "shfolder.dll"] {
        assert_eq!(
            dispatch_win64_import(library, "SHGetFolderPathA"),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
}

#[test]
fn msvcp_exception_ptr_null_lifecycle_is_deterministic_and_library_qualified() {
    const BASE: u64 = STUB_BASE + 0x5b0;
    const SYMBOLS: [(&str, LegacyWin64Import); 6] = [
        (
            "?__ExceptionPtrCreate@@YAXPEAX@Z",
            LegacyWin64Import::MsvcpExceptionPtrCreate,
        ),
        (
            "?__ExceptionPtrCopy@@YAXPEAXPEBX@Z",
            LegacyWin64Import::MsvcpExceptionPtrCopy,
        ),
        (
            "?__ExceptionPtrAssign@@YAXPEAXPEBX@Z",
            LegacyWin64Import::MsvcpExceptionPtrAssign,
        ),
        (
            "?__ExceptionPtrDestroy@@YAXPEAX@Z",
            LegacyWin64Import::MsvcpExceptionPtrDestroy,
        ),
        (
            "?__ExceptionPtrCurrentException@@YAXPEAX@Z",
            LegacyWin64Import::MsvcpExceptionPtrCurrentException,
        ),
        (
            "?__ExceptionPtrRethrow@@YAXPEBX@Z",
            LegacyWin64Import::MsvcpExceptionPtrRethrow,
        ),
    ];
    let mut engine = test_engine(&[0xc3]);
    for (index, (symbol, implementation)) in SYMBOLS.iter().copied().enumerate() {
        let stub = BASE + index as u64 * 0x10;
        assert_eq!(
            install_win64_import(&mut engine.unicorn, stub, "MSVCP140.DLL", symbol).unwrap(),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    let create = BASE;
    let copy = BASE + 0x10;
    let assign = BASE + 0x20;
    let destroy = BASE + 0x30;
    let current = BASE + 0x40;
    let first = DATA_BASE + 0xb00;
    let second = DATA_BASE + 0xb20;
    engine
        .write(first, &[0xaa; MSVCP_EXCEPTION_PTR_BYTES])
        .unwrap();
    engine
        .write(second, &[0xbb; MSVCP_EXCEPTION_PTR_BYTES])
        .unwrap();

    assert_eq!(
        engine.call_win64(create, [first, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(first, 16).unwrap(), [0; 16]);
    assert_eq!(
        engine
            .call_win64(copy, [second, first, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(second, 16).unwrap(), [0; 16]);
    assert_eq!(
        engine
            .call_win64(assign, [first, second, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(current, [first, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(destroy, [first, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(destroy, [second, 0, 0, 0, 0, 0]).unwrap(),
        0
    );

    let other = test_engine(&[0xc3]);
    assert_eq!(
        other
            .unicorn
            .mem_read_as_vec(first, MSVCP_EXCEPTION_PTR_BYTES)
            .unwrap(),
        [0; MSVCP_EXCEPTION_PTR_BYTES]
    );
}

#[test]
fn msvcp_exception_ptr_nonnull_rethrow_and_invalid_memory_fail_closed() {
    let mut engine = test_engine(&[0xc3]);
    let object = DATA_BASE + 0xb00;
    engine
        .write(object, &[0; MSVCP_EXCEPTION_PTR_BYTES])
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, object).unwrap();
    emulate_msvcp_exception_ptr(
        &mut engine.unicorn,
        LegacyWin64Import::MsvcpExceptionPtrRethrow,
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("bad_exception"))
    );

    let mut engine = test_engine(&[0xc3]);
    engine
        .write(object, &[1; MSVCP_EXCEPTION_PTR_BYTES])
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, object).unwrap();
    emulate_msvcp_exception_ptr(
        &mut engine.unicorn,
        LegacyWin64Import::MsvcpExceptionPtrDestroy,
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("unmodeled non-null"))
    );

    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_msvcp_exception_ptr(
        &mut engine.unicorn,
        LegacyWin64Import::MsvcpExceptionPtrDestroy,
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("read failed"))
    );
}

#[test]
fn vcruntime_exception_copy_shares_unowned_data_and_destroy_is_idempotent() {
    const COPY: u64 = STUB_BASE + 0x2c0;
    const DESTROY: u64 = STUB_BASE + 0x2d0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            COPY,
            "VCRUNTIME140.DLL",
            "__std_exception_copy",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::VcruntimeExceptionCopy)
    );
    install_win64_import(
        &mut engine.unicorn,
        DESTROY,
        "vcruntime140.dll",
        "__std_exception_destroy",
    )
    .unwrap();
    let source = DATA_BASE + 0x100;
    let destination = DATA_BASE + 0x120;
    let message = DATA_BASE + 0x200;
    engine
        .unicorn
        .mem_write(message, b"static error\0")
        .unwrap();
    write_vcruntime_exception_data(
        &mut engine.unicorn,
        source,
        VcruntimeExceptionData {
            what: message,
            do_free: false,
        },
        "test source",
    )
    .unwrap();
    write_vcruntime_exception_data(
        &mut engine.unicorn,
        destination,
        VcruntimeExceptionData {
            what: 0,
            do_free: false,
        },
        "test destination",
    )
    .unwrap();

    assert_eq!(
        engine
            .call_win64(COPY, [source, destination, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        read_vcruntime_exception_data(&engine.unicorn, destination, "test").unwrap(),
        VcruntimeExceptionData {
            what: message,
            do_free: false,
        }
    );
    assert_eq!(
        engine
            .call_win64(DESTROY, [destination, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(DESTROY, [destination, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination, VCRUNTIME_EXCEPTION_DATA_BYTES)
            .unwrap(),
        [0; VCRUNTIME_EXCEPTION_DATA_BYTES]
    );
}

#[test]
fn vcruntime_exception_copy_deep_copies_and_frees_owned_crt_string() {
    const COPY: u64 = STUB_BASE + 0x2e0;
    const DESTROY: u64 = STUB_BASE + 0x2f0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        COPY,
        "vcruntime140.dll",
        "__std_exception_copy",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        DESTROY,
        "vcruntime140.dll",
        "__std_exception_destroy",
    )
    .unwrap();
    let source = DATA_BASE + 0x300;
    let destination = DATA_BASE + 0x320;
    let message = b"owned exception\0";
    let owned = allocate_crt_region(&mut engine.unicorn, message.len() as u64).unwrap();
    engine.unicorn.mem_write(owned, message).unwrap();
    write_vcruntime_exception_data(
        &mut engine.unicorn,
        source,
        VcruntimeExceptionData {
            what: owned,
            do_free: true,
        },
        "test source",
    )
    .unwrap();
    engine
        .unicorn
        .mem_write(destination, &[0; VCRUNTIME_EXCEPTION_DATA_BYTES])
        .unwrap();

    engine
        .call_win64(COPY, [source, destination, 0, 0, 0, 0])
        .unwrap();
    let copied = read_vcruntime_exception_data(&engine.unicorn, destination, "test").unwrap();
    assert!(copied.do_free);
    assert_ne!(copied.what, owned);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(copied.what, message.len())
            .unwrap(),
        message
    );
    assert_eq!(
        engine.unicorn.get_data().crt_heap.live_bytes(),
        (message.len() * 2) as u64
    );

    engine
        .call_win64(DESTROY, [destination, 0, 0, 0, 0, 0])
        .unwrap();
    engine.call_win64(DESTROY, [source, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);
    assert!(engine.unicorn.mem_read_as_vec(owned, 1).is_err());
    assert!(engine.unicorn.mem_read_as_vec(copied.what, 1).is_err());
}

#[test]
fn vcruntime_exception_data_is_library_qualified_and_fails_closed() {
    for symbol in ["__std_exception_copy", "__std_exception_destroy"] {
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
    const COPY: u64 = STUB_BASE + 0x300;
    const DESTROY: u64 = STUB_BASE + 0x310;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        COPY,
        "vcruntime140.dll",
        "__std_exception_copy",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        DESTROY,
        "vcruntime140.dll",
        "__std_exception_destroy",
    )
    .unwrap();
    let source = DATA_BASE + 0x400;
    let destination = DATA_BASE + 0x420;
    let mut malformed = [0u8; VCRUNTIME_EXCEPTION_DATA_BYTES];
    malformed[8] = 2;
    engine.unicorn.mem_write(source, &malformed).unwrap();
    let error = engine
        .call_win64(DESTROY, [source, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("ownership flag"), "{error}");

    engine.unicorn.get_data_mut().callback_error = None;
    write_vcruntime_exception_data(
        &mut engine.unicorn,
        source,
        VcruntimeExceptionData {
            what: DATA_BASE + 0x500,
            do_free: true,
        },
        "test foreign",
    )
    .unwrap();
    let error = engine
        .call_win64(DESTROY, [source, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("not a live CRT allocation"),
        "{error}"
    );

    engine.unicorn.get_data_mut().callback_error = None;
    let unterminated = allocate_crt_region(&mut engine.unicorn, 4).unwrap();
    engine.unicorn.mem_write(unterminated, b"abcd").unwrap();
    write_vcruntime_exception_data(
        &mut engine.unicorn,
        source,
        VcruntimeExceptionData {
            what: unterminated,
            do_free: true,
        },
        "test unterminated",
    )
    .unwrap();
    engine
        .unicorn
        .mem_write(destination, &[0; VCRUNTIME_EXCEPTION_DATA_BYTES])
        .unwrap();
    let error = engine
        .call_win64(COPY, [source, destination, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("exceeds 4 bytes"), "{error}");
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(destination, VCRUNTIME_EXCEPTION_DATA_BYTES)
            .unwrap(),
        [0; VCRUNTIME_EXCEPTION_DATA_BYTES]
    );
}

fn write_initializer_fixture(
    engine: &mut GuestEngine<'static>,
    function: u64,
    marker: u64,
    value: u8,
    returned: u32,
) {
    let mut code = vec![0x48, 0xb8]; // mov rax, marker
    code.extend_from_slice(&marker.to_le_bytes());
    code.extend_from_slice(&[0xc6, 0x00, value]); // mov byte ptr [rax], value
    code.push(0xb8); // mov eax, returned
    code.extend_from_slice(&returned.to_le_bytes());
    code.push(0xc3);
    engine.write(function, &code).unwrap();
}

#[test]
fn crt_initterm_executes_non_null_initializers_in_order() {
    const INITTERM: u64 = STUB_BASE + 0x350;
    let mut engine = test_engine(&vec![0x90; 0x400]);
    engine.unicorn.get_data_mut().image_executable_ranges =
        vec![(TEST_CODE, TEST_CODE + PAGE_SIZE)];
    install_win64_import(
        &mut engine.unicorn,
        INITTERM,
        "api-ms-win-crt-runtime-l1-1-0.dll",
        "_initterm",
    )
    .unwrap();
    let first = TEST_CODE + 0x100;
    let second = TEST_CODE + 0x140;
    let marker = DATA_BASE + 0x700;
    write_initializer_fixture(&mut engine, first, marker, 1, 99);
    write_initializer_fixture(&mut engine, second, marker, 2, 88);
    let table = DATA_BASE + 0x600;
    let mut entries = Vec::new();
    for function in [first, 0, second] {
        entries.extend_from_slice(&function.to_le_bytes());
    }
    engine.write(table, &entries).unwrap();

    assert_eq!(
        engine
            .call_win64(INITTERM, [table, table + entries.len() as u64, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(marker, 1).unwrap(), [2]);
    assert!(engine.unicorn.get_data().pending_crt_initterm.is_none());
}

#[test]
fn process_attach_runs_tls_callbacks_in_order_before_dll_entry() {
    let mut engine = test_engine(&vec![0x90; 0x400]);
    engine.unicorn.get_data_mut().image_executable_ranges =
        vec![(TEST_CODE, TEST_CODE + PAGE_SIZE)];
    let first = TEST_CODE + 0x100;
    let second = TEST_CODE + 0x140;
    let dll_entry = TEST_CODE + 0x180;
    let marker = DATA_BASE + 0x700;
    write_initializer_fixture(&mut engine, first, marker, 1, 0);
    write_initializer_fixture(&mut engine, second, marker, 2, 0);
    write_initializer_fixture(&mut engine, dll_entry, marker, 3, 1);

    engine
        .run_process_attach_addresses(engine.image_base, &[first, second], Some(dll_entry))
        .unwrap();

    assert_eq!(engine.unicorn.mem_read_as_vec(marker, 1).unwrap(), [3]);
}

#[test]
fn static_tls_installs_template_index_and_teb_pointer_array() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let index_address = DATA_BASE + 0x800;
    engine.write(index_address, &[0xff; 4]).unwrap();
    let tls = StaticTlsImage {
        bytes: vec![0x80, 0xff, 0xff, 0xff, 0, 0],
        index_address,
    };

    let next = initialize_static_tls_image(&mut engine.unicorn, Some(&tls)).unwrap();

    assert_eq!(
        engine.unicorn.mem_read_as_vec(DATA_BASE, 6).unwrap(),
        tls.bytes
    );
    let pointer_array = DATA_BASE + 8;
    assert_eq!(
        engine.unicorn.mem_read_as_vec(pointer_array, 8).unwrap(),
        DATA_BASE.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(0x58, 8).unwrap(),
        pointer_array.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(index_address, 4).unwrap(),
        0u32.to_le_bytes()
    );
    assert_eq!(next, DATA_BASE + 16);
}

#[test]
fn process_attach_preserves_tls_callback_failure_context() {
    let mut engine = test_engine(&[0xc3]);
    let callback = DATA_BASE + 0x700;
    let error = engine
        .run_process_attach_addresses(engine.image_base, &[callback], None)
        .unwrap_err();
    assert!(matches!(
        &error,
        GuestError::TlsProcessAttach {
            index: 1,
            address,
            ..
        } if *address == callback
    ));
    assert_eq!(error.diagnostic_category(), "dllmain");
}

#[test]
fn crt_initterm_e_stops_at_first_nonzero_initializer() {
    const INITTERM_E: u64 = STUB_BASE + 0x360;
    let mut engine = test_engine(&vec![0x90; 0x400]);
    engine.unicorn.get_data_mut().image_executable_ranges =
        vec![(TEST_CODE, TEST_CODE + PAGE_SIZE)];
    install_win64_import(
        &mut engine.unicorn,
        INITTERM_E,
        "api-ms-win-crt-runtime-l1-1-0.dll",
        "_initterm_e",
    )
    .unwrap();
    let first = TEST_CODE + 0x180;
    let second = TEST_CODE + 0x1c0;
    let first_marker = DATA_BASE + 0x710;
    let second_marker = DATA_BASE + 0x711;
    write_initializer_fixture(&mut engine, first, first_marker, 1, 7);
    write_initializer_fixture(&mut engine, second, second_marker, 1, 0);
    let table = DATA_BASE + 0x650;
    let mut entries = Vec::new();
    for function in [first, second] {
        entries.extend_from_slice(&function.to_le_bytes());
    }
    engine.write(table, &entries).unwrap();

    assert_eq!(
        engine
            .call_win64(
                INITTERM_E,
                [table, table + entries.len() as u64, 0, 0, 0, 0],
            )
            .unwrap(),
        7
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(first_marker, 2).unwrap(),
        [1, 0]
    );
    assert!(engine.unicorn.get_data().pending_crt_initterm.is_none());
}

#[test]
fn crt_initterm_is_library_qualified_bounded_and_fail_closed() {
    assert_eq!(
        dispatch_win64_import("fixture.dll", "_initterm"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "_initterm_e"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    const INITTERM: u64 = STUB_BASE + 0x370;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        INITTERM,
        "api-ms-win-crt-runtime-l1-1-0.dll",
        "_initterm",
    )
    .unwrap();
    let table = DATA_BASE + 0x680;
    engine
        .write(table, &(DATA_BASE + 0x800).to_le_bytes())
        .unwrap();
    let error = engine
        .call_win64(INITTERM, [table, table + 8, 0, 0, 0, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("outside the executable image"),
        "{error}"
    );
}

#[test]
fn crt_onexit_registers_and_executes_callbacks_in_reverse_order() {
    const INITIALIZE: u64 = STUB_BASE + 0x380;
    const REGISTER: u64 = STUB_BASE + 0x390;
    const EXECUTE: u64 = STUB_BASE + 0x3a0;
    let mut engine = test_engine(&vec![0x90; 0x400]);
    engine.unicorn.get_data_mut().image_executable_ranges =
        vec![(TEST_CODE, TEST_CODE + PAGE_SIZE)];
    for (stub, symbol) in [
        (INITIALIZE, "_initialize_onexit_table"),
        (REGISTER, "_register_onexit_function"),
        (EXECUTE, "_execute_onexit_table"),
    ] {
        install_win64_import(
            &mut engine.unicorn,
            stub,
            "api-ms-win-crt-runtime-l1-1-0.dll",
            symbol,
        )
        .unwrap();
    }
    let first = TEST_CODE + 0x240;
    let second = TEST_CODE + 0x280;
    let marker = DATA_BASE + 0x780;
    let table = DATA_BASE + 0x7a0;
    write_initializer_fixture(&mut engine, first, marker, 1, 0);
    write_initializer_fixture(&mut engine, second, marker, 2, 0);

    assert_eq!(
        engine
            .call_win64(INITIALIZE, [table, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(REGISTER, [table, first, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(REGISTER, [table, second, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(EXECUTE, [table, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(marker, 1).unwrap(), [1]);
    assert!(engine.unicorn.get_data().crt_onexit_tables.is_empty());
    let error = engine
        .call_win64(EXECUTE, [table, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not initialized"), "{error}");
}

#[test]
fn crt_set_terminate_is_library_qualified_stateful_validated_and_session_local() {
    const SET_TERMINATE: u64 = STUB_BASE + 0x430;
    let runtime = "api-ms-win-crt-runtime-l1-1-0.dll";
    assert_eq!(
        dispatch_win64_import(runtime, "set_terminate"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CrtSetTerminate)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "set_terminate"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let mut first = test_engine(&[0xc3, 0xc3]);
    first.unicorn.mem_write(SET_TERMINATE, &[0xc3]).unwrap();
    install_win64_import(&mut first.unicorn, SET_TERMINATE, runtime, "set_terminate").unwrap();
    let handler = TEST_CODE + 1;
    assert_eq!(
        first
            .call_win64(SET_TERMINATE, [handler, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        first
            .call_win64(SET_TERMINATE, [TEST_CODE, 0, 0, 0, 0, 0])
            .unwrap(),
        handler
    );
    assert_eq!(
        first.call_win64(SET_TERMINATE, [0, 0, 0, 0, 0, 0]).unwrap(),
        TEST_CODE
    );

    let error = first
        .call_win64(SET_TERMINATE, [DATA_BASE, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("outside the executable image"),
        "{error}"
    );
    assert_eq!(first.unicorn.get_data().crt_terminate_handler, 0);

    let second = test_engine(&[0xc3]);
    assert_eq!(second.unicorn.get_data().crt_terminate_handler, 0);
}

#[test]
fn windows_critical_section_is_recursive_bounded_and_library_qualified() {
    let operations = [
        (STUB_BASE + 0x3b0, "InitializeCriticalSectionAndSpinCount"),
        (STUB_BASE + 0x3c0, "EnterCriticalSection"),
        (STUB_BASE + 0x3d0, "LeaveCriticalSection"),
        (STUB_BASE + 0x3e0, "DeleteCriticalSection"),
    ];
    for (_, symbol) in operations {
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol) in operations {
        install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap();
    }
    let object = DATA_BASE + 0x800;
    assert_eq!(
        engine
            .call_win64(operations[0].0, [object, 4000, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    engine
        .call_win64(operations[1].0, [object, 0, 0, 0, 0, 0])
        .unwrap();
    engine
        .call_win64(operations[1].0, [object, 0, 0, 0, 0, 0])
        .unwrap();
    engine
        .call_win64(operations[2].0, [object, 0, 0, 0, 0, 0])
        .unwrap();
    engine
        .call_win64(operations[2].0, [object, 0, 0, 0, 0, 0])
        .unwrap();
    engine
        .call_win64(operations[3].0, [object, 0, 0, 0, 0, 0])
        .unwrap();
    let error = engine
        .call_win64(operations[2].0, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not initialized"), "{error}");
}

#[test]
fn initialize_critical_section_ex_accepts_observed_spin_and_shares_recursive_lifecycle() {
    const INIT_EX: u64 = STUB_BASE + 0x3a0;
    const ENTER: u64 = STUB_BASE + 0x3b0;
    const LEAVE: u64 = STUB_BASE + 0x3c0;
    const DELETE: u64 = STUB_BASE + 0x3d0;
    const NO_DEBUG_INFO: u64 = 0x0100_0000;
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "InitializeCriticalSectionEx"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::InitializeCriticalSectionEx)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "InitializeCriticalSectionEx"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let operations = [
        (INIT_EX, "InitializeCriticalSectionEx"),
        (ENTER, "EnterCriticalSection"),
        (LEAVE, "LeaveCriticalSection"),
        (DELETE, "DeleteCriticalSection"),
    ];
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol) in operations {
        install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap();
    }
    let object = DATA_BASE + 0x900;
    engine
        .write(object, &[0x5a; WINDOWS_CRITICAL_SECTION_BYTES])
        .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine
            .call_win64(INIT_EX, [object, 4000, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    let mut initialized = [0xff; WINDOWS_CRITICAL_SECTION_BYTES];
    engine.read(object, &mut initialized).unwrap();
    assert_eq!(initialized, [0; WINDOWS_CRITICAL_SECTION_BYTES]);
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_critical_sections
            .get(&object),
        Some(&0)
    );

    for _ in 0..2 {
        assert_eq!(
            engine.call_win64(ENTER, [object, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
    }
    for _ in 0..2 {
        assert_eq!(
            engine.call_win64(LEAVE, [object, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
    }
    assert_eq!(
        engine.call_win64(DELETE, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert!(
        !engine
            .unicorn
            .get_data()
            .windows_critical_sections
            .contains_key(&object)
    );

    engine
        .write(object, &[0xa5; WINDOWS_CRITICAL_SECTION_BYTES])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(
                INIT_EX,
                [object, u64::from(u32::MAX), NO_DEBUG_INFO, 0, 0, 0],
            )
            .unwrap(),
        1
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_critical_sections
            .get(&object),
        Some(&0)
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn initialize_critical_section_ex_rejects_invalid_inputs_without_partial_initialization() {
    const INIT_EX: u64 = STUB_BASE + 0x3a0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        INIT_EX,
        "kernel32.dll",
        "InitializeCriticalSectionEx",
    )
    .unwrap();
    let object = DATA_BASE + 0xa00;
    let sentinel = [0x5a; WINDOWS_CRITICAL_SECTION_BYTES];

    for (pointer, flags) in [
        (0, 0),
        (0xdead_beef, 0),
        (DATA_BASE + PAGE_SIZE - 20, 0),
        (object, 1),
        (object, 0x0200_0000),
    ] {
        engine.write(object, &sentinel).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x9999;
        assert_eq!(
            engine
                .call_win64(INIT_EX, [pointer, 4000, flags, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        let mut actual = [0; WINDOWS_CRITICAL_SECTION_BYTES];
        engine.read(object, &mut actual).unwrap();
        assert_eq!(actual, sentinel);
        assert!(
            !engine
                .unicorn
                .get_data()
                .windows_critical_sections
                .contains_key(&pointer)
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }

    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
        .unwrap();
    assert_eq!(
        engine.call_win64(INIT_EX, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    let mut actual = [0; WINDOWS_CRITICAL_SECTION_BYTES];
    engine.read(object, &mut actual).unwrap();
    assert_eq!(actual, sentinel);
}

#[test]
fn initialize_critical_section_ex_reinit_capacity_and_session_state_are_bounded() {
    const INIT_EX: u64 = STUB_BASE + 0x3a0;
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    let object = DATA_BASE + 0xb00;
    let mut first = test_engine(&[0xc3]);
    install_win64_import(
        &mut first.unicorn,
        INIT_EX,
        "kernel32.dll",
        "InitializeCriticalSectionEx",
    )
    .unwrap();
    assert_eq!(
        first.call_win64(INIT_EX, [object, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        first.call_win64(INIT_EX, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        first.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(
        first
            .unicorn
            .get_data()
            .windows_critical_sections
            .get(&object),
        Some(&0)
    );

    first
        .unicorn
        .get_data_mut()
        .windows_critical_sections
        .clear();
    for index in 0..MAX_WINDOWS_CRITICAL_SECTIONS {
        first
            .unicorn
            .get_data_mut()
            .windows_critical_sections
            .insert(0x2000_0000 + index as u64 * 0x40, 0);
    }
    first
        .write(object, &[0xa5; WINDOWS_CRITICAL_SECTION_BYTES])
        .unwrap();
    assert_eq!(
        first.call_win64(INIT_EX, [object, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        first.unicorn.get_data().windows_last_error,
        ERROR_NOT_ENOUGH_MEMORY
    );
    let mut actual = [0; WINDOWS_CRITICAL_SECTION_BYTES];
    first.read(object, &mut actual).unwrap();
    assert_eq!(actual, [0xa5; WINDOWS_CRITICAL_SECTION_BYTES]);

    let mut second = test_engine(&[0xc3]);
    install_win64_import(
        &mut second.unicorn,
        INIT_EX,
        "kernel32.dll",
        "InitializeCriticalSectionEx",
    )
    .unwrap();
    assert_eq!(
        second.call_win64(INIT_EX, [object, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        second
            .unicorn
            .get_data()
            .windows_critical_sections
            .get(&object),
        Some(&0)
    );
    assert!(second.unicorn.get_data().callback_error.is_none());
}

#[test]
fn srw_exclusive_zero_initializes_acquires_releases_and_is_session_local() {
    const ACQUIRE: u64 = STUB_BASE + 0x410;
    const RELEASE: u64 = STUB_BASE + 0x420;
    const LOCK: u64 = DATA_BASE + 0x900;
    let prepare = || {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            ACQUIRE,
            "kernel32.dll",
            "AcquireSRWLockExclusive",
        )
        .unwrap();
        install_win64_import(
            &mut engine.unicorn,
            RELEASE,
            "kernel32.dll",
            "ReleaseSRWLockExclusive",
        )
        .unwrap();
        engine
    };
    let mut first = prepare();
    first.write(LOCK, &[0; 8]).unwrap();
    assert_eq!(first.call_win64(ACQUIRE, [LOCK, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        first.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        Some(1)
    );
    assert_eq!(first.call_win64(RELEASE, [LOCK, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        first.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        None
    );
    assert_eq!(first.unicorn.mem_read_as_vec(LOCK, 8).unwrap(), vec![0; 8]);

    let mut second = prepare();
    second.write(LOCK, &[0; 8]).unwrap();
    assert!(
        !second
            .unicorn
            .get_data()
            .windows_srw_locks
            .contains_key(&LOCK)
    );
    assert_eq!(
        second.call_win64(ACQUIRE, [LOCK, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        second.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        Some(1)
    );
}

#[test]
fn srw_exclusive_rejects_storage_recursive_and_unbalanced_misuse() {
    const ACQUIRE: u64 = STUB_BASE + 0x430;
    const RELEASE: u64 = STUB_BASE + 0x440;
    let prepare = || {
        let mut engine = test_engine(&[0xc3]);
        for (address, symbol) in [
            (ACQUIRE, "AcquireSRWLockExclusive"),
            (RELEASE, "ReleaseSRWLockExclusive"),
        ] {
            install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
        }
        engine
    };
    let mut wrong_library = prepare();
    install_win64_import(
        &mut wrong_library.unicorn,
        STUB_BASE + 0x450,
        "fixture.dll",
        "AcquireSRWLockExclusive",
    )
    .unwrap();
    assert!(
        wrong_library
            .call_win64(STUB_BASE + 0x450, [DATA_BASE + 0x908, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );

    let mut null = prepare();
    assert!(
        null.call_win64(ACQUIRE, [0; 6])
            .unwrap_err()
            .to_string()
            .contains("not readable")
    );

    let lock = DATA_BASE + 0x908;
    let mut nonzero = prepare();
    nonzero.write(lock, &2u64.to_le_bytes()).unwrap();
    assert!(
        nonzero
            .call_win64(ACQUIRE, [lock, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("not initialized by zero")
    );

    let mut recursive = prepare();
    recursive.write(lock, &[0; 8]).unwrap();
    recursive
        .call_win64(ACQUIRE, [lock, 0, 0, 0, 0, 0])
        .unwrap();
    assert!(
        recursive
            .call_win64(ACQUIRE, [lock, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("recursive")
    );

    let mut unbalanced = prepare();
    unbalanced.write(lock, &[0; 8]).unwrap();
    assert!(
        unbalanced
            .call_win64(RELEASE, [lock, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("does not own")
    );
}

#[test]
fn srw_exclusive_try_acquire_is_nonblocking_and_never_queues() {
    const TRY_ACQUIRE: u64 = STUB_BASE + 0x450;
    const RELEASE: u64 = STUB_BASE + 0x458;
    const LOCK: u64 = DATA_BASE + 0x910;
    let mut engine = test_engine(&[0xc3]);
    for (address, symbol) in [
        (TRY_ACQUIRE, "TryAcquireSRWLockExclusive"),
        (RELEASE, "ReleaseSRWLockExclusive"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    engine.write(LOCK, &[0; 8]).unwrap();

    assert_eq!(
        engine
            .call_win64(TRY_ACQUIRE, [LOCK, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        Some(1)
    );

    engine.unicorn.get_data_mut().current_windows_thread_id = 2;
    assert_eq!(
        engine
            .call_win64(TRY_ACQUIRE, [LOCK, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        Some(1)
    );
    assert!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK]
            .waiters
            .is_empty()
    );

    engine.unicorn.get_data_mut().current_windows_thread_id = 1;
    assert_eq!(
        engine
            .call_win64(TRY_ACQUIRE, [LOCK, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK]
            .waiters
            .is_empty()
    );
    engine.call_win64(RELEASE, [LOCK, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(
        engine
            .call_win64(TRY_ACQUIRE, [LOCK, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
}

#[test]
fn srw_exclusive_cross_thread_contention_never_false_succeeds() {
    const ACQUIRE: u64 = STUB_BASE + 0x460;
    const LOCK: u64 = DATA_BASE + 0x918;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        ACQUIRE,
        "kernel32.dll",
        "AcquireSRWLockExclusive",
    )
    .unwrap();
    engine.write(LOCK, &[0; 8]).unwrap();
    engine.call_win64(ACQUIRE, [LOCK, 0, 0, 0, 0, 0]).unwrap();
    engine.unicorn.get_data_mut().current_windows_thread_id = 2;
    let error = engine
        .call_win64(ACQUIRE, [LOCK, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("SRW lock deadlock"));
    assert_eq!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        Some(1)
    );
    assert_eq!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK]
            .waiters
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![2]
    );
}

#[test]
fn srw_exclusive_scheduler_parks_transfers_and_resumes_a_guest_waiter() {
    const CREATE: u64 = STUB_BASE + 0x470;
    const SWITCH: u64 = STUB_BASE + 0x480;
    const ACQUIRE: u64 = STUB_BASE + 0x490;
    const RELEASE: u64 = STUB_BASE + 0x4a0;
    const CHILD_OFFSET: usize = 0x180;
    const LOCK: u64 = DATA_BASE + 0x920;
    const OUTPUT: u64 = DATA_BASE + 0x930;

    let mut code = vec![0x48, 0x83, 0xec, 0x38]; // sub rsp, 38h
    push_mov_imm64(&mut code, [0x48, 0xb9], LOCK);
    push_mov_imm64(&mut code, [0x48, 0xb8], ACQUIRE);
    code.extend_from_slice(&[0xff, 0xd0]);
    // CreateThread(NULL, 0, child, OUTPUT, 0, NULL).
    code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
    push_mov_imm64(&mut code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
    push_mov_imm64(&mut code, [0x49, 0xb9], OUTPUT);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], CREATE);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x89, 0xc3]); // call; mov rbx,rax
    push_mov_imm64(&mut code, [0x48, 0xb9], LOCK);
    push_mov_imm64(&mut code, [0x48, 0xb8], RELEASE);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x89, 0xd8, 0x48, 0x83, 0xc4, 0x38, 0xc3]);

    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    push_mov_imm64(&mut code, [0x48, 0xb9], LOCK);
    push_mov_imm64(&mut code, [0x48, 0xb8], ACQUIRE);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], OUTPUT);
    code.extend_from_slice(&[0x48, 0xc7, 0x00, 1, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb9], LOCK);
    push_mov_imm64(&mut code, [0x48, 0xb8], RELEASE);
    code.extend_from_slice(&[0xff, 0xd0, 0xb8, 42, 0, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [
        (CREATE, "CreateThread"),
        (SWITCH, "SwitchToThread"),
        (ACQUIRE, "AcquireSRWLockExclusive"),
        (RELEASE, "ReleaseSRWLockExclusive"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    engine.write(LOCK, &[0; 8]).unwrap();
    let handle = engine.call_win64(TEST_CODE, [0; 6]).unwrap();
    assert_eq!(
        engine.unicorn.mem_read_as_vec(OUTPUT, 4).unwrap(),
        1u32.to_le_bytes()
    );
    let child = engine
        .unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .unwrap();
    assert!(child.completed);
    assert_eq!(child.exit_code, 42);
    assert_eq!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK].owner,
        None
    );
    assert!(
        engine.unicorn.get_data().windows_srw_locks[&LOCK]
            .waiters
            .is_empty()
    );
    assert!(engine.scheduled_windows_threads.is_empty());
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.unicorn.get_data().scheduler_woken_threads.is_empty());
    assert_eq!(engine.unicorn.get_data().current_windows_thread_id, 1);
}

#[test]
fn dynamic_condition_variables_are_bounded_and_blocking_fails_closed() {
    const GET_MODULE: u64 = STUB_BASE + 0x3f0;
    const GET_PROC: u64 = STUB_BASE + 0x400;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE,
        "kernel32.dll",
        "GetModuleHandleW",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        GET_PROC,
        "kernel32.dll",
        "GetProcAddress",
    )
    .unwrap();
    let module_name = DATA_BASE + 0x880;
    let mut wide = "kernel32.dll".encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    let wide_bytes = wide
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect::<Vec<_>>();
    engine.write(module_name, &wide_bytes).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_MODULE, [module_name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );

    let name = DATA_BASE + 0x900;
    engine
        .write(name, b"InitializeConditionVariable\0")
        .unwrap();
    let initialize = engine
        .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(initialize, HOST_INITIALIZE_CONDITION_VARIABLE);
    let condition = DATA_BASE + 0x980;
    engine
        .call_win64(initialize, [condition, 0, 0, 0, 0, 0])
        .unwrap();

    engine.write(name, b"WakeAllConditionVariable\0").unwrap();
    let wake = engine
        .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0])
        .unwrap();
    engine.call_win64(wake, [condition, 0, 0, 0, 0, 0]).unwrap();
    let static_condition = DATA_BASE + 0x9c0;
    engine
        .call_win64(wake, [static_condition, 0, 0, 0, 0, 0])
        .unwrap();
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_condition_variables
            .contains(&static_condition)
    );
    let malformed_condition = DATA_BASE + 0x9d0;
    engine.write(malformed_condition, &[1; 8]).unwrap();
    let error = engine
        .call_win64(wake, [malformed_condition, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("zero-initialized"), "{error}");
    let error = engine
        .call_win64(
            HOST_SLEEP_CONDITION_VARIABLE_CS,
            [condition, DATA_BASE + 0xa00, 0, 0, 0, 0],
        )
        .unwrap_err();
    assert!(error.to_string().contains("blocking"), "{error}");
}

#[test]
fn issue1398_wake_by_address_all_is_exact_bounded_and_preserves_void_abi() {
    const WAKE_API_SET: u64 = STUB_BASE + 0x418;
    const WAKE_KERNEL32: u64 = STUB_BASE + 0x420;
    let api_set = "api-ms-win-core-synch-l1-2-0.dll";
    assert_eq!(
        dispatch_win64_import(api_set, "WakeByAddressAll"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WakeByAddressAll)
    );
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "WakeByAddressAll"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WakeByAddressAll)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "WakeByAddressAll"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        WAKE_API_SET,
        api_set,
        "WakeByAddressAll",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        WAKE_KERNEL32,
        "kernel32.dll",
        "WakeByAddressAll",
    )
    .unwrap();
    let first = DATA_BASE + 0xc00;
    let second = DATA_BASE + 0xc08;
    record_windows_address_waiter(engine.unicorn.get_data_mut(), first, 2).unwrap();
    record_windows_address_waiter(engine.unicorn.get_data_mut(), first, 3).unwrap();
    record_windows_address_waiter(engine.unicorn.get_data_mut(), second, 4).unwrap();

    let marker = 0x8877_6655_4433_2211;
    engine.unicorn.reg_write(RegisterX86::RAX, marker).unwrap();
    assert_eq!(
        engine
            .call_win64(WAKE_API_SET, [first, 0, 0, 0, 0, 0])
            .unwrap(),
        marker
    );
    assert!(
        !engine
            .unicorn
            .get_data()
            .windows_address_waiters
            .contains_key(&first)
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_address_waiters
            .get(&second),
        Some(&BTreeSet::from([4]))
    );

    // No waiter means no retained address state and no guest-memory probe,
    // including for NULL and otherwise unmapped pointer values.
    for address in [first, 0, u64::MAX] {
        engine.unicorn.reg_write(RegisterX86::RAX, marker).unwrap();
        assert_eq!(
            engine
                .call_win64(WAKE_KERNEL32, [address, 0, 0, 0, 0, 0])
                .unwrap(),
            marker
        );
    }
    assert_eq!(engine.unicorn.get_data().windows_address_waiters.len(), 1);
}

#[test]
fn issue1398_address_waiter_state_is_bounded_and_session_local() {
    let mut first = test_engine(&[0xc3]);
    let second = test_engine(&[0xc3]);
    let address = DATA_BASE + 0xc80;
    for thread_id in 0..MAX_WINDOWS_ADDRESS_WAITERS_PER_LOCATION as u32 {
        record_windows_address_waiter(first.unicorn.get_data_mut(), address, thread_id).unwrap();
    }
    let error = record_windows_address_waiter(
        first.unicorn.get_data_mut(),
        address,
        MAX_WINDOWS_ADDRESS_WAITERS_PER_LOCATION as u32,
    )
    .unwrap_err();
    assert!(error.contains("waiter count"), "{error}");
    assert!(second.unicorn.get_data().windows_address_waiters.is_empty());

    let mut bounded = test_engine(&[0xc3]);
    for index in 0..MAX_WINDOWS_ADDRESS_WAIT_LOCATIONS {
        record_windows_address_waiter(
            bounded.unicorn.get_data_mut(),
            DATA_BASE + 0x1000 + index as u64 * 8,
            1,
        )
        .unwrap();
    }
    let error = record_windows_address_waiter(
        bounded.unicorn.get_data_mut(),
        DATA_BASE + 0x1000 + MAX_WINDOWS_ADDRESS_WAIT_LOCATIONS as u64 * 8,
        1,
    )
    .unwrap_err();
    assert!(error.contains("location count"), "{error}");
}

#[test]
fn output_debug_string_a_is_kernel32_scoped_discards_bounded_ansi_and_preserves_void_abi() {
    const OUTPUT_DEBUG: u64 = STUB_BASE + 0x428;
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "OutputDebugStringA"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::OutputDebugStringA)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "OutputDebugStringA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        OUTPUT_DEBUG,
        "KERNEL32.DLL",
        "OutputDebugStringA",
    )
    .unwrap();
    let message = DATA_BASE + 0xd00;
    engine
        .write(message, b"[fixture] debug bytes are guest-private\n\0")
        .unwrap();
    let marker = 0x8877_6655_4433_2211;
    for pointer in [message, message + 40, 0] {
        engine.unicorn.reg_write(RegisterX86::RAX, marker).unwrap();
        assert_eq!(
            engine
                .call_win64(OUTPUT_DEBUG, [pointer, 0, 0, 0, 0, 0])
                .unwrap(),
            marker
        );
    }
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn output_debug_string_a_fails_closed_on_unreadable_or_unterminated_non_null_input() {
    const OUTPUT_DEBUG: u64 = STUB_BASE + 0x428;
    const LARGE_STRING: u64 = 0x5000_0000;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        OUTPUT_DEBUG,
        "kernel32.dll",
        "OutputDebugStringA",
    )
    .unwrap();

    for pointer in [0xdead_beef, u64::MAX] {
        let error = engine
            .call_win64(OUTPUT_DEBUG, [pointer, 0, 0, 0, 0, 0])
            .unwrap_err();
        assert!(error.to_string().contains("unreadable"), "{error}");
    }

    engine
        .unicorn
        .mem_map(
            LARGE_STRING,
            MAX_CRT_STRING_BYTES + PAGE_SIZE,
            Prot::READ | Prot::WRITE,
        )
        .unwrap();
    engine
        .unicorn
        .mem_write(
            LARGE_STRING,
            &vec![b'X'; (MAX_CRT_STRING_BYTES + 1) as usize],
        )
        .unwrap();
    engine
        .unicorn
        .mem_write(LARGE_STRING + MAX_CRT_STRING_BYTES, &[0])
        .unwrap();
    let marker = 0x1122_3344_5566_7788;
    engine.unicorn.reg_write(RegisterX86::RAX, marker).unwrap();
    assert_eq!(
        engine
            .call_win64(OUTPUT_DEBUG, [LARGE_STRING, 0, 0, 0, 0, 0])
            .unwrap(),
        marker
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    engine
        .unicorn
        .mem_write(LARGE_STRING + MAX_CRT_STRING_BYTES, &[b'X'])
        .unwrap();
    let error = engine
        .call_win64(OUTPUT_DEBUG, [LARGE_STRING, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("without a terminator"),
        "{error}"
    );
}

#[test]
fn output_debug_string_a_stops_guest_before_post_failure_side_effects() {
    const OUTPUT_DEBUG: u64 = STUB_BASE + 0x428;
    let marker = DATA_BASE + 0xe00;
    let mut code = vec![0x48, 0xb8]; // mov rax, OUTPUT_DEBUG
    code.extend_from_slice(&OUTPUT_DEBUG.to_le_bytes());
    code.extend_from_slice(&[0x48, 0xb9]); // mov rcx, invalid string
    code.extend_from_slice(&0xdead_beefu64.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0]); // call rax
    code.extend_from_slice(&[0x48, 0xba]); // mov rdx, marker
    code.extend_from_slice(&marker.to_le_bytes());
    code.extend_from_slice(&[0xc6, 0x02, 0x5a, 0xc3]); // mov byte [rdx], 0x5a; ret

    let mut engine = test_engine(&code);
    install_win64_import(
        &mut engine.unicorn,
        OUTPUT_DEBUG,
        "kernel32.dll",
        "OutputDebugStringA",
    )
    .unwrap();
    engine.write(marker, &[0]).unwrap();
    let error = engine.call_win64(TEST_CODE, [0; 6]).unwrap_err();
    assert!(error.to_string().contains("unreadable"), "{error}");
    let mut observed = [0xff];
    engine.read(marker, &mut observed).unwrap();
    assert_eq!(observed, [0], "guest continued after the failed import");
}

#[test]
fn issue1402_wait_on_address_sizes_timeouts_pointers_and_scope() {
    const WAIT_API: u64 = STUB_BASE + 0x428;
    const WAIT_KERNEL: u64 = STUB_BASE + 0x430;
    let api_set = "api-ms-win-core-synch-l1-2-0.dll";
    for library in [api_set, "kernel32.dll"] {
        assert_eq!(
            dispatch_win64_import(library, "WaitOnAddress"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WaitOnAddress)
        );
    }
    assert_eq!(
        dispatch_win64_import("fixture.dll", "WaitOnAddress"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, WAIT_API, api_set, "WaitOnAddress").unwrap();
    install_win64_import(
        &mut engine.unicorn,
        WAIT_KERNEL,
        "kernel32.dll",
        "WaitOnAddress",
    )
    .unwrap();
    let address = DATA_BASE + 0xd00;
    let compare = DATA_BASE + 0xd20;
    for size in [1_u64, 2, 4, 8] {
        let current = 0x8877_6655_4433_2211_u64.to_le_bytes();
        let mut different = current;
        different[(size - 1) as usize] ^= 0xff;
        engine.write(address, &current).unwrap();
        engine.write(compare, &different).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x1234;
        assert_eq!(
            engine
                .call_win64(
                    WAIT_API,
                    [address, compare, size, u64::from(u32::MAX), 0, 0],
                )
                .unwrap(),
            1
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

        engine.write(compare, &current).unwrap();
        assert_eq!(
            engine
                .call_win64(WAIT_KERNEL, [address, compare, size, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 1460);
    }
    assert_eq!(
        engine
            .call_win64(WAIT_API, [address, compare, 8, 17, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1460);
    assert!(engine.unicorn.get_data().windows_address_waiters.is_empty());
    for args in [
        [address, compare, 0, 0, 0, 0],
        [address, compare, 3, 0, 0, 0],
        [0, compare, 4, 0, 0, 0],
        [address, 0, 4, 0, 0, 0],
        [DATA_BASE + DATA_SIZE - 3, compare, 4, 0, 0, 0],
        [address, DATA_BASE + DATA_SIZE - 3, 4, 0, 0, 0],
    ] {
        assert_eq!(engine.call_win64(WAIT_API, args).unwrap(), 0);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        assert!(engine.unicorn.get_data().windows_address_waiters.is_empty());
    }
}

fn issue1402_wait_scheduler_fixture(timeout: u32, wake: bool) -> (GuestEngine<'static>, u64) {
    const CREATE: u64 = STUB_BASE + 0x450;
    const WAIT: u64 = STUB_BASE + 0x460;
    const WAKE: u64 = STUB_BASE + 0x470;
    const GET_LAST_ERROR: u64 = STUB_BASE + 0x480;
    const CHILD_OFFSET: usize = 0x180;
    let address = DATA_BASE + 0xd80;
    let compare = DATA_BASE + 0xda0;
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
    push_mov_imm64(&mut code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
    code.extend_from_slice(&[0x45, 0x31, 0xc9]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], CREATE);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x89, 0xc3]);
    if wake {
        push_mov_imm64(&mut code, [0x48, 0xb8], address);
        code.extend_from_slice(&[0xc7, 0x00, 1, 0, 0, 0]);
        push_mov_imm64(&mut code, [0x48, 0xb8], WAKE);
        push_mov_imm64(&mut code, [0x48, 0xb9], address);
        code.extend_from_slice(&[0xff, 0xd0]);
    }
    code.extend_from_slice(&[0x48, 0x89, 0xd8, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    push_mov_imm64(&mut code, [0x48, 0xb9], address);
    push_mov_imm64(&mut code, [0x48, 0xba], compare);
    code.extend_from_slice(&[0x49, 0xc7, 0xc0, 4, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x49, 0xb9], u64::from(timeout));
    push_mov_imm64(&mut code, [0x48, 0xb8], WAIT);
    code.extend_from_slice(&[0xff, 0xd0]);
    if timeout != u32::MAX && !wake {
        push_mov_imm64(&mut code, [0x48, 0xb8], GET_LAST_ERROR);
        code.extend_from_slice(&[0xff, 0xd0]);
    }
    code.extend_from_slice(&[0x48, 0x83, 0xc4, 0x28, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(address, &[0; 8]).unwrap();
    engine.write(compare, &[0; 8]).unwrap();
    for (stub, symbol) in [
        (CREATE, "CreateThread"),
        (WAIT, "WaitOnAddress"),
        (WAKE, "WakeByAddressAll"),
        (GET_LAST_ERROR, "GetLastError"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap();
    }
    let handle = engine.call_win64(TEST_CODE, [0; 6]).unwrap();
    (engine, handle)
}

#[test]
fn issue1402_infinite_and_finite_waiters_run_peer_before_wake_or_timeout() {
    for timeout in [u32::MAX, 25] {
        let (engine, handle) = issue1402_wait_scheduler_fixture(timeout, true);
        let thread = engine
            .unicorn
            .get_data()
            .windows_threads
            .get(&handle)
            .unwrap();
        assert!(thread.completed);
        assert_eq!(thread.exit_code, 1);
        assert!(engine.scheduled_windows_threads.is_empty());
        assert!(engine.scheduler_ready.is_empty());
        assert!(engine.unicorn.get_data().windows_address_waiters.is_empty());
    }
    let (engine, handle) = issue1402_wait_scheduler_fixture(25, false);
    let thread = engine
        .unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .unwrap();
    assert!(thread.completed);
    assert_eq!(thread.exit_code, 1460);
    assert!(engine.scheduled_windows_threads.is_empty());
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.unicorn.get_data().windows_address_waiters.is_empty());

    // A detached/background child may legitimately remain parked after the
    // main call returns; this is not a main-thread deadlock and remains bounded
    // for a later exact wake in the same guest session.
    let (engine, handle) = issue1402_wait_scheduler_fixture(u32::MAX, false);
    assert!(
        !engine
            .unicorn
            .get_data()
            .windows_threads
            .get(&handle)
            .unwrap()
            .completed
    );
    assert_eq!(engine.scheduled_windows_threads.len(), 1);
    assert_eq!(engine.unicorn.get_data().windows_address_waiters.len(), 1);
}

#[test]
fn issue1402_wake_single_is_exact_and_infinite_deadlock_fails_closed() {
    const WAKE_SINGLE: u64 = STUB_BASE + 0x490;
    const WAIT: u64 = STUB_BASE + 0x4a0;
    assert_eq!(
        dispatch_win64_import("api-ms-win-core-synch-l1-2-0.dll", "WakeByAddressSingle"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WakeByAddressSingle)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "WakeByAddressSingle"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        WAKE_SINGLE,
        "api-ms-win-core-synch-l1-2-0.dll",
        "WakeByAddressSingle",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        WAIT,
        "api-ms-win-core-synch-l1-2-0.dll",
        "WaitOnAddress",
    )
    .unwrap();
    let first = DATA_BASE + 0xdc0;
    let second = DATA_BASE + 0xde0;
    record_windows_address_waiter(engine.unicorn.get_data_mut(), first, 3).unwrap();
    record_windows_address_waiter(engine.unicorn.get_data_mut(), first, 2).unwrap();
    record_windows_address_waiter(engine.unicorn.get_data_mut(), second, 4).unwrap();
    let marker = 0x8877_6655_4433_2211;
    engine.unicorn.reg_write(RegisterX86::RAX, marker).unwrap();
    assert_eq!(
        engine
            .call_win64(WAKE_SINGLE, [first, 0, 0, 0, 0, 0])
            .unwrap(),
        marker
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_address_waiters
            .get(&first),
        Some(&BTreeSet::from([3]))
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_address_waiters
            .get(&second),
        Some(&BTreeSet::from([4]))
    );

    engine.write(first, &[0; 8]).unwrap();
    engine.write(second, &[0; 8]).unwrap();
    let error = engine
        .call_win64(WAIT, [first, second, 8, u64::from(u32::MAX), 0, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("WaitOnAddress deadlock"),
        "{error}"
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_address_waiters
            .values()
            .all(|waiters| !waiters.contains(&1))
    );
}

#[test]
fn issue1402_finite_main_wait_runs_ready_peer_then_times_out_without_starvation() {
    const CREATE: u64 = STUB_BASE + 0x4b0;
    const SWITCH: u64 = STUB_BASE + 0x4c0;
    const WAIT: u64 = STUB_BASE + 0x4d0;
    const GET_LAST_ERROR: u64 = STUB_BASE + 0x4e0;
    const CHILD_OFFSET: usize = 0x180;
    const DRAIN_OFFSET: usize = 0x280;
    let address = DATA_BASE + 0xe20;
    let compare = DATA_BASE + 0xe40;
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
    push_mov_imm64(&mut code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
    code.extend_from_slice(&[0x45, 0x31, 0xc9]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], CREATE);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb9], address);
    push_mov_imm64(&mut code, [0x48, 0xba], compare);
    code.extend_from_slice(&[0x49, 0xc7, 0xc0, 4, 0, 0, 0]);
    code.extend_from_slice(&[0x49, 0xc7, 0xc1, 0xe8, 0x03, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], WAIT);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], GET_LAST_ERROR);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    code.extend_from_slice(&[0xbb, 0x2c, 0x01, 0, 0]); // mov ebx, 300
    let yield_loop = code.len();
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0, 0xff, 0xcb, 0x75, 0]);
    let after_branch = code.len();
    code[after_branch - 1] = (yield_loop as isize - after_branch as isize) as i8 as u8;
    code.extend_from_slice(&[0x48, 0x83, 0xc4, 0x28, 0xb8, 7, 0, 0, 0, 0xc3]);
    code.resize(DRAIN_OFFSET, 0x90);
    code.extend_from_slice(&[0xb8, 0x55, 0, 0, 0, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(address, &[0; 8]).unwrap();
    engine.write(compare, &[0; 8]).unwrap();
    for (stub, symbol) in [
        (CREATE, "CreateThread"),
        (SWITCH, "SwitchToThread"),
        (WAIT, "WaitOnAddress"),
        (GET_LAST_ERROR, "GetLastError"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap();
    }
    assert_eq!(engine.call_win64(TEST_CODE, [0; 6]).unwrap(), 1460);
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .values()
            .any(|thread| !thread.completed)
    );
    assert!(engine.scheduler_ready.is_empty());
    assert_eq!(engine.scheduler_deferred_ready.len(), 1);
    assert_eq!(engine.scheduled_windows_threads.len(), 1);
    assert!(engine.unicorn.get_data().windows_address_waiters.is_empty());
    assert_eq!(
        engine
            .call_win64(TEST_CODE + DRAIN_OFFSET as u64, [0; 6])
            .unwrap(),
        0x55
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .values()
            .all(|thread| thread.completed && thread.exit_code == 7)
    );
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.scheduler_deferred_ready.is_empty());
    assert!(engine.scheduled_windows_threads.is_empty());
}

#[test]
fn issue1402_finite_main_wait_survives_multiple_peer_slices_until_exact_wake() {
    const CREATE: u64 = STUB_BASE + 0x4f0;
    const SWITCH: u64 = STUB_BASE + 0x500;
    const WAIT: u64 = STUB_BASE + 0x510;
    const WAKE: u64 = STUB_BASE + 0x520;
    const CHILD_OFFSET: usize = 0x180;
    let address = DATA_BASE + 0xe60;
    let compare = DATA_BASE + 0xe80;
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
    push_mov_imm64(&mut code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
    code.extend_from_slice(&[0x45, 0x31, 0xc9]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], CREATE);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb9], address);
    push_mov_imm64(&mut code, [0x48, 0xba], compare);
    code.extend_from_slice(&[0x49, 0xc7, 0xc0, 4, 0, 0, 0]);
    code.extend_from_slice(&[0x49, 0xc7, 0xc1, 25, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], WAIT);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    for _ in 0..2 {
        push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
        code.extend_from_slice(&[0xff, 0xd0]);
    }
    push_mov_imm64(&mut code, [0x48, 0xb9], address);
    push_mov_imm64(&mut code, [0x48, 0xb8], WAKE);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xb8, 7, 0, 0, 0, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(address, &[0; 8]).unwrap();
    engine.write(compare, &[0; 8]).unwrap();
    for (stub, symbol) in [
        (CREATE, "CreateThread"),
        (SWITCH, "SwitchToThread"),
        (WAIT, "WaitOnAddress"),
        (WAKE, "WakeByAddressSingle"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap();
    }
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(engine.call_win64(TEST_CODE, [0; 6]).unwrap(), 1);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .values()
            .any(|thread| thread.completed && thread.exit_code == 7)
    );
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.scheduled_windows_threads.is_empty());
    assert!(engine.unicorn.get_data().windows_address_waiters.is_empty());
}

#[test]
fn get_proc_address_resolves_fls_alloc_to_a_stable_callable_guest_address() {
    const GET_PROC: u64 = STUB_BASE + 0x408;
    let name = DATA_BASE + 0xb00;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_PROC,
        "kernel32.dll",
        "GetProcAddress",
    )
    .unwrap();
    engine.write(name, b"FlsAlloc\0").unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;

    let first = engine
        .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0])
        .unwrap();
    let repeated = engine
        .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(first, HOST_DYNAMIC_FLS_ALLOC);
    assert_ne!(first, HOST_INITIALIZE_CONDITION_VARIABLE);
    assert_eq!(repeated, first);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert_eq!(engine.call_win64(first, [0; 6]).unwrap(), 0);
    assert_eq!(engine.call_win64(first, [0; 6]).unwrap(), 1);

    let mut other = test_engine(&[0xc3]);
    install_win64_import(
        &mut other.unicorn,
        GET_PROC,
        "kernel32.dll",
        "GetProcAddress",
    )
    .unwrap();
    other.write(name, b"FlsAlloc\0").unwrap();
    let other_pointer = other
        .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(other_pointer, first);
    assert_eq!(other.call_win64(other_pointer, [0; 6]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_fls_slots.len(), 2);
    assert_eq!(other.unicorn.get_data().windows_fls_slots.len(), 1);
}

#[test]
fn get_proc_address_rejects_foreign_modules_names_ordinals_and_unsafe_pointers() {
    const GET_PROC: u64 = STUB_BASE + 0x408;
    let name = DATA_BASE + 0xb00;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_PROC,
        "kernel32.dll",
        "GetProcAddress",
    )
    .unwrap();
    engine.write(name, b"FlsAlloc\0").unwrap();

    assert_eq!(
        engine
            .call_win64(GET_PROC, [0x1234, name, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );

    for symbol in [b"FlsGetValue\0".as_slice(), b"flsalloc\0".as_slice()] {
        engine.write(name, symbol).unwrap();
        assert_eq!(
            engine
                .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_PROC_NOT_FOUND
        );
    }

    for pointer in [1, 0xffff, 0xdead_beef] {
        assert_eq!(
            engine
                .call_win64(
                    GET_PROC,
                    [WINDOWS_KERNEL32_MODULE_TOKEN, pointer, 0, 0, 0, 0],
                )
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_PROC_NOT_FOUND
        );
    }

    let unterminated = DATA_BASE + PAGE_SIZE - 128;
    engine.write(unterminated, &[b'A'; 128]).unwrap();
    assert_eq!(
        engine
            .call_win64(
                GET_PROC,
                [WINDOWS_KERNEL32_MODULE_TOKEN, unterminated, 0, 0, 0, 0],
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_PROC_NOT_FOUND
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
    assert!(engine.unicorn.get_data().windows_fls_slots.is_empty());
}

#[test]
fn get_module_handle_w_resolves_current_kernel32_and_ntdll_basename_capabilities() {
    const GET_MODULE: u64 = STUB_BASE + 0x408;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "GetModuleHandleW"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetModuleHandleW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetModuleHandleW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE,
        "kernel32.dll",
        "GetModuleHandleW",
    )
    .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine.call_win64(GET_MODULE, [0, 0, 0, 0, 0, 0]).unwrap(),
        TEST_CODE
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    let name = DATA_BASE + 0xa00;
    for (value, expected) in [
        ("KeRnEl32.DlL", WINDOWS_KERNEL32_MODULE_TOKEN),
        ("ntdll.dll", WINDOWS_NTDLL_MODULE_TOKEN),
        (r"C:\Windows\System32\NTDLL.DLL", WINDOWS_NTDLL_MODULE_TOKEN),
    ] {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        engine
            .write(
                name,
                &units
                    .iter()
                    .flat_map(|unit| unit.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x5678;
        assert_eq!(
            engine
                .call_win64(GET_MODULE, [name, 0, 0, 0, 0, 0])
                .unwrap(),
            expected
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x5678);
    }
    assert_ne!(WINDOWS_NTDLL_MODULE_TOKEN, WINDOWS_KERNEL32_MODULE_TOKEN);
    assert_ne!(WINDOWS_NTDLL_MODULE_TOKEN, TEST_CODE);
    assert!(
        ![
            WINDOWS_STANDARD_INPUT_TOKEN,
            WINDOWS_STANDARD_OUTPUT_TOKEN,
            WINDOWS_STANDARD_ERROR_TOKEN,
            WINDOWS_THREAD_HANDLE_BASE,
        ]
        .contains(&WINDOWS_NTDLL_MODULE_TOKEN)
    );

    let mut second = test_engine(&[0xc3]);
    install_win64_import(
        &mut second.unicorn,
        GET_MODULE,
        "kernel32.dll",
        "GetModuleHandleW",
    )
    .unwrap();
    let second_name = DATA_BASE + 0xa00;
    let mut units = "NTDLL.DLL".encode_utf16().collect::<Vec<_>>();
    units.push(0);
    second
        .write(
            second_name,
            &units
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        second
            .call_win64(GET_MODULE, [second_name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_NTDLL_MODULE_TOKEN
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
    assert!(second.unicorn.get_data().callback_error.is_none());
}

#[test]
fn get_module_handle_w_bounds_utf16_and_reports_missing_modules_without_stopping() {
    const GET_MODULE: u64 = STUB_BASE + 0x408;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE,
        "kernel32.dll",
        "GetModuleHandleW",
    )
    .unwrap();
    let name = DATA_BASE + 0xa00;

    let exact = format!("{}\\ntdll.dll", "A".repeat(117));
    assert_eq!(exact.encode_utf16().count(), 127);
    let mut units = exact.encode_utf16().collect::<Vec<_>>();
    units.push(0);
    engine
        .write(
            name,
            &units
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(GET_MODULE, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_NTDLL_MODULE_TOKEN
    );

    for units in [
        "missing.dll".encode_utf16().chain([0]).collect::<Vec<_>>(),
        r"C:\Windows\System32\"
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>(),
        vec![0xd800, 0],
        vec![u16::from(b'A'); 128],
    ] {
        engine
            .write(
                name,
                &units
                    .iter()
                    .flat_map(|unit| unit.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(
            engine
                .call_win64(GET_MODULE, [name, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }
    for pointer in [u64::MAX, 0xdead_beef, DATA_BASE + PAGE_SIZE - 1] {
        assert_eq!(
            engine
                .call_win64(GET_MODULE, [pointer, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }

    let mut inaccessible = test_engine(&[0xc3]);
    install_win64_import(
        &mut inaccessible.unicorn,
        GET_MODULE,
        "kernel32.dll",
        "GetModuleHandleW",
    )
    .unwrap();
    let mut units = "ntdll.dll".encode_utf16().collect::<Vec<_>>();
    units.push(0);
    inaccessible
        .write(
            name,
            &units
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    inaccessible
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::WRITE)
        .unwrap();
    assert_eq!(
        inaccessible
            .call_win64(GET_MODULE, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        inaccessible.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
    assert!(inaccessible.unicorn.get_data().callback_error.is_none());
}

#[test]
fn ntdll_module_token_isolated_across_filename_and_proc_consumers() {
    const GET_FILE_NAME: u64 = STUB_BASE + 0x418;
    const GET_PROC: u64 = STUB_BASE + 0x428;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_FILE_NAME,
        "kernel32.dll",
        "GetModuleFileNameW",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        GET_PROC,
        "kernel32.dll",
        "GetProcAddress",
    )
    .unwrap();
    let output = DATA_BASE + 0xb00;
    let expected = r"C:\Windows\System32\ntdll.dll";
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine
            .call_win64(
                GET_FILE_NAME,
                [WINDOWS_NTDLL_MODULE_TOKEN, output, 64, 0, 0, 0],
            )
            .unwrap(),
        expected.encode_utf16().count() as u64
    );
    let mut bytes = vec![0; (expected.encode_utf16().count() + 1) * 2];
    engine.read(output, &mut bytes).unwrap();
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect::<Vec<_>>();
    assert_eq!(String::from_utf16(&units).unwrap(), expected);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    let name = DATA_BASE + 0xc00;
    engine.write(name, b"NtCreateFile\0").unwrap();
    assert_eq!(
        engine
            .call_win64(GET_PROC, [WINDOWS_NTDLL_MODULE_TOKEN, name, 0, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_PROC_NOT_FOUND
    );

    assert_eq!(
        engine
            .call_win64(
                GET_FILE_NAME,
                [WINDOWS_STANDARD_INPUT_TOKEN, output, 64, 0, 0, 0],
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
    assert_eq!(
        engine
            .call_win64(GET_PROC, [WINDOWS_STANDARD_INPUT_TOKEN, name, 0, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );

    engine.write(name, b"FlsAlloc\0").unwrap();
    assert_eq!(
        engine
            .call_win64(GET_PROC, [WINDOWS_NTDLL_MODULE_TOKEN, name, 0, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_PROC_NOT_FOUND
    );
    assert_ne!(
        engine
            .call_win64(GET_PROC, [WINDOWS_KERNEL32_MODULE_TOKEN, name, 0, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn get_module_handle_ex_a_resolves_name_and_address_with_win32_flags() {
    const GET_MODULE_EX: u64 = STUB_BASE + 0x410;
    const PIN: u64 = 0x1;
    const UNCHANGED_REFCOUNT: u64 = 0x2;
    const FROM_ADDRESS: u64 = 0x4;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "GetModuleHandleExA"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetModuleHandleExA)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetModuleHandleExA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE_EX,
        "kernel32.dll",
        "GetModuleHandleExA",
    )
    .unwrap();

    let name = DATA_BASE + 0xa40;
    let output = DATA_BASE + 0xa80;
    engine.write(name, b"KeRnEl32.DlL\0").unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [PIN, name, output, 0, 0, 0])
            .unwrap(),
        1
    );
    let mut module_bytes = [0; 8];
    engine.read(output, &mut module_bytes).unwrap();
    assert_eq!(
        u64::from_le_bytes(module_bytes),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    for flags in [0, PIN, UNCHANGED_REFCOUNT] {
        engine.write(output, &[0; 8]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x5678;
        assert_eq!(
            engine
                .call_win64(GET_MODULE_EX, [flags, 0, output, 0, 0, 0])
                .unwrap(),
            1
        );
        engine.read(output, &mut module_bytes).unwrap();
        assert_eq!(u64::from_le_bytes(module_bytes), TEST_CODE);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x5678);
    }

    engine.write(output, &[0; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(
                GET_MODULE_EX,
                [
                    FROM_ADDRESS | UNCHANGED_REFCOUNT,
                    TEST_CODE,
                    output,
                    0,
                    0,
                    0
                ],
            )
            .unwrap(),
        1
    );
    engine.read(output, &mut module_bytes).unwrap();
    assert_eq!(u64::from_le_bytes(module_bytes), TEST_CODE);

    engine.write(name, b"missing.dll\0").unwrap();
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, name, output, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [FROM_ADDRESS, DATA_BASE, output, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
    for flags in [PIN | UNCHANGED_REFCOUNT, 0x8] {
        assert_eq!(
            engine
                .call_win64(GET_MODULE_EX, [flags, name, output, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
    }
    engine.read(output, &mut module_bytes).unwrap();
    assert_eq!(
        u64::from_le_bytes(module_bytes),
        TEST_CODE,
        "failed lookups must not overwrite the caller's output handle"
    );
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, name, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, 0, 0xdead_beef, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
}

#[test]
fn get_module_handle_ex_w_resolves_guest_modules_with_win32_flags() {
    const GET_MODULE_EX: u64 = STUB_BASE + 0x418;
    const PIN: u64 = 0x1;
    const UNCHANGED_REFCOUNT: u64 = 0x2;
    const FROM_ADDRESS: u64 = 0x4;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "GetModuleHandleExW"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetModuleHandleExW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetModuleHandleExW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE_EX,
        "kernel32.dll",
        "GetModuleHandleExW",
    )
    .unwrap();

    let name = DATA_BASE + 0xb00;
    let output = DATA_BASE + 0xb80;
    let mut encoded = "KeRnEl32.DlL".encode_utf16().collect::<Vec<_>>();
    encoded.push(0);
    engine
        .write(
            name,
            &encoded
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [PIN, name, output, 0, 0, 0])
            .unwrap(),
        1
    );
    let mut module_bytes = [0; 8];
    engine.read(output, &mut module_bytes).unwrap();
    assert_eq!(
        u64::from_le_bytes(module_bytes),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    for flags in [0, PIN, UNCHANGED_REFCOUNT] {
        engine.write(output, &[0; 8]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x5678;
        assert_eq!(
            engine
                .call_win64(GET_MODULE_EX, [flags, 0, output, 0, 0, 0])
                .unwrap(),
            1
        );
        engine.read(output, &mut module_bytes).unwrap();
        assert_eq!(u64::from_le_bytes(module_bytes), TEST_CODE);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x5678);
    }

    for address in [TEST_CODE, TEST_CODE + PAGE_SIZE - 1] {
        engine.write(output, &[0; 8]).unwrap();
        assert_eq!(
            engine
                .call_win64(
                    GET_MODULE_EX,
                    [FROM_ADDRESS | UNCHANGED_REFCOUNT, address, output, 0, 0, 0],
                )
                .unwrap(),
            1
        );
        engine.read(output, &mut module_bytes).unwrap();
        assert_eq!(u64::from_le_bytes(module_bytes), TEST_CODE);
    }
}

#[test]
fn get_module_handle_ex_w_rejects_invalid_flags_names_addresses_and_outputs_atomically() {
    const GET_MODULE_EX: u64 = STUB_BASE + 0x418;
    const PIN: u64 = 0x1;
    const UNCHANGED_REFCOUNT: u64 = 0x2;
    const FROM_ADDRESS: u64 = 0x4;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE_EX,
        "kernel32.dll",
        "GetModuleHandleExW",
    )
    .unwrap();
    let name = DATA_BASE + 0xc00;
    let output = DATA_BASE + 0xc80;
    let sentinel = 0x1122_3344_5566_7788u64.to_le_bytes();

    let mut missing = "missing.dll".encode_utf16().collect::<Vec<_>>();
    missing.push(0);
    engine
        .write(
            name,
            &missing
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    for (flags, name_or_address, expected_error) in [
        (0, name, ERROR_MOD_NOT_FOUND),
        (FROM_ADDRESS, DATA_BASE, ERROR_MOD_NOT_FOUND),
        (FROM_ADDRESS, TEST_CODE + PAGE_SIZE, ERROR_MOD_NOT_FOUND),
        (PIN | UNCHANGED_REFCOUNT, 0, ERROR_INVALID_PARAMETER),
        (0x8, 0, ERROR_INVALID_PARAMETER),
    ] {
        engine.write(output, &sentinel).unwrap();
        assert_eq!(
            engine
                .call_win64(GET_MODULE_EX, [flags, name_or_address, output, 0, 0, 0],)
                .unwrap(),
            0
        );
        let mut actual = [0; 8];
        engine.read(output, &mut actual).unwrap();
        assert_eq!(actual, sentinel);
        assert_eq!(engine.unicorn.get_data().windows_last_error, expected_error);
    }

    engine.write(name, &[b'A', 0].repeat(128)).unwrap();
    for invalid_name in [name, u64::MAX, DATA_BASE + DATA_SIZE - 1] {
        engine.write(output, &sentinel).unwrap();
        assert_eq!(
            engine
                .call_win64(GET_MODULE_EX, [0, invalid_name, output, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
        let mut actual = [0; 8];
        engine.read(output, &mut actual).unwrap();
        assert_eq!(actual, sentinel);
    }

    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );

    engine
        .unicorn
        .mem_protect(TEST_CODE, PAGE_SIZE, Prot::READ | Prot::EXEC)
        .unwrap();
    let mut code_before = [0; 8];
    engine.read(TEST_CODE, &mut code_before).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, 0, TEST_CODE, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    let mut code_after = [0; 8];
    engine.read(TEST_CODE, &mut code_after).unwrap();
    assert_eq!(code_after, code_before);

    let partial_output = DATA_BASE + PAGE_SIZE - 4;
    engine.write(partial_output, &[0x5a; 4]).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, 0, partial_output, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    let mut tail = [0; 4];
    engine.read(partial_output, &mut tail).unwrap();
    assert_eq!(tail, [0x5a; 4]);
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn get_module_handle_ex_w_does_not_read_guest_inaccessible_names() {
    const GET_MODULE_EX: u64 = STUB_BASE + 0x418;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE_EX,
        "kernel32.dll",
        "GetModuleHandleExW",
    )
    .unwrap();
    let name = DATA_BASE + 0xd00;
    let output = HANDLE_DATA_BASE + 0x100;
    let mut encoded = "kernel32.dll".encode_utf16().collect::<Vec<_>>();
    encoded.push(0);
    engine
        .write(
            name,
            &encoded
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let sentinel = 0x1122_3344_5566_7788u64.to_le_bytes();
    engine.write(output, &sentinel).unwrap();
    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::WRITE)
        .unwrap();

    assert_eq!(
        engine
            .call_win64(GET_MODULE_EX, [0, name, output, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
    let mut actual = [0; 8];
    engine.read(output, &mut actual).unwrap();
    assert_eq!(actual, sentinel);
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn get_module_file_name_w_bounds_utf16_and_reports_win32_errors() {
    const GET_MODULE_FILE_NAME: u64 = STUB_BASE + 0x420;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "GetModuleFileNameW"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetModuleFileNameW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetModuleFileNameW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    install_win64_import(
        &mut engine.unicorn,
        GET_MODULE_FILE_NAME,
        "kernel32.dll",
        "GetModuleFileNameW",
    )
    .unwrap();
    let output = DATA_BASE + 0xb00;
    let guest_path = r"C:\AEXCompat\guest-plugin.aex";
    let guest_units = guest_path.encode_utf16().collect::<Vec<_>>();

    for module in [0, TEST_CODE] {
        engine.write(output, &[0xa5; 128]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x1234;
        let returned = engine
            .call_win64(GET_MODULE_FILE_NAME, [module, output, 64, 0, 0, 0])
            .unwrap();
        assert_eq!(returned, guest_units.len() as u64);
        let bytes = engine
            .unicorn
            .mem_read_as_vec(output, (returned as usize + 1) * 2)
            .unwrap();
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        assert_eq!(
            String::from_utf16(&units[..units.len() - 1]).unwrap(),
            guest_path
        );
        assert_eq!(units.last(), Some(&0));
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    }

    for (capacity, expected_error) in [
        (guest_units.len() + 1, 0x5678),
        (guest_units.len(), ERROR_INSUFFICIENT_BUFFER),
    ] {
        engine.write(output, &[0xa5; 128]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x5678;
        assert_eq!(
            engine
                .call_win64(GET_MODULE_FILE_NAME, [0, output, capacity as u64, 0, 0, 0],)
                .unwrap(),
            if capacity > guest_units.len() {
                guest_units.len() as u64
            } else {
                capacity as u64
            }
        );
        let bytes = engine
            .unicorn
            .mem_read_as_vec(output, capacity * 2)
            .unwrap();
        assert_eq!(&bytes[bytes.len() - 2..], &[0, 0]);
        assert_eq!(engine.unicorn.get_data().windows_last_error, expected_error);
    }

    engine.write(output, &[0xa5; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64(
                GET_MODULE_FILE_NAME,
                [WINDOWS_KERNEL32_MODULE_TOKEN, output, 4, 0, 0, 0],
            )
            .unwrap(),
        4
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [b'C', 0, b':', 0, b'\\', 0, 0, 0]
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INSUFFICIENT_BUFFER
    );

    engine.write(output, &[0xa5; 4]).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_MODULE_FILE_NAME, [0, output, 1, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 2).unwrap(), [0, 0]);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INSUFFICIENT_BUFFER
    );

    engine.write(output, &[0xa5; 16]).unwrap();
    for (module, pointer, capacity, error) in [
        (0xdead_beef, output, 64, ERROR_MOD_NOT_FOUND),
        (0, 0, 64, ERROR_INVALID_PARAMETER),
        (0, output, 0, ERROR_INSUFFICIENT_BUFFER),
        (0, 0xdead_beef, 64, ERROR_INVALID_PARAMETER),
    ] {
        assert_eq!(
            engine
                .call_win64(GET_MODULE_FILE_NAME, [module, pointer, capacity, 0, 0, 0],)
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, error);
    }
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 16).unwrap(),
        [0xa5; 16],
        "failed calls must not overwrite the caller's output buffer"
    );
}

#[test]
fn load_library_w_only_resolves_loaded_modules_and_counts_stable_handles() {
    const LOAD_LIBRARY: u64 = STUB_BASE + 0x430;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "LoadLibraryW"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LoadLibraryW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "LoadLibraryW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    install_win64_import(
        &mut engine.unicorn,
        LOAD_LIBRARY,
        "kernel32.dll",
        "LoadLibraryW",
    )
    .unwrap();
    let name = DATA_BASE + 0xc00;
    let write_wide = |engine: &mut GuestEngine<'static>, value: &str| {
        let bytes = value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        engine.write(name, &bytes).unwrap();
    };

    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    write_wide(&mut engine, r"C:\Windows\System32\KERNEL32.DLL");
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    write_wide(&mut engine, "kernel32.dll");
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    write_wide(&mut engine, "kernel32");
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_module_refcounts
            .get(&WINDOWS_KERNEL32_MODULE_TOKEN),
        Some(&3)
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    write_wide(&mut engine, r"C:/AEXCompat/guest-plugin.aex");
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        TEST_CODE
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_module_refcounts
            .get(&TEST_CODE),
        Some(&1)
    );

    engine
        .unicorn
        .get_data_mut()
        .windows_module_refcounts
        .insert(
            WINDOWS_KERNEL32_MODULE_TOKEN,
            MAX_WINDOWS_MODULE_REFERENCES - 1,
        );
    write_wide(&mut engine, "kernel32.dll");
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_module_refcounts
            .get(&WINDOWS_KERNEL32_MODULE_TOKEN),
        Some(&MAX_WINDOWS_MODULE_REFERENCES)
    );
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_NOT_ENOUGH_MEMORY
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_module_refcounts
            .get(&WINDOWS_KERNEL32_MODULE_TOKEN),
        Some(&MAX_WINDOWS_MODULE_REFERENCES)
    );

    let counts_after_capacity = engine.unicorn.get_data().windows_module_refcounts.clone();
    for rejected in [
        "opencv_core_parallel_onetbb455_64.dll",
        r"C:\attacker\kernel32.dll",
        r"C:\missing\guest-plugin.aex",
        r"C:\Windows\System32\..\System32\kernel32.dll",
        r".\kernel32.dll",
        "guest-plugin",
    ] {
        write_wide(&mut engine, rejected);
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
                .unwrap(),
            0,
            "{rejected:?} must not alias a loaded module"
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
        assert_eq!(
            engine.unicorn.get_data().windows_module_refcounts,
            counts_after_capacity,
            "failed lookup must not change module reference counts"
        );
    }
    engine.write(name, &[0x00, 0xd8, 0, 0]).unwrap();
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    engine.write(name, &[1; 1024]).unwrap();
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(
        engine.call_win64(LOAD_LIBRARY, [0, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
}

#[test]
fn deterministic_getenv_and_openmp_dynamic_policy_are_bounded() {
    const GETENV: u64 = STUB_BASE + 0x410;
    const OMP_SET_DYNAMIC: u64 = STUB_BASE + 0x420;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GETENV,
        "api-ms-win-crt-environment-l1-1-0.dll",
        "getenv",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        OMP_SET_DYNAMIC,
        "vcomp140.dll",
        "omp_set_dynamic",
    )
    .unwrap();
    let name = DATA_BASE + 0xa80;
    engine.write(name, b"opencv_for_threads_num\0").unwrap();
    let value = engine.call_win64(GETENV, [name, 0, 0, 0, 0, 0]).unwrap();
    assert!(
        guest_range_has_permission(&engine.unicorn, value, 2, Prot::READ | Prot::WRITE).unwrap()
    );
    assert!(!guest_range_has_permission(&engine.unicorn, value, 2, Prot::EXEC).unwrap());
    assert_eq!(engine.unicorn.mem_read_as_vec(value, 2).unwrap(), b"1\0");
    engine.write(name, b"HOME\0").unwrap();
    assert_eq!(engine.call_win64(GETENV, [name, 0, 0, 0, 0, 0]).unwrap(), 0);
    engine
        .call_win64(OMP_SET_DYNAMIC, [1, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(engine.unicorn.get_data().omp_dynamic_requested, Some(true));
    let error = engine
        .call_win64(OMP_SET_DYNAMIC, [2, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("expected 0 or 1"), "{error}");
}

#[test]
fn get_environment_variable_a_matches_win32_size_and_allowlist_contract() {
    const GET_ENVIRONMENT: u64 = STUB_BASE + 0x4d0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_ENVIRONMENT,
            "kernel32.dll",
            "GetEnvironmentVariableA",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetEnvironmentVariableA)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetEnvironmentVariableA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let name = DATA_BASE + 0xb00;
    let output = DATA_BASE + 0xb80;
    engine.write(name, b"OpEnCv_FoR_ThReAdS_NuM\0").unwrap();
    engine.write(output, &[0xaa; 4]).unwrap();

    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        2
    );
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, output, 1, 0, 0, 0])
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [0xaa; 4]
    );
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, output, 2, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 2).unwrap(), b"1\0");

    engine.write(name, b"HOME\0").unwrap();
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, 0, u32::MAX as u64, 0, 0, 0])
            .unwrap(),
        0
    );
}

#[test]
fn wide_char_to_multi_byte_converts_utf16_and_matches_size_query_contract() {
    const CONVERT: u64 = STUB_BASE + 0x4e0;
    const CP_OEMCP: u64 = 1;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            CONVERT,
            "KERNEL32.DLL",
            "WideCharToMultiByte",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WideCharToMultiByte)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "WideCharToMultiByte"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let source = DATA_BASE + 0xc40;
    let output = DATA_BASE + 0xc80;
    let utf16: Vec<u8> = "A日本"
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    engine.write(source, &utf16).unwrap();
    engine.write(output, &[0xaa; 16]).unwrap();

    let required = engine
        .call_win64_with_timeout(
            CONVERT,
            &[CP_OEMCP, 0, source, u32::MAX as u64, 0, 0, 0, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_eq!(required, 6);
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CONVERT,
                &[CP_OEMCP, 0, source, u32::MAX as u64, output, required, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        required
    );
    let expected = vec![b'A', 0x93, 0xfa, 0x96, 0x7b, 0];
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(output, expected.len())
            .unwrap(),
        expected
    );

    engine.write(output, &[0xaa; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CONVERT,
                &[CP_OEMCP, 0, source, 2, output, 16, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        3
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [b'A', 0x93, 0xfa, 0xaa]
    );

    engine.write(output, &[0xaa; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CONVERT,
                &[65_001, 0x80, source, u32::MAX as u64, output, 16, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        8
    );
    let mut utf8 = "A日本".as_bytes().to_vec();
    utf8.push(0);
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 8).unwrap(), utf8);
}

#[test]
fn wide_char_to_multi_byte_substitutes_default_and_rejects_negative_output_size() {
    const CONVERT: u64 = STUB_BASE + 0x4f0;
    const CP_OEMCP: u64 = 1;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CONVERT,
        "kernel32.dll",
        "WideCharToMultiByte",
    )
    .unwrap();
    let source = DATA_BASE + 0xcc0;
    let default = DATA_BASE + 0xce0;
    let used_default = DATA_BASE + 0xcf0;
    let output = DATA_BASE + 0xd00;
    let utf16: Vec<u8> = "😀"
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    engine.write(source, &utf16).unwrap();
    engine.write(default, b"*").unwrap();
    engine.write(used_default, &[0xaa]).unwrap();

    assert_eq!(
        engine
            .call_win64_with_timeout(
                CONVERT,
                &[
                    CP_OEMCP,
                    0,
                    source,
                    u32::MAX as u64,
                    0,
                    0,
                    default,
                    used_default,
                ],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(used_default, 1).unwrap(),
        [1]
    );
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CONVERT,
                &[
                    CP_OEMCP,
                    0,
                    source,
                    u32::MAX as u64,
                    output,
                    2,
                    default,
                    used_default,
                ],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        2
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 2).unwrap(), b"*\0");

    let mut invalid = test_engine(&[0xc3]);
    install_win64_import(
        &mut invalid.unicorn,
        CONVERT,
        "kernel32.dll",
        "WideCharToMultiByte",
    )
    .unwrap();
    invalid.write(source, &[b'A', 0]).unwrap();
    assert_eq!(
        invalid
            .call_win64_with_timeout(
                CONVERT,
                &[CP_OEMCP, 0, source, 1, output, u32::MAX as u64, 0, 0,],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        invalid.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );

    assert_eq!(
        invalid
            .call_win64_with_timeout(
                CONVERT,
                &[CP_OEMCP, 0, source, u32::MAX as u64 - 1, 0, 0, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        invalid.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );

    invalid.write(source, &[b'A', 0, b'B', 0]).unwrap();
    assert_eq!(
        invalid
            .call_win64_with_timeout(
                CONVERT,
                &[CP_OEMCP, 0, source, 2, output, 1, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(invalid.unicorn.get_data().windows_last_error, 122);

    assert_eq!(
        invalid
            .call_win64_with_timeout(
                CONVERT,
                &[65_001, 0, source, 1, output, 1, default, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        invalid.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );

    invalid.write(source, &[0x00, 0xd8]).unwrap();
    assert_eq!(
        invalid
            .call_win64_with_timeout(
                CONVERT,
                &[65_001, 0x80, source, 1, 0, 0, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(invalid.unicorn.get_data().windows_last_error, 1113);
}

#[test]
fn multi_byte_to_wide_char_converts_cp932_and_utf8_with_win32_lengths() {
    const CONVERT: u64 = STUB_BASE + 0x4f8;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            CONVERT,
            "KERNEL32.DLL",
            "MultiByteToWideChar",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::MultiByteToWideChar)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "MultiByteToWideChar"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let source = DATA_BASE + 0xd40;
    let output = DATA_BASE + 0xd80;
    engine
        .write(source, &[b'A', 0x93, 0xfa, 0x96, 0x7b, 0])
        .unwrap();
    engine.write(output, &[0xaa; 20]).unwrap();

    let required = engine
        .call_win64(CONVERT, [0, 0, source, u32::MAX as u64, 0, 0])
        .unwrap();
    assert_eq!(required, 4);
    engine.unicorn.get_data_mut().windows_last_error = 0xdead_beef;
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 0, source, u32::MAX as u64, output, required])
            .unwrap(),
        required
    );
    let expected = "A日本"
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 8).unwrap(), expected);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0xdead_beef);

    engine.write(source, "A😀Z".as_bytes()).unwrap();
    engine.write(output, &[0xaa; 20]).unwrap();
    assert_eq!(
        engine
            .call_win64(CONVERT, [65_001, 0x08, source, 6, output, 8])
            .unwrap(),
        4
    );
    let expected = "A😀Z"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 8).unwrap(), expected);
}

#[test]
fn multi_byte_to_wide_char_reports_validation_and_malformed_input_failures() {
    const CONVERT: u64 = STUB_BASE + 0x4f8;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CONVERT,
        "kernel32.dll",
        "MultiByteToWideChar",
    )
    .unwrap();
    let source = DATA_BASE + 0xdc0;
    let output = DATA_BASE + 0xde0;
    engine.write(source, &[0xff, 0]).unwrap();
    engine.write(output, &[0xaa; 8]).unwrap();

    assert_eq!(
        engine
            .call_win64(CONVERT, [65_001, 0x08, source, 1, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1113);
    assert_eq!(
        engine
            .call_win64(CONVERT, [65_001, 0, source, 1, output, 2])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 2).unwrap(),
        0xfffdu16.to_le_bytes()
    );

    engine.write(source, &[0x81]).unwrap();
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 0x08, source, 1, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1113);
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 0, source, 1, output, 2])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 2).unwrap(),
        0xfffdu16.to_le_bytes()
    );
    engine.write(source, &[0x82, 0xa0]).unwrap();
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 1, source, 2, output, 1])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 2).unwrap(),
        0x3042u16.to_le_bytes()
    );
    engine.write(source, &[0x82, 0xaa, 0x07]).unwrap();
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 2, source, 2, output, 2])
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [0x4b, 0x30, 0x99, 0x30]
    );
    // CP932 is an ANSI code page and has no OEM glyph table, so
    // MB_USEGLYPHCHARS leaves its C0 control mapping unchanged.
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 4, source + 2, 1, output, 1])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 2).unwrap(),
        0x0007u16.to_le_bytes()
    );

    for arguments in [
        [1_250, 0, source, 1, 0, 0],
        [932, 3, source, 1, 0, 0],
        [65_001, 1, source, 1, 0, 0],
        [932, 0, 0, 1, 0, 0],
        [932, 0, source, 0, 0, 0],
        [932, 0, source, u32::MAX as u64 - 1, 0, 0],
        [932, 0, source, 1, output, u32::MAX as u64],
        [932, 0, source, 1, 0, 1],
        [932, 0, source, 1, source, 1],
    ] {
        assert_eq!(engine.call_win64(CONVERT, arguments).unwrap(), 0);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
    }
    let overlap_start = source - 2;
    let overlap_sentinel = [0x11, 0x22, b'A', b'B', 0x55, 0x66, 0x77, 0x88];
    engine.write(overlap_start, &overlap_sentinel).unwrap();
    for destination in [source + 1, source - 2] {
        assert_eq!(
            engine
                .call_win64(CONVERT, [932, 0, source, 2, destination, 2])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(overlap_start, overlap_sentinel.len())
                .unwrap(),
            overlap_sentinel
        );
    }
    engine.write(source, b"AB").unwrap();
    engine.write(output, &[0xaa; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(CONVERT, [932, 0, source, 2, output, 1])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 122);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [0xaa; 8]
    );
}

#[test]
fn multi_byte_to_wide_char_preflights_the_entire_output_before_writing() {
    const CONVERT: u64 = STUB_BASE + 0x4f8;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CONVERT,
        "kernel32.dll",
        "MultiByteToWideChar",
    )
    .unwrap();
    let source = DATA_BASE + 0xe20;
    let crossing = DATA_BASE + PAGE_SIZE - 2;
    engine.write(source, b"AB").unwrap();
    engine.write(crossing, &[0x5a; 2]).unwrap();
    let error = engine
        .call_win64(CONVERT, [932, 0, source, 2, crossing, 2])
        .unwrap_err();
    assert!(error.to_string().contains("not fully writable"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(crossing, 2).unwrap(),
        [0x5a; 2]
    );
}

#[test]
fn get_string_type_w_is_kernel32_scoped_and_classifies_ctype1() {
    const CLASSIFY: u64 = STUB_BASE + 0x500;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            CLASSIFY,
            "KERNEL32.DLL",
            "GetStringTypeW",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetStringTypeW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetStringTypeW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let source = DATA_BASE + 0xe40;
    let output = DATA_BASE + 0xe80;
    let units = [
        'A' as u16,
        'z' as u16,
        '9' as u16,
        ' ' as u16,
        '!' as u16,
        '\n' as u16,
        0x3042,
        0x0301,
        0x0378,
    ];
    engine
        .write(
            source,
            &units
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0xdead_beef;
    assert_eq!(
        engine
            .call_win64(CLASSIFY, [1, source, units.len() as u64, output, 0, 0])
            .unwrap(),
        1
    );
    let words = engine
        .unicorn
        .mem_read_as_vec(output, units.len() * 2)
        .unwrap()
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    assert_eq!(
        words,
        [
            0x0181, 0x0102, 0x0084, 0x0048, 0x0010, 0x0028, 0x0100, 0x0200, 0
        ]
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0xdead_beef);
}

#[test]
fn get_string_type_w_classifies_bidi_japanese_combining_and_utf16_units() {
    const CLASSIFY: u64 = STUB_BASE + 0x500;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CLASSIFY,
        "kernel32.dll",
        "GetStringTypeW",
    )
    .unwrap();
    let source = DATA_BASE + 0xea0;
    let output = DATA_BASE + 0xee0;
    let units = [
        b'A' as u16,
        b'7' as u16,
        b'+' as u16,
        b',' as u16,
        0x05d0,
        0x0661,
        0x2029,
        0x200b,
        0x0301,
    ];
    engine
        .write(
            source,
            &units
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(CLASSIFY, [2, source, 9, output, 0, 0])
            .unwrap(),
        1
    );
    let words = engine
        .unicorn
        .mem_read_as_vec(output, 18)
        .unwrap()
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    assert_eq!(words, [1, 3, 4, 7, 2, 6, 8, 0, 11]);

    let units = [0x3042, 0x30a2, 0xff71, 0x6f22, 0x0301, 0xd83d, 0xde00];
    engine
        .write(
            source,
            &units
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(CLASSIFY, [3, source, 7, output, 0, 0])
            .unwrap(),
        1
    );
    let words = engine
        .unicorn
        .mem_read_as_vec(output, 14)
        .unwrap()
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    assert_eq!(
        words,
        [0x80a0, 0x8090, 0x8050, 0x8180, 0x0003, 0x0800, 0x1000]
    );

    let units = [
        0x093e,
        0x0941,
        0x0a41,
        0x09be,
        0x0903,
        0x0640,
        b'!' as u16,
        b'-' as u16,
        b'=' as u16,
        0x00aa,
        0x00ba,
        0x20ac,
        0xffc0,
    ];
    engine
        .write(
            source,
            &units
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(CLASSIFY, [3, source, 13, output, 0, 0])
            .unwrap(),
        1
    );
    let words = engine
        .unicorn
        .mem_read_as_vec(output, 26)
        .unwrap()
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    assert_eq!(
        words,
        [
            0x8004, 0x8005, 0x8005, 0x8004, 0x8000, 0x8600, 0, 0x0400, 0x0408, 0x8400, 0x8400,
            0x0008, 0,
        ]
    );
}

#[test]
fn get_string_type_w_supports_minus_one_and_reports_validation_failures() {
    const CLASSIFY: u64 = STUB_BASE + 0x500;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CLASSIFY,
        "kernel32.dll",
        "GetStringTypeW",
    )
    .unwrap();
    let source = DATA_BASE + 0xf00;
    let output = DATA_BASE + 0xf20;
    engine.write(source, &[b'A', 0, 0, 0]).unwrap();
    engine.write(output, &[0xaa; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(CLASSIFY, [1, source, u32::MAX as u64, output, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [0x81, 0x01, 0x20, 0x00]
    );

    for arguments in [
        [0, source, 1, output, 0, 0],
        [4, source, 1, output, 0, 0],
        [1, 0, 1, output, 0, 0],
        [1, source, 0, output, 0, 0],
        [1, source, 1, 0, 0, 0],
        [1, source, 1, source, 0, 0],
        [1, source, 2, source + 2, 0, 0],
    ] {
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(engine.call_win64(CLASSIFY, arguments).unwrap(), 0);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
    }

    engine.write(output, &[0xaa; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(CLASSIFY, [1, source, u32::MAX as u64 - 1, output, 0, 0],)
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [0x81, 0x01, 0x20, 0x00]
    );
}

#[test]
fn get_string_type_w_preflights_the_entire_output_before_writing() {
    const CLASSIFY: u64 = STUB_BASE + 0x500;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CLASSIFY,
        "kernel32.dll",
        "GetStringTypeW",
    )
    .unwrap();
    let source = DATA_BASE + 0xf40;
    let crossing = DATA_BASE + PAGE_SIZE - 2;
    engine.write(source, &[b'A', 0, b'B', 0]).unwrap();
    engine.write(crossing, &[0x5a; 2]).unwrap();
    let error = engine
        .call_win64(CLASSIFY, [1, source, 2, crossing, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not fully writable"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(crossing, 2).unwrap(),
        [0x5a; 2]
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CLASSIFY,
        "kernel32.dll",
        "GetStringTypeW",
    )
    .unwrap();
    let output = DATA_BASE + 0xf80;
    engine.write(output, &[0x5a; 4]).unwrap();
    let error = engine
        .call_win64(CLASSIFY, [1, DATA_BASE + PAGE_SIZE, 2, output, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("source read failed"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [0x5a; 4]
    );
}

#[test]
fn lc_map_string_w_is_kernel32_scoped_and_maps_case_with_win32_lengths() {
    const MAP: u64 = STUB_BASE + 0x508;
    const LOWERCASE: u64 = 0x100;
    const UPPERCASE: u64 = 0x200;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(&mut engine.unicorn, MAP, "KERNEL32.DLL", "LCMapStringW").unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LCMapStringW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "LCMapStringW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let source = DATA_BASE + 0xfa0;
    let output = DATA_BASE + 0xfc0;
    let input = "aßあ😀\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine.write(source, &input).unwrap();
    engine.write(output, &[0xaa; 32]).unwrap();

    assert_eq!(
        engine
            .call_win64(MAP, [0x411, UPPERCASE, source, u32::MAX as u64, 0, 0])
            .unwrap(),
        6
    );
    // Win32 treats any negative cchSrc as a request to scan through NUL.
    assert_eq!(
        engine
            .call_win64(MAP, [0x411, UPPERCASE, source, u32::MAX as u64 - 1, 0, 0],)
            .unwrap(),
        6
    );
    engine.unicorn.get_data_mut().windows_last_error = 0xdead_beef;
    assert_eq!(
        engine
            .call_win64(MAP, [0x411, UPPERCASE, source, u32::MAX as u64, output, 6])
            .unwrap(),
        6
    );
    let expected = "Aßあ😀\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 12).unwrap(),
        expected
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0xdead_beef);

    let explicit = "AZあ".encode_utf16().collect::<Vec<_>>();
    engine
        .write(
            source,
            &explicit
                .iter()
                .copied()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(MAP, [0x7f, LOWERCASE, source, 3, output, 3])
            .unwrap(),
        3
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 6).unwrap(),
        "azあ"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        engine
            .call_win64(MAP, [0x7f, UPPERCASE, source, 3, source, 3])
            .unwrap(),
        3
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(source, 6).unwrap(),
        "AZあ"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
}

#[test]
fn lc_map_string_w_uses_locale_tailored_icu_sort_keys() {
    const MAP: u64 = STUB_BASE + 0x508;
    const SORTKEY_IGNORECASE: u64 = 0x401;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MAP, "kernel32.dll", "LCMapStringW").unwrap();
    let source = DATA_BASE + 0xf00;
    let output = DATA_BASE + 0xf80;
    let mut make_key = |locale: u64, flags: u64, value: &str| -> Vec<u8> {
        let units = value.encode_utf16().collect::<Vec<_>>();
        engine
            .write(
                source,
                &units
                    .iter()
                    .copied()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let required = engine
            .call_win64(MAP, [locale, flags, source, units.len() as u64, 0, 0])
            .unwrap();
        assert!(required > 1);
        assert_eq!(
            engine
                .call_win64(
                    MAP,
                    [locale, flags, source, units.len() as u64, output, required]
                )
                .unwrap(),
            required
        );
        let key = engine
            .unicorn
            .mem_read_as_vec(output, required as usize)
            .unwrap();
        assert_eq!(key.last(), Some(&0));
        key
    };

    // Treat sort keys as opaque: verify their documented ordering/equivalence
    // properties instead of freezing ICU's private byte representation.
    assert!(make_key(0x409, 0x400, "apple") < make_key(0x409, 0x400, "banana"));
    assert_eq!(
        make_key(0x409, SORTKEY_IGNORECASE, "Signal"),
        make_key(0x409, SORTKEY_IGNORECASE, "signal")
    );
    // Win32's default ja-JP key preserves kana type.
    assert_ne!(make_key(0x411, 0x400, "あ"), make_key(0x411, 0x400, "ア"));
    // NORM_IGNOREKANATYPE alone lowers the tailored kana distinction.
    assert_eq!(
        make_key(0x411, 0x0001_0400, "あ"),
        make_key(0x411, 0x0001_0400, "ア")
    );
    assert_ne!(make_key(0x411, 0x400, "ア"), make_key(0x411, 0x400, "ｱ"));
    assert_ne!(
        make_key(0x411, 0x0001_0400, "ア"),
        make_key(0x411, 0x0001_0400, "ｱ")
    );
    assert_eq!(
        make_key(0x411, 0x0001_0400, "ヷヽ"),
        make_key(0x411, 0x0001_0400, "わ\u{3099}ゝ")
    );

    drop(make_key);
    let mut make_raw_key = |units: &[u16]| -> Vec<u8> {
        engine
            .write(
                source,
                &units
                    .iter()
                    .copied()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let required = engine
            .call_win64(
                MAP,
                [0x409, SORTKEY_IGNORECASE, source, units.len() as u64, 0, 0],
            )
            .unwrap();
        assert_eq!(
            engine
                .call_win64(
                    MAP,
                    [
                        0x409,
                        SORTKEY_IGNORECASE,
                        source,
                        units.len() as u64,
                        output,
                        required,
                    ],
                )
                .unwrap(),
            required
        );
        engine
            .unicorn
            .mem_read_as_vec(output, required as usize)
            .unwrap()
    };
    assert_ne!(make_raw_key(&[0xd800]), make_raw_key(&[0xdc00]));
    assert_ne!(
        make_raw_key(&[0xd800, 0xfffd]),
        make_raw_key(&[0xfffd, 0xd800])
    );
}

#[test]
fn lc_map_string_w_distinguishes_simple_and_linguistic_casing() {
    const MAP: u64 = STUB_BASE + 0x508;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MAP, "kernel32.dll", "LCMapStringW").unwrap();
    let source = DATA_BASE + 0xf00;
    let output = DATA_BASE + 0xf40;
    let units = "ΟΣ".encode_utf16().collect::<Vec<_>>();
    engine
        .write(
            source,
            &units
                .iter()
                .copied()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();

    assert_eq!(
        engine
            .call_win64(MAP, [0x409, 0x100, source, 2, output, 2])
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        "οσ"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    // The linguistic table may expand a scalar, but mapping is still
    // context-insensitive: the final sigma remains ordinary sigma.
    assert_eq!(
        engine
            .call_win64(MAP, [0x409, 0x0100_0100, source, 2, output, 2])
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        "οσ"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );

    engine.write(source, &0x00dfu16.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(MAP, [0x409, 0x0100_0200, source, 1, 0, 0])
            .unwrap(),
        2
    );
    assert_eq!(
        engine
            .call_win64(MAP, [0x409, 0x0100_0200, source, 1, output, 2])
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [b'S', 0, b'S', 0]
    );
}

#[test]
fn lc_map_string_w_reports_flags_lengths_buffers_locales_and_overlap() {
    const MAP: u64 = STUB_BASE + 0x508;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MAP, "kernel32.dll", "LCMapStringW").unwrap();
    let source = DATA_BASE + 0xf40;
    let output = DATA_BASE + 0xf60;
    engine.write(source, &[b'A', 0, 0, 0]).unwrap();
    engine.write(output, &[0xaa; 8]).unwrap();

    for (arguments, error) in [
        ([0x409, 0, source, 1, output, 1], 1004),
        ([0x409, 0x300, source, 1, output, 1], 1004),
        ([0x409, 0x0001_0100, source, 1, output, 1], 1004),
        ([0x409, 0x0002_0400, source, 1, output, 64], 1004),
        ([0x409, 0x100, 0, 1, output, 1], ERROR_INVALID_PARAMETER),
        (
            [0x409, 0x100, source, 0, output, 1],
            ERROR_INVALID_PARAMETER,
        ),
        ([0x409, 0x100, source, 1, 0, 1], ERROR_INVALID_PARAMETER),
        (
            [0x9999, 0x100, source, 1, output, 1],
            ERROR_INVALID_PARAMETER,
        ),
        ([0x409, 0x100, source, 2, output, 1], 122),
        ([0x409, 0x100, source, 2, source + 2, 2], 1004),
        ([0x409, 0x400, source, 1, source, 64], 1004),
    ] {
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(engine.call_win64(MAP, arguments).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().windows_last_error, error);
    }
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [0xaa; 8]
    );
}

#[test]
fn lc_map_string_w_preflights_complete_output_before_mutation() {
    const MAP: u64 = STUB_BASE + 0x508;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MAP, "kernel32.dll", "LCMapStringW").unwrap();
    let source = DATA_BASE + 0xf80;
    let crossing = DATA_BASE + PAGE_SIZE - 2;
    engine.write(source, &[b'A', 0, b'B', 0]).unwrap();
    engine.write(crossing, &[0x5a; 2]).unwrap();
    let error = engine
        .call_win64(MAP, [0x7f, 0x100, source, 2, crossing, 2])
        .unwrap_err();
    assert!(error.to_string().contains("not fully writable"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(crossing, 2).unwrap(),
        [0x5a; 2]
    );

    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, MAP, "kernel32.dll", "LCMapStringW").unwrap();
    let output = DATA_BASE + 0xfa0;
    engine.write(output, &[0x5a; 4]).unwrap();
    let error = engine
        .call_win64(MAP, [0x7f, 0x100, DATA_BASE + PAGE_SIZE, 2, output, 2])
        .unwrap_err();
    assert!(error.to_string().contains("source read failed"), "{error}");
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        [0x5a; 4]
    );
}

#[test]
fn get_environment_variable_a_rejects_invalid_names_and_outputs() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_get_environment_variable_a(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error == "GetEnvironmentVariableA name pointer is null")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    let name = DATA_BASE + 0xc00;
    engine
        .write(name, &vec![b'A'; MAX_WINDOWS_ENVIRONMENT_NAME_BYTES + 1])
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, name).unwrap();
    emulate_get_environment_variable_a(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("name exceeds 255 bytes"))
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.write(name, b"OPENCV_FOR_THREADS_NUM\0").unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, name).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 2).unwrap();
    emulate_get_environment_variable_a(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error == "GetEnvironmentVariableA output pointer is null")
    );
}

#[test]
fn get_environment_variable_w_matches_size_write_and_not_found_contracts() {
    const GET_ENVIRONMENT: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_ENVIRONMENT,
            "kernel32.dll",
            "GetEnvironmentVariableW",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetEnvironmentVariableW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetEnvironmentVariableW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    let name = DATA_BASE + 0xc00;
    let output = DATA_BASE + 0xd00;
    let mut encoded_name = Vec::new();
    for unit in "OpenCv_For_Threads_Num"
        .encode_utf16()
        .chain(std::iter::once(0))
    {
        encoded_name.extend_from_slice(&unit.to_le_bytes());
    }
    engine.write(name, &encoded_name).unwrap();
    engine.write(output, &[0xa5; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        2
    );
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, output, 1, 0, 0, 0])
            .unwrap(),
        2
    );
    let mut unchanged = [0; 8];
    engine.read(output, &mut unchanged).unwrap();
    assert_eq!(unchanged, [0xa5; 8]);
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, output, 2, 0, 0, 0])
            .unwrap(),
        1
    );
    let mut written = [0; 4];
    engine.read(output, &mut written).unwrap();
    assert_eq!(written, [b'1', 0, 0, 0]);

    let mut missing = Vec::new();
    for unit in "HOME".encode_utf16().chain(std::iter::once(0)) {
        missing.extend_from_slice(&unit.to_le_bytes());
    }
    engine.write(name, &missing).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, output, 2, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_ENVVAR_NOT_FOUND
    );
}

#[test]
fn get_environment_variable_w_rejects_invalid_guest_pointers() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_get_environment_variable_w(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("GetEnvironmentVariableW name pointer is null")
    );

    let name = DATA_BASE + 0xc00;
    engine.unicorn.get_data_mut().callback_error = None;
    engine.write(name, &vec![b'A'; 512]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, name).unwrap();
    emulate_get_environment_variable_w(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("name exceeds 255 UTF-16 units"))
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.write(name, &[0x00, 0xd8, 0, 0]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, name).unwrap();
    emulate_get_environment_variable_w(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("GetEnvironmentVariableW name is invalid UTF-16")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    let mut allowed = Vec::new();
    for unit in "OPENCV_FOR_THREADS_NUM"
        .encode_utf16()
        .chain(std::iter::once(0))
    {
        allowed.extend_from_slice(&unit.to_le_bytes());
    }
    engine.write(name, &allowed).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, name).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, 0xdead_beef)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 2).unwrap();
    emulate_get_environment_variable_w(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("output 0xdeadbeef is not writable"))
    );
}

#[test]
fn environment_strings_w_is_kernel32_scoped_sorted_writable_and_double_nul_terminated() {
    const GET_STRINGS: u64 = STUB_BASE + 0x510;
    const FREE_STRINGS: u64 = STUB_BASE + 0x520;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol, implementation) in [
        (
            GET_STRINGS,
            "GetEnvironmentStringsW",
            LegacyWin64Import::GetEnvironmentStringsW,
        ),
        (
            FREE_STRINGS,
            "FreeEnvironmentStringsW",
            LegacyWin64Import::FreeEnvironmentStringsW,
        ),
    ] {
        assert_eq!(
            install_win64_import(&mut engine.unicorn, stub, "KERNEL32.DLL", symbol).unwrap(),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    engine.unicorn.get_data_mut().windows_last_error = 0xdead_beef;
    let pointer = engine.call_win64(GET_STRINGS, [0, 0, 0, 0, 0, 0]).unwrap();
    assert_ne!(pointer, 0);
    let expected = "OPENCV_FOR_THREADS_NUM=1\0\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(pointer, expected.len())
            .unwrap(),
        expected
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0xdead_beef);

    // Windows returns caller-owned writable blocks, not a shared host buffer.
    engine.unicorn.mem_write(pointer, &[b'X', 0]).unwrap();
    assert_eq!(
        engine.unicorn.mem_read_as_vec(pointer, 2).unwrap(),
        [b'X', 0]
    );
    assert_eq!(
        engine
            .call_win64(FREE_STRINGS, [pointer, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0xdead_beef);
    assert!(engine.unicorn.mem_read_as_vec(pointer, 2).is_err());
}

#[test]
fn environment_strings_w_allocations_are_independent_owned_and_reject_invalid_free() {
    const GET_STRINGS: u64 = STUB_BASE + 0x510;
    const FREE_STRINGS: u64 = STUB_BASE + 0x520;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_STRINGS,
        "kernel32.dll",
        "GetEnvironmentStringsW",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        FREE_STRINGS,
        "kernel32.dll",
        "FreeEnvironmentStringsW",
    )
    .unwrap();

    let first = engine.call_win64(GET_STRINGS, [0, 0, 0, 0, 0, 0]).unwrap();
    let second = engine.call_win64(GET_STRINGS, [0, 0, 0, 0, 0, 0]).unwrap();
    assert_ne!(first, second);
    engine.unicorn.mem_write(first, &[b'X', 0]).unwrap();
    assert_eq!(
        engine.unicorn.mem_read_as_vec(second, 2).unwrap(),
        [b'O', 0]
    );

    assert_eq!(
        engine
            .call_win64(FREE_STRINGS, [first, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    for invalid in [first, DATA_BASE, 0] {
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(
            engine
                .call_win64(FREE_STRINGS, [invalid, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
    }
    assert_eq!(
        engine
            .call_win64(FREE_STRINGS, [second, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
}

#[test]
fn environment_strings_w_ownership_does_not_cross_guest_sessions() {
    const GET_STRINGS: u64 = STUB_BASE + 0x510;
    const FREE_STRINGS: u64 = STUB_BASE + 0x520;
    let mut first = test_engine(&[0xc3]);
    install_win64_import(
        &mut first.unicorn,
        GET_STRINGS,
        "kernel32.dll",
        "GetEnvironmentStringsW",
    )
    .unwrap();
    let mut second = test_engine(&[0xc3]);
    for (stub, symbol) in [
        (GET_STRINGS, "GetEnvironmentStringsW"),
        (FREE_STRINGS, "FreeEnvironmentStringsW"),
    ] {
        install_win64_import(&mut second.unicorn, stub, "kernel32.dll", symbol).unwrap();
    }
    let stale = first.call_win64(GET_STRINGS, [0, 0, 0, 0, 0, 0]).unwrap();
    let live = second.call_win64(GET_STRINGS, [0, 0, 0, 0, 0, 0]).unwrap();
    assert_ne!(stale, live, "sessions must not reuse environment tokens");
    assert_eq!(
        second
            .call_win64(FREE_STRINGS, [stale, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        second.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(
        second.unicorn.mem_read_as_vec(live, 2).unwrap(),
        [b'O', 0],
        "rejecting another session's token must preserve this session's allocation"
    );
    assert_eq!(
        second
            .call_win64(FREE_STRINGS, [live, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
}

#[test]
fn environment_strings_w_respects_shared_guest_allocation_budget() {
    const GET_STRINGS: u64 = STUB_BASE + 0x510;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_STRINGS,
        "kernel32.dll",
        "GetEnvironmentStringsW",
    )
    .unwrap();
    // Two maximum-size allocations consume the allocator's 128 MiB aggregate
    // budget without depending on the host process environment or allocator.
    let _first = allocate_crt_region(
        &mut engine.unicorn,
        crate::crt_heap::MAX_CRT_ALLOCATION_BYTES,
    )
    .unwrap();
    let _second = allocate_crt_region(
        &mut engine.unicorn,
        crate::crt_heap::MAX_CRT_ALLOCATION_BYTES,
    )
    .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0;
    assert_eq!(
        engine.call_win64(GET_STRINGS, [0, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 8);
}

#[test]
fn windows_last_error_is_session_local_and_tracks_missing_environment() {
    const GET_ENVIRONMENT: u64 = STUB_BASE + 0x540;
    const GET_LAST_ERROR: u64 = STUB_BASE + 0x550;
    const SET_LAST_ERROR: u64 = STUB_BASE + 0x560;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol, implementation) in [
        (
            GET_ENVIRONMENT,
            "GetEnvironmentVariableA",
            LegacyWin64Import::GetEnvironmentVariableA,
        ),
        (
            GET_LAST_ERROR,
            "GetLastError",
            LegacyWin64Import::GetLastError,
        ),
        (
            SET_LAST_ERROR,
            "SetLastError",
            LegacyWin64Import::SetLastError,
        ),
    ] {
        assert_eq!(
            install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap(),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    assert_eq!(engine.call_win64(GET_LAST_ERROR, [0; 6]).unwrap(), 0);
    assert_eq!(
        engine
            .call_win64(SET_LAST_ERROR, [0x1234_5678, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(GET_LAST_ERROR, [0; 6]).unwrap(),
        0x1234_5678
    );

    let name = DATA_BASE + 0xd80;
    engine.write(name, b"HOME\0").unwrap();
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(GET_LAST_ERROR, [0; 6]).unwrap(),
        u64::from(ERROR_ENVVAR_NOT_FOUND)
    );

    engine
        .call_win64(SET_LAST_ERROR, [77, 0, 0, 0, 0, 0])
        .unwrap();
    engine.write(name, b"OPENCV_FOR_THREADS_NUM\0").unwrap();
    assert_eq!(
        engine
            .call_win64(GET_ENVIRONMENT, [name, 0, 0, 0, 0, 0])
            .unwrap(),
        2
    );
    assert_eq!(engine.call_win64(GET_LAST_ERROR, [0; 6]).unwrap(), 77);

    let other = test_engine(&[0xc3]);
    assert_eq!(other.unicorn.get_data().windows_last_error, 0);
}

#[test]
fn set_thread_error_mode_is_stateful_validated_and_session_local() {
    const SET_MODE: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            SET_MODE,
            "kernel32.dll",
            "SetThreadErrorMode",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::SetThreadErrorMode)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "SetThreadErrorMode"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let old_mode = DATA_BASE + 0xe00;
    engine.write(old_mode, &[0xa5; 4]).unwrap();
    assert_eq!(
        engine
            .call_win64(SET_MODE, [0x0003, old_mode, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(old_mode, 4).unwrap(),
        0u32.to_le_bytes()
    );
    assert_eq!(engine.unicorn.get_data().windows_thread_error_mode, 0x0003);

    assert_eq!(
        engine
            .call_win64(SET_MODE, [0x8001, old_mode, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(old_mode, 4).unwrap(),
        0x0003u32.to_le_bytes()
    );
    assert_eq!(engine.unicorn.get_data().windows_thread_error_mode, 0x8001);

    engine.write(old_mode, &[0x5a; 4]).unwrap();
    assert_eq!(
        engine
            .call_win64(SET_MODE, [0x0004, old_mode, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(old_mode, 4).unwrap(),
        vec![0x5a; 4]
    );
    assert_eq!(engine.unicorn.get_data().windows_thread_error_mode, 0x8001);

    assert_eq!(engine.call_win64(SET_MODE, [0, 0, 0, 0, 0, 0]).unwrap(), 1);
    assert_eq!(engine.unicorn.get_data().windows_thread_error_mode, 0);
    let other = test_engine(&[0xc3]);
    assert_eq!(other.unicorn.get_data().windows_thread_error_mode, 0);
}

#[test]
fn set_thread_error_mode_does_not_commit_after_an_unwritable_old_mode_output() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.get_data_mut().windows_thread_error_mode = 0x0001;
    engine.unicorn.reg_write(RegisterX86::RCX, 0x0002).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, 0xdead_beef)
        .unwrap();
    emulate_set_thread_error_mode(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_thread_error_mode, 0x0001);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("old-mode output 0xdeadbeef is not writable"))
    );
}

#[test]
fn load_library_ex_w_is_allowlisted_bounded_and_library_scoped() {
    for library in ["kernel32.dll", "api-ms-win-core-libraryloader-l1-2-0.dll"] {
        const LOAD_LIBRARY: u64 = STUB_BASE + 0x1a0;
        let mut engine = test_engine(&[0xc3]);
        assert_eq!(
            install_win64_import(&mut engine.unicorn, LOAD_LIBRARY, library, "LoadLibraryExW",)
                .unwrap(),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LoadLibraryExW)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", "LoadLibraryExW"),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
        let path = DATA_BASE + 0xe00;
        let write_path = |engine: &mut GuestEngine<'static>, value: &str| {
            let mut bytes = Vec::new();
            for unit in value.encode_utf16().chain(std::iter::once(0)) {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            engine.write(path, &bytes).unwrap();
        };
        write_path(&mut engine, r"C:\Windows\System32\KERNEL32.DLL");
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0x0800, 0, 0, 0])
                .unwrap(),
            WINDOWS_KERNEL32_MODULE_TOKEN
        );

        engine.unicorn.get_data_mut().windows_last_error = 0;
        write_path(&mut engine, "dxgi.dll");
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0x1000, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );

        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0x8000_0000, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0x0808, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        write_path(&mut engine, "kernel32.dll");
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0x0100, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0x2000, 0, 0, 0])
                .unwrap(),
            WINDOWS_KERNEL32_MODULE_TOKEN
        );
        for non_executable_flag in [0x0002, 0x0020, 0x0040, 0x0062] {
            engine.unicorn.get_data_mut().windows_last_error = 0;
            assert_eq!(
                engine
                    .call_win64(LOAD_LIBRARY, [path, 0, non_executable_flag, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(
                engine.unicorn.get_data().windows_last_error,
                ERROR_INVALID_PARAMETER
            );
        }
        for malformed_absolute in [r"1:\kernel32.dll", r"\\kernel32.dll"] {
            write_path(&mut engine, malformed_absolute);
            assert_eq!(
                engine
                    .call_win64(LOAD_LIBRARY, [path, 0, 0x0100, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(
                engine.unicorn.get_data().windows_last_error,
                ERROR_INVALID_PARAMETER
            );
        }
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 1, 0, 0, 0, 0])
                .unwrap(),
            0
        );

        write_path(&mut engine, "");
        engine.unicorn.get_data_mut().callback_error = None;
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
        write_path(&mut engine, r"C:\Windows\System32\");
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
    }
}

#[test]
fn load_library_ex_w_rejects_invalid_or_unterminated_utf16_without_host_loading() {
    let mut engine = test_engine(&[0xc3]);
    let path = DATA_BASE + 0xc00;
    engine.write(path, &[0x00, 0xd8, 0, 0]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, path).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 0).unwrap();
    emulate_load_library_ex_w(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("LoadLibraryExW path is not valid UTF-16")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.write(path, &vec![b'A'; 520]).unwrap();
    emulate_load_library_ex_w(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("LoadLibraryExW path exceeds 259 UTF-16 code units")
    );
}

#[test]
fn raise_exception_reports_bounded_msvc_record_without_host_dispatch() {
    const RAISE: u64 = STUB_BASE + 0x198;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(&mut engine.unicorn, RAISE, "KERNEL32.DLL", "RaiseException").unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::RaiseException)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "RaiseException"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let arguments = DATA_BASE + 0xa00;
    let values = [0x1993_0520, DATA_BASE + 0x800, TEST_CODE + 0x950, TEST_CODE];
    engine
        .write(
            arguments,
            &values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let error = engine
        .call_win64(
            RAISE,
            [0xe06d_7363, 1, values.len() as u64, arguments, 0, 0],
        )
        .unwrap_err();
    assert!(
        error.to_string().contains(
            "unhandled guest RaiseException code=0xe06d7363 flags=0x1 parameters=[0x19930520,0x40000800,0x10000950,0x10000000]; x64 SEH dispatch is not modeled"
        ),
        "{error}"
    );
}

#[test]
fn raise_exception_validates_flags_count_and_complete_parameter_array() {
    let cases = [
        (0x2, 0, 0, "RaiseException flags 0x2 are invalid"),
        (
            0,
            16,
            DATA_BASE,
            "RaiseException parameter count 16 exceeds 15",
        ),
        (
            0,
            1,
            0,
            "RaiseException parameter array 0x0 is not fully readable for 1 entries",
        ),
        (
            0,
            2,
            DATA_BASE + DATA_SIZE - 8,
            "is not fully readable for 2 entries",
        ),
        (0, 1, u64::MAX - 3, "is not fully readable for 1 entries"),
    ];
    for (flags, count, arguments, expected) in cases {
        let mut engine = test_engine(&[0xc3]);
        engine.unicorn.reg_write(RegisterX86::RCX, 0x1234).unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, flags).unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, count).unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::R9, arguments)
            .unwrap();
        emulate_raise_exception(&mut engine.unicorn);
        assert!(
            engine
                .unicorn
                .get_data()
                .callback_error
                .as_deref()
                .is_some_and(|error| error.contains(expected))
        );
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    }
}

#[test]
fn raise_exception_allows_zero_parameters_without_reading_the_pointer() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0x1234).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::R8, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::R9, u64::MAX).unwrap();
    emulate_raise_exception(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some(
            "unhandled guest RaiseException code=0x1234 flags=0x0 parameters=[]; x64 SEH dispatch is not modeled"
        )
    );
}

#[test]
fn rtl_pc_to_file_header_resolves_only_modeled_guest_modules() {
    const RTL_PC: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            RTL_PC,
            "KERNEL32.DLL",
            "RtlPcToFileHeader",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::RtlPcToFileHeader)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "RtlPcToFileHeader"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let output = DATA_BASE + 0xb00;
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    for (pc, expected) in [
        (TEST_CODE, TEST_CODE),
        (TEST_CODE + PAGE_SIZE - 1, TEST_CODE),
        (WINDOWS_KERNEL32_MODULE_TOKEN, WINDOWS_KERNEL32_MODULE_TOKEN),
        (DATA_BASE, 0),
        (0, 0),
    ] {
        engine.write(output, &[0x5a; 8]).unwrap();
        assert_eq!(
            engine.call_win64(RTL_PC, [pc, output, 0, 0, 0, 0]).unwrap(),
            expected
        );
        let mut returned = [0; 8];
        engine.read(output, &mut returned).unwrap();
        assert_eq!(u64::from_le_bytes(returned), expected);
    }
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert_eq!(
        engine
            .call_win64(RTL_PC, [TEST_CODE, 0, 0, 0, 0, 0])
            .unwrap(),
        TEST_CODE
    );
}

#[test]
fn rtl_pc_to_file_header_rejects_invalid_outputs_without_mutation() {
    const RTL_PC: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        RTL_PC,
        "kernel32.dll",
        "RtlPcToFileHeader",
    )
    .unwrap();
    let readonly = TEST_CODE;
    engine
        .unicorn
        .mem_protect(readonly, PAGE_SIZE, Prot::READ | Prot::EXEC)
        .unwrap();
    let before = engine.unicorn.mem_read_as_vec(readonly, 8).unwrap();
    for output in [readonly, DATA_BASE + DATA_SIZE - 7, u64::MAX - 3] {
        assert_eq!(
            engine
                .call_win64(RTL_PC, [TEST_CODE, output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }
    assert_eq!(engine.unicorn.mem_read_as_vec(readonly, 8).unwrap(), before);
}

#[test]
fn wsa_startup_ordinal_writes_deterministic_x64_wsadata() {
    for library in ["ws2_32.dll", "wsock32.dll"] {
        const STARTUP: u64 = STUB_BASE + 0x1a8;
        let mut engine = test_engine(&[0xc3]);
        assert_eq!(
            install_win64_import(&mut engine.unicorn, STARTUP, library, "ORDINAL 115").unwrap(),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WsaStartup)
        );
        assert_eq!(
            dispatch_win64_import("ws2_32.dll", "WSAStartup"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::WsaStartup)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", "ORDINAL 115"),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
        let output = DATA_BASE + 0xc00;
        engine.write(output, &[0xaa; 408]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x1234;
        assert_eq!(
            engine
                .call_win64(STARTUP, [0x0002, output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut data = vec![0; 408];
        engine.read(output, &mut data).unwrap();
        assert_eq!(&data[0..4], &[0x02, 0x00, 0x02, 0x02]);
        assert_eq!(&data[16..57], b"AEXCompat deterministic Winsock 2.2 guest");
        assert_eq!(&data[273..280], b"Running");
        assert!(data[57..273].iter().all(|byte| *byte == 0));
        assert!(data[280..].iter().all(|byte| *byte == 0));
        assert_eq!(engine.unicorn.get_data().windows_socket_startups, 1);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
        assert_eq!(&data[4..16], &[0; 12]); // Win64 limits and vendor pointer
        let cleanup = STARTUP + 16;
        install_win64_import(&mut engine.unicorn, cleanup, library, "ORDINAL 116").unwrap();
        assert_eq!(engine.call_win64(cleanup, [0; 6]).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().windows_socket_startups, 0);
    }
}

#[test]
fn wsa_startup_rejects_versions_and_invalid_outputs_atomically() {
    const STARTUP: u64 = STUB_BASE + 0x1a8;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, STARTUP, "ws2_32.dll", "WSAStartup").unwrap();
    let output = DATA_BASE + 0xc00;
    for version in [0x0001, 0x0101, 0x0002, 0x0102, 0x0202] {
        assert_eq!(
            engine
                .call_win64(STARTUP, [version, output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }
    let count = engine.unicorn.get_data().windows_socket_startups;
    engine.write(output, &[0x5a; 408]).unwrap();
    assert_eq!(
        engine
            .call_win64(STARTUP, [0x0302, output, 0, 0, 0, 0])
            .unwrap(),
        u64::from(WINDOWS_WSAVERNOTSUPPORTED)
    );
    let mut unchanged = vec![0; 408];
    engine.read(output, &mut unchanged).unwrap();
    assert_eq!(unchanged, vec![0x5a; 408]);
    for invalid in [0, DATA_BASE + DATA_SIZE - 407, u64::MAX - 200] {
        assert_eq!(
            engine
                .call_win64(STARTUP, [0x0202, invalid, 0, 0, 0, 0])
                .unwrap(),
            u64::from(WINDOWS_WSAEFAULT)
        );
    }
    let readonly = TEST_CODE;
    engine
        .unicorn
        .mem_protect(readonly, PAGE_SIZE, Prot::READ | Prot::EXEC)
        .unwrap();
    let before = engine.unicorn.mem_read_as_vec(readonly, 408).unwrap();
    assert_eq!(
        engine
            .call_win64(STARTUP, [0x0202, readonly, 0, 0, 0, 0])
            .unwrap(),
        u64::from(WINDOWS_WSAEFAULT)
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(readonly, 408).unwrap(),
        before
    );
    assert_eq!(engine.unicorn.get_data().windows_socket_startups, count);
}

#[test]
fn wsa_startup_cleanup_balance_and_session_state_are_bounded() {
    const STARTUP: u64 = STUB_BASE + 0x1a8;
    const CLEANUP: u64 = STUB_BASE + 0x1b0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, STARTUP, "ws2_32.dll", "ORDINAL 115").unwrap();
    install_win64_import(&mut engine.unicorn, CLEANUP, "ws2_32.dll", "ORDINAL 116").unwrap();
    let output = DATA_BASE + 0xc00;
    for _ in 0..2 {
        assert_eq!(
            engine
                .call_win64(STARTUP, [0x0202, output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
    }
    assert_eq!(engine.unicorn.get_data().windows_socket_startups, 2);
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(engine.call_win64(CLEANUP, [0; 6]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert_eq!(engine.call_win64(CLEANUP, [0; 6]).unwrap(), 0);
    assert_eq!(
        engine.call_win64(CLEANUP, [0; 6]).unwrap(),
        u64::from(u32::MAX)
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        WINDOWS_WSANOTINITIALISED
    );
    engine.unicorn.get_data_mut().windows_socket_startups = MAX_WINDOWS_SOCKET_STARTUPS;
    assert_eq!(
        engine
            .call_win64(STARTUP, [0x0202, output, 0, 0, 0, 0])
            .unwrap(),
        u64::from(WINDOWS_WSAEPROCLIM)
    );
    assert_eq!(
        engine.unicorn.get_data().windows_socket_startups,
        MAX_WINDOWS_SOCKET_STARTUPS
    );
    let second_session = test_engine(&[0xc3]);
    assert_eq!(second_session.unicorn.get_data().windows_socket_startups, 0);
}

#[test]
fn load_library_a_is_allowlisted_bounded_and_library_scoped() {
    const LOAD_LIBRARY: u64 = STUB_BASE + 0x1a8;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            LOAD_LIBRARY,
            "KERNEL32.DLL",
            "LoadLibraryA",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LoadLibraryA)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "LoadLibraryA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let path = DATA_BASE + 0xd00;
    let write_path = |engine: &mut GuestEngine<'static>, value: &[u8]| {
        let mut bytes = value.to_vec();
        bytes.push(0);
        engine.write(path, &bytes).unwrap();
    };
    write_path(&mut engine, br"C:\Windows\System32\KERNEL32.DLL");
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
            .unwrap(),
        WINDOWS_KERNEL32_MODULE_TOKEN
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);

    engine.unicorn.get_data_mut().windows_last_error = 0;
    write_path(&mut engine, b"dxgi.dll");
    assert_eq!(
        engine
            .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    for ordinary_missing in [b"".as_slice(), br"C:\Windows\System32\".as_slice()] {
        write_path(&mut engine, ordinary_missing);
        assert_eq!(
            engine
                .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_MOD_NOT_FOUND
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }
    assert_eq!(
        engine.call_win64(LOAD_LIBRARY, [0, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
}

#[test]
fn load_library_a_rejects_unmapped_or_unterminated_paths_without_host_loading() {
    let mut engine = test_engine(&[0xc3]);
    let path = DATA_BASE + 0xc00;
    engine.write(path, &vec![b'A'; 260]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, path).unwrap();
    emulate_load_library_a(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("LoadLibraryA path exceeds 259 bytes")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_load_library_a(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("LoadLibraryA path read failed"))
    );
}

#[test]
fn load_library_a_fails_closed_for_missing_rgbranding_dependency() {
    const LOAD_LIBRARY: u64 = STUB_BASE + 0x1a8;
    const RG_BRANDING_ERROR: &str = "external guest dependency unavailable: RGBranding.dll (LoadLibraryA does not host-load DLLs)";
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        LOAD_LIBRARY,
        "kernel32.dll",
        "LoadLibraryA",
    )
    .unwrap();
    let path = DATA_BASE + 0xd00;
    engine
        .write(
            path,
            b"C:\\ProgramData\\Red Giant\\Common\\Libraries\\rGbRaNdInG.DlL\0",
        )
        .unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    let error = engine
        .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains(RG_BRANDING_ERROR));
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );

    let mut second_session = test_engine(&[0xc3]);
    install_win64_import(
        &mut second_session.unicorn,
        LOAD_LIBRARY,
        "kernel32.dll",
        "LoadLibraryA",
    )
    .unwrap();
    second_session.write(path, b"RGBranding.dll\0").unwrap();
    let error = second_session
        .call_win64(LOAD_LIBRARY, [path, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains(RG_BRANDING_ERROR));
    assert_eq!(
        second_session.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        0
    );
    assert_eq!(
        second_session.unicorn.get_data().windows_last_error,
        ERROR_MOD_NOT_FOUND
    );
}

#[test]
fn crt_heap_imports_return_null_for_overflow_and_budget_failure() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, u64::MAX)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 2).unwrap();
    emulate_crt_malloc(&mut engine.unicorn, true);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);

    engine
        .unicorn
        .reg_write(
            RegisterX86::RCX,
            crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1,
        )
        .unwrap();
    emulate_crt_malloc(&mut engine.unicorn, false);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

fn read_u64(engine: &GuestEngine<'static>, address: u64) -> u64 {
    let mut bytes = [0u8; 8];
    engine.read(address, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn call_aegp_new_mem_handle(
    engine: &mut GuestEngine<'static>,
    callback: u64,
    label: u64,
    output: u64,
    size: u64,
) -> (u64, u64) {
    engine.write(output, &u64::MAX.to_le_bytes()).unwrap();
    let result = engine
        .call_win64(callback, [1, label, size, 0, output, 0])
        .unwrap();
    (result, read_u64(engine, output))
}

fn aegp_memory_state_snapshot(
    engine: &GuestEngine<'static>,
) -> (u64, u64, Vec<(u64, u64, u64, u32, u64)>, Vec<(u64, u64)>) {
    let state = engine.unicorn.get_data();
    let mut handles = state
        .aegp_memory_handles
        .iter()
        .map(|(&handle, record)| (handle, record.data, record.size, record.locks, record.end))
        .collect::<Vec<_>>();
    handles.sort_by_key(|record| record.0);
    let free = state
        .aegp_memory_free
        .iter()
        .map(|block| (block.data, block.end))
        .collect();
    (
        state.next_handle_data,
        state.next_aegp_memory_handle,
        handles,
        free,
    )
}

#[test]
fn win64_call_places_register_arguments_and_returns_rax() {
    const CODE: u64 = 0x1000_0000;
    // mov rax, rcx; add rax, rdx; ret
    let mut engine = test_engine(&[0x48, 0x89, 0xc8, 0x48, 0x01, 0xd0, 0xc3]);
    assert_eq!(engine.call_win64(CODE, [40, 2, 0, 0, 0, 0]).unwrap(), 42);
}

#[test]
fn win64_import_dispatch_is_library_aware_and_case_normalized() {
    assert_eq!(
        dispatch_win64_import("UCRTBASE.DLL", "malloc"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Malloc)
    );
    assert_eq!(
        dispatch_win64_import(r"C:\Windows\System32\OPENCL.DLL", "malloc"),
        Win64ImportDispatch::UnsupportedGpuLibrary(GpuImportLibrary::OpenCl)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "malloc"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Malloc)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "unknown_scalar"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("KERNEL32.DLL", "unknown_system_symbol"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("KERNEL32.DLL", "GetSystemTimeAsFileTime"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetSystemTimeAsFileTime)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetSystemTimeAsFileTime"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("bcryptprimitives.dll", "ProcessPrng"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::ProcessPrng)
    );
    assert_eq!(
        dispatch_win64_import("BCRYPTPRIMITIVES.DLL", "ProcessPrng"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::ProcessPrng)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "ProcessPrng"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("kernel32.dll", "ProcessPrng"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    for (symbol, implementation) in [
        ("GetProcessHeap", LegacyWin64Import::GetProcessHeap),
        ("HeapAlloc", LegacyWin64Import::HeapAlloc),
        ("HeapFree", LegacyWin64Import::HeapFree),
        ("HeapReAlloc", LegacyWin64Import::HeapReAlloc),
    ] {
        assert_eq!(
            dispatch_win64_import("KERNEL32.DLL", symbol),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
    assert_eq!(
        canonical_import_trace_label(r"C:\Windows\System32\OPENCL.DLL", "clCreateKernel"),
        "opencl.dll!clCreateKernel"
    );
}

#[test]
fn process_prng_fills_a_reproducible_process_local_stream() {
    let mut engine = test_engine(&[0xc3]);
    let output = engine.allocate(32, 8).unwrap();
    engine.unicorn.mem_write(output, &[0x11; 32]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, output).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 16).unwrap();
    emulate_process_prng(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 1);
    let first = engine.unicorn.mem_read_as_vec(output, 16).unwrap();
    assert_ne!(first, vec![0x11; 16]);

    engine
        .unicorn
        .reg_write(RegisterX86::RCX, output + 16)
        .unwrap();
    emulate_process_prng(&mut engine.unicorn);
    let second = engine.unicorn.mem_read_as_vec(output + 16, 16).unwrap();
    assert_ne!(second, first, "successive calls must advance the stream");

    let mut replay = test_engine(&[0xc3]);
    let replay_output = replay.allocate(16, 8).unwrap();
    replay
        .unicorn
        .reg_write(RegisterX86::RCX, replay_output)
        .unwrap();
    replay.unicorn.reg_write(RegisterX86::RDX, 16).unwrap();
    emulate_process_prng(&mut replay.unicorn);
    assert_eq!(
        replay.unicorn.mem_read_as_vec(replay_output, 16).unwrap(),
        first,
        "fresh guest processes must replay the deterministic stream"
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    emulate_process_prng(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 1);
    assert!(engine.unicorn.get_data().callback_error.is_none());

    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 8).unwrap();
    emulate_process_prng(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error == "ProcessPrng buffer pointer is null")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.reg_write(RegisterX86::RCX, output).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, MAX_PROCESS_PRNG_BYTES + 1)
        .unwrap();
    emulate_process_prng(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("exceeds"))
    );
}

#[test]
fn process_heap_alloc_free_uses_an_opaque_handle_and_separate_ownership() {
    const GET_PROCESS_HEAP: u64 = STUB_BASE + 0x600;
    const HEAP_ALLOC: u64 = STUB_BASE + 0x610;
    const HEAP_FREE: u64 = STUB_BASE + 0x620;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET_PROCESS_HEAP,
        "kernel32.dll",
        "GetProcessHeap",
    )
    .unwrap();
    install_win64_import(&mut engine.unicorn, HEAP_ALLOC, "kernel32.dll", "HeapAlloc").unwrap();
    install_win64_import(&mut engine.unicorn, HEAP_FREE, "kernel32.dll", "HeapFree").unwrap();

    let heap = engine.call_win64(GET_PROCESS_HEAP, [0; 6]).unwrap();
    assert_eq!(heap, PROCESS_HEAP_HANDLE);
    let pointer = engine
        .call_win64(HEAP_ALLOC, [heap, u64::from(HEAP_ZERO_MEMORY), 32, 0, 0, 0])
        .unwrap();
    assert_ne!(pointer, 0);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(pointer, 32).unwrap(),
        vec![0; 32]
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .crt_heap
            .process_heap_allocation(pointer)
            .is_ok()
    );
    assert_eq!(
        engine.unicorn.get_data_mut().crt_heap.remove(pointer),
        Err(CrtHeapError::AllocatorMismatch),
        "CRT free must not own a process-heap allocation"
    );
    assert_eq!(
        engine
            .call_win64(HEAP_FREE, [heap, 0, pointer, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.call_win64(HEAP_FREE, [heap, 0, 0, 0, 0, 0]).unwrap(),
        1,
        "HeapFree accepts a null pointer"
    );
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);

    let crt_pointer = allocate_crt_region(&mut engine.unicorn, 8).unwrap();
    let error = engine
        .call_win64(HEAP_FREE, [heap, 0, crt_pointer, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("CRT free API does not own"));
}

#[test]
fn process_heap_realloc_preserves_bytes_zero_extends_and_keeps_old_on_failure() {
    const HEAP_ALLOC: u64 = STUB_BASE + 0x630;
    const HEAP_REALLOC: u64 = STUB_BASE + 0x640;
    const HEAP_FREE: u64 = STUB_BASE + 0x650;
    let mut engine = test_engine(&[0xc3]);
    for (address, symbol) in [
        (HEAP_ALLOC, "HeapAlloc"),
        (HEAP_REALLOC, "HeapReAlloc"),
        (HEAP_FREE, "HeapFree"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }

    let first = engine
        .call_win64(HEAP_ALLOC, [PROCESS_HEAP_HANDLE, 0, 8, 0, 0, 0])
        .unwrap();
    engine
        .unicorn
        .mem_write(first, &[1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
    let in_place = engine
        .call_win64(
            HEAP_REALLOC,
            [
                PROCESS_HEAP_HANDLE,
                u64::from(HEAP_ZERO_MEMORY),
                first,
                16,
                0,
                0,
            ],
        )
        .unwrap();
    assert_eq!(in_place, first);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(in_place, 16).unwrap(),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0]
    );

    let moved = engine
        .call_win64(HEAP_REALLOC, [PROCESS_HEAP_HANDLE, 0, in_place, 8192, 0, 0])
        .unwrap();
    assert_ne!(moved, 0);
    assert_ne!(moved, in_place);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(moved, 16).unwrap(),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0]
    );

    let refused = engine
        .call_win64(
            HEAP_REALLOC,
            [
                PROCESS_HEAP_HANDLE,
                u64::from(HEAP_REALLOC_IN_PLACE_ONLY),
                moved,
                16_384,
                0,
                0,
            ],
        )
        .unwrap();
    assert_eq!(refused, 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .crt_heap
            .process_heap_allocation(moved)
            .is_ok()
    );

    let oversized = engine
        .call_win64(
            HEAP_REALLOC,
            [
                PROCESS_HEAP_HANDLE,
                0,
                moved,
                crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1,
                0,
                0,
            ],
        )
        .unwrap();
    assert_eq!(oversized, 0);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(moved, 8).unwrap(),
        vec![1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(
        engine
            .call_win64(HEAP_FREE, [PROCESS_HEAP_HANDLE, 0, moved, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);
}

#[test]
fn private_heap_lifecycle_tracks_allocations_and_releases_them_on_destroy() {
    const HEAP_CREATE: u64 = STUB_BASE + 0x660;
    const HEAP_DESTROY: u64 = STUB_BASE + 0x670;
    const HEAP_ALLOC: u64 = STUB_BASE + 0x680;
    const HEAP_REALLOC: u64 = STUB_BASE + 0x690;
    let mut engine = test_engine(&[0xc3]);
    for (address, symbol) in [
        (HEAP_CREATE, "HeapCreate"),
        (HEAP_DESTROY, "HeapDestroy"),
        (HEAP_ALLOC, "HeapAlloc"),
        (HEAP_REALLOC, "HeapReAlloc"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }

    let heap = engine.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    assert_ne!(heap, 0);
    assert_ne!(heap, PROCESS_HEAP_HANDLE);
    let pointer = engine
        .call_win64(HEAP_ALLOC, [heap, u64::from(HEAP_ZERO_MEMORY), 8, 0, 0, 0])
        .unwrap();
    engine
        .unicorn
        .mem_write(pointer, &[1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
    let moved = engine
        .call_win64(HEAP_REALLOC, [heap, 0, pointer, 8192, 0, 0])
        .unwrap();
    assert_ne!(moved, pointer);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(moved, 8).unwrap(),
        vec![1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_private_heaps
            .get(&heap)
            .unwrap()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![moved]
    );
    assert_eq!(
        engine
            .call_win64(HEAP_DESTROY, [heap, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert!(
        !engine
            .unicorn
            .get_data()
            .windows_private_heaps
            .contains_key(&heap)
    );
    assert!(engine.unicorn.mem_read_as_vec(moved, 1).is_err());
    assert_eq!(engine.unicorn.get_data().crt_heap.live_bytes(), 0);
}

#[test]
fn private_heap_creation_is_bounded_and_session_handles_are_isolated() {
    const HEAP_CREATE: u64 = STUB_BASE + 0x6a0;
    let mut first = test_engine(&[0xc3]);
    let mut second = test_engine(&[0xc3]);
    for engine in [&mut first, &mut second] {
        install_win64_import(
            &mut engine.unicorn,
            HEAP_CREATE,
            "kernel32.dll",
            "HeapCreate",
        )
        .unwrap();
    }

    assert_eq!(
        first
            .call_win64(HEAP_CREATE, [u64::from(HEAP_ZERO_MEMORY), 0, 0, 0, 0, 0])
            .unwrap(),
        0,
        "allocation-only flags are not HeapCreate flags"
    );
    assert_eq!(
        first.call_win64(HEAP_CREATE, [0, 1, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        first.call_win64(HEAP_CREATE, [0, 0, 1, 0, 0, 0]).unwrap(),
        0
    );

    let first_handle = first.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    let second_handle = second.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    assert_ne!(first_handle, second_handle);
    for _ in 1..64 {
        assert_ne!(first.call_win64(HEAP_CREATE, [0; 6]).unwrap(), 0);
    }
    assert_eq!(first.call_win64(HEAP_CREATE, [0; 6]).unwrap(), 0);
}

#[test]
fn private_heap_rejects_foreign_stale_and_cross_heap_ownership() {
    const HEAP_CREATE: u64 = STUB_BASE + 0x6b0;
    const HEAP_DESTROY: u64 = STUB_BASE + 0x6c0;
    const HEAP_ALLOC: u64 = STUB_BASE + 0x6d0;
    const HEAP_FREE: u64 = STUB_BASE + 0x6e0;
    let prepare = || {
        let mut engine = test_engine(&[0xc3]);
        for (address, symbol) in [
            (HEAP_CREATE, "HeapCreate"),
            (HEAP_DESTROY, "HeapDestroy"),
            (HEAP_ALLOC, "HeapAlloc"),
            (HEAP_FREE, "HeapFree"),
        ] {
            install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
        }
        engine
    };

    let mut cross_heap = prepare();
    let owner = cross_heap.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    let other = cross_heap.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    let pointer = cross_heap
        .call_win64(HEAP_ALLOC, [owner, 0, 8, 0, 0, 0])
        .unwrap();
    let error = cross_heap
        .call_win64(HEAP_FREE, [other, 0, pointer, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("not owned by heap"));
    assert!(
        cross_heap
            .unicorn
            .get_data()
            .windows_private_heaps
            .get(&owner)
            .unwrap()
            .contains(&pointer)
    );

    let mut foreign = prepare();
    let error = foreign
        .call_win64(
            HEAP_ALLOC,
            [PRIVATE_HEAP_TOKEN_BASE + 0xffff, 0, 8, 0, 0, 0],
        )
        .unwrap_err();
    assert!(error.to_string().contains("unknown heap handle"));

    let mut stale = prepare();
    let heap = stale.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    assert_eq!(
        stale
            .call_win64(HEAP_DESTROY, [heap, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    let error = stale
        .call_win64(HEAP_DESTROY, [heap, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("unknown heap handle"));

    let mut budget = prepare();
    let heap = budget.call_win64(HEAP_CREATE, [0; 6]).unwrap();
    assert_eq!(
        budget
            .call_win64(
                HEAP_ALLOC,
                [
                    heap,
                    0,
                    crate::crt_heap::MAX_CRT_ALLOCATION_BYTES + 1,
                    0,
                    0,
                    0
                ],
            )
            .unwrap(),
        0
    );
    assert!(budget.unicorn.get_data().windows_private_heaps[&heap].is_empty());
}

#[test]
fn system_time_import_writes_current_validated_filetime() {
    let mut engine = test_engine(&[0xc3]);
    let output = engine.allocate(8, 8).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, output).unwrap();
    let before = windows_filetime(std::time::SystemTime::now()).unwrap();
    emulate_get_system_time_as_file_time(&mut engine.unicorn);
    let after = windows_filetime(std::time::SystemTime::now()).unwrap();
    let value = u64::from_le_bytes(
        engine
            .unicorn
            .mem_read_as_vec(output, 8)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert!((before..=after).contains(&value));
    assert!(engine.unicorn.get_data().callback_error.is_none());

    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_get_system_time_as_file_time(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("output pointer is null"))
    );
}

#[test]
fn get_system_info_writes_the_deterministic_win64_layout() {
    const GET_SYSTEM_INFO: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_SYSTEM_INFO,
            "kernel32.dll",
            "GetSystemInfo",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetSystemInfo)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetSystemInfo"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let output = engine.allocate(48, 8).unwrap();
    engine
        .call_win64(GET_SYSTEM_INFO, [output, 0, 0, 0, 0, 0])
        .unwrap();
    let info = engine.unicorn.mem_read_as_vec(output, 48).unwrap();
    assert_eq!(u16::from_le_bytes(info[0..2].try_into().unwrap()), 9);
    assert_eq!(u16::from_le_bytes(info[2..4].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(info[4..8].try_into().unwrap()), 4096);
    assert_eq!(
        u64::from_le_bytes(info[8..16].try_into().unwrap()),
        0x1_0000
    );
    assert_eq!(
        u64::from_le_bytes(info[16..24].try_into().unwrap()),
        0x0000_7fff_fffe_ffff
    );
    assert_eq!(u64::from_le_bytes(info[24..32].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(info[32..36].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(info[36..40].try_into().unwrap()), 8664);
    assert_eq!(u32::from_le_bytes(info[40..44].try_into().unwrap()), 65_536);
    assert_eq!(u16::from_le_bytes(info[44..46].try_into().unwrap()), 6);
    assert_eq!(u16::from_le_bytes(info[46..48].try_into().unwrap()), 0);
}

#[test]
fn get_system_info_rejects_null_and_unwritable_outputs() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_get_system_info(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("GetSystemInfo output pointer is null")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_get_system_info(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("output 0xdeadbeef is not writable"))
    );
}

#[test]
fn get_startup_info_w_writes_the_deterministic_win64_layout() {
    const GET_STARTUP_INFO: u64 = STUB_BASE + 0x1a8;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_STARTUP_INFO,
            "KERNEL32.DLL",
            "GetStartupInfoW",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetStartupInfoW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetStartupInfoW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let output = engine.allocate(104, 8).unwrap();
    engine.write(output, &[0xa5; 104]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RAX, 0x1234).unwrap();
    engine
        .call_win64(GET_STARTUP_INFO, [output, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0x1234);
    let startup_info = engine.unicorn.mem_read_as_vec(output, 104).unwrap();
    assert_eq!(
        u32::from_le_bytes(startup_info[..4].try_into().unwrap()),
        104
    );
    assert!(startup_info[4..].iter().all(|byte| *byte == 0));
}

#[test]
fn get_startup_info_w_rejects_null_and_unwritable_outputs() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_get_startup_info_w(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("GetStartupInfoW output pointer is null")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_get_startup_info_w(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| {
                error.contains("output 0xdeadbeef") && error.contains("not fully writable")
            })
    );

    let boundary = DATA_BASE + PAGE_SIZE - 52;
    let sentinel = [0x5a; 52];
    engine.write(boundary, &sentinel).unwrap();
    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, boundary)
        .unwrap();
    emulate_get_startup_info_w(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("is not fully writable"))
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(boundary, 52).unwrap(),
        sentinel
    );
}

#[test]
fn rtl_capture_context_writes_the_win64_caller_context() {
    const RTL_CAPTURE_CONTEXT: u64 = STUB_BASE + 0x1b0;
    const CONTEXT_SIZE: usize = 0x4d0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            RTL_CAPTURE_CONTEXT,
            "kernel32.dll",
            "RtlCaptureContext",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::RtlCaptureContext)
    );
    assert_eq!(
        dispatch_win64_import("ntdll.dll", "RtlCaptureContext"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let output = engine.allocate(CONTEXT_SIZE, 16).unwrap();
    engine.write(output, &vec![0xa5; CONTEXT_SIZE]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RAX, 0x1111).unwrap();
    engine.unicorn.reg_write(RegisterX86::RBX, 0x2222).unwrap();
    engine.unicorn.reg_write(RegisterX86::RBP, 0x3333).unwrap();
    engine.unicorn.reg_write(RegisterX86::RSI, 0x4444).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDI, 0x5555).unwrap();
    engine.unicorn.reg_write(RegisterX86::R10, 0xaaaa).unwrap();
    engine.unicorn.reg_write(RegisterX86::R11, 0xbbbb).unwrap();
    engine.unicorn.reg_write(RegisterX86::R12, 0xcccc).unwrap();
    engine.unicorn.reg_write(RegisterX86::R13, 0xdddd).unwrap();
    engine.unicorn.reg_write(RegisterX86::R14, 0xeeee).unwrap();
    engine.unicorn.reg_write(RegisterX86::R15, 0xffff).unwrap();
    let xmm0 = [0x5au8; 16];
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM0, &xmm0)
        .unwrap();
    let xmm15 = [0xc3u8; 16];
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM15, &xmm15)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::FPCW, 0x027f).unwrap();
    engine.unicorn.reg_write(RegisterX86::FPSW, 0x1821).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::FPTAG, 0x3fff)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::FOP, 0x0555).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::FIP, 0x1234_5678)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::FCS, 0x0033).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::FDP, 0x8765_4321)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::FDS, 0x002b).unwrap();
    let st4 = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    engine
        .unicorn
        .reg_write_long(RegisterX86::ST4, &st4)
        .unwrap();
    let expected_mxcsr = engine.unicorn.reg_read(RegisterX86::MXCSR).unwrap() as u32;
    let expected_eflags = engine.unicorn.reg_read(RegisterX86::EFLAGS).unwrap() as u32;
    let expected_segments = [
        RegisterX86::CS,
        RegisterX86::DS,
        RegisterX86::ES,
        RegisterX86::FS,
        RegisterX86::GS,
        RegisterX86::SS,
    ]
    .map(|register| engine.unicorn.reg_read(register).unwrap() as u16);
    engine
        .call_win64(RTL_CAPTURE_CONTEXT, [output, 0x7777, 0x8888, 0x9999, 0, 0])
        .unwrap();

    let context = engine
        .unicorn
        .mem_read_as_vec(output, CONTEXT_SIZE)
        .unwrap();
    let qword = |offset: usize| u64::from_le_bytes(context[offset..offset + 8].try_into().unwrap());
    assert_eq!(
        u32::from_le_bytes(context[0x30..0x34].try_into().unwrap()),
        0x0010_000f
    );
    assert_eq!(
        u32::from_le_bytes(context[0x34..0x38].try_into().unwrap()),
        expected_mxcsr
    );
    assert_eq!(
        u32::from_le_bytes(context[0x44..0x48].try_into().unwrap()),
        expected_eflags
    );
    assert_eq!(
        u16::from_le_bytes(context[0x100..0x102].try_into().unwrap()),
        0x027f
    );
    assert_eq!(
        u16::from_le_bytes(context[0x102..0x104].try_into().unwrap()),
        0x1821
    );
    assert_eq!(context[0x104], 0x80);
    assert_eq!(
        u16::from_le_bytes(context[0x106..0x108].try_into().unwrap()),
        0x0555
    );
    assert_eq!(
        u32::from_le_bytes(context[0x108..0x10c].try_into().unwrap()),
        0x1234_5678
    );
    assert_eq!(
        u16::from_le_bytes(context[0x10c..0x10e].try_into().unwrap()),
        0x0033
    );
    assert_eq!(
        u32::from_le_bytes(context[0x110..0x114].try_into().unwrap()),
        0x8765_4321
    );
    assert_eq!(
        u16::from_le_bytes(context[0x114..0x116].try_into().unwrap()),
        0x002b
    );
    assert_eq!(
        u32::from_le_bytes(context[0x11c..0x120].try_into().unwrap()),
        0x0000_ffff
    );
    assert_eq!(&context[0x160..0x16a], &st4);
    for (index, expected) in expected_segments.into_iter().enumerate() {
        let offset = 0x38 + index * 2;
        assert_eq!(
            u16::from_le_bytes(context[offset..offset + 2].try_into().unwrap()),
            expected
        );
    }
    assert_eq!(qword(0x78), 0x1111);
    assert_eq!(qword(0x80), output);
    assert_eq!(qword(0x88), 0x7777);
    assert_eq!(qword(0x90), 0x2222);
    assert_eq!(qword(0xa0), 0x3333);
    assert_eq!(qword(0xa8), 0x4444);
    assert_eq!(qword(0xb0), 0x5555);
    assert_eq!(qword(0xb8), 0x8888);
    assert_eq!(qword(0xc0), 0x9999);
    assert_eq!(qword(0xc8), 0xaaaa);
    assert_eq!(qword(0xd0), 0xbbbb);
    assert_eq!(qword(0xd8), 0xcccc);
    assert_eq!(qword(0xe0), 0xdddd);
    assert_eq!(qword(0xe8), 0xeeee);
    assert_eq!(qword(0xf0), 0xffff);
    assert_eq!(qword(0xf8), RETURN_ADDRESS);
    assert_eq!(qword(0x98), win64_entry_rsp(6) + 8);
    assert_eq!(&context[0x1a0..0x1b0], &xmm0);
    assert_eq!(&context[0x290..0x2a0], &xmm15);
    assert!(context[0x48..0x78].iter().all(|byte| *byte == 0));
    assert!(context[0x300..].iter().all(|byte| *byte == 0));
}

#[test]
fn rtl_capture_context_rejects_null_and_partially_writable_outputs() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_rtl_capture_context(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.get_data().callback_error.as_deref(),
        Some("RtlCaptureContext output pointer is null")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_rtl_capture_context(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("is not fully writable"))
    );

    let boundary = DATA_BASE + PAGE_SIZE - 0x268;
    let sentinel = vec![0x5a; 0x268];
    engine.write(boundary, &sentinel).unwrap();
    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, boundary)
        .unwrap();
    emulate_rtl_capture_context(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("is not fully writable"))
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(boundary, 0x268).unwrap(),
        sentinel
    );
}

#[test]
fn get_std_handle_returns_deterministic_synthetic_handles_and_is_library_scoped() {
    const GET_STD_HANDLE: u64 = STUB_BASE + 0x1b0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_STD_HANDLE,
            "KERNEL32.DLL",
            "GetStdHandle",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetStdHandle)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetStdHandle"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );

    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    for (selector, expected) in [
        ((-10i32) as u32, WINDOWS_STANDARD_INPUT_TOKEN),
        ((-11i32) as u32, WINDOWS_STANDARD_OUTPUT_TOKEN),
        ((-12i32) as u32, WINDOWS_STANDARD_ERROR_TOKEN),
    ] {
        assert_eq!(
            engine
                .call_win64(GET_STD_HANDLE, [u64::from(selector), 0, 0, 0, 0, 0])
                .unwrap(),
            expected
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    }
    assert_ne!(WINDOWS_STANDARD_INPUT_TOKEN, WINDOWS_STANDARD_OUTPUT_TOKEN);
    assert_ne!(WINDOWS_STANDARD_OUTPUT_TOKEN, WINDOWS_STANDARD_ERROR_TOKEN);
}

#[test]
fn get_std_handle_rejects_invalid_selector_without_host_handle_leakage() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_get_std_handle(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), u64::MAX);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_HANDLE
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    let mut second = test_engine(&[0xc3]);
    second
        .unicorn
        .reg_write(RegisterX86::RCX, u64::from((-11i32) as u32))
        .unwrap();
    emulate_get_std_handle(&mut second.unicorn);
    assert_eq!(
        second.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        WINDOWS_STANDARD_OUTPUT_TOKEN
    );
}

#[test]
fn get_console_mode_models_synthetic_standard_handles_as_redirected() {
    const GET_CONSOLE_MODE: u64 = STUB_BASE + 0x1b8;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_CONSOLE_MODE,
            "KERNEL32.DLL",
            "GetConsoleMode",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetConsoleMode)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetConsoleMode"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let output = DATA_BASE + 0xb00;
    let sentinel = 0x1234_5678u32.to_le_bytes();
    engine.write(output, &sentinel).unwrap();
    for handle in [
        WINDOWS_STANDARD_INPUT_TOKEN,
        WINDOWS_STANDARD_OUTPUT_TOKEN,
        WINDOWS_STANDARD_ERROR_TOKEN,
    ] {
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(
            engine
                .call_win64(GET_CONSOLE_MODE, [handle, output, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_HANDLE
        );
        assert_eq!(engine.unicorn.mem_read_as_vec(output, 4).unwrap(), sentinel);
    }
}

#[test]
fn get_console_mode_rejects_foreign_handles_without_touching_guest_memory() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, 0xdead_beef)
        .unwrap();
    emulate_get_console_mode(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_HANDLE
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    for mode_output in [0, 0xdead_beef] {
        engine
            .unicorn
            .reg_write(RegisterX86::RCX, WINDOWS_STANDARD_OUTPUT_TOKEN)
            .unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::RDX, mode_output)
            .unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0;
        emulate_get_console_mode(&mut engine.unicorn);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_HANDLE
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }

    let boundary = DATA_BASE + PAGE_SIZE - 2;
    let sentinel = [0x5a, 0xa5];
    engine.write(boundary, &sentinel).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, WINDOWS_STANDARD_ERROR_TOKEN)
        .unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, boundary)
        .unwrap();
    emulate_get_console_mode(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_HANDLE
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(boundary, 2).unwrap(),
        sentinel
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn get_file_type_classifies_synthetic_standard_handles_as_pipes() {
    const GET_FILE_TYPE: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_FILE_TYPE,
            "KERNEL32.DLL",
            "GetFileType",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetFileType)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetFileType"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    for handle in [
        WINDOWS_STANDARD_INPUT_TOKEN,
        WINDOWS_STANDARD_OUTPUT_TOKEN,
        WINDOWS_STANDARD_ERROR_TOKEN,
    ] {
        engine.unicorn.get_data_mut().windows_last_error = 0x1234;
        assert_eq!(
            engine
                .call_win64(GET_FILE_TYPE, [handle, 0, 0, 0, 0, 0])
                .unwrap(),
            3
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    }
}

#[test]
fn get_file_type_rejects_foreign_handles_without_host_descriptor_access() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_get_file_type(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_HANDLE
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    let mut second = test_engine(&[0xc3]);
    second
        .unicorn
        .reg_write(RegisterX86::RCX, WINDOWS_STANDARD_INPUT_TOKEN)
        .unwrap();
    emulate_get_file_type(&mut second.unicorn);
    assert_eq!(second.unicorn.reg_read(RegisterX86::RAX).unwrap(), 3);
}

fn call_test_nt_write_file(
    engine: &mut GuestEngine<'static>,
    stub: u64,
    arguments: [u64; 9],
) -> u64 {
    engine
        .call_win64_with_timeout(stub, &arguments, TIMEOUT_MICROSECONDS)
        .unwrap()
}

#[test]
fn nt_write_file_is_ntdll_scoped_and_sinks_stdout_and_stderr_synchronously() {
    const NT_WRITE_FILE: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            NT_WRITE_FILE,
            "NTDLL.DLL",
            "NtWriteFile"
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::NtWriteFile)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "NtWriteFile"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let payload = engine.allocate(5, 1).unwrap();
    engine.write(payload, b"hello").unwrap();
    let io_status = engine.allocate(16, 8).unwrap();
    for handle in [WINDOWS_STANDARD_OUTPUT_TOKEN, WINDOWS_STANDARD_ERROR_TOKEN] {
        engine.write(io_status, &[0xa5; 16]).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0x1234;
        assert_eq!(
            call_test_nt_write_file(
                &mut engine,
                NT_WRITE_FILE,
                [handle, 0, 0, 0, io_status, payload, 0x1_0000_0005, 0, 0],
            ),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(io_status, 16).unwrap(),
            [0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }
}

#[test]
fn nt_write_file_accepts_zero_length_null_buffer() {
    const NT_WRITE_FILE: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        NT_WRITE_FILE,
        "ntdll.dll",
        "NtWriteFile",
    )
    .unwrap();
    let io_status = engine.allocate(16, 8).unwrap();
    engine.write(io_status, &[0xa5; 16]).unwrap();
    assert_eq!(
        call_test_nt_write_file(
            &mut engine,
            NT_WRITE_FILE,
            [WINDOWS_STANDARD_ERROR_TOKEN, 0, 0, 0, io_status, 0, 0, 0, 0],
        ),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(io_status, 16).unwrap(),
        [0; 16]
    );
}

#[test]
fn nt_write_file_rejects_handles_and_unsupported_modes_atomically() {
    const NT_WRITE_FILE: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        NT_WRITE_FILE,
        "ntdll.dll",
        "NtWriteFile",
    )
    .unwrap();
    let payload = engine.allocate(1, 1).unwrap();
    engine.write(payload, b"x").unwrap();
    let io_status = engine.allocate(16, 8).unwrap();
    let sentinel = [0x5a; 16];
    for handle in [WINDOWS_STANDARD_INPUT_TOKEN, 0xdead_beef] {
        engine.write(io_status, &sentinel).unwrap();
        assert_eq!(
            call_test_nt_write_file(
                &mut engine,
                NT_WRITE_FILE,
                [handle, 0, 0, 0, io_status, payload, 1, 0, 0],
            ),
            0xc000_0008
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(io_status, 16).unwrap(),
            sentinel
        );
    }
    for arguments in [
        [
            WINDOWS_STANDARD_ERROR_TOKEN,
            1,
            0,
            0,
            io_status,
            payload,
            1,
            0,
            0,
        ],
        [
            WINDOWS_STANDARD_ERROR_TOKEN,
            0,
            1,
            0,
            io_status,
            payload,
            1,
            0,
            0,
        ],
        [
            WINDOWS_STANDARD_ERROR_TOKEN,
            0,
            0,
            1,
            io_status,
            payload,
            1,
            0,
            0,
        ],
        [
            WINDOWS_STANDARD_ERROR_TOKEN,
            0,
            0,
            0,
            io_status,
            payload,
            1,
            1,
            0,
        ],
        [
            WINDOWS_STANDARD_ERROR_TOKEN,
            0,
            0,
            0,
            io_status,
            payload,
            1,
            0,
            1,
        ],
        [
            WINDOWS_STANDARD_ERROR_TOKEN,
            0,
            0,
            0,
            io_status,
            payload,
            1024 * 1024 + 1,
            0,
            0,
        ],
    ] {
        engine.write(io_status, &sentinel).unwrap();
        assert_eq!(
            call_test_nt_write_file(&mut engine, NT_WRITE_FILE, arguments),
            0xc000_000d
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(io_status, 16).unwrap(),
            sentinel
        );
    }
}

#[test]
fn nt_write_file_preflights_payload_and_io_status_ranges_atomically() {
    const NT_WRITE_FILE: u64 = STUB_BASE + 0x1c0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        NT_WRITE_FILE,
        "ntdll.dll",
        "NtWriteFile",
    )
    .unwrap();
    let payload = engine.allocate(8, 1).unwrap();
    engine.write(payload, b"payload!").unwrap();
    let io_status = engine.allocate(16, 8).unwrap();
    let sentinel = [0x7c; 16];
    for bad_buffer in [0, 0xdead_beef, DATA_BASE + PAGE_SIZE - 4, u64::MAX] {
        engine.write(io_status, &sentinel).unwrap();
        assert_eq!(
            call_test_nt_write_file(
                &mut engine,
                NT_WRITE_FILE,
                [
                    WINDOWS_STANDARD_ERROR_TOKEN,
                    0,
                    0,
                    0,
                    io_status,
                    bad_buffer,
                    8,
                    0,
                    0
                ],
            ),
            0xc000_0005
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(io_status, 16).unwrap(),
            sentinel
        );
    }
    for bad_output in [0, 0xdead_beef, DATA_BASE + PAGE_SIZE - 8, u64::MAX - 7] {
        assert_eq!(
            call_test_nt_write_file(
                &mut engine,
                NT_WRITE_FILE,
                [
                    WINDOWS_STANDARD_ERROR_TOKEN,
                    0,
                    0,
                    0,
                    bad_output,
                    payload,
                    8,
                    0,
                    0
                ],
            ),
            0xc000_0005
        );
    }
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

fn write_test_wide_path(engine: &mut GuestEngine<'static>, path: &str) -> u64 {
    let units = path.encode_utf16().chain(std::iter::once(0));
    let bytes = units.flat_map(u16::to_le_bytes).collect::<Vec<_>>();
    let pointer = engine.allocate(bytes.len(), 2).unwrap();
    engine.write(pointer, &bytes).unwrap();
    pointer
}

#[test]
fn create_file_w_is_kernel32_scoped_and_returns_no_host_handle() {
    const CREATE_FILE: u64 = STUB_BASE + 0x1c4;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            CREATE_FILE,
            "KERNEL32.DLL",
            "CreateFileW",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CreateFileW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "CreateFileW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let path = write_test_wide_path(&mut engine, r"C:\AEXCompat\assets\model.bin");
    engine.unicorn.get_data_mut().windows_last_error = 0;
    let result = engine
        .call_win64_with_timeout(
            CREATE_FILE,
            &[path, 0x8000_0000, 1, 0, 3, 0x80, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_eq!(result, u64::MAX);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_FILE_NOT_FOUND
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn create_file_w_denies_traversal_device_unc_and_write_paths() {
    let mut engine = test_engine(&[0xc3]);
    for (path, access, disposition) in [
        (r"C:\AEXCompat\assets\..\..\secret.txt", 0x8000_0000, 3),
        (r"\\server\share\secret.txt", 0x8000_0000, 3),
        (r"\\.\PhysicalDrive0", 0x8000_0000, 3),
        (r"C:\AEXCompat\assets\output.bin", 0x4000_0000, 3),
        (r"C:\AEXCompat\assets\output.bin", 0x8000_0000, 2),
    ] {
        let pointer = write_test_wide_path(&mut engine, path);
        engine.unicorn.reg_write(RegisterX86::RCX, pointer).unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, access).unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, 1).unwrap();
        engine.unicorn.reg_write(RegisterX86::R9, 0).unwrap();
        let rsp = STACK_BASE + STACK_SIZE - 0x108 | 8;
        engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();
        for (index, value) in [disposition as u64, 0x80, 0].into_iter().enumerate() {
            engine
                .write(rsp + 0x28 + (index as u64 * 8), &value.to_le_bytes())
                .unwrap();
        }
        emulate_create_file_w(&mut engine.unicorn);
        assert_eq!(
            engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
            u64::MAX,
            "{path}"
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_ACCESS_DENIED,
            "{path}"
        );
    }
}

#[test]
fn create_file_w_validates_disposition_and_share() {
    let mut engine = test_engine(&[0xc3]);
    let pointer = write_test_wide_path(&mut engine, r"C:\AEXCompat\assets\model.bin");
    for args in [[pointer, 0x8000_0000, 8, 0, 3, 0x80, 0]] {
        engine.unicorn.reg_write(RegisterX86::RCX, args[0]).unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, args[1]).unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, args[2]).unwrap();
        engine.unicorn.reg_write(RegisterX86::R9, args[3]).unwrap();
        let rsp = STACK_BASE + STACK_SIZE - 0x108 | 8;
        engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();
        for (index, value) in args[4..].iter().copied().enumerate() {
            engine
                .write(rsp + 0x28 + index as u64 * 8, &value.to_le_bytes())
                .unwrap();
        }
        emulate_create_file_w(&mut engine.unicorn);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), u64::MAX);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
    }
}

#[test]
fn create_file_w_ignores_legal_optional_arguments_for_missing_open_existing_path() {
    const CREATE_FILE: u64 = STUB_BASE + 0x1c4;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CREATE_FILE,
        "kernel32.dll",
        "CreateFileW",
    )
    .unwrap();
    let pointer = write_test_wide_path(&mut engine, r"C:\AEXCompat\assets\missing.bin");
    for (security_attributes, template_file) in [(0xdead_beef, 0), (0, 0xdead_beef), (1, 2)] {
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    CREATE_FILE,
                    &[
                        pointer,
                        0x8000_0000,
                        1,
                        security_attributes,
                        3,
                        0x80,
                        template_file,
                    ],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            u64::MAX
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_FILE_NOT_FOUND
        );
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }
}

#[test]
fn create_file_w_treats_null_as_api_failure_but_aborts_on_unreadable_non_null_path() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_create_file_w(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), u64::MAX);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_PATH_NOT_FOUND
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());

    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_create_file_w(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), u64::MAX);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("not fully readable"))
    );
}

#[test]
fn create_file_w_distinguishes_maximum_and_over_limit_readable_paths() {
    const CREATE_FILE: u64 = STUB_BASE + 0x1c4;
    const MAX_PATH_UNITS: usize = 32_767;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CREATE_FILE,
        "kernel32.dll",
        "CreateFileW",
    )
    .unwrap();
    engine
        .unicorn
        .mem_map(DATA_BASE + PAGE_SIZE, 0x1_0000, Prot::READ | Prot::WRITE)
        .unwrap();
    let path = DATA_BASE + 0x800;

    for (non_nul_units, expected_error) in [
        (MAX_PATH_UNITS, ERROR_FILE_NOT_FOUND),
        (MAX_PATH_UNITS + 1, ERROR_FILENAME_EXCED_RANGE),
    ] {
        let mut bytes = Vec::with_capacity((non_nul_units + 1) * 2);
        for _ in 0..non_nul_units {
            bytes.extend_from_slice(&u16::from(b'a').to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        engine.write(path, &bytes).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 0;
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    CREATE_FILE,
                    &[path, 0x8000_0000, 1, 0, 3, 0x80, 0],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            u64::MAX
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, expected_error);
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }
}

#[test]
fn find_first_file_ex_w_is_kernel32_scoped_and_observes_empty_namespace() {
    const FIND_FIRST: u64 = STUB_BASE + 0x1c8;
    const FIND_DATA_SIZE: usize = 592;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            FIND_FIRST,
            "KERNEL32.DLL",
            "FindFirstFileExW",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::FindFirstFileExW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "FindFirstFileExW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let path = write_test_wide_path(&mut engine, r"C:\AEXCompat\assets\*.onnx");
    let output = engine.allocate(FIND_DATA_SIZE, 8).unwrap();
    let sentinel = vec![0xa5; FIND_DATA_SIZE];
    engine.write(output, &sentinel).unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 0;

    assert_eq!(
        engine
            .call_win64_with_timeout(
                FIND_FIRST,
                &[path, 1, output, 0, 0, 0x1_0000_0000],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u64::MAX
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_FILE_NOT_FOUND
    );
    let mut actual = vec![0; FIND_DATA_SIZE];
    engine.read(output, &mut actual).unwrap();
    assert_eq!(actual, sentinel);
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn find_first_file_ex_w_denies_traversal_device_and_unc_paths_without_output_mutation() {
    const FIND_DATA_SIZE: usize = 592;
    let mut engine = test_engine(&[0xc3]);
    let output = engine.allocate(FIND_DATA_SIZE, 8).unwrap();
    let sentinel = vec![0x5a; FIND_DATA_SIZE];
    for path in [
        r"D:\plugin\crates\core\../../model",
        r"\\server\share\*.onnx",
        r"\\.\PhysicalDrive0",
        r"\\?\C:\secret\*",
    ] {
        let path = write_test_wide_path(&mut engine, path);
        engine.write(output, &sentinel).unwrap();
        engine.unicorn.reg_write(RegisterX86::RCX, path).unwrap();
        engine.unicorn.reg_write(RegisterX86::RDX, 1).unwrap();
        engine.unicorn.reg_write(RegisterX86::R8, output).unwrap();
        engine.unicorn.reg_write(RegisterX86::R9, 0).unwrap();
        let rsp = STACK_BASE + STACK_SIZE - 0x108 | 8;
        engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();
        for (index, value) in [0u64, 0].into_iter().enumerate() {
            engine
                .write(rsp + 0x28 + index as u64 * 8, &value.to_le_bytes())
                .unwrap();
        }
        emulate_find_first_file_ex_w(&mut engine.unicorn);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), u64::MAX);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_ACCESS_DENIED
        );
        let mut actual = vec![0; FIND_DATA_SIZE];
        engine.read(output, &mut actual).unwrap();
        assert_eq!(actual, sentinel);
    }
}

#[test]
fn find_first_file_ex_w_validates_enums_flags_filter_and_output() {
    const FIND_FIRST: u64 = STUB_BASE + 0x1c8;
    const FIND_DATA_SIZE: usize = 592;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FIND_FIRST,
        "kernel32.dll",
        "FindFirstFileExW",
    )
    .unwrap();
    let path = write_test_wide_path(&mut engine, r"C:\AEXCompat\assets\*");
    let output = engine.allocate(FIND_DATA_SIZE, 8).unwrap();
    let sentinel = vec![0x3c; FIND_DATA_SIZE];

    for arguments in [
        [path, 2, output, 0, 0, 0],
        [path, 1, output, 3, 0, 0],
        [path, 1, output, 0, 1, 0],
        [path, 1, output, 0, 0, 8],
        [path, 1, 0, 0, 0, 0],
        [path, 1, 0xdead_beef, 0, 0, 0],
    ] {
        engine.write(output, &sentinel).unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(FIND_FIRST, &arguments, TIMEOUT_MICROSECONDS)
                .unwrap(),
            u64::MAX
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        let mut actual = vec![0; FIND_DATA_SIZE];
        engine.read(output, &mut actual).unwrap();
        assert_eq!(actual, sentinel);
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }

    assert_eq!(
        engine
            .call_win64_with_timeout(
                FIND_FIRST,
                &[path, 1, output, 2, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u64::MAX
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_NOT_SUPPORTED
    );

    for (info_level, search_op, flags) in [(0, 0, 0), (1, 1, 7)] {
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    FIND_FIRST,
                    &[path, info_level, output, search_op, 0, flags],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            u64::MAX
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_FILE_NOT_FOUND
        );
    }
}

#[test]
fn find_first_file_ex_w_bounds_and_validates_utf16_input() {
    const FIND_FIRST: u64 = STUB_BASE + 0x1c8;
    const FIND_DATA_SIZE: usize = 592;
    const MAX_PATH_UNITS: usize = 32_767;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        FIND_FIRST,
        "kernel32.dll",
        "FindFirstFileExW",
    )
    .unwrap();
    let output = engine.allocate(FIND_DATA_SIZE, 8).unwrap();

    for (path, expected_error) in [(0, ERROR_PATH_NOT_FOUND)] {
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    FIND_FIRST,
                    &[path, 1, output, 0, 0, 0],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            u64::MAX
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, expected_error);
    }
    let malformed = engine.allocate(4, 2).unwrap();
    engine.write(malformed, &[0x00, 0xd8, 0x00, 0x00]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                FIND_FIRST,
                &[malformed, 1, output, 0, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u64::MAX
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );

    engine
        .unicorn
        .mem_map(DATA_BASE + PAGE_SIZE, 0x1_0000, Prot::READ | Prot::WRITE)
        .unwrap();
    let long_path = DATA_BASE + 0x800;
    let mut bytes = Vec::with_capacity((MAX_PATH_UNITS + 2) * 2);
    for _ in 0..=MAX_PATH_UNITS {
        bytes.extend_from_slice(&u16::from(b'a').to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    engine.write(long_path, &bytes).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                FIND_FIRST,
                &[long_path, 1, output, 0, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        u64::MAX
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_FILENAME_EXCED_RANGE
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, 0xdead_beef)
        .unwrap();
    emulate_find_first_file_ex_w(&mut engine.unicorn);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("not fully readable"))
    );
}

#[test]
fn get_command_line_a_returns_stable_writable_guest_owned_storage() {
    const GET_COMMAND_LINE: u64 = STUB_BASE + 0x1c8;
    const COMMAND_LINE: &[u8] = b"\"aex-guest-worker.exe\"\0";
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_COMMAND_LINE,
            "KERNEL32.DLL",
            "GetCommandLineA",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetCommandLineA)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetCommandLineA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    initialize_windows_command_line_a(&mut engine).unwrap();
    let command_line = engine.unicorn.get_data().windows_command_line_a;
    for _ in 0..2 {
        assert_eq!(
            engine.call_win64(GET_COMMAND_LINE, [0; 6]).unwrap(),
            command_line
        );
    }
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(command_line, COMMAND_LINE.len())
            .unwrap(),
        COMMAND_LINE
    );
    engine.write(command_line + 1, b"A").unwrap();
    assert_eq!(
        engine.call_win64(GET_COMMAND_LINE, [0; 6]).unwrap(),
        command_line
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(command_line + 1, 1).unwrap(),
        b"A"
    );
}

#[test]
fn get_command_line_a_is_session_local_and_fails_closed_if_uninitialized() {
    const COMMAND_LINE: &[u8] = b"\"aex-guest-worker.exe\"\0";
    let mut first = test_engine(&[0xc3]);
    initialize_windows_command_line_a(&mut first).unwrap();
    let first_pointer = first.unicorn.get_data().windows_command_line_a;
    first.write(first_pointer + 1, b"X").unwrap();

    let mut second = test_engine(&[0xc3]);
    initialize_windows_command_line_a(&mut second).unwrap();
    let second_pointer = second.unicorn.get_data().windows_command_line_a;
    emulate_get_command_line_a(&mut second.unicorn);
    assert_eq!(
        second.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        second_pointer
    );
    assert_eq!(
        second
            .unicorn
            .mem_read_as_vec(second_pointer, COMMAND_LINE.len())
            .unwrap(),
        COMMAND_LINE
    );

    let mut uninitialized = test_engine(&[0xc3]);
    emulate_get_command_line_a(&mut uninitialized.unicorn);
    assert_eq!(uninitialized.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        uninitialized.unicorn.get_data().callback_error.as_deref(),
        Some("GetCommandLineA process string is not initialized")
    );
}

#[test]
fn get_command_line_w_returns_stable_aligned_writable_guest_storage() {
    const GET_COMMAND_LINE: u64 = STUB_BASE + 0x1d0;
    let mut expected = Vec::new();
    for unit in "\"aex-guest-worker.exe\""
        .encode_utf16()
        .chain(std::iter::once(0))
    {
        expected.extend_from_slice(&unit.to_le_bytes());
    }
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_COMMAND_LINE,
            "KERNEL32.DLL",
            "GetCommandLineW",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetCommandLineW)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetCommandLineW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    initialize_windows_command_line_w(&mut engine).unwrap();
    let pointer = engine.unicorn.get_data().windows_command_line_w;
    assert_eq!(pointer % 2, 0);
    for _ in 0..2 {
        assert_eq!(
            engine.call_win64(GET_COMMAND_LINE, [0; 6]).unwrap(),
            pointer
        );
    }
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(pointer, expected.len())
            .unwrap(),
        expected
    );
    engine
        .write(pointer + 2, &('A' as u16).to_le_bytes())
        .unwrap();
    assert_eq!(
        engine.call_win64(GET_COMMAND_LINE, [0; 6]).unwrap(),
        pointer
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(pointer + 2, 2).unwrap(),
        ('A' as u16).to_le_bytes()
    );
}

#[test]
fn get_command_line_w_is_session_local_and_fails_closed_if_uninitialized() {
    let mut first = test_engine(&[0xc3]);
    initialize_windows_command_line_w(&mut first).unwrap();
    let first_pointer = first.unicorn.get_data().windows_command_line_w;
    first
        .write(first_pointer + 2, &('X' as u16).to_le_bytes())
        .unwrap();

    let mut second = test_engine(&[0xc3]);
    initialize_windows_command_line_w(&mut second).unwrap();
    let second_pointer = second.unicorn.get_data().windows_command_line_w;
    emulate_get_command_line_w(&mut second.unicorn);
    assert_eq!(
        second.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        second_pointer
    );
    assert_eq!(
        second
            .unicorn
            .mem_read_as_vec(second_pointer + 2, 2)
            .unwrap(),
        ('a' as u16).to_le_bytes()
    );

    let mut uninitialized = test_engine(&[0xc3]);
    emulate_get_command_line_w(&mut uninitialized.unicorn);
    assert_eq!(uninitialized.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        uninitialized.unicorn.get_data().callback_error.as_deref(),
        Some("GetCommandLineW process string is not initialized")
    );
}

#[test]
fn get_acp_is_deterministic_cp932_and_matches_cp_acp_conversion() {
    const GET_ACP: u64 = STUB_BASE + 0x1d8;
    const CONVERT: u64 = STUB_BASE + 0x1e0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(&mut engine.unicorn, GET_ACP, "KERNEL32.DLL", "GetACP").unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetACP)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetACP"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    engine
        .unicorn
        .reg_write(RegisterX86::RAX, u64::MAX)
        .unwrap();
    assert_eq!(engine.call_win64(GET_ACP, [u64::MAX; 6]).unwrap(), 932);
    assert_eq!(engine.call_win64(GET_ACP, [0; 6]).unwrap(), 932);

    install_win64_import(
        &mut engine.unicorn,
        CONVERT,
        "kernel32.dll",
        "WideCharToMultiByte",
    )
    .unwrap();
    let source = DATA_BASE + 0xd00;
    let output = DATA_BASE + 0xd20;
    engine.write(source, &0x3042u16.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CONVERT,
                &[0, 0, source, 1, output, 2, 0, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        2
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 2).unwrap(),
        [0x82, 0xa0]
    );

    let mut second = test_engine(&[0xc3]);
    install_win64_import(&mut second.unicorn, GET_ACP, "kernel32.dll", "GetACP").unwrap();
    assert_eq!(second.call_win64(GET_ACP, [0; 6]).unwrap(), 932);
}

#[test]
fn get_cp_info_reports_cp932_and_rejects_invalid_outputs_atomically() {
    const GET_CP_INFO: u64 = STUB_BASE + 0x1e8;
    const EXPECTED: [u8; 20] = [
        2, 0, 0, 0, b'?', 0, 0x81, 0x9f, 0xe0, 0xfc, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            GET_CP_INFO,
            "KERNEL32.DLL",
            "GetCPInfo",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::GetCPInfo)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "GetCPInfo"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    let output = DATA_BASE + 0xe00;
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    engine
        .unicorn
        .reg_write(RegisterX86::RAX, u64::MAX)
        .unwrap();
    assert_eq!(
        engine
            .call_win64(GET_CP_INFO, [0, output, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 20).unwrap(),
        EXPECTED
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    engine.write(output, &[0xaa; 20]).unwrap();
    assert_eq!(
        engine
            .call_win64(GET_CP_INFO, [932, output, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 20).unwrap(),
        EXPECTED
    );

    engine.unicorn.get_data_mut().windows_last_error = 0;
    assert_eq!(
        engine
            .call_win64(GET_CP_INFO, [65001, output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 20).unwrap(),
        EXPECTED
    );

    for bad_output in [0, 0xdead_beef] {
        engine.unicorn.get_data_mut().callback_error = None;
        engine.unicorn.reg_write(RegisterX86::RCX, 932).unwrap();
        engine
            .unicorn
            .reg_write(RegisterX86::RDX, bad_output)
            .unwrap();
        emulate_get_cp_info(&mut engine.unicorn);
        assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
        assert!(engine.unicorn.get_data().callback_error.is_some());
    }

    let crossing = DATA_BASE + PAGE_SIZE - 10;
    engine.write(crossing, &[0x5a; 10]).unwrap();
    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.reg_write(RegisterX86::RCX, 932).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::RDX, crossing)
        .unwrap();
    emulate_get_cp_info(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(crossing, 10).unwrap(),
        [0x5a; 10]
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("not fully writable"))
    );
}

#[test]
fn is_debugger_present_is_false_deterministic_and_library_scoped() {
    const IS_DEBUGGER_PRESENT: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            IS_DEBUGGER_PRESENT,
            "kernel32.dll",
            "IsDebuggerPresent",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::IsDebuggerPresent)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "IsDebuggerPresent"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        engine
            .call_win64(IS_DEBUGGER_PRESENT, [u64::MAX; 6])
            .unwrap(),
        0
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn issue1347_create_thread_runs_bounded_guest_callback_and_completes_handle() {
    const CREATE: u64 = STUB_BASE + 0x1a0;
    const WAIT: u64 = STUB_BASE + 0x1b0;
    const CLOSE: u64 = STUB_BASE + 0x1c0;
    // mov [rcx], rsp; mov eax, 42; ret
    let mut engine = test_engine(&[0x48, 0x89, 0x21, 0xb8, 42, 0, 0, 0, 0xc3]);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let mut caller_teb_stack = [0u8; 16];
    caller_teb_stack[0..8].copy_from_slice(&(STACK_BASE + STACK_SIZE).to_le_bytes());
    caller_teb_stack[8..16].copy_from_slice(&STACK_BASE.to_le_bytes());
    engine.unicorn.mem_write(0x08, &caller_teb_stack).unwrap();
    for (address, symbol) in [
        (CREATE, "CreateThread"),
        (WAIT, "WaitForSingleObject"),
        (CLOSE, "CloseHandle"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let parameter = DATA_BASE + 0x300;
    let thread_id = DATA_BASE + 0x320;
    engine
        .unicorn
        .get_data_mut()
        .windows_tls_slots
        .insert(3, 0xaaaa);
    engine.unicorn.get_data_mut().windows_fls_slots.insert(
        4,
        WindowsFlsSlot {
            callback: 0,
            value: 0xbbbb,
        },
    );
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    let handle = engine
        .call_win64_with_timeout(
            CREATE,
            &[0, STACK_SIZE, TEST_CODE, parameter, 0x1_0000, thread_id],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_ne!(handle, 0);
    let callback_rsp = u64::from_le_bytes(
        engine
            .unicorn
            .mem_read_as_vec(parameter, 8)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert!(
        (WINDOWS_THREAD_STACK_BASE..WINDOWS_THREAD_STACK_BASE + STACK_SIZE).contains(&callback_rsp)
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(thread_id, 4).unwrap(),
        2u32.to_le_bytes()
    );
    let thread = engine
        .unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .unwrap();
    assert!(thread.completed);
    assert_eq!(thread.exit_code, 42);
    assert!(!thread.stack_mapped);
    assert!(engine.unicorn.mem_read_as_vec(callback_rsp, 1).is_err());
    assert_eq!(
        engine.unicorn.mem_read_as_vec(0x08, 16).unwrap(),
        caller_teb_stack
    );
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RSP).unwrap(),
        win64_entry_rsp(6) + 8
    );
    assert_eq!(engine.unicorn.get_data().current_windows_thread_id, 1);
    assert_eq!(
        engine.unicorn.get_data().windows_tls_slots.get(&3),
        Some(&0xaaaa)
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_fls_slots
            .get(&4)
            .unwrap()
            .value,
        0xbbbb
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert_eq!(
        engine
            .call_win64(WAIT, [handle, u32::MAX as u64, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.call_win64(CLOSE, [handle, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert!(
        !engine
            .unicorn
            .get_data()
            .windows_threads
            .contains_key(&handle)
    );
}

#[test]
fn issue1404_yielding_child_parks_then_parent_resumes_and_runs_child_to_completion() {
    const CREATE: u64 = STUB_BASE + 0x2a0;
    const SWITCH: u64 = STUB_BASE + 0x2b0;
    const CHILD_OFFSET: usize = 0x100;
    let parameter = DATA_BASE + 0x500;
    let mut code = vec![0x48, 0x83, 0xec, 0x38]; // sub rsp, 38h
    // CreateThread(NULL, 0, child, parameter, 0, NULL)
    code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
    push_mov_imm64(&mut code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
    push_mov_imm64(&mut code, [0x49, 0xb9], parameter);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], CREATE);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x89, 0xc3]); // call; mov rbx, rax
    push_mov_imm64(&mut code, [0x48, 0xb8], parameter);
    code.extend_from_slice(&[0x48, 0xc7, 0x00, 7, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x89, 0xd8, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28]);
    push_mov_imm64(&mut code, [0x48, 0xb8], parameter);
    code.extend_from_slice(&[0x8b, 0x00, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [(CREATE, "CreateThread"), (SWITCH, "SwitchToThread")] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let handle = engine.call_win64(TEST_CODE, [0; 6]).unwrap();
    let thread = engine
        .unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .unwrap();
    assert!(thread.completed);
    assert_eq!(thread.exit_code, 7);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(parameter, 8).unwrap(),
        7u64.to_le_bytes()
    );
    assert!(engine.scheduled_windows_threads.is_empty());
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.parked_main_context.is_none());
    assert_eq!(engine.unicorn.get_data().current_windows_thread_id, 1);
}

#[test]
fn issue1404_repeated_yields_share_one_total_timeout() {
    const SWITCH: u64 = STUB_BASE + 0x2b8;
    const SLOW: u64 = STUB_BASE + 0x2c8;
    const TIMEOUT_US: u64 = 12_000;
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    for _ in 0..3 {
        push_mov_imm64(&mut code, [0x48, 0xb8], SLOW);
        code.extend_from_slice(&[0xff, 0xd0]);
        push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
        code.extend_from_slice(&[0xff, 0xd0]);
    }
    code.extend_from_slice(&[0x48, 0x83, 0xc4, 0x28, 0xc3]);

    let mut engine = test_engine(&code);
    install_win64_import(
        &mut engine.unicorn,
        SWITCH,
        "kernel32.dll",
        "SwitchToThread",
    )
    .unwrap();
    engine.unicorn.mem_write(SLOW, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(SLOW, SLOW, |_, _, _| {
            std::thread::sleep(Duration::from_millis(8));
        })
        .unwrap();

    let started = Instant::now();
    let error = engine
        .call_win64_with_timeout(TEST_CODE, &[0; 6], TIMEOUT_US)
        .unwrap_err();
    let elapsed = started.elapsed();
    let error = error.to_string();
    assert!(
        error.contains("total timeout") || error.contains("before the guest returned"),
        "unexpected error after {elapsed:?}: {error}"
    );
    assert!(
        elapsed < Duration::from_millis(50),
        "repeated yields exceeded the bounded total timeout: {elapsed:?}"
    );
}

#[test]
fn issue1404_two_children_queue_fifo_and_complete_once() {
    const CREATE: u64 = STUB_BASE + 0x2c0;
    const SWITCH: u64 = STUB_BASE + 0x2d0;
    const CHILD_OFFSET: usize = 0x180;
    let first_output = DATA_BASE + 0x540;
    let second_output = DATA_BASE + 0x548;
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    let append_create = |code: &mut Vec<u8>, parameter: u64| {
        code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
        push_mov_imm64(code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
        push_mov_imm64(code, [0x49, 0xb9], parameter);
        code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
        code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
        push_mov_imm64(code, [0x48, 0xb8], CREATE);
        code.extend_from_slice(&[0xff, 0xd0]);
    };
    append_create(&mut code, first_output);
    append_create(&mut code, second_output);
    code.extend_from_slice(&[0x48, 0x89, 0xc3]);
    for _ in 0..2 {
        push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
        code.extend_from_slice(&[0xff, 0xd0]);
    }
    code.extend_from_slice(&[0x48, 0x89, 0xd8, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28]);
    code.extend_from_slice(&[0x48, 0xff, 0x01, 0xb8, 9, 0, 0, 0, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [(CREATE, "CreateThread"), (SWITCH, "SwitchToThread")] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let second_handle = engine.call_win64(TEST_CODE, [0; 6]).unwrap();
    assert_eq!(engine.unicorn.get_data().windows_threads.len(), 2);
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .values()
            .all(|thread| thread.completed && thread.exit_code == 9)
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .contains_key(&second_handle)
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(first_output, 8).unwrap(),
        1u64.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(second_output, 8).unwrap(),
        1u64.to_le_bytes()
    );
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.scheduled_windows_threads.is_empty());
}

#[test]
fn issue1404_reyielded_child_does_not_misclassify_a_new_synchronous_child() {
    const CREATE: u64 = STUB_BASE + 0x330;
    const SWITCH: u64 = STUB_BASE + 0x340;
    const CHILD_A_OFFSET: usize = 0x180;
    const CHILD_B_OFFSET: usize = 0x1c0;
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    let append_create = |code: &mut Vec<u8>, start: u64| {
        code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
        push_mov_imm64(code, [0x49, 0xb8], start);
        code.extend_from_slice(&[0x45, 0x31, 0xc9]);
        code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
        code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
        push_mov_imm64(code, [0x48, 0xb8], CREATE);
        code.extend_from_slice(&[0xff, 0xd0]);
    };
    append_create(&mut code, TEST_CODE + CHILD_A_OFFSET as u64);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0]);
    append_create(&mut code, TEST_CODE + CHILD_B_OFFSET as u64);
    code.extend_from_slice(&[0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_A_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28]);
    for _ in 0..2 {
        push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
        code.extend_from_slice(&[0xff, 0xd0]);
    }
    code.extend_from_slice(&[0x48, 0x83, 0xc4, 0x28, 0xb8, 7, 0, 0, 0, 0xc3]);
    code.resize(CHILD_B_OFFSET, 0x90);
    code.extend_from_slice(&[0xb8, 5, 0, 0, 0, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [(CREATE, "CreateThread"), (SWITCH, "SwitchToThread")] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let child_b_handle = engine.call_win64(TEST_CODE, [0; 6]).unwrap();
    assert_eq!(engine.unicorn.get_data().windows_threads.len(), 2);
    let exit_codes = engine
        .unicorn
        .get_data()
        .windows_threads
        .values()
        .map(|thread| thread.exit_code)
        .collect::<BTreeSet<_>>();
    assert_eq!(exit_codes, BTreeSet::from([5, 7]));
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .get(&child_b_handle)
            .unwrap()
            .exit_code,
        5
    );
    assert!(engine.scheduled_windows_threads.is_empty());
    assert!(engine.scheduler_ready.is_empty());
    assert!(engine.parked_main_context.is_none());
    assert!(!engine.unicorn.get_data().scheduler_resume_active);
}

#[test]
fn issue1404_context_switch_isolates_last_error_and_callee_saved_registers() {
    const CREATE: u64 = STUB_BASE + 0x2e0;
    const SWITCH: u64 = STUB_BASE + 0x2f0;
    const SET_LAST_ERROR: u64 = STUB_BASE + 0x300;
    const GET_LAST_ERROR: u64 = STUB_BASE + 0x310;
    const TLS_SET_VALUE: u64 = STUB_BASE + 0x320;
    const CHILD_OFFSET: usize = 0x140;
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    code.extend_from_slice(&[0x31, 0xc9, 0x31, 0xd2]);
    push_mov_imm64(&mut code, [0x49, 0xb8], TEST_CODE + CHILD_OFFSET as u64);
    code.extend_from_slice(&[0x45, 0x31, 0xc9]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x20, 0, 0, 0, 0]);
    code.extend_from_slice(&[0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], CREATE);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xbb], 0x1122_3344_5566_7788);
    code.extend_from_slice(&[0x66, 0x48, 0x0f, 0x6e, 0xf3]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], GET_LAST_ERROR);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code.resize(CHILD_OFFSET, 0x90);
    code.extend_from_slice(&[0x48, 0x83, 0xec, 0x28, 0xb9, 0x22, 0x22, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SET_LAST_ERROR);
    code.extend_from_slice(&[0xff, 0xd0]);
    code.extend_from_slice(&[0xb9, 3, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xba], 0xbbbb);
    push_mov_imm64(&mut code, [0x48, 0xb8], TLS_SET_VALUE);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xbb], 0xdead_beef_dead_beef);
    code.extend_from_slice(&[0x66, 0x48, 0x0f, 0x6e, 0xf3]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], GET_LAST_ERROR);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [
        (CREATE, "CreateThread"),
        (SWITCH, "SwitchToThread"),
        (SET_LAST_ERROR, "SetLastError"),
        (GET_LAST_ERROR, "GetLastError"),
        (TLS_SET_VALUE, "TlsSetValue"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    engine.unicorn.get_data_mut().windows_last_error = 0x1111;
    engine
        .unicorn
        .get_data_mut()
        .windows_tls_slots
        .insert(3, 0xaaaa);
    assert_eq!(engine.call_win64(TEST_CODE, [0; 6]).unwrap(), 0x1111);
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RBX).unwrap(),
        0x1122_3344_5566_7788
    );
    assert_eq!(
        &engine.unicorn.reg_read_long(RegisterX86::XMM6).unwrap()[..8],
        &0x1122_3344_5566_7788u64.to_le_bytes()
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .values()
            .next()
            .unwrap()
            .exit_code,
        0x2222
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1111);
    assert_eq!(
        engine.unicorn.get_data().windows_tls_slots.get(&3),
        Some(&0xaaaa)
    );
}

#[test]
fn issue1347_suspended_thread_times_out_then_resume_runs_once() {
    const CREATE: u64 = STUB_BASE + 0x1d0;
    const RESUME: u64 = STUB_BASE + 0x1e0;
    const WAIT_EX: u64 = STUB_BASE + 0x1f0;
    // inc qword ptr [rcx]; mov eax, 7; ret
    let mut engine = test_engine(&[0x48, 0xff, 0x01, 0xb8, 7, 0, 0, 0, 0xc3]);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [
        (CREATE, "CreateThread"),
        (RESUME, "ResumeThread"),
        (WAIT_EX, "WaitForSingleObjectEx"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let parameter = DATA_BASE + 0x340;
    let handle = engine
        .call_win64_with_timeout(
            CREATE,
            &[0, PAGE_SIZE * 3, TEST_CODE, parameter, 4 | 0x1_0000, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    let thread = engine
        .unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .unwrap();
    assert_eq!(thread.stack_size, PAGE_SIZE * 3);
    assert!(thread.stack_mapped);
    assert_eq!(
        engine.call_win64(WAIT_EX, [handle, 0, 1, 0, 0, 0]).unwrap(),
        258
    );
    assert_eq!(
        engine.call_win64(RESUME, [handle, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(parameter, 8).unwrap(),
        1u64.to_le_bytes()
    );
    assert!(
        !engine
            .unicorn
            .get_data()
            .windows_threads
            .get(&handle)
            .unwrap()
            .stack_mapped
    );
    assert_eq!(
        engine.call_win64(RESUME, [handle, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(parameter, 8).unwrap(),
        1u64.to_le_bytes()
    );
    assert_eq!(
        engine.call_win64(WAIT_EX, [handle, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
}

#[test]
fn issue1347_close_suspended_handle_succeeds_and_stack_is_session_owned() {
    const CREATE: u64 = STUB_BASE + 0x210;
    const CLOSE: u64 = STUB_BASE + 0x220;
    const WAIT: u64 = STUB_BASE + 0x230;
    let mut first = test_engine(&[0xc3]);
    first
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [
        (CREATE, "CreateThread"),
        (CLOSE, "CloseHandle"),
        (WAIT, "WaitForSingleObject"),
    ] {
        install_win64_import(&mut first.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let handle = first
        .call_win64_with_timeout(
            CREATE,
            &[0, PAGE_SIZE, TEST_CODE, 0, 4 | 0x1_0000, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_eq!(first.call_win64(CLOSE, [handle, 0, 0, 0, 0, 0]).unwrap(), 1);
    let thread = first
        .unicorn
        .get_data()
        .windows_threads
        .get(&handle)
        .unwrap();
    assert!(!thread.handle_open);
    assert!(thread.stack_mapped);
    assert_eq!(
        first.call_win64(WAIT, [handle, 0, 0, 0, 0, 0]).unwrap(),
        u32::MAX as u64
    );
    assert_eq!(
        first.unicorn.get_data().windows_last_error,
        ERROR_INVALID_HANDLE
    );

    drop(first);
    let mut second = test_engine(&[0xc3]);
    second
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    install_win64_import(&mut second.unicorn, CREATE, "kernel32.dll", "CreateThread").unwrap();
    let second_handle = second
        .call_win64_with_timeout(
            CREATE,
            &[0, PAGE_SIZE, TEST_CODE, 0, 4 | 0x1_0000, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_eq!(
        second
            .unicorn
            .get_data()
            .windows_threads
            .get(&second_handle)
            .unwrap()
            .stack_base,
        WINDOWS_THREAD_STACK_BASE
    );
}

#[test]
fn issue1347_thread_stack_slots_are_unique_bounded_and_report_exhaustion() {
    const CREATE: u64 = STUB_BASE + 0x238;
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    install_win64_import(&mut engine.unicorn, CREATE, "kernel32.dll", "CreateThread").unwrap();
    let mut bases = BTreeSet::new();
    for _ in 0..32 {
        let handle = engine
            .call_win64_with_timeout(
                CREATE,
                &[0, PAGE_SIZE, TEST_CODE, 0, 4 | 0x1_0000, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap();
        let thread = engine
            .unicorn
            .get_data()
            .windows_threads
            .get(&handle)
            .unwrap();
        assert_eq!(thread.stack_size, PAGE_SIZE);
        assert!(thread.stack_mapped);
        assert!(bases.insert(thread.stack_base));
    }
    assert_eq!(bases.len(), 32);
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CREATE,
                &[0, PAGE_SIZE, TEST_CODE, 0, 4 | 0x1_0000, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 8);
}

#[test]
fn issue1347_thread_exit_runs_installed_fls_destructor_repeat_passes() {
    const FLS_ALLOC: u64 = STUB_BASE + 0x240;
    const FLS_SET: u64 = STUB_BASE + 0x250;
    const CREATE: u64 = STUB_BASE + 0x260;
    const DESTRUCTOR_OFFSET: usize = 0x80;
    let value_output = DATA_BASE + 0x380;
    let count_output = DATA_BASE + 0x388;
    let mut code = vec![
        0x48, 0x83, 0xec, 0x28, 0xb9, 0, 0, 0, 0, 0xba, 0x34, 0x12, 0, 0,
    ];
    push_mov_imm64(&mut code, [0x48, 0xb8], FLS_SET);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0x31, 0xc0, 0xc3]);
    code.resize(DESTRUCTOR_OFFSET, 0x90);
    push_mov_imm64(&mut code, [0x48, 0xb8], value_output);
    code.extend_from_slice(&[0x48, 0x89, 0x08]);
    push_mov_imm64(&mut code, [0x48, 0xb8], count_output);
    code.extend_from_slice(&[0x48, 0xff, 0x00, 0x48, 0x83, 0x38, 0x02, 0x73, 0]);
    let jump_displacement = code.len() - 1;
    code.extend_from_slice(&[
        0x48, 0x83, 0xec, 0x28, 0xb9, 0, 0, 0, 0, 0xba, 0x78, 0x56, 0, 0,
    ]);
    push_mov_imm64(&mut code, [0x48, 0xb8], FLS_SET);
    code.extend_from_slice(&[0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28]);
    let destructor_return = code.len();
    code.push(0xc3);
    code[jump_displacement] = u8::try_from(destructor_return - jump_displacement - 1).unwrap();

    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (address, symbol) in [
        (FLS_ALLOC, "FlsAlloc"),
        (FLS_SET, "FlsSetValue"),
        (CREATE, "CreateThread"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let destructor = TEST_CODE + DESTRUCTOR_OFFSET as u64;
    assert_eq!(
        engine
            .call_win64(FLS_ALLOC, [destructor, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let handle = engine
        .call_win64_with_timeout(CREATE, &[0, 0, TEST_CODE, 0, 0, 0], TIMEOUT_MICROSECONDS)
        .unwrap();
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_threads
            .get(&handle)
            .unwrap()
            .completed
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(count_output, 8).unwrap(),
        2u64.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(value_output, 8).unwrap(),
        0x5678u64.to_le_bytes()
    );
    assert_eq!(
        engine
            .unicorn
            .get_data()
            .windows_fls_slots
            .get(&0)
            .unwrap()
            .value,
        0
    );
}

#[test]
fn issue1347_create_thread_rejects_unbounded_or_non_image_execution() {
    const CREATE: u64 = STUB_BASE + 0x200;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, CREATE, "kernel32.dll", "CreateThread").unwrap();
    for args in [
        [0, STACK_SIZE + 1, TEST_CODE, 0, 0, 0],
        [0, 0, DATA_BASE, 0, 0, 0],
        [1, 0, TEST_CODE, 0, 0, 0],
        [0, 0, TEST_CODE, 0, 0x8000_0000, 0],
        [0, 0, TEST_CODE, 0, 0, 0xdead_beef],
    ] {
        let error = engine
            .call_win64_with_timeout(CREATE, &args, TIMEOUT_MICROSECONDS)
            .unwrap_err()
            .to_string();
        assert!(error.contains("CreateThread"), "{error}");
        assert!(engine.unicorn.get_data().windows_threads.is_empty());
    }
}

#[test]
fn issue1347_create_thread_rejects_bad_caller_return_before_allocating() {
    let mut engine = test_engine(&[0xc3]);
    let rsp = STACK_BASE + 0x100;
    let thread_id = DATA_BASE + 0x380;
    engine
        .unicorn
        .mem_write(rsp, &0xdead_beefu64.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x28, &0u64.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x30, &thread_id.to_le_bytes())
        .unwrap();
    engine.unicorn.mem_write(thread_id, &[0xaa; 4]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::R8, TEST_CODE)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::R9, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();

    emulate_create_thread(&mut engine.unicorn);

    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("caller return"))
    );
    assert!(engine.unicorn.get_data().windows_threads.is_empty());
    assert_eq!(engine.unicorn.get_data().next_windows_thread_id, 2);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(thread_id, 4).unwrap(),
        [0xaa; 4]
    );
    assert!(
        engine
            .unicorn
            .mem_read_as_vec(WINDOWS_THREAD_STACK_BASE, 1)
            .is_err()
    );
}

#[test]
fn issue1347_create_thread_stack_map_failure_does_not_commit_outputs() {
    let mut engine = test_engine(&[0xc3]);
    let rsp = STACK_BASE + 0x100;
    let thread_id = DATA_BASE + 0x380;
    engine
        .unicorn
        .mem_map(
            WINDOWS_THREAD_STACK_BASE,
            PAGE_SIZE,
            Prot::READ | Prot::WRITE,
        )
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp, &RETURN_ADDRESS.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x28, &0u64.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x30, &thread_id.to_le_bytes())
        .unwrap();
    engine.unicorn.mem_write(thread_id, &[0xaa; 4]).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::R8, TEST_CODE)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::R9, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();

    emulate_create_thread(&mut engine.unicorn);

    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("stack map failed"))
    );
    assert!(engine.unicorn.get_data().windows_threads.is_empty());
    assert_eq!(engine.unicorn.get_data().next_windows_thread_id, 2);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(thread_id, 4).unwrap(),
        [0xaa; 4]
    );
}

#[test]
fn issue1347_create_thread_does_not_validate_output_against_its_new_stack() {
    let mut engine = test_engine(&[0xc3]);
    let rsp = STACK_BASE + 0x100;
    engine
        .unicorn
        .mem_write(rsp, &RETURN_ADDRESS.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x28, &0u64.to_le_bytes())
        .unwrap();
    engine
        .unicorn
        .mem_write(rsp + 0x30, &WINDOWS_THREAD_STACK_BASE.to_le_bytes())
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 0).unwrap();
    engine
        .unicorn
        .reg_write(RegisterX86::R8, TEST_CODE)
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::R9, 0).unwrap();
    engine.unicorn.reg_write(RegisterX86::RSP, rsp).unwrap();

    emulate_create_thread(&mut engine.unicorn);

    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("not fully writable"))
    );
    assert!(engine.unicorn.get_data().windows_threads.is_empty());
    assert_eq!(engine.unicorn.get_data().next_windows_thread_id, 2);
    assert!(
        engine
            .unicorn
            .mem_read_as_vec(WINDOWS_THREAD_STACK_BASE, 1)
            .is_err()
    );
}

#[test]
fn bounded_windows_runtime_imports_write_outputs_and_remain_library_scoped() {
    for (symbol, implementation) in [
        ("GetCurrentThreadId", LegacyWin64Import::GetCurrentThreadId),
        (
            "GetCurrentProcessId",
            LegacyWin64Import::GetCurrentProcessId,
        ),
        (
            "QueryPerformanceCounter",
            LegacyWin64Import::QueryPerformanceCounter,
        ),
        (
            "QueryPerformanceFrequency",
            LegacyWin64Import::QueryPerformanceFrequency,
        ),
        (
            "InitializeSListHead",
            LegacyWin64Import::InitializeSListHead,
        ),
        (
            "DisableThreadLibraryCalls",
            LegacyWin64Import::DisableThreadLibraryCalls,
        ),
    ] {
        assert_eq!(
            dispatch_win64_import("kernel32.dll", symbol),
            Win64ImportDispatch::LegacyImplemented(implementation)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    let mut engine = test_engine(&[0xc3]);
    let output = engine.allocate(16, 16).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, output).unwrap();
    emulate_query_performance_counter(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 1);
    assert!(
        engine
            .unicorn
            .get_data()
            .performance_counter_origin
            .is_some()
    );
    engine.unicorn.reg_write(RegisterX86::RCX, output).unwrap();
    emulate_query_performance_frequency(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 1);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        10_000_000u64.to_le_bytes()
    );
    engine.unicorn.mem_write(output, &[0xff; 16]).unwrap();
    emulate_initialize_slist_head(&mut engine.unicorn);
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 16).unwrap(), [0; 16]);
}

#[test]
fn performance_frequency_rejects_null_and_unwritable_outputs() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    emulate_query_performance_frequency(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error == "QueryPerformanceFrequency output pointer is null")
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.reg_write(RegisterX86::RCX, 0x1234).unwrap();
    emulate_query_performance_frequency(&mut engine.unicorn);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("output 0x1234 is not writable"))
    );
}

#[test]
fn fls_lifecycle_is_bounded_reuses_indices_and_runs_free_callback() {
    const ALLOC: u64 = STUB_BASE + 0x500;
    const GET: u64 = STUB_BASE + 0x510;
    const SET: u64 = STUB_BASE + 0x520;
    const FREE: u64 = STUB_BASE + 0x530;
    let mut engine = test_engine(&vec![0x90; 0x400]);
    engine.unicorn.get_data_mut().image_executable_ranges =
        vec![(TEST_CODE, TEST_CODE + PAGE_SIZE)];
    for (stub, symbol) in [
        (ALLOC, "FlsAlloc"),
        (GET, "FlsGetValue"),
        (SET, "FlsSetValue"),
        (FREE, "FlsFree"),
    ] {
        assert!(matches!(
            install_win64_import(&mut engine.unicorn, stub, "kernel32.dll", symbol).unwrap(),
            Win64ImportDispatch::LegacyImplemented(_)
        ));
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    let callback = TEST_CODE + 0x100;
    let marker = DATA_BASE + 0x700;
    let mut callback_code = vec![0x48, 0xb8]; // mov rax, marker
    callback_code.extend_from_slice(&marker.to_le_bytes());
    callback_code.extend_from_slice(&[0x48, 0x89, 0x08, 0xc3]); // mov [rax], rcx; ret
    engine.write(callback, &callback_code).unwrap();

    let index = engine.call_win64(ALLOC, [callback, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(index, 0);
    assert_eq!(engine.call_win64(GET, [index, 0, 0, 0, 0, 0]).unwrap(), 0);
    let value = 0x1234_5678_9abc_def0;
    assert_eq!(
        engine.call_win64(SET, [index, value, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        engine.call_win64(GET, [index, 0, 0, 0, 0, 0]).unwrap(),
        value
    );
    assert_eq!(engine.call_win64(FREE, [index, 0, 0, 0, 0, 0]).unwrap(), 1);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(marker, 8).unwrap(),
        value.to_le_bytes()
    );
    assert!(engine.unicorn.get_data().windows_fls_slots.is_empty());
    assert!(engine.unicorn.get_data().pending_fls_free.is_none());

    assert_eq!(engine.call_win64(ALLOC, [0; 6]).unwrap(), 0);
    assert_eq!(engine.call_win64(FREE, [0; 6]).unwrap(), 1);
    let error = engine.call_win64(FREE, [0; 6]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("FlsFree index 0 is not allocated"),
        "{error}"
    );
}

#[test]
fn fls_allocation_rejects_nonexecutable_callbacks_and_capacity_overflow() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, DATA_BASE)
        .unwrap();
    emulate_fls(&mut engine.unicorn, LegacyWin64Import::FlsAlloc);
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        u64::from(u32::MAX)
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("outside the executable image"))
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine.unicorn.reg_write(RegisterX86::RCX, 0).unwrap();
    for expected in 0..MAX_WINDOWS_FLS_SLOTS {
        emulate_fls(&mut engine.unicorn, LegacyWin64Import::FlsAlloc);
        assert_eq!(
            engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
            u64::from(expected)
        );
    }
    emulate_fls(&mut engine.unicorn, LegacyWin64Import::FlsAlloc);
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        u64::from(u32::MAX)
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("slot count exceeds"))
    );

    engine.unicorn.get_data_mut().callback_error = None;
    engine
        .unicorn
        .reg_write(RegisterX86::RCX, u64::from(MAX_WINDOWS_FLS_SLOTS))
        .unwrap();
    engine.unicorn.reg_write(RegisterX86::RDX, 1).unwrap();
    emulate_fls(&mut engine.unicorn, LegacyWin64Import::FlsSetValue);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("FlsSetValue index 128 is not allocated"))
    );

    engine.unicorn.get_data_mut().callback_error = None;
    emulate_fls(&mut engine.unicorn, LegacyWin64Import::FlsGetValue);
    assert_eq!(engine.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert!(
        engine
            .unicorn
            .get_data()
            .callback_error
            .as_deref()
            .is_some_and(|error| error.contains("FlsGetValue index 128 is not allocated"))
    );
}

#[test]
fn tls_lifecycle_is_bounded_stateful_and_library_scoped() {
    const ALLOC: u64 = STUB_BASE + 0x540;
    const GET: u64 = STUB_BASE + 0x550;
    const SET: u64 = STUB_BASE + 0x560;
    const FREE: u64 = STUB_BASE + 0x570;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol) in [
        (ALLOC, "TlsAlloc"),
        (GET, "TlsGetValue"),
        (SET, "TlsSetValue"),
        (FREE, "TlsFree"),
    ] {
        assert!(matches!(
            install_win64_import(&mut engine.unicorn, stub, "KERNEL32.DLL", symbol).unwrap(),
            Win64ImportDispatch::LegacyImplemented(_)
        ));
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    engine.unicorn.get_data_mut().windows_last_error = 0x1234;
    let index = engine.call_win64(ALLOC, [0; 6]).unwrap();
    assert_eq!(index, 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert_eq!(engine.call_win64(GET, [index, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0);

    let value = 0x1234_5678_9abc_def0;
    engine.unicorn.get_data_mut().windows_last_error = 0x5678;
    assert_eq!(
        engine.call_win64(SET, [index, value, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x5678);
    assert_eq!(
        engine.call_win64(GET, [index, 0, 0, 0, 0, 0]).unwrap(),
        value
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0);

    engine.unicorn.get_data_mut().windows_last_error = 0x9abc;
    assert_eq!(engine.call_win64(FREE, [index, 0, 0, 0, 0, 0]).unwrap(), 1);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x9abc);
    assert_eq!(engine.call_win64(GET, [index, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(engine.call_win64(SET, [index, 1, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert_eq!(engine.call_win64(FREE, [index, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.unicorn.get_data().windows_last_error,
        ERROR_INVALID_PARAMETER
    );
    assert!(engine.unicorn.get_data().callback_error.is_none());
}

#[test]
fn tls_allocation_reuses_indices_enforces_capacity_and_is_session_local() {
    let mut first = test_engine(&[0xc3]);
    for expected in 0..MAX_WINDOWS_TLS_SLOTS {
        emulate_tls(&mut first.unicorn, LegacyWin64Import::TlsAlloc);
        assert_eq!(
            first.unicorn.reg_read(RegisterX86::RAX).unwrap(),
            u64::from(expected)
        );
    }
    emulate_tls(&mut first.unicorn, LegacyWin64Import::TlsAlloc);
    assert_eq!(
        first.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        u64::from(u32::MAX)
    );
    assert_eq!(first.unicorn.get_data().windows_last_error, 8);

    first.unicorn.reg_write(RegisterX86::RCX, 5).unwrap();
    emulate_tls(&mut first.unicorn, LegacyWin64Import::TlsFree);
    assert_eq!(first.unicorn.reg_read(RegisterX86::RAX).unwrap(), 1);
    emulate_tls(&mut first.unicorn, LegacyWin64Import::TlsAlloc);
    assert_eq!(first.unicorn.reg_read(RegisterX86::RAX).unwrap(), 5);

    let mut second = test_engine(&[0xc3]);
    emulate_tls(&mut second.unicorn, LegacyWin64Import::TlsAlloc);
    assert_eq!(second.unicorn.reg_read(RegisterX86::RAX).unwrap(), 0);
    assert_eq!(second.unicorn.get_data().windows_tls_slots.len(), 1);
}

#[test]
fn win64_crt_math_imports_classify_without_bypassing_library_routing() {
    let crt_math = "api-ms-win-crt-math-l1-1-0.dll";
    assert_eq!(
        dispatch_win64_import(crt_math, "cosf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CosF)
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "ceilf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CeilF)
    );
    assert_eq!(
        dispatch_win64_import("ucrtbase.dll", "ceilf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CeilF)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "ceilf"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "lround"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LRound)
    );
    assert_eq!(
        dispatch_win64_import("ucrtbase.dll", "lround"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LRound)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "lround"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "lroundf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LRoundF)
    );
    assert_eq!(
        dispatch_win64_import("ucrtbase.dll", "lroundf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LRoundF)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "lroundf"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "cos"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Cos)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "cos"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "sinf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::SinF)
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "sin"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Sin)
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "sin"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("OpenCL.DLL", "cosf"),
        Win64ImportDispatch::UnsupportedGpuLibrary(GpuImportLibrary::OpenCl)
    );
    assert_eq!(
        dispatch_win64_import(crt_math, "fmodf"),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::FmodF)
    );
    assert_eq!(
        dispatch_win64_import("ucrtbase.dll", "fmodf"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("fixture.dll", "fmodf"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn win64_crt_fmodf_uses_scalar_xmm_abi_and_deterministic_c_edges() {
    const FMODF_IMPORT: u64 = STUB_BASE + 0x1f0;
    const XMM0_UPPER: [u8; 12] = [0xa5; 12];
    const XMM1_UPPER: [u8; 12] = [0x5a; 12];

    fn call_fmodf(engine: &mut GuestEngine<'static>, left: f32, right: f32) -> u32 {
        let mut xmm0 = [0u8; 16];
        xmm0[..4].copy_from_slice(&left.to_le_bytes());
        xmm0[4..].copy_from_slice(&XMM0_UPPER);
        let mut xmm1 = [0u8; 16];
        xmm1[..4].copy_from_slice(&right.to_le_bytes());
        xmm1[4..].copy_from_slice(&XMM1_UPPER);
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM1, &xmm1)
            .unwrap();
        engine.call_win64(FMODF_IMPORT, [0; 6]).unwrap();
        let result = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        assert_eq!(&result[4..], &XMM0_UPPER);
        assert_eq!(
            engine
                .unicorn
                .reg_read_long(RegisterX86::XMM1)
                .unwrap()
                .as_ref(),
            &xmm1
        );
        u32::from_le_bytes(result[..4].try_into().unwrap())
    }

    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(FMODF_IMPORT, &[0xc3]).unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            FMODF_IMPORT,
            "api-ms-win-crt-math-l1-1-0.dll",
            "fmodf",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::FmodF)
    );

    assert_eq!(call_fmodf(&mut engine, 5.5, 2.0), 1.5f32.to_bits());
    assert_eq!(
        call_fmodf(
            &mut engine,
            f32::from_bits(0x3e66_3922),
            f32::from_bits(0x42fc_0000),
        ),
        0x3e66_3922
    );
    assert_eq!(call_fmodf(&mut engine, -5.5, 2.0), (-1.5f32).to_bits());
    assert_eq!(call_fmodf(&mut engine, 5.5, -2.0), 1.5f32.to_bits());
    assert_eq!(call_fmodf(&mut engine, -4.0, 2.0), (-0.0f32).to_bits());
    assert_eq!(call_fmodf(&mut engine, -0.0, 3.0), (-0.0f32).to_bits());
    assert_eq!(
        call_fmodf(&mut engine, 3.0, f32::INFINITY),
        3.0f32.to_bits()
    );
    assert_eq!(call_fmodf(&mut engine, f32::INFINITY, 3.0), 0x7fc0_0000);
    assert_eq!(call_fmodf(&mut engine, 3.0, 0.0), 0x7fc0_0000);

    let signaling_nan = f32::from_bits(0xff81_2345);
    assert_eq!(call_fmodf(&mut engine, signaling_nan, 2.0), 0xffc1_2345);
    assert_eq!(
        call_fmodf(&mut engine, 2.0, f32::from_bits(0x7f81_5678)),
        0x7fc1_5678
    );
    assert_eq!(
        call_fmodf(&mut engine, f32::from_bits(3), f32::from_bits(2)),
        1
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .math_calls
            .iter()
            .any(|call| call.starts_with("fmodf("))
    );
}

#[test]
fn win64_crt_lround_uses_xmm0_f64_and_returns_a_windows_long_in_eax() {
    const LROUND_IMPORT: u64 = STUB_BASE + 0x1a8;

    fn call_lround(engine: &mut GuestEngine<'static>, input: f64) -> i32 {
        let mut xmm0 = [0xa5; 16];
        xmm0[..8].copy_from_slice(&input.to_le_bytes());
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(LROUND_IMPORT, [0; 6]).unwrap();
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap() as u32 as i32
    }

    let mut engine = test_engine(&[0xc3]);
    // Import slots are initially poison/placeholder code. The installer must
    // replace it with a bare return so the hook's EAX value survives.
    engine
        .unicorn
        .mem_write(LROUND_IMPORT, &[0x31, 0xc0, 0xc3])
        .unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            LROUND_IMPORT,
            "api-ms-win-crt-math-l1-1-0.dll",
            "lround",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LRound)
    );

    assert_eq!(call_lround(&mut engine, 200.4), 200);
    assert_eq!(call_lround(&mut engine, 1.5), 2);
    assert_eq!(call_lround(&mut engine, -1.5), -2);
    assert_eq!(call_lround(&mut engine, f64::NAN), i32::MIN);
    assert_eq!(call_lround(&mut engine, f64::INFINITY), i32::MIN);
    assert!(
        engine
            .unicorn
            .get_data()
            .math_calls
            .iter()
            .any(|call| call.starts_with("lround("))
    );
}

#[test]
fn win64_crt_lroundf_uses_scalar_xmm0_bits_and_windows_long_boundaries() {
    const LROUNDF_IMPORT: u64 = STUB_BASE + 0x1ac;
    // BlobTrack SHA 8eb82e8d... made one lroundf call in the SMART_RENDER
    // Release render-trace-png capture with this exact scalar XMM0 bit pattern.
    const OBSERVED_BLOBTRACK_XMM0_LOW_BITS: u32 = 0x0000_0000;

    fn call_lroundf(engine: &mut GuestEngine<'static>, input_bits: u32) -> (u64, [u8; 16]) {
        let mut xmm0 = [0xa5; 16];
        xmm0[..4].copy_from_slice(&input_bits.to_le_bytes());
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(LROUNDF_IMPORT, [0; 6]).unwrap();
        let output_xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        let mut output_xmm0_bytes = [0u8; 16];
        output_xmm0_bytes.copy_from_slice(&output_xmm0);
        (
            engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
            output_xmm0_bytes,
        )
    }

    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_write(LROUNDF_IMPORT, &[0x31, 0xc0, 0xc3])
        .unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            LROUNDF_IMPORT,
            "api-ms-win-crt-math-l1-1-0.dll",
            "lroundf",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::LRoundF)
    );
    engine.unicorn.get_data_mut().crt_errno = 73;
    engine.unicorn.get_data_mut().windows_last_error = 0x1234;

    for (input, expected) in [
        (f32::from_bits(OBSERVED_BLOBTRACK_XMM0_LOW_BITS), 0i32),
        (f32::from_bits(0x0000_0001), 0),
        (f32::from_bits(0x8000_0001), 0),
        (-0.0, 0),
        (1.4, 1),
        (1.5, 2),
        (-1.5, -2),
        (f32::from_bits(0x4eff_ffff), 2_147_483_520),
        (-2_147_483_648.0, i32::MIN),
        (2_147_483_648.0, i32::MIN),
        (f32::INFINITY, i32::MIN),
        (f32::NEG_INFINITY, i32::MIN),
        (f32::from_bits(0x7fc1_2345), i32::MIN),
    ] {
        let (rax, xmm0) = call_lroundf(&mut engine, input.to_bits());
        assert_eq!(
            rax,
            u64::from(expected as u32),
            "input bits {:#010x}",
            input.to_bits()
        );
        let mut original = [0xa5; 16];
        original[..4].copy_from_slice(&input.to_bits().to_le_bytes());
        assert_eq!(xmm0, original, "lroundf must not mutate XMM0");
    }
    assert_eq!(engine.unicorn.get_data().crt_errno, 73);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0x1234);
    assert!(
        engine
            .unicorn
            .get_data()
            .math_calls
            .iter()
            .any(|call| call.starts_with("lroundf("))
    );
}

#[test]
fn win64_crt_round_floor_ceil_family_uses_xmm0_and_preserves_edges() {
    const ROUND: u64 = STUB_BASE + 0x1a0;
    const FLOOR: u64 = STUB_BASE + 0x1b0;
    const CEIL: u64 = STUB_BASE + 0x1c0;
    const ROUNDF: u64 = STUB_BASE + 0x1d0;
    let mut engine = test_engine(&[0xc3]);
    for (stub, symbol, expected) in [
        (ROUND, "round", LegacyWin64Import::Round),
        (FLOOR, "floor", LegacyWin64Import::Floor),
        (CEIL, "ceil", LegacyWin64Import::Ceil),
        (ROUNDF, "roundf", LegacyWin64Import::RoundF),
    ] {
        engine.unicorn.mem_write(stub, &[0xc3]).unwrap();
        assert_eq!(
            install_win64_import(
                &mut engine.unicorn,
                stub,
                "api-ms-win-crt-math-l1-1-0.dll",
                symbol,
            )
            .unwrap(),
            Win64ImportDispatch::LegacyImplemented(expected)
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }

    let call_f64 = |engine: &mut GuestEngine<'static>, stub, value: f64| {
        let mut xmm0 = [0xa5; 16];
        xmm0[..8].copy_from_slice(&value.to_le_bytes());
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(stub, [0; 6]).unwrap();
        let result = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        assert_eq!(&result[8..], &[0xa5; 8]);
        f64::from_le_bytes(result[..8].try_into().unwrap())
    };
    assert_eq!(call_f64(&mut engine, ROUND, 1.5), 2.0);
    assert_eq!(call_f64(&mut engine, ROUND, -1.5), -2.0);
    assert_eq!(call_f64(&mut engine, FLOOR, -1.25), -2.0);
    assert_eq!(call_f64(&mut engine, CEIL, -1.25), -1.0);
    assert_eq!(
        call_f64(&mut engine, ROUND, -0.0).to_bits(),
        (-0.0f64).to_bits()
    );
    assert!(call_f64(&mut engine, ROUND, f64::NAN).is_nan());
    assert_eq!(call_f64(&mut engine, FLOOR, f64::INFINITY), f64::INFINITY);

    let mut xmm0 = [0x5a; 16];
    xmm0[..4].copy_from_slice(&(-2.5f32).to_le_bytes());
    engine
        .unicorn
        .reg_write_long(RegisterX86::XMM0, &xmm0)
        .unwrap();
    engine.call_win64(ROUNDF, [0; 6]).unwrap();
    let result = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
    assert_eq!(f32::from_le_bytes(result[..4].try_into().unwrap()), -3.0);
}

#[test]
fn win64_crt_ceilf_uses_xmm0_f32_abi_and_preserves_special_values() {
    const CEILF_IMPORT: u64 = STUB_BASE + 0x1a0;

    fn call_ceilf(engine: &mut GuestEngine<'static>, input: f32) -> f32 {
        let upper = [0xa5; 12];
        let mut xmm0 = [0u8; 16];
        xmm0[..4].copy_from_slice(&input.to_le_bytes());
        xmm0[4..].copy_from_slice(&upper);
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(CEILF_IMPORT, [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        f32::from_le_bytes(xmm0[..4].try_into().unwrap())
    }

    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(CEILF_IMPORT, &[0xc3]).unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            CEILF_IMPORT,
            "api-ms-win-crt-math-l1-1-0.dll",
            "ceilf",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CeilF)
    );

    assert_eq!(call_ceilf(&mut engine, 1.25), 2.0);
    assert_eq!(call_ceilf(&mut engine, -1.25), -1.0);
    assert_eq!(call_ceilf(&mut engine, f32::INFINITY), f32::INFINITY);
    assert_eq!(
        call_ceilf(&mut engine, f32::NEG_INFINITY),
        f32::NEG_INFINITY
    );
    assert!(call_ceilf(&mut engine, f32::NAN).is_nan());
    assert_eq!(call_ceilf(&mut engine, -0.0).to_bits(), (-0.0f32).to_bits());
    assert!(
        engine
            .unicorn
            .get_data()
            .math_calls
            .iter()
            .any(|call| call.starts_with("ceilf("))
    );
}

#[test]
fn win64_crt_sincos_use_xmm0_f32_abi_and_bound_math_trace() {
    const COSF_IMPORT: u64 = STUB_BASE + 0x1b0;
    const SINF_IMPORT: u64 = STUB_BASE + 0x1c0;

    fn call_f32(engine: &mut GuestEngine<'static>, import: u64, input: f32) -> f32 {
        let mut xmm0 = [0u8; 16];
        xmm0[..4].copy_from_slice(&input.to_le_bytes());
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(import, [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        f32::from_le_bytes(xmm0[..4].try_into().unwrap())
    }

    let mut engine = test_engine(&[0xc3]);
    for (import, symbol, expected) in [
        (COSF_IMPORT, "cosf", LegacyWin64Import::CosF),
        (SINF_IMPORT, "sinf", LegacyWin64Import::SinF),
    ] {
        engine.unicorn.mem_write(import, &[0xc3]).unwrap();
        assert_eq!(
            install_win64_import(
                &mut engine.unicorn,
                import,
                "api-ms-win-crt-math-l1-1-0.dll",
                symbol,
            )
            .unwrap(),
            Win64ImportDispatch::LegacyImplemented(expected)
        );
    }

    assert_eq!(call_f32(&mut engine, COSF_IMPORT, 0.0), 1.0);
    assert_eq!(call_f32(&mut engine, SINF_IMPORT, 0.0), 0.0);
    let cosine = call_f32(&mut engine, COSF_IMPORT, std::f32::consts::FRAC_PI_2);
    let sine = call_f32(&mut engine, SINF_IMPORT, std::f32::consts::FRAC_PI_2);
    assert!(cosine.is_finite());
    assert!(cosine.abs() <= f32::EPSILON);
    assert!(sine.is_finite());
    assert_eq!(sine, 1.0);

    for _ in 0..40 {
        assert!(call_f32(&mut engine, COSF_IMPORT, 0.25).is_finite());
    }
    let math_calls = &engine.unicorn.get_data().math_calls;
    assert_eq!(math_calls.len(), 32);
    assert!(math_calls.iter().any(|call| call.starts_with("cosf(")));
    assert!(math_calls.iter().any(|call| call.starts_with("sinf(")));
}

#[test]
fn win64_crt_sin_uses_xmm0_f64_abi_and_preserves_special_values() {
    const SIN_IMPORT: u64 = STUB_BASE + 0x1d0;

    fn call_f64(engine: &mut GuestEngine<'static>, input: f64) -> f64 {
        let mut xmm0 = [0u8; 16];
        xmm0[..8].copy_from_slice(&input.to_le_bytes());
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(SIN_IMPORT, [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        f64::from_le_bytes(xmm0[..8].try_into().unwrap())
    }

    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(SIN_IMPORT, &[0xc3]).unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            SIN_IMPORT,
            "api-ms-win-crt-math-l1-1-0.dll",
            "sin",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Sin)
    );
    assert_eq!(call_f64(&mut engine, 0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(call_f64(&mut engine, -0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(call_f64(&mut engine, std::f64::consts::FRAC_PI_2), 1.0);
    assert!(call_f64(&mut engine, f64::NAN).is_nan());
    assert!(
        engine
            .unicorn
            .get_data()
            .math_calls
            .iter()
            .any(|call| call.starts_with("sin("))
    );
}

#[test]
fn win64_crt_cos_uses_xmm0_f64_abi_preserves_upper_lane_and_bounds_trace() {
    const COS_IMPORT: u64 = STUB_BASE + 0x1e0;
    const UPPER_LANE: [u8; 8] = [0x5a; 8];

    fn call_f64(engine: &mut GuestEngine<'static>, input: f64) -> f64 {
        let mut xmm0 = [0u8; 16];
        xmm0[..8].copy_from_slice(&input.to_le_bytes());
        xmm0[8..].copy_from_slice(&UPPER_LANE);
        engine
            .unicorn
            .reg_write_long(RegisterX86::XMM0, &xmm0)
            .unwrap();
        engine.call_win64(COS_IMPORT, [0; 6]).unwrap();
        let xmm0 = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
        assert_eq!(&xmm0[8..], UPPER_LANE);
        f64::from_le_bytes(xmm0[..8].try_into().unwrap())
    }

    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(COS_IMPORT, &[0xc3]).unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            COS_IMPORT,
            "api-ms-win-crt-math-l1-1-0.dll",
            "cos",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::Cos)
    );
    assert_eq!(call_f64(&mut engine, 0.0), 1.0);
    assert_eq!(call_f64(&mut engine, -0.0), 1.0);
    assert_eq!(call_f64(&mut engine, std::f64::consts::PI), -1.0);
    assert!(call_f64(&mut engine, f64::NAN).is_nan());
    for _ in 0..40 {
        assert!(call_f64(&mut engine, 0.25).is_finite());
    }
    let math_calls = &engine.unicorn.get_data().math_calls;
    assert_eq!(math_calls.len(), 32);
    assert!(math_calls.iter().all(|call| call.starts_with("cos(")));
}

#[test]
fn opencl_bridge_symbols_and_unsupported_gpu_libraries_are_explicit() {
    for (symbol, expected) in [
        (
            "clCreateProgramWithSource",
            OpenClBridgeSymbol::CreateProgramWithSource,
        ),
        ("clBuildProgram", OpenClBridgeSymbol::BuildProgram),
        ("clCreateKernel", OpenClBridgeSymbol::CreateKernel),
        ("clSetKernelArg", OpenClBridgeSymbol::SetKernelArg),
        (
            "clEnqueueNDRangeKernel",
            OpenClBridgeSymbol::EnqueueNdRangeKernel,
        ),
        ("clReleaseKernel", OpenClBridgeSymbol::ReleaseKernel),
    ] {
        assert_eq!(
            dispatch_win64_import("OpenCL.DLL", symbol),
            Win64ImportDispatch::OpenClBridge(expected)
        );
    }
    assert_eq!(
        dispatch_win64_import("opencl.dll", "clCreateBuffer"),
        Win64ImportDispatch::UnsupportedGpuLibrary(GpuImportLibrary::OpenCl)
    );
    for (library, family) in [
        ("NVCUDA.DLL", GpuImportLibrary::Cuda),
        ("cudart64_12.dll", GpuImportLibrary::Cuda),
        (r"C:\CUDA\bin\CUBLAS64_12.DLL", GpuImportLibrary::Cuda),
        ("cublasLt64_12.dll", GpuImportLibrary::Cuda),
        ("cufft64_11.dll", GpuImportLibrary::Cuda),
        ("cudnn64_9.dll", GpuImportLibrary::Cuda),
        ("cudnn_ops_infer64_8.dll", GpuImportLibrary::Cuda),
        ("cusparse64_12.dll", GpuImportLibrary::Cuda),
        ("d3d11.dll", GpuImportLibrary::DirectX),
        ("D3DCOMPILER_47.DLL", GpuImportLibrary::DirectX),
        ("dxgi.dll", GpuImportLibrary::DirectX),
        ("DXCORE.DLL", GpuImportLibrary::DirectX),
    ] {
        assert_eq!(
            dispatch_win64_import(library, "same_symbol"),
            Win64ImportDispatch::UnsupportedGpuLibrary(family)
        );
    }
    for library in [
        "cublast64_12.dll",
        "cufftest64_11.dll",
        "cudnnot64_8.dll",
        "dxcore_helper.dll",
    ] {
        assert_eq!(
            dispatch_win64_import(library, "same_symbol"),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
}

#[test]
fn unknown_opencl_import_traps_instead_of_returning_zero_success() {
    const IMPORT: u64 = STUB_BASE + 0x1a0;
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&IMPORT.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0xc3]);
    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_write(IMPORT, &[0x31, 0xc0, 0xc3])
        .unwrap();
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            IMPORT,
            r"C:\Windows\System32\OPENCL.DLL",
            "clUnknownExtension"
        )
        .unwrap(),
        Win64ImportDispatch::UnsupportedGpuLibrary(GpuImportLibrary::OpenCl)
    );

    let error = engine.call_win64(TEST_CODE, [0; 6]).unwrap_err();
    assert!(matches!(
        error,
        GuestError::UnsupportedImport { library, symbol }
            if library == "opencl.dll" && symbol == "clUnknownExtension"
    ));
}

#[test]
fn cuda_and_directx_family_imports_trap_instead_of_returning_zero_success() {
    const IMPORT: u64 = STUB_BASE + 0x1d0;
    for (library, normalized, symbol, family) in [
        (
            r"C:\CUDA\bin\CUBLAS64_12.DLL",
            "cublas64_12.dll",
            "cublasCreate_v2",
            GpuImportLibrary::Cuda,
        ),
        (
            r"C:\Windows\System32\DXCORE.DLL",
            "dxcore.dll",
            "DXCoreCreateAdapterFactory",
            GpuImportLibrary::DirectX,
        ),
    ] {
        let mut code = vec![0x48, 0xb8];
        code.extend_from_slice(&IMPORT.to_le_bytes());
        code.extend_from_slice(&[0xff, 0xd0, 0xc3]);
        let mut engine = test_engine(&code);
        engine
            .unicorn
            .mem_write(IMPORT, &[0x31, 0xc0, 0xc3])
            .unwrap();
        assert_eq!(
            install_win64_import(&mut engine.unicorn, IMPORT, library, symbol,).unwrap(),
            Win64ImportDispatch::UnsupportedGpuLibrary(family)
        );

        let error = engine.call_win64(TEST_CODE, [0; 6]).unwrap_err();
        assert!(matches!(
            error,
            GuestError::UnsupportedImport {
                library,
                symbol: trapped_symbol
            } if library == normalized && trapped_symbol == symbol
        ));
    }
}

#[test]
fn win64_import_argument_reader_supports_twelve_total_arguments() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .add_code_hook(TEST_CODE, TEST_CODE, |unicorn, _, _| {
            for index in 0..MAX_WIN64_IMPORT_ARGUMENTS {
                match read_win64_import_argument(unicorn, index) {
                    Ok(value) if value == index as u64 + 1 => {}
                    Ok(value) => {
                        unicorn.get_data_mut().callback_error = Some(format!(
                            "argument {} was {value}, expected {}",
                            index + 1,
                            index + 1
                        ));
                        return;
                    }
                    Err(error) => {
                        unicorn.get_data_mut().callback_error = Some(error);
                        return;
                    }
                }
            }
            if read_win64_import_argument(unicorn, MAX_WIN64_IMPORT_ARGUMENTS).is_ok() {
                unicorn.get_data_mut().callback_error =
                    Some("argument reader accepted a thirteenth argument".into());
            }
        })
        .unwrap();
    let arguments = (1..=MAX_WIN64_IMPORT_ARGUMENTS as u64).collect::<Vec<_>>();
    engine
        .call_win64_with_timeout(TEST_CODE, &arguments, TIMEOUT_MICROSECONDS)
        .unwrap();
}

#[test]
fn import_trace_records_twelve_total_win64_arguments() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.get_data_mut().trace_labels.insert(
        TEST_CODE,
        TraceLabel {
            kind: TraceLabelKind::Import,
            name: canonical_import_trace_label("OPENCL.DLL", "clTraceFixture"),
        },
    );
    engine
        .begin_execution_trace("GPU_DEVICE_SETUP", TEST_CODE)
        .unwrap();
    let arguments = (1..=MAX_WIN64_IMPORT_ARGUMENTS as u64).collect::<Vec<_>>();
    let result = engine
        .call_win64_with_timeout(TEST_CODE, &arguments, TIMEOUT_MICROSECONDS)
        .unwrap();
    let trace = engine.finish_execution_trace(result).unwrap();
    let import = trace
        .events
        .iter()
        .find(|event| event.kind == "import_call")
        .unwrap();
    assert_eq!(import.name.as_deref(), Some("opencl.dll!clTraceFixture"));
    assert_eq!(import.arguments.len(), 4);
    assert_eq!(import.stack_arguments.len(), 8);
    assert_eq!(
        import
            .stack_arguments
            .iter()
            .map(|argument| argument.index)
            .collect::<Vec<_>>(),
        (5..=12).collect::<Vec<_>>()
    );
    assert_eq!(
        import.stack_arguments[7].value.raw,
        MAX_WIN64_IMPORT_ARGUMENTS as u64
    );
}

#[test]
fn selector_call_converts_unsupported_suite_cxx_throw_to_selector_abort() {
    const CODE: u64 = 0x1000_0000;
    let error_pointer = DATA_BASE + 0x300;
    let code = selector_throw_fixture(true, error_pointer, TEST_THROW_INFO, 0);
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);
    install_test_i32_throw_info(&mut engine);
    engine
        .write(DATA_BASE + 0x100, b"FLT Blur Suite\0")
        .unwrap();
    engine.write(error_pointer, &13i32.to_le_bytes()).unwrap();

    let error = engine.call_selector_win64(CODE, [0; 6]).unwrap_err();
    match error {
        GuestError::SelectorAbort {
            error,
            suite_name,
            suite_version,
            acquire_error,
        } => {
            assert_eq!(error, 13);
            assert_eq!(suite_name, "FLT Blur Suite");
            assert_eq!(suite_version, 1);
            assert_eq!(acquire_error, -1);
        }
        other => panic!("expected selector abort, got {other:?}"),
    }
    assert_eq!(engine.suite_requests(), ["FLT Blur Suite v1"]);
}

#[test]
fn failed_suite_keeps_a_different_typed_exception_fail_closed() {
    let error_pointer = DATA_BASE + 0x300;
    let code = selector_throw_fixture(true, error_pointer, TEST_THROW_INFO, 0);
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);
    install_test_i32_throw_info(&mut engine);
    engine
        .write(DATA_BASE + 0x100, b"Optional Missing Suite\0")
        .unwrap();
    engine.write(error_pointer, &13i32.to_le_bytes()).unwrap();
    // Replace the built-in int TypeDescriptor with a different MSVC type.
    engine
        .write(TEST_CODE + 0x850 + 16, b".?AVfailure@@\0")
        .unwrap();

    assert!(matches!(
        engine.call_selector_win64(TEST_CODE, [0; 6]),
        Err(GuestError::Callback(message))
            if message.contains("_CxxThrowException")
                && message.contains("msvc_type=.?AVfailure@@")
    ));
}

#[test]
fn cxx_throw_type_diagnostic_is_image_bounded_nul_terminated_and_sanitized() {
    let mut engine = test_engine(&vec![0u8; 0x1000]);
    install_test_i32_throw_info(&mut engine);
    assert_eq!(
        msvc_throw_type_name(&engine.unicorn, TEST_THROW_INFO).as_deref(),
        Some(".H")
    );
    assert_eq!(msvc_throw_type_name(&engine.unicorn, DATA_BASE), None);

    let type_name = TEST_CODE + 0x850 + 16;
    engine.write(type_name, &[b'A'; 128]).unwrap();
    assert_eq!(msvc_throw_type_name(&engine.unicorn, TEST_THROW_INFO), None);
    engine.write(type_name, b".?AVbad type@@\0").unwrap();
    assert_eq!(msvc_throw_type_name(&engine.unicorn, TEST_THROW_INFO), None);
}

fn write_msvc_x64_string(engine: &mut GuestEngine<'static>, object: u64, heap: u64, value: &[u8]) {
    let mut bytes = [0u8; 32];
    if value.len() <= 15 {
        bytes[..value.len()].copy_from_slice(value);
        bytes[value.len()] = 0;
        bytes[24..32].copy_from_slice(&15u64.to_le_bytes());
    } else {
        engine.write(heap, value).unwrap();
        engine.write(heap + value.len() as u64, &[0]).unwrap();
        bytes[..8].copy_from_slice(&heap.to_le_bytes());
        bytes[24..32].copy_from_slice(&(value.len() as u64).to_le_bytes());
    }
    bytes[16..24].copy_from_slice(&(value.len() as u64).to_le_bytes());
    engine.write(object, &bytes).unwrap();
}

#[test]
fn cv_exception_message_requires_exact_type_valid_msvc_string_and_diagnostic_shape() {
    let mut engine = test_engine(&[0xc3]);
    let exception = DATA_BASE + 0x400;
    let heap = DATA_BASE + 0x700;
    let message = b"OpenCV(4.5.5) source.cpp:7: error: (-215) assertion in function 'f'\n";
    write_msvc_x64_string(&mut engine, exception + 16, heap, message);

    assert_eq!(
        cv_exception_message(&engine.unicorn, exception, ".?AVException@cv@@").as_deref(),
        Some("OpenCV(4.5.5) source.cpp:7: error: (-215) assertion in function 'f'")
    );
    assert_eq!(
        cv_exception_message(&engine.unicorn, exception, ".?AVfailure@@"),
        None
    );

    write_msvc_x64_string(&mut engine, exception + 16, heap, b"short");
    assert_eq!(
        read_msvc_x64_string(&engine.unicorn, exception + 16).as_deref(),
        Some("short")
    );
    assert_eq!(
        cv_exception_message(&engine.unicorn, exception, ".?AVException@cv@@"),
        None
    );

    let malformed = exception + 16;
    engine.write(malformed + 16, &513u64.to_le_bytes()).unwrap();
    assert_eq!(read_msvc_x64_string(&engine.unicorn, malformed), None);
    engine.write(malformed + 16, &4u64.to_le_bytes()).unwrap();
    engine.write(malformed + 24, &3u64.to_le_bytes()).unwrap();
    assert_eq!(read_msvc_x64_string(&engine.unicorn, malformed), None);
    engine
        .write(malformed, &0xdead_beefu64.to_le_bytes())
        .unwrap();
    engine.write(malformed + 16, &4u64.to_le_bytes()).unwrap();
    engine.write(malformed + 24, &16u64.to_le_bytes()).unwrap();
    assert_eq!(read_msvc_x64_string(&engine.unicorn, malformed), None);

    write_msvc_x64_string(&mut engine, malformed, heap, b"bad\x01text");
    assert_eq!(read_msvc_x64_string(&engine.unicorn, malformed), None);
}

#[test]
fn failed_suite_provenance_expires_and_distant_i32_throw_fails_closed() {
    let error_pointer = DATA_BASE + 0x300;
    let code = selector_throw_fixture(true, error_pointer, TEST_THROW_INFO, 0x401);
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);
    install_test_i32_throw_info(&mut engine);
    engine
        .write(DATA_BASE + 0x100, b"Optional Missing Suite\0")
        .unwrap();
    engine.write(error_pointer, &13i32.to_le_bytes()).unwrap();

    assert!(matches!(
        engine.call_selector_win64(TEST_CODE, [0; 6]),
        Err(GuestError::Callback(message))
            if message.contains("_CxxThrowException")
    ));
}

#[test]
fn selector_abort_trace_can_be_discarded_before_cleanup_and_next_trace() {
    let error_pointer = DATA_BASE + 0x300;
    let code = selector_throw_fixture(true, error_pointer, TEST_THROW_INFO, 0);
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);
    install_test_i32_throw_info(&mut engine);
    engine
        .write(DATA_BASE + 0x100, b"FLT Blur Suite\0")
        .unwrap();
    engine.write(error_pointer, &13i32.to_le_bytes()).unwrap();

    engine
        .begin_execution_trace("SMART_RENDER", TEST_CODE)
        .unwrap();
    assert!(matches!(
        engine.call_selector_win64(TEST_CODE, [0; 6]),
        Err(GuestError::SelectorAbort { error: 13, .. })
    ));
    engine.discard_execution_trace().unwrap();
    assert!(engine.trace_hooks.is_empty());
    assert!(engine.unicorn.get_data().trace.is_none());

    let cleanup = TEST_CODE + 0x100;
    engine.write(cleanup, &[0xb8, 0, 0, 0, 0, 0xc3]).unwrap();
    engine.unicorn.get_data_mut().image_executable_ranges = vec![
        (TEST_CODE, TEST_CODE + code.len() as u64),
        (cleanup, cleanup + 6),
    ];
    engine
        .begin_execution_trace("FRAME_SETDOWN", cleanup)
        .unwrap();
    let cleanup_result = engine.call_selector_win64(cleanup, [0; 6]).unwrap();
    let trace = engine.finish_execution_trace(cleanup_result).unwrap();
    assert_eq!(cleanup_result, 0);
    assert_eq!(trace.selector, "FRAME_SETDOWN");
}

#[test]
fn unsupported_suite_without_throw_falls_back_but_does_not_mask_next_selector_throw() {
    const CODE: u64 = 0x1000_0000;
    let error_pointer = DATA_BASE + 0x300;
    let code = unsupported_acquire_fallback_fixture();
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);
    engine
        .write(DATA_BASE + 0x100, b"Optional Missing Suite\0")
        .unwrap();
    engine.write(error_pointer, &13i32.to_le_bytes()).unwrap();
    assert_eq!(engine.call_selector_win64(CODE, [0; 6]).unwrap(), 0);

    let code = selector_throw_fixture(false, error_pointer, TEST_THROW_INFO, 0);
    let second_selector = CODE + 0x100;
    engine.write(second_selector, &code).unwrap();
    engine.unicorn.get_data_mut().image_executable_ranges =
        vec![(second_selector, second_selector + code.len() as u64)];
    assert!(matches!(
        engine.call_selector_win64(second_selector, [0; 6]),
        Err(GuestError::Callback(message))
            if message.contains("_CxxThrowException")
    ));
}

#[test]
fn cxx_throw_without_pending_suite_or_readable_error_fails_closed() {
    const CODE: u64 = 0x1000_0000;
    let code = selector_throw_fixture(false, 0xdead_beef, TEST_THROW_INFO, 0);
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);

    assert!(matches!(
        engine.call_selector_win64(CODE, [0; 6]),
        Err(GuestError::Callback(message))
            if message.contains("_CxxThrowException")
    ));
    assert!(matches!(
        engine.call_win64(CODE, [0; 6]),
        Err(GuestError::Callback(message))
            if message.contains("_CxxThrowException")
    ));
}

#[test]
fn vcomp_fork_tail_calls_outlined_worker_with_captured_arguments() {
    const CODE: u64 = 0x1000_0000;
    const VCOMP_FORK: u64 = STUB_BASE + 0x100;
    let mut engine = test_engine(&[
        0x48, 0x89, 0xc8, // mov rax, rcx
        0x48, 0x01, 0xd0, // add rax, rdx
        0x4c, 0x01, 0xc0, // add rax, r8
        0xc3, // ret
    ]);
    engine
        .unicorn
        .mem_write(VCOMP_FORK, &[0x41, 0xff, 0xe3])
        .unwrap();
    engine
        .unicorn
        .add_code_hook(VCOMP_FORK, VCOMP_FORK, |unicorn, _, _| {
            emulate_vcomp_fork(unicorn);
        })
        .unwrap();

    assert_eq!(
        engine
            .call_win64(VCOMP_FORK, [1, 3, CODE, 11, 22, 33])
            .unwrap(),
        66
    );
}

#[test]
fn vcomp_set_num_threads_records_a_bounded_request_but_remains_serial() {
    const VCOMP_SET_THREADS: u64 = STUB_BASE + 0x180;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            VCOMP_SET_THREADS,
            "VCOMP140.DLL",
            "_vcomp_set_num_threads",
        )
        .unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::VcompSetNumThreads)
    );

    engine
        .call_win64(VCOMP_SET_THREADS, [8, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(engine.unicorn.get_data().vcomp_requested_threads, Some(8));
    assert_eq!(deterministic_import_i32("omp_get_max_threads"), Some(1));

    engine
        .call_win64(VCOMP_SET_THREADS, [2, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(engine.unicorn.get_data().vcomp_requested_threads, Some(2));
    assert!(
        test_engine(&[0xc3])
            .unicorn
            .get_data()
            .vcomp_requested_threads
            .is_none()
    );
}

#[test]
fn vcomp_set_num_threads_rejects_invalid_counts_without_losing_prior_state() {
    const VCOMP_SET_THREADS: u64 = STUB_BASE + 0x190;
    for invalid in [
        0u64,
        u32::MAX as u64,
        (MAX_VCOMP_REQUESTED_THREADS + 1) as u64,
    ] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            VCOMP_SET_THREADS,
            "vcomp140.dll",
            "_vcomp_set_num_threads",
        )
        .unwrap();
        engine
            .call_win64(VCOMP_SET_THREADS, [4, 0, 0, 0, 0, 0])
            .unwrap();

        let error = engine
            .call_win64(VCOMP_SET_THREADS, [invalid, 0, 0, 0, 0, 0])
            .unwrap_err();

        assert!(
            error.to_string().contains("requested thread count"),
            "{error}"
        );
        assert_eq!(engine.unicorn.get_data().vcomp_requested_threads, Some(4));
    }
}

#[test]
fn vcomp_set_num_threads_is_library_qualified_and_unknown_vcomp_stays_closed() {
    assert_eq!(
        dispatch_win64_import("fixture.dll", "_vcomp_set_num_threads"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
    assert_eq!(
        dispatch_win64_import("vcomp140.dll", "_vcomp_future_runtime_entry"),
        Win64ImportDispatch::UnsupportedVcomp
    );
}

#[test]
fn plugin_data_v2_and_v1_callbacks_decode_distinct_win64_stack_arguments() {
    const CALLBACK_V2: u64 = STUB_BASE + 0x180;
    const CALLBACK_V1: u64 = STUB_BASE + 0x190;
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(CALLBACK_V2, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(CALLBACK_V2, CALLBACK_V2, |unicorn, _, _| {
            capture_plugin_data_registration(unicorn, true);
        })
        .unwrap();
    engine.unicorn.mem_write(CALLBACK_V1, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(CALLBACK_V1, CALLBACK_V1, |unicorn, _, _| {
            capture_plugin_data_registration(unicorn, false);
        })
        .unwrap();
    let mut cursor = DATA_BASE;
    let mut write_text = |text: &[u8]| {
        let address = cursor;
        engine.unicorn.mem_write(address, text).unwrap();
        cursor += text.len() as u64;
        address
    };
    let name = write_text(b"Fixture\0");
    let match_name = write_text(b"fixture.match\0");
    let category = write_text(b"Tests\0");
    let entrypoint = write_text(b"FilterMain\0");
    let support_url = write_text(b"https://example.invalid\0");

    assert_eq!(
        engine
            .call_win64_with_timeout(
                CALLBACK_V2,
                &[
                    1,
                    name,
                    match_name,
                    category,
                    entrypoint,
                    crate::plugin_data::EFFECT_KIND as u32 as u64,
                    13,
                    29,
                    9,
                    support_url,
                ],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let registration = engine
        .unicorn
        .get_data()
        .plugin_data_registry
        .select(Some("fixture.match"))
        .unwrap();
    assert_eq!(registration.entrypoint, "FilterMain");
    assert_eq!(registration.reserved_info, 9);
    assert_eq!(
        registration.support_url.as_deref(),
        Some(b"https://example.invalid".as_slice())
    );

    engine.unicorn.get_data_mut().plugin_data_registry = EffectRegistry::default();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                CALLBACK_V1,
                &[
                    1,
                    name,
                    match_name,
                    category,
                    entrypoint,
                    crate::plugin_data::EFFECT_KIND as u32 as u64,
                    13,
                    29,
                    11,
                ],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let registration = engine
        .unicorn
        .get_data()
        .plugin_data_registry
        .select(Some("fixture.match"))
        .unwrap();
    assert_eq!(registration.reserved_info, 11);
    assert_eq!(registration.support_url, None);
}

#[test]
fn vcomp_dynamic_loop_returns_serial_chunks_until_exhausted() {
    const VCOMP_INIT: u64 = STUB_BASE + 0x110;
    const VCOMP_NEXT: u64 = STUB_BASE + 0x120;
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(VCOMP_INIT, &[0xc3]).unwrap();
    engine.unicorn.mem_write(VCOMP_NEXT, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(VCOMP_INIT, VCOMP_INIT, |unicorn, _, _| {
            emulate_vcomp_for_dynamic_init(unicorn);
        })
        .unwrap();
    engine
        .unicorn
        .add_code_hook(VCOMP_NEXT, VCOMP_NEXT, |unicorn, _, _| {
            emulate_vcomp_for_dynamic_next(unicorn);
        })
        .unwrap();
    let lower_output = DATA_BASE;
    let upper_output = DATA_BASE + 4;

    engine
        .call_win64(VCOMP_INIT, [0x62, 2, 10, 1, 8, 0])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(VCOMP_NEXT, [lower_output, upper_output, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(lower_output, 4).unwrap(),
        2i32.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(upper_output, 4).unwrap(),
        9i32.to_le_bytes()
    );
    assert_eq!(
        engine
            .call_win64(VCOMP_NEXT, [lower_output, upper_output, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(lower_output, 4).unwrap(),
        10i32.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(upper_output, 4).unwrap(),
        10i32.to_le_bytes()
    );
    assert_eq!(
        engine
            .call_win64(VCOMP_NEXT, [lower_output, upper_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
}

#[test]
fn vcomp_static_loop_writes_single_thread_bounds() {
    const VCOMP_STATIC_INIT: u64 = STUB_BASE + 0x130;
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_write(VCOMP_STATIC_INIT, &[0xc3])
        .unwrap();
    engine
        .unicorn
        .add_code_hook(VCOMP_STATIC_INIT, VCOMP_STATIC_INIT, |unicorn, _, _| {
            emulate_vcomp_for_static_simple_init(unicorn);
        })
        .unwrap();
    let lower_output = DATA_BASE;
    let upper_output = DATA_BASE + 4;

    engine
        .call_win64(VCOMP_STATIC_INIT, [2, 9, 1, 1, lower_output, upper_output])
        .unwrap();
    assert_eq!(
        engine.unicorn.mem_read_as_vec(lower_output, 4).unwrap(),
        2i32.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(upper_output, 4).unwrap(),
        9i32.to_le_bytes()
    );
}

#[test]
fn vcomp_rejects_unsupported_worker_count_and_dynamic_schedule() {
    const CODE: u64 = 0x1000_0000;
    const VCOMP_FORK: u64 = STUB_BASE + 0x140;
    let mut fork_engine = test_engine(&[0xc3]);
    fork_engine
        .unicorn
        .mem_write(VCOMP_FORK, &[0x41, 0xff, 0xe3])
        .unwrap();
    fork_engine
        .unicorn
        .add_code_hook(VCOMP_FORK, VCOMP_FORK, |unicorn, _, _| {
            emulate_vcomp_fork(unicorn);
        })
        .unwrap();
    assert!(
        fork_engine
            .call_win64(VCOMP_FORK, [2, 1, CODE, 7, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("worker count 2 is unsupported")
    );

    const VCOMP_INIT: u64 = STUB_BASE + 0x150;
    let mut init_engine = test_engine(&[0xc3]);
    init_engine.unicorn.mem_write(VCOMP_INIT, &[0xc3]).unwrap();
    init_engine
        .unicorn
        .add_code_hook(VCOMP_INIT, VCOMP_INIT, |unicorn, _, _| {
            emulate_vcomp_for_dynamic_init(unicorn);
        })
        .unwrap();
    assert!(
        init_engine
            .call_win64(VCOMP_INIT, [0x61, 0, 7, 1, 8, 0])
            .unwrap_err()
            .to_string()
            .contains("dynamic schedule 0x61 is unsupported")
    );
}

#[test]
fn openmp_thread_count_is_positive_and_deterministic() {
    const CODE: u64 = 0x1000_0000;
    assert_eq!(deterministic_import_i32("omp_get_max_threads"), Some(1));
    assert_eq!(deterministic_import_i32("unknown_import"), None);
    assert_eq!(deterministic_i32_stub(1), [0xb8, 1, 0, 0, 0, 0xc3]);
    let mut engine = test_engine(&deterministic_i32_stub(1));
    assert_eq!(engine.call_win64(CODE, [0; 6]).unwrap(), 1);
}

#[test]
fn cpp_object_returns_are_not_treated_as_scalar_zero_imports() {
    const GET_ENTRY: &str = "?GetEntry@DebugDatabase@debug@dvacore@@QEBA?AV?$basic_string@EU?$char_traits@E@std@@U?$STLAllocator@E@allocator@dvacore@@@std@@AEBV45@0@Z";
    assert!(msvc_udt_by_value_return_import(GET_ENTRY));
    assert!(msvc_udt_by_value_return_import(
        "?Create@ImmutableString@utility@dvacore@@SA?AV123@AEBV?$basic_string_view@D@std@@@Z"
    ));
    assert!(msvc_udt_by_value_return_import(
        "?FormatErrorMessage@dva_exception@config@dvacore@@MEBA?AV?$basic_string@EU?$char_traits@E@std@@@Z"
    ));
    assert!(msvc_udt_by_value_return_import(
        "?GetUnion@Thing@@QEBA?ATPayload@@XZ"
    ));
    assert!(msvc_udt_by_value_return_import(
        "?GetConstClass@Thing@@QEBA?BVPayload@@XZ"
    ));
    assert!(msvc_udt_by_value_return_import(
        "?GetVolatileStruct@Thing@@QEBA?CUPoint@@XZ"
    ));
    assert!(msvc_udt_by_value_return_import(
        "?GetConstVolatileUnion@Thing@@QEBA?DTPayload@@XZ"
    ));
    assert!(msvc_udt_by_value_return_import(
        "?GetQualifiedClass@Thing@@QEBA?ABVPayload@@XZ"
    ));
    assert!(!msvc_udt_by_value_return_import(
        "?Consume@Thing@@QEBAHVPayload@@@Z"
    ));
    assert!(!msvc_udt_by_value_return_import(
        "?Consume@@YAHUPayload@@@Z"
    ));
    assert!(!msvc_udt_by_value_return_import(
        "??0dva_exception@config@dvacore@@QEAA@PEBDH@Z"
    ));
    assert!(!msvc_udt_by_value_return_import(
        "?GetValue@Thing@@QEBAHAEBVOther@@@Z"
    ));
    assert!(!msvc_udt_by_value_return_import(
        "?GetValue@Thing@@QEBAHV?$vector@VPayload@@V?$allocator@VPayload@@@std@@@std@@XZ"
    ));
    assert!(!msvc_udt_by_value_return_import(
        "?GetInvalidTag@Thing@@QEBA?AEPayload@@XZ"
    ));
    assert!(!msvc_udt_by_value_return_import("GetLastError"));
}

#[test]
fn unsupported_cpp_object_return_import_stops_before_guest_uses_unwritten_sret() {
    const IMPORT: u64 = STUB_BASE + 0x170;
    const SYMBOL: &str = "?GetEntry@DebugDatabase@debug@dvacore@@QEBA?AVstring@std@@XZ";
    let mut code = vec![0x48, 0xb8];
    code.extend_from_slice(&IMPORT.to_le_bytes());
    code.extend_from_slice(&[
        0xff, 0xd0, // call rax
        0x48, 0x8b, 0x02, // mov rax, [rdx] (must never execute)
        0xc3,
    ]);
    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_write(IMPORT, &[0x31, 0xc0, 0xc3])
        .unwrap();
    install_unsupported_import_trap(
        &mut engine.unicorn,
        IMPORT,
        "dvacore.dll".into(),
        SYMBOL.into(),
    )
    .unwrap();

    let error = engine
        .call_win64(TEST_CODE, [0, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(matches!(
        &error,
        GuestError::UnsupportedImport { library, symbol }
            if library == "dvacore.dll" && symbol == SYMBOL
    ));
    assert_eq!(error.diagnostic_category(), "import");
}

#[test]
fn pf_handle_suite_maps_large_allocations_outside_guest_data() {
    let mut engine = test_engine(&[0xc3]);
    let size = 333_294_848;
    let handle = engine
        .call_win64(HOST_NEW_HANDLE, [size, 0, 0, 0, 0, 0])
        .unwrap();
    assert!(handle >= PF_HANDLE_DATA_BASE);
    let data = engine
        .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap();
    assert!(data > handle);
    assert_eq!(
        engine
            .call_win64(HOST_HANDLE_SIZE, [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        size
    );
    engine.write(data + size - 1, &[0x5a]).unwrap();
    let mut last = [0u8; 1];
    engine.read(data + size - 1, &mut last).unwrap();
    assert_eq!(last, [0x5a]);
    assert_eq!(
        engine
            .call_win64(HOST_UNLOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(HOST_DISPOSE_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let reused = engine
        .call_win64(HOST_NEW_HANDLE, [size, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(reused, handle, "disposed address space must be reusable");
}

#[test]
fn pf_handle_region_finder_skips_the_mapped_pe_image() {
    let state = GuestState {
        image_region: Some((
            PF_HANDLE_DATA_BASE + PAGE_SIZE,
            PF_HANDLE_DATA_BASE + 3 * PAGE_SIZE,
        )),
        ..GuestState::default()
    };
    assert_eq!(
        find_pf_region(&state, 2 * PAGE_SIZE, None).unwrap(),
        PF_HANDLE_DATA_BASE + 3 * PAGE_SIZE
    );
}

#[test]
fn pf_handle_budget_bounds_live_bytes_and_handle_count() {
    let observed_size = 333_294_848;
    let mut state = GuestState::default();
    for index in 0..6u64 {
        state.handles.insert(
            PF_HANDLE_DATA_BASE + index * PAGE_SIZE,
            GuestHandle {
                data: 0,
                size: observed_size,
                locks: 0,
                pending_dispose: false,
                handle_region: 0,
                data_region: 0,
                data_mapped_size: PAGE_SIZE,
            },
        );
    }
    assert!(validate_pf_handle_budget(&state, observed_size, None).is_err());
    let existing = state.handles.values().next().unwrap().clone();
    assert!(validate_pf_handle_budget(&state, observed_size, Some(&existing)).is_ok());

    state.handles.clear();
    for index in 0..1025u64 {
        state.handles.insert(
            PF_HANDLE_DATA_BASE + index * PAGE_SIZE,
            GuestHandle {
                data: 0,
                size: 0,
                locks: 0,
                pending_dispose: false,
                handle_region: 0,
                data_region: 0,
                data_mapped_size: PAGE_SIZE,
            },
        );
    }
    assert!(
        validate_pf_handle_budget(&state, 0, None).is_ok(),
        "real AEX workloads must be allowed to exceed the old 1024-handle cap"
    );
    for index in 1025..MAX_PF_HANDLE_COUNT as u64 {
        state.handles.insert(
            PF_HANDLE_DATA_BASE + index * PAGE_SIZE,
            GuestHandle {
                data: 0,
                size: 0,
                locks: 0,
                pending_dispose: false,
                handle_region: 0,
                data_region: 0,
                data_mapped_size: PAGE_SIZE,
            },
        );
    }
    assert!(validate_pf_handle_budget(&state, 0, None).is_err());
    let existing = state.handles.values().next().unwrap().clone();
    assert!(validate_pf_handle_budget(&state, 1, Some(&existing)).is_ok());
}

#[test]
fn pf_world_suite_v2_owns_formats_and_fails_closed() {
    let mut engine = test_engine(&[0xc3]);
    let suite_name = engine.allocate(15, 1).unwrap();
    engine.write(suite_name, b"PF World Suite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [suite_name, 2, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut suite = [0u8; 8];
    engine.read(suite_output, &mut suite).unwrap();
    assert_eq!(u64::from_le_bytes(suite), HOST_WORLD_SUITE);
    let world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_NEW_WORLD, [1, 3, 2, 1, 0xdead_beef, world])
            .unwrap(),
        4
    );
    assert!(engine.unicorn.get_data().worlds.is_empty());
    assert_eq!(
        engine
            .call_win64(HOST_NEW_WORLD, [1, 3, 2, 1, 0x3631_6561, world])
            .unwrap(),
        0
    );
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    engine.unicorn.mem_read(world, &mut definition).unwrap();
    let data = u64::from_le_bytes(
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        i32::from_le_bytes(
            definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                .try_into()
                .unwrap()
        ),
        24
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(data, 48).unwrap(),
        vec![0; 48]
    );
    assert_eq!(
        engine
            .call_win64(HOST_NEW_WORLD, [1, 3, 2, 1, 0x3631_6561, world])
            .unwrap(),
        4
    );
    assert_eq!(engine.unicorn.get_data().worlds.len(), 1);

    let format_output = engine.allocate(4, 4).unwrap();
    engine
        .unicorn
        .mem_write(format_output, &0xfeed_beefu32.to_le_bytes())
        .unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_GET_WORLD_PIXEL_FORMAT, [0, format_output, 0, 0, 0, 0])
            .unwrap(),
        4
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
        0xfeed_beefu32.to_le_bytes()
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_GET_WORLD_PIXEL_FORMAT,
                [world, format_output, 0, 0, 0, 0]
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
        0x3631_6561u32.to_le_bytes()
    );
    let smart_input = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let smart_output = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    engine.configure_smart_render(
        smart_input,
        smart_output,
        3,
        2,
        crate::pixel::PF_PIXEL_FORMAT_ARGB128,
        0,
        1,
    );
    for smart_world in [smart_input, smart_output] {
        assert_eq!(
            engine
                .call_win64(
                    HOST_GET_WORLD_PIXEL_FORMAT,
                    [smart_world, format_output, 0, 0, 0, 0]
                )
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
            crate::pixel::PF_PIXEL_FORMAT_ARGB128.to_le_bytes()
        );
    }
    assert_eq!(
        engine
            .call_win64(HOST_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(engine.unicorn.get_data().worlds.is_empty());
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(world, abi::PF_LAYER_DEF_SIZE)
            .unwrap(),
        vec![0; abi::PF_LAYER_DEF_SIZE]
    );
    assert_eq!(
        engine
            .call_win64(HOST_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
            .unwrap(),
        4
    );
    let classic_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut classic_definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    classic_definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&2i32.to_le_bytes());
    classic_definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    engine.write(classic_world, &classic_definition).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_GET_WORLD_PIXEL_FORMAT,
                [classic_world, format_output, 0, 0, 0, 0]
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(format_output, 4).unwrap(),
        0x6267_7261u32.to_le_bytes()
    );

    let float_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_NEW_WORLD, [1, 1, 1, 0x100, 0x3233_6561, float_world],)
            .unwrap(),
        0
    );
    engine
        .unicorn
        .mem_read(float_world, &mut definition)
        .unwrap();
    let float_data = u64::from_le_bytes(
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(float_data, 16).unwrap(),
        vec![0xcd; 16]
    );
    assert_eq!(
        engine
            .call_win64(HOST_DISPOSE_WORLD, [1, float_world, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_NEW_WORLD,
                [1, i32::MAX as u64, i32::MAX as u64, 1, 0x6267_7261, world,]
            )
            .unwrap(),
        4
    );
    assert!(engine.unicorn.get_data().worlds.is_empty());
    let error = engine
        .call_win64(HOST_NEW_WORLD8, [1, 3, 2, 0x2, world, 0])
        .unwrap_err();
    assert!(
        error.to_string().contains("unsupported DEEP_PIXELS"),
        "{error}"
    );
    assert!(engine.unicorn.get_data().worlds.is_empty());
}

#[test]
fn pf_handle_suite_fails_closed_on_unknown_lock() {
    let mut engine = test_engine(&[0xc3]);
    let error = engine
        .call_win64(HOST_LOCK_HANDLE, [0xdead_beef, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("unknown handle"));
}

#[test]
fn pf_handle_dispose_defers_locked_unmap_until_final_unlock() {
    let mut engine = test_engine(&[0xc3]);
    let handle = engine
        .call_win64(HOST_NEW_HANDLE, [4096, 0, 0, 0, 0, 0])
        .unwrap();
    let data = engine
        .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap();
    assert_ne!(data, 0);
    assert_eq!(engine.unicorn.get_data().handles[&handle].locks, 1);

    assert_eq!(
        engine
            .call_win64(HOST_DISPOSE_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let pending = &engine.unicorn.get_data().handles[&handle];
    assert_eq!(pending.locks, 1);
    assert!(pending.pending_dispose);
    engine.write(data, &[0xa5]).unwrap();

    let error = engine
        .call_win64(HOST_HANDLE_SIZE, [handle, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("unknown handle"));
    let error = engine
        .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("unknown handle"));
    assert_eq!(
        engine
            .call_win64(HOST_UNLOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(!engine.unicorn.get_data().handles.contains_key(&handle));
    assert!(engine.write(data, &[0x5a]).is_err());

    let error = engine
        .call_win64(HOST_DISPOSE_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("stale or foreign handle"));
}

#[test]
fn pf_handle_resize_preserves_bytes_and_stable_handle() {
    let mut engine = test_engine(&[0xc3]);
    let handle = engine
        .call_win64(HOST_NEW_HANDLE, [16, 0, 0, 0, 0, 0])
        .unwrap();
    let old_data = engine
        .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap();
    engine.write(old_data, &[1, 2, 3, 4]).unwrap();
    engine
        .call_win64(HOST_UNLOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap();
    let handle_pointer = engine.allocate(8, 8).unwrap();
    engine.write(handle_pointer, &handle.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_RESIZE_HANDLE, [8192, handle_pointer, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut stable = [0u8; 8];
    engine.read(handle_pointer, &mut stable).unwrap();
    assert_eq!(u64::from_le_bytes(stable), handle);
    let new_data = engine
        .call_win64(HOST_LOCK_HANDLE, [handle, 0, 0, 0, 0, 0])
        .unwrap();
    assert_ne!(new_data, old_data);
    let mut preserved = [0u8; 4];
    engine.read(new_data, &mut preserved).unwrap();
    assert_eq!(preserved, [1, 2, 3, 4]);
}

#[test]
fn color_param_suite_is_stateful_and_fails_closed() {
    let mut engine = test_engine(&[0xc3]);
    let name = engine.allocate(32, 1).unwrap();
    engine.write(name, b"PF ColorParamSuite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(suite_output, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), HOST_COLOR_PARAM_SUITE);

    let mut captured = vec![0u8; abi::PF_PARAM_DEF_SIZE];
    captured[..4].copy_from_slice(&101i32.to_le_bytes());
    captured[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
        .copy_from_slice(&5i32.to_le_bytes());
    captured[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + 8]
        .copy_from_slice(&[255, 10, 20, 30, 255, 1, 2, 3]);
    engine.unicorn.get_data_mut().params.push(GuestParam {
        index: 1,
        param_type: 5,
        name: "Key Color".into(),
        bytes: captured.clone(),
    });
    let definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    engine.write(definition, &captured).unwrap();
    engine
        .configure_parameter_definitions(definition, vec![definition])
        .unwrap();
    let definition_copy = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    engine.write(definition_copy, &captured).unwrap();
    let definition = definition_copy;
    let output = engine.allocate(abi::PF_PIXEL_FLOAT_SIZE, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
            .unwrap(),
        0
    );
    let mut values = [0u8; abi::PF_PIXEL_FLOAT_SIZE];
    engine.read(output, &mut values).unwrap();
    let channels = (0..4)
        .map(|index| f32::from_le_bytes(values[index * 4..index * 4 + 4].try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(channels, [1.0, 10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0]);

    engine
        .write(definition + abi::PARAM_U_OFFSET as u64, &[255, 1, 2, 3])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
            .unwrap(),
        0
    );
    engine
        .write(definition + abi::PARAM_U_OFFSET as u64, &[9, 9, 9, 9])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
            .unwrap(),
        516
    );
    engine.write(definition, &999i32.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
            .unwrap(),
        513
    );
    engine.write(definition, &101i32.to_le_bytes()).unwrap();
    engine
        .write(
            definition + abi::PARAM_PARAM_TYPE_OFFSET as u64,
            &6i32.to_le_bytes(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_COLOR_PARAM_VALUE, [1, definition, output, 0, 0, 0],)
            .unwrap(),
        514
    );
    assert_eq!(
        engine
            .call_win64(HOST_COLOR_PARAM_VALUE, [0, definition, output, 0, 0, 0])
            .unwrap(),
        516
    );
}

#[test]
fn smart_checkout_tracks_multiple_opaque_ids_and_selector_scope_cleanup() {
    let mut engine = test_engine(&[0xc3]);
    let input_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let output_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let input_pixels = engine.allocate(8 * 6 * 4, 8).unwrap();
    let mut input_definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    input_definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&input_pixels.to_le_bytes());
    input_definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&32i32.to_le_bytes());
    input_definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&8i32.to_le_bytes());
    input_definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&6i32.to_le_bytes());
    engine.write(input_world, &input_definition).unwrap();
    engine.configure_smart_render(
        input_world,
        output_world,
        8,
        6,
        crate::pixel::PF_PIXEL_FORMAT_ARGB32,
        0,
        1,
    );
    let request = engine.allocate(16, 4).unwrap();
    engine
        .write(
            request,
            &[0i32, 0, 8, 6]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let result = engine.allocate(76, 8).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_PRE_CHECKOUT_LAYER,
                &[1, 0, 9999, request, 1, 0, 1, result],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0,
        "primary input permits temporal requests against the current fallback world"
    );
    engine.finish_smart_checkout_scope();
    assert!(engine.unicorn.get_data().smart_checkout_ids.is_empty());
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_PRE_CHECKOUT_LAYER,
                &[1, 0, 9999, request, 0, 1, 1, result],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let second_result = engine.allocate(76, 8).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_PRE_CHECKOUT_LAYER,
                &[1, 0, 0, request, 0, 1, 1, second_result],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );

    let checked_out_world = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_CHECKOUT_LAYER_PIXELS,
                [1, 9999, checked_out_world, 0, 0, 0],
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(checked_out_world, 8)
            .unwrap(),
        input_world.to_le_bytes()
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_CHECKOUT_LAYER_PIXELS,
                [1, 9999, checked_out_world, 0, 0, 0],
            )
            .unwrap(),
        0,
        "a repeated checkout of the same registered token is idempotent"
    );
    let different_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_CHECKOUT_LAYER_PIXELS,
                [1, 9999, different_output, 0, 0, 0],
            )
            .unwrap(),
        0,
        "the token may be replayed into another mapped output slot"
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(different_output, 8).unwrap(),
        input_world.to_le_bytes()
    );
    let invalid_output_error = engine
        .call_win64(HOST_CHECKOUT_LAYER_PIXELS, [1, 9999, 0xdead_beef, 0, 0, 0])
        .unwrap_err();
    assert!(
        invalid_output_error
            .to_string()
            .contains("checkout-pixels world write")
    );
    assert_eq!(
        engine
            .call_win64(HOST_CHECKIN_LAYER_PIXELS, [1, 9999, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_CHECKOUT_LAYER_PIXELS,
                [1, 0, checked_out_world, 0, 0, 0],
            )
            .unwrap(),
        0
    );
    assert!(!engine.finish_smart_checkout_scope());
    assert!(engine.unicorn.get_data().smart_checkout_ids.is_empty());
    assert!(
        engine
            .call_win64(HOST_CHECKIN_LAYER_PIXELS, [1, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("invalid checkin-pixels id=0")
    );
    let empty_request = engine.allocate(16, 4).unwrap();
    let empty_result = engine.allocate(76, 8).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_PRE_CHECKOUT_LAYER,
                &[1, 0, 41, empty_request, 0, 0, 1, empty_result],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(empty_result, 16).unwrap(),
        [0; 16],
        "an empty request has an empty availability rectangle"
    );
    assert_eq!(
        engine
            .call_win64(
                HOST_CHECKOUT_LAYER_PIXELS,
                [1, 41, checked_out_world, 0, 0, 0],
            )
            .unwrap(),
        0,
        "an empty availability rectangle does not invalidate the host-owned world"
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(checked_out_world, 8)
            .unwrap(),
        input_world.to_le_bytes()
    );
    assert_eq!(
        engine
            .call_win64(HOST_CHECKIN_LAYER_PIXELS, [1, 41, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_PRE_CHECKOUT_LAYER,
                &[1, 0, 42, request, 0, 0, 1, result],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert!(
        engine.finish_smart_checkout_scope(),
        "a pre-checkout-only token is balanced"
    );
}

#[test]
fn smart_checkout_resolves_declared_secondary_layer_definition_and_owns_token() {
    let mut engine = test_engine(&[0xc3]);
    let input_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let output_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let input_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let layer_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let layer_pixels = engine.allocate(4 * 3 * 4, 8).unwrap();
    let mut definition = vec![0u8; abi::PF_PARAM_DEF_SIZE];
    let world = abi::PARAM_U_OFFSET;
    definition[world + abi::LAYER_DATA_OFFSET..world + abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&layer_pixels.to_le_bytes());
    definition[world + abi::LAYER_ROWBYTES_OFFSET..world + abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&16i32.to_le_bytes());
    definition[world + abi::LAYER_WIDTH_OFFSET..world + abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&4i32.to_le_bytes());
    definition[world + abi::LAYER_HEIGHT_OFFSET..world + abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&3i32.to_le_bytes());
    engine.write(layer_definition, &definition).unwrap();
    engine.unicorn.get_data_mut().params.push(GuestParam {
        index: 1,
        param_type: 0,
        name: "Map".into(),
        bytes: definition,
    });
    engine
        .configure_parameter_definitions(input_definition, vec![layer_definition])
        .unwrap();
    engine.configure_smart_render(
        input_world,
        output_world,
        8,
        6,
        crate::pixel::PF_PIXEL_FORMAT_ARGB32,
        0,
        1,
    );
    let request = engine.allocate(16, 4).unwrap();
    engine
        .write(
            request,
            &[-2i32, 1, 5, 9]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let result = engine.allocate(76, 8).unwrap();
    let temporal_error = engine
        .call_win64_with_timeout(
            HOST_PRE_CHECKOUT_LAYER,
            &[1, 1, 9999, request, 1, 0, 1, result],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(
        temporal_error
            .to_string()
            .contains("unsupported temporal smart checkout")
    );
    assert!(engine.unicorn.get_data().smart_checkout_ids.is_empty());
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_PRE_CHECKOUT_LAYER,
                &[1, 1, 10000, request, 0, 0, 1, result],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(result, 16).unwrap(),
        [0i32, 1, 4, 3]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>()
    );
    let checked_out = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_CHECKOUT_LAYER_PIXELS, [1, 10000, checked_out, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(checked_out, 8).unwrap(),
        (layer_definition + abi::PARAM_U_OFFSET as u64).to_le_bytes()
    );
    assert_eq!(
        engine
            .call_win64(HOST_CHECKIN_LAYER_PIXELS, [1, 10000, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(HOST_CHECKOUT_LAYER_PIXELS, [1, 10000, checked_out, 0, 0, 0],)
            .unwrap(),
        0
    );
    assert!(!engine.finish_smart_checkout_scope());
    assert!(engine.unicorn.get_data().smart_checkout_ids.is_empty());
}

#[test]
fn smart_checkout_inherits_input_for_an_unselected_declared_layer() {
    let mut engine = test_engine(&[0xc3]);
    let input_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let output_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let input_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let layer_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let input_pixels = engine.allocate(4 * 3 * 4, 8).unwrap();
    let mut world = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    world[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&input_pixels.to_le_bytes());
    world[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
        .copy_from_slice(&16i32.to_le_bytes());
    world[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&4i32.to_le_bytes());
    world[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
        .copy_from_slice(&3i32.to_le_bytes());
    engine.write(input_world, &world).unwrap();
    engine
        .write(layer_definition, &vec![0; abi::PF_PARAM_DEF_SIZE])
        .unwrap();
    engine.unicorn.get_data_mut().params.push(GuestParam {
        index: 1,
        param_type: 0,
        name: "Optional Map".into(),
        bytes: vec![0; abi::PF_PARAM_DEF_SIZE],
    });
    engine
        .configure_parameter_definitions(input_definition, vec![layer_definition])
        .unwrap();
    engine.configure_smart_render(
        input_world,
        output_world,
        4,
        3,
        crate::pixel::PF_PIXEL_FORMAT_ARGB32,
        0,
        1,
    );

    assert_eq!(
        smart_checkout_world(&mut engine.unicorn, 1).unwrap(),
        (input_world, 4, 3)
    );

    engine
        .write(
            layer_definition + abi::PARAM_U_OFFSET as u64 + abi::LAYER_WORLD_FLAGS_OFFSET as u64,
            &1i32.to_le_bytes(),
        )
        .unwrap();
    let error = smart_checkout_world(&mut engine.unicorn, 1).unwrap_err();
    assert!(error.contains("invalid smart checkout world"), "{error}");
}

fn disk_id_parameter(disk_id: i32, param_type: i32, name: &str) -> GuestParam {
    let mut bytes = vec![0; abi::PF_PARAM_DEF_SIZE];
    bytes[..4].copy_from_slice(&disk_id.to_le_bytes());
    GuestParam {
        index: -1,
        param_type,
        name: name.into(),
        bytes,
    }
}

#[test]
fn smart_checkout_resolves_layer_disk_id_without_aliasing_positional_point_storage() {
    let parameters = vec![
        disk_id_parameter(2, 6, "Center"),
        disk_id_parameter(30, 10, "Amount"),
        disk_id_parameter(1, 0, "Noise Layer"),
    ];

    assert_eq!(
        resolve_layer_parameter_offset(&parameters, 1).unwrap(),
        LayerParameterResolution {
            offset: 2,
            disk_id_fallback: true,
        }
    );
    assert_eq!(
        parameters[0].param_type, 6,
        "the positional point stays distinct"
    );

    let mut duplicate = parameters.clone();
    duplicate.push(disk_id_parameter(1, 0, "Duplicate Layer"));
    assert!(
        resolve_layer_parameter_offset(&duplicate, 1)
            .unwrap_err()
            .contains("duplicated")
    );
    assert!(
        resolve_layer_parameter_offset(&parameters, 2)
            .unwrap_err()
            .contains("not a PF_Param_LAYER")
    );
    assert!(
        resolve_layer_parameter_offset(&parameters, 99)
            .unwrap_err()
            .contains("does not resolve")
    );
}

#[test]
fn smart_checkout_positional_layer_wins_over_colliding_non_layer_disk_id() {
    // AE and the minihost `pre_checkout_layer` match `slot == index` only: a
    // popup whose disk id equals the checkout index must not make the
    // positional layer ambiguous.
    let parameters = vec![
        disk_id_parameter(3, 1, "Slider"),
        disk_id_parameter(1, 0, "Layer"),
        disk_id_parameter(2, 7, "Popup"),
    ];
    assert_eq!(
        resolve_layer_parameter_offset(&parameters, 2).unwrap(),
        LayerParameterResolution {
            offset: 1,
            disk_id_fallback: false,
        }
    );
    // A non-layer disk id never resolves a non-positional checkout either.
    assert!(
        resolve_layer_parameter_offset(&parameters, 3)
            .unwrap_err()
            .contains("not a PF_Param_LAYER")
    );
}

#[test]
fn smart_checkout_duplicated_disk_id_does_not_block_positional_layer() {
    let parameters = vec![
        disk_id_parameter(4, 0, "Layer A"),
        disk_id_parameter(4, 0, "Layer B"),
        disk_id_parameter(4, 1, "Slider"),
    ];
    for index in [1, 2] {
        assert_eq!(
            resolve_layer_parameter_offset(&parameters, index).unwrap(),
            LayerParameterResolution {
                offset: (index - 1) as usize,
                disk_id_fallback: false,
            },
            "index {index}"
        );
    }
    // Only when positional resolution fails does the duplicated id matter.
    assert!(
        resolve_layer_parameter_offset(&parameters, 4)
            .unwrap_err()
            .contains("duplicated")
    );
    assert!(
        resolve_layer_parameter_offset(&parameters, 3)
            .unwrap_err()
            .contains("not a PF_Param_LAYER")
    );
}

#[test]
fn smart_checkout_disk_id_fallback_is_recorded_per_index_and_slot() {
    let mut records = Vec::new();
    record_smart_checkout_disk_id_fallback(&mut records, 1, 3);
    record_smart_checkout_disk_id_fallback(&mut records, 1, 3);
    record_smart_checkout_disk_id_fallback(&mut records, 2, 4);
    assert_eq!(
        records,
        vec![
            SmartCheckoutDiskIdFallback {
                requested_index: 1,
                resolved_slot: 3,
                call_count: 2,
            },
            SmartCheckoutDiskIdFallback {
                requested_index: 2,
                resolved_slot: 4,
                call_count: 1,
            },
        ]
    );
}

#[test]
fn smart_checkout_rejects_non_layer_secondary_parameter() {
    let mut engine = test_engine(&[0xc3]);
    let input_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let output_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let input_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let scalar_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    engine.unicorn.get_data_mut().params.push(GuestParam {
        index: 1,
        param_type: 1,
        name: "Amount".into(),
        bytes: vec![0; abi::PF_PARAM_DEF_SIZE],
    });
    engine
        .configure_parameter_definitions(input_definition, vec![scalar_definition])
        .unwrap();
    engine.configure_smart_render(
        input_world,
        output_world,
        8,
        6,
        crate::pixel::PF_PIXEL_FORMAT_ARGB32,
        0,
        1,
    );
    let result = engine.allocate(76, 8).unwrap();
    let error = engine
        .call_win64_with_timeout(
            HOST_PRE_CHECKOUT_LAYER,
            &[1, 1, 10000, 0, 0, 1, 1, result],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(error.to_string().contains("not a PF_Param_LAYER"));
    assert!(engine.unicorn.get_data().smart_checkout_ids.is_empty());
}

#[test]
fn classic_checkout_param_maps_sdk_index_zero_to_the_input_layer_definition() {
    let mut engine = test_engine(&[0xc3]);
    let input_definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    let mut input = vec![0u8; abi::PF_PARAM_DEF_SIZE];
    input[abi::PARAM_U_OFFSET + abi::LAYER_DATA_OFFSET
        ..abi::PARAM_U_OFFSET + abi::LAYER_DATA_OFFSET + 8]
        .copy_from_slice(&0x1234_5678u64.to_le_bytes());
    input[abi::PARAM_U_OFFSET + abi::LAYER_WIDTH_OFFSET
        ..abi::PARAM_U_OFFSET + abi::LAYER_WIDTH_OFFSET + 4]
        .copy_from_slice(&1920i32.to_le_bytes());
    engine.write(input_definition, &input).unwrap();
    engine
        .configure_parameter_definitions(input_definition, Vec::new())
        .unwrap();

    let checkout = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_CHECKOUT_PARAM, [1, 0, 0, 1, 1, checkout])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(checkout, abi::PF_PARAM_DEF_SIZE)
            .unwrap(),
        input
    );

    let error = engine
        .call_win64(HOST_CHECKOUT_PARAM, [1, 1, 0, 1, 1, checkout])
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("checkout-param index is outside definitions: 1")
    );
}

#[test]
fn point_param_suite_returns_signed_fixed_values_as_doubles() {
    let mut engine = test_engine(&[0xc3]);
    let name = engine.allocate(32, 1).unwrap();
    engine.write(name, b"PF PointParamSuite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(suite_output, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), HOST_POINT_PARAM_SUITE);
    engine.read(HOST_POINT_PARAM_SUITE, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), HOST_POINT_PARAM_VALUE);

    let definition = engine.allocate(abi::PF_PARAM_DEF_SIZE, 8).unwrap();
    engine
        .write(
            definition + abi::PARAM_U_OFFSET as u64,
            &[98304i32.to_le_bytes(), (-147456i32).to_le_bytes()].concat(),
        )
        .unwrap();
    let output = engine.allocate(16, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_POINT_PARAM_VALUE, [1, definition, output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut values = [0u8; 16];
    engine.read(output, &mut values).unwrap();
    assert_eq!(f64::from_le_bytes(values[..8].try_into().unwrap()), 1.5);
    assert_eq!(f64::from_le_bytes(values[8..].try_into().unwrap()), -2.25);
    assert_eq!(
        engine
            .call_win64(HOST_POINT_PARAM_VALUE, [1, 0, output, 0, 0, 0])
            .unwrap(),
        4
    );
    assert_eq!(
        engine
            .call_win64(HOST_POINT_PARAM_VALUE, [1, definition, 0, 0, 0, 0])
            .unwrap(),
        4
    );
}

#[test]
fn iterate8_calls_guest_pixel_callback_for_each_argb8_pixel() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; mov edx,[r9]; mov [rax],edx; xor eax,eax; ret
    let mut engine = test_engine(&[
        0x48, 0x8b, 0x44, 0x24, 0x28, 0x41, 0x8b, 0x11, 0x89, 0x10, 0x31, 0xc0, 0xc3,
    ]);
    let source_pixels = engine.allocate(8, 4).unwrap();
    let destination_pixels = engine.allocate(8, 4).unwrap();
    let source_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    engine
        .write(source_pixels, &[1, 2, 3, 4, 5, 6, 7, 8])
        .unwrap();
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
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE8,
                &[0, 0, 1, source_world, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut output = [0u8; 8];
    engine.read(destination_pixels, &mut output).unwrap();
    assert_eq!(output, [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn iterate8_aliases_null_source_to_the_destination_pixel() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; xor edx,edx; test r9,r9; setne dl;
    // mov [rax],edx; xor eax,eax; ret
    let mut engine = test_engine(&[
        0x48, 0x8b, 0x44, 0x24, 0x28, 0x31, 0xd2, 0x4d, 0x85, 0xc9, 0x0f, 0x95, 0xc2, 0x89, 0x10,
        0x31, 0xc0, 0xc3,
    ]);
    let destination_pixels = engine.allocate(4, 4).unwrap();
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
                HOST_ITERATE8,
                &[0, 0, 1, 0, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut output = [0xffu8; 4];
    engine.read(destination_pixels, &mut output).unwrap();
    assert_eq!(output, [1, 0, 0, 0]);
}

#[test]
fn iterate16_calls_guest_pixel_callback_for_each_argb16_pixel() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; mov rdx,[r9]; mov [rax],rdx; xor eax,eax; ret
    let mut engine = test_engine(&[
        0x48, 0x8b, 0x44, 0x24, 0x28, 0x49, 0x8b, 0x11, 0x48, 0x89, 0x10, 0x31, 0xc0, 0xc3,
    ]);
    let source_pixels = engine.allocate(16, 8).unwrap();
    let destination_pixels = engine.allocate(16, 8).unwrap();
    let source_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let pixels = [
        0x7fff_u16, 0x1111, 0x2222, 0x3333, 0x1234, 0x4567, 0x5abc, 0x6def,
    ]
    .into_iter()
    .flat_map(u16::to_le_bytes)
    .collect::<Vec<_>>();
    engine.write(source_pixels, &pixels).unwrap();
    for (world, data) in [
        (source_world, source_pixels),
        (destination_world, destination_pixels),
    ] {
        let mut bytes = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        bytes[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&data.to_le_bytes());
        bytes[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&16i32.to_le_bytes());
        bytes[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&2i32.to_le_bytes());
        bytes[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        engine.write(world, &bytes).unwrap();
    }
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_ITERATE16,
                &[0, 0, 1, source_world, 0, 0, CODE, destination_world],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut output = [0u8; 16];
    engine.read(destination_pixels, &mut output).unwrap();
    assert_eq!(output.as_slice(), pixels);
}

#[test]
fn typed_iterate_suites_acquire_and_invoke_their_pixel_callbacks() {
    const CODE: u64 = 0x1000_0000;
    // mov rax,[rsp+0x28]; movdqu xmm0,[r9]; movdqu [rax],xmm0;
    // xor eax,eax; ret
    let callback = [
        0x48, 0x8b, 0x44, 0x24, 0x28, 0xf3, 0x41, 0x0f, 0x6f, 0x01, 0xf3, 0x0f, 0x7f, 0x00, 0x31,
        0xc0, 0xc3,
    ];
    for (name, expected_table, expected_callback, pixel_bytes) in [
        (
            "PF iterate16 Suite",
            HOST_ITERATE16_SUITE,
            HOST_ITERATE16,
            8usize,
        ),
        (
            "PF iterateFloat Suite",
            HOST_ITERATE_FLOAT_SUITE,
            HOST_ITERATE_FLOAT,
            16usize,
        ),
    ] {
        let mut engine = test_engine(&callback);
        let name_address = engine.allocate(name.len() + 1, 1).unwrap();
        engine
            .write(name_address, &[name.as_bytes(), &[0]].concat())
            .unwrap();
        let suite_output = engine.allocate(8, 8).unwrap();
        assert_eq!(
            engine
                .call_win64(HOST_ACQUIRE_SUITE, [name_address, 1, suite_output, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut pointer = [0u8; 8];
        engine.read(suite_output, &mut pointer).unwrap();
        assert_eq!(u64::from_le_bytes(pointer), expected_table);
        engine.read(expected_table, &mut pointer).unwrap();
        assert_eq!(u64::from_le_bytes(pointer), expected_callback);

        let source_pixels = engine.allocate(16, 16).unwrap();
        let destination_pixels = engine.allocate(16, 16).unwrap();
        let source_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        let destination_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
        let pixels = (1u8..=16).collect::<Vec<_>>();
        engine.write(source_pixels, &pixels).unwrap();
        for (world, data) in [
            (source_world, source_pixels),
            (destination_world, destination_pixels),
        ] {
            let mut bytes = vec![0u8; abi::PF_LAYER_DEF_SIZE];
            bytes[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
                .copy_from_slice(&data.to_le_bytes());
            bytes[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                .copy_from_slice(&(pixel_bytes as i32).to_le_bytes());
            bytes[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
                .copy_from_slice(&1i32.to_le_bytes());
            bytes[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
                .copy_from_slice(&1i32.to_le_bytes());
            engine.write(world, &bytes).unwrap();
        }
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    expected_callback,
                    &[0, 0, 1, source_world, 0, 0, CODE, destination_world],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap(),
            0
        );
        let mut output = [0u8; 16];
        engine.read(destination_pixels, &mut output).unwrap();
        assert_eq!(&output[..pixel_bytes], &pixels[..pixel_bytes]);

        for short_world in [source_world, destination_world] {
            engine
                .write(
                    short_world + abi::LAYER_ROWBYTES_OFFSET as u64,
                    &i32::try_from(pixel_bytes - 1).unwrap().to_le_bytes(),
                )
                .unwrap();
            let sentinel = [0xcc; 16];
            engine.write(destination_pixels, &sentinel).unwrap();
            assert_eq!(
                engine
                    .call_win64_with_timeout(
                        expected_callback,
                        &[0, 0, 1, source_world, 0, 0, CODE, destination_world],
                        TIMEOUT_MICROSECONDS,
                    )
                    .unwrap(),
                4
            );
            engine.read(destination_pixels, &mut output).unwrap();
            assert_eq!(output, sentinel, "short row must reject before callback");
            engine
                .write(
                    short_world + abi::LAYER_ROWBYTES_OFFSET as u64,
                    &i32::try_from(pixel_bytes).unwrap().to_le_bytes(),
                )
                .unwrap();
        }
    }
}

#[test]
fn vcruntime_strstr_searches_bytes_and_preserves_guest_pointer_identity() {
    const STRSTR: u64 = STUB_BASE + 0x1d0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(&mut engine.unicorn, STRSTR, "VCRUNTIME140.DLL", "strstr").unwrap(),
        Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::StrStr)
    );
    let source = DATA_BASE + 0x100;
    let needle = DATA_BASE + 0x200;
    for (haystack, query, expected) in [
        (&b"abababac\0"[..], &b"ababac\0"[..], Some(2)),
        (&b"aaa\0"[..], &b"aa\0"[..], Some(0)),
        (&b"ABC\0abc\0"[..], &b"abc\0"[..], None),
        (&b"a\xffb\0"[..], &b"\xffb\0"[..], Some(1)),
        (&b"\0"[..], &b"\0"[..], Some(0)),
        (&b"\0"[..], &b"a\0"[..], None),
    ] {
        engine.unicorn.mem_write(source, haystack).unwrap();
        engine.unicorn.mem_write(needle, query).unwrap();
        assert_eq!(
            engine
                .call_win64(STRSTR, [source, needle, 0, 0, 0, 0])
                .unwrap(),
            expected.map_or(0, |offset| source + offset)
        );
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "strstr"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn vcruntime_strstr_does_not_read_past_match_or_string_terminator() {
    const STRSTR: u64 = STUB_BASE + 0x1d0;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, STRSTR, "vcruntime140.dll", "strstr").unwrap();
    const PAGE: u64 = 0x30_0000_0000;
    engine
        .unicorn
        .mem_map(PAGE, 4096, Prot::READ | Prot::WRITE)
        .unwrap();
    let needle = DATA_BASE + 0x200;
    engine.unicorn.mem_write(needle, b"z\0").unwrap();
    engine.unicorn.mem_write(PAGE + 4095, b"z").unwrap();
    assert_eq!(
        engine
            .call_win64(STRSTR, [PAGE + 4095, needle, 0, 0, 0, 0])
            .unwrap(),
        PAGE + 4095
    );
    engine.unicorn.mem_write(PAGE + 4095, b"\0").unwrap();
    assert_eq!(
        engine
            .call_win64(STRSTR, [PAGE + 4095, needle, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.unicorn.mem_write(PAGE + 4095, b"x").unwrap();
    assert!(
        engine
            .call_win64(STRSTR, [PAGE + 4095, needle, 0, 0, 0, 0])
            .is_err()
    );
}

#[test]
fn vcruntime_strstr_rejects_null_and_unreadable_needle() {
    const STRSTR: u64 = STUB_BASE + 0x1d0;
    for (source, needle) in [(0, DATA_BASE), (DATA_BASE, 0), (DATA_BASE, 0xdead_beef)] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, STRSTR, "vcruntime140.dll", "strstr").unwrap();
        assert!(
            engine
                .call_win64(STRSTR, [source, needle, 0, 0, 0, 0])
                .is_err()
        );
    }
}

#[test]
fn vcruntime_strstr_rejects_unterminated_strings_at_the_host_bound() {
    const STRSTR: u64 = STUB_BASE + 0x1d0;
    const REGION: u64 = 0x30_0000_0000;
    for needle_unterminated in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, STRSTR, "vcruntime140.dll", "strstr").unwrap();
        engine
            .unicorn
            .mem_map(REGION, MAX_CRT_STRING_BYTES, Prot::READ | Prot::WRITE)
            .unwrap();
        engine
            .unicorn
            .mem_write(REGION, &vec![b'x'; MAX_CRT_STRING_BYTES as usize])
            .unwrap();
        let short = DATA_BASE + 0x200;
        engine.unicorn.mem_write(short, b"z\0").unwrap();
        let (source, needle) = if needle_unterminated {
            (short, REGION)
        } else {
            (REGION, short)
        };
        let error = engine
            .call_win64(STRSTR, [source, needle, 0, 0, 0, 0])
            .unwrap_err();
        assert!(error.to_string().contains("exceeds"), "{error}");
    }
}

#[test]
fn win64_strlen_counts_bytes_and_stops_at_page_edge_nul() {
    const STRLEN: u64 = STUB_BASE + 0x1e0;
    const PAGE: u64 = 0x30_0000_0000;
    for library in ["api-ms-win-crt-string-l1-1-0.dll", "UCRTBASE.DLL"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, STRLEN, library, "strlen").unwrap();
        engine
            .unicorn
            .mem_map(PAGE, 4096, Prot::READ | Prot::WRITE)
            .unwrap();
        engine.unicorn.mem_write(PAGE + 4092, b"a\xffb\0").unwrap();
        assert_eq!(
            engine
                .call_win64(STRLEN, [PAGE + 4092, 0, 0, 0, 0, 0])
                .unwrap(),
            3
        );
        assert_eq!(
            engine
                .call_win64(STRLEN, [PAGE + 4095, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        engine.unicorn.mem_write(PAGE + 4095, b"x").unwrap();
        assert!(
            engine
                .call_win64(STRLEN, [PAGE + 4095, 0, 0, 0, 0, 0])
                .is_err()
        );
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "strlen"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn win64_strlen_rejects_null_unreadable_and_unterminated_input() {
    const STRLEN: u64 = STUB_BASE + 0x1e0;
    const REGION: u64 = 0x30_0000_0000;
    for mode in 0..3 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, STRLEN, "ucrtbase.dll", "strlen").unwrap();
        engine
            .unicorn
            .mem_map(REGION, MAX_CRT_STRING_BYTES, Prot::READ | Prot::WRITE)
            .unwrap();
        engine
            .unicorn
            .mem_write(REGION, &vec![b'x'; MAX_CRT_STRING_BYTES as usize])
            .unwrap();
        if mode == 1 {
            engine
                .unicorn
                .mem_protect(REGION, MAX_CRT_STRING_BYTES, Prot::WRITE)
                .unwrap();
        }
        let address = if mode == 0 { 0 } else { REGION };
        let error = engine
            .call_win64(STRLEN, [address, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string();
        let expected = ["null", "not readable", "exceeds"][mode];
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn registry_open_empty_roots_and_missing_application_keys() {
    const OPEN: u64 = STUB_BASE + 0x1f0;
    const CLOSE: u64 = STUB_BASE + 0x200;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, OPEN, "ADVAPI32.DLL", "RegOpenKeyExA").unwrap();
    install_win64_import(&mut engine.unicorn, CLOSE, "advapi32.dll", "RegCloseKey").unwrap();
    let name = DATA_BASE + 0x300;
    let output = DATA_BASE + 0x400;
    for root in [
        0xffff_ffff_8000_0000,
        0xffff_ffff_8000_0001,
        0xffff_ffff_8000_0002,
        0xffff_ffff_8000_0003,
        0xffff_ffff_8000_0005,
    ] {
        engine.unicorn.mem_write(name, b"\0").unwrap();
        for subkey in [0, name] {
            assert_eq!(
                engine
                    .call_win64(OPEN, [root, subkey, 0, 0x20019, output, 0])
                    .unwrap(),
                0
            );
            let mut handle = [0; 8];
            engine.unicorn.mem_read(output, &mut handle).unwrap();
            assert_eq!(u64::from_le_bytes(handle), root);
        }
        engine
            .unicorn
            .mem_write(name, b"Software\\Example\\Missing\0")
            .unwrap();
        assert_eq!(
            engine
                .call_win64(OPEN, [root, name, 0, 0x20019, output, 0])
                .unwrap(),
            2
        );
        let mut handle = [0xff; 8];
        engine.unicorn.mem_read(output, &mut handle).unwrap();
        assert_eq!(handle, [0; 8]);
        assert_eq!(engine.call_win64(CLOSE, [root, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert_eq!(
            engine
                .call_win64(OPEN, [root, 0, 0, 0x20019, output, 0])
                .unwrap(),
            0
        );
    }
    engine.unicorn.get_data_mut().windows_last_error = 1234;
    for invalid in [0, 0x12345678, WINDOWS_KERNEL32_MODULE_TOKEN] {
        assert_eq!(
            engine.call_win64(CLOSE, [invalid, 0, 0, 0, 0, 0]).unwrap(),
            u64::from(ERROR_INVALID_HANDLE)
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 1234);
        assert!(engine.unicorn.get_data().callback_error.is_none());
    }
    for name in ["RegOpenKeyExA", "RegCloseKey"] {
        assert_eq!(
            dispatch_win64_import("other.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
}

#[test]
fn registry_open_rejects_unsupported_handles_options_and_bad_pointers() {
    const OPEN: u64 = STUB_BASE + 0x1f0;
    const ROOT: u64 = 0xffff_ffff_8000_0002;
    for args in [
        [123, 0, 0, 0x20019, DATA_BASE, 0],
        [ROOT, 0, 8, 0x20019, DATA_BASE, 0],
        [ROOT, 0xdead_beef, 0, 0x20019, DATA_BASE, 0],
        [ROOT, 0, 0, 0x20019, 0xdead_beef, 0],
    ] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, OPEN, "advapi32.dll", "RegOpenKeyExA").unwrap();
        assert!(engine.call_win64(OPEN, args).is_err());
    }
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, OPEN, "advapi32.dll", "RegOpenKeyExA").unwrap();
    assert_eq!(
        engine
            .call_win64(OPEN, [ROOT, 0, 0, 0x20019, 0, 0])
            .unwrap(),
        87
    );
    assert_eq!(
        engine
            .call_win64(OPEN, [ROOT, 0, 0, 0x20319, DATA_BASE, 0])
            .unwrap(),
        87
    );
}

#[test]
fn win64_strcpy_copies_nul_and_returns_destination_without_overwrite() {
    const COPY: u64 = STUB_BASE + 0x210;
    for library in ["api-ms-win-crt-string-l1-1-0.dll", "UCRTBASE.DLL"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, COPY, library, "strcpy").unwrap();
        let source = DATA_BASE + 0x300;
        let destination = DATA_BASE + 0x400;
        for value in [&b"abc\xff\0ignored"[..], &b"\0ignored"[..]] {
            engine.unicorn.mem_write(source, value).unwrap();
            engine.unicorn.mem_write(destination, &[0xaa; 16]).unwrap();
            assert_eq!(
                engine
                    .call_win64(COPY, [destination, source, 0, 0, 0, 0])
                    .unwrap(),
                destination
            );
            let mut actual = [0; 16];
            engine.unicorn.mem_read(destination, &mut actual).unwrap();
            let length = value.iter().position(|byte| *byte == 0).unwrap() + 1;
            assert_eq!(&actual[..length], &value[..length]);
            assert!(actual[length..].iter().all(|byte| *byte == 0xaa));
        }
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "strcpy"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn win64_strcpy_validates_complete_output_before_writing() {
    const COPY: u64 = STUB_BASE + 0x210;
    const PAGE: u64 = 0x30_0000_0000;
    for protect in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, COPY, "ucrtbase.dll", "strcpy").unwrap();
        engine
            .unicorn
            .mem_map(PAGE, 4096, Prot::READ | Prot::WRITE)
            .unwrap();
        let source = DATA_BASE + 0x300;
        engine.unicorn.mem_write(source, b"abc\0").unwrap();
        engine.unicorn.mem_write(PAGE + 4090, &[0xaa; 6]).unwrap();
        let destination = if protect { PAGE + 4090 } else { PAGE + 4094 };
        if protect {
            engine.unicorn.mem_protect(PAGE, 4096, Prot::READ).unwrap();
        }
        assert!(
            engine
                .call_win64(COPY, [destination, source, 0, 0, 0, 0])
                .is_err()
        );
        let mut actual = [0; 6];
        engine.unicorn.mem_read(PAGE + 4090, &mut actual).unwrap();
        assert_eq!(actual, [0xaa; 6]);
    }
}

#[test]
fn system_time_api_set_executes_and_rejects_read_only_output() {
    const ENTRY: u64 = STUB_BASE + 0x100;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        ENTRY,
        "api-ms-win-core-sysinfo-l1-1-0.dll",
        "GetSystemTimeAsFileTime",
    )
    .unwrap();
    let output = engine.allocate(16, 8).unwrap();
    engine.write(output, &[0x5a; 16]).unwrap();
    engine.call_win64(ENTRY, [output, 0, 0, 0, 0, 0]).unwrap();
    let mut bytes = [0; 16];
    engine.read(output, &mut bytes).unwrap();
    let filetime = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    let now = windows_filetime(std::time::SystemTime::now()).unwrap();
    assert!(filetime <= now && now - filetime < 100_000_000);
    assert_eq!(&bytes[8..], &[0x5a; 8]);
    engine
        .unicorn
        .mem_protect(0x10000000, PAGE_SIZE, Prot::READ | Prot::EXEC)
        .unwrap();
    let mut before = [0; 8];
    engine.read(0x10000000, &mut before).unwrap();
    assert!(
        engine
            .call_win64(ENTRY, [0x10000000, 0, 0, 0, 0, 0])
            .is_err()
    );
    let mut after = [0; 8];
    engine.read(0x10000000, &mut after).unwrap();
    assert_eq!(before, after);
}

#[test]
fn cuda_startup_identity_and_counter_api_sets_execute_existing_guest_semantics() {
    let mut engine = test_engine(&[0xc3]);
    let thread = STUB_BASE + 0x100;
    let process = STUB_BASE + 0x110;
    let counter = STUB_BASE + 0x120;
    for (entry, dll, symbol) in [
        (
            thread,
            "api-ms-win-core-processthreads-l1-1-0.dll",
            "GetCurrentThreadId",
        ),
        (
            process,
            "api-ms-win-core-processthreads-l1-1-0.dll",
            "GetCurrentProcessId",
        ),
        (
            counter,
            "api-ms-win-core-profile-l1-1-0.dll",
            "QueryPerformanceCounter",
        ),
    ] {
        install_win64_import(&mut engine.unicorn, entry, dll, symbol).unwrap();
    }
    engine.unicorn.get_data_mut().current_windows_thread_id = 73;
    assert_eq!(engine.call_win64(thread, [0; 6]).unwrap(), 73);
    engine.unicorn.get_data_mut().current_windows_thread_id = 81;
    assert_eq!(engine.call_win64(thread, [0; 6]).unwrap(), 81);
    assert_eq!(engine.call_win64(process, [0; 6]).unwrap(), 1);
    let output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine.call_win64(counter, [output, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    let mut bytes = [0; 8];
    engine.read(output, &mut bytes).unwrap();
    assert!(
        u64::from_le_bytes(bytes)
            <= engine
                .unicorn
                .get_data()
                .performance_counter_origin
                .unwrap()
                .elapsed()
                .as_nanos() as u64
                / 100
    );
    assert!(matches!(
        dispatch_win64_import("fixture.dll", "GetCurrentThreadId"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn cuda_api_sets_share_error_tls_and_heap_state_with_kernel32() {
    let mut engine = test_engine(&[0xc3]);
    let apis = [
        ("api-ms-win-core-errorhandling-l1-1-0.dll", "SetLastError"),
        ("kernel32.dll", "GetLastError"),
        ("api-ms-win-core-errorhandling-l1-1-0.dll", "GetLastError"),
        ("api-ms-win-core-processthreads-l1-1-0.dll", "TlsAlloc"),
        ("api-ms-win-core-processthreads-l1-1-0.dll", "TlsSetValue"),
        ("kernel32.dll", "TlsGetValue"),
        ("api-ms-win-core-processthreads-l1-1-0.dll", "TlsFree"),
        ("api-ms-win-core-heap-l1-1-0.dll", "HeapCreate"),
        ("api-ms-win-core-heap-l1-1-0.dll", "HeapAlloc"),
        ("api-ms-win-core-heap-l1-1-0.dll", "HeapReAlloc"),
        ("kernel32.dll", "HeapFree"),
        ("api-ms-win-core-heap-l1-1-0.dll", "HeapDestroy"),
    ];
    for (index, (dll, symbol)) in apis.iter().enumerate() {
        install_win64_import(
            &mut engine.unicorn,
            STUB_BASE + 0x100 + index as u64 * 16,
            dll,
            symbol,
        )
        .unwrap();
    }
    let mut call = |index: u64, args| {
        engine
            .call_win64(STUB_BASE + 0x100 + index * 16, args)
            .unwrap()
    };
    call(0, [1234, 0, 0, 0, 0, 0]);
    assert_eq!(call(1, [0; 6]), 1234);
    assert_eq!(call(2, [0; 6]), 1234);
    let slot = call(3, [0; 6]);
    assert_eq!(call(4, [slot, 0x12345678, 0, 0, 0, 0]), 1);
    assert_eq!(call(5, [slot, 0, 0, 0, 0, 0]), 0x12345678);
    assert_eq!(call(6, [slot, 0, 0, 0, 0, 0]), 1);
    let heap = call(7, [0; 6]);
    assert_ne!(heap, 0);
    let block = call(8, [heap, 8, 16, 0, 0, 0]);
    assert_ne!(block, 0);
    let resized = call(9, [heap, 8, block, 32, 0, 0]);
    assert_ne!(resized, 0);
    assert_eq!(call(10, [heap, 0, resized, 0, 0, 0]), 1);
    assert_eq!(call(11, [heap, 0, 0, 0, 0, 0]), 1);
    assert!(engine.unicorn.get_data().windows_private_heaps.is_empty());
}

#[test]
fn initialize_srw_lock_sets_storage_and_preserves_active_ownership() {
    for dll in ["kernel32.dll", "api-ms-win-core-synch-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let init = STUB_BASE + 0x100;
        let acquire = init + 16;
        let release = init + 32;
        for (entry, symbol) in [
            (init, "InitializeSRWLock"),
            (acquire, "AcquireSRWLockExclusive"),
            (release, "ReleaseSRWLockExclusive"),
        ] {
            install_win64_import(&mut engine.unicorn, entry, dll, symbol).unwrap();
        }
        let address = engine.allocate(16, 8).unwrap();
        engine.write(address, &[0x5a; 16]).unwrap();
        engine.call_win64(init, [address, 0, 0, 0, 0, 0]).unwrap();
        let mut bytes = [0; 16];
        engine.read(address, &mut bytes).unwrap();
        assert_eq!(&bytes[..8], &[0; 8]);
        assert_eq!(&bytes[8..], &[0x5a; 8]);
        engine
            .call_win64(acquire, [address, 0, 0, 0, 0, 0])
            .unwrap();
        let state = engine.unicorn.get_data().windows_srw_locks[&address].clone();
        assert!(engine.call_win64(init, [address, 0, 0, 0, 0, 0]).is_err());
        assert_eq!(engine.unicorn.get_data().windows_srw_locks[&address], state);
        engine
            .call_win64(release, [address, 0, 0, 0, 0, 0])
            .unwrap();
        engine.call_win64(init, [address, 0, 0, 0, 0, 0]).unwrap();
        assert!(
            !engine
                .unicorn
                .get_data()
                .windows_srw_locks
                .contains_key(&address)
        );
        engine
            .unicorn
            .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
            .unwrap();
        assert!(engine.call_win64(init, [DATA_BASE, 0, 0, 0, 0, 0]).is_err());
    }
}

#[test]
fn security_descriptor_initialization_writes_absolute_layout_and_validates_output() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "advapi32.dll",
        "InitializeSecurityDescriptor",
    )
    .unwrap();
    let output = engine.allocate(48, 8).unwrap();
    engine.write(output, &[0x5a; 48]).unwrap();
    assert_eq!(
        engine.call_win64(entry, [output, 1, 0, 0, 0, 0]).unwrap(),
        1
    );
    let mut actual = [0; 48];
    engine.read(output, &mut actual).unwrap();
    assert_eq!(actual[0], 1);
    assert_eq!(&actual[1..40], &[0; 39]);
    assert_eq!(&actual[40..], &[0x5a; 8]);
    assert_eq!(
        engine.call_win64(entry, [output, 2, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1305);
    let mut after = [0; 48];
    engine.read(output, &mut after).unwrap();
    assert_eq!(after, actual);
    engine
        .unicorn
        .mem_map(0x50000000, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.unicorn.mem_write(0x50000ff0, &[0x5a; 16]).unwrap();
    assert_eq!(
        engine
            .call_win64(entry, [0x50000ff0, 1, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(0x50000ff0, 16).unwrap(),
        [0x5a; 16]
    );
    assert_eq!(engine.call_win64(entry, [0, 1, 0, 0, 0, 0]).unwrap(), 0);
}

#[test]
fn descriptor_dacl_setter_preserves_references_and_unrelated_fields() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "advapi32.dll",
        "SetSecurityDescriptorDacl",
    )
    .unwrap();
    let output = engine.allocate(48, 8).unwrap();
    let acl = engine.allocate(8, 8).unwrap();
    engine.write(acl, &[2, 0, 8, 0, 0, 0, 0, 0]).unwrap();
    let mut bytes = [0x5a; 48];
    bytes[0] = 1;
    bytes[2..4].copy_from_slice(&0x1010u16.to_le_bytes());
    engine.write(output, &bytes).unwrap();
    for (present, pointer, defaulted, control, stored) in [
        (1, acl, 1, 0x101c, acl),
        (0, u64::MAX, 0, 0x1018, acl),
        (1, 0, 0, 0x1014, 0),
    ] {
        assert_eq!(
            engine
                .call_win64(entry, [output, present, pointer, defaulted, 0, 0])
                .unwrap(),
            1
        );
        bytes[2..4].copy_from_slice(&(control as u16).to_le_bytes());
        bytes[32..40].copy_from_slice(&stored.to_le_bytes());
        let mut actual = [0; 48];
        engine.read(output, &mut actual).unwrap();
        assert_eq!(actual, bytes);
    }
    bytes[2..4].copy_from_slice(&0x8000u16.to_le_bytes());
    engine.write(output, &bytes).unwrap();
    assert_eq!(
        engine.call_win64(entry, [output, 1, acl, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1338);
    let mut actual = [0; 48];
    engine.read(output, &mut actual).unwrap();
    assert_eq!(actual, bytes);
    assert_eq!(engine.call_win64(entry, [0, 1, acl, 0, 0, 0]).unwrap(), 0);
}

#[test]
fn msvcp_lockit_tracks_recursive_ownership_and_balances_destructors() {
    let mut engine = test_engine(&[0xc3]);
    let ctor = STUB_BASE + 0x100;
    let dtor = ctor + 16;
    install_win64_import(
        &mut engine.unicorn,
        ctor,
        "msvcp140.dll",
        "??0_Lockit@std@@QEAA@H@Z",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        dtor,
        "msvcp140.dll",
        "??1_Lockit@std@@QEAA@XZ",
    )
    .unwrap();
    let a = engine.allocate(8, 8).unwrap();
    let b = engine.allocate(8, 8).unwrap();
    let c = engine.allocate(8, 8).unwrap();
    engine.write(a, &[0x5a; 8]).unwrap();
    assert_eq!(engine.call_win64(ctor, [a, 2, 0, 0, 0, 0]).unwrap(), a);
    engine.call_win64(ctor, [b, 2, 0, 0, 0, 0]).unwrap();
    assert_eq!(
        engine.unicorn.get_data().msvcp_lockit_locks[2],
        Some((1, 2))
    );
    let mut actual = [0; 8];
    engine.read(a, &mut actual).unwrap();
    assert_eq!(actual, [2, 0, 0, 0, 0x5a, 0x5a, 0x5a, 0x5a]);
    engine.unicorn.get_data_mut().current_windows_thread_id = 2;
    assert!(engine.call_win64(ctor, [c, 2, 0, 0, 0, 0]).is_err());
    assert!(engine.call_win64(dtor, [a, 0, 0, 0, 0, 0]).is_err());
    assert_eq!(
        engine.unicorn.get_data().msvcp_lockit_locks[2],
        Some((1, 2))
    );
    engine.unicorn.get_data_mut().current_windows_thread_id = 1;
    engine.call_win64(dtor, [b, 0, 0, 0, 0, 0]).unwrap();
    engine.call_win64(dtor, [a, 0, 0, 0, 0, 0]).unwrap();
    assert!(engine.unicorn.get_data().msvcp_lockit_objects.is_empty());
    assert!(
        engine
            .unicorn
            .get_data()
            .msvcp_lockit_locks
            .iter()
            .all(Option::is_none)
    );
    assert!(engine.call_win64(dtor, [a, 0, 0, 0, 0, 0]).is_err());
    engine.call_win64(ctor, [a, 8, 0, 0, 0, 0]).unwrap();
    engine.call_win64(dtor, [a, 0, 0, 0, 0, 0]).unwrap();
    assert!(
        engine
            .call_win64(ctor, [a, u32::MAX as u64, 0, 0, 0, 0])
            .is_err()
    );
}

#[test]
fn putenv_updates_all_guest_readers_and_preserves_snapshots() {
    let mut engine = test_engine(&[0xc3]);
    let entries = [
        ("api-ms-win-crt-environment-l1-1-0.dll", "_putenv"),
        ("api-ms-win-crt-environment-l1-1-0.dll", "getenv"),
        ("kernel32.dll", "GetEnvironmentVariableA"),
        ("kernel32.dll", "GetEnvironmentVariableW"),
        ("kernel32.dll", "GetEnvironmentStringsW"),
        ("kernel32.dll", "FreeEnvironmentStringsW"),
    ];
    for (i, (dll, symbol)) in entries.iter().enumerate() {
        install_win64_import(
            &mut engine.unicorn,
            STUB_BASE + 0x100 + i as u64 * 16,
            dll,
            symbol,
        )
        .unwrap();
    }
    let text = engine.allocate(256, 8).unwrap();
    let out = engine.allocate(256, 8).unwrap();
    engine.write(text, b"TEST_AEX=alpha=beta\0").unwrap();
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x100, [text, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.write(text, b"test_aex\0").unwrap();
    let value = engine
        .call_win64(STUB_BASE + 0x110, [text, 0, 0, 0, 0, 0])
        .unwrap();
    assert_eq!(
        engine.unicorn.mem_read_as_vec(value, 11).unwrap(),
        b"alpha=beta\0"
    );
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x120, [text, out, 256, 0, 0, 0])
            .unwrap(),
        10
    );
    engine
        .unicorn
        .mem_map(0x50000000, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.unicorn.mem_write(0x50000ffc, &[0x5a; 4]).unwrap();
    assert!(
        engine
            .call_win64(STUB_BASE + 0x120, [text, 0x50000ffc, 256, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(0x50000ffc, 4).unwrap(),
        [0x5a; 4]
    );
    engine
        .unicorn
        .mem_protect(0x50000000, PAGE_SIZE, Prot::READ | Prot::EXEC)
        .unwrap();
    assert!(
        engine
            .call_win64(STUB_BASE + 0x120, [text, 0x50000000, 256, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(0x50000000, 11).unwrap(),
        [0; 11]
    );
    let wide: Vec<u8> = "TeSt_AeX\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    engine.write(text, &wide).unwrap();
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x130, [text, out, 128, 0, 0, 0])
            .unwrap(),
        10
    );
    let snapshot = engine.call_win64(STUB_BASE + 0x140, [0; 6]).unwrap();
    let before = guest_environment_block_w(engine.unicorn.get_data());
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(snapshot, before.len())
            .unwrap(),
        before
    );
    engine.write(text, b"TEST_AEX=\0").unwrap();
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x100, [text, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.write(text, b"test_aex\0").unwrap();
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x110, [text, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(snapshot, before.len())
            .unwrap(),
        before
    );
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x150, [snapshot, 0, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    engine.write(text, b"OPENCV_FOR_THREADS_NUM=\0").unwrap();
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x100, [text, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(guest_environment_block_w(engine.unicorn.get_data()), [0; 4]);
    engine.write(text, b"INVALID\0").unwrap();
    assert_eq!(
        engine
            .call_win64(STUB_BASE + 0x100, [text, 0, 0, 0, 0, 0])
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 22);
    assert_eq!(guest_environment_block_w(engine.unicorn.get_data()), [0; 4]);
}

#[test]
fn strcmp_uses_unsigned_case_sensitive_bytes_and_stops_at_decisive_byte() {
    for dll in ["api-ms-win-crt-string-l1-1-0.dll", "ucrtbase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "strcmp").unwrap();
        let left = DATA_BASE + 0x100;
        let right = DATA_BASE + 0x200;
        for (a, b, expected) in [
            (&b"abc\0"[..], &b"abc\0"[..], 0),
            (b"A\0", b"a\0", -1),
            (b"\xff\0", b"\x7f\0", 1),
            (b"\0", b"a\0", -1),
            (b"abc\0", b"ab\0", 1),
        ] {
            engine.write(left, a).unwrap();
            engine.write(right, b).unwrap();
            let result = engine.call_win64(entry, [left, right, 0, 0, 0, 0]).unwrap() as u32 as i32;
            assert_eq!(result.signum(), expected);
        }
        let edge = DATA_BASE + PAGE_SIZE - 1;
        engine.write(edge, b"a").unwrap();
        engine.write(right, b"b\0").unwrap();
        assert_eq!(
            engine.call_win64(entry, [edge, right, 0, 0, 0, 0]).unwrap() as u32 as i32,
            -1
        );
        engine.write(right, b"a\0").unwrap();
        assert!(engine.call_win64(entry, [edge, right, 0, 0, 0, 0]).is_err());
        assert!(engine.call_win64(entry, [0, right, 0, 0, 0, 0]).is_err());
        engine
            .unicorn
            .mem_map(0x50000000, PAGE_SIZE, Prot::WRITE)
            .unwrap();
        assert!(
            engine
                .call_win64(entry, [0x50000000, right, 0, 0, 0, 0])
                .is_err()
        );
    }
}

#[test]
fn guest_clocks_convert_epochs_and_counter_tracks_elapsed_time() {
    use std::time::{Duration, Instant, UNIX_EPOCH};
    assert_eq!(
        windows_filetime(UNIX_EPOCH).unwrap(),
        116_444_736_000_000_000
    );
    assert_eq!(
        windows_filetime(UNIX_EPOCH + Duration::from_nanos(199)).unwrap(),
        116_444_736_000_000_001
    );
    assert_eq!(
        windows_filetime(UNIX_EPOCH - Duration::from_nanos(1)).unwrap(),
        116_444_736_000_000_000 - 1
    );
    assert_eq!(
        windows_filetime(UNIX_EPOCH - Duration::from_secs(11_644_473_600)).unwrap(),
        0
    );
    assert!(windows_filetime(UNIX_EPOCH - Duration::from_secs(11_644_473_601)).is_err());
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "kernel32.dll",
        "QueryPerformanceCounter",
    )
    .unwrap();
    let output = engine.allocate(8, 8).unwrap();
    let origin = Instant::now() - Duration::from_secs(2);
    engine.unicorn.get_data_mut().performance_counter_origin = Some(origin);
    let before = origin.elapsed().as_nanos() as u64 / 100;
    engine.call_win64(entry, [output, 0, 0, 0, 0, 0]).unwrap();
    let after = origin.elapsed().as_nanos() as u64 / 100;
    let mut bytes = [0; 8];
    engine.read(output, &mut bytes).unwrap();
    let first = u64::from_le_bytes(bytes);
    assert!((before..=after).contains(&first));
    assert!(first >= 20_000_000);
    engine.call_win64(entry, [output, 0, 0, 0, 0, 0]).unwrap();
    engine.read(output, &mut bytes).unwrap();
    assert!(u64::from_le_bytes(bytes) >= first);
    install_win64_import(&mut engine.unicorn, entry + 16, "msvcp140.dll", "_Thrd_id").unwrap();
    for id in [1, 123] {
        engine.unicorn.get_data_mut().current_windows_thread_id = id;
        assert_eq!(
            engine.call_win64(entry + 16, [0; 6]).unwrap(),
            u64::from(id)
        );
    }
}

#[test]
fn crt_time64_returns_current_seconds_and_checks_optional_output() {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    assert_eq!(crt_time64_seconds(UNIX_EPOCH), 0);
    assert_eq!(
        crt_time64_seconds(UNIX_EPOCH + Duration::from_millis(1999)),
        1
    );
    assert_eq!(crt_time64_seconds(UNIX_EPOCH - Duration::from_nanos(1)), -1);
    assert_eq!(
        crt_time64_seconds(UNIX_EPOCH + Duration::from_secs(32_535_215_999)),
        32_535_215_999
    );
    assert_eq!(
        crt_time64_seconds(UNIX_EPOCH + Duration::from_secs(32_535_216_000)),
        -1
    );
    for dll in ["api-ms-win-crt-time-l1-1-0.dll", "ucrtbase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "_time64").unwrap();
        let output = DATA_BASE + 0x100;
        for pointer in [0, output] {
            engine.write(output - 1, &[0xa5; 10]).unwrap();
            let before = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let result = engine.call_win64(entry, [pointer, 0, 0, 0, 0, 0]).unwrap();
            let after = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            assert!((before..=after).contains(&result));
            let mut bytes = [0; 10];
            engine.read(output - 1, &mut bytes).unwrap();
            assert_eq!(bytes[0], 0xa5);
            assert_eq!(bytes[9], 0xa5);
            if pointer == 0 {
                assert_eq!(bytes, [0xa5; 10]);
            } else {
                assert_eq!(u64::from_le_bytes(bytes[1..9].try_into().unwrap()), result);
            }
        }
        let edge = DATA_BASE + PAGE_SIZE - 4;
        engine.write(edge, &[0xa5; 4]).unwrap();
        assert!(engine.call_win64(entry, [edge, 0, 0, 0, 0, 0]).is_err());
        let mut bytes = [0; 4];
        engine.read(edge, &mut bytes).unwrap();
        assert_eq!(bytes, [0xa5; 4]);
        engine
            .unicorn
            .mem_map(0x50000000, PAGE_SIZE, Prot::READ | Prot::EXEC)
            .unwrap();
        assert!(
            engine
                .call_win64(entry, [0x50000000, 0, 0, 0, 0, 0])
                .is_err()
        );
    }
    assert!(matches!(
        dispatch_win64_import("other.dll", "_time64"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn guest_asset_streams_read_real_bytes_and_close_without_reusing_tokens() {
    let dir = std::env::temp_dir().join(format!(
        "aex-assets-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let raw = b"a\r\nb\x1az";
    std::fs::write(dir.join("asset.bin"), raw).unwrap();
    let manifest = dir.join("files.json");
    std::fs::write(
        &manifest,
        r#"{"files":[{"name":"c:/Assets/data.bin","path":"asset.bin"}]}"#,
    )
    .unwrap();
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        engine.unicorn.get_data_mut().guest_files = GuestFiles::from_manifest(&manifest).unwrap();
        let open = STUB_BASE + 0x100;
        let read = open + 16;
        let close = open + 32;
        let secure = open + 48;
        for (entry, name) in [
            (open, "fopen"),
            (read, "fread"),
            (close, "fclose"),
            (secure, "fopen_s"),
        ] {
            install_win64_import(&mut engine.unicorn, entry, dll, name).unwrap();
        }
        let name = DATA_BASE + 0x100;
        let mode = DATA_BASE + 0x200;
        let output = DATA_BASE + 0x300;
        engine.write(name, b"C:\\ASSETS\\data.bin\0").unwrap();
        let mut last_token = 0;
        for (flags, expected) in [
            (b"rb\0".as_slice(), raw.as_slice()),
            (b"rt\0", b"a\nb".as_slice()),
        ] {
            engine.write(mode, flags).unwrap();
            engine.unicorn.get_data_mut().crt_errno = 77;
            let token = engine.call_win64(open, [name, mode, 0, 0, 0, 0]).unwrap();
            assert_ne!(token, 0);
            assert_ne!(token, last_token);
            last_token = token;
            assert_eq!(engine.unicorn.get_data().crt_errno, 77);
            assert!(!guest_range_has_permission(&engine.unicorn, token, 8, Prot::EXEC).unwrap());
            if !guest_range_has_permission(&engine.unicorn, 0, 8, Prot::WRITE).unwrap() {
                engine
                    .unicorn
                    .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
                    .unwrap();
            }
            engine.write(0, &[0xa5; 8]).unwrap();
            assert!(engine.call_win64(read, [0, 1, 2, token, 0, 0]).is_err());
            assert_eq!(engine.unicorn.mem_read_as_vec(0, 8).unwrap(), [0xa5; 8]);
            assert_eq!(
                engine.unicorn.get_data().guest_files.streams[&token].position,
                0
            );
            let edge = DATA_BASE + PAGE_SIZE - 1;
            engine.write(edge, &[0xa5]).unwrap();
            assert!(engine.call_win64(read, [edge, 1, 6, token, 0, 0]).is_err());
            assert_eq!(
                engine.unicorn.get_data().guest_files.streams[&token].position,
                0
            );
            engine.write(output, &[0xa5; 16]).unwrap();
            assert_eq!(
                engine
                    .call_win64(read, [output, 2, 4, token, 0, 0])
                    .unwrap(),
                expected.len() as u64 / 2
            );
            let mut buffer = [0; 16];
            engine.read(output, &mut buffer).unwrap();
            assert_eq!(&buffer[..expected.len()], expected);
            assert!(buffer[expected.len()..].iter().all(|b| *b == 0xa5));
            assert_eq!(
                engine
                    .call_win64(read, [output, 1, 1, token, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(engine.call_win64(close, [token, 0, 0, 0, 0, 0]).unwrap(), 0);
            assert!(engine.call_win64(close, [token, 0, 0, 0, 0, 0]).is_err());
            assert!(
                engine
                    .call_win64(read, [output, 1, 1, token, 0, 0])
                    .is_err()
            );
            assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 0);
        }
        assert_eq!(
            engine.unicorn.get_data().guest_files.reports[0]
                .sha256
                .as_deref(),
            Some(format!("{:x}", Sha256::digest(raw)).as_str())
        );
        engine.write(mode, b"rb\0").unwrap();
        assert_eq!(
            engine
                .call_win64(secure, [output, name, mode, 0, 0, 0])
                .unwrap(),
            0
        );
        let mut bytes = [0; 8];
        engine.read(output, &mut bytes).unwrap();
        let token = u64::from_le_bytes(bytes);
        assert_ne!(token, 0);
        engine.call_win64(close, [token, 0, 0, 0, 0, 0]).unwrap();
        engine.write(mode, b"w\0").unwrap();
        assert_eq!(
            engine.call_win64(open, [name, mode, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 13);
        assert_eq!(std::fs::read(dir.join("asset.bin")).unwrap(), raw);
        engine.write(mode, b"r\0").unwrap();
        engine.write(name, b"unmounted.file\0").unwrap();
        assert_eq!(
            engine.call_win64(open, [name, mode, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 2);
        assert_eq!(engine.call_win64(open, [0, mode, 0, 0, 0, 0]).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().crt_errno, 22);
        engine.write(name, b"c:/Assets/../data.bin\0").unwrap();
        assert!(engine.call_win64(open, [name, mode, 0, 0, 0, 0]).is_err());
        assert_eq!(engine.call_win64(read, [0, 0, 99, 0, 0, 0]).unwrap(), 0);
        assert!(
            engine
                .call_win64(read, [output, u64::MAX, 2, 0, 0, 0])
                .is_err()
        );
    }
    std::fs::write(
        &manifest,
        r#"{"files":[{"name":"a","path":"asset.bin"},{"name":"A","path":"asset.bin"}]}"#,
    )
    .unwrap();
    assert!(GuestFiles::from_manifest(&manifest).is_err());
    for symbol in ["fopen", "fread", "fclose"] {
        assert!(matches!(
            dispatch_win64_import("other.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn copysign_preserves_payloads_and_uses_scalar_xmm_sign_bits() {
    for dll in ["api-ms-win-crt-math-l1-1-0.dll", "ucrtbase.dll"] {
        for symbol in ["_copysign", "copysign"] {
            let mut engine = test_engine(&[0xc3]);
            let entry = STUB_BASE + 0x100;
            install_win64_import(&mut engine.unicorn, entry, dll, symbol).unwrap();
            for bits in [
                0u64,
                1,
                0x8000000000000000,
                0x3ff4000000000000,
                0xfff0000000000000,
                0x7ff0000000001234,
                0xfff8000000004321,
            ] {
                for sign_bits in [0u64, 0x8000000000000000, 0xfff8000000000000] {
                    let mut left = [0xa5; 16];
                    left[..8].copy_from_slice(&bits.to_le_bytes());
                    let mut right = [0x5a; 16];
                    right[..8].copy_from_slice(&sign_bits.to_le_bytes());
                    engine
                        .unicorn
                        .reg_write_long(RegisterX86::XMM0, &left)
                        .unwrap();
                    engine
                        .unicorn
                        .reg_write_long(RegisterX86::XMM1, &right)
                        .unwrap();
                    engine.call_win64(entry, [0; 6]).unwrap();
                    let result = engine.unicorn.reg_read_long(RegisterX86::XMM0).unwrap();
                    assert_eq!(
                        u64::from_le_bytes(result[..8].try_into().unwrap()),
                        (bits & 0x7fffffffffffffff) | (sign_bits & 0x8000000000000000)
                    );
                    assert_eq!(&result[8..], &left[8..]);
                    assert_eq!(
                        engine
                            .unicorn
                            .reg_read_long(RegisterX86::XMM1)
                            .unwrap()
                            .as_ref(),
                        &right
                    );
                }
            }
            assert!(matches!(
                dispatch_win64_import("other.dll", symbol),
                Win64ImportDispatch::UnsupportedLegacyImport
            ));
        }
    }
}

#[test]
fn windows_mutex_named_ownership_recursion_and_close_are_stateful() {
    let mut engine = test_engine(&[0xc3]);
    let create = STUB_BASE + 0x100;
    let wait = create + 16;
    let release = create + 32;
    let close = create + 48;
    for (entry, name) in [
        (create, "CreateMutexA"),
        (wait, "WaitForSingleObject"),
        (release, "ReleaseMutex"),
        (close, "CloseHandle"),
    ] {
        install_win64_import(&mut engine.unicorn, entry, "kernel32.dll", name).unwrap();
    }
    engine.unicorn.get_data_mut().current_windows_thread_id = 1;
    let name = DATA_BASE + 0x100;
    engine.write(name, b"Local\\Fixture\0").unwrap();
    let first = engine.call_win64(create, [0, 1, name, 0, 0, 0]).unwrap();
    assert_ne!(first, 0);
    engine.write(name, b"Fixture\0").unwrap();
    let second = engine.call_win64(create, [0, 1, name, 0, 0, 0]).unwrap();
    assert_ne!(first, second);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 183);
    assert_eq!(engine.unicorn.get_data().windows_objects.objects.len(), 1);
    assert_eq!(engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(), 0);
    engine.unicorn.get_data_mut().current_windows_thread_id = 2;
    assert_eq!(
        engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(),
        258
    );
    assert!(engine.call_win64(wait, [second, 100, 0, 0, 0, 0]).is_err());
    assert_eq!(
        engine.call_win64(release, [second, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 288);
    engine.unicorn.get_data_mut().current_windows_thread_id = 1;
    assert_eq!(
        engine.call_win64(release, [second, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    engine.call_win64(close, [first, 0, 0, 0, 0, 0]).unwrap();
    engine.unicorn.get_data_mut().current_windows_thread_id = 2;
    assert_eq!(
        engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(),
        258
    );
    engine.unicorn.get_data_mut().windows_objects.abandon(1);
    assert_eq!(
        engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(),
        128
    );
    assert_eq!(
        engine.call_win64(release, [second, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        engine.call_win64(close, [second, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        engine.call_win64(close, [second, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    assert_eq!(
        engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(),
        u32::MAX as u64
    );
    assert_eq!(
        engine.call_win64(release, [second, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert!(engine.unicorn.get_data().windows_objects.names.is_empty());
    let replacement = engine.call_win64(create, [0, 0, name, 0, 0, 0]).unwrap();
    assert_ne!(replacement, second);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 0);
    engine.write(name, b"fixture\0").unwrap();
    engine.call_win64(create, [0, 0, name, 0, 0, 0]).unwrap();
    assert_eq!(engine.unicorn.get_data().windows_objects.names.len(), 2);
    engine.write(name, b"Local\\Global\\bad\0").unwrap();
    assert!(engine.call_win64(create, [0, 0, name, 0, 0, 0]).is_err());
    assert!(
        engine
            .call_win64(create, [DATA_BASE, 0, 0, 0, 0, 0])
            .is_err()
    );
    engine.unicorn.get_data_mut().windows_objects.issued = 65536;
    assert_eq!(engine.call_win64(create, [0, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 8);
    for symbol in ["CreateMutexA", "ReleaseMutex"] {
        assert!(matches!(
            dispatch_win64_import("other.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn windows_mutex_is_abandoned_when_guest_owner_thread_returns() {
    const CREATE_MUTEX: u64 = STUB_BASE + 0x100;
    const CREATE_THREAD: u64 = STUB_BASE + 0x200;
    const WAIT: u64 = STUB_BASE + 0x300;
    const OUT: u64 = DATA_BASE + 0x100;
    let mut code = vec![
        0x48, 0x83, 0xec, 0x28, 0x31, 0xc9, 0xba, 1, 0, 0, 0, 0x45, 0x31, 0xc0, 0x48, 0xb8,
    ];
    code.extend_from_slice(&CREATE_MUTEX.to_le_bytes());
    code.extend_from_slice(&[0xff, 0xd0, 0x49, 0xbb]);
    code.extend_from_slice(&OUT.to_le_bytes());
    code.extend_from_slice(&[0x49, 0x89, 0x03, 0x48, 0x83, 0xc4, 0x28, 0x31, 0xc0, 0xc3]);
    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine
        .write(8, &(STACK_BASE + STACK_SIZE).to_le_bytes())
        .unwrap();
    engine.write(16, &STACK_BASE.to_le_bytes()).unwrap();
    engine.unicorn.get_data_mut().current_windows_thread_id = 1;
    engine.unicorn.get_data_mut().next_windows_thread_id = 2;

    for (entry, name) in [
        (CREATE_MUTEX, "CreateMutexA"),
        (CREATE_THREAD, "CreateThread"),
        (WAIT, "WaitForSingleObject"),
    ] {
        install_win64_import(&mut engine.unicorn, entry, "kernel32.dll", name).unwrap();
    }
    engine
        .call_win64(CREATE_THREAD, [0, STACK_SIZE, TEST_CODE, 0, 0, 0])
        .unwrap();
    let mut bytes = [0; 8];
    engine.read(OUT, &mut bytes).unwrap();
    let mutex = u64::from_le_bytes(bytes);
    assert_ne!(mutex, 0);
    assert_eq!(
        engine.call_win64(WAIT, [mutex, 0, 0, 0, 0, 0]).unwrap(),
        128
    );
    assert_eq!(engine.call_win64(WAIT, [mutex, 0, 0, 0, 0, 0]).unwrap(), 0);
}

#[test]
fn windows_semaphore_counts_share_namespace_and_reject_invalid_release_atomically() {
    let mut engine = test_engine(&[0xc3]);
    let create = STUB_BASE + 0x100;
    let release = create + 16;
    let wait = create + 32;
    let close = create + 48;
    let mutex = create + 64;
    let release_mutex = create + 80;
    for (entry, name) in [
        (create, "CreateSemaphoreA"),
        (release, "ReleaseSemaphore"),
        (wait, "WaitForSingleObject"),
        (close, "CloseHandle"),
        (mutex, "CreateMutexA"),
        (release_mutex, "ReleaseMutex"),
    ] {
        install_win64_import(&mut engine.unicorn, entry, "kernel32.dll", name).unwrap();
    }
    let name = DATA_BASE + 0x100;
    let output = DATA_BASE + 0x200;
    engine.write(name, b"Counter\0").unwrap();
    let first = engine.call_win64(create, [0, 1, 2, name, 0, 0]).unwrap();
    assert_ne!(first, 0);
    let second = engine.call_win64(create, [0, 0, 99, name, 0, 0]).unwrap();
    assert_ne!(first, second);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 183);
    assert_eq!(engine.call_win64(mutex, [0, 0, name, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    assert_eq!(
        engine
            .call_win64(release_mutex, [first, 0, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    assert_eq!(engine.call_win64(wait, [first, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(),
        258
    );
    assert!(engine.call_win64(wait, [second, 10, 0, 0, 0, 0]).is_err());
    engine.write(output, &[0xa5; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(release, [second, 2, output, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [0, 0, 0, 0, 0xa5, 0xa5, 0xa5, 0xa5]
    );
    engine.write(output, &[0xa5; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64(release, [second, 1, output, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 298);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [0xa5; 8]
    );
    assert_eq!(engine.call_win64(wait, [first, 0, 0, 0, 0, 0]).unwrap(), 0);
    let edge = DATA_BASE + PAGE_SIZE - 2;
    engine.write(edge, &[0xa5; 2]).unwrap();
    assert!(
        engine
            .call_win64(release, [first, 1, edge, 0, 0, 0])
            .is_err()
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(edge, 2).unwrap(), [0xa5; 2]);
    assert_eq!(engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.call_win64(wait, [second, 0, 0, 0, 0, 0]).unwrap(),
        258
    );
    for amount in [0, u32::MAX as u64] {
        assert_eq!(
            engine
                .call_win64(release, [first, amount, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 87);
    }
    engine.call_win64(close, [first, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(
        engine.call_win64(release, [first, 1, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    assert_eq!(
        engine.call_win64(release, [second, 1, 0, 0, 0, 0]).unwrap(),
        1
    );
    engine.call_win64(close, [second, 0, 0, 0, 0, 0]).unwrap();
    assert!(engine.unicorn.get_data().windows_objects.names.is_empty());
    let owned = engine.call_win64(mutex, [0, 0, name, 0, 0, 0]).unwrap();
    assert_ne!(owned, 0);
    assert_eq!(engine.call_win64(create, [0, 0, 2, name, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    assert_eq!(
        engine.call_win64(release, [owned, 1, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    for (initial, maximum) in [(0, 0), (3, 2), (u32::MAX as u64, 2)] {
        assert_eq!(
            engine
                .call_win64(create, [0, initial, maximum, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 87);
    }
    for symbol in ["CreateSemaphoreA", "ReleaseSemaphore"] {
        assert!(matches!(
            dispatch_win64_import("other.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn avx_fallback_requires_registered_executable_code_and_complete_memory_permissions() {
    let mut engine = test_engine(&[0xc5, 0xfc, 0x10, 0x01, 0xc5, 0xfc, 0x11, 0x02, 0xc3]);
    let source = 0x51000000;
    let target = 0x52000000;
    engine
        .unicorn
        .mem_map(source, PAGE_SIZE, Prot::WRITE)
        .unwrap();
    engine
        .unicorn
        .mem_map(target, PAGE_SIZE, Prot::READ)
        .unwrap();
    engine.write(source, &[0x5a; 32]).unwrap();
    engine.write(target, &[0xa5; 32]).unwrap();
    assert!(
        engine
            .call_win64(TEST_CODE, [source, target, 0, 0, 0, 0])
            .is_err()
    );
    engine
        .unicorn
        .mem_protect(source, PAGE_SIZE, Prot::READ)
        .unwrap();
    assert!(
        engine
            .call_win64(TEST_CODE, [source, target, 0, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(target, 32).unwrap(),
        [0xa5; 32]
    );
    engine
        .unicorn
        .mem_protect(target, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let edge = target + PAGE_SIZE - 16;
    engine.write(edge, &[0xa5; 16]).unwrap();
    assert!(
        engine
            .call_win64(TEST_CODE, [source, edge, 0, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(edge, 16).unwrap(),
        [0xa5; 16]
    );
    engine
        .unicorn
        .get_data_mut()
        .image_executable_ranges
        .clear();
    assert!(
        engine
            .call_win64(TEST_CODE, [source, target, 0, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(target, 32).unwrap(),
        [0xa5; 32]
    );
}

#[test]
fn stdio_char_conversion_preserves_embedded_nul_and_consumes_promoted_int_slots() {
    for library in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        for secure in [false, true] {
            let mut engine = test_engine(&[0xc3]);
            let entry = STUB_BASE + 0x100;
            let symbol = if secure {
                "__stdio_common_vsnprintf_s"
            } else {
                "__stdio_common_vsprintf"
            };
            install_win64_import(&mut engine.unicorn, entry, library, symbol).unwrap();
            let format = DATA_BASE + 0x100;
            let arguments = DATA_BASE + 0x200;
            let output = DATA_BASE + 0x300;
            engine.write(format, b"%c%c%d\0").unwrap();
            for (index, value) in [0xdeadbeef112233ffu64, 0, 7].iter().enumerate() {
                engine
                    .write(arguments + index as u64 * 8, &value.to_le_bytes())
                    .unwrap();
            }
            engine.write(output, &[0xa5; 8]).unwrap();
            let args = if secure {
                vec![0x24, output, 8, u64::MAX, format, 0, arguments]
            } else {
                vec![0x25, output, 8, format, 0, arguments]
            };
            assert_eq!(
                engine
                    .call_win64_with_timeout(entry, &args, TIMEOUT_MICROSECONDS)
                    .unwrap(),
                3
            );
            assert_eq!(
                engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
                [0xff, 0, b'7', 0, 0xa5, 0xa5, 0xa5, 0xa5]
            );
            if secure {
                engine.write(output, &[0xa5; 8]).unwrap();
                assert_eq!(
                    engine
                        .call_win64_with_timeout(
                            entry,
                            &[0x24, output, 3, u64::MAX, format, 0, arguments],
                            TIMEOUT_MICROSECONDS
                        )
                        .unwrap() as u32,
                    u32::MAX
                );
                assert_eq!(
                    engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
                    [0xff, 0, 0, 0xa5]
                );
            }
        }
    }
}

#[test]
fn allocated_sids_encode_authority_and_stack_arguments_and_free_only_owned_storage() {
    let mut engine = test_engine(&[0xc3]);
    let allocate = STUB_BASE + 0x100;
    let free = allocate + 16;
    install_win64_import(
        &mut engine.unicorn,
        allocate,
        "advapi32.dll",
        "AllocateAndInitializeSid",
    )
    .unwrap();
    install_win64_import(&mut engine.unicorn, free, "advapi32.dll", "FreeSid").unwrap();
    let authority = DATA_BASE + 0x100;
    let output = DATA_BASE + 0x200;
    let authority_bytes = [1, 2, 3, 4, 5, 6];
    engine.write(authority, &authority_bytes).unwrap();
    let mut previous = 0;
    for count in [0u64, 1, 8] {
        let mut args = vec![authority, count];
        args.extend((0..8).map(|i| 0x1122334400000100u64 + i));
        args.push(output);
        engine.write(output - 1, &[0xa5; 10]).unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(allocate, &args, TIMEOUT_MICROSECONDS)
                .unwrap(),
            1
        );
        let mut slot = [0; 10];
        engine.read(output - 1, &mut slot).unwrap();
        assert_eq!(slot[0], 0xa5);
        assert_eq!(slot[9], 0xa5);
        let sid = u64::from_le_bytes(slot[1..9].try_into().unwrap());
        assert_ne!(sid, previous);
        previous = sid;
        let mut expected = vec![1, count as u8];
        expected.extend(authority_bytes);
        for i in 0..count {
            expected.extend_from_slice(&(0x100u32 + i as u32).to_le_bytes());
        }
        assert_eq!(
            engine.unicorn.mem_read_as_vec(sid, expected.len()).unwrap(),
            expected
        );
        assert!(!guest_range_has_permission(&engine.unicorn, sid, 8, Prot::EXEC).unwrap());
        assert!(engine.call_win64(free, [sid + 4, 0, 0, 0, 0, 0]).is_err());
        assert_eq!(engine.call_win64(free, [sid, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert!(engine.unicorn.mem_read_as_vec(sid, 1).is_err());
        assert!(engine.call_win64(free, [sid, 0, 0, 0, 0, 0]).is_err());
    }
    let mut args = vec![authority, 9, 0, 0, 0, 0, 0, 0, 0, 0, output];
    engine.write(output, &[0xa5; 8]).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(allocate, &args, TIMEOUT_MICROSECONDS)
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1337);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [0xa5; 8]
    );
    args[1] = 1;
    args[10] = DATA_BASE + PAGE_SIZE - 4;
    engine.write(args[10], &[0xa5; 4]).unwrap();
    assert!(
        engine
            .call_win64_with_timeout(allocate, &args, TIMEOUT_MICROSECONDS)
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(args[10], 4).unwrap(),
        [0xa5; 4]
    );
    assert!(engine.unicorn.get_data().windows_sids.is_empty());
    args[10] = output;
    engine.unicorn.get_data_mut().windows_sid_issued = 4096;
    assert_eq!(
        engine
            .call_win64_with_timeout(allocate, &args, TIMEOUT_MICROSECONDS)
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 8);
    for symbol in ["AllocateAndInitializeSid", "FreeSid"] {
        assert!(matches!(
            dispatch_win64_import("other.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn acl_builder_encodes_deny_before_allow_and_localfree_owns_the_buffer() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    let free = entry + 16;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "advapi32.dll",
        "SetEntriesInAclA",
    )
    .unwrap();
    install_win64_import(&mut engine.unicorn, free, "kernel32.dll", "LocalFree").unwrap();
    let entries = DATA_BASE + 0x100;
    let sid1 = DATA_BASE + 0x200;
    let sid2 = sid1 + 32;
    let output = DATA_BASE + 0x300;
    let first = [1, 1, 0, 0, 0, 0, 0, 5, 1, 0, 0, 0];
    let second = [1, 1, 0, 0, 0, 0, 0, 5, 2, 0, 0, 0];
    engine.write(sid1, &first).unwrap();
    engine.write(sid2, &second).unwrap();
    let mut input = [0u8; 96];
    for (index, mask, mode, flags, sid) in [
        (0, 0x11u32, 1u32, 1u32, sid1),
        (1, 0x22u32, 3u32, 2u32, sid2),
    ] {
        let offset = index * 48;
        input[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
        input[offset + 4..offset + 8].copy_from_slice(&mode.to_le_bytes());
        input[offset + 8..offset + 12].copy_from_slice(&flags.to_le_bytes());
        input[offset + 40..offset + 48].copy_from_slice(&sid.to_le_bytes());
    }
    engine.write(entries, &input).unwrap();
    engine.write(output - 1, &[0xa5; 10]).unwrap();
    assert_eq!(
        engine
            .call_win64(entry, [2, entries, 0, output, 0, 0])
            .unwrap(),
        0
    );
    let slot = engine.unicorn.mem_read_as_vec(output - 1, 10).unwrap();
    assert_eq!(slot[0], 0xa5);
    assert_eq!(slot[9], 0xa5);
    let acl = u64::from_le_bytes(slot[1..9].try_into().unwrap());
    assert_ne!(acl, 0);
    let mut expected = vec![2, 0, 48, 0, 2, 0, 0, 0, 1, 2, 20, 0, 0x22, 0, 0, 0];
    expected.extend(second);
    expected.extend([0, 1, 20, 0, 0x11, 0, 0, 0]);
    expected.extend(first);
    assert_eq!(engine.unicorn.mem_read_as_vec(acl, 48).unwrap(), expected);
    engine.write(sid1, &[0; 12]).unwrap();
    assert_eq!(engine.unicorn.mem_read_as_vec(acl, 48).unwrap(), expected);
    assert!(!guest_range_has_permission(&engine.unicorn, acl, 48, Prot::EXEC).unwrap());
    assert_eq!(
        engine.call_win64(free, [acl + 4, 0, 0, 0, 0, 0]).unwrap(),
        acl + 4
    );
    assert_eq!(engine.call_win64(free, [acl, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert!(engine.unicorn.mem_read_as_vec(acl, 1).is_err());
    assert_eq!(engine.call_win64(free, [acl, 0, 0, 0, 0, 0]).unwrap(), acl);
    engine.write(sid1, &first).unwrap();
    let edge = DATA_BASE + PAGE_SIZE - 4;
    engine.write(edge, &[0xa5; 4]).unwrap();
    assert!(
        engine
            .call_win64(entry, [2, entries, 0, edge, 0, 0])
            .is_err()
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(edge, 4).unwrap(), [0xa5; 4]);
    assert!(engine.unicorn.get_data().windows_acl_allocations.is_empty());
    assert!(
        engine
            .call_win64(entry, [1025, entries, 0, output, 0, 0])
            .is_err()
    );
    assert!(
        engine
            .call_win64(entry, [2, entries, sid1, output, 0, 0])
            .is_err()
    );
    engine
        .write(entries + 48 + 40, &sid1.to_le_bytes())
        .unwrap();
    assert!(
        engine
            .call_win64(entry, [2, entries, 0, output, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.call_win64(entry, [0, 0, 0, output, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 8).unwrap(), [0; 8]);
    assert!(matches!(
        dispatch_win64_import("other.dll", "SetEntriesInAclA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn named_file_dacl_updates_report_readonly_or_missing_without_changing_host_files() {
    let path = std::env::temp_dir().join(format!(
        "aex-security-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, b"original").unwrap();
    let original_readonly = std::fs::metadata(&path).unwrap().permissions().readonly();
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    let name = DATA_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "advapi32.dll",
        "SetNamedSecurityInfoA",
    )
    .unwrap();
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .sources
        .insert("c:/assets/config.text".into(), path.clone());
    engine.unicorn.get_data_mut().windows_last_error = 77;
    for (filename, expected) in [
        ("C:\\Assets\\config.text", 5),
        ("c:/assets", 5),
        ("c:/missing", 2),
    ] {
        engine
            .write(name, format!("{filename}\0").as_bytes())
            .unwrap();
        assert_eq!(
            engine
                .call_win64_with_timeout(
                    entry,
                    &[name, 1, 4, u64::MAX, u64::MAX, u64::MAX, u64::MAX],
                    TIMEOUT_MICROSECONDS
                )
                .unwrap(),
            expected
        );
    }
    assert_eq!(engine.unicorn.get_data().windows_last_error, 77);
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().readonly(),
        original_readonly
    );
    assert_eq!(
        engine
            .call_win64_with_timeout(entry, &[0, 1, 4, 0, 0, 0, 0], TIMEOUT_MICROSECONDS)
            .unwrap(),
        87
    );
    assert!(
        engine
            .call_win64_with_timeout(entry, &[name, 6, 4, 0, 0, 0, 0], TIMEOUT_MICROSECONDS)
            .is_err()
    );
    engine.write(name, b"c:/assets/config.text\0").unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(entry, &[name, 1, 4, 0, 0, 0, 0], TIMEOUT_MICROSECONDS)
            .unwrap(),
        2
    );
    engine
        .unicorn
        .mem_map(0x55000000, PAGE_SIZE, Prot::WRITE)
        .unwrap();
    engine.write(0x55000000, b"x\0").unwrap();
    assert!(
        engine
            .call_win64_with_timeout(entry, &[0x55000000, 1, 4, 0, 0, 0, 0], TIMEOUT_MICROSECONDS)
            .is_err()
    );
    assert!(matches!(
        dispatch_win64_import("other.dll", "SetNamedSecurityInfoA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn getc_reads_unsigned_bytes_and_preserves_stream_position_at_eof() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let token = GUEST_STREAM_BASE;
        engine.unicorn.get_data_mut().guest_files.streams.insert(
            token,
            GuestFileStream {
                name: None,
                bytes: vec![0, 127, 128, 255].into_boxed_slice(),
                position: 0,
                readable: true,
                buffer_state: None,
            },
        );
        engine.unicorn.get_data_mut().crt_errno = 77;
        for (offset, name) in [(0x100, "getc"), (0x110, "fgetc")] {
            install_win64_import(&mut engine.unicorn, STUB_BASE + offset, dll, name).unwrap();
        }
        for (index, expected) in [0, 127, 128, 255, u32::MAX as u64, u32::MAX as u64]
            .into_iter()
            .enumerate()
        {
            let entry = STUB_BASE + if index % 2 == 0 { 0x100 } else { 0x110 };
            assert_eq!(
                engine.call_win64(entry, [token, 0, 0, 0, 0, 0]).unwrap(),
                expected
            );
        }
        assert_eq!(
            engine.unicorn.get_data().guest_files.streams[&token].position,
            4
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 77);
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .streams
            .remove(&token);
        assert!(
            engine
                .call_win64(STUB_BASE + 0x100, [token, 0, 0, 0, 0, 0])
                .is_err()
        );
        assert!(
            engine
                .call_win64(STUB_BASE + 0x110, [0, 0, 0, 0, 0, 0])
                .is_err()
        );
    }
    for name in ["getc", "fgetc"] {
        assert!(matches!(
            dispatch_win64_import("foreign.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn isspace_classifies_all_c_locale_bytes_and_int_eof() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-string-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "isspace").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 77;
        for input in -1i32..=255 {
            let argument = 0x1234_5678_0000_0000 | u64::from(input as u32);
            let result = engine.call_win64(entry, [argument, 0, 0, 0, 0, 0]).unwrap();
            assert_eq!(result != 0, [9, 10, 11, 12, 13, 32].contains(&input));
        }
        assert_eq!(engine.unicorn.get_data().crt_errno, 77);
        for input in [256u64, 0xffff_fffe] {
            assert!(engine.call_win64(entry, [input, 0, 0, 0, 0, 0]).is_err());
        }
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "isspace"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn isalnum_classifies_all_c_locale_bytes_and_int_eof() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-string-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "isalnum").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 77;
        for input in -1i32..=255 {
            let argument = 0x1234_5678_0000_0000 | u64::from(input as u32);
            let result = engine.call_win64(entry, [argument, 0, 0, 0, 0, 0]).unwrap();
            assert_eq!(
                result != 0,
                b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                    .iter()
                    .any(|byte| i32::from(*byte) == input)
            );
        }
        assert_eq!(engine.unicorn.get_data().crt_errno, 77);
        for input in [256u64, 0xffff_fffe] {
            assert!(engine.call_win64(entry, [input, 0, 0, 0, 0, 0]).is_err());
        }
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "isalnum"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn scanf_decimal_assigns_values_and_distinguishes_eof_from_mismatch() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "__stdio_common_vsscanf").unwrap();
        let (input, format, args, output) = (
            DATA_BASE + 0x100,
            DATA_BASE + 0x200,
            DATA_BASE + 0x300,
            DATA_BASE + 0x400,
        );
        engine.write(args, &output.to_le_bytes()).unwrap();
        engine.write(args + 8, &(output + 4).to_le_bytes()).unwrap();
        for (text, fmt, status, values) in [
            (" \t-2147483648", "%d", 1, vec![i32::MIN]),
            ("+2147483647tail", "%d", 1, vec![i32::MAX]),
            ("12, -7", "%d, %d", 2, vec![12, -7]),
            ("1234", "%2d%d", 2, vec![12, 34]),
            ("99 8", "%*d%d", 1, vec![8]),
            ("", "%d", u32::MAX as u64, vec![]),
            (" \n", "%d", u32::MAX as u64, vec![]),
            ("x", "%d", 0, vec![]),
            ("+", "%d", 0, vec![]),
            ("1", "%d%d", 1, vec![1]),
            ("1", "%*d%d", u32::MAX as u64, vec![]),
            ("1", "%*dx", u32::MAX as u64, vec![]),
        ] {
            engine.write(input, format!("{text}\0").as_bytes()).unwrap();
            engine.write(format, format!("{fmt}\0").as_bytes()).unwrap();
            engine.write(output, &[0xa5; 8]).unwrap();
            assert_eq!(
                engine
                    .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                    .unwrap(),
                status
            );
            let bytes = engine.unicorn.mem_read_as_vec(output, 8).unwrap();
            for (i, value) in values.iter().enumerate() {
                assert_eq!(&bytes[i * 4..i * 4 + 4], &value.to_le_bytes());
            }
            assert!(bytes[values.len() * 4..].iter().all(|b| *b == 0xa5));
        }
        engine.write(input, b"42xx\0").unwrap();
        engine.write(format, b"%d\0").unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [0, input, 2, format, 0, args])
                .unwrap(),
            1
        );
        engine
            .write(args, &(DATA_BASE + PAGE_SIZE - 2).to_le_bytes())
            .unwrap();
        assert!(
            engine
                .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                .is_err()
        );
        engine.write(args, &output.to_le_bytes()).unwrap();
        engine.write(input, b"2147483648\0").unwrap();
        assert!(
            engine
                .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                .is_err()
        );
        assert!(
            engine
                .call_win64(entry, [1, input, u64::MAX, format, 0, args])
                .is_err()
        );
    }
}

#[test]
fn load_library_ex_a_system32_search_reports_missing_driver() {
    for dll in ["kernel32.dll", "api-ms-win-core-libraryloader-l1-2-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        let name = DATA_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "LoadLibraryExA").unwrap();
        for (path, flags, expected) in [
            ("nvcuda.dll", 0x800, 0),
            ("KERNEL32", 0x800, WINDOWS_KERNEL32_MODULE_TOKEN),
            ("kernel32.dll", 0, WINDOWS_KERNEL32_MODULE_TOKEN),
            (
                "C:\\Windows\\System32\\kernel32.dll",
                0x800,
                WINDOWS_KERNEL32_MODULE_TOKEN,
            ),
            ("c:/wrong/kernel32.dll", 0x800, 0),
            ("", 0, 0),
        ] {
            engine.write(name, format!("{path}\0").as_bytes()).unwrap();
            engine.unicorn.get_data_mut().windows_last_error = 77;
            assert_eq!(
                engine.call_win64(entry, [name, 0, flags, 0, 0, 0]).unwrap(),
                expected
            );
            assert_eq!(
                engine.unicorn.get_data().windows_last_error,
                if expected == 0 {
                    ERROR_MOD_NOT_FOUND
                } else {
                    77
                }
            );
        }
        assert_eq!(engine.call_win64(entry, [name, 1, 0, 0, 0, 0]).unwrap(), 0);
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            ERROR_INVALID_PARAMETER
        );
        assert!(engine.call_win64(entry, [name, 0, 2, 0, 0, 0]).is_err());
        engine
            .unicorn
            .mem_protect(DATA_BASE, PAGE_SIZE, Prot::WRITE)
            .unwrap();
        assert!(engine.call_win64(entry, [name, 0, 0, 0, 0, 0]).is_err());
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "LoadLibraryExA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn guest_file_search_returns_real_metadata_and_owns_cursor_and_handles() {
    let source = std::env::temp_dir().join(format!(
        "aex-find-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&source, b"actual bytes").unwrap();
    let mut engine = test_engine(&[0xc3]);
    for name in [
        "c:/assets/nvrtc64_a.dll",
        "c:/assets/nvrtc64_b.dll",
        "c:/assets/nested/other.dll",
    ] {
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .sources
            .insert(name.into(), source.clone());
    }
    let (first, next, close) = (STUB_BASE + 0x100, STUB_BASE + 0x110, STUB_BASE + 0x120);
    for (entry, name) in [
        (first, "FindFirstFileA"),
        (next, "FindNextFileA"),
        (close, "FindClose"),
    ] {
        install_win64_import(&mut engine.unicorn, entry, "kernel32.dll", name).unwrap();
        assert!(matches!(
            dispatch_win64_import("foreign.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
    let (query, output) = (DATA_BASE + 0x100, DATA_BASE + 0x300);
    engine.write(query, b"C:\\ASSETS\\nvrtc64_*.dll\0").unwrap();
    let edge = DATA_BASE + PAGE_SIZE - 319;
    assert!(engine.call_win64(first, [query, edge, 0, 0, 0, 0]).is_err());
    assert!(engine.unicorn.get_data().guest_files.searches.is_empty());
    let token = engine
        .call_win64(first, [query, output, 0, 0, 0, 0])
        .unwrap();
    let record = engine.unicorn.mem_read_as_vec(output, 320).unwrap();
    assert_eq!(&record[..4], &1u32.to_le_bytes());
    assert_eq!(&record[32..36], &12u32.to_le_bytes());
    assert_eq!(&record[44..58], b"nvrtc64_a.dll\0");
    assert!(engine.call_win64(next, [token, edge, 0, 0, 0, 0]).is_err());
    assert_eq!(
        engine
            .call_win64(next, [token, output, 0, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output + 44, 14).unwrap(),
        b"nvrtc64_b.dll\0"
    );
    assert_eq!(
        engine
            .call_win64(next, [token, output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 18);
    assert_eq!(engine.call_win64(close, [token, 0, 0, 0, 0, 0]).unwrap(), 1);
    assert_eq!(engine.call_win64(close, [token, 0, 0, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
    let second = engine
        .call_win64(first, [query, output, 0, 0, 0, 0])
        .unwrap();
    assert_ne!(second, token);
    assert_eq!(
        engine
            .call_win64(next, [token, output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.call_win64(close, [second, 0, 0, 0, 0, 0]).unwrap();
    engine.write(query, b"c:/assets/absent*\0").unwrap();
    assert_eq!(
        engine
            .call_win64(first, [query, output, 0, 0, 0, 0])
            .unwrap(),
        u64::MAX
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 2);
    engine.write(query, b"c:/missing/*.dll\0").unwrap();
    assert_eq!(
        engine
            .call_win64(first, [query, output, 0, 0, 0, 0])
            .unwrap(),
        u64::MAX
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 3);
    let records =
        guest_find_records(&engine.unicorn.get_data().guest_files, "c:/assets/*.*").unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(&records[0][..4], &16u32.to_le_bytes());
    std::fs::remove_file(source).unwrap();
}

#[test]
fn guest_search_dos_dot_star_accepts_absent_extensions() {
    for (pattern, name, expected) in [
        ("name.*", "name", true),
        ("na*.*", "name", true),
        ("name.*", "name.dll", true),
        ("name.*", "named", false),
        ("*.*", "plain", true),
        ("nvrtc64_*.dll", "nvrtc64_120_0.dll", true),
        ("nvrtc64_*.dll", "nvrtc-builtins.dll", false),
    ] {
        assert_eq!(
            guest_star_match(pattern.as_bytes(), name.as_bytes()),
            expected
        );
    }
}

#[test]
fn crt_ascii_classes_cover_bytes_eof_and_reject_invalid_ints() {
    let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let lower = "abcdefghijklmnopqrstuvwxyz";
    let digits = "0123456789";
    let punct = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
    let cases = [
        ("isalpha", format!("{upper}{lower}")),
        ("isdigit", digits.to_owned()),
        ("isgraph", format!("{upper}{lower}{digits}{punct}")),
        ("islower", lower.to_owned()),
        ("isprint", format!(" {upper}{lower}{digits}{punct}")),
        ("ispunct", punct.to_owned()),
        ("isupper", upper.to_owned()),
        ("isxdigit", "0123456789ABCDEFabcdef".to_owned()),
    ];
    for (name, accepted) in cases {
        for dll in ["ucrtbase.dll", "api-ms-win-crt-string-l1-1-0.dll"] {
            let mut engine = test_engine(&[0xc3]);
            let entry = STUB_BASE + 0x100;
            install_win64_import(&mut engine.unicorn, entry, dll, name).unwrap();
            engine.unicorn.get_data_mut().crt_errno = 77;
            for input in -1i32..=255 {
                let argument = 0x1234_5678_0000_0000 | u64::from(input as u32);
                let result = engine.call_win64(entry, [argument, 0, 0, 0, 0, 0]).unwrap();
                assert_eq!(
                    result != 0,
                    accepted.bytes().any(|byte| i32::from(byte) == input),
                    "{dll}!{name}({input})"
                );
            }
            assert_eq!(engine.unicorn.get_data().crt_errno, 77);
            for input in [256u64, 0xffff_fffe, 0x8000_0000] {
                assert!(engine.call_win64(entry, [input, 0, 0, 0, 0, 0]).is_err());
            }
        }
        assert!(matches!(
            dispatch_win64_import("foreign.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn crt_string_read_refreshes_permissions_and_crosses_only_readable_spans() {
    const PAGE: u64 = 0x30_0000_0000;
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_map(PAGE, 4096, Prot::READ | Prot::WRITE)
        .unwrap();
    engine
        .unicorn
        .mem_map(PAGE + 4096, 4096, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.unicorn.mem_write(PAGE + 4094, b"ab").unwrap();
    engine.unicorn.mem_write(PAGE + 4096, b"c\0").unwrap();
    let read =
        |engine: &GuestEngine| read_crt_stdio_c_string(&engine.unicorn, PAGE + 4094, 4, "test");
    assert_eq!(read(&engine).unwrap(), b"abc");
    assert!(
        read_crt_stdio_c_string(&engine.unicorn, PAGE + 4094, 3, "test")
            .unwrap_err()
            .contains("exceeds")
    );
    engine
        .unicorn
        .mem_protect(PAGE + 4096, 4096, Prot::WRITE)
        .unwrap();
    assert!(read(&engine).unwrap_err().contains("not readable"));
    engine.unicorn.mem_write(PAGE + 4095, b"\0").unwrap();
    assert_eq!(read(&engine).unwrap(), b"a");
    engine.unicorn.mem_write(PAGE + 4095, b"b").unwrap();
    engine
        .unicorn
        .mem_protect(PAGE + 4096, 4096, Prot::READ)
        .unwrap();
    assert_eq!(read(&engine).unwrap(), b"abc");
    engine.unicorn.mem_unmap(PAGE + 4096, 4096).unwrap();
    assert!(read(&engine).unwrap_err().contains("not readable"));
    engine
        .unicorn
        .mem_map(PAGE + 4096, 4096, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.unicorn.mem_write(PAGE + 4096, b"d\0").unwrap();
    assert_eq!(read(&engine).unwrap(), b"abd");
}

#[test]
fn strncmp_obeys_count_nul_unsigned_ordering_and_read_boundaries() {
    const ENTRY: u64 = STUB_BASE + 0x100;
    const PAGE: u64 = 0x30_0000_0000;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-string-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, ENTRY, dll, "strncmp").unwrap();
        engine
            .unicorn
            .mem_map(PAGE, 4096, Prot::READ | Prot::WRITE)
            .unwrap();
        let left = DATA_BASE + 0x100;
        let right = DATA_BASE + 0x200;
        engine.unicorn.get_data_mut().crt_errno = 71;
        for (a, b, count, expected) in [
            (&b"abcX\0"[..], &b"abcY\0"[..], 3, 0i32),
            (&b"abcX\0"[..], &b"abcY\0"[..], 4, -1),
            (&b"a\0z"[..], &b"a\0b"[..], u64::MAX, 0),
            (&b"\xff\0"[..], &b"\x7f\0"[..], 1, 1),
            (&b"\0"[..], &b"A\0"[..], 1, -1),
            (&b"b\0"[..], &b"A\0"[..], 0x1_0000_0000, 1),
        ] {
            engine.write(left, a).unwrap();
            engine.write(right, b).unwrap();
            let got = engine
                .call_win64(ENTRY, [left, right, count, 0, 0, 0])
                .unwrap();
            assert_eq!((got as u32 as i32).signum(), expected);
            assert_eq!(got >> 32, 0);
        }
        assert_eq!(
            engine.call_win64(ENTRY, [0, u64::MAX, 0, 0, 0, 0]).unwrap(),
            0
        );
        engine.unicorn.mem_write(PAGE + 4095, b"x").unwrap();
        engine.write(right, b"xy\0").unwrap();
        assert_eq!(
            engine
                .call_win64(ENTRY, [PAGE + 4095, right, 1, 0, 0, 0])
                .unwrap(),
            0
        );
        assert!(
            engine
                .call_win64(ENTRY, [PAGE + 4095, right, 2, 0, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("not readable")
        );
        engine.unicorn.mem_write(PAGE + 4095, b"\0").unwrap();
        engine.write(right, b"\0").unwrap();
        assert_eq!(
            engine
                .call_win64(ENTRY, [PAGE + 4095, right, u64::MAX, 0, 0, 0])
                .unwrap(),
            0
        );
        engine.unicorn.mem_protect(PAGE, 4096, Prot::WRITE).unwrap();
        assert!(
            engine
                .call_win64(ENTRY, [right, PAGE + 4095, 1, 0, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("not readable")
        );
        assert!(engine.call_win64(ENTRY, [0, right, 1, 0, 0, 0]).is_err());
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "strncmp"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn strncmp_distinguishes_exact_count_from_exhausted_host_bound() {
    const PAGE: u64 = 0x30_0000_0000;
    const ENTRY: u64 = STUB_BASE + 0x100;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, ENTRY, "ucrtbase.dll", "strncmp").unwrap();
    engine
        .unicorn
        .mem_map(PAGE, MAX_CRT_STRING_BYTES, Prot::READ | Prot::WRITE)
        .unwrap();
    engine
        .unicorn
        .mem_write(PAGE, &vec![b'x'; MAX_CRT_STRING_BYTES as usize])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(ENTRY, [PAGE, PAGE, MAX_CRT_STRING_BYTES, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(
        engine
            .call_win64(ENTRY, [PAGE, PAGE, MAX_CRT_STRING_BYTES + 1, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("exceed")
    );
}

#[test]
fn strcmp_tracks_each_readable_span_and_refreshes_changed_mappings() {
    const LEFT: u64 = 0x30_0000_0000;
    const RIGHT: u64 = LEFT + 0x10000;
    const ENTRY: u64 = STUB_BASE + 0x100;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, ENTRY, "ucrtbase.dll", "strcmp").unwrap();
    for base in [LEFT, RIGHT] {
        for offset in [0, 4096] {
            engine
                .unicorn
                .mem_map(base + offset, 4096, Prot::READ | Prot::WRITE)
                .unwrap();
        }
    }
    engine.unicorn.mem_write(LEFT + 4094, b"ab").unwrap();
    engine.unicorn.mem_write(LEFT + 4096, b"c\0").unwrap();
    engine.unicorn.mem_write(RIGHT + 4095, b"a").unwrap();
    engine.unicorn.mem_write(RIGHT + 4096, b"bc\0").unwrap();
    let compare = |engine: &mut GuestEngine<'static>| {
        engine.call_win64(ENTRY, [LEFT + 4094, RIGHT + 4095, 0, 0, 0, 0])
    };
    assert_eq!(compare(&mut engine).unwrap(), 0);
    engine
        .unicorn
        .mem_protect(RIGHT + 4096, 4096, Prot::WRITE)
        .unwrap();
    assert!(
        compare(&mut engine)
            .unwrap_err()
            .to_string()
            .contains("right string address")
    );
    engine.unicorn.mem_write(LEFT + 4094, b"z").unwrap();
    assert!((compare(&mut engine).unwrap() as u32 as i32) > 0);
    engine.unicorn.mem_write(LEFT + 4094, b"a").unwrap();
    engine
        .unicorn
        .mem_protect(RIGHT + 4096, 4096, Prot::READ)
        .unwrap();
    assert_eq!(compare(&mut engine).unwrap(), 0);
    engine.unicorn.mem_unmap(LEFT + 4096, 4096).unwrap();
    assert!(
        compare(&mut engine)
            .unwrap_err()
            .to_string()
            .contains("left string address")
    );
    engine
        .unicorn
        .mem_map(LEFT + 4096, 4096, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.unicorn.mem_write(LEFT + 4096, b"c\0").unwrap();
    assert_eq!(compare(&mut engine).unwrap(), 0);
}

#[test]
fn standard_files_are_stable_owned_typed_and_not_reopened_after_close() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let iob = STUB_BASE + 0x100;
        let getc = iob + 16;
        let read = getc + 16;
        let close = read + 16;
        for (entry, name) in [
            (iob, "__acrt_iob_func"),
            (getc, "getc"),
            (read, "fread"),
            (close, "fclose"),
        ] {
            install_win64_import(&mut engine.unicorn, entry, dll, name).unwrap();
        }
        engine.unicorn.get_data_mut().crt_errno = 71;
        let mut tokens = Vec::new();
        for index in 0..3 {
            let token = engine.call_win64(iob, [index, 0, 0, 0, 0, 0]).unwrap();
            assert_eq!(
                token,
                engine
                    .call_win64(iob, [0xabcd_0000_0000_0000 | index, 0, 0, 0, 0, 0])
                    .unwrap()
            );
            assert!(!tokens.contains(&token));
            assert!(guest_range_has_permission(&engine.unicorn, token, 8, Prot::READ).unwrap());
            assert!(!guest_range_has_permission(&engine.unicorn, token, 8, Prot::WRITE).unwrap());
            assert!(!guest_range_has_permission(&engine.unicorn, token, 8, Prot::EXEC).unwrap());
            tokens.push(token);
        }
        assert_eq!(engine.unicorn.get_data().guest_files.next_stream, 3);
        assert_eq!(
            engine.call_win64(getc, [tokens[0], 0, 0, 0, 0, 0]).unwrap(),
            u32::MAX as u64
        );
        engine.write(DATA_BASE, &[0x5a; 8]).unwrap();
        assert_eq!(
            engine
                .call_win64(read, [DATA_BASE, 1, 8, tokens[0], 0, 0])
                .unwrap(),
            0
        );
        let mut output = [0; 8];
        engine.read(DATA_BASE, &mut output).unwrap();
        assert_eq!(output, [0x5a; 8]);
        for token in &tokens[1..] {
            assert!(
                engine
                    .call_win64(getc, [*token, 0, 0, 0, 0, 0])
                    .unwrap_err()
                    .to_string()
                    .contains("output-only")
            );
            assert!(
                engine
                    .call_win64(read, [DATA_BASE, 1, 8, *token, 0, 0])
                    .unwrap_err()
                    .to_string()
                    .contains("output-only")
            );
        }
        assert!(
            engine
                .call_win64(close, [tokens[0] + 8, 0, 0, 0, 0, 0])
                .is_err()
        );
        for (index, token) in tokens.iter().enumerate() {
            assert_eq!(
                engine.call_win64(close, [*token, 0, 0, 0, 0, 0]).unwrap(),
                0
            );
            assert_eq!(
                engine
                    .call_win64(iob, [index as u64, 0, 0, 0, 0, 0])
                    .unwrap(),
                *token
            );
            assert!(engine.call_win64(getc, [*token, 0, 0, 0, 0, 0]).is_err());
            assert!(engine.call_win64(close, [*token, 0, 0, 0, 0, 0]).is_err());
        }
        assert!(engine.unicorn.get_data().guest_files.streams.is_empty());
        assert_eq!(engine.unicorn.get_data().guest_files.next_stream, 3);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        for index in [3, u32::MAX as u64] {
            assert!(engine.call_win64(iob, [index, 0, 0, 0, 0, 0]).is_err());
        }
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "__acrt_iob_func"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn crt_locale_lock_balances_recursion_and_shares_lockit_locale_ownership() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let lock = STUB_BASE + 0x100;
        let unlock = lock + 16;
        let ctor = unlock + 16;
        let dtor = ctor + 16;
        install_win64_import(&mut engine.unicorn, lock, dll, "_lock_locales").unwrap();
        install_win64_import(&mut engine.unicorn, unlock, dll, "_unlock_locales").unwrap();
        install_win64_import(
            &mut engine.unicorn,
            ctor,
            "msvcp140.dll",
            "??0_Lockit@std@@QEAA@H@Z",
        )
        .unwrap();
        install_win64_import(
            &mut engine.unicorn,
            dtor,
            "msvcp140.dll",
            "??1_Lockit@std@@QEAA@XZ",
        )
        .unwrap();
        engine.unicorn.get_data_mut().current_windows_thread_id = 7;
        assert!(engine.call_win64(unlock, [0; 6]).is_err());
        engine.call_win64(lock, [0; 6]).unwrap();
        engine.call_win64(lock, [0; 6]).unwrap();
        engine.call_win64(ctor, [DATA_BASE, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(
            engine.unicorn.get_data().msvcp_lockit_locks[0],
            Some((7, 3))
        );
        engine.unicorn.get_data_mut().current_windows_thread_id = 8;
        assert!(
            engine
                .call_win64(lock, [0; 6])
                .unwrap_err()
                .to_string()
                .contains("contended")
        );
        assert!(engine.call_win64(unlock, [0; 6]).is_err());
        assert_eq!(
            engine.unicorn.get_data().msvcp_lockit_locks[0],
            Some((7, 3))
        );
        engine.unicorn.get_data_mut().current_windows_thread_id = 7;
        engine.call_win64(dtor, [DATA_BASE, 0, 0, 0, 0, 0]).unwrap();
        engine.call_win64(unlock, [0; 6]).unwrap();
        assert_eq!(
            engine.unicorn.get_data().msvcp_lockit_locks[0],
            Some((7, 1))
        );
        engine.call_win64(unlock, [0; 6]).unwrap();
        assert_eq!(engine.unicorn.get_data().msvcp_lockit_locks[0], None);
        engine.unicorn.get_data_mut().current_windows_thread_id = 8;
        engine.call_win64(lock, [0; 6]).unwrap();
        engine.call_win64(unlock, [0; 6]).unwrap();
        engine.unicorn.get_data_mut().msvcp_lockit_locks[0] = Some((8, u32::MAX));
        assert!(
            engine
                .call_win64(lock, [0; 6])
                .unwrap_err()
                .to_string()
                .contains("overflow")
        );
        assert_eq!(
            engine.unicorn.get_data().msvcp_lockit_locks[0],
            Some((8, u32::MAX))
        );
    }
    for name in ["_lock_locales", "_unlock_locales"] {
        assert!(matches!(
            dispatch_win64_import("foreign.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn pointer_encoding_round_trips_full_width_values_across_kernel_aliases() {
    let mut engine = test_engine(&[0xc3]);
    let encode = STUB_BASE + 0x100;
    let decode = encode + 16;
    install_win64_import(&mut engine.unicorn, encode, "kernel32.dll", "EncodePointer").unwrap();
    install_win64_import(
        &mut engine.unicorn,
        decode,
        "kernelbase.dll",
        "DecodePointer",
    )
    .unwrap();
    engine.unicorn.get_data_mut().crt_errno = 71;
    let old_error = engine.unicorn.get_data().windows_last_error;
    let inputs = [
        0,
        1,
        DATA_BASE,
        u64::MAX,
        0x8000_0000_0000_0000,
        0xdead_beef_1234_5678,
    ];
    let mut encodings = Vec::new();
    for value in inputs {
        let encoded = engine.call_win64(encode, [value, 0, 0, 0, 0, 0]).unwrap();
        assert!(!encodings.contains(&encoded));
        encodings.push(encoded);
        assert_eq!(
            engine.call_win64(encode, [value, 0, 0, 0, 0, 0]).unwrap(),
            encoded
        );
        assert_eq!(
            engine.call_win64(decode, [encoded, 0, 0, 0, 0, 0]).unwrap(),
            value
        );
    }
    assert_ne!(encodings[0], 0);
    assert!(engine.unicorn.get_data().pointer_encoding_key.is_some());
    assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    assert_eq!(engine.unicorn.get_data().windows_last_error, old_error);
    let encode2 = encode + 32;
    let decode2 = decode + 32;
    install_win64_import(
        &mut engine.unicorn,
        encode2,
        "kernelbase.dll",
        "EncodePointer",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        decode2,
        "kernel32.dll",
        "DecodePointer",
    )
    .unwrap();
    for (value, encoded) in inputs.into_iter().zip(encodings) {
        assert_eq!(
            engine.call_win64(encode2, [value, 0, 0, 0, 0, 0]).unwrap(),
            encoded
        );
        assert_eq!(
            engine
                .call_win64(decode2, [encoded, 0, 0, 0, 0, 0])
                .unwrap(),
            value
        );
    }
    let mut other = test_engine(&[0xc3]);
    assert!(other.unicorn.get_data().pointer_encoding_key.is_none());
    other.unicorn.get_data_mut().pointer_encoding_key = Some(0x1234_5678_9abc_def1);
    install_win64_import(&mut other.unicorn, encode, "kernel32.dll", "EncodePointer").unwrap();
    let original_key = engine.unicorn.get_data().pointer_encoding_key;
    other.call_win64(encode, [0; 6]).unwrap();
    assert_eq!(engine.unicorn.get_data().pointer_encoding_key, original_key);
    for name in ["EncodePointer", "DecodePointer"] {
        assert!(matches!(
            dispatch_win64_import("foreign.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn stream_buffer_pointer_queries_are_stable_optional_and_preflight_outputs() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let query = STUB_BASE + 0x100;
        let iob = query + 16;
        let close = iob + 16;
        install_win64_import(
            &mut engine.unicorn,
            query,
            dll,
            "_get_stream_buffer_pointers",
        )
        .unwrap();
        install_win64_import(&mut engine.unicorn, iob, dll, "__acrt_iob_func").unwrap();
        install_win64_import(&mut engine.unicorn, close, dll, "fclose").unwrap();
        let token = engine.call_win64(iob, [0; 6]).unwrap();
        let out = DATA_BASE + 0x100;
        engine.write(out, &[0x5a; 24]).unwrap();
        assert_eq!(engine.call_win64(query, [token, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert!(
            engine.unicorn.get_data().guest_files.streams[&token]
                .buffer_state
                .is_none()
        );
        assert!(
            engine
                .call_win64(query, [token, out, out + 8, token, 0, 0])
                .is_err()
        );
        let mut unchanged = [0; 24];
        engine.read(out, &mut unchanged).unwrap();
        assert_eq!(unchanged, [0x5a; 24]);
        assert!(
            engine.unicorn.get_data().guest_files.streams[&token]
                .buffer_state
                .is_none()
        );
        engine.unicorn.get_data_mut().crt_errno = 71;
        assert_eq!(
            engine
                .call_win64(query, [token, out, out + 8, out + 16, 0, 0])
                .unwrap(),
            0
        );
        let mut values = [0; 24];
        engine.read(out, &mut values).unwrap();
        let addresses: Vec<u64> = values
            .chunks_exact(8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
            .collect();
        assert_eq!(
            addresses,
            [
                GUEST_STREAM_BUFFER_BASE,
                GUEST_STREAM_BUFFER_BASE + 8,
                GUEST_STREAM_BUFFER_BASE + 16
            ]
        );
        let mut cells = [1; 20];
        engine.read(addresses[0], &mut cells).unwrap();
        assert_eq!(cells, [0; 20]);
        assert!(
            guest_range_has_permission(&engine.unicorn, addresses[0], 20, Prot::READ | Prot::WRITE)
                .unwrap()
        );
        assert!(
            !guest_range_has_permission(&engine.unicorn, addresses[0], 20, Prot::EXEC).unwrap()
        );
        for mask in 0..8 {
            let args = [
                token,
                if mask & 1 != 0 { out } else { 0 },
                if mask & 2 != 0 { out + 8 } else { 0 },
                if mask & 4 != 0 { out + 16 } else { 0 },
                0,
                0,
            ];
            assert_eq!(engine.call_win64(query, args).unwrap(), 0);
            engine.read(out, &mut unchanged).unwrap();
            assert_eq!(unchanged, values);
        }
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        assert!(
            engine
                .call_win64(query, [token + 8, out, 0, 0, 0, 0])
                .is_err()
        );
        assert_eq!(engine.call_win64(close, [token, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert!(
            !guest_range_has_permission(&engine.unicorn, addresses[0], 20, Prot::READ).unwrap()
        );
        assert!(engine.call_win64(query, [token, out, 0, 0, 0, 0]).is_err());
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_get_stream_buffer_pointers"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn exposed_file_cells_preserve_unbuffered_reads_and_reject_unknown_buffering() {
    let mut engine = test_engine(&[0xc3]);
    let token = GUEST_STREAM_BASE + 32 * PAGE_SIZE;
    engine
        .unicorn
        .mem_map(token, PAGE_SIZE, Prot::READ)
        .unwrap();
    engine.unicorn.get_data_mut().guest_files.live_bytes = 3;
    engine.unicorn.get_data_mut().guest_files.streams.insert(
        token,
        GuestFileStream {
            name: None,
            bytes: Box::from(&b"abc"[..]),
            position: 0,
            readable: true,
            buffer_state: None,
        },
    );
    let query = STUB_BASE + 0x100;
    let getc = query + 16;
    let read = getc + 16;
    let close = read + 16;
    for (entry, name) in [
        (query, "_get_stream_buffer_pointers"),
        (getc, "getc"),
        (read, "fread"),
        (close, "fclose"),
    ] {
        install_win64_import(&mut engine.unicorn, entry, "ucrtbase.dll", name).unwrap();
    }
    let out = DATA_BASE + 0x100;
    engine
        .call_win64(query, [token, out, out + 8, out + 16, 0, 0])
        .unwrap();
    let state = engine.unicorn.get_data().guest_files.streams[&token]
        .buffer_state
        .unwrap();
    assert_eq!(
        engine.call_win64(getc, [token, 0, 0, 0, 0, 0]).unwrap(),
        b'a' as u64
    );
    engine.write(state + 16, &1i32.to_le_bytes()).unwrap();
    assert!(
        engine
            .call_win64(getc, [token, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("buffering state")
    );
    assert_eq!(
        engine.unicorn.get_data().guest_files.streams[&token].position,
        1
    );
    engine.write(state + 16, &0i32.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(read, [out + 32, 1, 8, token, 0, 0])
            .unwrap(),
        2
    );
    let mut bytes = [0; 2];
    engine.read(out + 32, &mut bytes).unwrap();
    assert_eq!(&bytes, b"bc");
    engine.call_win64(close, [token, 0, 0, 0, 0, 0]).unwrap();
    assert!(!guest_range_has_permission(&engine.unicorn, state, 20, Prot::READ).unwrap());
    assert!(!guest_range_has_permission(&engine.unicorn, token, 1, Prot::READ).unwrap());
    assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 0);
}

#[test]
fn wsetlocale_queries_all_c_categories_and_rejects_unimplemented_mutation() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "_wsetlocale").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        let address = engine.call_win64(entry, [0; 6]).unwrap();
        let mut bytes = [0; 4];
        engine.unicorn.mem_read(address, &mut bytes).unwrap();
        assert_eq!(bytes, [b'C', 0, 0, 0]);
        assert!(guest_range_has_permission(&engine.unicorn, address, 4, Prot::READ).unwrap());
        assert!(!guest_range_has_permission(&engine.unicorn, address, 4, Prot::WRITE).unwrap());
        assert!(!guest_range_has_permission(&engine.unicorn, address, 4, Prot::EXEC).unwrap());
        for category in 0..=5 {
            assert_eq!(
                engine.call_win64(entry, [category, 0, 0, 0, 0, 0]).unwrap(),
                address
            );
            assert_eq!(
                engine
                    .call_win64(entry, [category, address, 0, 0, 0, 0])
                    .unwrap(),
                address
            );
        }
        assert_eq!(
            engine
                .call_win64(entry, [0xffff_ffff_0000_0001, 0, 0, 0, 0, 0])
                .unwrap(),
            address
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        for category in [6, u32::MAX as u64] {
            assert!(engine.call_win64(entry, [category, 0, 0, 0, 0, 0]).is_err());
        }
        for value in [[0, 0, 0, 0], [b'J', 0, 0, 0], [b'C', 0, b'x', 0]] {
            engine.unicorn.mem_write(DATA_BASE, &value).unwrap();
            assert!(
                engine
                    .call_win64(entry, [0, DATA_BASE, 0, 0, 0, 0])
                    .is_err()
            );
        }
        assert!(engine.call_win64(entry, [0, u64::MAX, 0, 0, 0, 0]).is_err());
        assert_eq!(engine.call_win64(entry, [0; 6]).unwrap(), address);
        let foreign = entry + 16;
        install_win64_import(&mut engine.unicorn, foreign, "foreign.dll", "_wsetlocale").unwrap();
        assert!(engine.call_win64(foreign, [0; 6]).is_err());
    }
}

#[test]
fn setlocale_ansi_and_wide_queries_share_c_locale_state() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let narrow = STUB_BASE + 0x100;
        let wide = narrow + 16;
        install_win64_import(&mut engine.unicorn, narrow, dll, "setlocale").unwrap();
        install_win64_import(&mut engine.unicorn, wide, dll, "_wsetlocale").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        let address = engine.call_win64(narrow, [0; 6]).unwrap();
        let mut bytes = [0; 2];
        engine.unicorn.mem_read(address, &mut bytes).unwrap();
        assert_eq!(bytes, *b"C\0");
        for category in 0..=5 {
            assert_eq!(
                engine
                    .call_win64(narrow, [category, address, 0, 0, 0, 0])
                    .unwrap(),
                address
            );
            assert_eq!(
                engine
                    .call_win64(narrow, [category, 0, 0, 0, 0, 0])
                    .unwrap(),
                address
            );
        }
        assert_eq!(engine.call_win64(wide, [0; 6]).unwrap() + 4, address);
        for value in [b"\0\0", b"Cx", b"ja"] {
            engine.unicorn.mem_write(DATA_BASE, value).unwrap();
            assert!(
                engine
                    .call_win64(narrow, [0, DATA_BASE, 0, 0, 0, 0])
                    .is_err()
            );
        }
        assert_eq!(engine.call_win64(narrow, [0; 6]).unwrap(), address);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        install_win64_import(&mut engine.unicorn, wide + 16, "foreign.dll", "setlocale").unwrap();
        assert!(engine.call_win64(wide + 16, [0; 6]).is_err());
    }
}

#[test]
fn crt_locale_codepage_tracks_supported_c_locale_and_preserves_errno() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let query = STUB_BASE + 0x100;
        let set = query + 32;
        install_win64_import(&mut engine.unicorn, query, dll, "___lc_codepage_func").unwrap();
        install_win64_import(&mut engine.unicorn, set, dll, "setlocale").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        assert_eq!(engine.call_win64(query, [u64::MAX; 6]).unwrap(), 0);
        let locale = engine.call_win64(set, [0; 6]).unwrap();
        engine.call_win64(set, [0, locale, 0, 0, 0, 0]).unwrap();
        assert_eq!(engine.call_win64(query, [0; 6]).unwrap(), 0);
        engine.unicorn.mem_write(DATA_BASE, b"ja-JP\0").unwrap();
        assert!(engine.call_win64(set, [0, DATA_BASE, 0, 0, 0, 0]).is_err());
        engine.unicorn.get_data_mut().current_windows_thread_id = 9;
        assert_eq!(engine.call_win64(query, [0; 6]).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        install_win64_import(
            &mut engine.unicorn,
            set + 32,
            "foreign.dll",
            "___lc_codepage_func",
        )
        .unwrap();
        assert!(engine.call_win64(set + 32, [0; 6]).is_err());
    }
}

#[test]
fn pctype_exposes_c_masks_signed_prefix_and_stable_readonly_storage() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "__pctype_func").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        let address = engine.call_win64(entry, [u64::MAX; 6]).unwrap();
        let mut table = [0; 768];
        engine.unicorn.mem_read(address - 256, &mut table).unwrap();
        for value in -128i32..=255 {
            let offset = ((value + 128) * 2) as usize;
            let mask = u16::from_le_bytes(table[offset..offset + 2].try_into().unwrap());
            let expected = match value {
                9..=13 => 0x28,
                0..=31 | 127 => 0x20,
                32 => 0x48,
                48..=57 => 0x84,
                65..=70 => 0x81,
                71..=90 => 0x01,
                97..=102 => 0x82,
                103..=122 => 0x02,
                33..=126 => 0x10,
                _ => 0,
            };
            assert_eq!(mask, expected, "character {value}");
        }
        assert!(
            guest_range_has_permission(&engine.unicorn, address - 256, 768, Prot::READ).unwrap()
        );
        assert!(!guest_range_has_permission(&engine.unicorn, address, 512, Prot::WRITE).unwrap());
        assert!(!guest_range_has_permission(&engine.unicorn, address, 512, Prot::EXEC).unwrap());
        assert_eq!(engine.call_win64(entry, [0; 6]).unwrap(), address);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        install_win64_import(
            &mut engine.unicorn,
            entry + 16,
            "foreign.dll",
            "__pctype_func",
        )
        .unwrap();
        assert!(engine.call_win64(entry + 16, [0; 6]).is_err());
    }
}

#[test]
fn crt_locale_names_are_six_null_c_categories_distinct_from_printable_names() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let query = STUB_BASE + 0x100;
        let set = query + 16;
        install_win64_import(&mut engine.unicorn, query, dll, "___lc_locale_name_func").unwrap();
        install_win64_import(&mut engine.unicorn, set, dll, "_wsetlocale").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        let address = engine.call_win64(query, [u64::MAX; 6]).unwrap();
        assert_ne!(address, 0);
        let mut values = [0xff; 48];
        engine.unicorn.mem_read(address, &mut values).unwrap();
        assert_eq!(values, [0; 48]);
        for category in 0..=5 {
            let printable = engine.call_win64(set, [category, 0, 0, 0, 0, 0]).unwrap();
            assert_ne!(printable, address);
            engine
                .call_win64(set, [category, printable, 0, 0, 0, 0])
                .unwrap();
        }
        engine.unicorn.get_data_mut().current_windows_thread_id = 9;
        assert_eq!(engine.call_win64(query, [0; 6]).unwrap(), address);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        assert!(guest_range_has_permission(&engine.unicorn, address, 48, Prot::READ).unwrap());
        assert!(!guest_range_has_permission(&engine.unicorn, address, 48, Prot::WRITE).unwrap());
        assert!(!guest_range_has_permission(&engine.unicorn, address, 48, Prot::EXEC).unwrap());
        install_win64_import(
            &mut engine.unicorn,
            set + 16,
            "foreign.dll",
            "___lc_locale_name_func",
        )
        .unwrap();
        assert!(engine.call_win64(set + 16, [0; 6]).is_err());
    }
}

#[test]
fn crt_mb_cur_max_tracks_supported_c_locale_and_preserves_errno() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-locale-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let query = STUB_BASE + 0x100;
        let set = query + 32;
        install_win64_import(&mut engine.unicorn, query, dll, "___mb_cur_max_func").unwrap();
        install_win64_import(&mut engine.unicorn, set, dll, "setlocale").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        assert_eq!(engine.call_win64(query, [u64::MAX; 6]).unwrap(), 1);
        let locale = engine.call_win64(set, [0; 6]).unwrap();
        engine.call_win64(set, [0, locale, 0, 0, 0, 0]).unwrap();
        assert_eq!(engine.call_win64(query, [0; 6]).unwrap(), 1);
        engine.unicorn.mem_write(DATA_BASE, b"ja-JP\0").unwrap();
        assert!(engine.call_win64(set, [0, DATA_BASE, 0, 0, 0, 0]).is_err());
        engine.unicorn.get_data_mut().current_windows_thread_id = 9;
        assert_eq!(engine.call_win64(query, [0; 6]).unwrap(), 1);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        install_win64_import(
            &mut engine.unicorn,
            set + 32,
            "foreign.dll",
            "___mb_cur_max_func",
        )
        .unwrap();
        assert!(engine.call_win64(set + 32, [0; 6]).is_err());
    }
}

#[test]
fn uncaught_exception_count_is_zero_at_normal_boundaries_without_suppressing_throws() {
    const CODE: u64 = 0x1000_0000;
    let code = selector_throw_fixture(false, 0xdead_beef, TEST_THROW_INFO, 0);
    let mut engine = test_engine(&code);
    install_test_cxx_throw(&mut engine);
    let query = STUB_BASE + 0x700;
    install_win64_import(
        &mut engine.unicorn,
        query,
        "vcruntime140.dll",
        "__uncaught_exceptions",
    )
    .unwrap();
    engine.unicorn.get_data_mut().crt_errno = 71;
    for thread in [1, 9] {
        engine.unicorn.get_data_mut().current_windows_thread_id = thread;
        assert_eq!(engine.call_win64(query, [u64::MAX; 6]).unwrap(), 0);
    }
    assert!(matches!(engine.call_win64(CODE, [0; 6]),
        Err(GuestError::Callback(message)) if message.contains("_CxxThrowException")));
    assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    install_win64_import(
        &mut engine.unicorn,
        query + 32,
        "foreign.dll",
        "__uncaught_exceptions",
    )
    .unwrap();
    assert!(engine.call_win64(query + 32, [0; 6]).is_err());
}

#[test]
fn host_read_failure_reports_address_and_extent() {
    let engine = test_engine(&[0xc3]);
    let mut bytes = [0; 7];
    let error = engine
        .read(0xdead_beef, &mut bytes)
        .unwrap_err()
        .to_string();
    assert!(error.contains("read guest data"));
    assert!(error.contains("address=0xdeadbeef, length=7"));
    assert!(error.contains("UC_ERR_READ_UNMAPPED"));
}

#[test]
fn popup_choices_survive_source_release_and_preserve_borrowed_descriptor() {
    let mut engine = test_engine(&[0xc3]);
    engine.unicorn.mem_write(HOST_ADD_PARAM, &[0xc3]).unwrap();
    engine
        .unicorn
        .add_code_hook(HOST_ADD_PARAM, HOST_ADD_PARAM, |uc, _, _| {
            capture_add_param(uc)
        })
        .unwrap();
    let source = 0x5000_0000;
    engine
        .unicorn
        .mem_map(source, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine
        .unicorn
        .mem_write(source, b"Union|Intersect\0")
        .unwrap();
    let mut definition = vec![0; abi::PF_PARAM_DEF_SIZE];
    definition[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
        .copy_from_slice(&7i32.to_le_bytes());
    let offset = abi::PARAM_U_OFFSET + abi::POPUP_NAMES_OFFSET;
    definition[offset..offset + 8].copy_from_slice(&source.to_le_bytes());
    engine.write(DATA_BASE, &definition).unwrap();
    engine
        .call_win64(HOST_ADD_PARAM, [1, u32::MAX as u64, DATA_BASE, 0, 0, 0])
        .unwrap();
    let captured = &engine.parameters()[0].bytes;
    let owned = u64::from_le_bytes(captured[offset..offset + 8].try_into().unwrap());
    assert_ne!(owned, source);
    let mut original = vec![0; definition.len()];
    engine.read(DATA_BASE, &mut original).unwrap();
    assert_eq!(original, definition);
    engine.unicorn.mem_unmap(source, PAGE_SIZE).unwrap();
    let mut text = [0; 16];
    engine.read(owned, &mut text).unwrap();
    assert_eq!(&text, b"Union|Intersect\0");
    assert!(!guest_range_has_permission(&engine.unicorn, owned, 16, Prot::WRITE).unwrap());
    assert!(!guest_range_has_permission(&engine.unicorn, owned, 16, Prot::EXEC).unwrap());
    // A stale source fails before publishing a parameter or consuming storage.
    assert!(
        engine
            .call_win64(HOST_ADD_PARAM, [1, 0, DATA_BASE, 0, 0, 0])
            .is_err()
    );
    assert_eq!(engine.parameters().len(), 1);
    assert_eq!(engine.unicorn.get_data().popup_choice_pages, 1);
}

#[test]
fn popup_choice_capture_bounds_null_and_failure_are_atomic() {
    let mut engine = test_engine(&[0xc3]);
    let mut definition = vec![0; abi::PF_PARAM_DEF_SIZE];
    let offset = abi::PARAM_U_OFFSET + abi::POPUP_NAMES_OFFSET;
    capture_popup_choices(&mut engine.unicorn, &mut definition).unwrap();
    assert_eq!(engine.unicorn.get_data().popup_choice_pages, 0);
    let source = 0x5000_0000u64;
    engine
        .unicorn
        .mem_map(source, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    definition[offset..offset + 8].copy_from_slice(&source.to_le_bytes());
    let original = definition.clone();
    engine.write(source, &[b'x'; 4096]).unwrap();
    assert!(capture_popup_choices(&mut engine.unicorn, &mut definition).is_err());
    assert_eq!(definition, original);
    assert_eq!(engine.unicorn.get_data().popup_choice_pages, 0);
    engine.write(source + 4095, &[0]).unwrap();
    capture_popup_choices(&mut engine.unicorn, &mut definition).unwrap();
    let owned = u64::from_le_bytes(definition[offset..offset + 8].try_into().unwrap());
    let mut bytes = [0; 4096];
    engine.read(owned, &mut bytes).unwrap();
    assert_eq!(bytes[4094], b'x');
    assert_eq!(bytes[4095], 0);
    engine.unicorn.get_data_mut().popup_choice_pages = MAX_POPUP_CHOICE_PAGES;
    definition = original.clone();
    assert!(capture_popup_choices(&mut engine.unicorn, &mut definition).is_err());
    assert_eq!(definition, original);
}

#[test]
fn current_process_returns_full_width_pseudo_handle_without_allocating() {
    for dll in [
        "kernel32.dll",
        "kernelbase.dll",
        "api-ms-win-core-processthreads-l1-1-0.dll",
    ] {
        let mut engine = test_engine(&[0xc3]);
        let query = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, query, dll, "GetCurrentProcess").unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 73;
        engine.unicorn.get_data_mut().crt_errno = 71;
        for thread in [1, 9] {
            engine.unicorn.get_data_mut().current_windows_thread_id = thread;
            assert_eq!(engine.call_win64(query, [0; 6]).unwrap(), u64::MAX);
        }
        assert_eq!(engine.unicorn.get_data().windows_last_error, 73);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        assert!(engine.unicorn.get_data().windows_threads.is_empty());
        install_win64_import(
            &mut engine.unicorn,
            query + 32,
            "foreign.dll",
            "GetCurrentProcess",
        )
        .unwrap();
        assert!(engine.call_win64(query + 32, [0; 6]).is_err());
    }
}

#[test]
fn affinity_masks_match_guest_topology_and_preflight_both_outputs() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "GetProcessAffinityMask").unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 73;
        engine.unicorn.get_data_mut().crt_errno = 71;
        let output = DATA_BASE + 0x100;
        engine.write(output, &[0xaa; 24]).unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [u64::MAX, output, output + 8, 0, 0, 0])
                .unwrap(),
            1
        );
        let mut bytes = [0; 24];
        engine.read(output, &mut bytes).unwrap();
        assert_eq!(&bytes[..8], &1u64.to_le_bytes());
        assert_eq!(&bytes[8..16], &1u64.to_le_bytes());
        assert_eq!(&bytes[16..], &[0xaa; 8]);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 73);
        for handle in [0, u32::MAX as u64, u64::MAX - 1] {
            assert_eq!(
                engine
                    .call_win64(entry, [handle, output, output + 8, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(
                engine.unicorn.get_data().windows_last_error,
                ERROR_INVALID_HANDLE
            );
        }
        engine.write(output, &[0xaa; 24]).unwrap();
        for invalid in [0, u64::MAX, DATA_BASE + PAGE_SIZE - 4] {
            assert_eq!(
                engine
                    .call_win64(entry, [u64::MAX, output, invalid, 0, 0, 0])
                    .unwrap(),
                0
            );
            engine.read(output, &mut bytes).unwrap();
            assert_eq!(bytes, [0xaa; 24]);
        }
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        let info = entry + 32;
        install_win64_import(&mut engine.unicorn, info, "kernel32.dll", "GetSystemInfo").unwrap();
        engine.call_win64(info, [output, 0, 0, 0, 0, 0]).unwrap();
        let mut mask = [0; 8];
        engine.read(output + 24, &mut mask).unwrap();
        assert_eq!(mask, 1u64.to_le_bytes());
        install_win64_import(
            &mut engine.unicorn,
            info + 32,
            "foreign.dll",
            "GetProcessAffinityMask",
        )
        .unwrap();
        assert!(engine.call_win64(info + 32, [0; 6]).is_err());
    }
}

#[test]
fn localtime64_converts_timestamp_and_keeps_independent_thread_storage() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-time-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "_localtime64").unwrap();
        let mut previous = None;
        for seconds in [0i64, 951_827_696, 2_147_483_648, 32_535_215_999] {
            engine.write(DATA_BASE, &seconds.to_le_bytes()).unwrap();
            engine.unicorn.get_data_mut().crt_errno = 71;
            let address = engine
                .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                .unwrap();
            assert_ne!(address, 0);
            if let Some(previous) = previous {
                assert_eq!(previous, address);
            }
            previous = Some(address);
            let mut bytes = [0; 36];
            engine.read(address, &mut bytes).unwrap();
            let expected: Vec<u8> = aex_host_time::localtime_fields(seconds)
                .unwrap()
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect();
            assert_eq!(&bytes[..], &expected);
            assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        }
        let first = previous.unwrap();
        let mut saved = [0; 36];
        engine.read(first, &mut saved).unwrap();
        engine.unicorn.get_data_mut().current_windows_thread_id = 9;
        engine.write(DATA_BASE, &0i64.to_le_bytes()).unwrap();
        let second = engine
            .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
            .unwrap();
        assert_ne!(first, second);
        let mut bytes = [0; 36];
        engine.read(first, &mut bytes).unwrap();
        assert_eq!(bytes, saved);
        for seconds in [-1i64, 32_535_216_000, i64::MAX] {
            engine.write(DATA_BASE, &seconds.to_le_bytes()).unwrap();
            assert_eq!(
                engine
                    .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, 22);
        }
        engine
            .unicorn
            .mem_protect(second, PAGE_SIZE, Prot::READ)
            .unwrap();
        let mut before = [0; 36];
        engine.read(second, &mut before).unwrap();
        engine
            .write(DATA_BASE, &951_827_696i64.to_le_bytes())
            .unwrap();
        assert!(
            engine
                .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                .is_err()
        );
        engine.read(second, &mut bytes).unwrap();
        assert_eq!(bytes, before);
        assert!(engine.call_win64(entry, [0; 6]).is_err());
        assert!(engine.call_win64(entry, [u64::MAX, 0, 0, 0, 0, 0]).is_err());
        install_win64_import(
            &mut engine.unicorn,
            entry + 32,
            "foreign.dll",
            "_localtime64",
        )
        .unwrap();
        assert!(engine.call_win64(entry + 32, [0; 6]).is_err());
    }
}

#[test]
fn ftime64_writes_current_time_and_preserves_padding() {
    use std::time::{SystemTime, UNIX_EPOCH};
    for dll in ["ucrtbase.dll", "api-ms-win-crt-time-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "_ftime64").unwrap();
        engine.write(DATA_BASE, &[0xa5; 16]).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        engine
            .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
            .unwrap();
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut bytes = [0; 16];
        engine.read(DATA_BASE, &mut bytes).unwrap();
        let seconds = i64::from_le_bytes(bytes[..8].try_into().unwrap());
        let millis = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
        assert!(millis < 1000);
        let actual = seconds as u128 * 1000 + u128::from(millis);
        assert!((before..=after).contains(&actual));
        let (west, dst) = aex_host_time::timeb_zone(seconds).unwrap();
        assert_eq!(&bytes[10..12], &west.to_le_bytes());
        assert_eq!(&bytes[12..14], &dst.to_le_bytes());
        assert_eq!(&bytes[14..], &[0xa5; 2]);
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        engine
            .unicorn
            .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
            .unwrap();
        assert!(
            engine
                .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                .is_err()
        );
        let mut unchanged = [0; 16];
        engine.read(DATA_BASE, &mut unchanged).unwrap();
        assert_eq!(bytes, unchanged);
        assert!(engine.call_win64(entry, [0; 6]).is_err());
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_ftime64"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn version_ex_a_checks_size_and_writes_unmanifested_process_view() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "GetVersionExA").unwrap();
        for size in [148u32, 156] {
            engine.write(DATA_BASE, &[0xa5; 160]).unwrap();
            engine.write(DATA_BASE, &size.to_le_bytes()).unwrap();
            engine.unicorn.get_data_mut().windows_last_error = 71;
            engine.unicorn.get_data_mut().crt_errno = 72;
            assert_eq!(
                engine
                    .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                    .unwrap(),
                1
            );
            let mut bytes = [0; 160];
            engine.read(DATA_BASE, &mut bytes).unwrap();
            let words: Vec<u32> = bytes[..20]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            assert_eq!(words, [size, 6, 2, 9200, 2]);
            assert!(bytes[20..148].iter().all(|b| *b == 0));
            if size == 156 {
                assert_eq!(&bytes[148..156], &[0, 0, 0, 0, 0, 0, 1, 0]);
            }
            assert!(bytes[size as usize..].iter().all(|b| *b == 0xa5));
            assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        }
        for size in [0u32, 147, 149, 155, 157, u32::MAX] {
            engine.write(DATA_BASE, &[0xa5; 160]).unwrap();
            engine.write(DATA_BASE, &size.to_le_bytes()).unwrap();
            let mut before = [0; 160];
            engine.read(DATA_BASE, &mut before).unwrap();
            assert_eq!(
                engine
                    .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            assert_eq!(engine.unicorn.get_data().windows_last_error, 87);
            let mut after = [0; 160];
            engine.read(DATA_BASE, &mut after).unwrap();
            assert_eq!(after, before);
        }
        engine.write(DATA_BASE, &148u32.to_le_bytes()).unwrap();
        engine
            .unicorn
            .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
            .unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 998);
        assert_eq!(engine.call_win64(entry, [0; 6]).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 998);
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "GetVersionExA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn system_metrics_reports_local_guest_session_and_rejects_unknown_metrics() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(&mut engine.unicorn, entry, "user32.dll", "GetSystemMetrics").unwrap();
    for argument in [0x1000, 0xabcd_1234_0000_1000] {
        engine.unicorn.get_data_mut().windows_last_error = 71;
        engine.unicorn.get_data_mut().crt_errno = 72;
        assert_eq!(
            engine.call_win64(entry, [argument, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    }
    for index in [0u64, 1, 0x2001, u64::MAX] {
        let error = engine
            .call_win64(entry, [index, 0, 0, 0, 0, 0])
            .unwrap_err();
        assert!(error.to_string().contains(&format!(
            "unsupported GetSystemMetrics index: {}",
            index as u32 as i32
        )));
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "GetSystemMetrics"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn strcat_appends_bytes_and_nul_without_touching_tail() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-string-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "strcat").unwrap();
        let source = DATA_BASE + 0x200;
        for (prefix, suffix) in [
            (b"ab".as_slice(), b"c\xff".as_slice()),
            (b"", b"x"),
            (b"x", b""),
        ] {
            engine.write(DATA_BASE, &[0xa5; 32]).unwrap();
            engine.write(DATA_BASE, &[prefix, b"\0"].concat()).unwrap();
            engine
                .write(source, &[suffix, b"\0ignored"].concat())
                .unwrap();
            engine.unicorn.get_data_mut().crt_errno = 71;
            assert_eq!(
                engine
                    .call_win64(entry, [DATA_BASE, source, 0, 0, 0, 0])
                    .unwrap(),
                DATA_BASE
            );
            let expected = [prefix, suffix, b"\0"].concat();
            let mut actual = [0; 32];
            engine.read(DATA_BASE, &mut actual).unwrap();
            assert_eq!(&actual[..expected.len()], &expected);
            assert!(actual[expected.len()..].iter().all(|b| *b == 0xa5));
            assert_eq!(engine.unicorn.get_data().crt_errno, 71);
        }
        engine.write(DATA_BASE, b"abc\0").unwrap();
        assert!(
            engine
                .call_win64(entry, [DATA_BASE, DATA_BASE + 1, 0, 0, 0, 0])
                .is_err()
        );
        let mut unchanged = [0; 4];
        engine.read(DATA_BASE, &mut unchanged).unwrap();
        assert_eq!(&unchanged, b"abc\0");
        assert!(engine.call_win64(entry, [0, source, 0, 0, 0, 0]).is_err());
        assert!(
            engine
                .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                .is_err()
        );
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "strcat"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn strcat_preflights_full_append_and_requires_terminated_inputs() {
    const PAGE: u64 = 0x30_0000_0000;
    for bad_source in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, "ucrtbase.dll", "strcat").unwrap();
        engine
            .unicorn
            .mem_map(PAGE, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        engine.write(PAGE + PAGE_SIZE - 4, b"ab\0X").unwrap();
        engine.write(DATA_BASE, b"test\0").unwrap();
        let (destination, source) = if bad_source {
            (DATA_BASE, PAGE + PAGE_SIZE - 1)
        } else {
            (PAGE + PAGE_SIZE - 4, DATA_BASE)
        };
        assert!(
            engine
                .call_win64(entry, [destination, source, 0, 0, 0, 0])
                .is_err()
        );
        let mut actual = [0; 4];
        engine.read(PAGE + PAGE_SIZE - 4, &mut actual).unwrap();
        assert_eq!(&actual, b"ab\0X");
        let mut original = [0; 5];
        engine.read(DATA_BASE, &mut original).unwrap();
        assert_eq!(&original, b"test\0");
        // An unterminated destination also fails before changing either input.
        assert!(
            engine
                .call_win64(entry, [PAGE + PAGE_SIZE - 1, DATA_BASE, 0, 0, 0, 0])
                .is_err()
        );
        // Empty suffix fits within the mapped page; this failure isolates WRITE protection.
        engine.write(DATA_BASE, b"\0").unwrap();
        engine
            .unicorn
            .mem_protect(PAGE, PAGE_SIZE, Prot::READ)
            .unwrap();
        assert!(
            engine
                .call_win64(entry, [PAGE + PAGE_SIZE - 4, DATA_BASE, 0, 0, 0, 0])
                .is_err()
        );
    }
}

#[test]
fn strchr_finds_first_byte_including_nul_and_preserves_source() {
    for dll in [
        "vcruntime140.dll",
        "ucrtbase.dll",
        "api-ms-win-crt-string-l1-1-0.dll",
    ] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "strchr").unwrap();
        engine.write(DATA_BASE, b"abac\xff\0z").unwrap();
        for (needle, expected) in [
            (b'a' as u64, DATA_BASE),
            (b'c' as u64, DATA_BASE + 3),
            (u64::MAX, DATA_BASE + 4),
            (0x1234_0000_0000_0100, DATA_BASE + 5),
            (b'z' as u64, 0),
        ] {
            engine.unicorn.get_data_mut().crt_errno = 71;
            engine.unicorn.get_data_mut().windows_last_error = 72;
            assert_eq!(
                engine
                    .call_win64(entry, [DATA_BASE, needle, 0, 0, 0, 0])
                    .unwrap(),
                expected
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, 71);
            assert_eq!(engine.unicorn.get_data().windows_last_error, 72);
        }
        let mut actual = [0; 7];
        engine.read(DATA_BASE, &mut actual).unwrap();
        assert_eq!(&actual, b"abac\xff\0z");
        assert!(engine.call_win64(entry, [0; 6]).is_err());
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "strchr"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn strchr_stops_at_boundary_match_and_rechecks_read_permissions() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    const PAGE: u64 = 0x30_0000_0000;
    let last = PAGE + PAGE_SIZE - 1;
    install_win64_import(&mut engine.unicorn, entry, "vcruntime140.dll", "strchr").unwrap();
    engine
        .unicorn
        .mem_map(PAGE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(last, b"X").unwrap();
    assert_eq!(
        engine
            .call_win64(entry, [last, b'X' as u64, 0, 0, 0, 0])
            .unwrap(),
        last
    );
    assert!(
        engine
            .call_win64(entry, [last, b'Y' as u64, 0, 0, 0, 0])
            .is_err()
    );
    engine.write(last, &[0]).unwrap();
    assert_eq!(
        engine.call_win64(entry, [last, 0, 0, 0, 0, 0]).unwrap(),
        last
    );
    assert_eq!(
        engine
            .call_win64(entry, [last, b'X' as u64, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    engine
        .unicorn
        .mem_protect(PAGE, PAGE_SIZE, Prot::WRITE)
        .unwrap();
    assert!(engine.call_win64(entry, [last, 0, 0, 0, 0, 0]).is_err());
}

#[test]
fn scanf_strings_and_scansets_honor_width_sets_and_suppression() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "ucrtbase.dll",
        "__stdio_common_vsscanf",
    )
    .unwrap();
    let (input, format, args, output) = (
        DATA_BASE + 0x100,
        DATA_BASE + 0x200,
        DATA_BASE + 0x300,
        DATA_BASE + 0x400,
    );
    engine.write(args, &output.to_le_bytes()).unwrap();
    engine
        .write(args + 8, &(output + 32).to_le_bytes())
        .unwrap();
    for (text, fmt, status, expected) in [
        (
            "key value with spaces\nnext",
            "%s %[^\n]",
            2,
            vec!["key", "value with spaces"],
        ),
        ("  abcdef", "%2s%s", 2, vec!["ab", "cdef"]),
        ("aBcdefZ0", "%[a-zA-Z]", 1, vec!["aBcdefZ"]),
        ("zyxa!", "%[z-a]", 1, vec!["zyxa"]),
        ("]]-x", "%[]-]", 1, vec!["]]-"]),
        ("abc]tail", "%[^]]", 1, vec!["abc"]),
        ("skip rest here", "%*s %[^\n]", 1, vec!["rest here"]),
        ("abc123", "%*[a-z]%s", 1, vec!["123"]),
        (" a", "%[a-z]", 0, vec![]),
        ("", "%[^\n]", u32::MAX as u64, vec![]),
        (" \t", "%s", u32::MAX as u64, vec![]),
        ("123", "%[a-z]", 0, vec![]),
    ] {
        engine.write(input, format!("{text}\0").as_bytes()).unwrap();
        engine.write(format, format!("{fmt}\0").as_bytes()).unwrap();
        engine.write(output, &[0xa5; 64]).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 71;
        assert_eq!(
            engine
                .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                .unwrap(),
            status,
            "{fmt}"
        );
        let mut actual = [0; 64];
        engine.read(output, &mut actual).unwrap();
        for index in 0..2 {
            let field = &actual[index * 32..index * 32 + 32];
            if let Some(value) = expected.get(index) {
                let value = format!("{value}\0");
                assert_eq!(&field[..value.len()], value.as_bytes(), "{fmt}");
                assert!(field[value.len()..].iter().all(|b| *b == 0xa5));
            } else {
                assert!(field.iter().all(|b| *b == 0xa5));
            }
        }
        assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    }
}

#[test]
fn scanf_scanset_rejects_malformed_format_and_preflights_output() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "api-ms-win-crt-stdio-l1-1-0.dll",
        "__stdio_common_vsscanf",
    )
    .unwrap();
    let (input, format, args) = (DATA_BASE + 0x100, DATA_BASE + 0x200, DATA_BASE + 0x300);
    const PAGE: u64 = 0x30_0000_0000;
    engine
        .unicorn
        .mem_map(PAGE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(input, b"abc\0").unwrap();
    engine
        .write(args, &(PAGE + PAGE_SIZE - 2).to_le_bytes())
        .unwrap();
    for fmt in [
        b"%[a-z]\0".as_slice(),
        b"%s\0",
        b"%[\0",
        b"%[]\0",
        b"%[^\0",
        b"%0s\0",
    ] {
        engine.write(format, fmt).unwrap();
        engine.write(PAGE + PAGE_SIZE - 2, &[0xa5; 2]).unwrap();
        assert!(
            engine
                .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                .is_err()
        );
        let mut actual = [0; 2];
        engine.read(PAGE + PAGE_SIZE - 2, &mut actual).unwrap();
        assert_eq!(actual, [0xa5; 2]);
    }
}

#[test]
fn atoi_converts_decimal_and_saturates_windows_int_overflow() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-convert-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "atoi").unwrap();
        for (text, expected, errno) in [
            (" \t\r\n\u{b}\u{c}-42tail", -42, 71),
            ("+17", 17, 71),
            ("", 0, 71),
            ("word", 0, 71),
            ("--2", 0, 71),
            ("+ 2", 0, 71),
            ("0x20", 0, 71),
            ("00123", 123, 71),
            ("2147483647", i32::MAX, 71),
            ("-2147483648", i32::MIN, 71),
            ("2147483648", i32::MAX, 34),
            ("-2147483649", i32::MIN, 34),
            ("999999999999999999999999999999999999", i32::MAX, 34),
            ("-999999999999999999999999999999999999", i32::MIN, 34),
        ] {
            let bytes = format!("{text}\0");
            engine.write(DATA_BASE, bytes.as_bytes()).unwrap();
            engine.unicorn.get_data_mut().crt_errno = 71;
            engine.unicorn.get_data_mut().windows_last_error = 72;
            assert_eq!(
                engine
                    .call_win64(entry, [DATA_BASE, 0, 0, 0, 0, 0])
                    .unwrap() as u32 as i32,
                expected,
                "{text}"
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, errno);
            assert_eq!(engine.unicorn.get_data().windows_last_error, 72);
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(DATA_BASE, bytes.len())
                    .unwrap(),
                bytes.as_bytes()
            );
        }
        assert!(engine.call_win64(entry, [0; 6]).is_err());
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "atoi"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn atoi_stops_at_first_non_digit_without_reading_next_page() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    const PAGE: u64 = 0x30_0000_0000;
    install_win64_import(&mut engine.unicorn, entry, "ucrtbase.dll", "atoi").unwrap();
    engine
        .unicorn
        .mem_map(PAGE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let source = PAGE + PAGE_SIZE - 3;
    engine.write(source, b"12x").unwrap();
    assert_eq!(
        engine.call_win64(entry, [source, 0, 0, 0, 0, 0]).unwrap(),
        12
    );
    engine.write(source, b"123").unwrap();
    assert!(engine.call_win64(entry, [source, 0, 0, 0, 0, 0]).is_err());
    engine.write(source, b"12\0").unwrap();
    engine
        .unicorn
        .mem_protect(PAGE, PAGE_SIZE, Prot::READ)
        .unwrap();
    assert_eq!(
        engine.call_win64(entry, [source, 0, 0, 0, 0, 0]).unwrap(),
        12
    );
    engine
        .unicorn
        .mem_protect(PAGE, PAGE_SIZE, Prot::WRITE)
        .unwrap();
    assert!(engine.call_win64(entry, [source, 0, 0, 0, 0, 0]).is_err());
}

#[test]
fn scanf_hexadecimal_accepts_sign_prefix_width_and_full_unsigned_word() {
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        let entry = STUB_BASE + 0x100;
        install_win64_import(&mut engine.unicorn, entry, dll, "__stdio_common_vsscanf").unwrap();
        let (input, format, args, output) = (
            DATA_BASE + 0x100,
            DATA_BASE + 0x200,
            DATA_BASE + 0x300,
            DATA_BASE + 0x400,
        );
        engine.write(args, &output.to_le_bytes()).unwrap();
        engine.write(args + 8, &(output + 4).to_le_bytes()).unwrap();
        for (text, fmt, status, errno, values) in [
            ("aBcD", "%x", 1, 71, vec![0xabcd]),
            ("  +0Xff", "%X", 1, 71, vec![255]),
            ("ffffffff", "%x", 1, 71, vec![u32::MAX]),
            ("-1", "%x", 1, 71, vec![u32::MAX]),
            ("-0x80000000", "%x", 1, 71, vec![0x80000000]),
            ("100000001", "%x", 1, 71, vec![1]),
            ("fffffffffffffffff", "%x", 1, 34, vec![u32::MAX]),
            ("-fffffffffffffffff", "%x", 1, 34, vec![u32::MAX]),
            ("1234", "%2x%x", 2, 71, vec![0x12, 0x34]),
            ("0x12", "%3x%x", 2, 71, vec![1, 2]),
            ("0x12", "%1x", 1, 71, vec![0]),
            ("0x12", "%2x", 0, 71, vec![]),
            ("ff 10", "%*x%d", 1, 71, vec![10]),
            ("g", "%x", 0, 71, vec![]),
            ("0x", "%x", 0, 71, vec![]),
            ("", "%x", u32::MAX as u64, 71, vec![]),
        ] {
            engine.write(input, format!("{text}\0").as_bytes()).unwrap();
            engine.write(format, format!("{fmt}\0").as_bytes()).unwrap();
            engine.write(output, &[0xa5; 12]).unwrap();
            engine.unicorn.get_data_mut().crt_errno = 71;
            assert_eq!(
                engine
                    .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                    .unwrap(),
                status,
                "{text} {fmt}"
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, errno);
            let bytes = engine.unicorn.mem_read_as_vec(output, 12).unwrap();
            for (index, value) in values.iter().enumerate() {
                assert_eq!(&bytes[index * 4..index * 4 + 4], &value.to_le_bytes());
            }
            assert!(bytes[values.len() * 4..].iter().all(|b| *b == 0xa5));
        }
        engine.write(input, b"ff\0").unwrap();
        engine.write(format, b"%x\0").unwrap();
        engine
            .write(args, &(DATA_BASE + PAGE_SIZE - 2).to_le_bytes())
            .unwrap();
        engine.write(DATA_BASE + PAGE_SIZE - 2, &[0xa5; 2]).unwrap();
        assert!(
            engine
                .call_win64(entry, [2, input, u64::MAX, format, 0, args])
                .is_err()
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(DATA_BASE + PAGE_SIZE - 2, 2)
                .unwrap(),
            [0xa5; 2]
        );
    }
}

#[test]
fn get_user_name_w_sizes_utf16_buffer_and_preserves_failed_outputs() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(&mut engine.unicorn, entry, "advapi32.dll", "GetUserNameW").unwrap();
    let output = DATA_BASE + 0x100;
    let size = DATA_BASE + 0x800;
    let mut name = aex_host_identity::current_username().unwrap();
    name.push(0);
    let required = name.len() as u32;
    let expected: Vec<u8> = name.into_iter().flat_map(u16::to_le_bytes).collect();
    engine.write(output, &[0xa5; 520]).unwrap();
    for capacity in [0u32, required - 1] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [output, size, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 122);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            required.to_le_bytes()
        );
        assert!(
            engine
                .unicorn
                .mem_read_as_vec(output, 520)
                .unwrap()
                .iter()
                .all(|b| *b == 0xa5)
        );
    }
    for capacity in [required, u32::MAX] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 71;
        engine.unicorn.get_data_mut().crt_errno = 72;
        assert_eq!(
            engine
                .call_win64(entry, [output, size, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(output, expected.len())
                .unwrap(),
            expected
        );
        assert!(
            engine
                .unicorn
                .mem_read_as_vec(output + expected.len() as u64, 520 - expected.len())
                .unwrap()
                .iter()
                .all(|b| *b == 0xa5)
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            required.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    }
    const PAGE: u64 = 0x30_0000_0000;
    engine
        .unicorn
        .mem_map(PAGE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(PAGE, &required.to_le_bytes()).unwrap();
    engine
        .unicorn
        .mem_protect(PAGE, PAGE_SIZE, Prot::READ)
        .unwrap();
    engine.write(output, &[0xa5; 520]).unwrap();
    assert_eq!(
        engine
            .call_win64(entry, [output, PAGE, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 998);
    assert!(
        engine
            .unicorn
            .mem_read_as_vec(output, 520)
            .unwrap()
            .iter()
            .all(|b| *b == 0xa5)
    );
    engine.write(size, &required.to_le_bytes()).unwrap();
    assert_eq!(
        engine.call_win64(entry, [PAGE, size, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 998);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
        required.to_le_bytes()
    );
    assert_eq!(
        engine.call_win64(entry, [output, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        dispatch_win64_import("other.dll", "GetUserNameW"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn get_user_name_a_sizes_ansi_buffer_and_preserves_failed_outputs() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(&mut engine.unicorn, entry, "advapi32.dll", "GetUserNameA").unwrap();
    let output = DATA_BASE + 0x100;
    let size = DATA_BASE + 0x800;
    let mut name = aex_host_identity::current_username().unwrap();
    name.push(0);
    let (expected, substituted) =
        encode_shift_jis_with_default(&String::from_utf16(&name).unwrap(), b'?');
    assert!(!substituted);
    let required = expected.len() as u32;
    engine.write(output, &[0xa5; 520]).unwrap();
    for capacity in [0u32, required - 1] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [output, size, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 122);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            required.to_le_bytes()
        );
        assert!(
            engine
                .unicorn
                .mem_read_as_vec(output, 520)
                .unwrap()
                .iter()
                .all(|b| *b == 0xa5)
        );
    }
    for capacity in [required, u32::MAX] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 71;
        engine.unicorn.get_data_mut().crt_errno = 72;
        assert_eq!(
            engine
                .call_win64(entry, [output, size, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(output, expected.len())
                .unwrap(),
            expected
        );
        assert!(
            engine
                .unicorn
                .mem_read_as_vec(output + expected.len() as u64, 520 - expected.len())
                .unwrap()
                .iter()
                .all(|b| *b == 0xa5)
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            required.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    }
    const PAGE: u64 = 0x30_0000_0000;
    engine
        .unicorn
        .mem_map(PAGE, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    engine.write(PAGE, &required.to_le_bytes()).unwrap();
    engine
        .unicorn
        .mem_protect(PAGE, PAGE_SIZE, Prot::READ)
        .unwrap();
    engine.write(output, &[0xa5; 520]).unwrap();
    assert_eq!(
        engine
            .call_win64(entry, [output, PAGE, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 998);
    assert!(
        engine
            .unicorn
            .mem_read_as_vec(output, 520)
            .unwrap()
            .iter()
            .all(|b| *b == 0xa5)
    );
    engine.write(size, &required.to_le_bytes()).unwrap();
    assert_eq!(
        engine.call_win64(entry, [PAGE, size, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 998);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
        required.to_le_bytes()
    );
    assert_eq!(
        engine.call_win64(entry, [output, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        dispatch_win64_import("other.dll", "GetUserNameA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn gethostname_ordinal_requires_startup_and_returns_actual_host_name() {
    for dll in ["ws2_32.dll", "wsock32.dll"] {
        for symbol in ["gethostname", "ORDINAL 57"] {
            let mut engine = test_engine(&[0xc3]);
            let (entry, startup, cleanup, error) = (
                STUB_BASE + 0x100,
                STUB_BASE + 0x110,
                STUB_BASE + 0x120,
                STUB_BASE + 0x130,
            );
            install_win64_import(&mut engine.unicorn, entry, dll, symbol).unwrap();
            install_win64_import(&mut engine.unicorn, startup, dll, "WSAStartup").unwrap();
            install_win64_import(&mut engine.unicorn, cleanup, dll, "WSACleanup").unwrap();
            install_win64_import(&mut engine.unicorn, error, dll, "ORDINAL 111").unwrap();
            let output = DATA_BASE + 0x400;
            let mut expected = aex_host_identity::current_hostname().unwrap();
            expected.push(0);
            engine
                .write(output, &vec![0xa5; expected.len() + 4])
                .unwrap();
            assert_eq!(
                engine
                    .call_win64(entry, [output, 4096, 0, 0, 0, 0])
                    .unwrap(),
                u32::MAX as u64
            );
            assert_eq!(
                engine.call_win64(error, [0; 6]).unwrap(),
                WINDOWS_WSANOTINITIALISED as u64
            );
            assert_eq!(
                engine
                    .call_win64(startup, [0x202, DATA_BASE, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            for capacity in [0, expected.len() as u64 - 1, u64::MAX] {
                assert_eq!(
                    engine
                        .call_win64(entry, [output, capacity, 0, 0, 0, 0])
                        .unwrap(),
                    u32::MAX as u64
                );
                assert_eq!(
                    engine.call_win64(error, [0; 6]).unwrap(),
                    WINDOWS_WSAEFAULT as u64
                );
                assert!(
                    engine
                        .unicorn
                        .mem_read_as_vec(output, expected.len() + 4)
                        .unwrap()
                        .iter()
                        .all(|b| *b == 0xa5)
                );
            }
            engine.unicorn.get_data_mut().windows_last_error = 71;
            engine.unicorn.get_data_mut().crt_errno = 72;
            assert_eq!(
                engine
                    .call_win64(
                        entry,
                        [
                            output,
                            0x1234_0000_0000_0000 | expected.len() as u64,
                            0,
                            0,
                            0,
                            0
                        ]
                    )
                    .unwrap(),
                0
            );
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(output, expected.len())
                    .unwrap(),
                expected
            );
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(output + expected.len() as u64, 4)
                    .unwrap(),
                [0xa5; 4]
            );
            assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
            assert_eq!(
                engine
                    .call_win64(entry, [output, 4096, 0, 0, 0, 0])
                    .unwrap(),
                u32::MAX as u64
            );
            assert_eq!(
                engine.call_win64(error, [0; 6]).unwrap(),
                WINDOWS_WSAEFAULT as u64
            );
            assert_eq!(engine.call_win64(cleanup, [0; 6]).unwrap(), 0);
            assert_eq!(
                engine
                    .call_win64(entry, [output, 4096, 0, 0, 0, 0])
                    .unwrap(),
                u32::MAX as u64
            );
            assert_eq!(
                engine.call_win64(error, [0; 6]).unwrap(),
                WINDOWS_WSANOTINITIALISED as u64
            );
        }
    }
    for symbol in [
        "gethostname",
        "ORDINAL 57",
        "WSAGetLastError",
        "ORDINAL 111",
    ] {
        assert_eq!(
            dispatch_win64_import("other.dll", symbol),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
    }
}

#[test]
fn wgetenv_preserves_borrowed_values_and_tracks_guest_environment() {
    const WGETENV: u64 = STUB_BASE + 0x410;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-environment-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, WGETENV, dll, "_wgetenv").unwrap();
        let name = DATA_BASE + 0xa80;
        let wide = |s: &str| {
            s.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        engine.write(name, &wide("opencv_for_threads_num")).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        engine.unicorn.get_data_mut().windows_last_error = 71;
        let first = engine.call_win64(WGETENV, [name, 0, 0, 0, 0, 0]).unwrap();
        assert_eq!(engine.unicorn.mem_read_as_vec(first, 4).unwrap(), wide("1"));
        assert!(!guest_range_has_permission(&engine.unicorn, first, 4, Prot::EXEC).unwrap());
        engine
            .unicorn
            .get_data_mut()
            .environment_overrides
            .insert(b"SECOND".to_vec(), Some(vec![b'z'; 4095]));
        engine.write(name, &wide("second")).unwrap();
        let second = engine.call_win64(WGETENV, [name, 0, 0, 0, 0, 0]).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(second, 8192).unwrap(),
            wide(&"z".repeat(4095))
        );
        assert_eq!(engine.unicorn.mem_read_as_vec(first, 4).unwrap(), wide("1"));
        engine
            .unicorn
            .get_data_mut()
            .environment_overrides
            .insert(b"SECOND".to_vec(), Some(b"new".to_vec()));
        assert_eq!(
            engine.call_win64(WGETENV, [name, 0, 0, 0, 0, 0]).unwrap(),
            second
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(second, 8).unwrap(),
            wide("new")
        );
        engine
            .unicorn
            .get_data_mut()
            .environment_overrides
            .insert(b"SECOND".to_vec(), None);
        assert_eq!(
            engine.call_win64(WGETENV, [name, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
        engine.write(name, &wide("HOME")).unwrap();
        assert_eq!(
            engine.call_win64(WGETENV, [name, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        let (_, snapshot_end) = environment_strings_range(engine.unicorn.get_data_mut()).unwrap();
        assert!(first >= snapshot_end + PAGE_SIZE);
        let narrow = guest_getenv_buffer(&mut engine.unicorn).unwrap();
        assert_eq!(narrow, snapshot_end);
        assert!(
            second + PAGE_SIZE * 2
                <= engine.unicorn.get_data().environment_strings_base
                    + ENVIRONMENT_STRINGS_NAMESPACE_SIZE
        );
        engine.write(name, &wide("OPENCV_FOR_THREADS_NUM")).unwrap();
        engine
            .unicorn
            .mem_protect(first, PAGE_SIZE * 2, Prot::READ)
            .unwrap();
        assert!(
            engine
                .call_win64(WGETENV, [name, 0, 0, 0, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("not writable")
        );
    }
}

#[test]
fn wgetenv_rejects_invalid_names_and_storage_exhaustion() {
    const WGETENV: u64 = STUB_BASE + 0x410;
    for case in 0..4 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, WGETENV, "ucrtbase.dll", "_wgetenv").unwrap();
        let name = DATA_BASE + 0xa80;
        let pointer = match case {
            0 => 0,
            1 => {
                engine.write(name, &[0, 0xd8, 0, 0]).unwrap();
                name
            }
            2 => u64::MAX,
            _ => {
                engine
                    .write(
                        name,
                        &"OPENCV_FOR_THREADS_NUM"
                            .encode_utf16()
                            .chain([0])
                            .flat_map(u16::to_le_bytes)
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                for i in 0..MAX_GUEST_WGETENV_BUFFERS {
                    engine
                        .unicorn
                        .get_data_mut()
                        .wgetenv_buffers
                        .insert(i.to_string().into_bytes(), 0);
                }
                name
            }
        };
        let error = engine
            .call_win64(WGETENV, [pointer, 0, 0, 0, 0, 0])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(
                [
                    "invalid parameter handler",
                    "invalid UTF-16",
                    "not readable",
                    "storage exhausted"
                ][case]
            ),
            "{error}"
        );
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_wgetenv"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn special_folder_creation_is_session_local_and_visible_to_file_operations() {
    const GET: u64 = STUB_BASE + 0x410;
    const FIND: u64 = STUB_BASE + 0x420;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET,
        "shell32.dll",
        "SHGetSpecialFolderPathA",
    )
    .unwrap();
    install_win64_import(&mut engine.unicorn, FIND, "kernel32.dll", "FindFirstFileA").unwrap();
    let output = DATA_BASE + 0x900;
    engine.write(output, &[0xa5; 260]).unwrap();
    engine.unicorn.get_data_mut().crt_errno = 72;
    engine.unicorn.get_data_mut().windows_last_error = 71;
    assert_eq!(
        engine.call_win64(GET, [0, output, 0x23, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 260).unwrap(),
        vec![0xa5; 260]
    );
    assert_eq!(
        engine.call_win64(GET, [0, output, 0x23, 1, 0, 0]).unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 16).unwrap(),
        b"C:\\ProgramData\0\xa5"
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .guest_files
            .directory_exists("c:/programdata")
    );
    assert_eq!(
        engine
            .call_win64(GET, [0, output, (1 << 32) | 0x23, 0, 0, 0])
            .unwrap(),
        1
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    let records = guest_find_records(&engine.unicorn.get_data().guest_files, "C:/*").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(&records[0][..4], &0x10u32.to_le_bytes());
    assert_eq!(&records[0][44..56], b"programdata\0");
    assert_eq!(
        open_guest_stream(&mut engine.unicorn, b"C:\\ProgramData", b"r").unwrap(),
        (0, 13)
    );
    let query = DATA_BASE + 0xb00;
    engine.write(query, b"C:\\ProgramData\\*\0").unwrap();
    assert_eq!(
        engine
            .call_win64(FIND, [query, output, 0, 0, 0, 0])
            .unwrap(),
        u64::MAX
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 2); // Exists but empty.
    assert!(
        test_engine(&[0xc3])
            .unicorn
            .get_data()
            .guest_files
            .directories
            .is_empty()
    );
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "SHGetSpecialFolderPathA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn special_folder_creation_preflights_output_and_file_collisions() {
    const GET: u64 = STUB_BASE + 0x410;
    for case in 0..3 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            GET,
            "shell32.dll",
            "SHGetSpecialFolderPathA",
        )
        .unwrap();
        let output = if case == 0 { 0 } else { DATA_BASE + 0x900 };
        if case == 1 {
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        if case == 2 {
            engine
                .unicorn
                .get_data_mut()
                .guest_files
                .sources
                .insert("c:/programdata".into(), std::path::PathBuf::from("unused"));
        }
        assert_eq!(
            engine.call_win64(GET, [0, output, 0x23, 1, 0, 0]).unwrap(),
            0
        );
        assert!(engine.unicorn.get_data().guest_files.directories.is_empty());
    }
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        GET,
        "shell32.dll",
        "SHGetSpecialFolderPathA",
    )
    .unwrap();
    engine.unicorn.get_data_mut().guest_files.sources.insert(
        "c:/program files/mounted.bin".into(),
        std::path::PathBuf::from("unused"),
    );
    assert_eq!(
        engine
            .call_win64(GET, [0, DATA_BASE + 0x900, 0x26, 0, 0, 0])
            .unwrap(),
        1
    );
    assert!(engine.unicorn.get_data().guest_files.directories.is_empty());
    assert!(
        engine
            .call_win64(GET, [0, DATA_BASE + 0x900, 0x1a, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("CSIDL 0x1a")
    );
}

#[test]
fn well_known_world_sid_uses_caller_buffer_and_reports_required_size() {
    const CREATE: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CREATE,
        "advapi32.dll",
        "CreateWellKnownSid",
    )
    .unwrap();
    let output = DATA_BASE + 0x900;
    let size = DATA_BASE + 0xb00;
    engine.write(output, &[0xa5; 68]).unwrap();
    for capacity in [0u32, 11] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(CREATE, [1, 0, output, size, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            12u32.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 122);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 68).unwrap(),
            vec![0xa5; 68]
        );
    }
    engine.write(size, &0u32.to_le_bytes()).unwrap();
    assert_eq!(engine.call_win64(CREATE, [1, 0, 0, size, 0, 0]).unwrap(), 0);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
        12u32.to_le_bytes()
    );
    for capacity in [12u32, 68] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        assert_eq!(engine.call_win64(CREATE, [1, 0, 0, size, 0, 0]).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 87);
    }
    for capacity in [12u32, 68, u32::MAX] {
        engine.write(size, &capacity.to_le_bytes()).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 71;
        engine.unicorn.get_data_mut().crt_errno = 72;
        assert_eq!(
            engine
                .call_win64(CREATE, [(1 << 32) | 1, u64::MAX, output, size, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 12).unwrap(),
            [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output + 12, 56).unwrap(),
            vec![0xa5; 56]
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            12u32.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        assert!(engine.unicorn.get_data().windows_sids.is_empty());
        assert_eq!(engine.unicorn.get_data().windows_sid_issued, 0);
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "CreateWellKnownSid"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn well_known_sid_rejects_unwritable_buffers_and_unimplemented_types() {
    const CREATE: u64 = STUB_BASE + 0x410;
    for case in 0..3 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            CREATE,
            "advapi32.dll",
            "CreateWellKnownSid",
        )
        .unwrap();
        let size = DATA_BASE + 0x900;
        engine.write(size, &68u32.to_le_bytes()).unwrap();
        let output = if case == 0 {
            u64::MAX
        } else {
            DATA_BASE + 0xb00
        };
        if case == 1 {
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        let kind = if case == 2 { 999 } else { 1 };
        assert!(
            engine
                .call_win64(CREATE, [kind, 0, output, size, 0, 0])
                .is_err()
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(size, 4).unwrap(),
            68u32.to_le_bytes()
        );
        assert!(engine.unicorn.get_data().windows_sids.is_empty());
    }
}

#[test]
fn initialize_acl_writes_only_empty_header_and_retains_caller_ownership() {
    const INIT: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, INIT, "advapi32.dll", "InitializeAcl").unwrap();
    let output = DATA_BASE + 0x900;
    for revision in [2u64, 3, 4] {
        for capacity in [8u64, 9, 68, 65535] {
            engine.write(output, &[0xa5; 16]).unwrap();
            engine.unicorn.get_data_mut().windows_last_error = 71;
            engine.unicorn.get_data_mut().crt_errno = 72;
            assert_eq!(
                engine
                    .call_win64(
                        INIT,
                        [output, (1 << 32) | capacity, (1 << 32) | revision, 0, 0, 0]
                    )
                    .unwrap(),
                1
            );
            let mut expected = vec![revision as u8, 0];
            expected.extend_from_slice(&(capacity as u16).to_le_bytes());
            expected.extend_from_slice(&[0; 4]);
            expected.extend_from_slice(&[0xa5; 8]);
            assert_eq!(
                engine.unicorn.mem_read_as_vec(output, 16).unwrap(),
                expected
            );
            assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            assert!(engine.unicorn.get_data().windows_acl_allocations.is_empty());
        }
    }
    for (size, rev, error) in [
        (0, 2, 122),
        (7, 2, 122),
        (65536, 2, 87),
        (8, 1, 87),
        (8, 5, 87),
    ] {
        engine.write(output, &[0xa5; 16]).unwrap();
        assert_eq!(
            engine
                .call_win64(INIT, [output, size, rev, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, error);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 16).unwrap(),
            vec![0xa5; 16]
        );
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "InitializeAcl"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn initialize_acl_rejects_inaccessible_headers_without_partial_writes() {
    const INIT: u64 = STUB_BASE + 0x410;
    for case in 0..3 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, INIT, "advapi32.dll", "InitializeAcl").unwrap();
        let output = if case == 0 { 0 } else { DATA_BASE + 0x900 };
        engine.write(DATA_BASE + 0x900, &[0xa5; 8]).unwrap();
        if case == 1 {
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        let pointer = if case == 2 { u64::MAX - 3 } else { output };
        assert!(
            engine
                .call_win64(INIT, [pointer, 8, 2, 0, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("not writable")
        );
        assert_eq!(
            engine
                .unicorn
                .mem_read_as_vec(DATA_BASE + 0x900, 8)
                .unwrap(),
            vec![0xa5; 8]
        );
    }
}

#[test]
fn allowed_ace_append_preserves_existing_entries_and_capacity() {
    const ADD: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        ADD,
        "advapi32.dll",
        "AddAccessAllowedAceEx",
    )
    .unwrap();
    let acl = DATA_BASE + 0x900;
    let sid = DATA_BASE + 0xb00;
    engine.write(acl, &[0xa5; 64]).unwrap();
    engine.write(acl, &[2, 0, 48, 0, 0, 0, 0, 0]).unwrap();
    let world = [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    engine.write(sid, &world).unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 71;
    engine.unicorn.get_data_mut().crt_errno = 72;
    assert_eq!(
        engine
            .call_win64(ADD, [acl, 2, 3, 0x10000000, sid, 0])
            .unwrap(),
        1
    );
    let first = engine.unicorn.mem_read_as_vec(acl + 8, 20).unwrap();
    let mut expected = vec![0, 3, 20, 0, 0, 0, 0, 0x10];
    expected.extend_from_slice(&world);
    assert_eq!(first, expected);
    assert_eq!(
        engine
            .call_win64(ADD, [acl, (1 << 32) | 4, 0x110, 0x12345678, sid, 0])
            .unwrap(),
        1
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(acl, 8).unwrap(),
        [4, 0, 48, 0, 2, 0, 0, 0]
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(acl + 8, 20).unwrap(), first);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(acl + 28, 8).unwrap(),
        [0, 0x10, 20, 0, 0x78, 0x56, 0x34, 0x12]
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(acl + 48, 16).unwrap(),
        vec![0xa5; 16]
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    let full = engine.unicorn.mem_read_as_vec(acl, 64).unwrap();
    assert_eq!(engine.call_win64(ADD, [acl, 2, 3, 0, sid, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 1344);
    assert_eq!(engine.unicorn.mem_read_as_vec(acl, 64).unwrap(), full);
    assert!(engine.unicorn.get_data().windows_acl_allocations.is_empty());
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "AddAccessAllowedAceEx"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn allowed_ace_failures_leave_acl_unchanged() {
    const ADD: u64 = STUB_BASE + 0x410;
    for case in 0..5 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            ADD,
            "advapi32.dll",
            "AddAccessAllowedAceEx",
        )
        .unwrap();
        let acl = DATA_BASE + 0x900;
        let sid = DATA_BASE + 0xb00;
        engine.write(acl, &[0xa5; 64]).unwrap();
        engine.write(acl, &[2, 0, 48, 0, 0, 0, 0, 0]).unwrap();
        engine
            .write(sid, &[1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0])
            .unwrap();
        if case == 0 {
            engine.write(sid, &[2]).unwrap();
        }
        if case == 1 {
            engine.write(acl + 4, &1u16.to_le_bytes()).unwrap();
            engine.write(acl + 8, &[0, 0, 60, 0]).unwrap();
        }
        if case == 3 {
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        let before = engine.unicorn.mem_read_as_vec(acl, 64).unwrap();
        let result = engine.call_win64(
            ADD,
            [
                acl,
                if case == 2 { 5 } else { 2 },
                3,
                1,
                if case == 4 { 0 } else { sid },
                0,
            ],
        );
        if case == 3 {
            assert!(result.unwrap_err().to_string().contains("not writable"));
        } else {
            assert_eq!(result.unwrap(), 0);
            assert_eq!(
                engine.unicorn.get_data().windows_last_error,
                [1337, 1336, 1306, 0, 1337][case]
            );
        }
        assert_eq!(engine.unicorn.mem_read_as_vec(acl, 64).unwrap(), before);
    }
}

#[test]
fn create_directory_tracks_parents_duplicates_and_acl_snapshot() {
    const CREATE: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CREATE,
        "kernel32.dll",
        "CreateDirectoryA",
    )
    .unwrap();
    let path = DATA_BASE + 0x900;
    let sa = DATA_BASE + 0xb00;
    let sd = sa + 32;
    let acl = sd + 48;
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .directories
        .insert("c:/programdata".into());
    engine.write(path, b"C:\\ProgramData\\BorisFX\0").unwrap();
    let mut attrs = [0u8; 24];
    attrs[..4].copy_from_slice(&24u32.to_le_bytes());
    attrs[8..16].copy_from_slice(&sd.to_le_bytes());
    engine.write(sa, &attrs).unwrap();
    let mut desc = [0u8; 40];
    desc[0] = 1;
    desc[2] = 4;
    desc[32..40].copy_from_slice(&acl.to_le_bytes());
    engine.write(sd, &desc).unwrap();
    let policy = [
        2, 0, 28, 0, 1, 0, 0, 0, 0, 3, 20, 0, 0, 0, 0, 0x10, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
    ];
    engine.write(acl, &policy).unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 71;
    engine.unicorn.get_data_mut().crt_errno = 72;
    assert_eq!(
        engine.call_win64(CREATE, [path, sa, 0, 0, 0, 0]).unwrap(),
        1
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    engine.write(acl, &[0; 28]).unwrap();
    assert_eq!(
        engine.unicorn.get_data().guest_files.directory_dacls["c:/programdata/borisfx"],
        policy
    );
    assert_eq!(
        engine.call_win64(CREATE, [path, sa, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 183);
    engine
        .write(path, b"C:\\ProgramData\\BorisFX\\Child\0")
        .unwrap();
    assert_eq!(engine.call_win64(CREATE, [path, 0, 0, 0, 0, 0]).unwrap(), 1);
    assert!(
        engine
            .unicorn
            .get_data()
            .guest_files
            .directory_dacls
            .contains_key("c:/programdata/borisfx/child")
    );
    assert_eq!(
        engine.unicorn.get_data().guest_files.directory_dacls["c:/programdata/borisfx"][9],
        3
    );
    assert_eq!(
        engine.unicorn.get_data().guest_files.directory_dacls["c:/programdata/borisfx/child"][9],
        0x13
    );
    engine
        .write(path, b"C:\\ProgramData\\BorisFX\\Child\\Grandchild\0")
        .unwrap();
    assert_eq!(engine.call_win64(CREATE, [path, 0, 0, 0, 0, 0]).unwrap(), 1);
    assert_eq!(
        engine.unicorn.get_data().guest_files.directory_dacls["c:/programdata/borisfx/child/grandchild"]
            [9],
        0x13
    );
    let records =
        guest_find_records(&engine.unicorn.get_data().guest_files, "c:/programdata/*").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(&records[0][..4], &16u32.to_le_bytes());
}

#[test]
fn create_directory_failures_do_not_publish_objects() {
    const CREATE: u64 = STUB_BASE + 0x410;
    for case in 0..4 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            CREATE,
            "kernelbase.dll",
            "CreateDirectoryA",
        )
        .unwrap();
        let path = DATA_BASE + 0x900;
        let sa = DATA_BASE + 0xb00;
        engine.write(path, b"C:\\ProgramData\\New\0").unwrap();
        if case != 0 {
            engine
                .unicorn
                .get_data_mut()
                .guest_files
                .directories
                .insert("c:/programdata".into());
        }
        if case == 1 {
            engine.write(sa, &[0; 24]).unwrap();
        }
        if case == 2 {
            for i in 0..MAX_GUEST_DIRECTORIES {
                engine
                    .unicorn
                    .get_data_mut()
                    .guest_files
                    .directories
                    .insert(format!("c:/other/{i}"));
            }
        }
        if case == 3 {
            engine
                .unicorn
                .get_data_mut()
                .guest_files
                .directories
                .clear();
            engine.unicorn.get_data_mut().guest_files.sources.insert(
                "c:/programdata/mounted".into(),
                std::path::PathBuf::from("unused"),
            );
        }
        assert_eq!(
            engine
                .call_win64(CREATE, [path, if case == 1 { sa } else { 0 }, 0, 0, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.get_data().windows_last_error,
            [3, 87, 8, 5][case]
        );
        assert!(
            !engine
                .unicorn
                .get_data()
                .guest_files
                .directories
                .contains("c:/programdata/new")
        );
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "CreateDirectoryA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn wstat64i32_reports_guest_directory_and_mounted_file_metadata() {
    const STAT: u64 = STUB_BASE + 0x410;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-filesystem-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, STAT, dll, "_wstat64i32").unwrap();
        let name = "c:/programdata/test";
        let files = &mut engine.unicorn.get_data_mut().guest_files;
        files.record_directory_creation(name);
        files.directories.insert(name.into());
        let expected_time = files.directory_times[name][0]
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let path = DATA_BASE + 0x100;
        let output = DATA_BASE + 0x900;
        let wide = |s: &str| {
            s.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        engine
            .write(path, &wide("C:\\ProgramData\\test\\"))
            .unwrap();
        engine.write(output, &[0xa5; 56]).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        assert_eq!(
            engine.call_win64(STAT, [path, output, 0, 0, 0, 0]).unwrap(),
            0
        );
        let result = engine.unicorn.mem_read_as_vec(output, 56).unwrap();
        assert_eq!(&result[..4], &2u32.to_le_bytes());
        assert_eq!(&result[6..8], &0x41ffu16.to_le_bytes());
        assert_eq!(&result[8..10], &1u16.to_le_bytes());
        assert_eq!(&result[16..20], &2u32.to_le_bytes());
        assert_eq!(&result[20..24], &[0; 4]);
        assert_eq!(&result[24..32], &expected_time.to_le_bytes());
        assert_eq!(&result[48..], &[0xa5; 8]);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        engine.write(path, &wide("c:/missing")).unwrap();
        assert_eq!(
            engine.call_win64(STAT, [path, output, 0, 0, 0, 0]).unwrap(),
            u32::MAX as u64
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 2);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 48).unwrap(),
            vec![0; 48]
        );

        let source =
            std::env::temp_dir().join(format!("aex-stat-{}-{}.bin", std::process::id(), dll));
        std::fs::write(&source, b"stat-content").unwrap();
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .sources
            .insert("c:/asset.exe".into(), source.clone());
        engine.write(path, &wide("c:/asset.exe")).unwrap();
        assert_eq!(
            engine.call_win64(STAT, [path, output, 0, 0, 0, 0]).unwrap(),
            0
        );
        let result = engine.unicorn.mem_read_as_vec(output, 48).unwrap();
        assert_eq!(&result[6..8], &0x816du16.to_le_bytes());
        assert_eq!(&result[20..24], &12i32.to_le_bytes());
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap();
        file.set_len(i32::MAX as u64 + 1).unwrap();
        assert_eq!(
            engine.call_win64(STAT, [path, output, 0, 0, 0, 0]).unwrap(),
            u32::MAX as u64
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 132);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 48).unwrap(),
            vec![0; 48]
        );
        drop(file);
        std::fs::remove_file(source).unwrap();
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_wstat64i32"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn wstat64i32_preflights_output_and_rejects_invalid_wide_paths() {
    const STAT: u64 = STUB_BASE + 0x410;
    for case in 0..4 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, STAT, "ucrtbase.dll", "_wstat64i32").unwrap();
        let path = DATA_BASE + 0x100;
        let output = DATA_BASE + 0x900;
        engine.write(path, &[0x00, 0xd8, 0, 0]).unwrap();
        engine.write(output, &[0xa5; 48]).unwrap();
        if case == 0 {
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        let actual_path = if case == 2 {
            0
        } else if case == 3 {
            u64::MAX - 1
        } else {
            path
        };
        assert!(
            engine
                .call_win64(STAT, [actual_path, output, 0, 0, 0, 0])
                .is_err()
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 48).unwrap(),
            vec![0xa5; 48]
        );
    }
}

#[test]
fn strrchr_returns_last_match_and_includes_terminator() {
    const CALL: u64 = STUB_BASE + 0x410;
    for dll in [
        "vcruntime140.dll",
        "ucrtbase.dll",
        "api-ms-win-crt-string-l1-1-0.dll",
    ] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CALL, dll, "strrchr").unwrap();
        let source = DATA_BASE + PAGE_SIZE - 7;
        engine.write(source, b"abac\xffa\0").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        for (needle, expected) in [
            (b'a' as u64, source + 5),
            (0, source + 6),
            (0x1ff, source + 4),
            (b'z' as u64, 0),
        ] {
            assert_eq!(
                engine
                    .call_win64(CALL, [source, needle, 0, 0, 0, 0])
                    .unwrap(),
                expected
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        }
        assert_eq!(
            engine.unicorn.mem_read_as_vec(source, 7).unwrap(),
            b"abac\xffa\0"
        );
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "strrchr"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn strrchr_requires_terminator_even_after_match_and_refreshes_permissions() {
    const CALL: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, CALL, "vcruntime140.dll", "strrchr").unwrap();
    let source = DATA_BASE + PAGE_SIZE - 1;
    engine.write(source, &[0]).unwrap();
    assert_eq!(
        engine.call_win64(CALL, [source, 0, 0, 0, 0, 0]).unwrap(),
        source
    );
    engine.write(source, b"a").unwrap();
    // Make the following page unavailable even if the fixture maps it.
    if guest_range_has_permission(&engine.unicorn, source + 1, 1, Prot::READ).unwrap() {
        engine
            .unicorn
            .mem_protect(source + 1, PAGE_SIZE, Prot::NONE)
            .unwrap();
    }
    assert!(
        engine
            .call_win64(CALL, [source, b'a' as u64, 0, 0, 0, 0])
            .is_err()
    );
}

#[test]
fn fullpath_resolves_paths_and_owns_allocated_buffers() {
    const CALL: u64 = STUB_BASE + 0x410;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-filesystem-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CALL, dll, "_fullpath").unwrap();
        let input = DATA_BASE + 0x100;
        let output = DATA_BASE + 0x900;
        for (source, expected) in [
            ("C:/alpha/../beta/./file", "C:\\beta\\file"),
            ("alpha/../file", "C:\\file"),
            ("\\root\\file", "C:\\root\\file"),
            ("c:relative", "c:\\relative"),
            ("D:/../../file", "D:\\file"),
            ("C:/folder/", "C:\\folder\\"),
            ("", "C:\\"),
        ] {
            engine
                .write(input, format!("{source}\0").as_bytes())
                .unwrap();
            engine.write(output, &[0xa5; 128]).unwrap();
            engine.unicorn.get_data_mut().crt_errno = 72;
            let count = expected.len() as u64 + 1;
            assert_eq!(
                engine
                    .call_win64(CALL, [output, input, count, 0, 0, 0])
                    .unwrap(),
                output
            );
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(output, count as usize)
                    .unwrap(),
                format!("{expected}\0").as_bytes()
            );
            assert_eq!(
                engine.unicorn.mem_read_as_vec(output + count, 1).unwrap(),
                [0xa5]
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            let allocated = engine.call_win64(CALL, [0, input, 0, 0, 0, 0]).unwrap();
            assert_ne!(allocated, 0);
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(allocated, count as usize)
                    .unwrap(),
                format!("{expected}\0").as_bytes()
            );
            free_crt_region(&mut engine.unicorn, allocated).unwrap();
            assert!(free_crt_region(&mut engine.unicorn, allocated).is_err());
        }
        assert_eq!(
            engine.call_win64(CALL, [output, 0, 128, 0, 0, 0]).unwrap(),
            output
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
            b"C:\\\0"
        );
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_fullpath"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn fullpath_preflights_capacity_and_rejects_unmodeled_namespaces() {
    const CALL: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, CALL, "ucrtbase.dll", "_fullpath").unwrap();
    let input = DATA_BASE + 0x100;
    let output = DATA_BASE + 0x900;
    engine.write(input, b"C:/folder/file\0").unwrap();
    engine.write(output, &[0xa5; 32]).unwrap();
    assert_eq!(
        engine
            .call_win64(CALL, [output, input, 3, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 34);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 32).unwrap(),
        [0xa5; 32]
    );
    engine
        .unicorn
        .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
        .unwrap();
    assert!(
        engine
            .call_win64(CALL, [output, input, 32, 0, 0, 0])
            .is_err()
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 32).unwrap(),
        [0xa5; 32]
    );
    for path in [
        b"\\\\server\\share".as_slice(),
        b"D:relative",
        b"C:/NUL",
        b"C:/trailing.",
        &[0xff],
    ] {
        assert!(canonical_guest_fullpath(path).is_err());
    }
}

#[test]
fn wfopen_reads_and_closes_owned_streams_and_preserves_readonly_namespace() {
    const OPEN: u64 = STUB_BASE + 0x410;
    const GET: u64 = STUB_BASE + 0x420;
    const CLOSE: u64 = STUB_BASE + 0x430;
    let wide = |s: &str| {
        s.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    for dll in ["ucrtbase.dll", "api-ms-win-crt-stdio-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        for (address, symbol) in [(OPEN, "_wfopen"), (GET, "fgetc"), (CLOSE, "fclose")] {
            install_win64_import(&mut engine.unicorn, address, dll, symbol).unwrap();
        }
        let source = std::env::temp_dir().join(format!("aex-wfopen-{}-{dll}", std::process::id()));
        std::fs::write(&source, b"A\r\nB").unwrap();
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .sources
            .insert("c:/wide.txt".into(), source.clone());
        let name = DATA_BASE + 0x100;
        let mode = DATA_BASE + 0x300;
        engine.write(name, &wide("C:\\Wide.txt")).unwrap();
        engine.write(mode, &wide("rt")).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        let token = engine.call_win64(OPEN, [name, mode, 0, 0, 0, 0]).unwrap();
        assert_ne!(token, 0);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        for byte in [65, 10, 66, u32::MAX as u64] {
            assert_eq!(
                engine.call_win64(GET, [token, 0, 0, 0, 0, 0]).unwrap(),
                byte
            );
        }
        assert_eq!(engine.call_win64(CLOSE, [token, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert!(engine.unicorn.get_data().guest_files.streams.is_empty());
        assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 0);
        engine.write(mode, &wide("wb")).unwrap();
        assert_eq!(
            engine.call_win64(OPEN, [name, mode, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 13);
        assert_eq!(std::fs::read(&source).unwrap(), b"A\r\nB");
        engine.write(name, &wide("c:/absent")).unwrap();
        engine.write(mode, &wide("rb")).unwrap();
        assert_eq!(
            engine.call_win64(OPEN, [name, mode, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 2);
        std::fs::remove_file(source).unwrap();
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_wfopen"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn wfopen_rejects_unreadable_and_malformed_wide_strings_before_opening() {
    const OPEN: u64 = STUB_BASE + 0x410;
    for case in 0..3 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, OPEN, "ucrtbase.dll", "_wfopen").unwrap();
        let name = DATA_BASE + 0x100;
        let mode = DATA_BASE + 0x300;
        engine.write(name, &[0, 0xd8, 0, 0]).unwrap();
        engine.write(mode, &[b'r', 0, 0, 0]).unwrap();
        let name = if case == 0 {
            name
        } else if case == 1 {
            0
        } else {
            u64::MAX - 1
        };
        assert!(engine.call_win64(OPEN, [name, mode, 0, 0, 0, 0]).is_err());
        assert!(engine.unicorn.get_data().guest_files.streams.is_empty());
    }
}

#[test]
fn strerror_maps_windows_errors_and_keeps_thread_borrowed_storage() {
    const CALL: u64 = STUB_BASE + 0x410;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-runtime-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CALL, dll, "strerror").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        engine.unicorn.get_data_mut().windows_last_error = 71;
        let mut first = 0;
        for (error, expected) in [
            (0, "No error"),
            (2, "No such file or directory"),
            (13, "Permission denied"),
            (34, "Result too large"),
            (132, "value too large"),
            (u32::MAX as u64, "Unknown error"),
            (99, "Unknown error"),
        ] {
            let pointer = engine
                .call_win64(CALL, [error | (1 << 32), 0, 0, 0, 0, 0])
                .unwrap();
            if first == 0 {
                first = pointer;
            }
            assert_eq!(pointer, first);
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(pointer, expected.len() + 1)
                    .unwrap(),
                format!("{expected}\0").as_bytes()
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        }
        engine.unicorn.get_data_mut().current_windows_thread_id += 1;
        let second = engine.call_win64(CALL, [2, 0, 0, 0, 0, 0]).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(first, 14).unwrap(),
            b"Unknown error\0"
        );
        assert!(free_crt_region(&mut engine.unicorn, first).is_err());
        engine
            .unicorn
            .mem_protect(second, PAGE_SIZE, Prot::READ)
            .unwrap();
        assert!(engine.call_win64(CALL, [13, 0, 0, 0, 0, 0]).is_err());
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "strerror"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn strncat_limits_reads_and_appends_one_terminator() {
    const CALL: u64 = STUB_BASE + 0x410;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-string-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CALL, dll, "strncat").unwrap();
        let destination = DATA_BASE + 0x100;
        let source = DATA_BASE + PAGE_SIZE - 3;
        engine.write(source, b"xyz").unwrap();
        engine.write(destination, b"ab\0?????").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        assert_eq!(
            engine
                .call_win64(CALL, [destination, source, 3, 0, 0, 0])
                .unwrap(),
            destination
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
            b"abxyz\0??"
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        engine.write(source, b"x\0z").unwrap();
        engine.write(destination, b"ab\0?????").unwrap();
        assert_eq!(
            engine
                .call_win64(CALL, [destination, source, u64::MAX, 0, 0, 0])
                .unwrap(),
            destination
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
            b"abx\0????"
        );
        assert_eq!(
            engine
                .call_win64(CALL, [destination, u64::MAX, 0, 0, 0, 0])
                .unwrap(),
            destination
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
            b"abx\0????"
        );
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "strncat"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn strncat_rejects_overlap_and_unwritable_append_without_partial_writes() {
    const CALL: u64 = STUB_BASE + 0x410;
    for readonly in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CALL, "ucrtbase.dll", "strncat").unwrap();
        let destination = DATA_BASE + 0x100;
        let source = if readonly {
            DATA_BASE + 0x300
        } else {
            destination + 1
        };
        engine.write(destination, b"abc\0????").unwrap();
        if readonly {
            engine.write(source, b"xy\0").unwrap();
            engine
                .unicorn
                .mem_protect(DATA_BASE, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        assert!(
            engine
                .call_win64(CALL, [destination, source, 2, 0, 0, 0])
                .is_err()
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 8).unwrap(),
            b"abc\0????"
        );
    }
}

#[test]
fn strncat_rejects_null_source_even_when_zero_page_is_readable() {
    const CALL: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, CALL, "ucrtbase.dll", "strncat").unwrap();
    if !guest_range_has_permission(&engine.unicorn, 0, 1, Prot::READ).unwrap() {
        engine
            .unicorn
            .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
    }
    let destination = DATA_BASE + 0x100;
    engine.write(destination, b"abc\0").unwrap();
    assert!(
        engine
            .call_win64(CALL, [destination, 0, 1, 0, 0, 0])
            .unwrap_err()
            .to_string()
            .contains("source is null")
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
        b"abc\0"
    );
}

#[test]
fn volume_information_reports_unavailable_metadata_without_fabricated_outputs() {
    const CALL: u64 = STUB_BASE + 0x410;
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CALL, dll, "GetVolumeInformationA").unwrap();
        let root = DATA_BASE + 0x100;
        let output = DATA_BASE + 0x900;
        engine.write(root, b"C:\\\0").unwrap();
        engine.write(output, &[0xa5; 32]).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        for pointer in [root, 0] {
            assert_eq!(
                engine
                    .call_win64(
                        CALL,
                        [pointer, output, 32, output + 8, output + 12, output + 16]
                    )
                    .unwrap(),
                0
            );
            assert_eq!(engine.unicorn.get_data().windows_last_error, 50);
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            assert_eq!(
                engine.unicorn.mem_read_as_vec(output, 32).unwrap(),
                [0xa5; 32]
            );
        }
        engine.write(root, b"Z:\\\0").unwrap();
        assert_eq!(engine.call_win64(CALL, [root, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 15);
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "GetVolumeInformationA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn com_initialization_balances_repeated_calls_and_thread_models() {
    const INIT: u64 = STUB_BASE + 0x410;
    const END: u64 = STUB_BASE + 0x420;
    for dll in ["ole32.dll", "combase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, INIT, dll, "CoInitializeEx").unwrap();
        install_win64_import(&mut engine.unicorn, END, dll, "CoUninitialize").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        engine.unicorn.get_data_mut().windows_last_error = 71;
        assert_eq!(engine.call_win64(INIT, [0, 0, 0, 0, 0, 0]).unwrap(), 0);
        assert_eq!(
            engine
                .call_win64(INIT, [0, (1 << 32) | 12, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.call_win64(INIT, [0, 2, 0, 0, 0, 0]).unwrap(),
            0x80010106
        );
        engine.call_win64(END, [0; 6]).unwrap();
        assert_eq!(
            engine.call_win64(INIT, [0, 2, 0, 0, 0, 0]).unwrap(),
            0x80010106
        );
        engine.call_win64(END, [0; 6]).unwrap();
        assert!(engine.unicorn.get_data().com_apartments.is_empty());
        assert_eq!(engine.call_win64(INIT, [0, 2, 0, 0, 0, 0]).unwrap(), 0);
        let first = engine.unicorn.get_data().current_windows_thread_id;
        engine.unicorn.get_data_mut().current_windows_thread_id = first + 1;
        assert_eq!(engine.call_win64(INIT, [0, 0, 0, 0, 0, 0]).unwrap(), 0);
        engine.call_win64(END, [0; 6]).unwrap();
        engine.call_win64(END, [0; 6]).unwrap();
        assert_eq!(engine.unicorn.get_data().com_apartments.len(), 1);
        engine.unicorn.get_data_mut().current_windows_thread_id = first;
        engine.call_win64(END, [0; 6]).unwrap();
        assert!(engine.unicorn.get_data().com_apartments.is_empty());
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    }
}

#[test]
fn com_initialization_rejects_invalid_arguments_and_bounded_exhaustion() {
    const INIT: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, INIT, "ole32.dll", "CoInitializeEx").unwrap();
    for (reserved, flags) in [(1, 0), (0, 1), (0, 16)] {
        assert_eq!(
            engine
                .call_win64(INIT, [reserved, flags, 0, 0, 0, 0])
                .unwrap(),
            0x80070057
        );
        assert!(engine.unicorn.get_data().com_apartments.is_empty());
    }
    let thread = engine.unicorn.get_data().current_windows_thread_id;
    engine
        .unicorn
        .get_data_mut()
        .com_apartments
        .insert(thread, (0, 1024));
    assert_eq!(engine.call_win64(INIT, [0; 6]).unwrap(), 0x8007000e);
    assert_eq!(engine.unicorn.get_data().com_apartments[&thread], (0, 1024));
    engine.unicorn.get_data_mut().com_apartments.clear();
    for id in 0..4096 {
        engine
            .unicorn
            .get_data_mut()
            .com_apartments
            .insert(id, (0, 1));
    }
    engine.unicorn.get_data_mut().current_windows_thread_id = 5000;
    assert_eq!(engine.call_win64(INIT, [0; 6]).unwrap(), 0x8007000e);
    for name in ["CoInitializeEx", "CoUninitialize"] {
        assert!(matches!(
            dispatch_win64_import("foreign.dll", name),
            Win64ImportDispatch::UnsupportedLegacyImport
        ));
    }
}

#[test]
fn com_security_defaults_are_process_wide_and_survive_apartment_teardown() {
    const SECURITY: u64 = STUB_BASE + 0x410;
    const INIT: u64 = STUB_BASE + 0x420;
    const END: u64 = STUB_BASE + 0x430;
    for dll in ["ole32.dll", "combase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, SECURITY, dll, "CoInitializeSecurity").unwrap();
        install_win64_import(&mut engine.unicorn, INIT, dll, "CoInitializeEx").unwrap();
        install_win64_import(&mut engine.unicorn, END, dll, "CoUninitialize").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        engine.unicorn.get_data_mut().windows_last_error = 71;
        engine.call_win64(INIT, [0; 6]).unwrap();
        let args = [0, u64::MAX, 0, 0, 0, (1 << 32) | 3, 0, 0, 0];
        assert_eq!(
            engine
                .call_win64_with_timeout(SECURITY, &args, TIMEOUT_MICROSECONDS)
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().com_security, Some((-1, 0, 3)));
        engine.call_win64(END, [0; 6]).unwrap();
        assert!(engine.unicorn.get_data().com_apartments.is_empty());
        engine.unicorn.get_data_mut().current_windows_thread_id += 1;
        assert_eq!(
            engine
                .call_win64_with_timeout(SECURITY, &[0; 9], TIMEOUT_MICROSECONDS)
                .unwrap(),
            0x80010119
        );
        assert_eq!(engine.unicorn.get_data().com_security, Some((-1, 0, 3)));
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "CoInitializeSecurity"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn com_security_validates_arguments_without_committing_failed_configuration() {
    const SECURITY: u64 = STUB_BASE + 0x410;
    for (index, value) in [
        (1, 0xffff_fffe),
        (2, 1),
        (3, 1),
        (4, 7),
        (5, 0),
        (5, 5),
        (8, 1),
    ] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            SECURITY,
            "ole32.dll",
            "CoInitializeSecurity",
        )
        .unwrap();
        let mut args = [0, u64::MAX, 0, 0, 0, 3, 0, 0, 0];
        args[index] = value;
        assert_eq!(
            engine
                .call_win64_with_timeout(SECURITY, &args, TIMEOUT_MICROSECONDS)
                .unwrap(),
            0x80070057
        );
        assert_eq!(engine.unicorn.get_data().com_security, None);
        args = [0, 0, 0, 0, 1, 2, 0, 0, 0];
        assert_eq!(
            engine
                .call_win64_with_timeout(SECURITY, &args, TIMEOUT_MICROSECONDS)
                .unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().com_security, Some((0, 1, 2)));
    }
    for (index, value) in [(0, 1), (1, 1), (6, 1), (7, 8)] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            SECURITY,
            "ole32.dll",
            "CoInitializeSecurity",
        )
        .unwrap();
        let mut args = [0, u64::MAX, 0, 0, 0, 3, 0, 0, 0];
        args[index] = value;
        assert!(
            engine
                .call_win64_with_timeout(SECURITY, &args, TIMEOUT_MICROSECONDS)
                .is_err()
        );
        assert_eq!(engine.unicorn.get_data().com_security, None);
    }
}

#[test]
fn com_create_instance_reports_empty_registration_and_clears_output() {
    const CREATE: u64 = STUB_BASE + 0x410;
    const INIT: u64 = STUB_BASE + 0x420;
    const END: u64 = STUB_BASE + 0x430;
    for dll in ["ole32.dll", "combase.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CREATE, dll, "CoCreateInstance").unwrap();
        install_win64_import(&mut engine.unicorn, INIT, dll, "CoInitializeEx").unwrap();
        install_win64_import(&mut engine.unicorn, END, dll, "CoUninitialize").unwrap();
        let class = DATA_BASE + 0x100;
        engine.write(class, &[0x31; 16]).unwrap();
        let iid = DATA_BASE + 0x200;
        engine.write(iid, &[0x42; 16]).unwrap();
        let output = DATA_BASE + 0x300;
        engine.write(output, &[0xa5; 16]).unwrap();
        engine.unicorn.get_data_mut().crt_errno = 72;
        engine.unicorn.get_data_mut().windows_last_error = 71;
        assert_eq!(
            engine
                .call_win64(CREATE, [class, 0, 1, iid, output, 0])
                .unwrap(),
            0x800401f0
        );
        engine.call_win64(INIT, [0, 2, 0, 0, 0, 0]).unwrap();
        for context in [1, 2, 3, 4, 5, 7, (1 << 32) | 1] {
            engine.write(output, &[0xa5; 16]).unwrap();
            assert_eq!(
                engine
                    .call_win64(CREATE, [class, 0, context, iid, output, 0])
                    .unwrap(),
                0x80040154
            );
            let mut actual = [0; 16];
            engine.unicorn.mem_read(output, &mut actual).unwrap();
            assert_eq!(&actual[..8], &[0; 8]);
            assert_eq!(&actual[8..], &[0xa5; 8]);
        }
        engine.call_win64(END, [0; 6]).unwrap();
        assert_eq!(
            engine
                .call_win64(CREATE, [class, 0, 1, iid, output, 0])
                .unwrap(),
            0x800401f0
        );
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "CoCreateInstance"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn com_create_instance_rejects_unsafe_pointers_and_unmodeled_contexts() {
    const CREATE: u64 = STUB_BASE + 0x410;
    for case in 0..7 {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, CREATE, "ole32.dll", "CoCreateInstance").unwrap();
        let guid = DATA_BASE + 0x100;
        engine.write(guid, &[0x31; 16]).unwrap();
        let output = DATA_BASE + 0x300;
        engine.write(output, &[0xa5; 8]).unwrap();
        let mut args = [guid, 0, 1, guid, output, 0];
        match case {
            0 => args[0] = 0,
            1 => args[3] = u64::MAX - 7,
            2 => args[1] = 1,
            3 => args[2] = 16,
            4 => args[2] = 0,
            5 => {
                engine
                    .unicorn
                    .mem_protect(output & !(PAGE_SIZE - 1), PAGE_SIZE, Prot::READ)
                    .unwrap();
            }
            _ => {
                engine
                    .unicorn
                    .get_data_mut()
                    .com_apartments
                    .insert(99999, (0, 1));
            }
        }
        assert!(engine.call_win64(CREATE, args).is_err());
        let mut actual = [0; 8];
        engine.unicorn.mem_read(output, &mut actual).unwrap();
        assert_eq!(actual, [0xa5; 8]);
    }
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, CREATE, "ole32.dll", "CoCreateInstance").unwrap();
    assert_eq!(engine.call_win64(CREATE, [0; 6]).unwrap(), 0x80004003);
}

#[test]
fn exit_thread_never_returns_and_preserves_exit_code_through_fls_cleanup() {
    const EXIT: u64 = STUB_BASE + 0x410;
    const CREATE: u64 = STUB_BASE + 0x420;
    const FLS_ALLOC: u64 = STUB_BASE + 0x430;
    const FLS_SET: u64 = STUB_BASE + 0x440;
    const SWITCH: u64 = STUB_BASE + 0x450;
    const DESTRUCTOR: usize = 0x100;
    for dll in [
        "kernel32.dll",
        "kernelbase.dll",
        "api-ms-win-core-processthreads-l1-1-0.dll",
    ] {
        for yielding in [false, true] {
            let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x31, 0xc9, 0xba, 0x34, 0x12, 0, 0];
            push_mov_imm64(&mut code, [0x48, 0xb8], FLS_SET);
            code.extend_from_slice(&[0xff, 0xd0]);
            if yielding {
                push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
                code.extend_from_slice(&[0xff, 0xd0]);
            }
            push_mov_imm64(&mut code, [0x48, 0xb9], 0xfeed_beef_1234_5678);
            push_mov_imm64(&mut code, [0x48, 0xb8], EXIT);
            code.extend_from_slice(&[0xff, 0xd0, 0x0f, 0x0b]); // UD2 must never execute
            code.resize(DESTRUCTOR, 0x90);
            push_mov_imm64(&mut code, [0x48, 0xb8], DATA_BASE + 0x380);
            code.extend_from_slice(&[0x48, 0x89, 0x08, 0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]);
            let mut engine = test_engine(&code);
            engine
                .unicorn
                .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
                .unwrap();
            install_win64_import(&mut engine.unicorn, EXIT, dll, "ExitThread").unwrap();
            for (address, name) in [
                (CREATE, "CreateThread"),
                (FLS_ALLOC, "FlsAlloc"),
                (FLS_SET, "FlsSetValue"),
                (SWITCH, "SwitchToThread"),
            ] {
                install_win64_import(&mut engine.unicorn, address, "kernel32.dll", name).unwrap();
            }
            assert_eq!(
                engine
                    .call_win64(FLS_ALLOC, [TEST_CODE + DESTRUCTOR as u64, 0, 0, 0, 0, 0])
                    .unwrap(),
                0
            );
            let caller = engine.unicorn.get_data().current_windows_thread_id;
            engine.unicorn.get_data_mut().crt_errno = 72;
            engine.unicorn.get_data_mut().windows_last_error = 71;
            let handle = engine
                .call_win64(CREATE, [0, 0, TEST_CODE, 0, 0, 0])
                .unwrap();
            let thread = &engine.unicorn.get_data().windows_threads[&handle];
            assert!(thread.completed);
            assert_eq!(thread.exit_code, 0x1234_5678);
            assert!(!thread.stack_mapped);
            assert!(
                !guest_range_has_permission(&engine.unicorn, thread.stack_base, 8, Prot::READ)
                    .unwrap()
            );
            assert_eq!(
                engine
                    .unicorn
                    .mem_read_as_vec(DATA_BASE + 0x380, 8)
                    .unwrap(),
                0x1234u64.to_le_bytes()
            );
            assert!(engine.unicorn.get_data().pending_windows_thread.is_none());
            assert_eq!(engine.unicorn.get_data().current_windows_thread_id, caller);
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        }
    }
}

#[test]
fn exit_thread_root_dispatch_is_a_diagnostic_and_foreign_dll_is_rejected() {
    const EXIT: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(&mut engine.unicorn, EXIT, "kernel32.dll", "ExitThread").unwrap();
    let error = engine
        .call_win64(EXIT, [9, 0, 0, 0, 0, 0])
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("root thread") && error.contains("exit code 9"),
        "{error}"
    );
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "ExitThread"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

fn test_ipv4_host() -> aex_host_identity::resolver::Ipv4Host {
    aex_host_identity::resolver::Ipv4Host {
        name: b"example.test".to_vec(),
        aliases: vec![b"alias.test".to_vec()],
        addresses: vec![[192, 0, 2, 1], [192, 0, 2, 2]],
    }
}

#[test]
fn hostent_serialization_has_win64_layout_and_terminated_pointer_arrays() {
    let host = test_ipv4_host();
    let base = 0x12340000;
    let bytes = serialize_guest_hostent(&host, base).unwrap();
    let pointer = |offset| u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    let name = (pointer(0) - base) as usize;
    assert_eq!(&bytes[name..name + 13], b"example.test\0");
    let aliases = (pointer(8) - base) as usize;
    let alias = (pointer(aliases) - base) as usize;
    assert_eq!(&bytes[alias..alias + 11], b"alias.test\0");
    assert_eq!(pointer(aliases + 8), 0);
    assert_eq!(&bytes[16..24], &[2, 0, 4, 0, 0, 0, 0, 0]);
    let addresses = (pointer(24) - base) as usize;
    for (index, expected) in host.addresses.iter().enumerate() {
        let address = (pointer(addresses + index * 8) - base) as usize;
        assert_eq!(&bytes[address..address + 4], expected);
    }
    assert_eq!(pointer(addresses + 16), 0);
    let mut excessive = host.clone();
    excessive.aliases = vec![vec![b'a'; 4095]; 256];
    assert!(serialize_guest_hostent(&excessive, base).is_err());
    assert!(serialize_guest_hostent(&host, u64::MAX - 4).is_err());
}

#[test]
fn hostent_storage_is_borrowed_per_thread_and_released_on_thread_exit() {
    const CREATE: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    let parent = engine.unicorn.get_data().current_windows_thread_id;
    let first = store_guest_hostent(&mut engine.unicorn, &test_ipv4_host()).unwrap();
    assert!(guest_range_has_permission(&engine.unicorn, first, 32, Prot::READ).unwrap());
    assert!(!guest_range_has_permission(&engine.unicorn, first, 32, Prot::WRITE).unwrap());
    assert!(!guest_range_has_permission(&engine.unicorn, first, 32, Prot::EXEC).unwrap());
    assert_eq!(
        store_guest_hostent(&mut engine.unicorn, &test_ipv4_host()).unwrap(),
        first
    );
    engine
        .unicorn
        .add_code_hook(TEST_CODE, TEST_CODE, |unicorn, _, _| {
            let address = store_guest_hostent(unicorn, &test_ipv4_host()).unwrap();
            unicorn
                .mem_write(DATA_BASE + 0x300, &address.to_le_bytes())
                .unwrap();
        })
        .unwrap();
    install_win64_import(&mut engine.unicorn, CREATE, "kernel32.dll", "CreateThread").unwrap();
    engine
        .call_win64(CREATE, [0, 0, TEST_CODE, 0, 0, 0])
        .unwrap();
    let child = u64::from_le_bytes(
        engine
            .unicorn
            .mem_read_as_vec(DATA_BASE + 0x300, 8)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert_ne!(first, child);
    assert!(!guest_range_has_permission(&engine.unicorn, child, 8, Prot::READ).unwrap());
    assert_eq!(engine.unicorn.get_data().windows_hostent_buffers.len(), 1);
    assert_eq!(
        engine.unicorn.get_data().windows_hostent_buffers[&parent],
        first
    );
    release_guest_hostent(&mut engine.unicorn, parent).unwrap();
    assert!(!guest_range_has_permission(&engine.unicorn, first, 8, Prot::READ).unwrap());
}

#[cfg(target_os = "macos")]
#[test]
fn gethostbyname_aliases_resolve_actual_ipv4_and_require_startup() {
    const LOOKUP: u64 = STUB_BASE + 0x410;
    for dll in ["wsock32.dll", "ws2_32.dll"] {
        for symbol in ["gethostbyname", "ORDINAL 52"] {
            let mut engine = test_engine(&[0xc3]);
            install_win64_import(&mut engine.unicorn, LOOKUP, dll, symbol).unwrap();
            let input = DATA_BASE + 0x100;
            engine.write(input, b"192.0.2.37\0").unwrap();
            assert_eq!(
                engine.call_win64(LOOKUP, [input, 0, 0, 0, 0, 0]).unwrap(),
                0
            );
            assert_eq!(engine.unicorn.get_data().windows_last_error, 10093);
            engine.unicorn.get_data_mut().windows_socket_startups = 1;
            engine.unicorn.get_data_mut().crt_errno = 72;
            let result = engine.call_win64(LOOKUP, [input, 0, 0, 0, 0, 0]).unwrap();
            assert_ne!(result, 0);
            let ptr_at = |engine: &GuestEngine, address| {
                u64::from_le_bytes(
                    engine
                        .unicorn
                        .mem_read_as_vec(address, 8)
                        .unwrap()
                        .try_into()
                        .unwrap(),
                )
            };
            let array = ptr_at(&engine, result + 24);
            let address = ptr_at(&engine, array);
            assert_eq!(
                engine.unicorn.mem_read_as_vec(address, 4).unwrap(),
                [192, 0, 2, 37]
            );
            assert_eq!(ptr_at(&engine, array + 8), 0);
            engine.write(input, b"127.0.0.1\0").unwrap();
            assert_eq!(
                engine.call_win64(LOOKUP, [input, 0, 0, 0, 0, 0]).unwrap(),
                result
            );
            assert_eq!(engine.unicorn.get_data().crt_errno, 72);
            engine.write(input, b"::1\0").unwrap();
            assert_eq!(
                engine.call_win64(LOOKUP, [input, 0, 0, 0, 0, 0]).unwrap(),
                0
            );
            assert_eq!(engine.unicorn.get_data().windows_last_error, 11004);
        }
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "gethostbyname"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn errno_pointer_is_coherent_with_crt_errors_and_memory_operations() {
    const ERRNO: u64 = STUB_BASE + 0x410;
    const OPEN: u64 = STUB_BASE + 0x420;
    const SET: u64 = STUB_BASE + 0x430;
    for dll in ["ucrtbase.dll", "api-ms-win-crt-runtime-l1-1-0.dll"] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(&mut engine.unicorn, ERRNO, dll, "_errno").unwrap();
        install_win64_import(&mut engine.unicorn, OPEN, "ucrtbase.dll", "fopen").unwrap();
        install_win64_import(&mut engine.unicorn, SET, "ucrtbase.dll", "memset").unwrap();
        engine.unicorn.get_data_mut().crt_errno = 77;
        let pointer = engine.call_win64(ERRNO, [0; 6]).unwrap();
        assert_eq!(get_guest_crt_errno(&engine.unicorn).unwrap(), 77);
        assert_eq!(engine.call_win64(ERRNO, [0; 6]).unwrap(), pointer);
        engine.call_win64(SET, [pointer, 0x11, 4, 0, 0, 0]).unwrap();
        assert_eq!(get_guest_crt_errno(&engine.unicorn).unwrap(), 0x11111111);
        engine.call_win64(OPEN, [0; 6]).unwrap();
        assert_eq!(
            engine.unicorn.mem_read_as_vec(pointer, 4).unwrap(),
            22u32.to_le_bytes()
        );
        assert_eq!(get_guest_crt_errno(&engine.unicorn).unwrap(), 22);
        assert!(!guest_range_has_permission(&engine.unicorn, pointer, 4, Prot::EXEC).unwrap());
    }
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "_errno"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn errno_pointer_survives_thread_yield_and_child_storage_is_released() {
    const ERRNO: u64 = STUB_BASE + 0x410;
    const CREATE: u64 = STUB_BASE + 0x420;
    const SWITCH: u64 = STUB_BASE + 0x430;
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    push_mov_imm64(&mut code, [0x48, 0xb8], ERRNO);
    code.extend_from_slice(&[0xff, 0xd0, 0xc7, 0x00, 42, 0, 0, 0]);
    push_mov_imm64(&mut code, [0x48, 0xb9], DATA_BASE + 0x300);
    code.extend_from_slice(&[0x48, 0x89, 0x01]);
    push_mov_imm64(&mut code, [0x48, 0xb8], SWITCH);
    code.extend_from_slice(&[0xff, 0xd0]);
    push_mov_imm64(&mut code, [0x48, 0xb8], ERRNO);
    code.extend_from_slice(&[0xff, 0xd0, 0x8b, 0x00, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    let mut engine = test_engine(&code);
    engine
        .unicorn
        .mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .unwrap();
    for (stub, dll, symbol) in [
        (ERRNO, "ucrtbase.dll", "_errno"),
        (CREATE, "kernel32.dll", "CreateThread"),
        (SWITCH, "kernel32.dll", "SwitchToThread"),
    ] {
        install_win64_import(&mut engine.unicorn, stub, dll, symbol).unwrap();
    }
    let parent = engine.call_win64(ERRNO, [0; 6]).unwrap();
    engine.write(parent, &77u32.to_le_bytes()).unwrap();
    let handle = engine
        .call_win64(CREATE, [0, 0, TEST_CODE, 0, 0, 0])
        .unwrap();
    assert_eq!(
        engine.unicorn.get_data().windows_threads[&handle].exit_code,
        42
    );
    assert_eq!(get_guest_crt_errno(&engine.unicorn).unwrap(), 77);
    assert_eq!(engine.call_win64(ERRNO, [0; 6]).unwrap(), parent);
    let child = u64::from_le_bytes(
        engine
            .unicorn
            .mem_read_as_vec(DATA_BASE + 0x300, 8)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert_ne!(child, parent);
    assert!(!guest_range_has_permission(&engine.unicorn, child, 4, Prot::READ).unwrap());
    assert_eq!(engine.unicorn.get_data().crt_errno_buffers.len(), 1);
}

#[test]
fn adapter_metadata_serialization_has_bounded_windows_pointers() {
    let first = WindowsAdapter {
        name: "en0".into(),
        description: "Wi-Fi 日本語".into(),
        dns_suffix: "example.test".into(),
        index: 7,
        physical_address: vec![2, 3, 4, 5, 6, 7],
        flags: 0x184,
        mtu: 1500,
        kind: 71,
        status: 1,
        ipv4: true,
        ipv6: true,
    };
    let second = WindowsAdapter {
        name: "lo0".into(),
        physical_address: vec![],
        index: 1,
        ..first.clone()
    };
    let bytes = serialize_windows_adapters(&[first.clone(), second], DATA_BASE, false).unwrap();
    let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let u64_at = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    assert_eq!(u32_at(0), 184);
    assert_eq!(u32_at(4), 7);
    assert_eq!(u64_at(8), DATA_BASE + 184);
    assert_eq!(u64_at(184 + 8), 0);
    assert_eq!(&bytes[80..86], &[2, 3, 4, 5, 6, 7]);
    assert_eq!(u32_at(88), 6);
    assert_eq!(u32_at(96), 1500);
    assert_eq!(u32_at(100), 71);
    for header in [0, 184] {
        for offset in [16, 56, 64, 72] {
            let pointer = u64_at(header + offset);
            assert!(pointer >= DATA_BASE + 368 && pointer < DATA_BASE + bytes.len() as u64);
        }
        for offset in [24, 32, 40, 48, 176] {
            assert_eq!(u64_at(header + offset), 0);
        }
    }
    let description = (u64_at(64) - DATA_BASE) as usize;
    let expected: Vec<_> = "Wi-Fi 日本語"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(&bytes[description..description + expected.len()], expected);
    let skipped = serialize_windows_adapters(&[first.clone()], DATA_BASE, true).unwrap();
    assert_eq!(&skipped[72..80], &[0; 8]);
    assert!(serialize_windows_adapters(&[first.clone()], u64::MAX - 100, false).is_err());
    let invalid = WindowsAdapter {
        physical_address: vec![0; 9],
        ..first
    };
    assert!(serialize_windows_adapters(&[invalid], 0, false).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn adapter_import_size_query_short_buffer_and_metadata_output() {
    const CALL: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CALL,
        "iphlpapi.dll",
        "GetAdaptersAddresses",
    )
    .unwrap();
    let size_pointer = DATA_BASE + 0x100;
    engine.write(size_pointer, &[0; 4]).unwrap();
    engine.unicorn.get_data_mut().windows_last_error = 71;
    engine.unicorn.get_data_mut().crt_errno = 72;
    assert_eq!(
        engine
            .call_win64(CALL, [0, 15, 0, 0, size_pointer, 0])
            .unwrap(),
        111
    );
    let needed = u32::from_le_bytes(
        engine
            .unicorn
            .mem_read_as_vec(size_pointer, 4)
            .unwrap()
            .try_into()
            .unwrap(),
    );
    assert!(needed >= 184 && needed <= 4 * 1024 * 1024);
    let output = allocate_crt_region(&mut engine.unicorn, needed as u64 + 4096).unwrap();
    engine.write(output, &[0x55; 8]).unwrap();
    engine.write(size_pointer, &1u32.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(CALL, [0, 15, 0, output, size_pointer, 0])
            .unwrap(),
        111
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        [0x55; 8]
    );
    engine
        .write(size_pointer, &(needed + 4096).to_le_bytes())
        .unwrap();
    assert_eq!(
        engine
            .call_win64(CALL, [0, 15, 0, output, size_pointer, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        184u32.to_le_bytes()
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    assert_eq!(
        engine
            .call_win64(CALL, [99, 15, 0, output, size_pointer, 0])
            .unwrap(),
        87
    );
    assert!(
        engine
            .call_win64(CALL, [0, 0x10, 0, output, size_pointer, 0])
            .is_err()
    );
    assert!(matches!(
        dispatch_win64_import("foreign.dll", "GetAdaptersAddresses"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn adapter_import_preflights_caller_storage() {
    const CALL: u64 = STUB_BASE + 0x410;
    for overlap in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        install_win64_import(
            &mut engine.unicorn,
            CALL,
            "iphlpapi.dll",
            "GetAdaptersAddresses",
        )
        .unwrap();
        let output = allocate_crt_region(&mut engine.unicorn, 65536).unwrap();
        let size = if overlap { output } else { DATA_BASE + 0x100 };
        engine.write(output, &[0x55; 8]).unwrap();
        engine.write(size, &65536u32.to_le_bytes()).unwrap();
        let before = engine.unicorn.mem_read_as_vec(output, 8).unwrap();
        if !overlap {
            engine
                .unicorn
                .mem_protect(output, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        assert!(
            engine
                .call_win64(CALL, [0, 15, 0, output, size, 0])
                .is_err()
        );
        assert_eq!(engine.unicorn.mem_read_as_vec(output, 8).unwrap(), before);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn adapter_translation_does_not_copy_physical_settings_to_vpn() {
    use aex_host_identity::{
        adapters::Interface,
        network_configuration::{Configuration, Property},
    };
    let interface = Interface {
        name: b"utun0".to_vec(),
        index: 9,
        native_flags: 1,
        native_type: 1,
        mtu: 1280,
        physical_address: vec![],
        addresses: vec![],
    };
    let mut config = Configuration::new();
    for (path, fields) in [
        (
            "Setup:/Network/Service/vpn/Interface",
            vec![("DeviceName", "en0"), ("Hardware", "AirPort")],
        ),
        (
            "Setup:/Network/Service/vpn/IPv4",
            vec![("ConfigMethod", "DHCP")],
        ),
        (
            "State:/Network/Service/vpn/IPv4",
            vec![("InterfaceName", "utun0")],
        ),
    ] {
        config.insert(
            path.into(),
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), Property::Text(value.into())))
                .collect(),
        );
    }
    assert!(windows_adapters_from_native(vec![interface.clone()], &config).is_err());
    config
        .get_mut("Setup:/Network/Service/vpn/Interface")
        .unwrap()
        .insert("DeviceName".into(), Property::Text("utun0".into()));
    let translated = windows_adapters_from_native(vec![interface], &config).unwrap();
    assert_eq!(translated[0].kind, 71);
    assert_eq!(translated[0].flags & 4, 4);
}

#[test]
fn stdio_integer_flags_precision_and_windows_lengths() {
    let mut engine = test_engine(&[0xc3]);
    let va = DATA_BASE + 0x100;
    let string = DATA_BASE + 0x500;
    engine.write(string, b"abcdef\0").unwrap();
    for (format, args, expected) in [
        (
            "%02x:%02X|%+06d|%#08x|%#o|%08.4d",
            vec![10, 254, (-42i32) as u32 as u64, 42, 9, 12],
            "0a:FE|-00042|0x00002a|011|    0012",
        ),
        (
            "%ld/%lld/%I64u/%hhd/%hu",
            vec![0x100000001, i64::MIN as u64, u64::MAX, 255, 0x10001],
            "1/-9223372036854775808/18446744073709551615/-1/1",
        ),
        (
            "%#.0o/%.0u/%-5.3s/%*.*d",
            vec![0, 0, string, (-7i32) as u32 as u64, 4, 12],
            "0//abc  /0012   ",
        ),
        (
            "%.*s|% 04d|%#X",
            vec![u32::MAX as u64, string, 7, 0],
            "abcdef| 007|0",
        ),
    ] {
        let bytes: Vec<_> = args.into_iter().flat_map(u64::to_le_bytes).collect();
        engine.write(va, &bytes).unwrap();
        assert_eq!(
            format_guest_stdio(&engine.unicorn, format.as_bytes(), va).unwrap(),
            expected.as_bytes(),
            "{format}"
        );
    }
}

#[test]
fn stdio_precision_stops_at_boundary_and_bounds_dynamic_dimensions() {
    let mut engine = test_engine(&[0xc3]);
    let va = DATA_BASE + 0x100;
    let text = DATA_BASE + PAGE_SIZE - 3;
    engine.write(text, b"abc").unwrap();
    engine.write(va, &text.to_le_bytes()).unwrap();
    assert_eq!(
        format_guest_stdio(&engine.unicorn, b"%.3s", va).unwrap(),
        b"abc"
    );
    assert!(format_guest_stdio(&engine.unicorn, b"%.4s", va).is_err());
    engine
        .write(va, &(i32::MIN as u32 as u64).to_le_bytes())
        .unwrap();
    assert!(format_guest_stdio(&engine.unicorn, b"%*d", va).is_err());
    assert!(format_guest_stdio(&engine.unicorn, b"%999999999999999999999d", va).is_err());
    assert!(format_guest_stdio(&engine.unicorn, b"%n", va).is_err());
    assert!(format_guest_stdio(&engine.unicorn, b"%d", u64::MAX - 3).is_err());
}

#[test]
fn stdio_zero_padded_hex_uses_existing_guest_buffer_contract() {
    const CALL: u64 = STUB_BASE + 0x410;
    let mut engine = test_engine(&[0xc3]);
    install_win64_import(
        &mut engine.unicorn,
        CALL,
        "ucrtbase.dll",
        "__stdio_common_vsprintf",
    )
    .unwrap();
    let format = DATA_BASE + 0x100;
    let va = DATA_BASE + 0x200;
    let output = DATA_BASE + 0x500;
    engine.write(format, b"%02x-%08lX\0").unwrap();
    engine
        .write(
            va,
            &[0xau64, 0x1000000ff]
                .into_iter()
                .flat_map(u64::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        engine
            .call_win64(CALL, [0x25, output, 32, format, 0, va])
            .unwrap(),
        11
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 12).unwrap(),
        b"0a-000000FF\0"
    );
}

#[test]
fn windows_asset_handles_own_binary_bytes_and_enforce_sharing_with_crt() {
    let mut engine = test_engine(&[0xc3]);
    let source = std::env::temp_dir().join(format!("aex-win-asset-{}.bin", std::process::id()));
    std::fs::write(&source, b"A\r\n\x1aB").unwrap();
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .sources
        .insert("c:/asset.bin".into(), source.clone());
    let handle = engine
        .unicorn
        .get_data_mut()
        .guest_files
        .open_windows_asset("c:/asset.bin", true, true, 0)
        .unwrap()
        .unwrap();
    let files = &engine.unicorn.get_data().guest_files;
    assert_eq!(&*files.windows_files[&handle].bytes, b"A\r\n\x1aB");
    assert_eq!(files.windows_files[&handle].position, 0);
    assert!(files.windows_files[&handle].readable);
    assert_eq!(files.live_bytes, 5);
    assert_eq!(
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .open_windows_asset("c:/asset.bin", true, true, 1)
            .unwrap(),
        Err(32)
    );
    assert_eq!(
        open_guest_stream(&mut engine.unicorn, b"c:/asset.bin", b"rb").unwrap(),
        (0, 13)
    );
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .close_windows_asset(handle)
        .unwrap();
    assert_eq!(
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .close_windows_asset(handle),
        Err(6)
    );
    assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 0);
    let (stream, error) = open_guest_stream(&mut engine.unicorn, b"c:/asset.bin", b"rb").unwrap();
    assert_eq!(error, 0);
    assert_eq!(
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .open_windows_asset("c:/asset.bin", true, true, 0)
            .unwrap(),
        Err(32)
    );
    let second = engine
        .unicorn
        .get_data_mut()
        .guest_files
        .open_windows_asset("c:/asset.bin", true, true, 1)
        .unwrap()
        .unwrap();
    assert_ne!(second, handle);
    assert_ne!(second, stream);
    assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 10);
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .close_windows_asset(second)
        .unwrap();
    const CLOSE: u64 = STUB_BASE + 0x410;
    install_win64_import(&mut engine.unicorn, CLOSE, "ucrtbase.dll", "fclose").unwrap();
    assert_eq!(
        engine.call_win64(CLOSE, [stream, 0, 0, 0, 0, 0]).unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 0);
    std::fs::remove_file(source).unwrap();
}

#[test]
fn windows_asset_limits_fail_without_issuing_handles() {
    let source = std::env::temp_dir().join(format!("aex-win-limit-{}.bin", std::process::id()));
    std::fs::write(&source, b"xy").unwrap();
    let mut files = GuestFiles::default();
    files.sources.insert("c:/asset.bin".into(), source.clone());
    assert_eq!(
        files
            .open_windows_asset("c:/missing.bin", true, true, 1)
            .unwrap(),
        Err(2)
    );
    assert_eq!(
        files
            .open_windows_asset("c:/asset.bin", true, true, 8)
            .unwrap(),
        Err(87)
    );
    files.live_bytes = MAX_GUEST_STREAM_BYTES - 1;
    assert!(
        files
            .open_windows_asset("c:/asset.bin", true, true, 1)
            .is_err()
    );
    assert!(files.windows_files.is_empty());
    assert_eq!(files.next_windows_file, 0);
    files.live_bytes = 0;
    files.next_windows_file = 65536;
    assert_eq!(
        files
            .open_windows_asset("c:/asset.bin", true, true, 1)
            .unwrap(),
        Err(4)
    );
    assert!(files.windows_files.is_empty());
    std::fs::remove_file(source).unwrap();
}

#[test]
fn standard_file_creation_shares_windows_handle_capacity() {
    let source =
        std::env::temp_dir().join(format!("aex-win-standard-limit-{}.bin", std::process::id()));
    std::fs::write(&source, b"").unwrap();
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .sources
        .insert("c:/empty.bin".into(), source.clone());
    let mut handles = Vec::new();
    for _ in 0..64 {
        handles.push(
            engine
                .unicorn
                .get_data_mut()
                .guest_files
                .open_windows_asset("c:/empty.bin", true, true, 1)
                .unwrap()
                .unwrap(),
        );
    }
    const CALL: u64 = STUB_BASE + 0x410;
    install_win64_import(&mut engine.unicorn, CALL, "ucrtbase.dll", "__acrt_iob_func").unwrap();
    assert!(engine.call_win64(CALL, [0; 6]).is_err());
    assert!(engine.unicorn.get_data().guest_files.streams.is_empty());
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .close_windows_asset(handles[0])
        .unwrap();
    engine.unicorn.get_data_mut().callback_error = None;
    let token = engine.call_win64(CALL, [0; 6]).unwrap();
    assert_eq!(engine.call_win64(CALL, [0; 6]).unwrap(), token);
    assert_eq!(
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .open_windows_asset("c:/empty.bin", true, true, 1)
            .unwrap(),
        Err(4)
    );
    assert_eq!(
        engine.unicorn.get_data().guest_files.windows_files.len()
            + engine.unicorn.get_data().guest_files.streams.len(),
        64
    );
    std::fs::remove_file(source).unwrap();
}

#[test]
fn windows_file_apis_open_read_seek_and_close_mounted_assets() {
    for (library, wide) in [
        ("kernel32.dll", false),
        ("kernelbase.dll", true),
        ("api-ms-win-core-file-l1-1-0.dll", false),
    ] {
        let mut engine = test_engine(&[0xc3]);
        let source =
            std::env::temp_dir().join(format!("aex-file-api-{}-{library}.bin", std::process::id()));
        std::fs::write(&source, b"A\r\n\x1aBC").unwrap();
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .sources
            .insert("c:/asset.bin".into(), source.clone());
        let open = STUB_BASE + 0x400;
        let read = open + 16;
        let size = open + 32;
        let seek = open + 48;
        let close = open + 64;
        let kind = open + 80;
        for (address, symbol) in [
            (open, if wide { "CreateFileW" } else { "CreateFileA" }),
            (read, "ReadFile"),
            (size, "GetFileSizeEx"),
            (seek, "SetFilePointerEx"),
        ] {
            install_win64_import(&mut engine.unicorn, address, library, symbol).unwrap();
        }
        install_win64_import(&mut engine.unicorn, close, "kernel32.dll", "CloseHandle").unwrap();
        install_win64_import(&mut engine.unicorn, kind, "kernel32.dll", "GetFileType").unwrap();
        let name = DATA_BASE + 0x100;
        let output = DATA_BASE + 0x300;
        let count = DATA_BASE + 0x500;
        let path: Vec<u8> = if wide {
            "C:\\asset.bin\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            b"C:\\asset.bin\0".to_vec()
        };
        engine.write(name, &path).unwrap();
        engine.unicorn.get_data_mut().windows_last_error = 71;
        engine.unicorn.get_data_mut().crt_errno = 72;
        let handle = engine
            .call_win64_with_timeout(
                open,
                &[name, 0x80000000, 1, 0, 3, 0x80, 0],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap();
        assert_ne!(handle, u64::MAX);
        assert_ne!(handle, 0);
        assert_eq!(engine.call_win64(kind, [handle, 0, 0, 0, 0, 0]).unwrap(), 1);
        assert_eq!(
            engine
                .call_win64(size, [handle, count, 0, 0, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(count, 8).unwrap(),
            6u64.to_le_bytes()
        );
        assert_eq!(
            engine
                .call_win64(read, [handle, output, 4, count, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
            b"A\r\n\x1a"
        );
        assert_eq!(
            engine
                .call_win64(seek, [handle, (-1i64) as u64, count, 2, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(count, 8).unwrap(),
            5u64.to_le_bytes()
        );
        assert_eq!(
            engine
                .call_win64(read, [handle, output, 9, count, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(count, 4).unwrap(),
            1u32.to_le_bytes()
        );
        assert_eq!(engine.unicorn.mem_read_as_vec(output, 1).unwrap(), b"C");
        assert_eq!(
            engine
                .call_win64(read, [handle, output, 9, count, 0, 0])
                .unwrap(),
            1
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(count, 4).unwrap(),
            0u32.to_le_bytes()
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
        assert_eq!(engine.unicorn.get_data().crt_errno, 72);
        assert_eq!(
            engine.call_win64(close, [handle, 0, 0, 0, 0, 0]).unwrap(),
            1
        );
        assert_eq!(engine.unicorn.get_data().guest_files.live_bytes, 0);
        assert_eq!(
            engine.call_win64(close, [handle, 0, 0, 0, 0, 0]).unwrap(),
            0
        );
        assert_eq!(engine.unicorn.get_data().windows_last_error, 6);
        assert_eq!(
            engine
                .call_win64(read, [handle, output, 1, count, 0, 0])
                .unwrap(),
            0
        );
        std::fs::remove_file(source).unwrap();
    }
}

#[test]
fn windows_read_preflight_keeps_cursor_and_outputs_unchanged() {
    for overlap in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        let source = std::env::temp_dir().join(format!(
            "aex-file-preflight-{}-{overlap}.bin",
            std::process::id()
        ));
        std::fs::write(&source, b"abcdef").unwrap();
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .sources
            .insert("c:/asset.bin".into(), source.clone());
        let handle = engine
            .unicorn
            .get_data_mut()
            .guest_files
            .open_windows_asset("c:/asset.bin", true, true, 1)
            .unwrap()
            .unwrap();
        let output = allocate_crt_region(&mut engine.unicorn, PAGE_SIZE).unwrap();
        let count = if overlap { output } else { DATA_BASE + 0x100 };
        engine.write(output, b"unchanged").unwrap();
        engine.write(count, &77u32.to_le_bytes()).unwrap();
        let before = engine.unicorn.mem_read_as_vec(output, 9).unwrap();
        if !overlap {
            engine
                .unicorn
                .mem_protect(output, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        const READ: u64 = STUB_BASE + 0x410;
        install_win64_import(&mut engine.unicorn, READ, "kernel32.dll", "ReadFile").unwrap();
        assert!(
            engine
                .call_win64(READ, [handle, output, 6, count, 0, 0])
                .is_err()
        );
        assert_eq!(
            engine.unicorn.get_data().guest_files.windows_files[&handle].position,
            0
        );
        assert_eq!(engine.unicorn.mem_read_as_vec(output, 9).unwrap(), before);
        assert_eq!(
            engine.unicorn.mem_read_as_vec(count, 4).unwrap(),
            77u32.to_le_bytes()
        );
        std::fs::remove_file(source).unwrap();
    }
}

#[test]
fn windows_file_open_always_inheritance_and_metadata_access() {
    let mut engine = test_engine(&[0xc3]);
    let source = std::env::temp_dir().join(format!("aex-file-access-{}.bin", std::process::id()));
    std::fs::write(&source, b"abc").unwrap();
    engine
        .unicorn
        .get_data_mut()
        .guest_files
        .sources
        .insert("c:/asset.bin".into(), source.clone());
    let open = STUB_BASE + 0x400;
    let read = open + 16;
    let seek = open + 32;
    for (address, symbol) in [
        (open, "CreateFileA"),
        (read, "ReadFile"),
        (seek, "SetFilePointerEx"),
    ] {
        install_win64_import(&mut engine.unicorn, address, "kernel32.dll", symbol).unwrap();
    }
    let name = DATA_BASE + 0x100;
    let security = DATA_BASE + 0x200;
    let output = DATA_BASE + 0x300;
    engine.write(name, b"C:/asset.bin\0").unwrap();
    let mut attributes = [0u8; 24];
    attributes[..4].copy_from_slice(&24u32.to_le_bytes());
    attributes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    attributes[16..20].copy_from_slice(&1u32.to_le_bytes());
    engine.write(security, &attributes).unwrap();
    let handle = engine
        .call_win64_with_timeout(
            open,
            &[name, 0, 1, security, 4, 0x80, 0],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_ne!(handle, u64::MAX);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 183);
    assert!(engine.unicorn.get_data().guest_files.windows_files[&handle].inheritable);
    engine.write(output, &77u64.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(read, [handle, output + 16, 1, output, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 5);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 4).unwrap(),
        0u32.to_le_bytes()
    );
    engine.write(output, &77u64.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(seek, [handle, 1, output, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        77u64.to_le_bytes()
    );
    assert_eq!(
        engine.unicorn.get_data().guest_files.windows_files[&handle].position,
        0
    );
    attributes[..4].copy_from_slice(&20u32.to_le_bytes());
    engine.write(security, &attributes).unwrap();
    assert_eq!(
        engine
            .call_win64_with_timeout(
                open,
                &[name, 0, 1, security, 3, 0x80, 0],
                TIMEOUT_MICROSECONDS
            )
            .unwrap(),
        u64::MAX
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 87);
    assert_eq!(engine.unicorn.get_data().guest_files.windows_files.len(), 1);
    std::fs::remove_file(source).unwrap();
}

#[test]
fn windows_create_file_resolves_guest_relative_and_dot_paths() {
    for wide in [false, true] {
        let mut engine = test_engine(&[0xc3]);
        let source =
            std::env::temp_dir().join(format!("aex-file-path-{}-{wide}.bin", std::process::id()));
        std::fs::write(&source, b"abc").unwrap();
        engine
            .unicorn
            .get_data_mut()
            .guest_files
            .sources
            .insert("c:/asset.bin".into(), source.clone());
        let open = STUB_BASE + 0x400;
        install_win64_import(
            &mut engine.unicorn,
            open,
            "kernel32.dll",
            if wide { "CreateFileW" } else { "CreateFileA" },
        )
        .unwrap();
        for path in [
            "asset.bin",
            "\\asset.bin",
            "C:\\.\\asset.bin",
            "C:asset.bin",
        ] {
            let path = format!("{path}\0");
            let bytes = if wide {
                path.encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            } else {
                path.into_bytes()
            };
            engine.write(DATA_BASE + 0x100, &bytes).unwrap();
            let handle = engine
                .call_win64_with_timeout(
                    open,
                    &[DATA_BASE + 0x100, 0x80000000, 1, 0, 3, 0x80, 0],
                    TIMEOUT_MICROSECONDS,
                )
                .unwrap();
            assert_ne!(handle, u64::MAX);
            assert_eq!(
                &*engine.unicorn.get_data().guest_files.windows_files[&handle].bytes,
                b"abc"
            );
        }
        std::fs::remove_file(source).unwrap();
    }
}

#[test]
fn critical_section_exhaustion_records_bounded_guest_evidence_without_mutation() {
    let mut engine = test_engine(&[0xc3]);
    let init = STUB_BASE + 0x400;
    install_win64_import(
        &mut engine.unicorn,
        init,
        "kernel32.dll",
        "InitializeCriticalSection",
    )
    .unwrap();
    for index in 0..MAX_WINDOWS_CRITICAL_SECTIONS {
        engine
            .unicorn
            .get_data_mut()
            .windows_critical_sections
            .insert(DATA_BASE + index as u64 * 40, 0);
    }
    let object = allocate_crt_region(&mut engine.unicorn, PAGE_SIZE).unwrap();
    engine
        .write(object, &[0xa5; WINDOWS_CRITICAL_SECTION_BYTES])
        .unwrap();
    let error = engine
        .call_win64(init, [object, 0, 0, 0, 0, 0])
        .unwrap_err()
        .to_string();
    assert!(error.contains("Windows critical-section count exceeds"));
    assert!(error.contains(&format!("requested={object:#x}")));
    assert!(error.contains(&format!("caller=Some({RETURN_ADDRESS:x})")));
    assert!(error.contains("live_sample=["));
    assert_eq!(
        engine.unicorn.get_data().windows_critical_sections.len(),
        MAX_WINDOWS_CRITICAL_SECTIONS
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(object, WINDOWS_CRITICAL_SECTION_BYTES)
            .unwrap(),
        [0xa5; WINDOWS_CRITICAL_SECTION_BYTES]
    );
}

#[test]
fn windows_critical_sections_support_multi_runtime_capacity_and_reclaim_slots() {
    let mut engine = test_engine(&[0xc3]);
    let init = STUB_BASE + 0x400;
    let delete = init + 16;
    install_win64_import(
        &mut engine.unicorn,
        init,
        "kernel32.dll",
        "InitializeCriticalSection",
    )
    .unwrap();
    install_win64_import(
        &mut engine.unicorn,
        delete,
        "kernel32.dll",
        "DeleteCriticalSection",
    )
    .unwrap();
    let storage = allocate_crt_region(&mut engine.unicorn, 4 * PAGE_SIZE).unwrap();
    for index in 0..300u64 {
        engine
            .call_win64(init, [storage + index * 40, 0, 0, 0, 0, 0])
            .unwrap();
    }
    assert_eq!(
        engine.unicorn.get_data().windows_critical_sections.len(),
        300
    );
    for index in 0..300u64 {
        engine
            .call_win64(delete, [storage + index * 40, 0, 0, 0, 0, 0])
            .unwrap();
    }
    assert!(
        engine
            .unicorn
            .get_data()
            .windows_critical_sections
            .is_empty()
    );
    engine.call_win64(init, [storage, 0, 0, 0, 0, 0]).unwrap();
    assert_eq!(engine.unicorn.get_data().windows_critical_sections.len(), 1);
}

#[test]
fn wsprintf_a_formats_register_and_stack_arguments_with_winuser_precision() {
    let mut engine = test_engine(&[0xc3]);
    let stub = STUB_BASE + 0x400;
    install_win64_import(&mut engine.unicorn, stub, "user32.dll", "wsprintfA").unwrap();
    let output = DATA_BASE + 0x100;
    let format = DATA_BASE + 0x500;
    let string = DATA_BASE + 0x700;
    engine.write(format, b"%s|%08X|%ld|%.0u|%Ix|%hs\0").unwrap();
    engine.write(string, b"abc\0").unwrap();
    engine.unicorn.get_data_mut().crt_errno = 71;
    engine.unicorn.get_data_mut().windows_last_error = 72;
    let expected = b"abc|0000002A|-1|0|123456789abcdef0|abc\0";
    let count = engine
        .call_win64_with_timeout(
            stub,
            &[
                output,
                format,
                string,
                42,
                u64::MAX,
                0,
                0x123456789abcdef0,
                string,
            ],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap();
    assert_eq!(count, (expected.len() - 1) as u64);
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(output, expected.len())
            .unwrap(),
        expected
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    assert_eq!(engine.unicorn.get_data().windows_last_error, 72);
    assert!(matches!(
        dispatch_win64_import("ucrtbase.dll", "wsprintfA"),
        Win64ImportDispatch::UnsupportedLegacyImport
    ));
}

#[test]
fn wsprintf_a_preflights_output_and_enforces_1024_byte_buffer_bound() {
    for (format_bytes, readonly, succeeds) in [
        (b"%1023s".as_slice(), false, true),
        (b"%1024s".as_slice(), false, false),
        (b"%s".as_slice(), true, false),
        (b"%ls".as_slice(), false, false),
        (b"%*s".as_slice(), false, false),
    ] {
        let mut engine = test_engine(&[0xc3]);
        let stub = STUB_BASE + 0x400;
        install_win64_import(&mut engine.unicorn, stub, "user32.dll", "wsprintfA").unwrap();
        let output = allocate_crt_region(&mut engine.unicorn, PAGE_SIZE).unwrap();
        engine.write(output, &[0xa5; 1024]).unwrap();
        let mut format = format_bytes.to_vec();
        format.push(0);
        engine.write(DATA_BASE + 0x100, &format).unwrap();
        engine.write(DATA_BASE + 0x200, b"a\0").unwrap();
        if readonly {
            engine
                .unicorn
                .mem_protect(output, PAGE_SIZE, Prot::READ)
                .unwrap();
        }
        let result = engine.call_win64(
            stub,
            [output, DATA_BASE + 0x100, DATA_BASE + 0x200, 0, 0, 0],
        );
        if succeeds {
            assert_eq!(result.unwrap(), 1023);
            let bytes = engine.unicorn.mem_read_as_vec(output, 1024).unwrap();
            assert!(bytes[..1022].iter().all(|byte| *byte == b' '));
            assert_eq!(&bytes[1022..], b"a\0");
        } else {
            assert!(result.is_err());
            assert_eq!(
                engine.unicorn.mem_read_as_vec(output, 1024).unwrap(),
                [0xa5; 1024]
            );
        }
    }
}

#[test]
fn wsprintf_a_long_string_and_winuser_precision_differ_from_crt() {
    let mut engine = test_engine(&[0xc3]);
    let stub = STUB_BASE + 0x400;
    install_win64_import(&mut engine.unicorn, stub, "user32.dll", "wsprintfA").unwrap();
    let output = allocate_crt_region(&mut engine.unicorn, PAGE_SIZE).unwrap();
    let string = allocate_crt_region(&mut engine.unicorn, PAGE_SIZE).unwrap();
    let mut bytes = vec![b'a'; 1023];
    bytes.push(0);
    engine.write(string, &bytes).unwrap();
    engine.write(DATA_BASE + 0x100, b"%s\0").unwrap();
    assert_eq!(
        engine
            .call_win64(stub, [output, DATA_BASE + 0x100, string, 0, 0, 0])
            .unwrap(),
        1023
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(output, 1024).unwrap(), bytes);
    engine.write(string, b"abc\0").unwrap();
    engine.write(DATA_BASE + 0x100, b"%.0s|%.4d|%#x\0").unwrap();
    let expected = b"abc|-012|0x0\0";
    assert_eq!(
        engine
            .call_win64(
                stub,
                [output, DATA_BASE + 0x100, string, (-12i64) as u64, 0, 0]
            )
            .unwrap(),
        expected.len() as u64 - 1
    );
    assert_eq!(
        engine
            .unicorn
            .mem_read_as_vec(output, expected.len())
            .unwrap(),
        expected
    );
}

#[test]
fn vex_prefilter_preserves_decoder_candidates_with_legacy_prefixes() {
    let mut factory = InstructionInfoFactory::new();
    for prefix in [
        None,
        Some(0x67),
        Some(0x64),
        Some(0x66),
        Some(0xf3),
        Some(0x48),
    ] {
        for first in 0..=255u8 {
            for second in 0..=255u8 {
                let mut bytes = vec![];
                bytes.extend(prefix);
                bytes.extend([first, second, 0x77, 0xc0, 0, 0, 0, 0]);
                let mut decoder = Decoder::with_ip(64, &bytes, TEST_CODE, DecoderOptions::NONE);
                let decoded = decoder.decode();
                if native_avx_state_sync(&decoded, &mut factory).is_some() {
                    assert!(may_start_vex_instruction(&bytes), "{bytes:x?}");
                }
            }
        }
    }
    assert!(!may_start_vex_instruction(&[0x48, 0x89, 0xc0]));
    assert!(!may_start_vex_instruction(&[0x90]));
    assert!(may_start_vex_instruction(&[0x64, 0x67, 0xc5, 0xf8, 0x77]));
}

#[test]
#[ignore = "manual performance comparison; no wall-clock correctness assertion"]
fn benchmark_non_vex_runtime_sync_decode() {
    let bytes = [0x48, 0x89, 0xc0];
    for filtered in [false, true] {
        let started = std::time::Instant::now();
        let mut matches = 0;
        for _ in 0..200_000 {
            let bytes = std::hint::black_box(&bytes);
            if filtered && !may_start_vex_instruction(bytes) {
                continue;
            }
            let mut decoder = Decoder::with_ip(64, bytes, TEST_CODE, DecoderOptions::NONE);
            let instruction = decoder.decode();
            let mut factory = InstructionInfoFactory::new();
            matches += usize::from(
                std::hint::black_box(native_avx_state_sync(&instruction, &mut factory)).is_some(),
            );
        }
        assert_eq!(matches, 0);
        eprintln!("non_vex_sync filtered={filtered}: {:?}", started.elapsed());
    }
}

#[test]
#[ignore = "manual hook dispatch performance comparison"]
fn benchmark_dense_runtime_hook_with_unrelated_import_hooks() {
    for unrelated in [0, 1024] {
        let mut uc = Unicorn::new_with_data(Arch::X86, Mode::MODE_64, 0usize).unwrap();
        uc.mem_map(TEST_CODE, PAGE_SIZE, Prot::ALL).unwrap();
        uc.mem_write(TEST_CODE, &[0x48, 0xff, 0xc9, 0x75, 0xfb, 0x90])
            .unwrap();
        uc.add_code_hook(TEST_CODE, TEST_CODE + 5, |uc, _, _| *uc.get_data_mut() += 1)
            .unwrap();
        for index in 0..unrelated {
            let address = TEST_CODE + PAGE_SIZE + index * 16;
            uc.add_code_hook(address, address, |_, _, _| panic!("unrelated hook invoked"))
                .unwrap();
        }
        uc.reg_write(RegisterX86::RCX, 100_000).unwrap();
        let started = std::time::Instant::now();
        uc.emu_start(TEST_CODE, TEST_CODE + 6, 10_000_000, 0)
            .unwrap();
        assert_eq!(*uc.get_data(), 200_001);
        assert_eq!(uc.reg_read(RegisterX86::RCX).unwrap(), 0);
        eprintln!(
            "hook_dispatch unrelated={unrelated}: {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn code_hook_cache_observes_callback_addition_deletion_and_stop() {
    let mut uc =
        Unicorn::new_with_data(Arch::X86, Mode::MODE_64, (Vec::<u8>::new(), false)).unwrap();
    uc.mem_map(TEST_CODE, PAGE_SIZE, Prot::ALL).unwrap();
    uc.mem_write(TEST_CODE, &[0x90, 0x90]).unwrap();
    let original = uc
        .add_code_hook(TEST_CODE, TEST_CODE + 1, |uc, _, _| {
            uc.get_data_mut().0.push(1);
            if uc.get_data().1 {
                uc.get_data_mut().1 = false;
                uc.add_code_hook(TEST_CODE, TEST_CODE + 1, |uc, _, _| {
                    uc.get_data_mut().0.push(2)
                })
                .unwrap();
            }
        })
        .unwrap();
    for index in 0..32 {
        let address = TEST_CODE + 0x100 + index;
        uc.add_code_hook(address, address, |_, _, _| panic!("unrelated hook"))
            .unwrap();
    }
    uc.emu_start(TEST_CODE, TEST_CODE + 2, 1_000_000, 0)
        .unwrap();
    assert_eq!(uc.get_data().0, [1, 1]);
    uc.get_data_mut().0.clear();
    uc.get_data_mut().1 = true;
    uc.emu_start(TEST_CODE, TEST_CODE + 2, 1_000_000, 0)
        .unwrap();
    assert_eq!(uc.get_data().0, [1, 2, 1, 2]);
    uc.remove_hook(original).unwrap();
    uc.get_data_mut().0.clear();
    uc.emu_start(TEST_CODE, TEST_CODE + 2, 1_000_000, 0)
        .unwrap();
    assert_eq!(uc.get_data().0, [2, 2]);
    uc.get_data_mut().0.clear();
    uc.emu_start(TEST_CODE, TEST_CODE + 2, 1_000_000, 1)
        .unwrap();
    assert_eq!(uc.get_data().0, [2]);
    uc.add_code_hook(TEST_CODE, TEST_CODE + 1, |uc, _, _| {
        uc.get_data_mut().0.push(3);
        uc.emu_stop().unwrap();
    })
    .unwrap();
    uc.get_data_mut().0.clear();
    uc.emu_start(TEST_CODE, TEST_CODE + 2, 1_000_000, 0)
        .unwrap();
    assert_eq!(uc.get_data().0, [2, 3]);
}

#[test]
fn strcpy_s_requires_terminator_and_ignores_fourth_register() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(&mut engine.unicorn, entry, "ucrtbase.dll", "strcpy_s").unwrap();
    let dst = DATA_BASE + 0x100;
    let src = DATA_BASE + 0x200;
    engine.write(src, b"abc\0").unwrap();
    for fourth in [0, u64::MAX] {
        engine.write(dst, &[0xa5; 8]).unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [dst, 4, src, fourth, 0, 0])
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(dst, 5).unwrap(),
            b"abc\0\xa5"
        );
        assert_eq!(
            engine
                .call_win64(entry, [dst, 3, src, fourth, 0, 0])
                .unwrap(),
            34
        );
        assert_eq!(engine.unicorn.mem_read_as_vec(dst, 4).unwrap(), b"\0bc\0");
        assert_eq!(engine.unicorn.get_data().crt_errno, 34);
    }
    assert_eq!(engine.call_win64(entry, [dst, 4, 0, 0, 0, 0]).unwrap(), 22);
    assert_eq!(
        dispatch_win64_import("other.dll", "strcpy_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn strcat_s_appends_and_clears_invalid_destinations() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(&mut engine.unicorn, entry, "ucrtbase.dll", "strcat_s").unwrap();
    let dst = DATA_BASE + 0x100;
    let src = DATA_BASE + 0x200;
    engine.write(src, b"cd\0").unwrap();
    engine.write(dst, b"ab\0xxx").unwrap();
    engine.unicorn.get_data_mut().crt_errno = 71;
    assert_eq!(engine.call_win64(entry, [dst, 5, src, 0, 0, 0]).unwrap(), 0);
    assert_eq!(engine.unicorn.mem_read_as_vec(dst, 6).unwrap(), b"abcd\0x");
    assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    for (initial, capacity, source, error) in [
        (b"ab\0xxx", 4, src, 34),
        (b"abcdef", 6, src, 22),
        (b"ab\0xxx", 6, 0, 22),
        (b"ab\0xxx", 6, dst, 22),
    ] {
        engine.write(dst, initial).unwrap();
        assert_eq!(
            engine
                .call_win64(entry, [dst, capacity, source, 0, 0, 0])
                .unwrap(),
            error
        );
        let mut expected = *initial;
        expected[0] = 0;
        assert_eq!(engine.unicorn.mem_read_as_vec(dst, 6).unwrap(), expected);
        assert_eq!(engine.unicorn.get_data().crt_errno as u64, error);
    }
    assert_eq!(
        dispatch_win64_import("other.dll", "strcat_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn get_version_matches_extended_version_and_preserves_errors() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    let extended = STUB_BASE + 0x110;
    install_win64_import(&mut engine.unicorn, entry, "kernel32.dll", "GetVersion").unwrap();
    install_win64_import(
        &mut engine.unicorn,
        extended,
        "kernel32.dll",
        "GetVersionExA",
    )
    .unwrap();
    let out = DATA_BASE + 0x100;
    engine.write(out, &148u32.to_le_bytes()).unwrap();
    assert_eq!(
        engine.call_win64(extended, [out, 0, 0, 0, 0, 0]).unwrap(),
        1
    );
    let bytes = engine.unicorn.mem_read_as_vec(out, 20).unwrap();
    let major = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let minor = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let build = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    engine.unicorn.get_data_mut().windows_last_error = 71;
    engine.unicorn.get_data_mut().crt_errno = 72;
    assert_eq!(
        engine.call_win64(entry, [0; 6]).unwrap(),
        ((build << 16) | (minor << 8) | major) as u64
    );
    assert_eq!(engine.unicorn.get_data().windows_last_error, 71);
    assert_eq!(engine.unicorn.get_data().crt_errno, 72);
    assert_eq!(
        dispatch_win64_import("other.dll", "GetVersion"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}

#[test]
fn vsprintf_s_uses_six_argument_abi_and_rejects_small_buffer() {
    let mut engine = test_engine(&[0xc3]);
    let entry = STUB_BASE + 0x100;
    install_win64_import(
        &mut engine.unicorn,
        entry,
        "ucrtbase.dll",
        "__stdio_common_vsprintf_s",
    )
    .unwrap();
    let dst = DATA_BASE + 0x100;
    let format = DATA_BASE + 0x200;
    let args = DATA_BASE + 0x300;
    engine.write(format, b"id=%d\0").unwrap();
    engine.write(args, &42u64.to_le_bytes()).unwrap();
    engine.write(dst, &[0xa5; 8]).unwrap();
    engine.unicorn.get_data_mut().crt_errno = 71;
    assert_eq!(
        engine
            .call_win64(entry, [0x24, dst, 6, format, 0, args])
            .unwrap(),
        5
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(dst, 7).unwrap(),
        b"id=42\0\xa5"
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 71);
    assert_eq!(
        engine
            .call_win64(entry, [0x24, dst, 5, format, 0, args])
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(engine.unicorn.mem_read_as_vec(dst, 6).unwrap(), b"\0d=42\0");
    assert_eq!(engine.unicorn.get_data().crt_errno, 34);
    assert_eq!(
        engine
            .call_win64(entry, [0x24, dst, 6, 0, 0, args])
            .unwrap(),
        u32::MAX as u64
    );
    assert_eq!(engine.unicorn.get_data().crt_errno, 22);
    assert_eq!(
        dispatch_win64_import("other.dll", "__stdio_common_vsprintf_s"),
        Win64ImportDispatch::UnsupportedLegacyImport
    );
}
