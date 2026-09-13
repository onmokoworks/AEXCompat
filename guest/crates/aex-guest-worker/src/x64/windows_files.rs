// Session-owned Windows file handles for explicitly mounted read-only assets.
// Separate token space from CRT FILE pointers; issued handles are never reused.
const WINDOWS_ASSET_HANDLE_BASE: u64 = 0xc00000000;
struct WindowsAssetFile {
    name: String,
    bytes: Box<[u8]>,
    position: u64,
    readable: bool,
    inheritable: bool,
    share_read_access: bool,
    share: u32,
    null_device: bool,
}

impl GuestFiles {
    fn open_windows_asset(
        &mut self,
        name: &str,
        readable: bool,
        share_read_access: bool,
        share: u32,
    ) -> Result<Result<u64, u32>, String> {
        use std::io::Read;
        if share & !7 != 0 {
            return Ok(Err(87));
        }
        if self.directory_exists(name) {
            return Ok(Err(5));
        }
        let Some(source) = self.sources.get(name) else {
            return Ok(Err(2));
        };
        if self.windows_files.values().any(|file| {
            file.name == name
                && ((share_read_access && file.share & 1 == 0)
                    || (file.share_read_access && share & 1 == 0))
        }) || (share & 1 == 0
            && self
                .streams
                .values()
                .any(|file| file.name.as_deref() == Some(name) && file.readable))
            || (share_read_access
                && self.streams.values().any(|file| {
                    file.name.as_deref() == Some(name)
                        && file.readable
                        && !file.share_read_access
                }))
        {
            return Ok(Err(32));
        }
        if self.windows_files.len() + self.streams.len() >= 64 || self.next_windows_file >= 65536 {
            return Ok(Err(4));
        }
        let file = match std::fs::File::open(source) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Err(2)),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(Err(5)),
            Err(e) => return Err(format!("Windows asset open: {e}")),
        };
        if !file
            .metadata()
            .map_err(|e| format!("Windows asset metadata: {e}"))?
            .is_file()
        {
            return Ok(Err(5));
        }
        let capacity = MAX_GUEST_FILE_BYTES.min(
            MAX_GUEST_STREAM_BYTES
                .checked_sub(self.live_bytes)
                .ok_or("guest asset byte accounting overflow")?,
        );
        let mut bytes = Vec::new();
        file.take(capacity as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Windows asset read: {e}"))?;
        if bytes.len() > capacity {
            return Err("Windows asset byte capacity exceeded".into());
        }
        let sha = format!("{:x}", Sha256::digest(&bytes));
        let handle = WINDOWS_ASSET_HANDLE_BASE + self.next_windows_file * 8;
        self.next_windows_file += 1;
        self.live_bytes += bytes.len();
        self.windows_files.insert(
            handle,
            WindowsAssetFile {
                name: name.into(),
                bytes: bytes.into_boxed_slice(),
                position: 0,
                readable,
                inheritable: false,
                share_read_access,
                share,
                null_device: false,
            },
        );
        self.reports.push(TraceModule {
            name: name.into(),
            kind: "guest_asset",
            sha256: Some(sha),
            symbols: vec![],
        });
        Ok(Ok(handle))
    }

    fn open_windows_null(
        &mut self,
        readable: bool,
        share_read_access: bool,
        share: u32,
    ) -> Result<Result<u64, u32>, String> {
        if self.windows_files.len() + self.streams.len() >= 64 || self.next_windows_file >= 65536 {
            return Ok(Err(4));
        }
        let handle = WINDOWS_ASSET_HANDLE_BASE + self.next_windows_file * 8;
        self.next_windows_file += 1;
        self.windows_files.insert(
            handle,
            WindowsAssetFile {
                name: "nul".into(),
                bytes: Box::default(),
                position: 0,
                readable,
                inheritable: false,
                share_read_access,
                share,
                null_device: true,
            },
        );
        Ok(Ok(handle))
    }

    fn close_windows_asset(&mut self, handle: u64) -> Result<(), u32> {
        let Some(file) = self.windows_files.remove(&handle) else {
            return Err(6);
        };
        self.live_bytes -= file.bytes.len();
        Ok(())
    }
}

fn finish_windows_file_api(
    unicorn: &mut Unicorn<'_, GuestState>,
    result: Result<(u64, u32), String>,
) {
    if result.is_err() {
        let _ = unicorn.reg_write(RegisterX86::RAX, u64::MAX);
    }
    let result = result.map(|(value, error)| {
        if error != 0 {
            unicorn.get_data_mut().windows_last_error = error;
        }
        value
    });
    finish_guest_stdio(unicorn, result);
}

