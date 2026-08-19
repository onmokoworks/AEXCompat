fn capture_add_param(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("read add_param index: {error}"))? as i32;
        let pointer = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("read add_param pointer: {error}"))?;
        let mut bytes = vec![0u8; abi::PF_PARAM_DEF_SIZE];
        unicorn
            .mem_read(pointer, &mut bytes)
            .map_err(|error| format!("read PF_ParamDef at {pointer:#x}: {error}"))?;
        let param_type = i32::from_le_bytes(
            bytes[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
                .try_into()
                .expect("generated PF_ParamDef field is four bytes"),
        );
        let name_bytes =
            &bytes[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + abi::PARAM_NAME_SIZE];
        let name_end = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_bytes.len());
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
        Ok(GuestParam {
            index,
            param_type,
            name,
            bytes,
        })
    })();
    match result {
        Ok(param) => {
            unicorn.get_data_mut().params.push(param);
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn capture_plugin_data_registration(
    unicorn: &mut Unicorn<'_, GuestState>,
    includes_support_url: bool,
) {
    let result = (|| -> Result<(), String> {
        let context = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("PluginData context read failed: {error}"))?;
        if context != 1 {
            return Err(format!(
                "PluginData callback context {context:#x} is invalid"
            ));
        }
        let register = |unicorn: &Unicorn<'_, GuestState>, register| {
            unicorn
                .reg_read(register)
                .map_err(|error| format!("PluginData register read failed: {error}"))
        };
        let rsp = register(unicorn, RegisterX86::RSP)?;
        let stack_u64 = |unicorn: &Unicorn<'_, GuestState>, offset: u64| {
            let address = rsp
                .checked_add(offset)
                .ok_or_else(|| "PluginData stack address overflow".to_string())?;
            let bytes = unicorn.mem_read_as_vec(address, 8).map_err(|error| {
                format!("PluginData stack read at {address:#x} failed: {error}")
            })?;
            Ok::<u64, String>(u64::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| "PluginData stack read returned wrong size".to_string())?,
            ))
        };
        let pointers = RegistrationPointers {
            name: register(unicorn, RegisterX86::RDX)?,
            match_name: register(unicorn, RegisterX86::R8)?,
            category: register(unicorn, RegisterX86::R9)?,
            entrypoint: stack_u64(unicorn, 0x28)?,
            kind: stack_u64(unicorn, 0x30)? as u32 as i32,
            api_major: stack_u64(unicorn, 0x38)? as u32 as i32,
            api_minor: stack_u64(unicorn, 0x40)? as u32 as i32,
            reserved_info: stack_u64(unicorn, 0x48)? as u32 as i32,
            support_url: includes_support_url
                .then(|| stack_u64(unicorn, 0x50))
                .transpose()?,
        };
        let registration = decode_registration(pointers, |address| {
            unicorn
                .mem_read_as_vec(address, 1)
                .ok()
                .and_then(|bytes| bytes.first().copied())
        })
        .map_err(|error| error.to_string())?;
        unicorn
            .get_data_mut()
            .plugin_data_registry
            .push(registration)
            .map_err(|error| error.to_string())
    })();
    let returned = match result {
        Ok(()) => 0,
        Err(error) => {
            if unicorn.get_data().plugin_data_error.is_none() {
                unicorn.get_data_mut().plugin_data_error = Some(error);
            }
            CALLBACK_REJECTED
        }
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, returned as u32 as u64);
}

fn vcomp_callback_error(unicorn: &mut Unicorn<'_, GuestState>, message: String) {
    if unicorn.get_data().callback_error.is_none() {
        unicorn.get_data_mut().callback_error = Some(message);
    }
}

fn read_vcomp_register(
    unicorn: &Unicorn<'_, GuestState>,
    register: RegisterX86,
) -> Result<u64, String> {
    unicorn
        .reg_read(register)
        .map_err(|error| format!("VCOMP register read failed: {error}"))
}

fn read_vcomp_u64(unicorn: &Unicorn<'_, GuestState>, address: u64) -> Result<u64, String> {
    let bytes = unicorn
        .mem_read_as_vec(address, 8)
        .map_err(|error| format!("VCOMP memory read at {address:#x} failed: {error}"))?;
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| {
        "VCOMP memory read returned the wrong size".to_string()
    })?))
}

fn write_vcomp_i32(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    value: i32,
) -> Result<(), String> {
    if address == 0 {
        return Err("VCOMP output pointer is null".to_string());
    }
    unicorn
        .mem_write(address, &value.to_le_bytes())
        .map_err(|error| format!("VCOMP memory write at {address:#x} failed: {error}"))
}

fn emulate_vcomp_set_num_threads(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let requested = read_vcomp_register(unicorn, RegisterX86::RCX)? as u32 as i32;
        if !(1..=MAX_VCOMP_REQUESTED_THREADS).contains(&requested) {
            return Err(format!(
                "VCOMP requested thread count {requested} is outside 1..={MAX_VCOMP_REQUESTED_THREADS}"
            ));
        }
        // Preserve the bounded request for diagnostics, but keep execution
        // deterministic: outlined work still uses the serial VCOMP runtime.
        unicorn.get_data_mut().vcomp_requested_threads = Some(requested as u32);
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_vcomp_fork(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let worker_count = read_vcomp_register(unicorn, RegisterX86::RCX)?;
        if worker_count != 1 {
            return Err(format!(
                "VCOMP worker count {worker_count} is unsupported; expected 1"
            ));
        }
        let argument_count = usize::try_from(read_vcomp_register(unicorn, RegisterX86::RDX)?)
            .map_err(|_| "VCOMP argument count does not fit usize".to_string())?;
        if argument_count > 64 {
            return Err(format!(
                "VCOMP outlined worker argument count {argument_count} exceeds 64"
            ));
        }
        let worker = read_vcomp_register(unicorn, RegisterX86::R8)?;
        if worker == 0 {
            return Err("VCOMP outlined worker pointer is null".to_string());
        }
        unicorn
            .mem_read_as_vec(worker, 1)
            .map_err(|error| format!("VCOMP outlined worker {worker:#x} is unmapped: {error}"))?;

        let rsp = read_vcomp_register(unicorn, RegisterX86::RSP)?;
        let mut arguments = Vec::with_capacity(argument_count);
        if argument_count != 0 {
            arguments.push(read_vcomp_register(unicorn, RegisterX86::R9)?);
        }
        for index in 1..argument_count {
            let address = rsp
                .checked_add(0x20)
                .and_then(|base| base.checked_add((index as u64) * 8))
                .ok_or_else(|| "VCOMP captured argument address overflow".to_string())?;
            arguments.push(read_vcomp_u64(unicorn, address)?);
        }

        for (index, register) in [
            RegisterX86::RCX,
            RegisterX86::RDX,
            RegisterX86::R8,
            RegisterX86::R9,
        ]
        .into_iter()
        .enumerate()
        {
            unicorn
                .reg_write(register, arguments.get(index).copied().unwrap_or(0))
                .map_err(|error| format!("VCOMP worker register write failed: {error}"))?;
        }
        for (index, value) in arguments.iter().copied().enumerate().skip(4) {
            let address = rsp
                .checked_add(0x28)
                .and_then(|base| base.checked_add(((index - 4) as u64) * 8))
                .ok_or_else(|| "VCOMP worker stack argument address overflow".to_string())?;
            unicorn
                .mem_write(address, &value.to_le_bytes())
                .map_err(|error| format!("VCOMP worker stack write failed: {error}"))?;
        }
        unicorn
            .reg_write(RegisterX86::R11, worker)
            .map_err(|error| format!("VCOMP worker target write failed: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
        let _ = unicorn.reg_write(RegisterX86::R11, RETURN_ADDRESS);
    }
}

fn emulate_vcomp_for_dynamic_init(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let schedule = read_vcomp_register(unicorn, RegisterX86::RCX)?;
        let lower = read_vcomp_register(unicorn, RegisterX86::RDX)? as u32 as i32;
        let upper = read_vcomp_register(unicorn, RegisterX86::R8)? as u32 as i32;
        let step = read_vcomp_register(unicorn, RegisterX86::R9)? as u32 as i32;
        let rsp = read_vcomp_register(unicorn, RegisterX86::RSP)?;
        let chunk = read_vcomp_u64(unicorn, rsp + 0x28)? as u32 as i32;
        if schedule != 0x62 {
            return Err(format!(
                "VCOMP dynamic schedule {schedule:#x} is unsupported; expected 0x62"
            ));
        }
        if step != 1 {
            return Err(format!(
                "VCOMP dynamic loop step {step} is unsupported; expected 1"
            ));
        }
        if chunk <= 0 {
            return Err(format!(
                "VCOMP dynamic loop chunk {chunk} is invalid; expected a positive value"
            ));
        }
        unicorn.get_data_mut().vcomp_dynamic_loop = Some(VcompDynamicLoop {
            current: lower,
            upper,
            chunk,
            exhausted: false,
        });
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
    }
}

fn emulate_vcomp_for_dynamic_next(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let lower_output = read_vcomp_register(unicorn, RegisterX86::RCX)?;
        let upper_output = read_vcomp_register(unicorn, RegisterX86::RDX)?;
        let loop_state = unicorn
            .get_data()
            .vcomp_dynamic_loop
            .clone()
            .ok_or_else(|| "VCOMP dynamic next called before dynamic init".to_string())?;
        if loop_state.exhausted || loop_state.current > loop_state.upper {
            unicorn
                .reg_write(RegisterX86::RAX, 0)
                .map_err(|error| format!("VCOMP return write failed: {error}"))?;
            return Ok(());
        }
        let chunk_end = loop_state
            .current
            .checked_add(loop_state.chunk - 1)
            .unwrap_or(i32::MAX)
            .min(loop_state.upper);
        write_vcomp_i32(unicorn, lower_output, loop_state.current)?;
        write_vcomp_i32(unicorn, upper_output, chunk_end)?;
        if let Some(loop_state) = unicorn.get_data_mut().vcomp_dynamic_loop.as_mut() {
            if chunk_end == loop_state.upper {
                loop_state.exhausted = true;
            } else {
                loop_state.current = chunk_end + 1;
            }
        }
        unicorn
            .reg_write(RegisterX86::RAX, 1)
            .map_err(|error| format!("VCOMP return write failed: {error}"))?;
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
    }
}

fn emulate_vcomp_for_static_simple_init(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), String> {
        let lower = read_vcomp_register(unicorn, RegisterX86::RCX)? as u32 as i32;
        let upper = read_vcomp_register(unicorn, RegisterX86::RDX)? as u32 as i32;
        let step = read_vcomp_register(unicorn, RegisterX86::R8)? as u32 as i32;
        let increment = read_vcomp_register(unicorn, RegisterX86::R9)? as u32 as i32;
        if step != 1 || increment != 1 {
            return Err(format!(
                "VCOMP static loop step/increment {step}/{increment} is unsupported; expected 1/1"
            ));
        }
        let rsp = read_vcomp_register(unicorn, RegisterX86::RSP)?;
        let lower_output = read_vcomp_u64(unicorn, rsp + 0x28)?;
        let upper_output = read_vcomp_u64(unicorn, rsp + 0x30)?;
        write_vcomp_i32(unicorn, lower_output, lower)?;
        write_vcomp_i32(unicorn, upper_output, upper)?;
        Ok(())
    })();
    if let Err(error) = result {
        vcomp_callback_error(unicorn, error);
    }
}

fn install_float_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f32) -> f32,
) -> Result<(), GuestError> {
    uc(
        "install unary float import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let Ok(bits) = unicorn.reg_read(RegisterX86::XMM0) {
                let value = f32::from_bits(bits as u32);
                let output = operation(value);
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn
                        .get_data_mut()
                        .math_calls
                        .push(format!("{name}({value})={output}"));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, output.to_bits() as u64);
            }
        }),
    )
    .map(|_| ())
}

