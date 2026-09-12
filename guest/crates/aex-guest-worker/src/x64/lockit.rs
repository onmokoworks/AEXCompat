// Native MSVC _Lockit(int) ABI stores an int at this+0 and holds the
// corresponding recursive runtime lock until destruction. Reference:
// https://github.com/microsoft/STL/blob/main/stl/src/xlock.cpp
fn emulate_msvcp_lockit(unicorn: &mut Unicorn<'_, GuestState>, destroy: bool) {
    let result = (|| -> Result<(), String> {
        let object = read_win64_import_argument(unicorn, 0)?;
        let permission = if destroy { Prot::READ } else { Prot::WRITE };
        if object == 0 || !guest_range_has_permission(unicorn, object, 4, permission)? {
            return Err("Lockit object storage is inaccessible".into());
        }
        let thread = unicorn.get_data().current_windows_thread_id;
        if destroy {
            let (kind, owner) = unicorn
                .get_data()
                .msvcp_lockit_objects
                .get(&object)
                .copied()
                .ok_or("Lockit destructor has no live constructor")?;
            let mut bytes = [0; 4];
            unicorn
                .mem_read(object, &mut bytes)
                .map_err(|e| e.to_string())?;
            if owner != thread || i32::from_le_bytes(bytes) != kind {
                return Err("Lockit destructor owner or stored kind mismatch".into());
            }
            if kind < 8 {
                let slot = &mut unicorn.get_data_mut().msvcp_lockit_locks[kind as usize];
                let (owner, depth) = slot.ok_or("Lockit lock is not held")?;
                if owner != thread || depth == 0 {
                    return Err("Lockit ownership mismatch".into());
                }
                *slot = if depth == 1 {
                    None
                } else {
                    Some((owner, depth - 1))
                };
            }
            unicorn.get_data_mut().msvcp_lockit_objects.remove(&object);
        } else {
            let kind = read_win64_import_argument(unicorn, 1)? as i32;
            if kind < 0 {
                return Err("negative Lockit kind is invalid".into());
            }
            if unicorn
                .get_data()
                .msvcp_lockit_objects
                .contains_key(&object)
            {
                return Err("Lockit object is already live".into());
            }
            if unicorn.get_data().msvcp_lockit_objects.len() >= 4096 {
                return Err("Lockit live object bound exceeded".into());
            }
            let next = if kind < 8 {
                match unicorn.get_data().msvcp_lockit_locks[kind as usize] {
                    None => Some((thread, 1)),
                    Some((owner, depth)) if owner == thread => {
                        Some((thread, depth.checked_add(1).ok_or("Lockit depth overflow")?))
                    }
                    Some(_) => return Err("contended Lockit requires scheduler support".into()),
                }
            } else {
                None
            }; // native constructor stores kinds >=8 without locking
            unicorn
                .mem_write(object, &kind.to_le_bytes())
                .map_err(|e| e.to_string())?;
            if kind < 8 {
                unicorn.get_data_mut().msvcp_lockit_locks[kind as usize] = next;
            }
            unicorn
                .get_data_mut()
                .msvcp_lockit_objects
                .insert(object, (kind, thread));
            unicorn
                .reg_write(RegisterX86::RAX, object)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if unicorn.get_data().callback_error.is_none() {
            unicorn.get_data_mut().callback_error = Some(error);
        }
        let _ = unicorn.emu_stop();
    }
}

// The CRT locale lock is recursive and shared with _Lockit(_LOCK_LOCALE=0).
// See Microsoft STL xlock.cpp and yvals.h. Contention stays explicit until
// blocking guest-thread scheduling is available for this internal CRT API.
fn emulate_crt_locale_lock(unicorn: &mut Unicorn<'_, GuestState>, release: bool) {
    let result = (|| -> Result<u64, String> {
        let thread = unicorn.get_data().current_windows_thread_id;
        let current = unicorn.get_data().msvcp_lockit_locks[0];
        let next = if release {
            match current {
                Some((owner, 1)) if owner == thread => None,
                Some((owner, depth)) if owner == thread && depth > 1 => Some((owner, depth - 1)),
                _ => return Err("CRT locale unlock has no matching thread ownership".into()),
            }
        } else {
            match current {
                None => Some((thread, 1)),
                Some((owner, depth)) if owner == thread => Some((
                    owner,
                    depth
                        .checked_add(1)
                        .ok_or("CRT locale lock depth overflow")?,
                )),
                Some(_) => return Err("contended CRT locale lock is not implemented".into()),
            }
        };
        unicorn.get_data_mut().msvcp_lockit_locks[0] = next;
        Ok(0)
    })();
    finish_guest_stdio(unicorn, result);
}

// Microsoft setlocale/_wsetlocale: NULL queries the current category. All CRT
// categories start in C. Non-C mutation must not silently leave ctype in C.
// https://learn.microsoft.com/cpp/c-runtime-library/reference/setlocale-wsetlocale
fn emulate_crt_setlocale(unicorn: &mut Unicorn<'_, GuestState>, wide: bool) {
    let result = (|| -> Result<u64, String> {
        let category = read_win64_import_argument(unicorn, 0)? as i32;
        let locale = read_win64_import_argument(unicorn, 1)?;
        if !(0..=5).contains(&category) {
            return Err(
                "_wsetlocale invalid category requires CRT invalid-parameter handling".into(),
            );
        }
        let width = if wide { 2 } else { 1 };
        let result_offset = if wide { 0 } else { 4 };
        if locale != 0 {
            for (index, expected) in [u16::from(b'C'), 0].into_iter().enumerate() {
                let address = locale
                    .checked_add((index * width) as u64)
                    .ok_or("_wsetlocale string address overflow")?;
                if !guest_range_has_permission(unicorn, address, width as u64, Prot::READ)? {
                    return Err("_wsetlocale string is inaccessible".into());
                }
                let mut bytes = [0; 2];
                unicorn
                    .mem_read(address, &mut bytes[..width])
                    .map_err(|e| e.to_string())?;
                if u16::from_le_bytes(bytes) != expected {
                    return Err("_wsetlocale non-C locale selection is not implemented".into());
                }
            }
        }
        if let Some(address) = unicorn.get_data().crt_wlocale_buffer {
            return Ok(address + result_offset);
        }
        // One borrowed read-only page after both reserved FILE namespaces.
        let address = GUEST_STREAM_BUFFER_BASE + MAX_GUEST_STREAM_OPENS * PAGE_SIZE;
        unicorn
            .mem_map(address, PAGE_SIZE, Prot::READ)
            .map_err(|e| e.to_string())?;
        unicorn
            .mem_write(address, &[b'C', 0, 0, 0, b'C', 0])
            .map_err(|e| e.to_string())?;
        unicorn.get_data_mut().crt_wlocale_buffer = Some(address);
        Ok(address + result_offset)
    })();
    finish_guest_stdio(unicorn, result);
}

// UCRT ctype.h permits signed-char indexing down to -127. Reserve -128 too;
// all negative entries (including EOF -1) and high bytes are unclassified in C.
// Mask ABI: Microsoft WinSDK ucrt/corecrt_wctype.h; _BLANK excludes tab.
fn emulate_crt_pctype(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        if let Some(address) = unicorn.get_data().crt_pctype_buffer {
            return Ok(address);
        }
        let base = GUEST_STREAM_BUFFER_BASE + MAX_GUEST_STREAM_OPENS * PAGE_SIZE + PAGE_SIZE;
        let address = base + 256;
        let mut table = [0u8; 768];
        for byte in 0u8..=127 {
            let mut mask: u16 = 0;
            if byte.is_ascii_uppercase() {
                mask |= 0x01;
            }
            if byte.is_ascii_lowercase() {
                mask |= 0x02;
            }
            if byte.is_ascii_digit() {
                mask |= 0x04;
            }
            if matches!(byte, 9..=13 | 32) {
                mask |= 0x08;
            }
            if byte.is_ascii_punctuation() {
                mask |= 0x10;
            }
            if byte.is_ascii_control() {
                mask |= 0x20;
            }
            if byte == b' ' {
                mask |= 0x40;
            }
            if byte.is_ascii_hexdigit() {
                mask |= 0x80;
            }
            let offset = 256 + usize::from(byte) * 2;
            table[offset..offset + 2].copy_from_slice(&mask.to_le_bytes());
        }
        unicorn
            .mem_map(base, PAGE_SIZE, Prot::READ)
            .map_err(|e| e.to_string())?;
        unicorn.mem_write(base, &table).map_err(|e| e.to_string())?;
        unicorn.get_data_mut().crt_pctype_buffer = Some(address);
        Ok(address)
    })();
    finish_guest_stdio(unicorn, result);
}