fn guest_create_file(
    unicorn: &mut Unicorn<'_, GuestState>,
    wide: bool,
) -> Result<(u64, u32), String> {
    let pointer = read_win64_import_argument(unicorn, 0)?;
    if pointer == 0 {
        return Ok((u64::MAX, 3));
    }
    let maximum = if wide { 32767 } else { 259 };
    let unit_size = if wide { 2 } else { 1 };
    let mut units = Vec::new();
    for i in 0..=maximum {
        let address = pointer
            .checked_add(i * unit_size)
            .ok_or("CreateFile path range overflows")?;
        if !guest_range_has_permission(unicorn, address, unit_size, Prot::READ)? {
            return Err("CreateFile path is not fully readable".into());
        }
        let mut bytes = [0; 2];
        unicorn
            .mem_read(address, &mut bytes[..unit_size as usize])
            .map_err(|e| format!("CreateFile path read: {e}"))?;
        let unit = u16::from_le_bytes(bytes);
        if unit == 0 {
            break;
        }
        if i == maximum {
            return Ok((u64::MAX, 206));
        }
        units.push(unit);
    }
    if units.is_empty() {
        return Ok((u64::MAX, 3));
    }
    if !wide && units.iter().any(|u| *u > 127) {
        return Err("CreateFileA non-ASCII codepage conversion is not implemented".into());
    }
    let path = match String::from_utf16(&units) {
        Ok(path) => path,
        Err(_) => return Ok((u64::MAX, 87)),
    };
    let access = read_win64_import_argument(unicorn, 1)? as u32;
    let share = read_win64_import_argument(unicorn, 2)? as u32;
    let security = read_win64_import_argument(unicorn, 3)?;
    let disposition = read_win64_import_argument(unicorn, 4)? as u32;
    let flags = read_win64_import_argument(unicorn, 5)? as u32;
    let _template = read_win64_import_argument(unicorn, 6)?;
    if share & !7 != 0 || !(1..=5).contains(&disposition) {
        return Ok((u64::MAX, 87));
    }
    let name = path.replace('\\', "/").to_ascii_lowercase();
    if name.starts_with("//")
        || name.starts_with("/?/")
        || name.starts_with("/./")
        || name.split('/').any(|p| p == "..")
        || access & 0x500d0116 != 0
        || flags & 0x04000000 != 0
    {
        return Ok((u64::MAX, 5));
    }
    if access & !0xa01200a9 != 0 {
        return Err("CreateFile access mask is not implemented".into());
    }
    let null_device = path.eq_ignore_ascii_case("nul") || path.eq_ignore_ascii_case("nul:");
    let name = if null_device {
        "nul".to_string()
    } else {
        let canonical = canonical_guest_fullpath(path.as_bytes())
            .map_err(|error| format!("CreateFile path resolution for {path:?}: {error}"))?;
        std::str::from_utf8(&canonical[..canonical.len() - 1])
            .unwrap()
            .replace('\\', "/")
            .to_ascii_lowercase()
    };
    let exists = null_device || unicorn.get_data().guest_files.sources.contains_key(&name);
    if disposition == 1 && (exists || unicorn.get_data().guest_files.directory_exists(&name)) {
        return Ok((u64::MAX, 80));
    }
    if !matches!(disposition, 3 | 4) || (!exists && disposition == 4) {
        return Ok((u64::MAX, 5));
    }
    if !exists {
        if unicorn.get_data().guest_files.directory_exists(&name) && flags & 0x02000000 != 0 {
            return Err("CreateFile directory handles are not implemented".into());
        }
        return Ok((
            u64::MAX,
            if unicorn.get_data().guest_files.directory_exists(&name) {
                5
            } else {
                2
            },
        ));
    }
    // Only caching hints and file attributes are accepted for a regular,
    // synchronous, read-only asset. No raw-device, asynchronous or reparse I/O.
    // BACKUP_SEMANTICS and OPEN_REPARSE_POINT do not change an immutable,
    // regular mounted file because the virtual tree contains no reparse nodes.
    if flags & !(0x98000000 | 0x02200000 | 0xffff) != 0 {
        return Err(format!(
            "CreateFile flags {flags:#x} are not implemented for mounted assets"
        ));
    }
    let mut inheritable = false;
    if security != 0 {
        if !guest_range_has_permission(unicorn, security, 24, Prot::READ)? {
            return Err("CreateFile SECURITY_ATTRIBUTES is not readable".into());
        }
        let bytes = unicorn
            .mem_read_as_vec(security, 24)
            .map_err(|e| format!("CreateFile security attributes: {e}"))?;
        if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != 24 {
            return Ok((u64::MAX, 87));
        }
        // Existing-file descriptor is ignored; the inheritance bit still applies.
        inheritable = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) != 0;
    }
    let readable = access & 0x80000001 != 0;
    let share_read_access = access & 0xa0000021 != 0;
    let opened = if null_device {
        unicorn
            .get_data_mut()
            .guest_files
            .open_windows_null(readable, share_read_access, share)?
    } else {
        unicorn.get_data_mut().guest_files.open_windows_asset(
            &name,
            readable,
            share_read_access,
            share,
        )?
    };
    match opened {
        Ok(handle) => {
            unicorn
                .get_data_mut()
                .guest_files
                .windows_files
                .get_mut(&handle)
                .unwrap()
                .inheritable = inheritable;
            Ok((handle, if disposition == 4 { 183 } else { 0 }))
        }
        Err(error) => Ok((u64::MAX, error)),
    }
}