fn deterministic_import_i32(name: &str) -> Option<i32> {
    match name {
        // The emulator is deliberately single-threaded. Returning one keeps
        // OpenMP-aware kernels deterministic while preserving the API's
        // required positive thread-count contract.
        "omp_get_max_threads" => Some(1),
        _ => None,
    }
}

fn deterministic_i32_stub(value: i32) -> [u8; 6] {
    let bytes = value.to_le_bytes();
    [0xb8, bytes[0], bytes[1], bytes[2], bytes[3], 0xc3]
}

fn deterministic_u64_stub(value: u64) -> [u8; 11] {
    let bytes = value.to_le_bytes();
    [
        0x48, 0xb8, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
        bytes[7], 0xc3,
    ]
}

pub(super) fn msvc_udt_by_value_return_import(symbol: &str) -> bool {
    // These MSVC decorations identify a class/struct returned by value. Win64
    // passes hidden return storage for nontrivial objects, so the scalar-zero
    // fallback cannot initialize the result and must not let execution continue.
    let Some((_, signature)) = symbol.split_once("@@") else {
        return false;
    };
    let Some(return_offset) = signature.find('?') else {
        return false;
    };
    if return_offset == 0
        || !signature[..return_offset]
            .bytes()
            .all(|byte| byte.is_ascii_uppercase())
    {
        return false;
    }
    let mut return_type = signature[return_offset + 1..].bytes();
    let mut qualifiers = 0usize;
    for byte in return_type.by_ref() {
        if matches!(byte, b'A'..=b'D') {
            qualifiers += 1;
            continue;
        }
        return qualifiers != 0 && matches!(byte, b'T' | b'U' | b'V');
    }
    false
}

fn install_unsupported_import_trap(
    unicorn: &mut Unicorn<'_, GuestState>,
    stub: u64,
    library: String,
    symbol: String,
) -> Result<(), GuestError> {
    uc(
        "install unsupported Win64 import trap",
        unicorn.add_code_hook(stub, stub, move |unicorn, _, _| {
            if unicorn.get_data().unsupported_import.is_none() {
                unicorn.get_data_mut().unsupported_import = Some((library.clone(), symbol.clone()));
            }
            let _ = unicorn.emu_stop();
        }),
    )?;
    Ok(())
}

fn install_float_binary_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f32, f32) -> f32,
) -> Result<(), GuestError> {
    uc(
        "install binary float import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let (Ok(left), Ok(right)) = (
                unicorn.reg_read(RegisterX86::XMM0),
                unicorn.reg_read(RegisterX86::XMM1),
            ) {
                let value = operation(f32::from_bits(left as u32), f32::from_bits(right as u32));
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn.get_data_mut().math_calls.push(format!(
                        "{name}({},{})={value}",
                        f32::from_bits(left as u32),
                        f32::from_bits(right as u32)
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, value.to_bits() as u64);
            }
        }),
    )
    .map(|_| ())
}

fn install_double_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f64) -> f64,
) -> Result<(), GuestError> {
    uc(
        "install unary double import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let Ok(mut xmm0) = unicorn.reg_read_long(RegisterX86::XMM0) {
                let value = f64::from_le_bytes(xmm0[..8].try_into().unwrap());
                let output = operation(value);
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn
                        .get_data_mut()
                        .math_calls
                        .push(format!("{name}({value})={output}"));
                }
                xmm0[..8].copy_from_slice(&output.to_le_bytes());
                let _ = unicorn.reg_write_long(RegisterX86::XMM0, &xmm0);
            }
        }),
    )
    .map(|_| ())
}

fn windows_lround(value: f64) -> i32 {
    let rounded = value.round();
    if !rounded.is_finite() || rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        // UCRT's 32-bit `long` conversion uses the integer-indefinite value for
        // a domain/range failure. errno/fenv are not otherwise virtualized by
        // this serial backend, but the returned word remains ABI-compatible.
        i32::MIN
    } else {
        rounded as i32
    }
}

fn install_lround_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
) -> Result<(), GuestError> {
    uc("write lround return", unicorn.mem_write(address, &[0xc3]))?;
    uc(
        "install lround import",
        unicorn.add_code_hook(address, address, |unicorn, _, _| {
            if let Ok(xmm0) = unicorn.reg_read_long(RegisterX86::XMM0) {
                let value = f64::from_le_bytes(xmm0[..8].try_into().unwrap());
                let output = windows_lround(value);
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn
                        .get_data_mut()
                        .math_calls
                        .push(format!("lround({value})={output}"));
                }
                // Win64 `long` is 32-bit. Returning through EAX zeroes the high
                // half of RAX; callers consume the low word as signed when
                // required.
                let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(output as u32));
            }
        }),
    )
    .map(|_| ())
}

fn install_double_binary_import(
    unicorn: &mut Unicorn<'static, GuestState>,
    address: u64,
    name: &'static str,
    operation: fn(f64, f64) -> f64,
) -> Result<(), GuestError> {
    uc(
        "install binary double import",
        unicorn.add_code_hook(address, address, move |unicorn, _, _| {
            if let (Ok(left), Ok(right)) = (
                unicorn.reg_read(RegisterX86::XMM0),
                unicorn.reg_read(RegisterX86::XMM1),
            ) {
                let value = operation(f64::from_bits(left), f64::from_bits(right));
                if unicorn.get_data().math_calls.len() < 32 {
                    unicorn.get_data_mut().math_calls.push(format!(
                        "{name}({},{})={value}",
                        f64::from_bits(left),
                        f64::from_bits(right)
                    ));
                }
                let _ = unicorn.reg_write(RegisterX86::XMM0, value.to_bits());
            }
        }),
    )
    .map(|_| ())
}

fn emulate_crt_malloc(unicorn: &mut Unicorn<'_, GuestState>, calloc: bool) {
    let requested_size = if calloc {
        let count = unicorn.reg_read(RegisterX86::RCX);
        let element_size = unicorn.reg_read(RegisterX86::RDX);
        match (count, element_size) {
            (Ok(count), Ok(element_size)) => CrtHeap::checked_calloc_size(count, element_size),
            _ => Err(CrtHeapError::SizeOverflow),
        }
    } else {
        unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|_| CrtHeapError::SizeOverflow)
    };
    let pointer = requested_size.and_then(|size| allocate_crt_region(unicorn, size));
    // malloc/calloc report normal bounded allocation failure as NULL. This is
    // distinct from free ownership violations, which stop at the callback boundary.
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer.unwrap_or(0));
}

fn read_bounded_crt_c_string(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for offset in 0..=limit {
        let current = address
            .checked_add(offset)
            .ok_or_else(|| format!("{label} source range overflow"))?;
        let mut byte = [0u8; 1];
        unicorn
            .mem_read(current, &mut byte)
            .map_err(|error| format!("{label} source read: {error}"))?;
        bytes.push(byte[0]);
        if byte[0] == 0 {
            return Ok(bytes);
        }
        if offset == limit {
            break;
        }
    }
    Err(format!("{label} source exceeds {limit} bytes"))
}

fn duplicate_crt_string(
    unicorn: &mut Unicorn<'_, GuestState>,
    source: u64,
    limit: u64,
) -> Result<u64, String> {
    let bytes = read_bounded_crt_c_string(unicorn, source, limit, "_strdup")?;
    let size = u64::try_from(bytes.len()).map_err(|_| "_strdup size overflow".to_string())?;
    let pointer = match allocate_crt_region(unicorn, size) {
        Ok(pointer) => pointer,
        Err(_) => return Ok(0),
    };
    if let Err(error) = unicorn.mem_write(pointer, &bytes) {
        let _ = free_crt_region(unicorn, pointer);
        return Err(format!("_strdup destination write: {error}"));
    }
    Ok(pointer)
}

fn emulate_crt_strdup(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let source = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("read _strdup source pointer: {error}"))?;
        if source == 0 {
            return Ok(0);
        }
        duplicate_crt_string(unicorn, source, MAX_CRT_STRING_BYTES)
    })();
    match result {
        Ok(pointer) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn allocate_crt_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    size: u64,
) -> Result<u64, CrtHeapError> {
    let allocation = unicorn.get_data().crt_heap.prepare_allocation(size)?;
    let pointer = unicorn
        .get_data()
        .crt_heap
        .first_fit(CRT_HEAP_BASE, CRT_HEAP_END, allocation)?;
    unicorn
        .mem_map(pointer, allocation.backing_size, Prot::READ | Prot::WRITE)
        .map_err(|_| CrtHeapError::AddressSpaceExhausted)?;
    if let Err(error) = unicorn.get_data_mut().crt_heap.insert(pointer, allocation) {
        let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
        return Err(error);
    }
    Ok(pointer)
}

fn allocate_aligned_crt_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    size: u64,
    alignment: u64,
) -> Result<u64, CrtHeapError> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return Err(CrtHeapError::InvalidAlignment);
    }
    let allocation = unicorn
        .get_data()
        .crt_heap
        .prepare_aligned_allocation(size)?;
    let pointer = unicorn.get_data().crt_heap.first_fit_aligned(
        CRT_HEAP_BASE,
        CRT_HEAP_END,
        allocation,
        alignment,
    )?;
    unicorn
        .mem_map(pointer, allocation.backing_size, Prot::READ | Prot::WRITE)
        .map_err(|_| CrtHeapError::AddressSpaceExhausted)?;
    if let Err(error) = unicorn.get_data_mut().crt_heap.insert(pointer, allocation) {
        let _ = unicorn.mem_unmap(pointer, allocation.backing_size);
        return Err(error);
    }
    Ok(pointer)
}

fn free_crt_region(unicorn: &mut Unicorn<'_, GuestState>, pointer: u64) -> Result<(), String> {
    let allocation = unicorn
        .get_data_mut()
        .crt_heap
        .remove(pointer)
        .map_err(|error| error.to_string())?;
    unicorn
        .mem_unmap(pointer, allocation.backing_size)
        .map_err(|error| format!("unmap CRT allocation {pointer:#x}: {error}"))
}

fn free_aligned_crt_region(
    unicorn: &mut Unicorn<'_, GuestState>,
    pointer: u64,
) -> Result<(), String> {
    let allocation = unicorn
        .get_data_mut()
        .crt_heap
        .remove_aligned(pointer)
        .map_err(|error| error.to_string())?;
    unicorn
        .mem_unmap(pointer, allocation.backing_size)
        .map_err(|error| format!("unmap aligned CRT allocation {pointer:#x}: {error}"))
}

fn emulate_crt_aligned_malloc(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, CrtHeapError> {
        let size = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|_| CrtHeapError::SizeOverflow)?;
        let alignment = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|_| CrtHeapError::InvalidAlignment)?;
        allocate_aligned_crt_region(unicorn, size, alignment)
    })();
    // UCRT allocation and parameter failures are reported as NULL here. They
    // remain distinct from ownership violations at either free boundary.
    let _ = unicorn.reg_write(RegisterX86::RAX, result.unwrap_or(0));
}

