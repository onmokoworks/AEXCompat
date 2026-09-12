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
    // Last page of this session's environment namespace is reserved for the
    // borrowed getenv result. Snapshot blocks cannot allocate in that page.
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
            unicorn.get_data_mut().crt_errno = errno;
            let _ = unicorn.reg_write(RegisterX86::RAX, u32::MAX as u64);
        }
    }
}
