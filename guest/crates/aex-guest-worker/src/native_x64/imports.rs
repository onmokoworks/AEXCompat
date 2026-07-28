use super::*;

pub(super) fn validate_native_import(name: &str) -> Result<(), GuestError> {
    // PeImage records the export name from the PE import directory. The
    // compiler-side COFF `__imp__CxxThrowException` alias is not an imported
    // function name, and x86 stdcall decoration cannot occur in the AMD64-only
    // images accepted by PeImage. Match the actual x64 runtime export exactly
    // so similarly named plugin functions are not rejected.
    if name == "_CxxThrowException" {
        return Err(GuestError::UnsupportedImport {
            name: name.to_string(),
        });
    }
    if native_import_is_implemented(name) {
        return Ok(());
    }
    if name.starts_with("_vcomp_") || msvc_udt_by_value_return_import(name) {
        return Err(GuestError::UnsupportedImport {
            name: name.to_string(),
        });
    }
    Ok(())
}

pub(super) fn native_import_is_implemented(name: &str) -> bool {
    matches!(
        name,
        "malloc"
            | "calloc"
            | "free"
            | "_callnewh"
            | "strncpy"
            | "memset"
            | "expf"
            | "floorf"
            | "powf"
            | "pow"
            | "omp_get_max_threads"
    )
}

pub(super) fn native_import_callback(name: &str) -> u64 {
    match name {
        "malloc" => callback_address!(native_crt_malloc),
        "calloc" => callback_address!(native_crt_calloc),
        "free" => callback_address!(native_crt_free),
        "_callnewh" => callback_address!(noop_import),
        "strncpy" => callback_address!(native_strncpy),
        "memset" => callback_address!(native_memset),
        "expf" => callback_address!(native_expf),
        "floorf" => callback_address!(native_floorf),
        "powf" => callback_address!(native_powf),
        "pow" => callback_address!(native_pow),
        "omp_get_max_threads" => callback_address!(native_omp_get_max_threads),
        _ => callback_address!(noop_import),
    }
}

pub(super) unsafe extern "win64" fn native_crt_malloc(
    size: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    native_crt_allocate(size)
}

pub(super) unsafe extern "win64" fn native_crt_calloc(
    count: u64,
    element_size: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    CrtHeap::checked_calloc_size(count, element_size)
        .map(native_crt_allocate)
        .unwrap_or(0)
}