fn emulate_crt_aligned_free(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = match unicorn.reg_read(RegisterX86::RCX) {
        Ok(pointer) => pointer,
        Err(error) => {
            unicorn.get_data_mut().callback_error =
                Some(format!("read aligned CRT free pointer: {error}"));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    if pointer != 0 {
        if let Err(error) = free_aligned_crt_region(unicorn, pointer) {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_extended_alloc(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let (output, size) = match (
        unicorn.reg_read(RegisterX86::RCX),
        unicorn.reg_read(RegisterX86::RDX),
    ) {
        (Ok(output), Ok(size)) => (output, size),
        (output, size) => {
            unicorn.get_data_mut().callback_error = Some(format!(
                "read extended allocation arguments: output={output:?}, size={size:?}"
            ));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    if output == 0 || size == 0 || size > 1 << 24 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 4);
        return;
    }
    let pointer = match allocate_crt_region(unicorn, size) {
        Ok(pointer) => pointer,
        Err(_) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            return;
        }
    };
    if let Err(error) = unicorn.mem_write(output, &pointer.to_le_bytes()) {
        let _ = free_crt_region(unicorn, pointer);
        unicorn.get_data_mut().callback_error =
            Some(format!("extended allocation output write: {error}"));
        let _ = unicorn.emu_stop();
        return;
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_extended_free(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let pointer_address = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("extended free pointer address: {error}"))?;
        if pointer_address == 0 {
            return Ok(());
        }
        let mut pointer = [0u8; 8];
        unicorn
            .mem_read(pointer_address, &mut pointer)
            .map_err(|error| format!("extended free pointer read: {error}"))?;
        let pointer = u64::from_le_bytes(pointer);
        if pointer != 0 {
            free_crt_region(unicorn, pointer)?;
        }
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_extended_lookup(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let id = match unicorn.reg_read(RegisterX86::RDX) {
        Ok(id) => id as i32,
        Err(error) => {
            unicorn.get_data_mut().callback_error =
                Some(format!("read extended lookup id: {error}"));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    let state = unicorn.get_data();
    let pointer = state
        .extended_strings
        .get(&id)
        .copied()
        .or_else(|| {
            state
                .extended_string_table_valid
                .then_some(state.extended_empty_string)
        })
        .unwrap_or(0);
    let _ = unicorn.reg_write(RegisterX86::RAX, pointer);
}

fn emulate_crt_free(unicorn: &mut Unicorn<'_, GuestState>) {
    let pointer = match unicorn.reg_read(RegisterX86::RCX) {
        Ok(pointer) => pointer,
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(format!("read CRT free pointer: {error}"));
            let _ = unicorn.emu_stop();
            return;
        }
    };
    if pointer == 0 {
        let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        return;
    }
    if let Err(error) = free_crt_region(unicorn, pointer) {
        unicorn.get_data_mut().callback_error = Some(error);
        let _ = unicorn.emu_stop();
    }
    let _ = unicorn.reg_write(RegisterX86::RAX, 0);
}

fn emulate_memset(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memset destination: {error}"))?;
        let value = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memset value: {error}"))? as u8;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memset length: {error}"))?;
        if length > DATA_SIZE {
            return Err(format!("memset length exceeds guest data bound: {length}"));
        }
        let bytes = vec![value; length as usize];
        unicorn
            .mem_write(destination, &bytes)
            .map_err(|error| format!("memset write: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_crt_memory_copy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memory-copy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memory-copy source: {error}"))?;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memory-copy length: {error}"))?;
        if length > MAX_CRT_MEMORY_COPY_BYTES {
            return Err(format!(
                "memory-copy length {length} exceeds {MAX_CRT_MEMORY_COPY_BYTES}"
            ));
        }
        if length == 0 {
            return Ok(destination);
        }
        let source_end = source
            .checked_add(length)
            .ok_or_else(|| "memory-copy source range overflow".to_string())?;
        destination
            .checked_add(length)
            .ok_or_else(|| "memory-copy destination range overflow".to_string())?;
        let copy_backward = destination > source && destination < source_end;
        let mut copied = 0u64;
        while copied < length {
            let chunk = (length - copied).min(CRT_MEMORY_COPY_CHUNK as u64);
            let offset = if copy_backward {
                length - copied - chunk
            } else {
                copied
            };
            let source_address = source
                .checked_add(offset)
                .ok_or_else(|| "memory-copy source address overflow".to_string())?;
            let destination_address = destination
                .checked_add(offset)
                .ok_or_else(|| "memory-copy destination address overflow".to_string())?;
            let mut bytes = vec![0u8; chunk as usize];
            unicorn
                .mem_read(source_address, &mut bytes)
                .map_err(|error| format!("memory-copy source read: {error}"))?;
            unicorn
                .mem_write(destination_address, &bytes)
                .map_err(|error| format!("memory-copy destination write: {error}"))?;
            copied += chunk;
        }
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_crt_memchr(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let source = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memchr source: {error}"))?;
        let needle = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memchr value: {error}"))? as u8;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memchr length: {error}"))?;
        if length > MAX_CRT_MEMORY_COPY_BYTES {
            return Err(format!(
                "memchr length {length} exceeds {MAX_CRT_MEMORY_COPY_BYTES}"
            ));
        }
        if length == 0 {
            return Ok(0);
        }
        source
            .checked_add(length)
            .ok_or_else(|| "memchr source range overflow".to_string())?;

        let mut scanned = 0u64;
        while scanned < length {
            let chunk = (length - scanned).min(CRT_MEMORY_COPY_CHUNK as u64);
            let address = source
                .checked_add(scanned)
                .ok_or_else(|| "memchr source address overflow".to_string())?;
            let mut bytes = vec![0u8; chunk as usize];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("memchr source read: {error}"))?;
            if let Some(offset) = bytes.iter().position(|byte| *byte == needle) {
                return address
                    .checked_add(offset as u64)
                    .ok_or_else(|| "memchr result address overflow".to_string());
            }
            scanned += chunk;
        }
        Ok(0)
    })();
    match result {
        Ok(address) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, address);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn compare_crt_memory(
    unicorn: &Unicorn<'_, GuestState>,
    left: u64,
    right: u64,
    length: u64,
) -> Result<i32, String> {
    if length > MAX_CRT_MEMORY_COPY_BYTES {
        return Err(format!(
            "memcmp length {length} exceeds {MAX_CRT_MEMORY_COPY_BYTES}"
        ));
    }
    if length == 0 {
        return Ok(0);
    }
    left.checked_add(length)
        .ok_or_else(|| "memcmp left range overflow".to_string())?;
    right
        .checked_add(length)
        .ok_or_else(|| "memcmp right range overflow".to_string())?;

    let mut compared = 0u64;
    while compared < length {
        let chunk = (length - compared).min(CRT_MEMORY_COPY_CHUNK as u64);
        let left_address = left
            .checked_add(compared)
            .ok_or_else(|| "memcmp left address overflow".to_string())?;
        let right_address = right
            .checked_add(compared)
            .ok_or_else(|| "memcmp right address overflow".to_string())?;
        let mut left_bytes = vec![0u8; chunk as usize];
        let mut right_bytes = vec![0u8; chunk as usize];
        unicorn
            .mem_read(left_address, &mut left_bytes)
            .map_err(|error| format!("memcmp left read: {error}"))?;
        unicorn
            .mem_read(right_address, &mut right_bytes)
            .map_err(|error| format!("memcmp right read: {error}"))?;
        if let Some((left, right)) = left_bytes
            .iter()
            .zip(&right_bytes)
            .find(|(left, right)| left != right)
        {
            return Ok(i32::from(*left) - i32::from(*right));
        }
        compared += chunk;
    }
    Ok(0)
}

fn emulate_crt_memcmp(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<i32, String> {
        let left = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("memcmp left pointer: {error}"))?;
        let right = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("memcmp right pointer: {error}"))?;
        let length = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("memcmp length: {error}"))?;
        compare_crt_memory(unicorn, left, right, length)
    })();
    match result {
        Ok(ordering) => {
            // An x64 `int` return is written through EAX, which clears the
            // upper half of RAX while preserving the signed 32-bit value.
            let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(ordering as u32));
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn read_crt_stdio_c_string(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, String> {
    if address == 0 {
        return Err(format!("stdio {label} pointer is null"));
    }
    let mut bytes = Vec::new();
    for offset in 0..limit {
        let address = address
            .checked_add(offset)
            .ok_or_else(|| format!("stdio {label} range overflow"))?;
        let mut byte = [0u8; 1];
        unicorn
            .mem_read(address, &mut byte)
            .map_err(|error| format!("stdio {label} read: {error}"))?;
        if byte[0] == 0 {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
    Err(format!("stdio {label} exceeds {limit} bytes"))
}

fn emulate_stdio_common_vsnprintf_s(unicorn: &mut Unicorn<'_, GuestState>) {
    emulate_stdio_common_printf(unicorn, true);
}

fn emulate_stdio_common_vsprintf(unicorn: &mut Unicorn<'_, GuestState>) {
    emulate_stdio_common_printf(unicorn, false);
}

fn emulate_stdio_common_printf(unicorn: &mut Unicorn<'_, GuestState>, secure: bool) {
    let result = (|| -> Result<(u64, Vec<u8>, u64), String> {
        let options = read_win64_import_argument(unicorn, 0)?;
        let destination = read_win64_import_argument(unicorn, 1)?;
        let requested_buffer_count = read_win64_import_argument(unicorn, 2)?;
        if !secure && requested_buffer_count != u64::MAX {
            return Err(format!(
                "stdio finite vsprintf buffer count {requested_buffer_count} is unsupported"
            ));
        }
        let (buffer_count, max_count, format_address, locale, va_list) = if secure {
            (
                requested_buffer_count,
                read_win64_import_argument(unicorn, 3)?,
                read_win64_import_argument(unicorn, 4)?,
                read_win64_import_argument(unicorn, 5)?,
                read_win64_import_argument(unicorn, 6)?,
            )
        } else {
            let buffer_count = if requested_buffer_count == u64::MAX {
                MAX_CRT_STDIO_BUFFER_BYTES + 1
            } else {
                requested_buffer_count
            };
            (
                buffer_count,
                buffer_count.saturating_sub(1),
                read_win64_import_argument(unicorn, 3)?,
                read_win64_import_argument(unicorn, 4)?,
                read_win64_import_argument(unicorn, 5)?,
            )
        };

        let expected_options = if secure { 0x24 } else { 0x25 };
        if options != expected_options {
            return Err(format!("stdio unsupported formatting options {options:#x}"));
        }
        if locale != 0 {
            return Err("stdio locale-aware formatting is unsupported".to_string());
        }
        if destination == 0 || buffer_count == 0 {
            return Err("stdio destination and buffer count must be nonzero".to_string());
        }
        let maximum_buffer_count = MAX_CRT_STDIO_BUFFER_BYTES + u64::from(!secure);
        if buffer_count > maximum_buffer_count {
            return Err(format!(
                "stdio buffer count {buffer_count} exceeds {maximum_buffer_count}"
            ));
        }
        if max_count != u64::MAX && max_count >= buffer_count {
            return Err(format!(
                "stdio max count {max_count} must be smaller than buffer count {buffer_count}"
            ));
        }

        let format = read_crt_stdio_c_string(
            unicorn,
            format_address,
            MAX_CRT_STDIO_FORMAT_BYTES,
            "format",
        )?;
        let mut output = Vec::new();
        let mut format_index = 0usize;
        let mut argument_index = 0usize;
        while format_index < format.len() {
            if format[format_index] != b'%' {
                output.push(format[format_index]);
                format_index += 1;
            } else {
                format_index += 1;
                let conversion = *format
                    .get(format_index)
                    .ok_or_else(|| "stdio format ends with '%'".to_string())?;
                format_index += 1;
                if conversion == b'%' {
                    output.push(b'%');
                    continue;
                }
                if argument_index >= MAX_CRT_STDIO_ARGUMENTS {
                    return Err(format!(
                        "stdio conversion count exceeds {MAX_CRT_STDIO_ARGUMENTS}"
                    ));
                }
                let slot_address = va_list
                    .checked_add((argument_index as u64) * 8)
                    .ok_or_else(|| "stdio va_list address overflow".to_string())?;
                let slot = unicorn
                    .mem_read_as_vec(slot_address, 8)
                    .map_err(|error| format!("stdio va_list read: {error}"))?;
                let value = u64::from_le_bytes(
                    slot.try_into()
                        .map_err(|_| "stdio va_list slot has wrong size".to_string())?,
                );
                argument_index += 1;
                match conversion {
                    b's' => output.extend(read_crt_stdio_c_string(
                        unicorn,
                        value,
                        MAX_CRT_STDIO_BUFFER_BYTES,
                        "string argument",
                    )?),
                    b'd' => output.extend((value as u32 as i32).to_string().as_bytes()),
                    other => {
                        return Err(format!(
                            "stdio unsupported conversion '%{}'",
                            char::from(other)
                        ));
                    }
                }
            }
            if output.len() as u64 > MAX_CRT_STDIO_BUFFER_BYTES {
                return Err(format!(
                    "stdio formatted output exceeds {MAX_CRT_STDIO_BUFFER_BYTES} bytes"
                ));
            }
        }

        let limit = if max_count == u64::MAX {
            buffer_count - 1
        } else {
            max_count
        };
        let truncated = output.len() as u64 > limit;
        output.truncate(limit as usize);
        output.push(0);
        let return_value = if truncated {
            u32::MAX as u64
        } else {
            (output.len() - 1) as u64
        };
        Ok((destination, output, return_value))
    })();
    match result {
        Ok((destination, output, return_value)) => {
            if let Err(error) = unicorn.mem_write(destination, &output) {
                if unicorn.get_data().callback_error.is_none() {
                    unicorn.get_data_mut().callback_error =
                        Some(format!("stdio destination write: {error}"));
                }
                let _ = unicorn.emu_stop();
            } else {
                let _ = unicorn.reg_write(RegisterX86::RAX, return_value);
            }
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn finish_msvcp_mutex_callback(unicorn: &mut Unicorn<'_, GuestState>, result: Result<(), String>) {
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn read_msvcp_mutex_object(
    unicorn: &Unicorn<'_, GuestState>,
    operation: &str,
) -> Result<u64, String> {
    let object = read_win64_import_argument(unicorn, 0)?;
    if object == 0 {
        return Err(format!("MSVCP mutex {operation} object is null"));
    }
    unicorn
        .mem_read_as_vec(object, 1)
        .map_err(|error| format!("MSVCP mutex {operation} object is not mapped: {error}"))?;
    Ok(object)
}

fn emulate_msvcp_mutex_init(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let object = read_msvcp_mutex_object(unicorn, "init")?;
        let mutex_type = read_win64_import_argument(unicorn, 1)? as u32;
        if mutex_type != OBSERVED_MSVCP_MUTEX_TYPE {
            return Err(format!(
                "MSVCP mutex type {mutex_type:#x} is unsupported; expected {OBSERVED_MSVCP_MUTEX_TYPE:#x}"
            ));
        }
        let state = unicorn.get_data_mut();
        if state.msvcp_mutexes.contains_key(&object) {
            return Err(format!("MSVCP mutex {object:#x} is already initialized"));
        }
        if state.msvcp_mutexes.len() >= MAX_MSVCP_MUTEXES {
            return Err(format!("MSVCP mutex count exceeds {MAX_MSVCP_MUTEXES}"));
        }
        state.msvcp_mutexes.insert(
            object,
            MsvcpMutex {
                mutex_type,
                lock_count: 0,
            },
        );
        Ok(())
    })();
    finish_msvcp_mutex_callback(unicorn, result);
}

fn emulate_msvcp_mutex_lock(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let object = read_msvcp_mutex_object(unicorn, "lock")?;
        let mutex = unicorn
            .get_data_mut()
            .msvcp_mutexes
            .get_mut(&object)
            .ok_or_else(|| format!("MSVCP mutex {object:#x} is not initialized"))?;
        if mutex.lock_count >= MAX_MSVCP_MUTEX_RECURSION {
            return Err(format!(
                "MSVCP mutex {object:#x} recursion exceeds {MAX_MSVCP_MUTEX_RECURSION}"
            ));
        }
        mutex.lock_count += 1;
        Ok(())
    })();
    finish_msvcp_mutex_callback(unicorn, result);
}

fn emulate_msvcp_mutex_unlock(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let object = read_msvcp_mutex_object(unicorn, "unlock")?;
        let mutex = unicorn
            .get_data_mut()
            .msvcp_mutexes
            .get_mut(&object)
            .ok_or_else(|| format!("MSVCP mutex {object:#x} is not initialized"))?;
        if mutex.lock_count == 0 {
            return Err(format!("MSVCP mutex {object:#x} unlock is unbalanced"));
        }
        mutex.lock_count -= 1;
        Ok(())
    })();
    finish_msvcp_mutex_callback(unicorn, result);
}

fn emulate_msvcp_mutex_destroy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let object = read_msvcp_mutex_object(unicorn, "destroy")?;
        let state = unicorn.get_data_mut();
        let mutex = state
            .msvcp_mutexes
            .get(&object)
            .ok_or_else(|| format!("MSVCP mutex {object:#x} is not initialized"))?;
        if mutex.lock_count != 0 {
            return Err(format!(
                "MSVCP mutex {object:#x} destroyed with lock count {}",
                mutex.lock_count
            ));
        }
        state.msvcp_mutexes.remove(&object);
        Ok(())
    })();
    finish_msvcp_mutex_callback(unicorn, result);
}

