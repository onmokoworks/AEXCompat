// Bounded integer/narrow-string formatting using Win64 va_list slots. Windows
// long remains 32 bits even though the native macOS ABI uses 64-bit long.
fn format_guest_stdio(
    unicorn: &Unicorn<'_, GuestState>,
    format: &[u8],
    va_list: u64,
) -> Result<Vec<u8>, String> {
    let mut argument = 0usize;
    let mut next = || -> Result<u64, String> {
        if argument >= MAX_CRT_STDIO_ARGUMENTS {
            return Err("stdio conversion count exceeds argument bound".into());
        }
        let address = va_list
            .checked_add(argument as u64 * 8)
            .ok_or("stdio va_list address overflow")?;
        if !guest_range_has_permission(unicorn, address, 8, Prot::READ)? {
            return Err("stdio va_list slot is not readable".into());
        }
        let mut bytes = [0; 8];
        unicorn
            .mem_read(address, &mut bytes)
            .map_err(|e| format!("stdio va_list read: {e}"))?;
        argument += 1;
        Ok(u64::from_le_bytes(bytes))
    };
    format_guest_values(unicorn, format, &mut next, false)
}

fn format_guest_values(
    unicorn: &Unicorn<'_, GuestState>,
    format: &[u8],
    next: &mut impl FnMut() -> Result<u64, String>,
    winuser: bool,
) -> Result<Vec<u8>, String> {
    let limit = if winuser {
        1023
    } else {
        MAX_CRT_STDIO_BUFFER_BYTES as usize
    };
    fn number(format: &[u8], at: &mut usize, limit: usize) -> Result<usize, String> {
        let mut value = 0usize;
        while let Some(digit @ b'0'..=b'9') = format.get(*at) {
            value = value
                .checked_mul(10)
                .and_then(|v| v.checked_add((digit - b'0') as usize))
                .ok_or("stdio width/precision overflow")?;
            if value > limit {
                return Err("stdio width/precision exceeds output bound".into());
            }
            *at += 1;
        }
        Ok(value)
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < format.len() {
        if format[at] != b'%' {
            out.push(format[at]);
            at += 1;
        } else {
            at += 1;
            if format.get(at) == Some(&b'%') {
                out.push(b'%');
                at += 1;
            } else {
                let (mut left, mut zero, mut plus, mut blank, mut alternate) =
                    (false, false, false, false, false);
                loop {
                    match format.get(at) {
                        Some(b'-') => left = true,
                        Some(b'0') => zero = true,
                        Some(b'+') => plus = true,
                        Some(b' ') => blank = true,
                        Some(b'#') => alternate = true,
                        _ => break,
                    }
                    at += 1;
                }
                if winuser && (plus || blank || format.get(at) == Some(&b'*')) {
                    return Err("wsprintfA unsupported flag or dynamic width".into());
                }
                let width = if format.get(at) == Some(&b'*') {
                    at += 1;
                    let value = next()? as u32 as i32;
                    if value < 0 {
                        left = true;
                    }
                    value.unsigned_abs() as usize
                } else {
                    number(format, &mut at, limit)?
                };
                let mut precision = if format.get(at) == Some(&b'.') {
                    at += 1;
                    if winuser && format.get(at) == Some(&b'*') {
                        return Err("wsprintfA dynamic precision is not supported".into());
                    }
                    if format.get(at) == Some(&b'*') {
                        at += 1;
                        let value = next()? as u32 as i32;
                        if value < 0 {
                            None
                        } else {
                            Some(value as usize)
                        }
                    } else {
                        Some(number(format, &mut at, limit)?)
                    }
                } else {
                    None
                };
                if width > limit || precision.is_some_and(|v| v > limit) {
                    return Err("stdio width/precision exceeds output bound".into());
                }
                let mut length = "";
                for candidate in ["I64", "I32", "hh", "ll", "h", "l", "I", "z", "t", "j"] {
                    if format[at..].starts_with(candidate.as_bytes()) {
                        length = candidate;
                        at += candidate.len();
                        break;
                    }
                }
                let mut conversion = *format
                    .get(at)
                    .ok_or("stdio format ends before conversion")?;
                at += 1;
                if winuser {
                    if length == "h" && matches!(conversion, b'S' | b'C') {
                        conversion = conversion.to_ascii_lowercase();
                    }
                    let supported = match conversion {
                        b'd' | b'i' | b'u' => matches!(length, "" | "h" | "l"),
                        b'x' | b'X' => matches!(length, "" | "l" | "I"),
                        b's' | b'c' => matches!(length, "" | "h"),
                        _ => false,
                    };
                    if !supported {
                        return Err(format!(
                            "wsprintfA unsupported conversion %{length}{}",
                            char::from(conversion)
                        ));
                    }
                    if matches!(conversion, b'd' | b'i' | b'u' | b'x' | b'X')
                        && precision == Some(0)
                    {
                        precision = Some(1);
                    }
                    if conversion == b's' && precision == Some(0) {
                        precision = None;
                    }
                    if zero && precision.is_some() {
                        return Err(
                            "wsprintfA combined zero padding and precision is not implemented"
                                .into(),
                        );
                    }
                }
                let mut prefix = Vec::new();
                let mut content;
                let numeric = matches!(conversion, b'd' | b'i' | b'u' | b'o' | b'x' | b'X');
                if numeric {
                    let value = next()?;
                    let bits = match length {
                        "hh" => 8,
                        "h" => 16,
                        "" | "l" | "I32" => 32,
                        _ => 64,
                    };
                    let unsigned = if bits == 64 {
                        value
                    } else {
                        value & ((1u64 << bits) - 1)
                    };
                    let signed = match bits {
                        8 => unsigned as i8 as i64,
                        16 => unsigned as i16 as i64,
                        32 => unsigned as i32 as i64,
                        _ => unsigned as i64,
                    };
                    let is_signed = matches!(conversion, b'd' | b'i');
                    let magnitude = if is_signed {
                        signed.unsigned_abs()
                    } else {
                        unsigned
                    };
                    if is_signed {
                        if signed < 0 {
                            prefix.push(b'-');
                        } else if plus {
                            prefix.push(b'+');
                        } else if blank {
                            prefix.push(b' ');
                        }
                    }
                    content = match conversion {
                        b'x' => format!("{magnitude:x}"),
                        b'X' => format!("{magnitude:X}"),
                        b'o' => format!("{magnitude:o}"),
                        _ => magnitude.to_string(),
                    }
                    .into_bytes();
                    if precision == Some(0) && magnitude == 0 {
                        content.clear();
                    }
                    let measured = content.len() + if winuser { prefix.len() } else { 0 };
                    let zeros = precision.unwrap_or(0).saturating_sub(measured);
                    if zeros > 0 {
                        let mut padded = vec![b'0'; zeros];
                        padded.extend(content);
                        content = padded;
                    }
                    if alternate {
                        if conversion == b'o' && content.first() != Some(&b'0') {
                            prefix.push(b'0');
                        }
                        if (winuser || magnitude != 0) && matches!(conversion, b'x' | b'X') {
                            prefix.extend(if conversion == b'x' { b"0x" } else { b"0X" });
                        }
                    }
                } else if matches!(conversion, b's' | b'c') && matches!(length, "" | "h") {
                    let value = next()?;
                    if conversion == b'c' {
                        if winuser && value as u8 == 0 {
                            content = Vec::new();
                        } else {
                            content = vec![value as u8];
                        }
                    } else if let Some(maximum) = precision {
                        if value == 0 {
                            return Err("stdio null string argument is unsupported".into());
                        }
                        content = Vec::new();
                        let regions = unicorn
                            .mem_regions()
                            .map_err(|e| format!("stdio memory map: {e}"))?;
                        let mut readable_end = None;
                        for i in 0..maximum {
                            let address = value
                                .checked_add(i as u64)
                                .ok_or("stdio string pointer overflow")?;
                            if readable_end.is_none_or(|end| address > end) {
                                readable_end = regions
                                    .iter()
                                    .find(|region| {
                                        region.begin <= address
                                            && address <= region.end
                                            && region.perms & Prot::READ.0 as u32 != 0
                                    })
                                    .map(|region| region.end);
                                if readable_end.is_none() {
                                    return Err("stdio string argument is not readable".into());
                                }
                            }
                            let mut byte = [0];
                            unicorn
                                .mem_read(address, &mut byte)
                                .map_err(|e| format!("stdio string read: {e}"))?;
                            if byte[0] == 0 {
                                break;
                            }
                            content.push(byte[0]);
                        }
                    } else {
                        content = read_crt_stdio_c_string(
                            unicorn,
                            value,
                            limit as u64 + u64::from(winuser),
                            "string argument",
                        )?;
                    }
                } else {
                    return Err(format!(
                        "stdio unsupported conversion '%{}{}'",
                        length,
                        char::from(conversion)
                    ));
                }
                let size = prefix.len() + content.len();
                let padding = width.saturating_sub(size);
                if out
                    .len()
                    .checked_add(size.max(width))
                    .is_none_or(|n| n > limit)
                {
                    return Err("stdio formatted output exceeds output bound".into());
                }
                if !left && !(zero && numeric && precision.is_none()) {
                    out.resize(out.len() + padding, b' ');
                }
                out.extend(prefix);
                if !left && zero && numeric && precision.is_none() {
                    out.resize(out.len() + padding, b'0');
                }
                out.extend(content);
                if left {
                    out.resize(out.len() + padding, b' ');
                }
            }
        }
        if out.len() > limit {
            return Err("stdio formatted output exceeds output bound".into());
        }
    }
    Ok(out)
}

fn guest_wsprintf_a(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let destination = read_win64_import_argument(unicorn, 0)?;
    let format_address = read_win64_import_argument(unicorn, 1)?;
    let format = read_crt_stdio_c_string(
        unicorn,
        format_address,
        MAX_CRT_STDIO_FORMAT_BYTES,
        "wsprintfA format",
    )?;
    let mut argument = 2usize;
    let mut next = || {
        if argument - 2 >= MAX_CRT_STDIO_ARGUMENTS {
            return Err("wsprintfA argument count exceeds bound".into());
        }
        let value = read_win64_import_argument(unicorn, argument)?;
        argument += 1;
        Ok(value)
    };
    let mut output = format_guest_values(unicorn, &format, &mut next, true)?;
    let count = output.len() as u64;
    output.push(0);
    if destination == 0
        || !guest_range_has_permission(unicorn, destination, output.len() as u64, Prot::WRITE)?
    {
        return Err("wsprintfA output is not writable".into());
    }
    unicorn
        .mem_write(destination, &output)
        .map_err(|error| format!("wsprintfA output: {error}"))?;
    Ok(count)
}
