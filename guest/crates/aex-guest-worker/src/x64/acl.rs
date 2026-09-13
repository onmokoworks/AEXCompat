const WINDOWS_ACL_BASE: u64 = 0x0000_0008_0000_0000;

fn acl_read(unicorn: &Unicorn<'_, GuestState>, address: u64, size: u64) -> Result<Vec<u8>, String> {
    if address == 0 || !guest_range_has_permission(unicorn, address, size, Prot::READ)? {
        return Err("ACL input is not readable".into());
    }
    unicorn
        .mem_read_as_vec(address, size as usize)
        .map_err(|e| format!("ACL input read: {e}"))
}

fn build_new_guest_acl(
    unicorn: &Unicorn<'_, GuestState>,
    entries: u64,
    count: u32,
) -> Result<Vec<u8>, String> {
    if count > 1024 {
        return Err("ACL explicit entry count exceeds 1024".into());
    }
    let input = acl_read(unicorn, entries, u64::from(count) * 48)?;
    let mut denied = Vec::new();
    let mut allowed = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in input.chunks_exact(48) {
        let dword = |at| u32::from_le_bytes(entry[at..at + 4].try_into().unwrap());
        let pointer = |at| u64::from_le_bytes(entry[at..at + 8].try_into().unwrap());
        let mode = dword(4);
        let flags = dword(8);
        if pointer(16) != 0 || dword(24) != 0 || dword(28) != 0 || dword(32) > 8 {
            return Err("ACL requires a single SID trustee".into());
        }
        if !matches!(mode, 1..=3) || flags & !0x1f != 0 {
            return Err(format!(
                "unsupported ACL access mode {mode} or inheritance {flags:#x}"
            ));
        }
        let sid_pointer = pointer(40);
        let header = acl_read(unicorn, sid_pointer, 8)?;
        if header[0] != 1 || header[1] > 15 {
            return Err("invalid ACL trustee SID".into());
        }
        let sid = acl_read(unicorn, sid_pointer, 8 + u64::from(header[1]) * 4)?;
        if !seen.insert(sid.clone()) {
            return Err("merging repeated ACL trustees is not implemented".into());
        }
        let size = (8 + sid.len()) as u16;
        let mut ace = vec![u8::from(mode == 3), flags as u8];
        ace.extend_from_slice(&size.to_le_bytes());
        ace.extend_from_slice(&dword(0).to_le_bytes());
        ace.extend(sid);
        if mode == 3 {
            denied.extend(ace);
        } else {
            allowed.extend(ace);
        }
    }
    let size =
        u16::try_from(8 + denied.len() + allowed.len()).map_err(|_| "ACL exceeds 65535 bytes")?;
    let mut result = vec![2, 0];
    result.extend_from_slice(&size.to_le_bytes());
    result.extend_from_slice(&(count as u16).to_le_bytes());
    result.extend_from_slice(&[0, 0]);
    result.extend(denied);
    result.extend(allowed);
    Ok(result)
}

