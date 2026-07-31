const MAX_OPENCL_SOURCE_STRINGS: usize = 1_024;
const MAX_OPENCL_BUILD_DEVICES: usize = 1;
const MAX_OPENCL_WORK_DIMENSIONS: usize = 3;

fn install_opencl_import_bridge(
    unicorn: &mut Unicorn<'static, GuestState>,
    stub: u64,
    symbol: OpenClBridgeSymbol,
) -> Result<(), GuestError> {
    // The generic import stub starts with `xor eax,eax`; replace it so the
    // typed bridge return value survives until the guest's RET.
    uc(
        "write OpenCL bridge return",
        unicorn.mem_write(stub, &[0xc3]),
    )?;
    uc(
        "install OpenCL import bridge",
        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
            emulate_opencl_import(unicorn, symbol);
        }),
    )?;
    Ok(())
}

fn emulate_opencl_import(
    unicorn: &mut Unicorn<'_, GuestState>,
    symbol: OpenClBridgeSymbol,
) {
    let operation = opencl_operation_name(symbol);
    unicorn
        .get_data_mut()
        .gpu_runtime
        .record_opencl_api_call(operation);
    match symbol {
        OpenClBridgeSymbol::CreateProgramWithSource => {
            let errcode = opencl_argument(unicorn, 4, operation).unwrap_or(0);
            let result = emulate_cl_create_program_with_source(unicorn);
            complete_opencl_pointer_call(unicorn, operation, errcode, result);
        }
        OpenClBridgeSymbol::BuildProgram => {
            let result = emulate_cl_build_program(unicorn);
            complete_opencl_status_call(unicorn, operation, result);
        }
        OpenClBridgeSymbol::CreateKernel => {
            let errcode = opencl_argument(unicorn, 2, operation).unwrap_or(0);
            let result = emulate_cl_create_kernel(unicorn);
            complete_opencl_pointer_call(unicorn, operation, errcode, result);
        }
        OpenClBridgeSymbol::SetKernelArg => {
            let result = emulate_cl_set_kernel_arg(unicorn);
            complete_opencl_status_call(unicorn, operation, result);
        }
        OpenClBridgeSymbol::EnqueueNdRangeKernel => {
            let result = emulate_cl_enqueue_nd_range_kernel(unicorn);
            complete_opencl_status_call(unicorn, operation, result);
        }
        OpenClBridgeSymbol::ReleaseKernel => {
            let result = emulate_cl_release_kernel(unicorn);
            complete_opencl_status_call(unicorn, operation, result);
        }
    }
}

fn opencl_operation_name(symbol: OpenClBridgeSymbol) -> &'static str {
    match symbol {
        OpenClBridgeSymbol::CreateProgramWithSource => "clCreateProgramWithSource",
        OpenClBridgeSymbol::BuildProgram => "clBuildProgram",
        OpenClBridgeSymbol::CreateKernel => "clCreateKernel",
        OpenClBridgeSymbol::SetKernelArg => "clSetKernelArg",
        OpenClBridgeSymbol::EnqueueNdRangeKernel => "clEnqueueNDRangeKernel",
        OpenClBridgeSymbol::ReleaseKernel => "clReleaseKernel",
    }
}

fn emulate_cl_create_program_with_source(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<u64, OpenClRuntimeError> {
    let operation = "clCreateProgramWithSource";
    let context = opencl_argument(unicorn, 0, operation)?;
    let count = opencl_u32_argument(unicorn, 1, operation)? as usize;
    let strings = opencl_argument(unicorn, 2, operation)?;
    let lengths = opencl_argument(unicorn, 3, operation)?;
    if count == 0 || count > MAX_OPENCL_SOURCE_STRINGS {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!(
                "OpenCL source string count {count} is outside 1..={MAX_OPENCL_SOURCE_STRINGS}"
            ),
        ));
    }
    if strings == 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            "OpenCL source string pointer array is null",
        ));
    }

    let mut source = Vec::new();
    for index in 0..count {
        let pointer_address = checked_opencl_array_address(strings, index, size_of::<u64>())?;
        let pointer = read_opencl_guest_u64(
            unicorn,
            pointer_address,
            "OpenCL source string pointer",
        )?;
        if pointer == 0 {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_VALUE,
                format!("OpenCL source string {index} is null"),
            ));
        }
        let explicit_length = if lengths == 0 {
            0
        } else {
            let length_address = checked_opencl_array_address(lengths, index, size_of::<u64>())?;
            let raw = read_opencl_guest_u64(
                unicorn,
                length_address,
                "OpenCL source string length",
            )?;
            usize::try_from(raw).map_err(|_| {
                OpenClRuntimeError::new(
                    CL_INVALID_VALUE,
                    format!("OpenCL source string {index} length does not fit this host"),
                )
            })?
        };
        let remaining = aex_apple_opencl::MAX_PROGRAM_SOURCE_BYTES
            .checked_sub(source.len())
            .ok_or_else(|| {
                OpenClRuntimeError::new(
                    CL_INVALID_VALUE,
                    "OpenCL aggregate program source exceeds its bound",
                )
            })?;
        let bytes = if explicit_length == 0 {
            read_opencl_c_string_bytes(
                unicorn,
                pointer,
                remaining,
                "OpenCL source string",
            )?
        } else {
            if explicit_length > remaining {
                return Err(OpenClRuntimeError::new(
                    CL_INVALID_VALUE,
                    format!(
                        "OpenCL aggregate program source exceeds {} bytes",
                        aex_apple_opencl::MAX_PROGRAM_SOURCE_BYTES
                    ),
                ));
            }
            read_opencl_guest_bytes(
                unicorn,
                pointer,
                explicit_length,
                "OpenCL source string",
            )?
        };
        source.extend_from_slice(&bytes);
    }
    let source = String::from_utf8(source).map_err(|error| {
        OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!("OpenCL program source is not UTF-8 at byte {}", error.utf8_error().valid_up_to()),
        )
    })?;
    unicorn
        .get_data_mut()
        .gpu_runtime
        .stage_opencl_program(context, source, count)
}

