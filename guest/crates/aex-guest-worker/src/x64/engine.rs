impl GuestEngine<'static> {
    pub fn backend_name(&self) -> &'static str {
        "unicorn-x86_64"
    }

    pub fn load(image: &PeImage) -> Result<Self, GuestError> {
        let trace_points = discover_trace_points(image);
        let image_report = image.report();
        let mut trace_modules = vec![TraceModule {
            name: image_report.entry_export.clone(),
            kind: "mapped_pe",
            sha256: Some(image_report.sha256.clone()),
            symbols: vec![image_report.entry_export.clone()],
        }];
        trace_modules.extend(image_report.imports.iter().map(|library| {
            TraceModule {
                name: library.name.clone(),
                kind: "emulated_import_stubs",
                sha256: None,
                symbols: library
                    .symbols
                    .iter()
                    .map(|symbol| symbol.name.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            }
        }));
        let mut unicorn = uc(
            "create x86_64 engine",
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()),
        )?;
        install_avx_fallback(&mut unicorn)?;
        unicorn.get_data_mut().next_handle_data = HANDLE_DATA_BASE;
        unicorn.get_data_mut().next_aegp_memory_handle = AEGP_MEMORY_HANDLE_BASE;
        unicorn.get_data_mut().next_pf_handle_data = PF_HANDLE_DATA_BASE;
        let image_size =
            u64::try_from(image.mapped_bytes().len()).map_err(|_| GuestError::ImageAlignment)?;
        unicorn.get_data_mut().image_region = Some((
            image.image_base(),
            image
                .image_base()
                .checked_add(image_size)
                .ok_or(GuestError::DataCapacity)?,
        ));
        unicorn.get_data_mut().image_executable_ranges = image
            .section_protections()
            .iter()
            .filter(|section| section.executable)
            .filter_map(|section| {
                let start = image
                    .image_base()
                    .checked_add(section.virtual_address as u64)?;
                let end = start.checked_add(section.virtual_size as u64)?;
                Some((start, end))
            })
            .collect();
        if image.image_base() % PAGE_SIZE != 0 || image_size % PAGE_SIZE != 0 {
            return Err(GuestError::ImageAlignment);
        }
        uc(
            "map PE image",
            unicorn.mem_map(image.image_base(), image_size, Prot::ALL),
        )?;
        uc(
            "write PE image",
            unicorn.mem_write(image.image_base(), image.mapped_bytes()),
        )?;
        install_avx_state_sync_points(&mut unicorn, discover_image_avx_state_sync_points(image)?)?;
        uc(
            "map import stubs",
            unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL),
        )?;
        uc(
            "map stack",
            unicorn.mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE),
        )?;
        // MSVC's x64 __chkstk reads the Windows TEB stack limit at GS:[0x10].
        // Unicorn starts with a zero GS base, so provide only the non-executable
        // first page needed by that helper.
        uc(
            "map minimal TEB page",
            unicorn.mem_map(0, PAGE_SIZE, Prot::READ | Prot::WRITE),
        )?;
        uc(
            "write TEB stack limit",
            unicorn.mem_write(0x10, &STACK_BASE.to_le_bytes()),
        )?;
        uc(
            "map guest data",
            unicorn.mem_map(DATA_BASE, DATA_SIZE, Prot::READ | Prot::WRITE),
        )?;
        let mut stub_index = 0u64;
        for library in image.imports() {
            for symbol in &library.symbols {
                let stub = STUB_BASE
                    .checked_add(stub_index * STUB_STRIDE)
                    .ok_or(GuestError::StubCapacity)?;
                if stub + STUB_STRIDE > HOST_ADD_PARAM {
                    return Err(GuestError::StubCapacity);
                }
                // Temporary import behavior for the first controlled fixture:
                // return zero inside the guest. Typed import traps replace these
                // entries before guest execution; no native host address is exposed.
                uc(
                    "write import stub",
                    unicorn.mem_write(stub, &[0x31, 0xc0, 0xc3]),
                )?;
                install_win64_import(&mut unicorn, stub, &library.name, &symbol.name)?;
                unicorn.get_data_mut().trace_labels.insert(
                    stub,
                    TraceLabel {
                        kind: TraceLabelKind::Import,
                        name: canonical_import_trace_label(&library.name, &symbol.name),
                    },
                );
                let iat_rva = u64::try_from(symbol.iat_rva).map_err(|_| GuestError::IatRange)?;
                let iat = image
                    .image_base()
                    .checked_add(iat_rva)
                    .ok_or(GuestError::IatRange)?;
                if iat + 8 > image.image_base() + image_size {
                    return Err(GuestError::IatRange);
                }
                uc("patch IAT", unicorn.mem_write(iat, &stub.to_le_bytes()))?;
                stub_index += 1;
            }
        }
        uc(
            "write return sentinel",
            unicorn.mem_write(RETURN_ADDRESS, &[0xcc]),
        )?;
        uc(
            "write add_param callback",
            unicorn.mem_write(HOST_ADD_PARAM, &[0xc3]),
        )?;
        uc(
            "write poison callback",
            unicorn.mem_write(HOST_POISON, &[0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]),
        )?;
        uc(
            "write unsupported AEGP Memory callback",
            unicorn.mem_write(HOST_AEGP_MEMORY_UNSUPPORTED, &[0xb8, 4, 0, 0, 0, 0xc3]),
        )?;
        uc(
            "write ANSI strcpy callback",
            unicorn.mem_write(HOST_ANSI_STRCPY, &[0xc3]),
        )?;
        uc("write copy callback", unicorn.mem_write(HOST_COPY, &[0xc3]))?;
        uc(
            "write blend callback",
            unicorn.mem_write(HOST_BLEND, &[0xc3]),
        )?;
        uc(
            "write no-op callback",
            unicorn.mem_write(HOST_NOOP, &[0x31, 0xc0, 0xc3]),
        )?;
        for (operation, address) in [
            ("write pre-checkout callback", HOST_PRE_CHECKOUT_LAYER),
            ("write checkout-pixels callback", HOST_CHECKOUT_LAYER_PIXELS),
            ("write checkin-pixels callback", HOST_CHECKIN_LAYER_PIXELS),
            ("write checkout-output callback", HOST_CHECKOUT_OUTPUT),
            ("write acquire-suite callback", HOST_ACQUIRE_SUITE),
            ("write checkout-param callback", HOST_CHECKOUT_PARAM),
            ("write checkin-param callback", HOST_CHECKIN_PARAM),
            ("write new-handle callback", HOST_NEW_HANDLE),
            ("write lock-handle callback", HOST_LOCK_HANDLE),
            ("write unlock-handle callback", HOST_UNLOCK_HANDLE),
            ("write dispose-handle callback", HOST_DISPOSE_HANDLE),
            ("write handle-size callback", HOST_HANDLE_SIZE),
            ("write resize-handle callback", HOST_RESIZE_HANDLE),
            ("write AEGP register callback", HOST_AEGP_REGISTER),
            ("write AEGP main-window callback", HOST_AEGP_GET_MAIN_WINDOW),
            ("write AEGP new-memory callback", HOST_AEGP_NEW_MEM_HANDLE),
            ("write AEGP free-memory callback", HOST_AEGP_FREE_MEM_HANDLE),
            ("write AEGP lock-memory callback", HOST_AEGP_LOCK_MEM_HANDLE),
            (
                "write AEGP unlock-memory callback",
                HOST_AEGP_UNLOCK_MEM_HANDLE,
            ),
            ("write AEGP memory-size callback", HOST_AEGP_MEM_HANDLE_SIZE),
            (
                "write AEGP resize-memory callback",
                HOST_AEGP_RESIZE_MEM_HANDLE,
            ),
            ("write new-world callback", HOST_NEW_WORLD),
            ("write dispose-world callback", HOST_DISPOSE_WORLD),
            (
                "write get-world-pixel-format callback",
                HOST_GET_WORLD_PIXEL_FORMAT,
            ),
            ("write PluginData v2 callback", HOST_PLUGIN_DATA_V2),
            ("write PluginData v1 callback", HOST_PLUGIN_DATA_V1),
            ("write Iterate8 callback", HOST_ITERATE8),
            ("write Iterate8 continuation", HOST_ITERATE8_CONTINUE),
            ("write color-param callback", HOST_COLOR_PARAM_VALUE),
            ("write point-param callback", HOST_POINT_PARAM_VALUE),
            ("write extended allocation callback", HOST_EXTENDED_ALLOC),
            ("write extended free callback", HOST_EXTENDED_FREE),
            ("write extended lookup callback", HOST_EXTENDED_LOOKUP),
            ("write Iterate8 origin callback", HOST_ITERATE8_ORIGIN),
            ("write Fill8 callback", HOST_FILL8),
            ("write legacy new-world callback", HOST_NEW_WORLD8),
            (
                "write get-callback-address callback",
                HOST_GET_CALLBACK_ADDR,
            ),
            ("write SubpixelSample8 callback", HOST_SUBPIXEL_SAMPLE8),
            ("write AreaSample8 callback", HOST_AREA_SAMPLE8),
            ("write TransferRect8 callback", HOST_TRANSFER_RECT8),
            ("write Iterate16 callback", HOST_ITERATE16),
            ("write Iterate16 continuation", HOST_ITERATE16_CONTINUE),
        ] {
            uc(operation, unicorn.mem_write(address, &[0xc3]))?;
        }
        uc(
            "write Iterate8 zero source pixel",
            unicorn.mem_write(HOST_ZERO_PIXEL, &[0; 16]),
        )?;
        uc(
            "install add_param callback",
            unicorn.add_code_hook(HOST_ADD_PARAM, HOST_ADD_PARAM, |unicorn, _, _| {
                capture_add_param(unicorn);
            }),
        )?;
        uc(
            "install ANSI strcpy callback",
            unicorn.add_code_hook(HOST_ANSI_STRCPY, HOST_ANSI_STRCPY, |unicorn, _, _| {
                emulate_strcpy(unicorn);
            }),
        )?;
        uc(
            "install copy callback",
            unicorn.add_code_hook(HOST_COPY, HOST_COPY, |unicorn, _, _| {
                emulate_copy(unicorn);
            }),
        )?;
        uc(
            "install blend callback",
            unicorn.add_code_hook(HOST_BLEND, HOST_BLEND, |unicorn, _, _| {
                emulate_blend(unicorn);
            }),
        )?;
        uc(
            "install pre-checkout callback",
            unicorn.add_code_hook(
                HOST_PRE_CHECKOUT_LAYER,
                HOST_PRE_CHECKOUT_LAYER,
                emulate_pre_checkout_layer,
            ),
        )?;
        uc(
            "install checkout-pixels callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_LAYER_PIXELS,
                HOST_CHECKOUT_LAYER_PIXELS,
                emulate_checkout_layer_pixels,
            ),
        )?;
        uc(
            "install checkin-pixels callback",
            unicorn.add_code_hook(
                HOST_CHECKIN_LAYER_PIXELS,
                HOST_CHECKIN_LAYER_PIXELS,
                emulate_checkin_layer_pixels,
            ),
        )?;
        uc(
            "install checkout-output callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_OUTPUT,
                HOST_CHECKOUT_OUTPUT,
                emulate_checkout_output,
            ),
        )?;
        uc(
            "install acquire-suite callback",
            unicorn.add_code_hook(
                HOST_ACQUIRE_SUITE,
                HOST_ACQUIRE_SUITE,
                emulate_acquire_suite,
            ),
        )?;
        uc(
            "install AEGP register callback",
            unicorn.add_code_hook(
                HOST_AEGP_REGISTER,
                HOST_AEGP_REGISTER,
                emulate_aegp_register,
            ),
        )?;
        uc(
            "install AEGP main-window callback",
            unicorn.add_code_hook(
                HOST_AEGP_GET_MAIN_WINDOW,
                HOST_AEGP_GET_MAIN_WINDOW,
                emulate_aegp_get_main_window,
            ),
        )?;
        uc(
            "install Iterate8 callback",
            unicorn.add_code_hook(HOST_ITERATE8, HOST_ITERATE8, emulate_iterate8),
        )?;
        uc(
            "install Iterate8 origin callback",
            unicorn.add_code_hook(
                HOST_ITERATE8_ORIGIN,
                HOST_ITERATE8_ORIGIN,
                emulate_iterate8_origin,
            ),
        )?;
        uc(
            "install Fill8 callback",
            unicorn.add_code_hook(HOST_FILL8, HOST_FILL8, emulate_fill8),
        )?;
        uc(
            "install SubpixelSample8 callback",
            unicorn.add_code_hook(
                HOST_SUBPIXEL_SAMPLE8,
                HOST_SUBPIXEL_SAMPLE8,
                emulate_subpixel_sample8,
            ),
        )?;
        uc(
            "install AreaSample8 callback",
            unicorn.add_code_hook(HOST_AREA_SAMPLE8, HOST_AREA_SAMPLE8, emulate_area_sample8),
        )?;
        uc(
            "install TransferRect8 callback",
            unicorn.add_code_hook(
                HOST_TRANSFER_RECT8,
                HOST_TRANSFER_RECT8,
                emulate_transfer_rect8,
            ),
        )?;
        uc(
            "install legacy new-world callback",
            unicorn.add_code_hook(HOST_NEW_WORLD8, HOST_NEW_WORLD8, emulate_new_world8),
        )?;
        uc(
            "install get-callback-address callback",
            unicorn.add_code_hook(
                HOST_GET_CALLBACK_ADDR,
                HOST_GET_CALLBACK_ADDR,
                emulate_get_callback_addr,
            ),
        )?;
        uc(
            "install Iterate8 continuation",
            unicorn.add_code_hook(
                HOST_ITERATE8_CONTINUE,
                HOST_ITERATE8_CONTINUE,
                continue_iterate,
            ),
        )?;
        uc(
            "install Iterate16 callback",
            unicorn.add_code_hook(HOST_ITERATE16, HOST_ITERATE16, emulate_iterate16),
        )?;
        uc(
            "install Iterate16 continuation",
            unicorn.add_code_hook(
                HOST_ITERATE16_CONTINUE,
                HOST_ITERATE16_CONTINUE,
                continue_iterate,
            ),
        )?;
        uc(
            "install color-param callback",
            unicorn.add_code_hook(
                HOST_COLOR_PARAM_VALUE,
                HOST_COLOR_PARAM_VALUE,
                emulate_color_param_value,
            ),
        )?;
        uc(
            "install point-param callback",
            unicorn.add_code_hook(
                HOST_POINT_PARAM_VALUE,
                HOST_POINT_PARAM_VALUE,
                emulate_point_param_value,
            ),
        )?;
        uc(
            "install PluginData v2 callback",
            unicorn.add_code_hook(HOST_PLUGIN_DATA_V2, HOST_PLUGIN_DATA_V2, |unicorn, _, _| {
                capture_plugin_data_registration(unicorn, true)
            }),
        )?;
        uc(
            "install PluginData v1 callback",
            unicorn.add_code_hook(HOST_PLUGIN_DATA_V1, HOST_PLUGIN_DATA_V1, |unicorn, _, _| {
                capture_plugin_data_registration(unicorn, false)
            }),
        )?;
        uc(
            "install checkout-param callback",
            unicorn.add_code_hook(
                HOST_CHECKOUT_PARAM,
                HOST_CHECKOUT_PARAM,
                emulate_checkout_param,
            ),
        )?;
        uc(
            "install checkin-param callback",
            unicorn.add_code_hook(HOST_CHECKIN_PARAM, HOST_CHECKIN_PARAM, |unicorn, _, _| {
                let _ = unicorn.reg_write(RegisterX86::RAX, 0);
            }),
        )?;
        uc(
            "install extended allocation callback",
            unicorn.add_code_hook(
                HOST_EXTENDED_ALLOC,
                HOST_EXTENDED_ALLOC,
                emulate_extended_alloc,
            ),
        )?;
        uc(
            "install extended free callback",
            unicorn.add_code_hook(
                HOST_EXTENDED_FREE,
                HOST_EXTENDED_FREE,
                emulate_extended_free,
            ),
        )?;
        uc(
            "install extended lookup callback",
            unicorn.add_code_hook(
                HOST_EXTENDED_LOOKUP,
                HOST_EXTENDED_LOOKUP,
                emulate_extended_lookup,
            ),
        )?;
        for (operation, address, callback) in [
            (
                "install new-handle callback",
                HOST_NEW_HANDLE,
                emulate_new_handle as fn(&mut Unicorn<'_, GuestState>, u64, u32),
            ),
            (
                "install lock-handle callback",
                HOST_LOCK_HANDLE,
                emulate_lock_handle,
            ),
            (
                "install unlock-handle callback",
                HOST_UNLOCK_HANDLE,
                emulate_unlock_handle,
            ),
            (
                "install dispose-handle callback",
                HOST_DISPOSE_HANDLE,
                emulate_dispose_handle,
            ),
            (
                "install handle-size callback",
                HOST_HANDLE_SIZE,
                emulate_handle_size,
            ),
            (
                "install resize-handle callback",
                HOST_RESIZE_HANDLE,
                emulate_resize_handle,
            ),
            (
                "install AEGP new-memory callback",
                HOST_AEGP_NEW_MEM_HANDLE,
                emulate_aegp_new_mem_handle,
            ),
            (
                "install AEGP free-memory callback",
                HOST_AEGP_FREE_MEM_HANDLE,
                emulate_aegp_free_mem_handle,
            ),
            (
                "install AEGP lock-memory callback",
                HOST_AEGP_LOCK_MEM_HANDLE,
                emulate_aegp_lock_mem_handle,
            ),
            (
                "install AEGP unlock-memory callback",
                HOST_AEGP_UNLOCK_MEM_HANDLE,
                emulate_aegp_unlock_mem_handle,
            ),
            (
                "install AEGP memory-size callback",
                HOST_AEGP_MEM_HANDLE_SIZE,
                emulate_aegp_mem_handle_size,
            ),
            (
                "install AEGP resize-memory callback",
                HOST_AEGP_RESIZE_MEM_HANDLE,
                emulate_aegp_resize_mem_handle,
            ),
            (
                "install new-world callback",
                HOST_NEW_WORLD,
                emulate_new_world,
            ),
            (
                "install dispose-world callback",
                HOST_DISPOSE_WORLD,
                emulate_dispose_world,
            ),
            (
                "install get-world-pixel-format callback",
                HOST_GET_WORLD_PIXEL_FORMAT,
                emulate_get_world_pixel_format,
            ),
        ] {
            uc(operation, unicorn.add_code_hook(address, address, callback))?;
        }
        let mut handle_suite = [0u8; 48];
        for (offset, address) in [
            HOST_NEW_HANDLE,
            HOST_LOCK_HANDLE,
            HOST_UNLOCK_HANDLE,
            HOST_DISPOSE_HANDLE,
            HOST_HANDLE_SIZE,
            HOST_RESIZE_HANDLE,
        ]
        .into_iter()
        .enumerate()
        {
            handle_suite[offset * 8..offset * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write PF Handle Suite",
            unicorn.mem_write(HOST_HANDLE_SUITE, &handle_suite),
        )?;
        let mut aegp_memory_suite = [0u8; 64];
        for (slot, address) in [
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
            aegp_memory_suite[slot * 8..slot * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write AEGP Memory Suite",
            unicorn.mem_write(HOST_AEGP_MEMORY_SUITE, &aegp_memory_suite),
        )?;
        let mut world_suite = [0u8; 24];
        for (slot, address) in [
            HOST_NEW_WORLD,
            HOST_DISPOSE_WORLD,
            HOST_GET_WORLD_PIXEL_FORMAT,
        ]
        .into_iter()
        .enumerate()
        {
            world_suite[slot * 8..slot * 8 + 8].copy_from_slice(&address.to_le_bytes());
        }
        uc(
            "write PF World Suite",
            unicorn.mem_write(HOST_WORLD_SUITE, &world_suite),
        )?;
        install_iterate8_suites(&mut unicorn)?;
        install_pf_ansi_suite_v2(&mut unicorn)?;
        install_gpu_device_suite(&mut unicorn).map_err(|error| GuestError::Unicorn {
            operation: "install PF GPU Device Suite",
            detail: error.to_string(),
        })?;
        uc(
            "write PF ColorParamSuite",
            unicorn.mem_write(
                HOST_COLOR_PARAM_SUITE,
                &HOST_COLOR_PARAM_VALUE.to_le_bytes(),
            ),
        )?;
        uc(
            "write PF PointParamSuite",
            unicorn.mem_write(
                HOST_POINT_PARAM_SUITE,
                &HOST_POINT_PARAM_VALUE.to_le_bytes(),
            ),
        )?;
        install_aegp_utility_suites(&mut unicorn)?;
        for (address, name) in [
            (HOST_ADD_PARAM, "add_param"),
            (HOST_POISON, "unsupported_callback"),
            (HOST_ANSI_STRCPY, "ansi_strcpy"),
            (HOST_COPY, "copy"),
            (HOST_BLEND, "blend"),
            (HOST_NOOP, "noop"),
            (HOST_PRE_CHECKOUT_LAYER, "pre_checkout_layer"),
            (HOST_CHECKOUT_LAYER_PIXELS, "checkout_layer_pixels"),
            (HOST_CHECKIN_LAYER_PIXELS, "checkin_layer_pixels"),
            (HOST_CHECKOUT_OUTPUT, "checkout_output"),
            (HOST_ACQUIRE_SUITE, "acquire_suite"),
            (HOST_CHECKOUT_PARAM, "checkout_param"),
            (HOST_CHECKIN_PARAM, "checkin_param"),
            (HOST_NEW_HANDLE, "new_handle"),
            (HOST_LOCK_HANDLE, "lock_handle"),
            (HOST_UNLOCK_HANDLE, "unlock_handle"),
            (HOST_DISPOSE_HANDLE, "dispose_handle"),
            (HOST_HANDLE_SIZE, "handle_size"),
            (HOST_RESIZE_HANDLE, "resize_handle"),
            (HOST_AEGP_REGISTER, "aegp_register_with_aegp"),
            (HOST_AEGP_GET_MAIN_WINDOW, "aegp_get_main_window"),
            (HOST_AEGP_NEW_MEM_HANDLE, "aegp_new_mem_handle"),
            (HOST_AEGP_FREE_MEM_HANDLE, "aegp_free_mem_handle"),
            (HOST_AEGP_LOCK_MEM_HANDLE, "aegp_lock_mem_handle"),
            (HOST_AEGP_UNLOCK_MEM_HANDLE, "aegp_unlock_mem_handle"),
            (HOST_AEGP_MEM_HANDLE_SIZE, "aegp_mem_handle_size"),
            (HOST_AEGP_RESIZE_MEM_HANDLE, "aegp_resize_mem_handle"),
            (HOST_NEW_WORLD, "new_world"),
            (HOST_DISPOSE_WORLD, "dispose_world"),
            (HOST_GET_WORLD_PIXEL_FORMAT, "get_world_pixel_format"),
            (HOST_PF_ANSI_ATAN, "pf_ansi_atan"),
            (HOST_PF_ANSI_ATAN2, "pf_ansi_atan2"),
            (HOST_PF_ANSI_CEIL, "pf_ansi_ceil"),
            (HOST_PF_ANSI_COS, "pf_ansi_cos"),
            (HOST_PF_ANSI_EXP, "pf_ansi_exp"),
            (HOST_PF_ANSI_FABS, "pf_ansi_fabs"),
            (HOST_PF_ANSI_FLOOR, "pf_ansi_floor"),
            (HOST_PF_ANSI_FMOD, "pf_ansi_fmod"),
            (HOST_PF_ANSI_HYPOT, "pf_ansi_hypot"),
            (HOST_PF_ANSI_LOG, "pf_ansi_log"),
            (HOST_PF_ANSI_LOG10, "pf_ansi_log10"),
            (HOST_PF_ANSI_POW, "pf_ansi_pow"),
            (HOST_PF_ANSI_SIN, "pf_ansi_sin"),
            (HOST_PF_ANSI_SQRT, "pf_ansi_sqrt"),
            (HOST_PF_ANSI_TAN, "pf_ansi_tan"),
            (HOST_PF_ANSI_SPRINTF, "pf_ansi_sprintf"),
            (HOST_PF_ANSI_STRCPY, "pf_ansi_strcpy"),
            (HOST_PF_ANSI_ASIN, "pf_ansi_asin"),
            (HOST_PF_ANSI_ACOS, "pf_ansi_acos"),
            (HOST_PF_ANSI_STRCPY_BOUNDED, "pf_ansi_strcpy_bounded"),
            (HOST_PLUGIN_DATA_V2, "plugin_data_v2"),
            (HOST_PLUGIN_DATA_V1, "plugin_data_v1"),
            (HOST_ITERATE8, "iterate8"),
            (HOST_ITERATE8_CONTINUE, "iterate8_continue"),
            (HOST_COLOR_PARAM_VALUE, "color_param_value"),
            (HOST_POINT_PARAM_VALUE, "point_param_value"),
            (HOST_EXTENDED_ALLOC, "extended_alloc"),
            (HOST_EXTENDED_FREE, "extended_free"),
            (HOST_EXTENDED_LOOKUP, "extended_lookup"),
            (HOST_ITERATE8_ORIGIN, "iterate8_origin"),
            (HOST_FILL8, "fill8"),
            (HOST_NEW_WORLD8, "new_world8"),
            (HOST_GET_CALLBACK_ADDR, "get_callback_addr"),
            (HOST_TRANSFER_RECT8, "transfer_rect8"),
            (HOST_ITERATE16, "iterate16"),
            (HOST_ITERATE16_CONTINUE, "iterate16_continue"),
            (HOST_GPU_GET_DEVICE_COUNT, "gpu_get_device_count"),
            (HOST_GPU_GET_DEVICE_INFO, "gpu_get_device_info"),
            (HOST_GPU_ACQUIRE_EXCLUSIVE, "gpu_acquire_exclusive"),
            (HOST_GPU_RELEASE_EXCLUSIVE, "gpu_release_exclusive"),
            (HOST_GPU_ALLOCATE_DEVICE, "gpu_allocate_device_memory"),
            (HOST_GPU_FREE_DEVICE, "gpu_free_device_memory"),
            (HOST_GPU_PURGE_DEVICE, "gpu_purge_device_memory"),
            (HOST_GPU_ALLOCATE_HOST, "gpu_allocate_host_memory"),
            (HOST_GPU_FREE_HOST, "gpu_free_host_memory"),
            (HOST_GPU_PURGE_HOST, "gpu_purge_host_memory"),
            (HOST_GPU_CREATE_WORLD, "gpu_create_world"),
            (HOST_GPU_DISPOSE_WORLD, "gpu_dispose_world"),
            (HOST_GPU_GET_WORLD_DATA, "gpu_get_world_data"),
            (HOST_GPU_GET_WORLD_SIZE, "gpu_get_world_size"),
            (
                HOST_GPU_GET_WORLD_DEVICE_INDEX,
                "gpu_get_world_device_index",
            ),
        ] {
            unicorn.get_data_mut().trace_labels.insert(
                address,
                TraceLabel {
                    kind: TraceLabelKind::HostCallback,
                    name: name.to_string(),
                },
            );
        }
        let mut engine = Self {
            unicorn,
            next_data: DATA_BASE,
            image_base: image.image_base(),
            image_end: image.image_base() + image_size,
            census_hook: None,
            trace_hooks: Vec::new(),
            trace_points,
            image_sha256: image_report.sha256,
            entry_export: image_report.entry_export,
            trace_modules,
        };
        if let Some(table) = image.string_table() {
            let empty = engine.allocate(1, 1)?;
            engine.write(empty, &[0])?;
            let mut strings = HashMap::with_capacity(table.len());
            for (id, value) in table {
                let address = engine.allocate(value.len() + 1, 1)?;
                engine.write(address, value)?;
                engine.write(address + value.len() as u64, &[0])?;
                strings.insert(*id, address);
            }
            let state = engine.unicorn.get_data_mut();
            state.extended_strings = strings;
            state.extended_empty_string = empty;
            state.extended_string_table_valid = true;
        }
        if let Some(entry) = image.dll_entry_address() {
            let attached = engine.call_win64(entry, [image.image_base(), 1, 0, 0, 0, 0])?;
            if attached == 0 {
                return Err(GuestError::DllProcessAttach);
            }
        }
        Ok(engine)
    }

    pub fn resolve_effect_entry(
        &mut self,
        image: &PeImage,
        selector: Option<&str>,
        basic_suite: u64,
    ) -> Result<u64, GuestError> {
        if let Some(entry) = image.entry_address() {
            if selector.is_some() {
                return Err(GuestError::Callback(
                    "effect selection is unavailable when a direct effect entrypoint exists".into(),
                ));
            }
            return Ok(entry);
        }
        let (registration_entry, callback) = if let Some(entry) =
            image.export_address("PluginDataEntryFunction2")
        {
            (entry, HOST_PLUGIN_DATA_V2)
        } else if let Some(entry) = image.export_address("PluginDataEntryFunction") {
            (entry, HOST_PLUGIN_DATA_V1)
        } else {
            return Err(GuestError::Callback(
                    "effect selector requires PluginData registration, but no registration export exists"
                        .into(),
                ));
        };
        self.unicorn.get_data_mut().plugin_data_registry = EffectRegistry::default();
        self.unicorn.get_data_mut().plugin_data_error = None;
        let host_name = self.allocate(10, 1)?;
        self.write(host_name, b"AEXCompat\0")?;
        let host_version = self.allocate(5, 1)?;
        self.write(host_version, b"2025\0")?;
        let returned = self.call_win64(
            registration_entry,
            [1, callback, basic_suite, host_name, host_version, 0],
        )? as i32;
        if let Some(error) = self.unicorn.get_data_mut().plugin_data_error.take() {
            return Err(GuestError::Callback(format!(
                "PluginData registration rejected: {error}"
            )));
        }
        if returned != 0 {
            return Err(GuestError::Callback(format!(
                "PluginData entrypoint returned {returned}"
            )));
        }
        let registration = self
            .unicorn
            .get_data()
            .plugin_data_registry
            .select(selector)
            .map_err(|error| GuestError::Callback(error.to_string()))?
            .clone();
        let entry = image
            .export_address(&registration.entrypoint)
            .ok_or_else(|| {
                GuestError::Callback(format!(
                    "registered effect entrypoint {} is not an executable export",
                    registration.entrypoint
                ))
            })?;
        self.entry_export.clone_from(&registration.entrypoint);
        if let Some(module) = self.trace_modules.first_mut() {
            module.name.clone_from(&registration.entrypoint);
            module.symbols = vec![registration.entrypoint];
        }
        Ok(entry)
    }

    pub fn begin_execution_trace(
        &mut self,
        selector: &str,
        entry_address: u64,
    ) -> Result<(), GuestError> {
        if self.unicorn.get_data().trace.is_some() {
            return Err(GuestError::Callback(
                "guest execution trace is already active".into(),
            ));
        }
        let watch_specs = self.unicorn.get_data().trace_watches.clone();
        let selector_watches = watch_specs
            .iter()
            .filter_map(|spec| {
                let address = spec.absolute_address?;
                Some(PendingTraceWatch {
                    spec_id: spec.id.clone(),
                    register: spec.register,
                    call_id: None,
                    function_rva: None,
                    pc_rva: None,
                    address,
                    before: trace_memory_snapshot(
                        &self.unicorn,
                        address,
                        spec.size,
                        self.image_base,
                        self.image_end,
                    ),
                    image_coordinate: spec.image_coordinate,
                    image_row_offset: spec.image_row_offset,
                    image_format: spec.image_format,
                })
            })
            .collect();
        self.unicorn.get_data_mut().trace = Some(TraceCapture {
            selector: selector.to_string(),
            entry_rva: entry_address.saturating_sub(self.image_base),
            events: vec![TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "selector_enter",
                call_id: None,
                function_rva: Some(entry_address.saturating_sub(self.image_base)),
                pc_rva: Some(entry_address.saturating_sub(self.image_base)),
                target_rva: None,
                name: Some(selector.to_string()),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            }],
            return_stack: Vec::new(),
            function_stack: vec![Some(entry_address.saturating_sub(self.image_base))],
            call_rsp_stack: Vec::new(),
            call_id_stack: Vec::new(),
            next_call_id: 1,
            watch_specs,
            watch_occurrence_counts: HashMap::new(),
            watch_stack: Vec::new(),
            selector_watches,
            witnesses: Vec::new(),
            dropped_witnesses: 0,
            basic_blocks: HashMap::new(),
            branch_edges: HashMap::new(),
            dropped_basic_blocks: 0,
            dropped_branch_edges: 0,
            previous_block: None,
            event_index: HashMap::new(),
            event_fingerprints: HashMap::new(),
            known_function_entries: HashSet::from([entry_address.saturating_sub(self.image_base)]),
            truncated: false,
            dropped_events: 0,
        });
        let image_base = self.image_base;
        let image_end = self.image_end;
        let mut hook_points = self.trace_points.clone();
        hook_points.push(entry_address);
        let label_points = self
            .unicorn
            .get_data()
            .trace_labels
            .keys()
            .copied()
            .collect::<Vec<_>>();
        hook_points.extend(label_points.iter().copied());
        for point in label_points {
            let mut bytes = [0u8; STUB_STRIDE as usize];
            if self.unicorn.mem_read(point, &mut bytes).is_err() {
                continue;
            }
            let mut decoder = Decoder::with_ip(64, &bytes, point, DecoderOptions::NONE);
            while decoder.can_decode() {
                let instruction = decoder.decode();
                if matches!(instruction.mnemonic(), Mnemonic::Ret | Mnemonic::Retf) {
                    hook_points.push(instruction.ip());
                    break;
                }
            }
        }
        hook_points.sort_unstable();
        hook_points.dedup();
        let block_hook = uc(
            "install guest trace block hook",
            self.unicorn.add_block_hook(
                image_base,
                image_end - 1,
                move |unicorn, address, size| {
                    advance_runtime_target_lifecycle(unicorn.get_data_mut(), address);
                    if let Some(capture) = unicorn.get_data_mut().trace.as_mut() {
                        let block_key = (address, size);
                        if let Some(observed) = capture.basic_blocks.get_mut(&block_key) {
                            *observed += 1;
                        } else if capture.basic_blocks.len() < MAX_TRACE_BASIC_BLOCKS {
                            capture.basic_blocks.insert(block_key, 1);
                        } else {
                            capture.dropped_basic_blocks += 1;
                        }
                        if let Some(previous) = capture.previous_block.replace(address) {
                            let edge_key = (previous, address);
                            if let Some(observed) = capture.branch_edges.get_mut(&edge_key) {
                                *observed += 1;
                            } else if capture.branch_edges.len() < MAX_TRACE_BRANCH_EDGES {
                                capture.branch_edges.insert(edge_key, 1);
                            } else {
                                capture.dropped_branch_edges += 1;
                            }
                        }
                    }
                },
            ),
        )?;
        self.trace_hooks.push(block_hook);
        for point in hook_points {
            let hook = uc(
                "install guest execution trace point",
                self.unicorn
                    .add_code_hook(point, point, move |unicorn, address, size| {
                        trace_instruction(unicorn, address, size, image_base, image_end);
                    }),
            )?;
            self.trace_hooks.push(hook);
        }
        Ok(())
    }

    pub fn finish_execution_trace(
        &mut self,
        return_value: u64,
    ) -> Result<ExecutionTrace, GuestError> {
        for hook in self.trace_hooks.drain(..) {
            uc(
                "remove guest execution trace hook",
                self.unicorn.remove_hook(hook),
            )?;
        }
        let selector_watches = self
            .unicorn
            .get_data()
            .trace
            .as_ref()
            .map(|capture| capture.selector_watches.clone())
            .unwrap_or_default();
        let selector_witnesses = selector_watches
            .into_iter()
            .map(|pending| {
                let after = trace_memory_snapshot(
                    &self.unicorn,
                    pending.address,
                    pending.before.size,
                    self.image_base,
                    self.image_end,
                );
                TraceMemoryWitness {
                    watch_id: pending.spec_id,
                    call_id: pending.call_id,
                    function_rva: pending.function_rva,
                    pc_rva: pending.pc_rva,
                    register: pending.register,
                    image_coordinate: pending.image_coordinate,
                    image_row_offset: pending.image_row_offset,
                    image_format: pending.image_format,
                    changed_ranges: trace_changed_ranges(&pending.before, &after),
                    before: pending.before,
                    after,
                }
            })
            .collect::<Vec<_>>();
        let mut capture =
            self.unicorn.get_data_mut().trace.take().ok_or_else(|| {
                GuestError::Callback("guest execution trace is not active".into())
            })?;
        for witness in selector_witnesses {
            if capture.witnesses.len() >= MAX_TRACE_WITNESSES {
                capture.witnesses.pop();
                capture.dropped_witnesses += 1;
            }
            capture.witnesses.push(witness);
        }
        if capture.events.len() >= MAX_TRACE_EVENTS {
            capture.events.pop();
            capture.truncated = true;
            capture.dropped_events += 1;
        }
        let entry_rva = capture.entry_rva;
        push_trace_event(
            &mut capture,
            TraceEvent {
                sequence: 0,
                observed_count: 1,
                depth: 0,
                kind: "selector_exit",
                call_id: None,
                function_rva: Some(entry_rva),
                pc_rva: Some(entry_rva),
                target_rva: None,
                name: Some(format!("return={return_value:#x}")),
                arguments: Vec::new(),
                xmm_arguments: Vec::new(),
                stack_arguments: Vec::new(),
                return_value: None,
                exemplars: TraceExemplars::default(),
                call_kind: None,
                instruction_bytes: None,
            },
        );
        let mut functions = aggregate_trace_functions(entry_rva, &capture.events);
        for function in &mut functions {
            let mut bytes = [0u8; 16];
            if self
                .unicorn
                .mem_read(self.image_base + function.entry_rva, &mut bytes)
                .is_ok()
            {
                function.entry_bytes = bytes_to_hex(&bytes);
            }
        }
        let timeline = capture.events.iter().map(format_trace_event).collect();
        let mut truncation = Vec::new();
        if capture.truncated {
            truncation.push(TraceTruncation {
                category: "events",
                reason: "event_budget",
                dropped: capture.dropped_events,
            });
        }
        if capture.dropped_witnesses > 0 {
            truncation.push(TraceTruncation {
                category: "memory_witnesses",
                reason: "witness_budget",
                dropped: capture.dropped_witnesses,
            });
        }
        if capture.dropped_basic_blocks > 0 {
            truncation.push(TraceTruncation {
                category: "basic_blocks",
                reason: "distinct_block_budget",
                dropped: capture.dropped_basic_blocks,
            });
        }
        if capture.dropped_branch_edges > 0 {
            truncation.push(TraceTruncation {
                category: "branch_edges",
                reason: "distinct_edge_budget",
                dropped: capture.dropped_branch_edges,
            });
        }
        let untracked_fingerprints = capture
            .events
            .iter()
            .map(|event| event.exemplars.untracked_fingerprint_observations)
            .sum();
        if untracked_fingerprints > 0 {
            truncation.push(TraceTruncation {
                category: "exemplar_fingerprints",
                reason: "fingerprint_budget",
                dropped: untracked_fingerprints,
            });
        }
        let trace_truncated = !truncation.is_empty();
        let trace_configuration = TraceConfiguration {
            max_events: MAX_TRACE_EVENTS,
            max_basic_blocks: MAX_TRACE_BASIC_BLOCKS,
            max_branch_edges: MAX_TRACE_BRANCH_EDGES,
            max_witnesses: MAX_TRACE_WITNESSES,
            max_watch_bytes: MAX_TRACE_WATCH_BYTES,
            max_distinct_fingerprints_per_event: TRACE_DISTINCT_FINGERPRINTS,
            watches: capture.watch_specs.clone(),
        };
        let mut basic_blocks = capture
            .basic_blocks
            .into_iter()
            .map(|((address, size), observed_count)| TraceBasicBlock {
                rva: address.saturating_sub(self.image_base),
                size,
                observed_count,
            })
            .collect::<Vec<_>>();
        basic_blocks.sort_by_key(|block| (block.rva, block.size));
        let mut branch_edges = capture
            .branch_edges
            .into_iter()
            .map(|((from, to), observed_count)| TraceBranchEdge {
                from_rva: from.saturating_sub(self.image_base),
                to_rva: to.saturating_sub(self.image_base),
                observed_count,
            })
            .collect::<Vec<_>>();
        branch_edges.sort_by_key(|edge| (edge.from_rva, edge.to_rva));
        Ok(ExecutionTrace {
            schema: "aexcompat.aex-execution-trace",
            schema_version: 1,
            execution_backend: self.backend_name(),
            image_sha256: self.image_sha256.clone(),
            preferred_image_base: self.image_base,
            entry_export: self.entry_export.clone(),
            worker_build_identity: format!(
                "aex-guest-worker/{} rev={} ({}/{})",
                env!("CARGO_PKG_VERSION"),
                env!("AEXCOMPAT_BUILD_REVISION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            modules: self.trace_modules.clone(),
            trace_configuration,
            selector: capture.selector,
            entry_rva: capture.entry_rva,
            return_value,
            truncated: trace_truncated,
            events: capture.events,
            functions,
            state_changes: Vec::new(),
            memory_witnesses: capture.witnesses,
            dropped_memory_witnesses: capture.dropped_witnesses,
            basic_blocks,
            branch_edges,
            truncation,
            timeline,
        })
    }

    pub fn discard_execution_trace(&mut self) -> Result<(), GuestError> {
        let mut first_error = None;
        for hook in self.trace_hooks.drain(..) {
            if let Err(error) = self.unicorn.remove_hook(hook)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        self.unicorn.get_data_mut().trace = None;
        if let Some(error) = first_error {
            return Err(GuestError::Callback(format!(
                "remove aborted guest execution trace hook: {error}"
            )));
        }
        Ok(())
    }

    pub fn configure_trace_watches(&mut self, watches: Vec<TraceWatchSpec>) {
        self.unicorn.get_data_mut().trace_watches = watches;
    }

    pub fn add_trace_watch(&mut self, watch: TraceWatchSpec) {
        self.unicorn.get_data_mut().trace_watches.push(watch);
    }

    pub fn begin_block_census(&mut self) -> Result<(), GuestError> {
        if self.census_hook.is_some() {
            return Err(GuestError::Callback(
                "guest block census is already active".into(),
            ));
        }
        self.unicorn.get_data_mut().census_blocks.clear();
        let hook = uc(
            "install guest block census",
            self.unicorn.add_block_hook(
                self.image_base,
                self.image_end - 1,
                |unicorn, address, size| {
                    *unicorn
                        .get_data_mut()
                        .census_blocks
                        .entry((address, size))
                        .or_default() += 1;
                },
            ),
        )?;
        self.census_hook = Some(hook);
        Ok(())
    }

    pub fn finish_block_census(&mut self, output_pixels: u64) -> Result<GuestCensus, GuestError> {
        let hook = self
            .census_hook
            .take()
            .ok_or_else(|| GuestError::Callback("guest block census is not active".into()))?;
        uc("remove guest block census", self.unicorn.remove_hook(hook))?;

        let counts = std::mem::take(&mut self.unicorn.get_data_mut().census_blocks);
        let mut blocks = Vec::with_capacity(counts.len());
        for ((address, size), executions) in counts {
            let mut bytes = vec![0u8; size as usize];
            uc(
                "read census block",
                self.unicorn.mem_read(address, &mut bytes),
            )?;
            let mut decoder = Decoder::with_ip(64, &bytes, address, DecoderOptions::NONE);
            let mut instructions = 0u32;
            let mut scalar_sse_fp_instructions = 0u32;
            while decoder.can_decode() {
                let instruction = decoder.decode();
                if instruction.is_invalid() {
                    break;
                }
                instructions += 1;
                if is_scalar_sse_fp(instruction.mnemonic()) {
                    scalar_sse_fp_instructions += 1;
                }
            }
            blocks.push(CensusBlock {
                address,
                rva: address - self.image_base,
                size_bytes: size,
                executions,
                instructions,
                dynamic_instructions: executions.saturating_mul(instructions as u64),
                scalar_sse_fp_instructions,
                dynamic_scalar_sse_fp_instructions: executions
                    .saturating_mul(scalar_sse_fp_instructions as u64),
            });
        }
        blocks.sort_by_key(|block| std::cmp::Reverse(block.dynamic_instructions));
        let estimated_dynamic_instructions = blocks
            .iter()
            .map(|block| block.dynamic_instructions)
            .sum::<u64>();
        let estimated_dynamic_scalar_sse_fp_instructions = blocks
            .iter()
            .map(|block| block.dynamic_scalar_sse_fp_instructions)
            .sum::<u64>();
        let dynamic_instructions_in_scalar_sse_blocks = blocks
            .iter()
            .filter(|block| block.scalar_sse_fp_instructions != 0)
            .map(|block| block.dynamic_instructions)
            .sum::<u64>();
        let fraction = |count: usize| {
            if estimated_dynamic_instructions == 0 {
                0.0
            } else {
                blocks
                    .iter()
                    .take(count)
                    .map(|block| block.dynamic_instructions)
                    .sum::<u64>() as f64
                    / estimated_dynamic_instructions as f64
            }
        };
        let blocks_for_80_percent = if estimated_dynamic_instructions == 0 {
            0
        } else {
            let mut cumulative = 0u64;
            blocks
                .iter()
                .position(|block| {
                    cumulative = cumulative.saturating_add(block.dynamic_instructions);
                    cumulative as f64 / estimated_dynamic_instructions as f64 >= 0.8
                })
                .map_or(blocks.len(), |index| index + 1)
        };
        let extents =
            coalesce_census_extents(&blocks, self.image_base, estimated_dynamic_instructions);
        let extent_fraction = |count: usize| {
            extents
                .iter()
                .take(count)
                .map(|extent| extent.dynamic_instruction_fraction)
                .sum()
        };
        Ok(GuestCensus {
            schema_version: 1,
            distinct_blocks: blocks.len(),
            total_block_executions: blocks.iter().map(|block| block.executions).sum(),
            estimated_dynamic_instructions,
            output_pixels,
            estimated_dynamic_instructions_per_pixel: if output_pixels == 0 {
                0.0
            } else {
                estimated_dynamic_instructions as f64 / output_pixels as f64
            },
            estimated_dynamic_scalar_sse_fp_instructions,
            scalar_sse_fp_fraction: if estimated_dynamic_instructions == 0 {
                0.0
            } else {
                estimated_dynamic_scalar_sse_fp_instructions as f64
                    / estimated_dynamic_instructions as f64
            },
            dynamic_instructions_in_scalar_sse_blocks,
            scalar_sse_block_work_fraction: if estimated_dynamic_instructions == 0 {
                0.0
            } else {
                dynamic_instructions_in_scalar_sse_blocks as f64
                    / estimated_dynamic_instructions as f64
            },
            top_1_dynamic_instruction_fraction: fraction(1),
            top_5_dynamic_instruction_fraction: fraction(5),
            top_20_dynamic_instruction_fraction: fraction(20),
            blocks_for_80_percent,
            blocks,
            distinct_extents: extents.len(),
            top_1_extent_dynamic_instruction_fraction: extent_fraction(1),
            top_2_extent_dynamic_instruction_fraction: extent_fraction(2),
            top_20_extent_dynamic_instruction_fraction: extent_fraction(20),
            extents,
        })
    }

    pub fn call_win64(&mut self, address: u64, args: [u64; 6]) -> Result<u64, GuestError> {
        self.call_win64_with_timeout(address, &args, TIMEOUT_MICROSECONDS)
    }

    pub fn call_selector_win64(&mut self, address: u64, args: [u64; 6]) -> Result<u64, GuestError> {
        {
            let state = self.unicorn.get_data_mut();
            state.selector_dispatch_active = true;
            state.pending_unsupported_suite = None;
            state.selector_abort = None;
        }
        let result = self.call_win64_with_timeout(address, &args, TIMEOUT_MICROSECONDS);
        {
            let state = self.unicorn.get_data_mut();
            state.selector_dispatch_active = false;
            state.pending_unsupported_suite = None;
            state.selector_abort = None;
        }
        result
    }

    fn call_win64_with_timeout(
        &mut self,
        address: u64,
        args: &[u64],
        timeout_microseconds: u64,
    ) -> Result<u64, GuestError> {
        if args.len() < 4 || args.len() > 16 {
            return Err(GuestError::Callback(format!(
                "Win64 call requires 4..=16 arguments, got {}",
                args.len()
            )));
        }
        self.unicorn.get_data_mut().avx_fallback_instructions = 0;
        self.unicorn.get_data_mut().avx_defined_ymm = [false; 16];
        self.unicorn.get_data_mut().latest_runtime_target = None;
        self.unicorn.get_data_mut().unsupported_import = None;
        let stack_top = STACK_BASE + STACK_SIZE;
        // Win64 function entry observes RSP % 16 == 8. Reserve a return
        // address, 32-byte shadow space, bounded stack arguments, and scratch.
        let rsp = (stack_top - 0x108) | 8;
        uc(
            "write return address",
            self.unicorn.mem_write(rsp, &RETURN_ADDRESS.to_le_bytes()),
        )?;
        for (index, value) in args.iter().copied().enumerate().skip(4) {
            uc(
                "write stack argument",
                self.unicorn
                    .mem_write(rsp + 0x28 + ((index - 4) * 8) as u64, &value.to_le_bytes()),
            )?;
        }
        for (register, value) in [
            (RegisterX86::RSP, rsp),
            (RegisterX86::RCX, args[0]),
            (RegisterX86::RDX, args[1]),
            (RegisterX86::R8, args[2]),
            (RegisterX86::R9, args[3]),
        ] {
            uc(
                "write argument register",
                self.unicorn.reg_write(register, value),
            )?;
        }
        if let Err(error) = self.unicorn.emu_start(
            address,
            RETURN_ADDRESS,
            timeout_microseconds,
            MAX_INSTRUCTIONS,
        ) {
            return Err(self.execution_crash(format!("emulation error: {error}")));
        }
        if let Some(abort) = self.unicorn.get_data_mut().selector_abort.take() {
            return Err(GuestError::SelectorAbort {
                error: abort.error,
                suite_name: abort.suite.name,
                suite_version: abort.suite.version,
                acquire_error: abort.suite.acquire_error,
            });
        }
        if let Some((library, symbol)) = self.unicorn.get_data_mut().unsupported_import.take() {
            return Err(GuestError::UnsupportedImport { library, symbol });
        }
        let rip = uc(
            "read instruction pointer",
            self.unicorn.reg_read(RegisterX86::RIP),
        )?;
        if let Some(error) = self.unicorn.get_data_mut().callback_error.take() {
            return Err(GuestError::Callback(error));
        }
        if rip != RETURN_ADDRESS {
            return Err(self.execution_crash(format!(
                "execution stopped before the guest returned (RIP={rip:#x})"
            )));
        }
        uc("read return value", self.unicorn.reg_read(RegisterX86::RAX))
    }

    fn execution_crash(&self, reason: String) -> GuestError {
        let registers = [
            ("rax", RegisterX86::RAX),
            ("rbx", RegisterX86::RBX),
            ("rcx", RegisterX86::RCX),
            ("rdx", RegisterX86::RDX),
            ("rsi", RegisterX86::RSI),
            ("rdi", RegisterX86::RDI),
            ("rbp", RegisterX86::RBP),
            ("rsp", RegisterX86::RSP),
            ("r8", RegisterX86::R8),
            ("r9", RegisterX86::R9),
            ("r10", RegisterX86::R10),
            ("r11", RegisterX86::R11),
            ("r12", RegisterX86::R12),
            ("r13", RegisterX86::R13),
            ("r14", RegisterX86::R14),
            ("r15", RegisterX86::R15),
            ("rip", RegisterX86::RIP),
            ("rflags", RegisterX86::EFLAGS),
        ]
        .into_iter()
        .map(|(name, register)| {
            (
                name.to_string(),
                self.unicorn.reg_read(register).unwrap_or(0),
            )
        })
        .collect::<BTreeMap<_, _>>();
        let xmm_registers = [
            ("xmm0", RegisterX86::XMM0),
            ("xmm1", RegisterX86::XMM1),
            ("xmm2", RegisterX86::XMM2),
            ("xmm3", RegisterX86::XMM3),
            ("xmm4", RegisterX86::XMM4),
            ("xmm5", RegisterX86::XMM5),
            ("xmm6", RegisterX86::XMM6),
            ("xmm7", RegisterX86::XMM7),
            ("xmm8", RegisterX86::XMM8),
            ("xmm9", RegisterX86::XMM9),
            ("xmm10", RegisterX86::XMM10),
            ("xmm11", RegisterX86::XMM11),
            ("xmm12", RegisterX86::XMM12),
            ("xmm13", RegisterX86::XMM13),
            ("xmm14", RegisterX86::XMM14),
            ("xmm15", RegisterX86::XMM15),
        ]
        .into_iter()
        .filter_map(|(name, register)| read_xmm(&self.unicorn, name, register))
        .collect();
        let call_stack = self
            .unicorn
            .get_data()
            .trace
            .as_ref()
            .map(|capture| {
                capture
                    .function_stack
                    .iter()
                    .enumerate()
                    .map(|(index, function_rva)| TraceCrashFrame {
                        call_id: index
                            .checked_sub(1)
                            .and_then(|index| capture.call_id_stack.get(index).copied()),
                        function_rva: *function_rva,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let rip = *registers.get("rip").unwrap_or(&0);
        let mut instruction = [0; 32];
        let instruction_bytes = if self.unicorn.mem_read(rip, &mut instruction).is_ok() {
            bytes_to_hex(&instruction)
        } else {
            String::new()
        };
        let runtime_target = self
            .unicorn
            .get_data()
            .latest_runtime_target
            .as_ref()
            .filter(|target| target.source_address == rip || target.effective_target == Some(rip))
            .cloned();
        let snapshot = TraceCrashSnapshot {
            reason: reason.clone(),
            registers,
            xmm_registers,
            call_stack,
            instruction_address: rip,
            instruction_rva: (self.image_base..self.image_end)
                .contains(&rip)
                .then(|| rip - self.image_base),
            instruction_bytes,
            runtime_target,
            handle_allocations: self.unicorn.get_data().handle_allocations.clone(),
            handle_allocation_failures: self.unicorn.get_data().handle_allocation_failures.clone(),
            live_handle_count: self.unicorn.get_data().handles.len(),
            next_pf_handle_data: self.unicorn.get_data().next_pf_handle_data,
            pf_handle_data_end: PF_HANDLE_DATA_END,
        };
        GuestError::ExecutionCrash {
            reason,
            snapshot_json: serde_json::to_string(&snapshot)
                .unwrap_or_else(|error| format!("{{\"serialization_error\":\"{error}\"}}")),
            snapshot: Box::new(snapshot),
        }
    }

    pub fn allocate(&mut self, size: usize, alignment: u64) -> Result<u64, GuestError> {
        let alignment = alignment.max(1).next_power_of_two();
        let start = self
            .next_data
            .checked_add(alignment - 1)
            .map(|value| value & !(alignment - 1))
            .ok_or(GuestError::DataCapacity)?;
        let end = start
            .checked_add(u64::try_from(size).map_err(|_| GuestError::DataCapacity)?)
            .ok_or(GuestError::DataCapacity)?;
        if end > HANDLE_DATA_BASE {
            return Err(GuestError::DataCapacity);
        }
        self.next_data = end;
        Ok(start)
    }

    pub fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), GuestError> {
        uc("write guest data", self.unicorn.mem_write(address, bytes))
    }

    pub fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), GuestError> {
        uc("read guest data", self.unicorn.mem_read(address, bytes))
    }

    pub fn write_u64(&mut self, address: u64, value: u64) -> Result<(), GuestError> {
        self.write(address, &value.to_le_bytes())
    }

    pub fn add_param_callback_address(&self) -> u64 {
        HOST_ADD_PARAM
    }

    pub fn poison_callback_address(&self) -> u64 {
        HOST_POISON
    }

    pub fn ansi_strcpy_callback_address(&self) -> u64 {
        HOST_ANSI_STRCPY
    }

    pub fn ansi_sprintf_callback_address(&self) -> u64 {
        HOST_PF_ANSI_SPRINTF
    }

    pub fn copy_callback_address(&self) -> u64 {
        HOST_COPY
    }

    pub fn blend_callback_address(&self) -> u64 {
        HOST_BLEND
    }

    pub fn noop_callback_address(&self) -> u64 {
        HOST_NOOP
    }

    pub fn acquire_suite_callback_address(&self) -> u64 {
        HOST_ACQUIRE_SUITE
    }

    pub fn checkout_param_callback_address(&self) -> u64 {
        HOST_CHECKOUT_PARAM
    }

    pub fn checkin_param_callback_address(&self) -> u64 {
        HOST_CHECKIN_PARAM
    }

    pub fn extended_alloc_callback_address(&self) -> u64 {
        HOST_EXTENDED_ALLOC
    }

    pub fn extended_free_callback_address(&self) -> u64 {
        HOST_EXTENDED_FREE
    }

    pub fn extended_lookup_callback_address(&self) -> u64 {
        HOST_EXTENDED_LOOKUP
    }

    pub fn iterate8_callback_address(&self) -> u64 {
        HOST_ITERATE8
    }

    pub fn iterate8_origin_callback_address(&self) -> u64 {
        HOST_ITERATE8_ORIGIN
    }

    pub fn iterate16_callback_address(&self) -> u64 {
        HOST_ITERATE16
    }

    pub fn fill8_callback_address(&self) -> u64 {
        HOST_FILL8
    }

    pub fn subpixel_sample8_callback_address(&self) -> u64 {
        HOST_SUBPIXEL_SAMPLE8
    }

    pub fn area_sample8_callback_address(&self) -> u64 {
        HOST_AREA_SAMPLE8
    }

    pub fn transfer_rect8_callback_address(&self) -> u64 {
        HOST_TRANSFER_RECT8
    }

    pub fn new_world8_callback_address(&self) -> u64 {
        HOST_NEW_WORLD8
    }

    pub fn dispose_world_callback_address(&self) -> u64 {
        HOST_DISPOSE_WORLD
    }

    pub fn get_callback_addr_callback_address(&self) -> u64 {
        HOST_GET_CALLBACK_ADDR
    }

    pub fn ansi_ceil_callback_address(&self) -> u64 {
        HOST_PF_ANSI_CEIL
    }

    pub fn ansi_cos_callback_address(&self) -> u64 {
        HOST_PF_ANSI_COS
    }

    pub fn ansi_fabs_callback_address(&self) -> u64 {
        HOST_PF_ANSI_FABS
    }

    pub fn ansi_hypot_callback_address(&self) -> u64 {
        HOST_PF_ANSI_HYPOT
    }

    pub fn ansi_pow_callback_address(&self) -> u64 {
        HOST_PF_ANSI_POW
    }

    pub fn ansi_sin_callback_address(&self) -> u64 {
        HOST_PF_ANSI_SIN
    }

    pub fn ansi_sqrt_callback_address(&self) -> u64 {
        HOST_PF_ANSI_SQRT
    }

    pub fn ansi_asin_callback_address(&self) -> u64 {
        HOST_PF_ANSI_ASIN
    }

    pub fn ansi_acos_callback_address(&self) -> u64 {
        HOST_PF_ANSI_ACOS
    }

    pub fn configure_parameter_definitions(
        &mut self,
        input_definition: u64,
        definitions: Vec<u64>,
    ) -> Result<(), GuestError> {
        if input_definition == 0 {
            return Err(GuestError::Callback(
                "active input parameter definition is null".into(),
            ));
        }
        let mut input_bytes = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        self.unicorn
            .mem_read(input_definition, &mut input_bytes)
            .map_err(|error| GuestError::Unicorn {
                operation: "read active input parameter definition",
                detail: error.to_string(),
            })?;
        if definitions.len() != self.unicorn.get_data().params.len() {
            return Err(GuestError::Callback(
                "active parameter definition count differs from setup".into(),
            ));
        }
        let mut active_colors = Vec::with_capacity(definitions.len());
        for (definition, parameter) in definitions
            .iter()
            .copied()
            .zip(self.unicorn.get_data().params.iter())
        {
            let mut color = [0u8; abi::PF_PIXEL_SIZE];
            if parameter.param_type == 5 {
                self.unicorn
                    .mem_read(definition + abi::PARAM_U_OFFSET as u64, &mut color)
                    .map_err(|error| GuestError::Unicorn {
                        operation: "read active color parameter",
                        detail: error.to_string(),
                    })?;
                active_colors.push(Some(color));
            } else {
                active_colors.push(None);
            }
        }
        let state = self.unicorn.get_data_mut();
        for (parameter, active) in state.params.iter_mut().zip(active_colors) {
            if let Some(color) = active {
                parameter.bytes[abi::PARAM_U_OFFSET..abi::PARAM_U_OFFSET + abi::PF_PIXEL_SIZE]
                    .copy_from_slice(&color);
            }
        }
        state.input_parameter_definition = input_definition;
        state.parameter_definitions = definitions;
        Ok(())
    }

    pub fn suite_requests(&self) -> &[String] {
        &self.unicorn.get_data().suite_requests
    }

    pub fn opencl_bridge_evidence(&self) -> OpenClBridgeEvidence {
        self.unicorn.get_data().gpu_runtime.opencl_evidence()
    }

    pub fn unsupported_suite_calls(&self) -> &[UnsupportedSuiteCall] {
        &self.unicorn.get_data().unsupported_suite_calls
    }

    pub fn dropped_unsupported_suite_calls(&self) -> u64 {
        self.unicorn.get_data().dropped_unsupported_suite_calls
    }

    pub fn smart_callback_counts(&self) -> (u32, u32, u32) {
        let state = self.unicorn.get_data();
        (
            state.pre_checkout_calls,
            state.checkout_pixels_calls,
            state.checkout_output_calls,
        )
    }

    pub fn pre_checkout_requests(&self) -> &[[i32; 4]] {
        &self.unicorn.get_data().pre_checkout_requests
    }

    pub fn handle_allocations(&self) -> &[u64] {
        &self.unicorn.get_data().handle_allocations
    }

    pub fn pre_checkout_layer_callback_address(&self) -> u64 {
        HOST_PRE_CHECKOUT_LAYER
    }

    pub fn checkout_layer_pixels_callback_address(&self) -> u64 {
        HOST_CHECKOUT_LAYER_PIXELS
    }

    pub fn checkin_layer_pixels_callback_address(&self) -> u64 {
        HOST_CHECKIN_LAYER_PIXELS
    }

    pub fn checkout_output_callback_address(&self) -> u64 {
        HOST_CHECKOUT_OUTPUT
    }

    pub fn new_handle_callback_address(&self) -> u64 {
        HOST_NEW_HANDLE
    }

    pub fn lock_handle_callback_address(&self) -> u64 {
        HOST_LOCK_HANDLE
    }

    pub fn unlock_handle_callback_address(&self) -> u64 {
        HOST_UNLOCK_HANDLE
    }

    pub fn dispose_handle_callback_address(&self) -> u64 {
        HOST_DISPOSE_HANDLE
    }

    pub fn handle_size_callback_address(&self) -> u64 {
        HOST_HANDLE_SIZE
    }

    pub fn resize_handle_callback_address(&self) -> u64 {
        HOST_RESIZE_HANDLE
    }

    pub fn configure_smart_render(
        &mut self,
        input_world: u64,
        output_world: u64,
        width: u32,
        height: u32,
        pixel_format: i32,
        current_time: i32,
        current_time_scale: u32,
    ) {
        let state = self.unicorn.get_data_mut();
        state.pre_checkout_requests.clear();
        state.smart_checkout_ids.clear();
        state.smart_input_world = input_world;
        state.smart_output_world = output_world;
        state.smart_width = width;
        state.smart_height = height;
        state.smart_pixel_format = pixel_format;
        state.render_pixel_format = pixel_format;
        state.smart_current_time = current_time;
        state.smart_current_time_scale = current_time_scale;
    }

    pub fn configure_render_pixel_format(&mut self, pixel_format: i32) {
        self.unicorn.get_data_mut().render_pixel_format = pixel_format;
    }

    pub fn finish_smart_checkout_scope(&mut self) -> bool {
        let state = self.unicorn.get_data_mut();
        let balanced = state
            .smart_checkout_ids
            .values()
            .all(|checkout| !checkout.checked_out);
        state.smart_checkout_ids.clear();
        balanced
    }

    pub fn parameters(&self) -> &[GuestParam] {
        &self.unicorn.get_data().params
    }
}
