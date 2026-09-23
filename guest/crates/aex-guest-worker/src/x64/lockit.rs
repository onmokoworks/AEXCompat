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

fn emulate_msvcp_codecvt(unicorn: &mut Unicorn<'_, GuestState>, wide_to_narrow: bool) {
    let result = (|| -> Result<u64, String> {
        let from = read_win64_import_argument(unicorn, 2)?;
        let from_end = read_win64_import_argument(unicorn, 3)?;
        let from_next = read_win64_import_argument(unicorn, 4)?;
        let to = read_win64_import_argument(unicorn, 5)?;
        let to_end = read_win64_import_argument(unicorn, 6)?;
        let to_next = read_win64_import_argument(unicorn, 7)?;
        let input_width = if wide_to_narrow { 2 } else { 1 };
        let output_width = if wide_to_narrow { 1 } else { 2 };
        if from_end < from || to_end < to || (from_end - from) % input_width != 0 {
            return Err("MSVCP codecvt received reversed or misaligned ranges".into());
        }
        let input_count = (from_end - from) / input_width;
        let output_count = (to_end - to) / output_width;
        if input_count > MAX_CRT_STRING_BYTES || output_count > MAX_CRT_STRING_BYTES {
            return Err("MSVCP codecvt range exceeds supported bound".into());
        }
        if !guest_range_has_permission(unicorn, from, input_count * input_width, Prot::READ)?
            || !guest_range_has_permission(unicorn, to, output_count * output_width, Prot::WRITE)?
            || !guest_range_has_permission(unicorn, from_next, 8, Prot::WRITE)?
            || !guest_range_has_permission(unicorn, to_next, 8, Prot::WRITE)?
        {
            return Err("MSVCP codecvt range or result pointer is inaccessible".into());
        }
        let converted = input_count.min(output_count);
        let mut output = Vec::with_capacity((converted * output_width) as usize);
        for index in 0..converted {
            let source = from + index * input_width;
            let value = if wide_to_narrow {
                let bytes = unicorn
                    .mem_read_as_vec(source, 2)
                    .map_err(|e| e.to_string())?;
                u16::from_le_bytes([bytes[0], bytes[1]])
            } else {
                unicorn
                    .mem_read_as_vec(source, 1)
                    .map_err(|e| e.to_string())?[0] as u16
            };
            if wide_to_narrow && value > 0x7f {
                return Ok(2); // codecvt_base::error in the C locale
            }
            if wide_to_narrow {
                output.push(value as u8);
            } else {
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
        unicorn.mem_write(to, &output).map_err(|e| e.to_string())?;
        unicorn
            .mem_write(from_next, &(from + converted * input_width).to_le_bytes())
            .map_err(|e| e.to_string())?;
        unicorn
            .mem_write(to_next, &(to + converted * output_width).to_le_bytes())
            .map_err(|e| e.to_string())?;
        Ok(u64::from(converted < input_count)) // ok=0, partial=1
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_global_memory_status_ex(unicorn: &mut Unicorn<'_, GuestState>) {
    const STRUCT_BYTES: u64 = 64;
    const GIB: u64 = 1024 * 1024 * 1024;
    let result = (|| -> Result<u64, String> {
        let output = read_win64_import_argument(unicorn, 0)?;
        if output == 0 || !guest_range_has_permission(unicorn, output, STRUCT_BYTES, Prot::WRITE)? {
            unicorn.get_data_mut().windows_last_error = 87;
            return Ok(0);
        }
        let mut length = [0; 4];
        unicorn
            .mem_read(output, &mut length)
            .map_err(|e| e.to_string())?;
        if u32::from_le_bytes(length) != STRUCT_BYTES as u32 {
            unicorn.get_data_mut().windows_last_error = 87;
            return Ok(0);
        }
        let mut bytes = [0u8; STRUCT_BYTES as usize];
        bytes[0..4].copy_from_slice(&(STRUCT_BYTES as u32).to_le_bytes());
        bytes[4..8].copy_from_slice(&50u32.to_le_bytes());
        for (offset, value) in [
            (8, 8 * GIB),
            (16, 4 * GIB),
            (24, 16 * GIB),
            (32, 8 * GIB),
            (40, 128 * 1024 * GIB),
            (48, 127 * 1024 * GIB),
            (56, 0),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        unicorn
            .mem_write(output, &bytes)
            .map_err(|e| e.to_string())?;
        Ok(1)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_init_once(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    const CHECK_ONLY: u32 = 1;
    const ASYNC: u32 = 2;
    const INIT_FAILED: u32 = 4;
    let result = (|| -> Result<u64, String> {
        let object = read_win64_import_argument(unicorn, 0)?;
        if object == 0 || !guest_range_has_permission(unicorn, object, 8, Prot::WRITE)? {
            unicorn.get_data_mut().windows_last_error = 87;
            return Ok(0);
        }
        if operation == LegacyWin64Import::InitOnceInitialize {
            unicorn
                .mem_write(object, &[0; 8])
                .map_err(|e| e.to_string())?;
            unicorn.get_data_mut().windows_init_once.remove(&object);
            return Ok(0);
        }
        let flags = read_win64_import_argument(unicorn, 1)? as u32;
        if operation == LegacyWin64Import::InitOnceComplete {
            if flags & !(ASYNC | INIT_FAILED) != 0 || flags & ASYNC != 0 && flags & INIT_FAILED != 0
            {
                unicorn.get_data_mut().windows_last_error = 87;
                return Ok(0);
            }
            let context = read_win64_import_argument(unicorn, 2)?;
            let Some(state) = unicorn.get_data().windows_init_once.get(&object).copied() else {
                unicorn.get_data_mut().windows_last_error = 87;
                return Ok(0);
            };
            if state.complete || state.owner != Some(unicorn.get_data().current_windows_thread_id) {
                unicorn.get_data_mut().windows_last_error = 87;
                return Ok(0);
            }
            if flags & INIT_FAILED != 0 {
                unicorn.get_data_mut().windows_init_once.remove(&object);
                unicorn
                    .mem_write(object, &[0; 8])
                    .map_err(|e| e.to_string())?;
            } else {
                if context & 3 != 0 {
                    unicorn.get_data_mut().windows_last_error = 87;
                    return Ok(0);
                }
                unicorn.get_data_mut().windows_init_once.insert(
                    object,
                    WindowsInitOnceState {
                        owner: None,
                        context,
                        complete: true,
                    },
                );
                unicorn
                    .mem_write(object, &(context | 1).to_le_bytes())
                    .map_err(|e| e.to_string())?;
            }
            return Ok(1);
        }
        if flags & !(CHECK_ONLY | ASYNC) != 0 || flags & CHECK_ONLY != 0 && flags & ASYNC != 0 {
            unicorn.get_data_mut().windows_last_error = 87;
            return Ok(0);
        }
        let pending_out = read_win64_import_argument(unicorn, 2)?;
        let context_out = read_win64_import_argument(unicorn, 3)?;
        if pending_out == 0
            || !guest_range_has_permission(unicorn, pending_out, 4, Prot::WRITE)?
            || context_out != 0
                && !guest_range_has_permission(unicorn, context_out, 8, Prot::WRITE)?
        {
            unicorn.get_data_mut().windows_last_error = 87;
            return Ok(0);
        }
        let state = unicorn.get_data().windows_init_once.get(&object).copied();
        let (pending, context) = match state {
            Some(state) if state.complete => (0u32, state.context),
            Some(state) if state.owner == Some(unicorn.get_data().current_windows_thread_id) => {
                return Err("recursive InitOnce initialization is not supported".into());
            }
            Some(_) => return Err("contended InitOnce requires guest scheduling".into()),
            None => {
                if flags & CHECK_ONLY == 0 {
                    let owner = unicorn.get_data().current_windows_thread_id;
                    unicorn.get_data_mut().windows_init_once.insert(
                        object,
                        WindowsInitOnceState {
                            owner: Some(owner),
                            context: 0,
                            complete: false,
                        },
                    );
                    unicorn
                        .mem_write(object, &2u64.to_le_bytes())
                        .map_err(|e| e.to_string())?;
                }
                (1, 0)
            }
        };
        unicorn
            .mem_write(pending_out, &pending.to_le_bytes())
            .map_err(|e| e.to_string())?;
        if context_out != 0 {
            unicorn
                .mem_write(context_out, &context.to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
        Ok(1)
    })();
    finish_guest_stdio(unicorn, result);
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

// ___lc_locale_name_func returns wchar_t*[6], indexed by LC_ALL..LC_TIME.
// UCRT nlsdata.cpp initializes all six Windows locale names to NULL for C;
// these differ from the printable "C" strings returned by setlocale.
fn emulate_crt_locale_names(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = (|| -> Result<u64, String> {
        if let Some(address) = unicorn.get_data().crt_locale_names_buffer {
            return Ok(address);
        }
        let address = GUEST_STREAM_BUFFER_BASE + MAX_GUEST_STREAM_OPENS * PAGE_SIZE + 2 * PAGE_SIZE;
        unicorn
            .mem_map(address, PAGE_SIZE, Prot::READ)
            .map_err(|e| e.to_string())?;
        unicorn
            .mem_write(address, &[0; 6 * 8])
            .map_err(|e| e.to_string())?;
        unicorn.get_data_mut().crt_locale_names_buffer = Some(address);
        Ok(address)
    })();
    finish_guest_stdio(unicorn, result);
}

// Windows x64 `struct lconv` has ten pointer fields followed by eight signed
// char fields. The deterministic C locale uses "." for decimal_point, empty
// strings for every other string field, and CHAR_MAX for monetary metadata.
fn crt_lconv_address(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    if let Some(address) = unicorn.get_data().crt_lconv_buffer {
        return Ok(address);
    }
    // Keep this page immediately before the disjoint errno thread arena.
    // The lower stream namespace after `+3 pages` belongs to popup text.
    let address = CRT_ERRNO_BASE - PAGE_SIZE;
    let decimal = address + 88;
    let empty = decimal + 2;
    let mut bytes = [0u8; 96];
    bytes[0..8].copy_from_slice(&decimal.to_le_bytes());
    for offset in (8..80).step_by(8) {
        bytes[offset..offset + 8].copy_from_slice(&empty.to_le_bytes());
    }
    bytes[80..88].fill(i8::MAX as u8);
    bytes[88..92].copy_from_slice(b".\0\0\0");
    unicorn
        .mem_map(address, PAGE_SIZE, Prot::READ)
        .map_err(|e| e.to_string())?;
    unicorn
        .mem_write(address, &bytes)
        .map_err(|e| e.to_string())?;
    unicorn.get_data_mut().crt_lconv_buffer = Some(address);
    Ok(address)
}

fn emulate_crt_localeconv(unicorn: &mut Unicorn<'_, GuestState>) {
    let result = crt_lconv_address(unicorn);
    finish_guest_stdio(unicorn, result);
}

// UCRT allocates these colon-delimited C-locale tables for the caller, which
// releases them through the ordinary CRT heap.
fn emulate_crt_time_names(unicorn: &mut Unicorn<'_, GuestState>, months: bool) {
    const DAYS: &[u8] =
        b":Sun:Sunday:Mon:Monday:Tue:Tuesday:Wed:Wednesday:Thu:Thursday:Fri:Friday:Sat:Saturday\0";
    const MONTHS: &[u8] = b":Jan:January:Feb:February:Mar:March:Apr:April:May:May:Jun:June:Jul:July:Aug:August:Sep:September:Oct:October:Nov:November:Dec:December\0";
    let result = (|| -> Result<u64, String> {
        let bytes = if months { MONTHS } else { DAYS };
        let pointer =
            allocate_crt_region(unicorn, bytes.len() as u64).map_err(|error| error.to_string())?;
        unicorn.mem_write(pointer, bytes).map_err(|error| {
            let _ = free_crt_region(unicorn, pointer);
            error.to_string()
        })?;
        Ok(pointer)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_crt_wide_time_names(unicorn: &mut Unicorn<'_, GuestState>, months: bool) {
    const DAYS: &str =
        ":Sun:Sunday:Mon:Monday:Tue:Tuesday:Wed:Wednesday:Thu:Thursday:Fri:Friday:Sat:Saturday";
    const MONTHS: &str = ":Jan:January:Feb:February:Mar:March:Apr:April:May:May:Jun:June:Jul:July:Aug:August:Sep:September:Oct:October:Nov:November:Dec:December";
    let result = (|| -> Result<u64, String> {
        let value = if months { MONTHS } else { DAYS };
        let mut bytes = Vec::with_capacity((value.len() + 1) * 2);
        for unit in value.encode_utf16().chain([0]) {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let pointer =
            allocate_crt_region(unicorn, bytes.len() as u64).map_err(|error| error.to_string())?;
        unicorn.mem_write(pointer, &bytes).map_err(|error| {
            let _ = free_crt_region(unicorn, pointer);
            error.to_string()
        })?;
        Ok(pointer)
    })();
    finish_guest_stdio(unicorn, result);
}

fn emulate_crt_time_locale_names(unicorn: &mut Unicorn<'_, GuestState>) {
    const NAMES: [&str; 43] = [
        "Sun",
        "Mon",
        "Tue",
        "Wed",
        "Thu",
        "Fri",
        "Sat",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Jan",
        "Feb",
        "Mar",
        "Apr",
        "May",
        "Jun",
        "Jul",
        "Aug",
        "Sep",
        "Oct",
        "Nov",
        "Dec",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
        "AM",
        "PM",
        "MM/dd/yy",
        "dddd, MMMM dd, yyyy",
        "HH:mm:ss",
    ];
    const POINTER_COUNT: usize = 43;
    const HEADER_BYTES: usize = POINTER_COUNT * 8 + 8 + POINTER_COUNT * 8 + 8;
    let result = (|| -> Result<u64, String> {
        let narrow_bytes: usize = NAMES.iter().map(|name| name.len() + 1).sum();
        let wide_start = (HEADER_BYTES + narrow_bytes + 1) & !1;
        let wide_bytes: usize = NAMES.iter().map(|name| (name.len() + 1) * 2).sum();
        let total = wide_start
            .checked_add(wide_bytes)
            .ok_or("_Gettnames size overflow")?;
        let pointer =
            allocate_crt_region(unicorn, total as u64).map_err(|error| error.to_string())?;
        let mut bytes = vec![0u8; total];
        let mut narrow = HEADER_BYTES;
        let mut wide = wide_start;
        for (index, name) in NAMES.iter().enumerate() {
            bytes[index * 8..index * 8 + 8]
                .copy_from_slice(&(pointer + narrow as u64).to_le_bytes());
            bytes[352 + index * 8..360 + index * 8]
                .copy_from_slice(&(pointer + wide as u64).to_le_bytes());
            bytes[narrow..narrow + name.len()].copy_from_slice(name.as_bytes());
            narrow += name.len() + 1;
            for byte in name.bytes() {
                bytes[wide..wide + 2].copy_from_slice(&u16::from(byte).to_le_bytes());
                wide += 2;
            }
            wide += 2;
        }
        // Fields between the pointer arrays are `unk` and `refcount`.
        bytes[348..352].copy_from_slice(&1i32.to_le_bytes());
        unicorn.mem_write(pointer, &bytes).map_err(|error| {
            let _ = free_crt_region(unicorn, pointer);
            error.to_string()
        })?;
        Ok(pointer)
    })();
    finish_guest_stdio(unicorn, result);
}

// UCRT exposes both values as pointers to process-global integers. In the C
// locale their code pages are zero; returning the integer itself breaks callers
// that immediately dereference the ABI result.
fn supported_crt_locale(unicorn: &Unicorn<'_, GuestState>, locale: u64) -> bool {
    locale == 0 || unicorn.get_data().crt_locales.contains(&locale)
}

fn emulate_crt_locale_object(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        if operation == LegacyWin64Import::CrtFreeLocale {
            let locale = read_win64_import_argument(unicorn, 0)?;
            if locale == 0 || unicorn.get_data_mut().crt_locales.remove(&locale) {
                return Ok(0);
            }
            return Err("_free_locale rejected unknown locale object".into());
        }
        let category = read_win64_import_argument(unicorn, 0)? as u32 as i32;
        let name = read_win64_import_argument(unicorn, 1)?;
        if !(0..=5).contains(&category) || name == 0 {
            set_guest_crt_errno(unicorn, 22)?;
            return Ok(0);
        }
        let name = read_crt_stdio_c_string(unicorn, name, 128, "_create_locale name")?;
        if name != b"C" {
            set_guest_crt_errno(unicorn, 22)?;
            return Ok(0);
        }
        if unicorn.get_data().crt_locales.len() >= 64 {
            set_guest_crt_errno(unicorn, 12)?;
            return Ok(0);
        }
        let index = unicorn.get_data().next_crt_locale;
        let handle = CRT_LOCALE_HANDLE_BASE
            .checked_add(index.checked_mul(16).ok_or("CRT locale token overflow")?)
            .ok_or("CRT locale token overflow")?;
        unicorn.get_data_mut().next_crt_locale = index + 1;
        unicorn.get_data_mut().crt_locales.insert(handle);
        Ok(handle)
    })();
    finish_guest_stdio(unicorn, result);
}