fn emulate_cl_build_program(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), OpenClRuntimeError> {
    let operation = "clBuildProgram";
    let program = opencl_argument(unicorn, 0, operation)?;
    let device_count = opencl_u32_argument(unicorn, 1, operation)? as usize;
    let device_list = opencl_argument(unicorn, 2, operation)?;
    let options = opencl_argument(unicorn, 3, operation)?;
    let notify = opencl_argument(unicorn, 4, operation)?;
    let user_data = opencl_argument(unicorn, 5, operation)?;
    if notify != 0 || user_data != 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            "OpenCL asynchronous build callbacks are not supported by the bounded bridge",
        ));
    }
    if device_count > MAX_OPENCL_BUILD_DEVICES {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!(
                "OpenCL build device count {device_count} exceeds {MAX_OPENCL_BUILD_DEVICES}"
            ),
        ));
    }
    if (device_count == 0) != (device_list == 0) {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            "OpenCL build device count and pointer must both be zero or both be nonzero",
        ));
    }
    let devices = if device_count == 0 {
        Vec::new()
    } else {
        read_opencl_guest_u64_array(
            unicorn,
            device_list,
            device_count,
            "OpenCL build device list",
        )?
    };
    let options = if options == 0 {
        None
    } else {
        let bytes = read_opencl_c_string_bytes(
            unicorn,
            options,
            aex_apple_opencl::MAX_BUILD_OPTIONS_BYTES,
            "OpenCL build options",
        )?;
        Some(String::from_utf8(bytes).map_err(|error| {
            OpenClRuntimeError::new(
                CL_INVALID_VALUE,
                format!(
                    "OpenCL build options are not UTF-8 at byte {}",
                    error.utf8_error().valid_up_to()
                ),
            )
        })?)
    };
    unicorn.get_data_mut().gpu_runtime.build_opencl_program(
        program,
        &devices,
        options.as_deref(),
    )
}

fn emulate_cl_create_kernel(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<u64, OpenClRuntimeError> {
    let operation = "clCreateKernel";
    let program = opencl_argument(unicorn, 0, operation)?;
    let name = opencl_argument(unicorn, 1, operation)?;
    if name == 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            "OpenCL kernel name pointer is null",
        ));
    }
    let name = read_opencl_c_string_bytes(
        unicorn,
        name,
        aex_apple_opencl::MAX_KERNEL_NAME_BYTES,
        "OpenCL kernel name",
    )?;
    let name = String::from_utf8(name).map_err(|error| {
        OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!(
                "OpenCL kernel name is not UTF-8 at byte {}",
                error.utf8_error().valid_up_to()
            ),
        )
    })?;
    unicorn
        .get_data_mut()
        .gpu_runtime
        .create_opencl_kernel(program, &name)
}

fn emulate_cl_set_kernel_arg(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), OpenClRuntimeError> {
    let operation = "clSetKernelArg";
    let kernel = opencl_argument(unicorn, 0, operation)?;
    let index = opencl_u32_argument(unicorn, 1, operation)?;
    let argument_size = opencl_usize_argument(unicorn, 2, operation)?;
    let argument_value = opencl_argument(unicorn, 3, operation)?;
    if argument_size == 0 || argument_size > aex_apple_opencl::MAX_KERNEL_ARGUMENT_BYTES {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_ARG_SIZE,
            format!(
                "OpenCL kernel argument size {argument_size} is outside 1..={}",
                aex_apple_opencl::MAX_KERNEL_ARGUMENT_BYTES
            ),
        ));
    }
    if argument_value == 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_ARG_VALUE,
            "OpenCL local-memory kernel arguments are not supported by this bridge",
        ));
    }
    let bytes = read_opencl_guest_bytes(
        unicorn,
        argument_value,
        argument_size,
        "OpenCL kernel argument",
    )?;
    unicorn
        .get_data_mut()
        .gpu_runtime
        .set_opencl_kernel_arg(kernel, index, &bytes)
}

