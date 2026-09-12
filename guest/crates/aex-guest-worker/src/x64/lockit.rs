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