pub(super) fn native_crt_allocate(requested_size: u64) -> u64 {
    with_state(|state| {
        let allocation = match state.crt_heap.prepare_allocation(requested_size) {
            Ok(allocation) => allocation,
            Err(_) => return 0,
        };
        let pointer = unsafe {
            mmap(
                ptr::null_mut(),
                allocation.backing_size as usize,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if pointer as usize == usize::MAX {
            return 0;
        }
        let address = pointer as u64;
        if let Err(error) = state.crt_heap.insert(address, allocation) {
            unsafe {
                munmap(pointer, allocation.backing_size as usize);
            }
            state.callback_error = Some(error.to_string());
            return 0;
        }
        address
    })
    .unwrap_or(0)
}

pub(super) unsafe extern "win64" fn native_crt_free(
    pointer: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if pointer == 0 {
        return 0;
    }
    with_state(|state| match state.crt_heap.remove(pointer) {
        Ok(allocation) => {
            if unsafe { munmap(pointer as *mut c_void, allocation.backing_size as usize) } != 0 {
                state.callback_error = Some(format!("unmap CRT allocation {pointer:#x} failed"));
            }
        }
        Err(error) => state.callback_error = Some(error.to_string()),
    });
    0
}

pub(super) unsafe extern "win64" fn poison_callback(
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| state.callback_error = Some("unsupported host callback".into()));
    u32::MAX as u64
}

pub(super) unsafe extern "win64" fn native_memset(
    destination: u64,
    value: u64,
    length: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if length <= ARENA_SIZE as u64 {
        unsafe {
            ptr::write_bytes(destination as *mut u8, value as u8, length as usize);
        }
        destination
    } else {
        0
    }
}

pub(super) unsafe extern "win64" fn native_strncpy(
    destination: u64,
    source: u64,
    count: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    if count > 4096 {
        return 0;
    }
    let mut terminated = false;
    for index in 0..count {
        let byte = if terminated {
            0
        } else {
            let value = unsafe { *((source + index) as *const u8) };
            terminated = value == 0;
            value
        };
        unsafe {
            *((destination + index) as *mut u8) = byte;
        }
    }
    destination
}

pub(super) unsafe extern "win64" fn native_strcpy(
    destination: u64,
    source: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    for index in 0..4096u64 {
        let byte = unsafe { *((source + index) as *const u8) };
        unsafe {
            *((destination + index) as *mut u8) = byte;
        }
        if byte == 0 {
            return destination;
        }
    }
    0
}

pub(super) unsafe extern "win64" fn native_ansi_sprintf(
    destination: u64,
    format: u64,
    _: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    with_state(|state| {
        if destination == 0 || format == 0 {
            return u32::MAX as u64;
        }
        let mut output = Vec::new();
        let mut cursor = 0u64;
        while cursor < 256 {
            let Some(address) = format.checked_add(cursor) else {
                return u32::MAX as u64;
            };
            if !native_guest_range_valid(state, address, 1) {
                return u32::MAX as u64;
            }
            let byte = unsafe { *(address as *const u8) };
            if byte == 0 {
                if !native_guest_range_valid(state, destination, output.len() as u64 + 1) {
                    return u32::MAX as u64;
                }
                unsafe {
                    ptr::copy_nonoverlapping(output.as_ptr(), destination as *mut u8, output.len());
                    *((destination + output.len() as u64) as *mut u8) = 0;
                }
                return output.len() as u64;
            }
            if byte == b'%' {
                let next = cursor + 1;
                let Some(next_address) = format.checked_add(next) else {
                    return u32::MAX as u64;
                };
                if next >= 256 || !native_guest_range_valid(state, next_address, 1) {
                    return u32::MAX as u64;
                }
                if unsafe { *(next_address as *const u8) } != b'%' {
                    // Rust cannot safely consume an arbitrary Win64 C vararg
                    // list. Keep conversion formats fail-closed; literal
                    // diagnostics and escaped percent signs remain supported.
                    return u32::MAX as u64;
                }
                output.push(b'%');
                cursor += 2;
                continue;
            }
            output.push(byte);
            cursor += 1;
        }
        u32::MAX as u64
    })
    .unwrap_or(u32::MAX as u64)
}

pub(super) unsafe extern "win64" fn native_expf(value: f32) -> f32 {
    value.exp()
}
pub(super) unsafe extern "win64" fn native_floorf(value: f32) -> f32 {
    value.floor()
}
pub(super) unsafe extern "win64" fn native_powf(left: f32, right: f32) -> f32 {
    left.powf(right)
}
pub(super) unsafe extern "win64" fn native_pow(left: f64, right: f64) -> f64 {
    left.powf(right)
}

pub(super) unsafe extern "win64" fn add_param(
    _: u64,
    index: u64,
    definition: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    let bytes = unsafe { copy_from_pointer(definition, abi::PF_PARAM_DEF_SIZE) };
    let param_type = i32::from_le_bytes(
        bytes[abi::PARAM_PARAM_TYPE_OFFSET..abi::PARAM_PARAM_TYPE_OFFSET + 4]
            .try_into()
            .expect("generated field is four bytes"),
    );
    let name_bytes = &bytes[abi::PARAM_NAME_OFFSET..abi::PARAM_NAME_OFFSET + abi::PARAM_NAME_SIZE];
    let name_end = name_bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name_bytes.len());
    let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
    with_state(|state| {
        state.params.push(GuestParam {
            index: index as i32,
            param_type,
            name,
            bytes,
        });
    });
    0
}

pub(super) unsafe extern "win64" fn copy_world(
    _: u64,
    source: u64,
    destination: u64,
    _: u64,
    _: u64,
    _: u64,
) -> u64 {
    let read_u64 = |base: u64, offset: usize| unsafe { *((base + offset as u64) as *const u64) };
    let read_i32 = |base: u64, offset: usize| unsafe { *((base + offset as u64) as *const i32) };
    let source_data = read_u64(source, abi::LAYER_DATA_OFFSET);
    let destination_data = read_u64(destination, abi::LAYER_DATA_OFFSET);
    let source_rowbytes = read_i32(source, abi::LAYER_ROWBYTES_OFFSET).max(0) as usize;
    let destination_rowbytes = read_i32(destination, abi::LAYER_ROWBYTES_OFFSET).max(0) as usize;
    let height = read_i32(source, abi::LAYER_HEIGHT_OFFSET)
        .min(read_i32(destination, abi::LAYER_HEIGHT_OFFSET))
        .max(0) as usize;
    let row_size = source_rowbytes.min(destination_rowbytes);
    for row in 0..height {
        unsafe {
            ptr::copy_nonoverlapping(
                (source_data as *const u8).add(row * source_rowbytes),
                (destination_data as *mut u8).add(row * destination_rowbytes),
                row_size,
            );
        }
    }
    0
}