fn emulate_cl_enqueue_nd_range_kernel(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), OpenClRuntimeError> {
    let operation = "clEnqueueNDRangeKernel";
    let queue = opencl_argument(unicorn, 0, operation)?;
    let kernel = opencl_argument(unicorn, 1, operation)?;
    let dimensions = opencl_u32_argument(unicorn, 2, operation)? as usize;
    let global_offset_pointer = opencl_argument(unicorn, 3, operation)?;
    let global_pointer = opencl_argument(unicorn, 4, operation)?;
    let local_pointer = opencl_argument(unicorn, 5, operation)?;
    let event_wait_count = opencl_u32_argument(unicorn, 6, operation)? as usize;
    let event_wait_list = opencl_argument(unicorn, 7, operation)?;
    let output_event = opencl_argument(unicorn, 8, operation)?;
    if !(1..=MAX_OPENCL_WORK_DIMENSIONS).contains(&dimensions) {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_WORK_DIMENSION,
            format!(
                "OpenCL work dimension {dimensions} is outside 1..={MAX_OPENCL_WORK_DIMENSIONS}"
            ),
        ));
    }
    if global_pointer == 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            "OpenCL global work size pointer is null",
        ));
    }
    if (event_wait_count == 0) != (event_wait_list == 0) {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_EVENT_WAIT_LIST,
            "OpenCL event wait count and list must both be zero or both be nonzero",
        ));
    }
    if event_wait_count != 0 || output_event != 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_EVENT_WAIT_LIST,
            "OpenCL event wait/output objects are not supported by the bounded bridge",
        ));
    }
    let global = read_opencl_guest_usize_array(
        unicorn,
        global_pointer,
        dimensions,
        "OpenCL global work sizes",
    )?;
    let global_offset = if global_offset_pointer == 0 {
        None
    } else {
        Some(read_opencl_guest_usize_array(
            unicorn,
            global_offset_pointer,
            dimensions,
            "OpenCL global work offsets",
        )?)
    };
    let local = if local_pointer == 0 {
        None
    } else {
        Some(read_opencl_guest_usize_array(
            unicorn,
            local_pointer,
            dimensions,
            "OpenCL local work sizes",
        )?)
    };
    unicorn.get_data_mut().gpu_runtime.enqueue_opencl_kernel(
        queue,
        kernel,
        global_offset.as_deref(),
        &global,
        local.as_deref(),
    )
}

fn emulate_cl_release_kernel(
    unicorn: &mut Unicorn<'_, GuestState>,
) -> Result<(), OpenClRuntimeError> {
    let kernel = opencl_argument(unicorn, 0, "clReleaseKernel")?;
    unicorn
        .get_data_mut()
        .gpu_runtime
        .release_opencl_kernel(kernel)
}

fn complete_opencl_pointer_call(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: &'static str,
    errcode_pointer: u64,
    result: Result<u64, OpenClRuntimeError>,
) {
    let (value, status, detail) = match result {
        Ok(value) => (value, CL_SUCCESS, None),
        Err(error) => (0, error.status, Some(error.detail)),
    };
    if let Some(detail) = detail.as_deref() {
        unicorn
            .get_data_mut()
            .gpu_runtime
            .record_opencl_error(operation, status, detail);
    }
    if errcode_pointer != 0
        && let Err(error) = unicorn.mem_write(errcode_pointer, &status.to_le_bytes())
    {
        stop_opencl_bridge_for_callback_error(
            unicorn,
            format!(
                "{operation} could not write errcode_ret at {errcode_pointer:#x}: {error}"
            ),
        );
        return;
    }
    if let Err(error) = unicorn.reg_write(RegisterX86::RAX, value) {
        stop_opencl_bridge_for_callback_error(
            unicorn,
            format!("{operation} could not write its return value: {error}"),
        );
    }
}

fn complete_opencl_status_call(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: &'static str,
    result: Result<(), OpenClRuntimeError>,
) {
    let (status, detail) = match result {
        Ok(()) => (CL_SUCCESS, None),
        Err(error) => (error.status, Some(error.detail)),
    };
    if let Some(detail) = detail.as_deref() {
        unicorn
            .get_data_mut()
            .gpu_runtime
            .record_opencl_error(operation, status, detail);
    }
    if let Err(error) = unicorn.reg_write(RegisterX86::RAX, status as u32 as u64) {
        stop_opencl_bridge_for_callback_error(
            unicorn,
            format!("{operation} could not write its status: {error}"),
        );
    }
}

