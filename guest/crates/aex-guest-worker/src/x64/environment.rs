const MAX_GUEST_ENVIRONMENT_ASSIGNMENT: usize = 4095;
const MAX_GUEST_ENVIRONMENT_OVERRIDES: usize = 256;

fn guest_environment_value(state: &GuestState, name: &[u8]) -> Option<Vec<u8>> {
    let key = name.to_ascii_uppercase();
    if let Some(value) = state.environment_overrides.get(&key) {
        return value.clone();
    }
    deterministic_guest_environment_value(name).map(Vec::from)
}

fn guest_environment_block_w(state: &GuestState) -> Vec<u8> {
    let mut entries: BTreeMap<Vec<u8>, Vec<u8>> = deterministic_guest_environment_entries()
        .iter()
        .map(|(name, value)| (name.to_ascii_uppercase(), value.to_vec()))
        .collect();
    for (name, value) in &state.environment_overrides {
        if let Some(value) = value {
            entries.insert(name.clone(), value.clone());
        } else {
            entries.remove(name);
        }
    }
    let mut block = Vec::new();
    for (name, value) in entries {
        for byte in name.into_iter().chain([b'=']).chain(value).chain([0]) {
            block.extend_from_slice(&u16::from(byte).to_le_bytes());
        }
    }
    if block.is_empty() {
        block.extend_from_slice(&[0, 0]);
    }
    block.extend_from_slice(&[0, 0]);
    block
}

fn guest_getenv_buffer(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    if let Some(address) = unicorn.get_data().getenv_buffer {
        return Ok(address);
    }
    // The reserved tail starts with the narrow borrowed result page.
    // Wide results follow it; snapshot blocks cannot allocate in this tail.
    let (_, address) = environment_strings_range(unicorn.get_data_mut())?;
    unicorn
        .mem_map(address, PAGE_SIZE, Prot::READ | Prot::WRITE)
        .map_err(|e| e.to_string())?;
    unicorn.get_data_mut().getenv_buffer = Some(address);
    Ok(address)
}

fn emulate_crt_putenv(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), u32> {
        let pointer = unicorn.reg_read(RegisterX86::RCX).unwrap_or_default();
        if pointer == 0 {
            return Err(22);
        }
        let mut assignment = Vec::new();
        let mut terminated = false;
        for offset in 0..=MAX_GUEST_ENVIRONMENT_ASSIGNMENT {
            let address = pointer.checked_add(offset as u64).ok_or(22u32)?;
            if !guest_range_has_permission(unicorn, address, 1, Prot::READ).unwrap_or(false) {
                return Err(22);
            }
            let mut byte = [0];
            unicorn.mem_read(address, &mut byte).map_err(|_| 22u32)?;
            if byte[0] == 0 {
                terminated = true;
                break;
            }
            assignment.push(byte[0]);
        }
        if !terminated || !assignment.is_ascii() {
            return Err(22);
        }
        let separator = assignment.iter().position(|b| *b == b'=').ok_or(22u32)?;
        if separator == 0 || separator > MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
            return Err(22);
        }
        let name = assignment[..separator].to_ascii_uppercase();
        let value = &assignment[separator + 1..];
        if !unicorn.get_data().environment_overrides.contains_key(&name)
            && unicorn.get_data().environment_overrides.len() >= MAX_GUEST_ENVIRONMENT_OVERRIDES
        {
            return Err(12);
        }
        unicorn.get_data_mut().environment_overrides.insert(
            name,
            if value.is_empty() {
                None
            } else {
                Some(value.to_vec())
            },
        );
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, 0);
        }
        Err(errno) => {
            if let Err(error) = set_guest_crt_errno(unicorn, errno) {
                finish_guest_stdio(unicorn, Err(error));
                return;
            }
            let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
        }
    }
}

fn emulate_crt_putenv_s(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<(), u32> {
        let name_pointer = unicorn.reg_read(RegisterX86::RCX).map_err(|_| 22u32)?;
        let value_pointer = unicorn.reg_read(RegisterX86::RDX).map_err(|_| 22u32)?;
        if name_pointer == 0 || value_pointer == 0 {
            return Err(22);
        }
        let name = read_crt_stdio_c_string(
            unicorn,
            name_pointer,
            (MAX_WINDOWS_ENVIRONMENT_NAME_BYTES + 1) as u64,
            "_putenv_s name",
        )
        .map_err(|_| 22u32)?;
        let value = read_crt_stdio_c_string(
            unicorn,
            value_pointer,
            (MAX_GUEST_ENVIRONMENT_ASSIGNMENT + 1) as u64,
            "_putenv_s value",
        )
        .map_err(|_| 22u32)?;
        if name.is_empty() || !name.is_ascii() || !value.is_ascii() || name.contains(&b'=') {
            return Err(22);
        }
        let name = name.to_ascii_uppercase();
        if !unicorn.get_data().environment_overrides.contains_key(&name)
            && unicorn.get_data().environment_overrides.len() >= MAX_GUEST_ENVIRONMENT_OVERRIDES
        {
            return Err(12);
        }
        unicorn.get_data_mut().environment_overrides.insert(
            name,
            if value.is_empty() { None } else { Some(value) },
        );
        Ok(())
    })();
    let returned = match result {
        Ok(()) => 0,
        Err(errno) => {
            if let Err(error) = set_guest_crt_errno(unicorn, errno) {
                finish_guest_stdio(unicorn, Err(error));
                return;
            }
            errno
        }
    };
    let _ = unicorn.reg_write(RegisterX86::RAX, u64::from(returned));
}