fn read_vcruntime_exception_data(
    unicorn: &Unicorn<'_, GuestState>,
    address: u64,
    label: &str,
) -> Result<VcruntimeExceptionData, String> {
    if address == 0 {
        return Err(format!("VCRUNTIME exception {label} pointer is null"));
    }
    let bytes = unicorn
        .mem_read_as_vec(address, VCRUNTIME_EXCEPTION_DATA_BYTES)
        .map_err(|error| format!("VCRUNTIME exception {label} read: {error}"))?;
    let what = u64::from_le_bytes(
        bytes[..8]
            .try_into()
            .map_err(|_| format!("VCRUNTIME exception {label} what has wrong size"))?,
    );
    let do_free = match bytes[8] {
        0 => false,
        1 => true,
        value => {
            return Err(format!(
                "VCRUNTIME exception {label} ownership flag {value} is invalid"
            ));
        }
    };
    Ok(VcruntimeExceptionData { what, do_free })
}

fn write_vcruntime_exception_data(
    unicorn: &mut Unicorn<'_, GuestState>,
    address: u64,
    data: VcruntimeExceptionData,
    label: &str,
) -> Result<(), String> {
    let mut bytes = [0u8; VCRUNTIME_EXCEPTION_DATA_BYTES];
    bytes[..8].copy_from_slice(&data.what.to_le_bytes());
    bytes[8] = u8::from(data.do_free);
    unicorn
        .mem_write(address, &bytes)
        .map_err(|error| format!("VCRUNTIME exception {label} write: {error}"))
}

fn finish_vcruntime_exception_callback(
    unicorn: &mut Unicorn<'_, GuestState>,
    result: Result<(), String>,
) {
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_vcruntime_exception_copy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let source_address = read_win64_import_argument(unicorn, 0)?;
        let destination_address = read_win64_import_argument(unicorn, 1)?;
        let source = read_vcruntime_exception_data(unicorn, source_address, "source")?;
        let destination =
            read_vcruntime_exception_data(unicorn, destination_address, "destination")?;
        if destination.what != 0 || destination.do_free {
            return Err("VCRUNTIME exception copy destination is not empty".to_string());
        }

        if !source.do_free || source.what == 0 {
            return write_vcruntime_exception_data(
                unicorn,
                destination_address,
                VcruntimeExceptionData {
                    what: source.what,
                    do_free: false,
                },
                "destination",
            );
        }

        let source_allocation = unicorn
            .get_data()
            .crt_heap
            .allocations()
            .find_map(|(pointer, allocation)| (pointer == source.what).then_some(allocation))
            .ok_or_else(|| {
                format!(
                    "VCRUNTIME exception owned string {:#x} is not a live CRT allocation",
                    source.what
                )
            })?;
        let string = read_crt_stdio_c_string(
            unicorn,
            source.what,
            source_allocation.requested_size,
            "exception string",
        )?;
        let allocation_size = (string.len() as u64)
            .checked_add(1)
            .ok_or_else(|| "VCRUNTIME exception string size overflow".to_string())?;
        let copy =
            allocate_crt_region(unicorn, allocation_size).map_err(|error| error.to_string())?;
        let mut terminated = string;
        terminated.push(0);
        if let Err(error) = unicorn.mem_write(copy, &terminated) {
            let _ = free_crt_region(unicorn, copy);
            return Err(format!("VCRUNTIME exception string copy write: {error}"));
        }
        if let Err(error) = write_vcruntime_exception_data(
            unicorn,
            destination_address,
            VcruntimeExceptionData {
                what: copy,
                do_free: true,
            },
            "destination",
        ) {
            let _ = free_crt_region(unicorn, copy);
            return Err(error);
        }
        Ok(())
    })();
    finish_vcruntime_exception_callback(unicorn, result);
}

fn emulate_vcruntime_exception_destroy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let data_address = read_win64_import_argument(unicorn, 0)?;
        let data = read_vcruntime_exception_data(unicorn, data_address, "data")?;
        // Verify writability before releasing owned storage so a read-only
        // object cannot turn a failed destroy into a leaked stale pointer.
        write_vcruntime_exception_data(unicorn, data_address, data, "data preflight")?;
        if data.do_free && data.what != 0 {
            let allocation = unicorn
                .get_data()
                .crt_heap
                .allocations()
                .find_map(|(pointer, allocation)| (pointer == data.what).then_some(allocation))
                .ok_or_else(|| {
                    format!(
                        "VCRUNTIME exception owned string {:#x} is not a live CRT allocation",
                        data.what
                    )
                })?;
            let allocation_end = data
                .what
                .checked_add(allocation.backing_size)
                .ok_or_else(|| "VCRUNTIME exception owned allocation overflow".to_string())?;
            if (data.what..allocation_end).contains(&data_address) {
                return Err("VCRUNTIME exception data overlaps its owned string allocation".into());
            }
            free_crt_region(unicorn, data.what)?;
        }
        write_vcruntime_exception_data(
            unicorn,
            data_address,
            VcruntimeExceptionData {
                what: 0,
                do_free: false,
            },
            "data",
        )
    })();
    finish_vcruntime_exception_callback(unicorn, result);
}