fn opencl_argument(
    unicorn: &Unicorn<'_, GuestState>,
    index: usize,
    operation: &'static str,
) -> Result<u64, OpenClRuntimeError> {
    read_win64_import_argument(unicorn, index).map_err(|detail| {
        OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!("{operation} argument {}: {detail}", index + 1),
        )
    })
}

fn opencl_u32_argument(
    unicorn: &Unicorn<'_, GuestState>,
    index: usize,
    operation: &'static str,
) -> Result<u32, OpenClRuntimeError> {
    // Win64 right-justifies 32-bit integer arguments in 64-bit ABI slots; the
    // upper half is unspecified and may contain stale stack/register bytes.
    Ok(opencl_argument(unicorn, index, operation)? as u32)
}

fn opencl_usize_argument(
    unicorn: &Unicorn<'_, GuestState>,
    index: usize,
    operation: &'static str,
) -> Result<usize, OpenClRuntimeError> {
    let value = opencl_argument(unicorn, index, operation)?;
    usize::try_from(value).map_err(|_| {
        OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!(
                "{operation} argument {} value {value:#x} does not fit size_t",
                index + 1
            ),
        )
    })
}

fn checked_opencl_array_address(
    base: u64,
    index: usize,
    element_bytes: usize,
) -> Result<u64, OpenClRuntimeError> {
    let offset = index
        .checked_mul(element_bytes)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| {
            OpenClRuntimeError::new(CL_INVALID_VALUE, "OpenCL guest array offset overflow")
        })?;
    base.checked_add(offset).ok_or_else(|| {
        OpenClRuntimeError::new(CL_INVALID_VALUE, "OpenCL guest array address overflow")
    })
}

fn read_opencl_guest_u64(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    field: &'static str,
) -> Result<u64, OpenClRuntimeError> {
    let bytes = read_opencl_guest_bytes(unicorn, address, size_of::<u64>(), field)?;
    Ok(u64::from_le_bytes(
        bytes
            .try_into()
            .expect("eight-byte OpenCL guest read has exact length"),
    ))
}

fn read_opencl_guest_u64_array(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    count: usize,
    field: &'static str,
) -> Result<Vec<u64>, OpenClRuntimeError> {
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        values.push(read_opencl_guest_u64(
            unicorn,
            checked_opencl_array_address(address, index, size_of::<u64>())?,
            field,
        )?);
    }
    Ok(values)
}

fn read_opencl_guest_usize_array(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    count: usize,
    field: &'static str,
) -> Result<Vec<usize>, OpenClRuntimeError> {
    read_opencl_guest_u64_array(unicorn, address, count, field)?
        .into_iter()
        .map(|value| {
            usize::try_from(value).map_err(|_| {
                OpenClRuntimeError::new(
                    CL_INVALID_VALUE,
                    format!("{field} value {value:#x} does not fit size_t"),
                )
            })
        })
        .collect()
}

fn read_opencl_guest_bytes(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    bytes: usize,
    field: &'static str,
) -> Result<Vec<u8>, OpenClRuntimeError> {
    if address == 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!("{field} pointer is null"),
        ));
    }
    let end = address.checked_add(bytes as u64).ok_or_else(|| {
        OpenClRuntimeError::new(CL_INVALID_VALUE, format!("{field} address range overflow"))
    })?;
    if end < address {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!("{field} address range overflow"),
        ));
    }
    unicorn.mem_read_as_vec(address, bytes).map_err(|error| {
        OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!("{field} at {address:#x} for {bytes} byte(s) is unreadable: {error}"),
        )
    })
}

fn read_opencl_c_string_bytes(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    maximum: usize,
    field: &'static str,
) -> Result<Vec<u8>, OpenClRuntimeError> {
    if address == 0 {
        return Err(OpenClRuntimeError::new(
            CL_INVALID_VALUE,
            format!("{field} pointer is null"),
        ));
    }
    let mut value = Vec::new();
    while value.len() <= maximum {
        let cursor = address.checked_add(value.len() as u64).ok_or_else(|| {
            OpenClRuntimeError::new(CL_INVALID_VALUE, format!("{field} address overflow"))
        })?;
        let page_remaining = PAGE_SIZE - (cursor % PAGE_SIZE);
        let remaining_with_terminator = maximum - value.len() + 1;
        let chunk_bytes = usize::try_from(page_remaining)
            .unwrap_or(usize::MAX)
            .min(remaining_with_terminator)
            .min(PAGE_SIZE as usize);
        let chunk = unicorn.mem_read_as_vec(cursor, chunk_bytes).map_err(|error| {
            OpenClRuntimeError::new(
                CL_INVALID_VALUE,
                format!("{field} at {cursor:#x} is unreadable: {error}"),
            )
        })?;
        if let Some(terminator) = chunk.iter().position(|byte| *byte == 0) {
            value.extend_from_slice(&chunk[..terminator]);
            return Ok(value);
        }
        value.extend_from_slice(&chunk);
        if value.len() > maximum {
            return Err(OpenClRuntimeError::new(
                CL_INVALID_VALUE,
                format!("{field} exceeds its {maximum}-byte bound"),
            ));
        }
    }
    Err(OpenClRuntimeError::new(
        CL_INVALID_VALUE,
        format!("{field} exceeds its {maximum}-byte bound"),
    ))
}