fn guest_windows_file_io(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) -> Result<(u64, u32), String> {
    let handle = read_win64_import_argument(unicorn, 0)?;
    let output = read_win64_import_argument(unicorn, 1)?;
    if operation == LegacyWin64Import::ReadFile {
        let requested = read_win64_import_argument(unicorn, 2)? as u32;
        let count_pointer = read_win64_import_argument(unicorn, 3)?;
        let overlapped = read_win64_import_argument(unicorn, 4)?;
        if overlapped != 0 {
            return Err("overlapped ReadFile is not implemented".into());
        }
        if count_pointer == 0 {
            return Ok((0, 87));
        }
        if !guest_range_has_permission(unicorn, count_pointer, 4, Prot::WRITE)? {
            return Err("ReadFile count is not writable".into());
        }
        let file = unicorn.get_data().guest_files.windows_files.get(&handle);
        let error = match file {
            None => 6,
            Some(file) if !file.readable => 5,
            _ => 0,
        };
        if error != 0 {
            unicorn
                .mem_write(count_pointer, &0u32.to_le_bytes())
                .map_err(|e| format!("ReadFile count: {e}"))?;
            return Ok((0, error));
        }
        let file = file.unwrap();
        let start = file.position.min(file.bytes.len() as u64) as usize;
        let count = (requested as usize).min(file.bytes.len() - start);
        let position = file.position;
        if count != 0 {
            let end = output
                .checked_add(count as u64)
                .ok_or("ReadFile range overflow")?;
            let count_end = count_pointer
                .checked_add(4)
                .ok_or("ReadFile count range overflow")?;
            if output < count_end && count_pointer < end {
                return Err("ReadFile outputs overlap".into());
            }
            if output == 0
                || !guest_range_has_permission(unicorn, output, count as u64, Prot::WRITE)?
            {
                return Err("ReadFile output is not writable".into());
            }
            let bytes = file.bytes[start..start + count].to_vec();
            unicorn
                .mem_write(output, &bytes)
                .map_err(|e| format!("ReadFile output: {e}"))?;
        }
        unicorn
            .mem_write(count_pointer, &(count as u32).to_le_bytes())
            .map_err(|e| format!("ReadFile count: {e}"))?;
        unicorn
            .get_data_mut()
            .guest_files
            .windows_files
            .get_mut(&handle)
            .unwrap()
            .position = position + count as u64;
        return Ok((1, 0));
    }
    let Some(file) = unicorn.get_data().guest_files.windows_files.get(&handle) else {
        return Ok((0, 6));
    };
    if operation == LegacyWin64Import::GetFileInformationByHandle {
        if output == 0 || !guest_range_has_permission(unicorn, output, 52, Prot::WRITE)? {
            return Err("GetFileInformationByHandle output is not writable".into());
        }
        let size = file.bytes.len() as u64;
        let mut bytes = [0u8; 52];
        bytes[0..4].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&((size >> 32) as u32).to_le_bytes());
        bytes[36..40].copy_from_slice(&(size as u32).to_le_bytes());
        bytes[40..44].copy_from_slice(&1u32.to_le_bytes());
        bytes[48..52].copy_from_slice(&(handle as u32).to_le_bytes());
        unicorn
            .mem_write(output, &bytes)
            .map_err(|e| format!("GetFileInformationByHandle output: {e}"))?;
    } else if operation == LegacyWin64Import::GetFileSizeEx {
        let size = file.bytes.len() as u64;
        if output == 0 || !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
            return Err("GetFileSizeEx output is not writable".into());
        }
        unicorn
            .mem_write(output, &size.to_le_bytes())
            .map_err(|e| format!("GetFileSizeEx output: {e}"))?;
    } else {
        if !file.readable {
            return Ok((0, 5));
        }
        let position_pointer = read_win64_import_argument(unicorn, 2)?;
        let origin = read_win64_import_argument(unicorn, 3)? as u32;
        let base = match origin {
            0 => 0,
            1 => file.position,
            2 => file.bytes.len() as u64,
            _ => return Ok((0, 87)),
        };
        if origin == 0 && output > i64::MAX as u64 {
            return Err(
                "SetFilePointerEx unsigned offsets above INT64_MAX are not implemented".into(),
            );
        }
        let next = base as i128 + (output as i64) as i128;
        if next < 0 {
            return Ok((0, 131));
        }
        if next > i64::MAX as i128 {
            return Ok((0, 87));
        }
        if position_pointer != 0 {
            if !guest_range_has_permission(unicorn, position_pointer, 8, Prot::WRITE)? {
                return Err("SetFilePointerEx output is not writable".into());
            }
            unicorn
                .mem_write(position_pointer, &(next as u64).to_le_bytes())
                .map_err(|e| format!("SetFilePointerEx output: {e}"))?;
        }
        unicorn
            .get_data_mut()
            .guest_files
            .windows_files
            .get_mut(&handle)
            .unwrap()
            .position = next as u64;
    }
    Ok((1, 0))
}