fn emulate_strncpy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("strncpy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("strncpy source: {error}"))?;
        let count = usize::try_from(
            unicorn
                .reg_read(RegisterX86::R8)
                .map_err(|error| format!("strncpy count: {error}"))?,
        )
        .map_err(|_| "strncpy count does not fit usize".to_string())?;
        if count > 4096 {
            return Err(format!("strncpy count {count} exceeds 4096"));
        }
        let mut output = vec![0u8; count];
        let mut terminated = false;
        for (index, byte) in output.iter_mut().enumerate() {
            if terminated {
                *byte = 0;
                continue;
            }
            let mut source_byte = [0u8; 1];
            unicorn
                .mem_read(source + index as u64, &mut source_byte)
                .map_err(|error| format!("strncpy source read: {error}"))?;
            *byte = source_byte[0];
            terminated = source_byte[0] == 0;
        }
        unicorn
            .mem_write(destination, &output)
            .map_err(|error| format!("strncpy destination write: {error}"))?;
        Ok(destination)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_strcpy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("strcpy destination: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("strcpy source: {error}"))?;
        if destination == 0 || source == 0 {
            return Ok(0);
        }
        let mut output = Vec::new();
        for index in 0..4096u64 {
            let mut byte = [0u8; 1];
            let source_address = source
                .checked_add(index)
                .ok_or_else(|| "strcpy source range overflow".to_string())?;
            unicorn
                .mem_read(source_address, &mut byte)
                .map_err(|error| format!("strcpy source read: {error}"))?;
            output.push(byte[0]);
            if byte[0] == 0 {
                unicorn
                    .mem_write(destination, &output)
                    .map_err(|error| format!("strcpy destination write: {error}"))?;
                return Ok(destination);
            }
        }
        Ok(0)
    })();
    match result {
        Ok(destination) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, destination);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_ansi_strcpy_bounded(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("PF ANSI bounded strcpy destination: {error}"))?;
        let destination_size = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("PF ANSI bounded strcpy size: {error}"))?;
        let source = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("PF ANSI bounded strcpy source: {error}"))?;
        if destination == 0 || source == 0 || destination_size == 0 {
            return Ok(4);
        }
        let mut source_bytes = Vec::new();
        for index in 0..4096u64 {
            let mut byte = [0u8; 1];
            let source_address = source
                .checked_add(index)
                .ok_or_else(|| "PF ANSI bounded strcpy source range overflow".to_string())?;
            unicorn
                .mem_read(source_address, &mut byte)
                .map_err(|error| format!("PF ANSI bounded strcpy source read: {error}"))?;
            if byte[0] == 0 {
                let copied = (source_bytes.len() as u64).min(destination_size - 1) as usize;
                source_bytes.truncate(copied);
                source_bytes.push(0);
                unicorn
                    .mem_write(destination, &source_bytes)
                    .map_err(|error| {
                        format!("PF ANSI bounded strcpy destination write: {error}")
                    })?;
                return Ok(0);
            }
            source_bytes.push(byte[0]);
        }
        Ok(4)
    })();
    match result {
        Ok(error) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, error);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_ansi_sprintf_literal(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<Option<Vec<u8>>, String> {
        let destination = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("PF ANSI sprintf destination: {error}"))?;
        let format = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("PF ANSI sprintf format: {error}"))?;
        if destination == 0 || format == 0 {
            return Ok(None);
        }
        let mut output = Vec::new();
        let mut index = 0u64;
        while index < 256 {
            let mut byte = [0u8; 1];
            let address = format
                .checked_add(index)
                .ok_or_else(|| "PF ANSI sprintf format range overflow".to_string())?;
            unicorn
                .mem_read(address, &mut byte)
                .map_err(|error| format!("PF ANSI sprintf format read: {error}"))?;
            match byte[0] {
                0 => {
                    output.push(0);
                    return Ok(Some(output));
                }
                b'%' => {
                    index += 1;
                    if index >= 256 {
                        return Ok(None);
                    }
                    let address = format
                        .checked_add(index)
                        .ok_or_else(|| "PF ANSI sprintf format range overflow".to_string())?;
                    unicorn
                        .mem_read(address, &mut byte)
                        .map_err(|error| format!("PF ANSI sprintf format read: {error}"))?;
                    if byte[0] != b'%' {
                        return Ok(None);
                    }
                    output.push(b'%');
                }
                value => output.push(value),
            }
            if output.len() > 4096 {
                return Ok(None);
            }
            index += 1;
        }
        Ok(None)
    })();
    match result {
        Ok(Some(output)) => {
            let written = output.len() - 1;
            if let Err(error) = unicorn.mem_write(
                unicorn.reg_read(RegisterX86::RCX).unwrap_or_default(),
                &output,
            ) {
                if unicorn.get_data().callback_error.is_none() {
                    unicorn.get_data_mut().callback_error =
                        Some(format!("PF ANSI sprintf destination write: {error}"));
                }
                let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
                let _ = unicorn.emu_stop();
            } else {
                let _ = unicorn.reg_write(RegisterX86::RAX, written as u64);
            }
        }
        Ok(None) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
            let _ = unicorn.emu_stop();
        }
    }
}

#[derive(Clone, Copy)]
struct BlendWorld {
    data: u64,
    rowbytes: usize,
    width: usize,
    height: usize,
}

fn read_blend_world(
    unicorn: &Unicorn<'_, GuestState>,
    world: u64,
    pixel_bytes: usize,
    name: &str,
) -> Result<Option<BlendWorld>, String> {
    if world == 0 {
        return Ok(None);
    }
    let data = read_guest_u64(
        unicorn,
        world + abi::LAYER_DATA_OFFSET as u64,
        &format!("blend {name} data"),
    )?;
    let rowbytes = read_guest_i32(
        unicorn,
        world + abi::LAYER_ROWBYTES_OFFSET as u64,
        &format!("blend {name} rowbytes"),
    )?;
    let width = read_guest_i32(
        unicorn,
        world + abi::LAYER_WIDTH_OFFSET as u64,
        &format!("blend {name} width"),
    )?;
    let height = read_guest_i32(
        unicorn,
        world + abi::LAYER_HEIGHT_OFFSET as u64,
        &format!("blend {name} height"),
    )?;
    if data == 0
        || rowbytes <= 0
        || width <= 0
        || height <= 0
        || width > MAX_WORLD_DIMENSION
        || height > MAX_WORLD_DIMENSION
    {
        return Ok(None);
    }
    let width = width as usize;
    let height = height as usize;
    let rowbytes = rowbytes as usize;
    let Some(packed_row) = width.checked_mul(pixel_bytes) else {
        return Ok(None);
    };
    let Some(mapped_bytes) = rowbytes.checked_mul(height) else {
        return Ok(None);
    };
    if rowbytes < packed_row || mapped_bytes > MAX_WORLD_SIZE as usize {
        return Ok(None);
    }
    Ok(Some(BlendWorld {
        data,
        rowbytes,
        width,
        height,
    }))
}

fn emulate_blend(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<bool, String> {
        let first_world = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("blend first world: {error}"))?;
        let second_world = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("blend second world: {error}"))?;
        let ratio = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("blend ratio: {error}"))? as u32 as i32;
        let destination_world = aegp_stack_arg(unicorn, 0x28)?;
        let pixel_format = unicorn.get_data().render_pixel_format;
        let Some(pixel_bytes) = world_pixel_bytes(pixel_format).map(|value| value as usize) else {
            return Ok(false);
        };
        if !(0..=65_536).contains(&ratio) {
            return Ok(false);
        }
        let Some(first) = read_blend_world(unicorn, first_world, pixel_bytes, "first source")?
        else {
            return Ok(false);
        };
        let Some(second) = read_blend_world(unicorn, second_world, pixel_bytes, "second source")?
        else {
            return Ok(false);
        };
        let Some(destination) =
            read_blend_world(unicorn, destination_world, pixel_bytes, "destination")?
        else {
            return Ok(false);
        };
        if first.width != second.width
            || first.width != destination.width
            || first.height != second.height
            || first.height != destination.height
        {
            return Ok(false);
        }
        let packed_row = first
            .width
            .checked_mul(pixel_bytes)
            .ok_or_else(|| "blend packed row overflow".to_string())?;
        let snapshot_size = packed_row
            .checked_mul(first.height)
            .filter(|size| *size <= MAX_WORLD_SIZE as usize)
            .ok_or_else(|| "blend snapshot exceeds worker bound".to_string())?;
        let mut first_snapshot = vec![0u8; snapshot_size];
        let mut second_snapshot = vec![0u8; snapshot_size];
        for row in 0..first.height {
            let first_address = first
                .data
                .checked_add((row * first.rowbytes) as u64)
                .ok_or_else(|| "blend first source address overflow".to_string())?;
            let second_address = second
                .data
                .checked_add((row * second.rowbytes) as u64)
                .ok_or_else(|| "blend second source address overflow".to_string())?;
            let range = row * packed_row..(row + 1) * packed_row;
            unicorn
                .mem_read(first_address, &mut first_snapshot[range.clone()])
                .map_err(|error| format!("blend first source pixels: {error}"))?;
            unicorn
                .mem_read(second_address, &mut second_snapshot[range])
                .map_err(|error| format!("blend second source pixels: {error}"))?;
        }
        let mut output = vec![0u8; packed_row];
        for row in 0..first.height {
            let row_start = row * packed_row;
            let first_row = &first_snapshot[row_start..row_start + packed_row];
            let second_row = &second_snapshot[row_start..row_start + packed_row];
            match pixel_bytes {
                4 => {
                    for index in 0..packed_row {
                        let value = (i32::from(first_row[index]) * (65_536 - ratio)
                            + i32::from(second_row[index]) * ratio
                            + 32_768)
                            >> 16;
                        output[index] = value.clamp(0, 255) as u8;
                    }
                }
                8 => {
                    for index in 0..first.width * 4 {
                        let byte = index * 2;
                        let first = u16::from_le_bytes([first_row[byte], first_row[byte + 1]]);
                        let second = u16::from_le_bytes([second_row[byte], second_row[byte + 1]]);
                        let value = (i64::from(first) * i64::from(65_536 - ratio)
                            + i64::from(second) * i64::from(ratio)
                            + 32_768)
                            >> 16;
                        output[byte..byte + 2]
                            .copy_from_slice(&(value.clamp(0, 32_768) as u16).to_le_bytes());
                    }
                }
                16 => {
                    let fraction = f64::from(ratio) / 65_536.0;
                    for index in 0..first.width * 4 {
                        let byte = index * 4;
                        let first = f32::from_le_bytes(
                            first_row[byte..byte + 4]
                                .try_into()
                                .expect("float channel is four bytes"),
                        );
                        let second = f32::from_le_bytes(
                            second_row[byte..byte + 4]
                                .try_into()
                                .expect("float channel is four bytes"),
                        );
                        output[byte..byte + 4].copy_from_slice(
                            &((f64::from(first) * (1.0 - fraction) + f64::from(second) * fraction)
                                as f32)
                                .to_le_bytes(),
                        );
                    }
                }
                _ => return Ok(false),
            }
            let destination_address = destination
                .data
                .checked_add((row * destination.rowbytes) as u64)
                .ok_or_else(|| "blend destination address overflow".to_string())?;
            unicorn
                .mem_write(destination_address, &output)
                .map_err(|error| format!("blend destination pixels: {error}"))?;
        }
        Ok(true)
    })();
    match result {
        Ok(valid) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, if valid { 0 } else { 4 });
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_copy(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| {
        let source = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("copy source world: {error}"))?;
        let destination = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("copy destination world: {error}"))?;
        let read_u64 = |unicorn: &Unicorn<'_, GuestState>, address| {
            let mut bytes = [0u8; 8];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("copy world pointer read: {error}"))?;
            Ok::<u64, String>(u64::from_le_bytes(bytes))
        };
        let read_i32 = |unicorn: &Unicorn<'_, GuestState>, address| {
            let mut bytes = [0u8; 4];
            unicorn
                .mem_read(address, &mut bytes)
                .map_err(|error| format!("copy world field read: {error}"))?;
            Ok::<i32, String>(i32::from_le_bytes(bytes))
        };
        let source_data = read_u64(unicorn, source + abi::LAYER_DATA_OFFSET as u64)?;
        let destination_data = read_u64(unicorn, destination + abi::LAYER_DATA_OFFSET as u64)?;
        let source_rowbytes =
            read_i32(unicorn, source + abi::LAYER_ROWBYTES_OFFSET as u64)?.max(0) as usize;
        let destination_rowbytes =
            read_i32(unicorn, destination + abi::LAYER_ROWBYTES_OFFSET as u64)?.max(0) as usize;
        let height = read_i32(unicorn, source + abi::LAYER_HEIGHT_OFFSET as u64)?
            .min(read_i32(
                unicorn,
                destination + abi::LAYER_HEIGHT_OFFSET as u64,
            )?)
            .max(0) as usize;
        let row_size = source_rowbytes.min(destination_rowbytes);
        for row in 0..height {
            let mut bytes = vec![0u8; row_size];
            unicorn
                .mem_read(source_data + (row * source_rowbytes) as u64, &mut bytes)
                .map_err(|error| format!("copy source pixels: {error}"))?;
            unicorn
                .mem_write(
                    destination_data + (row * destination_rowbytes) as u64,
                    &bytes,
                )
                .map_err(|error| format!("copy destination pixels: {error}"))?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(error) => {
            unicorn.get_data_mut().callback_error = Some(error);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_fill8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let color = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("Fill8 color: {error}"))?;
        let area = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("Fill8 area: {error}"))?;
        let world = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("Fill8 world: {error}"))?;
        if world == 0 {
            return Ok(4);
        }
        let data = read_guest_u64(
            unicorn,
            world + abi::LAYER_DATA_OFFSET as u64,
            "Fill8 world data",
        )?;
        let rowbytes = read_guest_i32(
            unicorn,
            world + abi::LAYER_ROWBYTES_OFFSET as u64,
            "Fill8 world rowbytes",
        )?;
        let width = read_guest_i32(
            unicorn,
            world + abi::LAYER_WIDTH_OFFSET as u64,
            "Fill8 world width",
        )?;
        let height = read_guest_i32(
            unicorn,
            world + abi::LAYER_HEIGHT_OFFSET as u64,
            "Fill8 world height",
        )?;
        if data == 0
            || width <= 0
            || height <= 0
            || rowbytes < width.saturating_mul(abi::PF_PIXEL_SIZE as i32)
        {
            return Ok(4);
        }
        let mut bounds = [0, 0, width, height];
        if area != 0 {
            for (index, value) in bounds.iter_mut().enumerate() {
                *value = read_guest_i32(unicorn, area + (index * 4) as u64, "Fill8 area field")?;
            }
            if bounds[0] < 0
                || bounds[1] < 0
                || bounds[2] < bounds[0]
                || bounds[3] < bounds[1]
                || bounds[2] > width
                || bounds[3] > height
            {
                return Ok(4);
            }
        }
        let mut pixel = [0u8; abi::PF_PIXEL_SIZE];
        if color != 0 {
            unicorn
                .mem_read(color, &mut pixel)
                .map_err(|error| format!("Fill8 color read: {error}"))?;
        }
        for y in bounds[1]..bounds[3] {
            for x in bounds[0]..bounds[2] {
                let offset = (y as u64)
                    .checked_mul(rowbytes as u64)
                    .and_then(|offset| offset.checked_add((x as u64) * abi::PF_PIXEL_SIZE as u64))
                    .ok_or_else(|| "Fill8 pixel offset overflow".to_string())?;
                unicorn
                    .mem_write(data + offset, &pixel)
                    .map_err(|error| format!("Fill8 pixel write: {error}"))?;
            }
        }
        Ok(0)
    })();
    match result {
        Ok(error) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, error);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

