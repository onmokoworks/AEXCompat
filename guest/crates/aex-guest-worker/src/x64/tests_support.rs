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
        install_pf_ansi_suite_v2(&mut unicorn).unwrap();
        install_gpu_device_suite(&mut unicorn).unwrap();
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
                    [1, 0x8000, 0x8000, params, destination, 0],
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
    fn legacy_area_sample8_matches_common_runtime_and_rejects_unsupported_edges() {
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
                    [1, 0x8000, 0x8000, params, destination, 0],
                )
                .unwrap(),
            0
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
            [255, 50, 50, 0]
        );

        engine.write(destination, &[7, 7, 7, 7]).unwrap();
        engine.write(params + 24, &1u32.to_le_bytes()).unwrap();
        assert_eq!(
            engine
                .call_win64(
                    HOST_AREA_SAMPLE8,
                    [1, 0x8000, 0x8000, params, destination, 0],
                )
                .unwrap(),
            4
        );
        assert_eq!(
            engine.unicorn.mem_read_as_vec(destination, 4).unwrap(),
            [7, 7, 7, 7]
        );
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
                0, 0, 0, 0, 128, 50, 0, 0, 128, 100, 0, 0, 0, 0, 0, 0, 128, 0, 50, 0, 128, 0, 100,
                0,
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
        let (source, _) =
            argb8_world_fixture(&mut engine, SIDE as i32, SIDE as i32, &source_pixels);
        let (destination, destination_pixels) =
            argb8_world_fixture(&mut engine, SIDE as i32, SIDE as i32, &destination_before);
        let (mask, _) =
            argb8_mask_world_fixture(&mut engine, SIDE as i32, SIDE as i32, &mask_pixels);
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
            install_win64_import(
                &mut engine.unicorn,
                MEMCHR,
                "VCRUNTIME140.DLL",
                "memchr",
            )
            .unwrap(),
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
        install_win64_import(
            &mut engine.unicorn,
            MEMCHR,
            "vcruntime140.dll",
            "memchr",
        )
        .unwrap();

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
        install_win64_import(
            &mut engine.unicorn,
            MEMCHR,
            "vcruntime140.dll",
            "memchr",
        )
        .unwrap();
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
        assert!(
            error.to_string().contains("memchr source read"),
            "{error}"
        );
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
        engine.unicorn.mem_write(format, b"unsupported=%x\0").unwrap();
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
            Win64ImportDispatch::LegacyImplemented(
                LegacyWin64Import::GetSystemTimeAsFileTime
            )
        );
        assert_eq!(
            dispatch_win64_import("fixture.dll", "GetSystemTimeAsFileTime"),
            Win64ImportDispatch::UnsupportedLegacyImport
        );
        assert_eq!(
            canonical_import_trace_label(
                r"C:\Windows\System32\OPENCL.DLL",
                "clCreateKernel"
            ),
            "opencl.dll!clCreateKernel"
        );
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
    fn bounded_windows_runtime_imports_write_outputs_and_remain_library_scoped() {
        for (symbol, implementation) in [
            (
                "GetCurrentThreadId",
                LegacyWin64Import::GetCurrentThreadId,
            ),
            (
                "GetCurrentProcessId",
                LegacyWin64Import::GetCurrentProcessId,
            ),
            (
                "QueryPerformanceCounter",
                LegacyWin64Import::QueryPerformanceCounter,
            ),
            ("InitializeSListHead", LegacyWin64Import::InitializeSListHead),
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
        assert_eq!(engine.unicorn.mem_read_as_vec(output, 8).unwrap(), 1u64.to_le_bytes());
        engine.unicorn.mem_write(output, &[0xff; 16]).unwrap();
        emulate_initialize_slist_head(&mut engine.unicorn);
        assert_eq!(engine.unicorn.mem_read_as_vec(output, 16).unwrap(), [0; 16]);
    }

    #[test]
    fn win64_crt_math_imports_classify_without_bypassing_library_routing() {
        let crt_math = "api-ms-win-crt-math-l1-1-0.dll";
        assert_eq!(
            dispatch_win64_import(crt_math, "cosf"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::CosF)
        );
        assert_eq!(
            dispatch_win64_import(crt_math, "sinf"),
            Win64ImportDispatch::LegacyImplemented(LegacyWin64Import::SinF)
        );
        assert_eq!(
            dispatch_win64_import("OpenCL.DLL", "cosf"),
            Win64ImportDispatch::UnsupportedGpuLibrary(GpuImportLibrary::OpenCl)
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
                install_win64_import(
                    &mut engine.unicorn,
                    IMPORT,
                    library,
                    symbol,
                )
                .unwrap(),
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
        engine.begin_execution_trace("GPU_DEVICE_SETUP", TEST_CODE).unwrap();
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
        ));
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
    fn pf_handle_dispose_consumes_outstanding_locks_and_rejects_stale_handles() {
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
        assert!(!engine.unicorn.get_data().handles.contains_key(&handle));

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
            0x48, 0x8b, 0x44, 0x24, 0x28, 0x31, 0xd2, 0x4d, 0x85, 0xc9, 0x0f, 0x95, 0xc2, 0x89,
            0x10, 0x31, 0xc0, 0xc3,
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