fn emulate_windows_acl(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        if operation == LegacyWin64Import::AddAccessAllowedAceEx {
            return append_guest_allowed_ace(unicorn);
        }
        if operation == LegacyWin64Import::InitializeAcl {
            let output = read_win64_import_argument(unicorn, 0)?;
            let size = read_win64_import_argument(unicorn, 1)? as u32;
            let revision = read_win64_import_argument(unicorn, 2)? as u32;
            if !(2..=4).contains(&revision) {
                unicorn.get_data_mut().windows_last_error = 87;
                return Ok(0);
            }
            if size < 8 {
                unicorn.get_data_mut().windows_last_error = 122;
                return Ok(0);
            }
            if size > u16::MAX as u32 {
                unicorn.get_data_mut().windows_last_error = 87;
                return Ok(0);
            }
            // RtlCreateAcl writes only the header and retains the supplied
            // capacity verbatim. The remaining buffer is uninitialized space.
            if output == 0 || !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
                return Err("InitializeAcl header is not writable".into());
            }
            let mut header = [0u8; 8];
            header[0] = revision as u8;
            header[2..4].copy_from_slice(&(size as u16).to_le_bytes());
            unicorn
                .mem_write(output, &header)
                .map_err(|e| e.to_string())?;
            // The buffer remains caller-owned, unlike a SetEntriesInAcl result.
            return Ok(1);
        }
        if operation == LegacyWin64Import::SetNamedSecurityInfoA {
            let address = read_win64_import_argument(unicorn, 0)?;
            let kind = read_win64_import_argument(unicorn, 1)? as u32;
            let flags = read_win64_import_argument(unicorn, 2)? as u32;
            if address == 0 {
                return Ok(87);
            }
            let name = read_crt_stdio_c_string(unicorn, address, 32768, "security object name")?;
            if kind != 1 || flags != 4 {
                return Err(format!(
                    "unsupported named security object type={kind} flags={flags:#x}"
                ));
            }
            let name =
                std::str::from_utf8(&name).map_err(|_| "unsupported security object encoding")?;
            let name = guest_file_name(name)?;
            // The asset namespace is read-only. No WRITE_DAC access is granted;
            // never mutate source permissions or pretend that a DACL was set.
            // Optional security pointers are not consumed when access is denied.
            if let Some(source) = unicorn.get_data().guest_files.sources.get(&name) {
                return match std::fs::metadata(source) {
                    Ok(_) => Ok(5),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(2),
                    Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Ok(5),
                    Err(error) => Err(format!("security object metadata: {error}")),
                };
            }
            return Ok(if unicorn.get_data().guest_files.directory_exists(&name) {
                5 // No WRITE_DAC rights are granted in the guest namespace.
            } else {
                2
            });
        }
        if operation == LegacyWin64Import::LocalFree {
            let pointer = read_win64_import_argument(unicorn, 0)?;
            if pointer == 0 {
                return Ok(0);
            }
            if unicorn
                .get_data()
                .crt_heap
                .allocations()
                .any(|(allocation, _)| allocation == pointer)
            {
                free_crt_region(unicorn, pointer)?;
                return Ok(0);
            }
            let Some(size) = unicorn
                .get_data()
                .windows_acl_allocations
                .get(&pointer)
                .copied()
            else {
                unicorn.get_data_mut().windows_last_error = 6;
                return Ok(pointer);
            };
            unicorn
                .mem_unmap(pointer, size)
                .map_err(|e| format!("LocalFree ACL: {e}"))?;
            unicorn
                .get_data_mut()
                .windows_acl_allocations
                .remove(&pointer);
            return Ok(0);
        }
        let count = read_win64_import_argument(unicorn, 0)? as u32;
        let entries = read_win64_import_argument(unicorn, 1)?;
        let old = read_win64_import_argument(unicorn, 2)?;
        let output = read_win64_import_argument(unicorn, 3)?;
        if output == 0 {
            return Ok(87);
        }
        if !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)? {
            return Err("ACL output is not writable".into());
        }
        if old != 0 {
            return Err("merging an existing ACL is not implemented".into());
        }
        if count == 0 {
            unicorn
                .mem_write(output, &0u64.to_le_bytes())
                .map_err(|e| e.to_string())?;
            return Ok(0);
        }
        let bytes = build_new_guest_acl(unicorn, entries, count)?;
        if unicorn.get_data().windows_acl_allocations.len() >= 256
            || unicorn.get_data().windows_acl_issued >= 1024
        {
            return Ok(8);
        }
        let pointer = WINDOWS_ACL_BASE + unicorn.get_data().windows_acl_issued * 65536;
        let size = (bytes.len() as u64 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        if unicorn
            .mem_map(pointer, size, Prot::READ | Prot::WRITE)
            .is_err()
        {
            return Ok(8);
        }
        if let Err(error) = unicorn
            .mem_write(pointer, &bytes)
            .and_then(|_| unicorn.mem_write(output, &pointer.to_le_bytes()))
        {
            let _ = unicorn.mem_unmap(pointer, size);
            return Err(format!("ACL output write: {error}"));
        }
        unicorn.get_data_mut().windows_acl_issued += 1;
        unicorn
            .get_data_mut()
            .windows_acl_allocations
            .insert(pointer, size);
        Ok(0)
    })();
    match result {
        Ok(value) => {
            let _ = unicorn.reg_write(RegisterX86::RAX, value);
        }
        Err(error) => {
            if unicorn.get_data().callback_error.is_none() {
                unicorn.get_data_mut().callback_error = Some(error);
            }
            let _ = unicorn.emu_stop();
        }
    }
}