#[derive(Clone, Copy)]
struct Argb8World {
    data: u64,
    rowbytes: u64,
    width: i32,
    height: i32,
}

fn read_argb8_world(
    unicorn: &Unicorn<'_, GuestState>,
    world: u64,
    callback: &str,
) -> Result<Option<Argb8World>, String> {
    if world == 0 {
        return Ok(None);
    }
    let data = read_guest_u64(
        unicorn,
        world + abi::LAYER_DATA_OFFSET as u64,
        &format!("{callback} world data"),
    )?;
    let rowbytes = read_guest_i32(
        unicorn,
        world + abi::LAYER_ROWBYTES_OFFSET as u64,
        &format!("{callback} world rowbytes"),
    )?;
    let width = read_guest_i32(
        unicorn,
        world + abi::LAYER_WIDTH_OFFSET as u64,
        &format!("{callback} world width"),
    )?;
    let height = read_guest_i32(
        unicorn,
        world + abi::LAYER_HEIGHT_OFFSET as u64,
        &format!("{callback} world height"),
    )?;
    if data == 0
        || width <= 0
        || height <= 0
        || width > MAX_WORLD_DIMENSION
        || height > MAX_WORLD_DIMENSION
        || rowbytes < width.saturating_mul(abi::PF_PIXEL_SIZE as i32)
    {
        return Ok(None);
    }
    Ok(Some(Argb8World {
        data,
        rowbytes: rowbytes as u64,
        width,
        height,
    }))
}

fn read_argb8_pixel(
    unicorn: &Unicorn<'_, GuestState>,
    world: Argb8World,
    x: i32,
    y: i32,
    callback: &str,
) -> Result<[u8; 4], String> {
    if x < 0 || x >= world.width || y < 0 || y >= world.height {
        return Ok([0; 4]);
    }
    let address = (y as u64)
        .checked_mul(world.rowbytes)
        .and_then(|offset| offset.checked_add((x as u64) * abi::PF_PIXEL_SIZE as u64))
        .and_then(|offset| world.data.checked_add(offset))
        .ok_or_else(|| format!("{callback} pixel address overflow"))?;
    let mut pixel = [0u8; 4];
    unicorn
        .mem_read(address, &mut pixel)
        .map_err(|error| format!("{callback} source pixel: {error}"))?;
    Ok(pixel)
}

fn legacy_sample_arguments(
    unicorn: &Unicorn<'_, GuestState>,
    callback: &str,
) -> Result<Option<(i32, i32, u64, u64)>, String> {
    // AE's legacy sampling callbacks accept a null effect_ref (Adobe's
    // Displacement passes one), and neither sampling operation consumes it.
    // Keep the pointers that are actually dereferenced fail-closed below.
    let fixed_x = unicorn
        .reg_read(RegisterX86::RDX)
        .map_err(|error| format!("{callback} x: {error}"))? as u32 as i32;
    let fixed_y = unicorn
        .reg_read(RegisterX86::R8)
        .map_err(|error| format!("{callback} y: {error}"))? as u32 as i32;
    let params = unicorn
        .reg_read(RegisterX86::R9)
        .map_err(|error| format!("{callback} params: {error}"))?;
    let destination = aegp_stack_arg(unicorn, 0x28)?;
    Ok((params != 0 && destination != 0).then_some((fixed_x, fixed_y, params, destination)))
}

fn finish_legacy_sample(unicorn: &mut Unicorn<'_, GuestState>, result: Result<bool, String>) {
    match result {
        Ok(valid) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, if valid { 0 } else { 4 });
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn emulate_subpixel_sample8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let Some((fixed_x, fixed_y, params, destination)) =
            legacy_sample_arguments(unicorn, "SubpixelSample8")?
        else {
            return Ok(false);
        };
        let source_world = read_guest_u64(unicorn, params + 16, "SubpixelSample8 source world")?;
        let Some(world) = read_argb8_world(unicorn, source_world, "SubpixelSample8")? else {
            return Ok(false);
        };
        let x = fixed_x as f64 / 65536.0;
        let y = fixed_y as f64 / 65536.0;
        let x0 = x.floor() as i32;
        let y0 = y.floor() as i32;
        let fraction_x = x - x0 as f64;
        let fraction_y = y - y0 as f64;
        let mut samples = [[0u8; 4]; 4];
        for dy in 0..2 {
            for dx in 0..2 {
                samples[dy * 2 + dx] = read_argb8_pixel(
                    unicorn,
                    world,
                    x0 + dx as i32,
                    y0 + dy as i32,
                    "SubpixelSample8",
                )?;
            }
        }
        let mut output = [0u8; 4];
        for channel in 0..4 {
            let mut value = 0.0;
            for dy in 0..2 {
                for dx in 0..2 {
                    let weight = (if dx == 0 {
                        1.0 - fraction_x
                    } else {
                        fraction_x
                    }) * (if dy == 0 {
                        1.0 - fraction_y
                    } else {
                        fraction_y
                    });
                    value += f64::from(samples[dy * 2 + dx][channel]) * weight;
                }
            }
            output[channel] = value.round().clamp(0.0, 255.0) as u8;
        }
        unicorn
            .mem_write(destination, &output)
            .map_err(|error| format!("SubpixelSample8 destination pixel: {error}"))?;
        Ok(true)
    })();
    finish_legacy_sample(unicorn, result);
}

fn emulate_area_sample8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let Some((fixed_x, fixed_y, params, destination)) =
            legacy_sample_arguments(unicorn, "AreaSample8")?
        else {
            return Ok(false);
        };
        let fixed_radius_x = read_guest_i32(unicorn, params, "AreaSample8 radius x")?;
        let fixed_radius_y = read_guest_i32(unicorn, params + 4, "AreaSample8 radius y")?;
        let _fixed_area = read_guest_i32(unicorn, params + 8, "AreaSample8 area")?;
        let source_world = read_guest_u64(unicorn, params + 16, "AreaSample8 source world")?;
        let edge_behavior =
            read_guest_i32(unicorn, params + 24, "AreaSample8 edge behavior")? as u32;
        let radius_x = fixed_radius_x as f64 / 65536.0;
        let radius_y = fixed_radius_y as f64 / 65536.0;
        if radius_x <= 0.0 || radius_y <= 0.0 || radius_x >= 128.0 || radius_y >= 128.0 {
            return Ok(false);
        }
        let Some(world) = read_argb8_world(unicorn, source_world, "AreaSample8")? else {
            return Ok(false);
        };
        let center_x = fixed_x as f64 / 65536.0;
        let center_y = fixed_y as f64 / 65536.0;
        let left = center_x - radius_x;
        let right = center_x + radius_x;
        let top = center_y - radius_y;
        let bottom = center_y + radius_y;
        let footprint = (right - left) * (bottom - top);
        const EDGE_ZERO: u32 = 0;
        const EDGE_REPEAT: u32 = 1;
        const EDGE_WRAP: u32 = 2;
        let edge = if edge_behavior <= EDGE_WRAP {
            edge_behavior
        } else {
            EDGE_ZERO
        };
        let clip_to_image = edge == EDGE_ZERO || world.width <= 0 || world.height <= 0;
        let span_first_x = (left - 0.5).floor() as i32;
        let span_last_x = (right + 0.5).ceil() as i32;
        let span_first_y = (top - 0.5).floor() as i32;
        let span_last_y = (bottom + 0.5).ceil() as i32;
        let first_x = if clip_to_image {
            0.max(span_first_x)
        } else {
            span_first_x
        };
        let last_x = if clip_to_image {
            (world.width - 1).min(span_last_x)
        } else {
            span_last_x
        };
        let first_y = if clip_to_image {
            0.max(span_first_y)
        } else {
            span_first_y
        };
        let last_y = if clip_to_image {
            (world.height - 1).min(span_last_y)
        } else {
            span_last_y
        };
        let mut weighted_alpha = 0.0;
        let mut weighted_color = [0.0; 3];
        for y in first_y..=last_y {
            let overlap_y = 0.0f64.max(bottom.min(y as f64 + 0.5) - top.max(y as f64 - 0.5));
            for x in first_x..=last_x {
                let overlap_x = 0.0f64.max(right.min(x as f64 + 0.5) - left.max(x as f64 - 0.5));
                let weight = overlap_x * overlap_y;
                if weight == 0.0 {
                    continue;
                }
                let edge_mapped = |value: i32, extent: i32| {
                    if edge == EDGE_REPEAT {
                        value.clamp(0, extent - 1)
                    } else {
                        value.rem_euclid(extent)
                    }
                };
                let sample_x = if clip_to_image {
                    x
                } else {
                    edge_mapped(x, world.width)
                };
                let sample_y = if clip_to_image {
                    y
                } else {
                    edge_mapped(y, world.height)
                };
                let pixel = read_argb8_pixel(unicorn, world, sample_x, sample_y, "AreaSample8")?;
                let alpha = f64::from(pixel[0]) / 255.0;
                weighted_alpha += weight * alpha;
                for channel in 0..3 {
                    weighted_color[channel] += weight * alpha * f64::from(pixel[channel + 1]);
                }
            }
        }
        let mut output = [0u8; 4];
        output[0] = (weighted_alpha / footprint * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        for channel in 0..3 {
            output[channel + 1] = (if weighted_alpha > 0.0 {
                weighted_color[channel] / weighted_alpha
            } else {
                0.0
            })
            .round()
            .clamp(0.0, 255.0) as u8;
        }
        unicorn
            .mem_write(destination, &output)
            .map_err(|error| format!("AreaSample8 destination pixel: {error}"))?;
        Ok(true)
    })();
    finish_legacy_sample(unicorn, result);
}