fn stop_opencl_bridge_for_callback_error(
    unicorn: &mut Unicorn<'_, GuestState>,
    detail: String,
) {
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(detail);
    }
    let _ = unicorn.emu_stop();
}

#[cfg(test)]
mod opencl_import_bridge_tests {
    use super::*;

    const CREATE_PROGRAM_STUB: u64 = STUB_BASE + 0x70000;
    const BUILD_PROGRAM_STUB: u64 = STUB_BASE + 0x70010;
    const CREATE_KERNEL_STUB: u64 = STUB_BASE + 0x70020;
    const SET_KERNEL_ARG_STUB: u64 = STUB_BASE + 0x70030;
    const ENQUEUE_STUB: u64 = STUB_BASE + 0x70040;
    const RELEASE_KERNEL_STUB: u64 = STUB_BASE + 0x70050;
    const TEST_RETURN: u64 = STUB_BASE + 0x70ff0;
    const TEST_DATA_SIZE: u64 = 0x20_000;

    fn test_unicorn() -> Unicorn<'static, GuestState> {
        let mut unicorn =
            Unicorn::new_with_data(Arch::X86, Mode::MODE_64, GuestState::default()).unwrap();
        unicorn.mem_map(STUB_BASE, STUB_SIZE, Prot::ALL).unwrap();
        unicorn
            .mem_map(STACK_BASE, STACK_SIZE, Prot::READ | Prot::WRITE)
            .unwrap();
        unicorn
            .mem_map(
                DATA_BASE,
                TEST_DATA_SIZE,
                Prot::READ | Prot::WRITE,
            )
            .unwrap();
        install_opencl_import_bridge(
            &mut unicorn,
            CREATE_PROGRAM_STUB,
            OpenClBridgeSymbol::CreateProgramWithSource,
        )
        .unwrap();
        install_opencl_import_bridge(
            &mut unicorn,
            BUILD_PROGRAM_STUB,
            OpenClBridgeSymbol::BuildProgram,
        )
        .unwrap();
        install_opencl_import_bridge(
            &mut unicorn,
            CREATE_KERNEL_STUB,
            OpenClBridgeSymbol::CreateKernel,
        )
        .unwrap();
        install_opencl_import_bridge(
            &mut unicorn,
            SET_KERNEL_ARG_STUB,
            OpenClBridgeSymbol::SetKernelArg,
        )
        .unwrap();
        install_opencl_import_bridge(
            &mut unicorn,
            ENQUEUE_STUB,
            OpenClBridgeSymbol::EnqueueNdRangeKernel,
        )
        .unwrap();
        install_opencl_import_bridge(
            &mut unicorn,
            RELEASE_KERNEL_STUB,
            OpenClBridgeSymbol::ReleaseKernel,
        )
        .unwrap();
        unicorn
    }

    fn call_import(
        unicorn: &mut Unicorn<'static, GuestState>,
        stub: u64,
        arguments: &[u64],
    ) -> u64 {
        assert!(arguments.len() <= MAX_WIN64_IMPORT_ARGUMENTS);
        let rsp = ((STACK_BASE + STACK_SIZE) - 0x108) | 8;
        unicorn.mem_write(rsp, &TEST_RETURN.to_le_bytes()).unwrap();
        for (index, argument) in arguments.iter().copied().enumerate().skip(4) {
            unicorn
                .mem_write(
                    rsp + 0x28 + ((index - 4) * size_of::<u64>()) as u64,
                    &argument.to_le_bytes(),
                )
                .unwrap();
        }
        for (register, value) in [
            (RegisterX86::RSP, rsp),
            (RegisterX86::RCX, arguments.first().copied().unwrap_or(0)),
            (RegisterX86::RDX, arguments.get(1).copied().unwrap_or(0)),
            (RegisterX86::R8, arguments.get(2).copied().unwrap_or(0)),
            (RegisterX86::R9, arguments.get(3).copied().unwrap_or(0)),
            (RegisterX86::RAX, 0),
        ] {
            unicorn.reg_write(register, value).unwrap();
        }
        unicorn.emu_start(stub, TEST_RETURN, 0, 10_000).unwrap();
        assert_eq!(unicorn.get_data().callback_error, None);
        unicorn.reg_read(RegisterX86::RAX).unwrap()
    }

    fn write_c_string(
        unicorn: &mut Unicorn<'static, GuestState>,
        address: u64,
        value: &str,
    ) {
        unicorn.mem_write(address, value.as_bytes()).unwrap();
        unicorn
            .mem_write(address + value.len() as u64, &[0])
            .unwrap();
    }

    fn read_i32(unicorn: &Unicorn<'static, GuestState>, address: u64) -> i32 {
        let bytes = unicorn.mem_read_as_vec(address, 4).unwrap();
        i32::from_le_bytes(bytes.try_into().unwrap())
    }

    fn create_program(
        unicorn: &mut Unicorn<'static, GuestState>,
        context: u64,
        source: &str,
    ) -> u64 {
        create_program_fragments(unicorn, context, &[source])
    }

    fn create_program_fragments(
        unicorn: &mut Unicorn<'static, GuestState>,
        context: u64,
        fragments: &[&str],
    ) -> u64 {
        let mut source_address = DATA_BASE + 0x1000;
        let source_pointers = DATA_BASE + 0x0800;
        let source_lengths = DATA_BASE + 0x0900;
        let errcode = DATA_BASE + 0x0a00;
        for (index, fragment) in fragments.iter().copied().enumerate() {
            unicorn
                .mem_write(source_address, fragment.as_bytes())
                .unwrap();
            unicorn
                .mem_write(
                    source_pointers + (index * size_of::<u64>()) as u64,
                    &source_address.to_le_bytes(),
                )
                .unwrap();
            unicorn
                .mem_write(
                    source_lengths + (index * size_of::<u64>()) as u64,
                    &(fragment.len() as u64).to_le_bytes(),
                )
                .unwrap();
            source_address += fragment.len() as u64 + 16;
        }
        let program = call_import(
            unicorn,
            CREATE_PROGRAM_STUB,
            &[
                context,
                fragments.len() as u64,
                source_pointers,
                source_lengths,
                errcode,
            ],
        );
        assert_eq!(read_i32(unicorn, errcode), CL_SUCCESS);
        program
    }

    fn build_program(
        unicorn: &mut Unicorn<'static, GuestState>,
        program: u64,
        device: u64,
    ) {
        let device_list = DATA_BASE + 0x0b00;
        unicorn
            .mem_write(device_list, &device.to_le_bytes())
            .unwrap();
        assert_eq!(
            call_import(
                unicorn,
                BUILD_PROGRAM_STUB,
                &[program, 1, device_list, 0, 0, 0],
            ),
            CL_SUCCESS as u64
        );
    }

    fn create_kernel(
        unicorn: &mut Unicorn<'static, GuestState>,
        program: u64,
        name: &str,
    ) -> u64 {
        let name_address = DATA_BASE + 0x0c00;
        let errcode = DATA_BASE + 0x0d00;
        write_c_string(unicorn, name_address, name);
        let kernel = call_import(
            unicorn,
            CREATE_KERNEL_STUB,
            &[program, name_address, errcode],
        );
        assert_eq!(read_i32(unicorn, errcode), CL_SUCCESS);
        kernel
    }

    fn set_argument(
        unicorn: &mut Unicorn<'static, GuestState>,
        kernel: u64,
        index: u32,
        address: u64,
        bytes: &[u8],
    ) {
        unicorn.mem_write(address, bytes).unwrap();
        assert_eq!(
            call_import(
                unicorn,
                SET_KERNEL_ARG_STUB,
                &[kernel, index as u64, bytes.len() as u64, address],
            ),
            CL_SUCCESS as u64
        );
    }

    #[test]
    fn mock_bridge_marshals_all_six_imports_and_rejects_forged_tokens() {
        let mut unicorn = test_unicorn();
        let tokens = unicorn
            .get_data_mut()
            .gpu_runtime
            .begin_mock(0)
            .unwrap();
        let rejected_errcode = DATA_BASE + 0x0780;
        assert_eq!(
            call_import(
                &mut unicorn,
                CREATE_PROGRAM_STUB,
                &[
                    tokens.context,
                    (MAX_OPENCL_SOURCE_STRINGS + 1) as u64,
                    DATA_BASE + 0x0800,
                    0,
                    rejected_errcode,
                ],
            ),
            0
        );
        assert_eq!(read_i32(&unicorn, rejected_errcode), CL_INVALID_VALUE);
        let program = create_program_fragments(
            &mut unicorn,
            tokens.context,
            &[
                "__kernel void pass",
                "through(__global int *value, int delta) {}",
            ],
        );
        assert!(unicorn.get_data().gpu_runtime.is_program_token(program));
        assert_eq!(
            call_import(
                &mut unicorn,
                BUILD_PROGRAM_STUB,
                &[program, 0, 0, 0, TEST_RETURN, 0],
            ),
            CL_INVALID_VALUE as u32 as u64
        );
        build_program(&mut unicorn, program, tokens.device);
        let kernel = create_kernel(&mut unicorn, program, "passthrough");
        assert!(unicorn.get_data().gpu_runtime.is_kernel_token(kernel));
        assert_eq!(
            call_import(
                &mut unicorn,
                SET_KERNEL_ARG_STUB,
                &[kernel, 0, 0, DATA_BASE + 0x0e00],
            ),
            CL_INVALID_ARG_SIZE as u32 as u64
        );
        assert_eq!(
            call_import(
                &mut unicorn,
                SET_KERNEL_ARG_STUB,
                &[kernel, 0, 16, 0],
            ),
            CL_INVALID_ARG_VALUE as u32 as u64
        );

        let buffer = unicorn
            .get_data_mut()
            .gpu_runtime
            .allocate_device(0, 64, BufferAccess::ReadWrite)
            .unwrap();
        set_argument(
            &mut unicorn,
            kernel,
            0,
            DATA_BASE + 0x0e00,
            &buffer.to_le_bytes(),
        );
        set_argument(
            &mut unicorn,
            kernel,
            1,
            DATA_BASE + 0x0e10,
            &7i32.to_le_bytes(),
        );

        let global = DATA_BASE + 0x0f00;
        let local = DATA_BASE + 0x0f20;
        unicorn
            .mem_write(global, &[8u64.to_le_bytes(), 4u64.to_le_bytes()].concat())
            .unwrap();
        unicorn
            .mem_write(local, &[4u64.to_le_bytes(), 2u64.to_le_bytes()].concat())
            .unwrap();
        assert_eq!(
            call_import(
                &mut unicorn,
                ENQUEUE_STUB,
                &[
                    tokens.queue,
                    kernel,
                    2,
                    0,
                    global,
                    local,
                    0xa5a5_a5a5_0000_0000,
                    0,
                    0,
                ],
            ),
            CL_SUCCESS as u64
        );
        assert_eq!(
            call_import(
                &mut unicorn,
                ENQUEUE_STUB,
                &[
                    tokens.queue,
                    kernel,
                    2,
                    0,
                    global,
                    local,
                    0,
                    0,
                    DATA_BASE + 0x0f40,
                ],
            ),
            CL_INVALID_EVENT_WAIT_LIST as u32 as u64
        );

        let mut other_runtime = GpuRuntime::default();
        other_runtime.begin_mock(0).unwrap();
        let foreign_buffer = other_runtime
            .allocate_device(0, 16, BufferAccess::ReadWrite)
            .unwrap();
        unicorn
            .mem_write(DATA_BASE + 0x0e20, &foreign_buffer.to_le_bytes())
            .unwrap();
        assert_eq!(
            call_import(
                &mut unicorn,
                SET_KERNEL_ARG_STUB,
                &[kernel, 2, 8, DATA_BASE + 0x0e20],
            ),
            CL_INVALID_MEM_OBJECT as u32 as u64
        );
        let token_shaped_scalar = program + (100 * GPU_TOKEN_OBJECT_STRIDE);
        assert!(GpuRuntime::looks_like_token(token_shaped_scalar));
        assert!(!GpuRuntime::is_issued_token(token_shaped_scalar));
        set_argument(
            &mut unicorn,
            kernel,
            2,
            DATA_BASE + 0x0e30,
            &token_shaped_scalar.to_le_bytes(),
        );

        assert_eq!(
            call_import(&mut unicorn, RELEASE_KERNEL_STUB, &[kernel]),
            CL_SUCCESS as u64
        );
        assert_eq!(
            call_import(&mut unicorn, RELEASE_KERNEL_STUB, &[kernel]),
            CL_INVALID_KERNEL as u32 as u64
        );
        let evidence = unicorn.get_data().gpu_runtime.opencl_evidence();
        assert_eq!(evidence.api_calls["clCreateProgramWithSource"], 2);
        assert_eq!(evidence.api_calls["clBuildProgram"], 2);
        assert_eq!(evidence.api_calls["clCreateKernel"], 1);
        assert_eq!(evidence.api_calls["clSetKernelArg"], 6);
        assert_eq!(evidence.api_calls["clEnqueueNDRangeKernel"], 2);
        assert_eq!(evidence.api_calls["clReleaseKernel"], 2);
        assert_eq!(evidence.source_strings, 2);
        assert_eq!(evidence.kernel_dispatches, 1);
        assert_eq!(evidence.dispatched_work_items, 32);
        assert_eq!(evidence.buffer_arguments, 1);
        assert_eq!(evidence.scalar_arguments, 2);
        assert_eq!(evidence.errors, 7);
        assert_eq!(evidence.live_programs, 1);
        assert_eq!(evidence.live_kernels, 0);

        let counts = unicorn
            .get_data_mut()
            .gpu_runtime
            .end()
            .unwrap();
        assert_eq!(counts, ObjectCounts::default());
        let evidence = unicorn.get_data().gpu_runtime.opencl_evidence();
        assert!(evidence.cleanup_balanced);
        assert_eq!(evidence.live_buffers, 0);
        assert_eq!(evidence.live_programs, 0);
        assert_eq!(evidence.live_kernels, 0);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn win64_import_bridge_executes_real_apple_gpu_kernel_and_cleans_up() {
        const ITEM_COUNT: usize = 256;
        let mut unicorn = test_unicorn();
        let tokens = unicorn
            .get_data_mut()
            .gpu_runtime
            .begin(GpuRuntimeBackendKind::AppleOpenCl, 0)
            .unwrap();
        let program = create_program(
            &mut unicorn,
            tokens.context,
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
        );
        build_program(&mut unicorn, program, tokens.device);
        let kernel = create_kernel(&mut unicorn, program, "affine");

        let input: Vec<f32> = (0..ITEM_COUNT).map(|index| index as f32 * 0.5).collect();
        let input_bytes: Vec<u8> = input.iter().flat_map(|value| value.to_ne_bytes()).collect();
        let buffer_bytes = ITEM_COUNT * size_of::<f32>();
        let input_buffer = unicorn
            .get_data_mut()
            .gpu_runtime
            .allocate_device(0, buffer_bytes, BufferAccess::ReadOnly)
            .unwrap();
        let output_buffer = unicorn
            .get_data_mut()
            .gpu_runtime
            .allocate_device(0, buffer_bytes, BufferAccess::WriteOnly)
            .unwrap();
        unicorn
            .get_data()
            .gpu_runtime
            .write_device(input_buffer, 0, &input_bytes)
            .unwrap();
        set_argument(
            &mut unicorn,
            kernel,
            0,
            DATA_BASE + 0x0e00,
            &input_buffer.to_le_bytes(),
        );
        set_argument(
            &mut unicorn,
            kernel,
            1,
            DATA_BASE + 0x0e10,
            &output_buffer.to_le_bytes(),
        );
        set_argument(
            &mut unicorn,
            kernel,
            2,
            DATA_BASE + 0x0e20,
            &2.5f32.to_ne_bytes(),
        );
        set_argument(
            &mut unicorn,
            kernel,
            3,
            DATA_BASE + 0x0e30,
            &(-3.0f32).to_ne_bytes(),
        );
        let global = DATA_BASE + 0x0f00;
        let local = DATA_BASE + 0x0f20;
        unicorn
            .mem_write(global, &(ITEM_COUNT as u64).to_le_bytes())
            .unwrap();
        unicorn.mem_write(local, &64u64.to_le_bytes()).unwrap();
        assert_eq!(
            call_import(
                &mut unicorn,
                ENQUEUE_STUB,
                &[tokens.queue, kernel, 1, 0, global, local, 0, 0, 0],
            ),
            CL_SUCCESS as u64
        );
        unicorn.get_data().gpu_runtime.finish().unwrap();
        let mut output_bytes = vec![0u8; buffer_bytes];
        unicorn
            .get_data()
            .gpu_runtime
            .read_device(output_buffer, 0, &mut output_bytes)
            .unwrap();
        let output: Vec<f32> = output_bytes
            .chunks_exact(size_of::<f32>())
            .map(|bytes| f32::from_ne_bytes(bytes.try_into().unwrap()))
            .collect();
        for (index, actual) in output.iter().copied().enumerate() {
            let expected = input[index] * 2.5 - 3.0;
            assert!((actual - expected).abs() <= f32::EPSILON);
        }

        assert_eq!(
            call_import(&mut unicorn, RELEASE_KERNEL_STUB, &[kernel]),
            CL_SUCCESS as u64
        );
        unicorn
            .get_data_mut()
            .gpu_runtime
            .free_device(0, input_buffer)
            .unwrap();
        unicorn
            .get_data_mut()
            .gpu_runtime
            .free_device(0, output_buffer)
            .unwrap();
        let before_end = unicorn.get_data().gpu_runtime.opencl_evidence();
        assert_eq!(before_end.kernel_dispatches, 1);
        assert_eq!(before_end.native_programs, 1);
        assert_eq!(before_end.native_kernels, 0);
        assert_eq!(before_end.native_buffers, 0);
        assert_eq!(
            unicorn
                .get_data_mut()
                .gpu_runtime
                .end()
                .unwrap(),
            ObjectCounts::default()
        );
        let evidence = unicorn.get_data().gpu_runtime.opencl_evidence();
        assert!(evidence.cleanup_balanced);
        assert_eq!(evidence.native_release_errors, 0);
    }
}