fn append_guest_allowed_ace(unicorn: &mut Unicorn<'_, GuestState>) -> Result<u64, String> {
    let acl = read_win64_import_argument(unicorn, 0)?;
    let revision = read_win64_import_argument(unicorn, 1)? as u32;
    let flags = read_win64_import_argument(unicorn, 2)? as u32;
    let mask = read_win64_import_argument(unicorn, 3)? as u32;
    let sid = read_win64_import_argument(unicorn, 4)?;
    let failure = |unicorn: &mut Unicorn<'_, GuestState>, error| {
        unicorn.get_data_mut().windows_last_error = error;
        Ok(0)
    };
    if sid == 0 {
        return failure(unicorn, 1337);
    }
    let sid_header = acl_read(unicorn, sid, 8)?;
    if sid_header[0] != 1 || sid_header[1] > 15 {
        return failure(unicorn, 1337);
    }
    let sid_bytes = acl_read(unicorn, sid, 8 + u64::from(sid_header[1]) * 4)?;
    let mut header = acl_read(unicorn, acl, 8)?;
    if header[0] > 4 || revision > 4 {
        return failure(unicorn, 1306);
    }
    let capacity = u16::from_le_bytes([header[2], header[3]]) as usize;
    let count = u16::from_le_bytes([header[4], header[5]]);
    if header[0] < 2 || capacity < 8 {
        return failure(unicorn, 1336);
    }
    let mut offset = 8usize;
    for _ in 0..count {
        if offset + 4 > capacity {
            return failure(unicorn, 1336);
        }
        let address = acl
            .checked_add(offset as u64)
            .ok_or("ACL address overflow")?;
        let entry = acl_read(unicorn, address, 4)?;
        let size = u16::from_le_bytes([entry[2], entry[3]]) as usize;
        if size < 4 || size % 4 != 0 || offset + size > capacity {
            return failure(unicorn, 1336);
        }
        offset += size;
    }
    let size = 8 + sid_bytes.len();
    if offset + size > capacity || count == u16::MAX {
        return failure(unicorn, 1344);
    }
    let address = acl
        .checked_add(offset as u64)
        .ok_or("ACL append address overflow")?;
    if !guest_range_has_permission(unicorn, acl, 8, Prot::WRITE)?
        || !guest_range_has_permission(unicorn, address, size as u64, Prot::WRITE)?
    {
        return Err("AddAccessAllowedAceEx output is not writable".into());
    }
    let mut ace = vec![0, flags as u8];
    ace.extend_from_slice(&(size as u16).to_le_bytes());
    ace.extend_from_slice(&mask.to_le_bytes());
    ace.extend_from_slice(&sid_bytes);
    header[0] = header[0].max(revision as u8);
    header[4..6].copy_from_slice(&(count + 1).to_le_bytes());
    unicorn
        .mem_write(address, &ace)
        .map_err(|e| e.to_string())?;
    unicorn.mem_write(acl, &header).map_err(|e| e.to_string())?;
    Ok(1)
}

// Copy caller-owned security data before attaching it to a virtual object.
// SID/access evaluation remains the responsibility of the object backend.
#[derive(Clone, Debug)]
struct GuestObjectSecurity {
    inherit_handle: bool,
    control: u16,
    owner: Option<Vec<u8>>,
    group: Option<Vec<u8>>,
    dacl: Option<Vec<u8>>,
}