fn emulate_transfer_rect8(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let effect_ref = unicorn
            .reg_read(RegisterX86::RCX)
            .map_err(|error| format!("TransferRect8 effect ref: {error}"))?;
        let quality = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("TransferRect8 quality: {error}"))?
            as u32 as i32;
        let mode_flags = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("TransferRect8 mode flags: {error}"))?
            as u32;
        let field = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("TransferRect8 field: {error}"))? as u32
            as i32;
        let source_rect = aegp_stack_arg(unicorn, 0x28)?;
        let source_world = aegp_stack_arg(unicorn, 0x30)?;
        let composite_mode = aegp_stack_arg(unicorn, 0x38)?;
        let mask_world = aegp_stack_arg(unicorn, 0x40)?;
        let destination_x = aegp_stack_arg(unicorn, 0x48)? as u32 as i32;
        let destination_y = aegp_stack_arg(unicorn, 0x50)? as u32 as i32;
        let destination_world = aegp_stack_arg(unicorn, 0x58)?;
        if effect_ref == 0
            || source_world == 0
            || composite_mode == 0
            || destination_world == 0
            || quality < 0
            || quality > 1
            || mode_flags > 1
            || field < 0
            || field > 2
            || !(-4096..=4096).contains(&destination_x)
            || !(-4096..=4096).contains(&destination_y)
        {
            return Ok(false);
        }
        let transfer_mode = read_guest_i32(unicorn, composite_mode, "TransferRect8 transfer mode")?;
        let mut mode_tail = [0u8; 4];
        unicorn
            .mem_read(composite_mode + 8, &mut mode_tail)
            .map_err(|error| format!("TransferRect8 composite mode: {error}"))?;
        let opacity = mode_tail[0];
        let rgb_only = mode_tail[1];
        if !(0..=2).contains(&transfer_mode) || rgb_only > 1 {
            return Ok(false);
        }
        let Some(source) = read_argb8_world(unicorn, source_world, "TransferRect8 source")? else {
            return Ok(false);
        };
        let Some(destination) =
            read_argb8_world(unicorn, destination_world, "TransferRect8 destination")?
        else {
            return Ok(false);
        };
        let bounds = if source_rect == 0 {
            [0, 0, source.width, source.height]
        } else {
            [
                read_guest_i32(unicorn, source_rect, "TransferRect8 source left")?,
                read_guest_i32(unicorn, source_rect + 4, "TransferRect8 source top")?,
                read_guest_i32(unicorn, source_rect + 8, "TransferRect8 source right")?,
                read_guest_i32(unicorn, source_rect + 12, "TransferRect8 source bottom")?,
            ]
        };
        // TransferRect8 consumes only the 8-bit opacity field. The adjacent
        // 16-bit field is unspecified for this pixel format and must not make
        // an otherwise valid 8-bit transfer fail. Clip the requested source
        // rectangle against both worlds in 64-bit arithmetic while retaining
        // its original top-left as the destination anchor.
        let bounds = bounds.map(i64::from);
        let destination_x = i64::from(destination_x);
        let destination_y = i64::from(destination_y);
        let clipped_left = bounds[0].max(bounds[0] - destination_x).max(0);
        let clipped_top = bounds[1].max(bounds[1] - destination_y).max(0);
        let clipped_right = bounds[2]
            .min(bounds[0] - destination_x + i64::from(destination.width))
            .min(i64::from(source.width));
        let clipped_bottom = bounds[3]
            .min(bounds[1] - destination_y + i64::from(destination.height))
            .min(i64::from(source.height));
        if clipped_right <= clipped_left || clipped_bottom <= clipped_top {
            return Ok(true);
        }
        let width = usize::try_from(clipped_right - clipped_left)
            .map_err(|_| "TransferRect8 width conversion failed".to_string())?;
        let height = usize::try_from(clipped_bottom - clipped_top)
            .map_err(|_| "TransferRect8 height conversion failed".to_string())?;
        let pixels = width
            .checked_mul(height)
            .filter(|count| *count <= 16_777_216)
            .ok_or_else(|| "TransferRect8 snapshot exceeds pixel bound".to_string())?;
        let mask_coverage = if mask_world == 0 {
            None
        } else {
            let Some(mask) = read_argb8_world(unicorn, mask_world, "TransferRect8 mask")? else {
                return Ok(false);
            };
            if mask.width > 4096 || mask.height > 4096 {
                return Ok(false);
            }
            let mask_offset_x = read_guest_i32(
                unicorn,
                mask_world + abi::PF_LAYER_DEF_SIZE as u64,
                "TransferRect8 mask offset x",
            )?;
            let mask_offset_y = read_guest_i32(
                unicorn,
                mask_world + abi::PF_LAYER_DEF_SIZE as u64 + 4,
                "TransferRect8 mask offset y",
            )?;
            let mask_flags = read_guest_i32(
                unicorn,
                mask_world + abi::PF_LAYER_DEF_SIZE as u64 + 8,
                "TransferRect8 mask flags",
            )? as u32;
            if mask_flags & !3 != 0 {
                return Ok(false);
            }
            let mut coverage = vec![0.0; pixels];
            for row in 0..height {
                let mask_y = clipped_top + row as i64 - i64::from(mask_offset_y);
                for column in 0..width {
                    let mask_x = clipped_left + column as i64 - i64::from(mask_offset_x);
                    let mut value = if mask_x >= 0
                        && mask_y >= 0
                        && mask_x < i64::from(mask.width)
                        && mask_y < i64::from(mask.height)
                    {
                        let address = mask
                            .data
                            .checked_add(mask_y as u64 * mask.rowbytes)
                            .and_then(|address| {
                                address.checked_add(mask_x as u64 * abi::PF_PIXEL_SIZE as u64)
                            })
                            .ok_or_else(|| {
                                "TransferRect8 mask pixel address overflow".to_string()
                            })?;
                        let mut pixel = [0u8; abi::PF_PIXEL_SIZE];
                        unicorn
                            .mem_read(address, &mut pixel)
                            .map_err(|error| format!("TransferRect8 mask pixel: {error}"))?;
                        if mask_flags & 2 != 0 {
                            (0.299 * f64::from(pixel[1])
                                + 0.587 * f64::from(pixel[2])
                                + 0.114 * f64::from(pixel[3]))
                                / 255.0
                        } else {
                            f64::from(pixel[0]) / 255.0
                        }
                    } else {
                        0.0
                    };
                    value = value.clamp(0.0, 1.0);
                    if mask_flags & 1 != 0 {
                        value = 1.0 - value;
                    }
                    coverage[row * width + column] = value;
                }
            }
            Some(coverage)
        };
        let mut snapshot = vec![0u8; pixels * abi::PF_PIXEL_SIZE];
        for row in 0..height {
            let source_y = u64::try_from(clipped_top + row as i64)
                .map_err(|_| "TransferRect8 source y conversion failed".to_string())?;
            let source_x = u64::try_from(clipped_left)
                .map_err(|_| "TransferRect8 source x conversion failed".to_string())?;
            let address = source
                .data
                .checked_add(source_y * source.rowbytes)
                .and_then(|address| address.checked_add(source_x * abi::PF_PIXEL_SIZE as u64))
                .ok_or_else(|| "TransferRect8 source row address overflow".to_string())?;
            let row_start = row * width * abi::PF_PIXEL_SIZE;
            let bytes = &mut snapshot[row_start..row_start + width * abi::PF_PIXEL_SIZE];
            unicorn
                .mem_read(address, bytes)
                .map_err(|error| format!("TransferRect8 source row: {error}"))?;
        }
        let opacity = f64::from(opacity) / 255.0;
        for row in 0..height {
            let source_y = clipped_top + row as i64;
            let output_y = destination_y + source_y - bounds[1];
            if (field == 1 && output_y & 1 != 0) || (field == 2 && output_y & 1 == 0) {
                continue;
            }
            for column in 0..width {
                let source_x = clipped_left + column as i64;
                let output_x = destination_x + source_x - bounds[0];
                let address = destination
                    .data
                    .checked_add(output_y as u64 * destination.rowbytes)
                    .and_then(|address| {
                        address.checked_add(output_x as u64 * abi::PF_PIXEL_SIZE as u64)
                    })
                    .ok_or_else(|| {
                        "TransferRect8 destination pixel address overflow".to_string()
                    })?;
                let input_start = (row * width + column) * abi::PF_PIXEL_SIZE;
                let input = &snapshot[input_start..input_start + abi::PF_PIXEL_SIZE];
                let mut output = [0u8; 4];
                unicorn
                    .mem_read(address, &mut output)
                    .map_err(|error| format!("TransferRect8 destination pixel read: {error}"))?;
                let effective_opacity = opacity
                    * mask_coverage
                        .as_ref()
                        .map_or(1.0, |coverage| coverage[row * width + column]);
                if transfer_mode == 0 {
                    for channel in if rgb_only == 0 { 0 } else { 1 }..4 {
                        output[channel] = (f64::from(input[channel]) * effective_opacity
                            + f64::from(output[channel]) * (1.0 - effective_opacity))
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                } else {
                    let source_alpha = f64::from(input[0]) / 255.0 * effective_opacity;
                    let destination_alpha = f64::from(output[0]) / 255.0;
                    let behind = transfer_mode == 1;
                    let output_alpha = if behind {
                        destination_alpha + source_alpha * (1.0 - destination_alpha)
                    } else {
                        source_alpha + destination_alpha * (1.0 - source_alpha)
                    };
                    for channel in 1..4 {
                        let value = if mode_flags == 1 {
                            if output_alpha == 0.0 {
                                0.0
                            } else if behind {
                                (f64::from(output[channel]) * destination_alpha
                                    + f64::from(input[channel])
                                        * source_alpha
                                        * (1.0 - destination_alpha))
                                    / output_alpha
                            } else {
                                (f64::from(input[channel]) * source_alpha
                                    + f64::from(output[channel])
                                        * destination_alpha
                                        * (1.0 - source_alpha))
                                    / output_alpha
                            }
                        } else if behind {
                            f64::from(output[channel])
                                + f64::from(input[channel])
                                    * effective_opacity
                                    * (1.0 - destination_alpha)
                        } else {
                            f64::from(input[channel]) * effective_opacity
                                + f64::from(output[channel]) * (1.0 - source_alpha)
                        };
                        output[channel] = value.round().clamp(0.0, 255.0) as u8;
                    }
                    if rgb_only == 0 {
                        output[0] = (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                }
                unicorn
                    .mem_write(address, &output)
                    .map_err(|error| format!("TransferRect8 destination pixel write: {error}"))?;
            }
        }
        Ok(true)
    })();
    finish_legacy_sample(unicorn, result);
}

