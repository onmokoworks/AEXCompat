use super::*;

const TEST_CODE: u64 = 0x1000_0000;
const TEST_CXX_THROW: u64 = STUB_BASE + 0x80460;
const TEST_THROW_INFO: u64 = TEST_CODE + 0x800;

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
        next_data: DATA_BASE,
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

    engine.unicorn.mem_write(output, b"unchanged\0").unwrap();
    let error = engine
        .call_win64_with_timeout(
            VSPRINTF,
            &[0x25, output, 8, format, 0, va_list],
            TIMEOUT_MICROSECONDS,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("finite vsprintf buffer count 8"),
        "{error}"
    );
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 10).unwrap(),
        b"unchanged\0"
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
        .mem_write(format, b"unsupported=%x\0")
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
        error.to_string().contains("unsupported conversion '%x'"),
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
            lock_count: MAX_MSVCP_MUTEX_RECURSION,
        },
    );
    let error = engine
        .call_win64(LOCK, [object, 0, 0, 0, 0, 0])
        .unwrap_err();
    assert!(error.to_string().contains("recursion"), "{error}");
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
    assert_eq!(value, HOST_ENVIRONMENT_VALUE);
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
    const LOAD_LIBRARY: u64 = STUB_BASE + 0x1a0;
    let mut engine = test_engine(&[0xc3]);
    assert_eq!(
        install_win64_import(
            &mut engine.unicorn,
            LOAD_LIBRARY,
            "kernel32.dll",
            "LoadLibraryExW",
        )
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
fn system_time_import_writes_a_deterministic_validated_filetime() {
    let mut engine = test_engine(&[0xc3]);
    let output = engine.allocate(8, 8).unwrap();
    engine.unicorn.reg_write(RegisterX86::RCX, output).unwrap();
    emulate_get_system_time_as_file_time(&mut engine.unicorn);
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        132_223_104_000_000_000u64.to_le_bytes()
    );
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
    assert_eq!(qword(0x98), ((STACK_BASE + STACK_SIZE - 0x108) | 8) + 8);
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
        (((STACK_BASE + STACK_SIZE) - 0x108) | 8) + 8
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
    assert_eq!(
        engine.unicorn.mem_read_as_vec(output, 8).unwrap(),
        1u64.to_le_bytes()
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
        smart_checkout_world(&engine.unicorn, 1).unwrap(),
        (input_world, 4, 3)
    );

    engine
        .write(
            layer_definition + abi::PARAM_U_OFFSET as u64 + abi::LAYER_WORLD_FLAGS_OFFSET as u64,
            &1i32.to_le_bytes(),
        )
        .unwrap();
    let error = smart_checkout_world(&engine.unicorn, 1).unwrap_err();
    assert!(error.contains("invalid smart checkout world"), "{error}");
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
