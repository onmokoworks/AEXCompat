#[test]
fn gpu_device_suite_v1_acquires_exact_table_and_writes_56_byte_device_info() {
    assert_eq!(PF_ERR_OUT_OF_MEMORY, 4);
    assert_eq!(PF_ERR_BAD_CALLBACK_PARAM, 516);
    let mut engine = test_engine(&[0xc3]);
    let tokens = engine
        .unicorn
        .get_data_mut()
        .gpu_runtime
        .begin_mock(0)
        .unwrap();
    let name = engine.allocate(20, 1).unwrap();
    engine.write(name, b"PF GPU Device Suite\0").unwrap();
    let suite_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [name, 1, suite_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(suite_output, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), HOST_GPU_DEVICE_SUITE_V1);

    let mut table = [0u8; 15 * 8];
    engine.read(HOST_GPU_DEVICE_SUITE_V1, &mut table).unwrap();
    let callbacks = table
        .chunks_exact(8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(callbacks, HOST_GPU_SUITE_CALLBACKS);

    let count_output = engine.allocate(4, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[0], [1, count_output, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut count = [0u8; 4];
    engine.read(count_output, &mut count).unwrap();
    assert_eq!(u32::from_le_bytes(count), 1);

    let info_output = engine.allocate(PF_GPU_DEVICE_INFO_SIZE, 8).unwrap();
    engine
        .write(info_output, &[0xff; PF_GPU_DEVICE_INFO_SIZE])
        .unwrap();
    assert_eq!(
        engine
            .call_win64(callbacks[1], [1, 0, info_output, 0, 0, 0])
            .unwrap(),
        0
    );
    let mut info = [0u8; PF_GPU_DEVICE_INFO_SIZE];
    engine.read(info_output, &mut info).unwrap();
    assert_eq!(i32::from_le_bytes(info[0..4].try_into().unwrap()), 1);
    assert_eq!(info[4], 1);
    assert_eq!(
        u64::from_le_bytes(info[8..16].try_into().unwrap()),
        tokens.platform
    );
    assert_eq!(
        u64::from_le_bytes(info[16..24].try_into().unwrap()),
        tokens.device
    );
    assert_eq!(
        u64::from_le_bytes(info[24..32].try_into().unwrap()),
        tokens.context
    );
    assert_eq!(
        u64::from_le_bytes(info[32..40].try_into().unwrap()),
        tokens.queue
    );
    assert_eq!(&info[40..56], &[0; 16]);
    assert_eq!(
        engine
            .call_win64(callbacks[1], [1, 1, info_output, 0, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );

    engine.write(suite_output, &u64::MAX.to_le_bytes()).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_ACQUIRE_SUITE, [name, 2, suite_output, 0, 0, 0])
            .unwrap(),
        u32::MAX as u64
    );
    engine.read(suite_output, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), 0);
}

#[test]
fn gpu_device_and_host_allocations_are_distinct_bounded_and_stale_safe() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .get_data_mut()
        .gpu_runtime
        .begin_mock(0)
        .unwrap();
    let output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_GPU_ALLOCATE_DEVICE, [1, 0, 64, output, 0, 0])
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(output, &mut pointer).unwrap();
    let device = u64::from_le_bytes(pointer);
    assert!(GpuRuntime::looks_like_token(device));
    assert!(
        engine
            .unicorn
            .get_data()
            .gpu_runtime
            .is_buffer_token(device)
    );
    assert!(engine.unicorn.mem_read_as_vec(device, 1).is_err());
    assert_eq!(
        engine
            .call_win64(HOST_GPU_FREE_DEVICE, [1, 1, device, 0, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .gpu_runtime
            .is_buffer_token(device)
    );
    assert_eq!(
        engine
            .call_win64(HOST_GPU_FREE_DEVICE, [1, 0, device, 0, 0, 0])
            .unwrap(),
        0
    );
    assert_eq!(
        engine
            .call_win64(HOST_GPU_FREE_DEVICE, [1, 0, device, 0, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );

    assert_eq!(
        engine
            .call_win64(HOST_GPU_ALLOCATE_HOST, [1, 0, 64, output, 0, 0])
            .unwrap(),
        0
    );
    engine.read(output, &mut pointer).unwrap();
    let host = u64::from_le_bytes(pointer);
    assert!(!GpuRuntime::looks_like_token(host));
    engine.write(host, b"host-visible").unwrap();
    let mut visible = [0u8; 12];
    engine.read(host, &mut visible).unwrap();
    assert_eq!(&visible, b"host-visible");
    assert_eq!(
        engine
            .call_win64(HOST_GPU_FREE_HOST, [1, 1, host, 0, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );
    assert_eq!(
        engine
            .call_win64(HOST_GPU_FREE_HOST, [1, 0, host, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(engine.unicorn.mem_read_as_vec(host, 1).is_err());
    assert_eq!(
        engine
            .call_win64(HOST_GPU_FREE_HOST, [1, 0, host, 0, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );
    assert_eq!(
        engine
            .call_win64(HOST_GPU_ALLOCATE_DEVICE, [1, 0, 0, output, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );
    let evidence = engine.gpu_suite_evidence();
    assert_eq!(evidence.allocations_created, 2);
    assert_eq!(evidence.allocations_freed, 2);
    assert_eq!(evidence.live_host_allocations, 0);
    assert_eq!(evidence.live_device_allocations, 0);
    assert!(evidence.invalid_operations >= 5);
}

#[test]
fn gpu_callback_error_codes_match_the_after_effects_sdk() {
    let mut engine = test_engine(&[0xc3]);
    finish_gpu_callback(
        &mut engine.unicorn,
        Err(GpuCallbackFailure::out_of_memory("test out of memory")),
    );
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        4,
        "PF_Err_OUT_OF_MEMORY"
    );
    finish_gpu_callback(
        &mut engine.unicorn,
        Err(GpuCallbackFailure::bad("test bad callback parameter")),
    );
    assert_eq!(
        engine.unicorn.reg_read(RegisterX86::RAX).unwrap(),
        516,
        "PF_Err_BAD_CALLBACK_PARAM"
    );
}

#[test]
fn gpu_world_roundtrip_uses_buffer_tokens_and_cleans_every_failure_path() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .get_data_mut()
        .gpu_runtime
        .begin_mock(0)
        .unwrap();
    let world_output = engine.allocate(8, 8).unwrap();
    let scale = 1u64 | (1u64 << 32);
    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_GPU_CREATE_WORLD,
                &[
                    1,
                    0,
                    3,
                    2,
                    scale,
                    0,
                    PF_PIXEL_FORMAT_GPU_BGRA128 as u32 as u64,
                    0xa5a5_a5a5_a5a5_a501,
                    world_output,
                ],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        0
    );
    let mut pointer = [0u8; 8];
    engine.read(world_output, &mut pointer).unwrap();
    let world = u64::from_le_bytes(pointer);
    let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
    engine.read(world, &mut definition).unwrap();
    let token = u64::from_le_bytes(
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    assert!(engine.unicorn.get_data().gpu_runtime.is_buffer_token(token));
    assert_eq!(
        i32::from_le_bytes(
            definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
                .try_into()
                .unwrap()
        ),
        48
    );
    assert_eq!(
        i32::from_le_bytes(
            definition[abi::LAYER_PIX_ASPECT_RATIO_OFFSET
                ..abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 4]
                .try_into()
                .unwrap()
        ),
        1
    );
    assert_eq!(
        u32::from_le_bytes(
            definition[abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 4
                ..abi::LAYER_PIX_ASPECT_RATIO_OFFSET + 8]
                .try_into()
                .unwrap()
        ),
        1
    );
    let mut device_bytes = vec![0xff; 96];
    engine
        .unicorn
        .get_data()
        .gpu_runtime
        .read_device(token, 0, &mut device_bytes)
        .unwrap();
    assert_eq!(device_bytes, vec![0; 96]);

    let data_output = engine.allocate(8, 8).unwrap();
    let size_output = engine.allocate(8, 8).unwrap();
    let index_output = engine.allocate(4, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_GPU_GET_WORLD_DATA, [1, world, data_output, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.read(data_output, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), token);
    assert_eq!(
        engine
            .call_win64(HOST_GPU_GET_WORLD_SIZE, [1, world, size_output, 0, 0, 0])
            .unwrap(),
        0
    );
    engine.read(size_output, &mut pointer).unwrap();
    assert_eq!(u64::from_le_bytes(pointer), 96);
    assert_eq!(
        engine
            .call_win64(
                HOST_GPU_GET_WORLD_DEVICE_INDEX,
                [1, world, index_output, 0, 0, 0]
            )
            .unwrap(),
        0
    );
    let mut index = [0u8; 4];
    engine.read(index_output, &mut index).unwrap();
    assert_eq!(u32::from_le_bytes(index), 0);

    let format_output = engine.allocate(4, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_GET_WORLD_PIXEL_FORMAT,
                [world, format_output, 0, 0, 0, 0]
            )
            .unwrap(),
        0
    );
    engine.read(format_output, &mut index).unwrap();
    assert_eq!(i32::from_le_bytes(index), PF_PIXEL_FORMAT_GPU_BGRA128);
    assert_eq!(
        engine
            .call_win64(HOST_GPU_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
            .unwrap(),
        0
    );
    assert!(!engine.unicorn.get_data().gpu_runtime.is_buffer_token(token));
    assert!(engine.unicorn.mem_read_as_vec(world, 1).is_err());
    assert_eq!(
        engine
            .call_win64(HOST_GPU_DISPOSE_WORLD, [1, world, 0, 0, 0, 0])
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );

    assert_eq!(
        engine
            .call_win64_with_timeout(
                HOST_GPU_CREATE_WORLD,
                &[
                    1,
                    0,
                    (GPU_MAX_WORLD_DIMENSION + 1) as u64,
                    2,
                    scale,
                    0,
                    PF_PIXEL_FORMAT_GPU_BGRA128 as u32 as u64,
                    0,
                    world_output,
                ],
                TIMEOUT_MICROSECONDS,
            )
            .unwrap(),
        PF_ERR_BAD_CALLBACK_PARAM
    );
    let evidence = engine.gpu_suite_evidence();
    assert_eq!(evidence.allocations_created, 1);
    assert_eq!(evidence.allocations_freed, 1);
    assert_eq!(evidence.worlds_created, 1);
    assert_eq!(evidence.worlds_disposed, 1);
    assert_eq!(evidence.live_gpu_worlds, 0);
    assert_eq!(evidence.live_device_allocations, 0);
    assert_eq!(evidence.live_bytes, 0);
}

#[test]
fn opencl_shutdown_rejects_live_or_release_failed_native_objects() {
    for counts in [
        ObjectCounts {
            buffers: 1,
            ..ObjectCounts::default()
        },
        ObjectCounts {
            release_errors: 1,
            ..ObjectCounts::default()
        },
    ] {
        let error = finish_opencl_shutdown(counts, None).unwrap_err();
        assert!(error
            .to_string()
            .contains("OpenCL runtime cleanup is unbalanced"));
    }
    assert_eq!(
        finish_opencl_shutdown(ObjectCounts::default(), None).unwrap(),
        ObjectCounts::default()
    );
}

#[test]
fn opencl_shutdown_reports_unfreed_device_suite_allocation_and_deactivates_runtime() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .get_data_mut()
        .gpu_runtime
        .begin_mock(0)
        .unwrap();
    let output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(HOST_GPU_ALLOCATE_DEVICE, [1, 0, 64, output, 0, 0])
            .unwrap(),
        0
    );
    let before = engine.gpu_suite_evidence();
    assert_eq!(before.allocations_created, 1);
    assert_eq!(before.allocations_freed, 0);
    assert_eq!(before.live_device_allocations, 1);
    assert!(!before.cleanup_balanced);

    let error = engine.end_opencl_gpu().unwrap_err();
    assert!(error
        .to_string()
        .contains("GPU Device Suite cleanup is unbalanced"));
    assert!(!engine.unicorn.get_data().gpu_runtime.is_active());
    let after = engine.gpu_suite_evidence();
    assert_eq!(after.allocations_created, 1);
    assert_eq!(after.allocations_freed, 0);
    assert_eq!(after.live_device_allocations, 0);
    assert_eq!(after.live_bytes, 0);
    assert!(!after.cleanup_balanced);
}

#[test]
fn gpu_render_transport_swaps_bgra_tokens_and_restores_argb32f_worlds() {
    let mut engine = test_engine(&[0xc3]);
    engine
        .unicorn
        .get_data_mut()
        .gpu_runtime
        .begin_mock(0)
        .unwrap();
    let input_data = engine.allocate(32, 16).unwrap();
    let output_data = engine.allocate(32, 16).unwrap();
    let input_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let output_world = engine.allocate(abi::PF_LAYER_DEF_SIZE, 8).unwrap();
    let mut input = Vec::new();
    for channels in [[0.4f32, 0.3, 0.2, 0.1], [0.8, 0.7, 0.6, 0.5]] {
        for channel in channels {
            input.extend_from_slice(&channel.to_le_bytes());
        }
    }
    engine.write(input_data, &input).unwrap();
    engine.write(output_data, &[0; 32]).unwrap();
    for (world, data) in [(input_world, input_data), (output_world, output_data)] {
        let mut definition = vec![0u8; abi::PF_LAYER_DEF_SIZE];
        definition[abi::LAYER_DATA_OFFSET..abi::LAYER_DATA_OFFSET + 8]
            .copy_from_slice(&data.to_le_bytes());
        definition[abi::LAYER_ROWBYTES_OFFSET..abi::LAYER_ROWBYTES_OFFSET + 4]
            .copy_from_slice(&32i32.to_le_bytes());
        definition[abi::LAYER_WIDTH_OFFSET..abi::LAYER_WIDTH_OFFSET + 4]
            .copy_from_slice(&2i32.to_le_bytes());
        definition[abi::LAYER_HEIGHT_OFFSET..abi::LAYER_HEIGHT_OFFSET + 4]
            .copy_from_slice(&1i32.to_le_bytes());
        engine.write(world, &definition).unwrap();
    }
    engine.configure_smart_render(
        input_world,
        output_world,
        2,
        1,
        crate::pixel::PF_PIXEL_FORMAT_ARGB128,
        0,
        1,
    );
    engine.prepare_gpu_render_transport().unwrap();
    assert!(engine.prepare_gpu_render_transport().is_err());
    let input_token = read_guest_u64(
        &engine.unicorn,
        input_world + abi::LAYER_DATA_OFFSET as u64,
        "test input token",
    )
    .unwrap();
    let output_token = read_guest_u64(
        &engine.unicorn,
        output_world + abi::LAYER_DATA_OFFSET as u64,
        "test output token",
    )
    .unwrap();
    assert_ne!(input_token, input_data);
    assert_ne!(output_token, output_data);
    assert!(
        engine
            .unicorn
            .get_data()
            .gpu_runtime
            .is_buffer_token(input_token)
    );
    assert!(
        engine
            .unicorn
            .get_data()
            .gpu_runtime
            .is_buffer_token(output_token)
    );

    let mut uploaded = vec![0u8; 32];
    engine
        .unicorn
        .get_data()
        .gpu_runtime
        .read_device(input_token, 0, &mut uploaded)
        .unwrap();
    let uploaded_f32 = uploaded
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(uploaded_f32, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]);

    let gpu_data_output = engine.allocate(8, 8).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_GPU_GET_WORLD_DATA,
                [1, input_world, gpu_data_output, 0, 0, 0]
            )
            .unwrap(),
        0
    );
    let format_output = engine.allocate(4, 4).unwrap();
    assert_eq!(
        engine
            .call_win64(
                HOST_GET_WORLD_PIXEL_FORMAT,
                [output_world, format_output, 0, 0, 0, 0]
            )
            .unwrap(),
        0
    );
    let mut format = [0u8; 4];
    engine.read(format_output, &mut format).unwrap();
    assert_eq!(i32::from_le_bytes(format), PF_PIXEL_FORMAT_GPU_BGRA128);

    let mut rendered = Vec::new();
    for channels in [[0.11f32, 0.22, 0.33, 0.44], [0.55, 0.66, 0.77, 0.88]] {
        for channel in channels {
            rendered.extend_from_slice(&channel.to_le_bytes());
        }
    }
    engine
        .unicorn
        .get_data()
        .gpu_runtime
        .write_device(output_token, 0, &rendered)
        .unwrap();
    engine.finish_gpu_render_transport().unwrap();
    assert!(engine.finish_gpu_render_transport().is_err());
    assert_eq!(
        read_guest_u64(
            &engine.unicorn,
            input_world + abi::LAYER_DATA_OFFSET as u64,
            "restored input",
        )
        .unwrap(),
        input_data
    );
    assert_eq!(
        read_guest_u64(
            &engine.unicorn,
            output_world + abi::LAYER_DATA_OFFSET as u64,
            "restored output",
        )
        .unwrap(),
        output_data
    );
    let mut output = vec![0u8; 32];
    engine.read(output_data, &mut output).unwrap();
    let output_f32 = output
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(output_f32, [0.44, 0.33, 0.22, 0.11, 0.88, 0.77, 0.66, 0.55]);
    let evidence = engine.gpu_suite_evidence();
    assert_eq!(evidence.upload_bytes, 32);
    assert_eq!(evidence.download_bytes, 32);
    assert_eq!(evidence.allocations_created, 2);
    assert_eq!(evidence.allocations_freed, 2);
    assert_eq!(evidence.live_device_allocations, 0);
    assert!(!evidence.transport_active);
    engine.end_opencl_gpu().unwrap();
    assert!(!engine.unicorn.get_data().gpu_runtime.is_active());
}