fn emulate_get_callback_addr(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    let result = (|| {
        let callback_id = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("get-callback-address id: {error}"))?
            as u32;
        let output = aegp_stack_arg(unicorn, 0x28)?;
        if output == 0 {
            return Err("get-callback-address output is null".to_string());
        }
        let callback = match callback_id {
            9 => HOST_COPY,
            _ => {
                unicorn
                    .mem_write(output, &0u64.to_le_bytes())
                    .map_err(|error| format!("get-callback-address clear output: {error}"))?;
                return Ok(4);
            }
        };
        unicorn
            .mem_write(output, &callback.to_le_bytes())
            .map_err(|error| format!("get-callback-address output: {error}"))?;
        Ok(0)
    })();
    match result {
        Ok(error) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, error);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, 4);
            let _ = unicorn.emu_stop();
        }
    }
}

fn smart_checkout_world(
    unicorn: &Unicorn<'_, GuestState>,
    index: i32,
) -> Result<(u64, i32, i32), String> {
    let state = unicorn.get_data();
    let mut world = if index == 0 {
        state.smart_input_world
    } else {
        let offset = usize::try_from(index)
            .ok()
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| format!("smart checkout index is negative: {index}"))?;
        let parameter = state
            .params
            .get(offset)
            .ok_or_else(|| format!("smart checkout index is outside parameters: {index}"))?;
        if parameter.param_type != 0 {
            return Err(format!(
                "smart checkout index={index} is not a PF_Param_LAYER"
            ));
        }
        state
            .parameter_definitions
            .get(offset)
            .copied()
            .filter(|definition| *definition != 0)
            .ok_or_else(|| format!("smart checkout layer index={index} has no definition"))?
            + abi::PARAM_U_OFFSET as u64
    };
    if world == 0 {
        return Err(format!("smart checkout layer index={index} has no world"));
    }
    let mut data = read_guest_u64(
        unicorn,
        world + abi::LAYER_DATA_OFFSET as u64,
        "smart checkout world data",
    )?;
    let mut rowbytes = read_guest_i32(
        unicorn,
        world + abi::LAYER_ROWBYTES_OFFSET as u64,
        "smart checkout world rowbytes",
    )?;
    let mut width = read_guest_i32(
        unicorn,
        world + abi::LAYER_WIDTH_OFFSET as u64,
        "smart checkout world width",
    )?;
    let mut height = read_guest_i32(
        unicorn,
        world + abi::LAYER_HEIGHT_OFFSET as u64,
        "smart checkout world height",
    )?;
    // AE exposes an unselected secondary layer as an empty PF_LayerDef. SmartFX
    // effects commonly still checkout that declared layer slot and expect the
    // current input world. Preserve explicitly materialized secondary worlds,
    // but inherit the active input only for the all-zero unselected descriptor.
    let unselected_secondary = index != 0
        && unicorn
            .mem_read_as_vec(world, abi::PF_LAYER_DEF_SIZE)
            .map_err(|error| format!("smart checkout layer descriptor read failed: {error}"))?
            .iter()
            .all(|byte| *byte == 0);
    if unselected_secondary {
        world = state.smart_input_world;
        data = read_guest_u64(
            unicorn,
            world + abi::LAYER_DATA_OFFSET as u64,
            "smart checkout inherited world data",
        )?;
        rowbytes = read_guest_i32(
            unicorn,
            world + abi::LAYER_ROWBYTES_OFFSET as u64,
            "smart checkout inherited world rowbytes",
        )?;
        width = read_guest_i32(
            unicorn,
            world + abi::LAYER_WIDTH_OFFSET as u64,
            "smart checkout inherited world width",
        )?;
        height = read_guest_i32(
            unicorn,
            world + abi::LAYER_HEIGHT_OFFSET as u64,
            "smart checkout inherited world height",
        )?;
    }
    let pixel_bytes = match state.smart_pixel_format {
        crate::pixel::PF_PIXEL_FORMAT_ARGB32 => abi::PF_PIXEL_SIZE as i32,
        crate::pixel::PF_PIXEL_FORMAT_ARGB64 => abi::PF_PIXEL16_SIZE as i32,
        crate::pixel::PF_PIXEL_FORMAT_ARGB128 => abi::PF_PIXEL_FLOAT_SIZE as i32,
        format => return Err(format!("unsupported smart pixel format={format:#x}")),
    };
    if data == 0
        || width <= 0
        || height <= 0
        || width > MAX_WORLD_DIMENSION
        || height > MAX_WORLD_DIMENSION
        || rowbytes < width.saturating_mul(pixel_bytes)
    {
        return Err(format!(
            "invalid smart checkout world index={index} data={data:#x} rowbytes={rowbytes} dimensions={width}x{height}"
        ));
    }
    Ok((world, width, height))
}

fn emulate_pre_checkout_layer(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().pre_checkout_calls += 1;
    let result = (|| {
        let index = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("pre-checkout index: {error}"))? as i32;
        let checkout_id = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("pre-checkout id: {error}"))? as i32;
        if unicorn
            .get_data()
            .smart_checkout_ids
            .contains_key(&checkout_id)
        {
            return Err(format!(
                "duplicate smart checkout id={checkout_id} for input index=0"
            ));
        }
        if unicorn.get_data().smart_checkout_ids.len() >= MAX_SMART_CHECKOUT_IDS {
            return Err(format!(
                "smart checkout token capacity exceeded: {MAX_SMART_CHECKOUT_IDS}"
            ));
        }
        let request_pointer = unicorn
            .reg_read(RegisterX86::R9)
            .map_err(|error| format!("pre-checkout request: {error}"))?;
        let rsp = unicorn
            .reg_read(RegisterX86::RSP)
            .map_err(|error| format!("pre-checkout stack: {error}"))?;
        let mut result_pointer = [0u8; 8];
        unicorn
            .mem_read(rsp + 0x40, &mut result_pointer)
            .map_err(|error| format!("pre-checkout result pointer: {error}"))?;
        let result_pointer = u64::from_le_bytes(result_pointer);
        if result_pointer == 0 {
            return Err("pre-checkout result is null".to_string());
        }
        let time_step = read_guest_i32(unicorn, rsp + 0x30, "pre-checkout time step")?;
        let what_time = read_guest_i32(unicorn, rsp + 0x28, "pre-checkout time")?;
        let mut time_scale = [0u8; 4];
        unicorn
            .mem_read(rsp + 0x38, &mut time_scale)
            .map_err(|error| format!("pre-checkout time scale: {error}"))?;
        // A zero step is used by still-frame SmartFX callers. Negative steps
        // and a zero scale cannot describe a valid host time.
        if time_step < 0 || u32::from_le_bytes(time_scale) == 0 {
            return Err(format!(
                "invalid pre-checkout time step={time_step} scale={}",
                u32::from_le_bytes(time_scale)
            ));
        }
        let time_scale = u32::from_le_bytes(time_scale);
        let state = unicorn.get_data();
        if index != 0
            && i64::from(what_time) * i64::from(state.smart_current_time_scale)
                != i64::from(state.smart_current_time) * i64::from(time_scale)
        {
            return Err(format!(
                "unsupported temporal smart checkout time={what_time}/{time_scale} current={}/{}",
                state.smart_current_time, state.smart_current_time_scale
            ));
        }
        let (world, width, height) = smart_checkout_world(unicorn, index)?;
        let mut request_rect = [0, 0, width, height];
        let mut observed_request = request_rect;
        if request_pointer != 0 {
            let mut request_rect_bytes = [0u8; 16];
            unicorn
                .mem_read(request_pointer, &mut request_rect_bytes)
                .map_err(|error| format!("pre-checkout request rect: {error}"))?;
            for (element, value) in request_rect.iter_mut().enumerate() {
                let offset = element * 4;
                *value = i32::from_le_bytes(
                    request_rect_bytes[offset..offset + 4]
                        .try_into()
                        .expect("render request rectangle element is four bytes"),
                );
            }
            if request_rect[2] < request_rect[0] || request_rect[3] < request_rect[1] {
                return Err(format!("malformed pre-checkout rectangle={request_rect:?}"));
            }
            observed_request = request_rect;
            request_rect = [
                request_rect[0].max(0),
                request_rect[1].max(0),
                request_rect[2].min(width),
                request_rect[3].min(height),
            ];
            if request_rect[0] >= request_rect[2] || request_rect[1] >= request_rect[3] {
                request_rect = [0; 4];
            }
        }
        unicorn
            .get_data_mut()
            .pre_checkout_requests
            .push(observed_request);
        let mut bytes = [0u8; 76];
        for (offset, value) in request_rect
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index * 4, value))
            .chain([
                (16, 0),
                (20, 0),
                (24, width),
                (28, height),
                (32, 1),
                (36, 1),
                (44, width),
                (48, height),
            ])
        {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        unicorn
            .mem_write(result_pointer, &bytes)
            .map_err(|error| format!("pre-checkout result write: {error}"))?;
        unicorn.get_data_mut().smart_checkout_ids.insert(
            checkout_id,
            SmartCheckout {
                index,
                world,
                checked_out: false,
            },
        );
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkout_layer_pixels(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkout_pixels_calls += 1;
    let result = (|| {
        let checkout_id = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-pixels id: {error}"))?
            as i32;
        let output = unicorn
            .reg_read(RegisterX86::R8)
            .map_err(|error| format!("checkout-pixels output: {error}"))?;
        let checkout = unicorn
            .get_data()
            .smart_checkout_ids
            .get(&checkout_id)
            .copied();
        let Some(checkout) = checkout else {
            return Err(format!(
                "invalid checkout-pixels id={checkout_id} output={output:#x}"
            ));
        };
        // A legal empty PF_CheckoutResult describes pixel availability, not
        // the lifetime of the host-owned PF_EffectWorld. Some effects still
        // check the world out to inspect its descriptor before doing no work.
        if output == 0 {
            return Err(format!(
                "invalid checkout-pixels id={checkout_id} index={} output={output:#x}",
                checkout.index
            ));
        }
        // A checkout id names one host-owned world, not one output variable.
        // OLMRadialBlur obtains the same primary id into two local pointers and
        // checks the id in once. Replaying a registered id is therefore
        // idempotent; the mapped destination is still validated by mem_write.
        unicorn
            .mem_write(output, &checkout.world.to_le_bytes())
            .map_err(|error| format!("checkout-pixels world write: {error}"))?;
        unicorn
            .get_data_mut()
            .smart_checkout_ids
            .get_mut(&checkout_id)
            .expect("checkout token remains present")
            .checked_out = true;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkin_layer_pixels(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkin_pixels_calls += 1;
    let result = (|| {
        let checkout_id = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkin-pixels id: {error}"))?
            as u32 as i32;
        if unicorn
            .get_data()
            .smart_checkout_ids
            .get(&checkout_id)
            .is_none_or(|checkout| !checkout.checked_out)
        {
            return Err(format!("invalid checkin-pixels id={checkout_id}"));
        }
        unicorn
            .get_data_mut()
            .smart_checkout_ids
            .get_mut(&checkout_id)
            .expect("checkout token remains present")
            .checked_out = false;
        Ok(())
    })();
    finish_callback(unicorn, result);
}

fn emulate_checkout_output(unicorn: &mut Unicorn<'_, GuestState>, _: u64, _: u32) {
    unicorn.get_data_mut().checkout_output_calls += 1;
    let result = (|| {
        let output = unicorn
            .reg_read(RegisterX86::RDX)
            .map_err(|error| format!("checkout-output pointer: {error}"))?;
        let output_world = unicorn.get_data().smart_output_world;
        if output == 0 || output_world == 0 {
            return Err(format!("invalid checkout-output pointer={output:#x}"));
        }
        unicorn
            .mem_write(output, &output_world.to_le_bytes())
            .map_err(|error| format!("checkout-output world write: {error}"))?;
        Ok(())
    })();
    finish_callback(unicorn, result);
}
