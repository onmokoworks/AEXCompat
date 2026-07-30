use aex_apple_opencl::{
    BufferAccess, Error, MAX_BUFFER_BYTES, MAX_KERNEL_ARGUMENT_BYTES, ObjectCounts, Session,
    enumerate_gpu_devices,
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn executes_real_kernel_on_apple_gpu_and_releases_every_object() {
    let devices = enumerate_gpu_devices().expect("enumerate Apple OpenCL GPUs");
    let device = devices.first().expect("at least one Apple GPU");
    assert!(!device.platform_name().is_empty());
    assert!(!device.name().is_empty());
    assert!(device.compute_units() > 0);
    eprintln!(
        "OpenCL GPU: platform={:?} vendor={:?} device={:?} compute_units={}",
        device.platform_name(),
        device.vendor(),
        device.name(),
        device.compute_units()
    );

    let session = Session::select_gpu(device.ordinal()).expect("select Apple GPU");
    assert_eq!(session.device(), device);
    let tracker = session.object_tracker();
    assert_eq!(
        tracker.snapshot(),
        ObjectCounts {
            contexts: 1,
            command_queues: 1,
            ..ObjectCounts::default()
        }
    );

    const ITEM_COUNT: usize = 1024;
    let input: Vec<f32> = (0..ITEM_COUNT).map(|index| index as f32 * 0.25).collect();
    let input_bytes: Vec<u8> = input.iter().flat_map(|value| value.to_ne_bytes()).collect();
    let mut output_bytes = vec![0u8; ITEM_COUNT * size_of::<f32>()];
    {
        let input_buffer = session
            .create_buffer(input_bytes.len(), BufferAccess::ReadOnly)
            .expect("create input buffer");
        let output_buffer = session
            .create_buffer(output_bytes.len(), BufferAccess::WriteOnly)
            .expect("create output buffer");
        session
            .write_buffer(&input_buffer, 0, &input_bytes)
            .expect("upload input");

        let program = session
            .build_program(
                r#"
                    __kernel void affine(
                        __global const float *input,
                        __global float *output,
                        float scale,
                        float bias
                    ) {
                        size_t index = get_global_id(0);
                        output[index] = input[index] * scale + bias;
                    }
                "#,
                None,
            )
            .expect("build test kernel");
        let mut kernel = program.create_kernel("affine").expect("create kernel");
        kernel.set_buffer_arg(0, &input_buffer).expect("set input");
        kernel
            .set_buffer_arg(1, &output_buffer)
            .expect("set output");
        kernel
            .set_raw_arg(2, &2.5f32.to_ne_bytes())
            .expect("set raw scale");
        kernel.set_scalar_arg(3, -3.0f32).expect("set bias");

        session
            .enqueue_nd_range_with_offset(&kernel, Some(&[0]), &[ITEM_COUNT], Some(&[64]))
            .expect("enqueue kernel");
        session.finish().expect("finish queue");
        session
            .read_buffer(&output_buffer, 0, &mut output_bytes)
            .expect("download output");

        let counts = tracker.snapshot();
        assert_eq!(counts.buffers, 2);
        assert_eq!(counts.programs, 1);
        assert_eq!(counts.kernels, 1);
        assert_eq!(counts.release_errors, 0);
    }

    let output: Vec<f32> = output_bytes
        .chunks_exact(size_of::<f32>())
        .map(|bytes| f32::from_ne_bytes(bytes.try_into().expect("one f32")))
        .collect();
    for (index, actual) in output.iter().copied().enumerate() {
        let expected = input[index] * 2.5 - 3.0;
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "index {index}: {actual} != {expected}"
        );
    }
    assert_eq!(
        tracker.snapshot(),
        ObjectCounts {
            contexts: 1,
            command_queues: 1,
            ..ObjectCounts::default()
        }
    );
    drop(session);
    assert_eq!(tracker.snapshot(), ObjectCounts::default());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn rejects_bounds_and_build_errors_without_leaking_objects() {
    let devices = enumerate_gpu_devices().expect("enumerate Apple OpenCL GPUs");
    assert_eq!(
        Session::select_gpu(devices.len())
            .err()
            .expect("out-of-range selection must fail"),
        Error::DeviceIndexOutOfRange {
            requested: devices.len(),
            available: devices.len(),
        }
    );

    let session = Session::select_gpu(0).expect("select Apple GPU");
    let tracker = session.object_tracker();
    assert!(matches!(
        session.create_buffer(MAX_BUFFER_BYTES + 1, BufferAccess::ReadWrite),
        Err(Error::LimitExceeded {
            resource: "buffer bytes",
            ..
        })
    ));
    assert_eq!(tracker.snapshot().buffers, 0);

    {
        let buffer = session
            .create_buffer(16, BufferAccess::ReadWrite)
            .expect("small buffer");
        assert!(matches!(
            session.write_buffer(&buffer, 15, &[1, 2]),
            Err(Error::BufferRange {
                operation: "buffer write",
                ..
            })
        ));
        assert_eq!(tracker.snapshot().buffers, 1);
    }
    assert_eq!(tracker.snapshot().buffers, 0);

    let error = session
        .build_program("__kernel void deliberately_broken(", None)
        .err()
        .expect("invalid source must fail");
    match error {
        Error::ProgramBuild { code, log } => {
            assert_ne!(code, 0);
            assert!(!log.is_empty(), "Apple OpenCL should return a build log");
        }
        other => panic!("unexpected error: {other:?}"),
    }
    let counts = tracker.snapshot();
    assert_eq!(counts.programs, 0);
    assert_eq!(counts.kernels, 0);
    assert_eq!(counts.release_errors, 0);

    let program = session
        .build_program("__kernel void one_arg(int value) {}", None)
        .expect("build argument-bound test kernel");
    let mut kernel = program.create_kernel("one_arg").expect("create kernel");
    assert_eq!(
        session
            .enqueue_nd_range_with_offset(&kernel, Some(&[0, 0]), &[1], None)
            .unwrap_err(),
        Error::GlobalOffsetDimensionMismatch
    );
    assert_eq!(
        kernel.set_raw_arg(0, &[]).unwrap_err(),
        Error::ZeroKernelArgumentSize
    );
    assert!(matches!(
        kernel.set_raw_arg(0, &vec![0; MAX_KERNEL_ARGUMENT_BYTES + 1]),
        Err(Error::LimitExceeded {
            resource: "kernel argument bytes",
            ..
        })
    ));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unsupported_platform_fails_explicitly() {
    assert_eq!(
        enumerate_gpu_devices().unwrap_err(),
        Error::UnsupportedPlatform
    );
    assert_eq!(
        Session::select_gpu(0)
            .err()
            .expect("unsupported platform must fail"),
        Error::UnsupportedPlatform
    );
}
