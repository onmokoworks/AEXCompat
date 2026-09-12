const WINDOWS_SID_BASE: u64 = 0x0000_0007_0000_0000;

fn emulate_windows_sid(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        if operation == LegacyWin64Import::FreeSid {
            let sid = read_win64_import_argument(unicorn, 0)?;
            if !unicorn.get_data().windows_sids.contains(&sid) {
                return Err("FreeSid received stale or foreign allocation".into());
            }
            unicorn
                .mem_unmap(sid, PAGE_SIZE)
                .map_err(|e| format!("FreeSid unmap: {e}"))?;
            unicorn.get_data_mut().windows_sids.remove(&sid);
            return Ok(0);
        }
        let authority = read_win64_import_argument(unicorn, 0)?;
        let count = read_win64_import_argument(unicorn, 1)? as u8;
        let output = read_win64_import_argument(unicorn, 10)?;
        if count > 8 {
            unicorn.get_data_mut().windows_last_error = 1337; // ERROR_INVALID_SID
            return Ok(0);
        }
        if authority == 0 || output == 0 {
            unicorn.get_data_mut().windows_last_error = 87;
            return Ok(0);
        }
        if !guest_range_has_permission(unicorn, authority, 6, Prot::READ)?
            || !guest_range_has_permission(unicorn, output, 8, Prot::WRITE)?
        {
            return Err(
                "AllocateAndInitializeSid requires readable authority and writable output".into(),
            );
        }
        let mut bytes = vec![1, count];
        bytes.extend(
            unicorn
                .mem_read_as_vec(authority, 6)
                .map_err(|e| format!("SID authority: {e}"))?,
        );
        for index in 0..count {
            bytes.extend_from_slice(
                &(read_win64_import_argument(unicorn, usize::from(index) + 2)? as u32)
                    .to_le_bytes(),
            );
        }
        if unicorn.get_data().windows_sids.len() >= 1024
            || unicorn.get_data().windows_sid_issued >= 4096
        {
            unicorn.get_data_mut().windows_last_error = 8;
            return Ok(0);
        }
        let sid = WINDOWS_SID_BASE + unicorn.get_data().windows_sid_issued * PAGE_SIZE;
        if unicorn
            .mem_map(sid, PAGE_SIZE, Prot::READ | Prot::WRITE)
            .is_err()
        {
            unicorn.get_data_mut().windows_last_error = 8;
            return Ok(0);
        }
        if let Err(error) = unicorn
            .mem_write(sid, &bytes)
            .and_then(|_| unicorn.mem_write(output, &sid.to_le_bytes()))
        {
            let _ = unicorn.mem_unmap(sid, PAGE_SIZE);
            return Err(format!("SID output write: {error}"));
        }
        unicorn.get_data_mut().windows_sid_issued += 1;
        unicorn.get_data_mut().windows_sids.insert(sid);
        Ok(1)
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