fn read_guest_object_security(
    unicorn: &Unicorn<'_, GuestState>,
    attributes: u64,
) -> Result<Option<GuestObjectSecurity>, String> {
    if attributes == 0 {
        return Ok(None);
    }
    let attrs = acl_read(unicorn, attributes, 24)?;
    if u32::from_le_bytes(attrs[..4].try_into().unwrap()) != 24 {
        return Err("invalid SECURITY_ATTRIBUTES size".into());
    }
    let pointer = u64::from_le_bytes(attrs[8..16].try_into().unwrap());
    let inherit_handle = u32::from_le_bytes(attrs[16..20].try_into().unwrap()) != 0;
    if pointer == 0 {
        return Ok(Some(GuestObjectSecurity {
            inherit_handle,
            control: 0,
            owner: None,
            group: None,
            dacl: None,
        }));
    }
    let header = acl_read(unicorn, pointer, 4)?;
    let control = u16::from_le_bytes([header[2], header[3]]);
    if header[0] != 1 {
        return Err("invalid security descriptor revision".into());
    }
    if control & 0x10 != 0 {
        return Err("object SACL support is not implemented".into());
    }
    let relative = control & 0x8000 != 0;
    let descriptor = acl_read(unicorn, pointer, if relative { 20 } else { 40 })?;
    let member = |relative_offset, absolute_offset| -> Result<u64, String> {
        if relative {
            let offset = u32::from_le_bytes(
                descriptor[relative_offset..relative_offset + 4]
                    .try_into()
                    .unwrap(),
            );
            if offset == 0 {
                Ok(0)
            } else {
                pointer
                    .checked_add(offset as u64)
                    .ok_or_else(|| "security descriptor offset overflow".into())
            }
        } else {
            Ok(u64::from_le_bytes(
                descriptor[absolute_offset..absolute_offset + 8]
                    .try_into()
                    .unwrap(),
            ))
        }
    };
    let sid = |address| -> Result<Option<Vec<u8>>, String> {
        if address == 0 {
            return Ok(None);
        }
        let header = acl_read(unicorn, address, 8)?;
        if header[0] != 1 || header[1] > 15 {
            return Err("invalid security descriptor SID".into());
        }
        Ok(Some(acl_read(unicorn, address, 8 + header[1] as u64 * 4)?))
    };
    let owner = sid(member(4, 8)?)?;
    let group = sid(member(8, 16)?)?;
    let acl = if control & 4 != 0 { member(16, 32)? } else { 0 };
    let dacl = if acl == 0 {
        None
    } else {
        let header = acl_read(unicorn, acl, 8)?;
        let size = u16::from_le_bytes([header[2], header[3]]) as usize;
        if !(2..=4).contains(&header[0]) || size < 8 || size % 4 != 0 {
            return Err("invalid object DACL".into());
        }
        let bytes = acl_read(unicorn, acl, size as u64)?;
        let count = u16::from_le_bytes([header[4], header[5]]);
        let mut offset = 8;
        for _ in 0..count {
            if offset + 4 > size {
                return Err("object DACL entry exceeds buffer".into());
            }
            let length = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
            if length < 4 || length % 4 != 0 || offset + length > size {
                return Err("invalid object DACL entry size".into());
            }
            offset += length;
        }
        Some(bytes)
    };
    Ok(Some(GuestObjectSecurity {
        inherit_handle,
        control,
        owner,
        group,
        dacl,
    }))
}

impl GuestObjectSecurity {
    fn registry_security(&self) -> Result<Option<crate::guest_registry::RegistrySecurity>, String> {
        use crate::guest_registry::{RegistryAce, RegistrySecurity};
        if self.owner.is_some() || self.group.is_some() {
            return Err("registry security owner/group token mapping is not implemented".into());
        }
        if self.control & 4 == 0 {
            return Ok(None);
        }
        let dacl = self
            .dacl
            .as_ref()
            .map(|bytes| -> Result<Vec<RegistryAce>, String> {
                let count = u16::from_le_bytes([bytes[4], bytes[5]]);
                let mut offset = 8;
                let mut entries = Vec::new();
                for _ in 0..count {
                    let length =
                        u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
                    let ace = &bytes[offset..offset + length];
                    if length != 20 || ace[0] > 1 || ace[1] & !0x1f != 0 {
                        return Err(
                            "registry security ACE type/size/flags is not implemented".into()
                        );
                    }
                    if ace[8..] != [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0] {
                        return Err(
                            "registry security trustee token mapping is not implemented".into()
                        );
                    }
                    entries.push(RegistryAce {
                        deny: ace[0] == 1,
                        flags: ace[1],
                        mask: u32::from_le_bytes(ace[4..8].try_into().unwrap()),
                    });
                    offset += length;
                }
                Ok(entries)
            })
            .transpose()?;
        Ok(Some(RegistrySecurity {
            dacl,
            protected: self.control & 0x1000 != 0,
        }))
    }
}