// One narrow page followed by independent wide results, each large enough for
// the maximum supported environment assignment. Results are borrowed for the
// engine lifetime; querying another variable never invalidates a prior pointer.
const MAX_GUEST_WGETENV_BUFFERS: usize = MAX_GUEST_ENVIRONMENT_OVERRIDES + 1;
const GUEST_WENVIRON_BYTES: u64 = 4 * 1024 * 1024;
const GUEST_ENVIRONMENT_BORROWED_BYTES: u64 =
    PAGE_SIZE * (1 + 2 * MAX_GUEST_WGETENV_BUFFERS as u64) + GUEST_WENVIRON_BYTES;

fn emulate_crt_wenviron(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let (_, base) = environment_strings_range(unicorn.get_data_mut())?;
        let cell = base + PAGE_SIZE * (1 + 2 * MAX_GUEST_WGETENV_BUFFERS as u64);
        if unicorn.get_data().wenviron_cell.is_none() {
            unicorn
                .mem_map(cell, GUEST_WENVIRON_BYTES, Prot::READ | Prot::WRITE)
                .map_err(|error| format!("CRT __p__wenviron storage map failed: {error}"))?;
            unicorn.get_data_mut().wenviron_cell = Some(cell);
        }
        let mut entries: BTreeMap<Vec<u8>, Vec<u8>> = deterministic_guest_environment_entries()
            .iter()
            .map(|(name, value)| (name.to_ascii_uppercase(), value.to_vec()))
            .collect();
        for (name, value) in &unicorn.get_data().environment_overrides {
            if let Some(value) = value {
                entries.insert(name.clone(), value.clone());
            } else {
                entries.remove(name);
            }
        }
        let array = cell + 8;
        let strings = array + (entries.len() as u64 + 1) * 8;
        let mut pointers = Vec::with_capacity((entries.len() + 1) * 8);
        let mut text = Vec::new();
        for (name, value) in entries {
            pointers.extend_from_slice(&(strings + text.len() as u64).to_le_bytes());
            for byte in name.into_iter().chain([b'=']).chain(value).chain([0]) {
                text.extend_from_slice(&u16::from(byte).to_le_bytes());
            }
        }
        pointers.extend_from_slice(&0u64.to_le_bytes());
        let used = 8usize
            .checked_add(pointers.len())
            .and_then(|size| size.checked_add(text.len()))
            .ok_or("CRT __p__wenviron storage size overflow")?;
        if used as u64 > GUEST_WENVIRON_BYTES {
            return Err("CRT __p__wenviron storage exceeds bound".into());
        }
        unicorn.mem_write(cell, &array.to_le_bytes()).map_err(|error| format!("CRT __p__wenviron cell write failed: {error}"))?;
        unicorn.mem_write(array, &pointers).map_err(|error| format!("CRT __p__wenviron pointer array write failed: {error}"))?;
        unicorn.mem_write(strings, &text).map_err(|error| format!("CRT __p__wenviron strings write failed: {error}"))?;
        Ok(cell)
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

fn emulate_crt_wgetenv(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        let pointer = read_win64_import_argument(unicorn, 0)?;
        if pointer == 0 {
            return Err("CRT _wgetenv requires an invalid parameter handler for null name".into());
        }
        let mut units = Vec::new();
        for index in 0..=MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
            let address = pointer
                .checked_add(index as u64 * 2)
                .ok_or("CRT _wgetenv name address overflow")?;
            if !guest_range_has_permission(unicorn, address, 2, Prot::READ)? {
                return Err("CRT _wgetenv name is not readable".into());
            }
            let bytes = unicorn
                .mem_read_as_vec(address, 2)
                .map_err(|e| e.to_string())?;
            let unit = u16::from_le_bytes([bytes[0], bytes[1]]);
            if unit == 0 {
                break;
            }
            if index == MAX_WINDOWS_ENVIRONMENT_NAME_BYTES {
                return Err("CRT _wgetenv name exceeds supported bound".into());
            }
            units.push(unit);
        }
        let name = String::from_utf16(&units).map_err(|_| "CRT _wgetenv invalid UTF-16 name")?;
        let key = name.as_bytes().to_ascii_uppercase();
        let Some(value) = guest_environment_value(unicorn.get_data(), &key) else {
            return Ok(0);
        };
        // Current guest environment assignments are ASCII. Do not silently
        // invent a code-page conversion if that environment model is extended.
        if !value.is_ascii() || value.contains(&0) || value.len() > MAX_GUEST_ENVIRONMENT_ASSIGNMENT
        {
            return Err("CRT _wgetenv unsupported environment value encoding or length".into());
        }
        let mut bytes = Vec::with_capacity((value.len() + 1) * 2);
        for byte in value.iter().copied().chain([0]) {
            bytes.extend_from_slice(&u16::from(byte).to_le_bytes());
        }
        let address = if let Some(address) = unicorn.get_data().wgetenv_buffers.get(&key) {
            *address
        } else {
            let count = unicorn.get_data().wgetenv_buffers.len();
            if count >= MAX_GUEST_WGETENV_BUFFERS {
                return Err("CRT _wgetenv borrowed storage exhausted".into());
            }
            let (_, base) = environment_strings_range(unicorn.get_data_mut())?;
            let address = base + PAGE_SIZE + count as u64 * PAGE_SIZE * 2;
            unicorn
                .mem_map(address, PAGE_SIZE * 2, Prot::READ | Prot::WRITE)
                .map_err(|e| e.to_string())?;
            unicorn.get_data_mut().wgetenv_buffers.insert(key, address);
            address
        };
        if !guest_range_has_permission(unicorn, address, bytes.len() as u64, Prot::WRITE)? {
            return Err("CRT _wgetenv borrowed storage is not writable".into());
        }
        unicorn
            .mem_write(address, &bytes)
            .map_err(|e| e.to_string())?;
        Ok(address)
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
