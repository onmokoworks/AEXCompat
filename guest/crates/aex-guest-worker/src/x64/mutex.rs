// Kernel mutexes belong to this guest process/session; no host handles escape.
const WINDOWS_MUTEX_BASE: u64 = 0x0000_0006_0000_0000;
#[derive(Default)]
struct WindowsMutexes {
    handles: BTreeMap<u64, u64>,
    objects: BTreeMap<u64, WindowsMutex>,
    names: BTreeMap<String, u64>,
    issued: u64,
}
struct WindowsMutex {
    name: Option<String>,
    owner: Option<u32>,
    depth: u32,
    abandoned: bool,
    references: u32,
}
impl WindowsMutexes {
    fn create(&mut self, name: Option<String>, owner: Option<u32>) -> Result<(u64, u32), String> {
        if self.handles.len() >= 4096 || self.issued >= 65536 {
            return Ok((0, 8));
        }
        let existing = name.as_ref().and_then(|name| self.names.get(name)).copied();
        if existing.is_none() && self.objects.len() >= 1024 {
            return Ok((0, 8));
        }
        let handle = WINDOWS_MUTEX_BASE + self.issued * 8;
        self.issued += 1;
        let object = existing.unwrap_or(handle);
        if let Some(mutex) = self.objects.get_mut(&object) {
            mutex.references += 1;
        } else {
            if let Some(name) = &name {
                self.names.insert(name.clone(), object);
            }
            self.objects.insert(
                object,
                WindowsMutex {
                    name,
                    owner,
                    depth: u32::from(owner.is_some()),
                    abandoned: false,
                    references: 1,
                },
            );
        }
        self.handles.insert(handle, object);
        Ok((handle, if existing.is_some() { 183 } else { 0 }))
    }
    fn wait(&mut self, handle: u64, thread: u32, timeout: u32) -> Result<u64, String> {
        let object = self.handles.get(&handle).ok_or("invalid mutex handle")?;
        let mutex = self.objects.get_mut(object).ok_or("missing mutex object")?;
        match mutex.owner {
            Some(owner) if owner != thread => {
                if timeout == 0 {
                    Ok(258)
                } else {
                    Err("blocking mutex contention requires scheduler integration".into())
                }
            }
            _ => {
                mutex.depth = mutex
                    .depth
                    .checked_add(1)
                    .ok_or("mutex recursion overflow")?;
                mutex.owner = Some(thread);
                let result = if mutex.abandoned { 128 } else { 0 };
                mutex.abandoned = false;
                Ok(result)
            }
        }
    }
    fn release(&mut self, handle: u64, thread: u32) -> Result<(), u32> {
        let object = self.handles.get(&handle).ok_or(6u32)?;
        let mutex = self.objects.get_mut(object).ok_or(6u32)?;
        if mutex.owner != Some(thread) {
            return Err(288);
        }
        mutex.depth -= 1;
        if mutex.depth == 0 {
            mutex.owner = None;
        }
        Ok(())
    }
    fn close(&mut self, handle: u64) -> Result<(), String> {
        let object = self.handles.remove(&handle).ok_or("invalid mutex close")?;
        let mutex = self
            .objects
            .get_mut(&object)
            .ok_or("missing mutex object")?;
        mutex.references -= 1;
        if mutex.references == 0 {
            let mutex = self.objects.remove(&object).unwrap();
            if let Some(name) = mutex.name {
                self.names.remove(&name);
            }
        }
        Ok(())
    }
    fn abandon(&mut self, thread: u32) {
        for mutex in self.objects.values_mut() {
            if mutex.owner == Some(thread) {
                mutex.owner = None;
                mutex.depth = 0;
                mutex.abandoned = true;
            }
        }
    }
}

fn emulate_windows_mutex(unicorn: &mut Unicorn<'_, GuestState>, operation: LegacyWin64Import) {
    let result = (|| -> Result<u64, String> {
        let thread = unicorn.get_data().current_windows_thread_id;
        if operation == LegacyWin64Import::CreateMutexA {
            let attributes = read_win64_import_argument(unicorn, 0)?;
            let initial = read_win64_import_argument(unicorn, 1)? as u32 != 0;
            let name = read_win64_import_argument(unicorn, 2)?;
            // Security and inheritance need their own process/object model.
            if attributes != 0 {
                return Err("CreateMutexA security attributes are unsupported".into());
            }
            let name = if name == 0 {
                None
            } else {
                let bytes = read_crt_stdio_c_string(unicorn, name, 260, "CreateMutexA name")?;
                if bytes.is_empty() || !bytes.is_ascii() {
                    return Err("unsupported mutex name".into());
                }
                let name = String::from_utf8(bytes).unwrap();
                let (key, remainder) = if let Some(local) = name.strip_prefix("Local\\") {
                    (local, local)
                } else if let Some(global) = name.strip_prefix("Global\\") {
                    (name.as_str(), global)
                } else {
                    (name.as_str(), name.as_str())
                };
                if remainder.is_empty() || remainder.contains('\\') {
                    return Err("unsupported mutex namespace".into());
                }
                Some(key.to_string())
            };
            let (handle, status) = unicorn
                .get_data_mut()
                .windows_mutexes
                .create(name, initial.then_some(thread))?;
            unicorn.get_data_mut().windows_last_error = status;
            return Ok(handle);
        }
        let handle = read_win64_import_argument(unicorn, 0)?;
        match operation {
            LegacyWin64Import::ReleaseMutex => {
                match unicorn
                    .get_data_mut()
                    .windows_mutexes
                    .release(handle, thread)
                {
                    Ok(()) => Ok(1),
                    Err(error) => {
                        unicorn.get_data_mut().windows_last_error = error;
                        Ok(0)
                    }
                }
            }
            LegacyWin64Import::CloseHandle => {
                unicorn.get_data_mut().windows_mutexes.close(handle)?;
                Ok(1)
            }
            LegacyWin64Import::WaitForSingleObject | LegacyWin64Import::WaitForSingleObjectEx => {
                let timeout = read_win64_import_argument(unicorn, 1)? as u32;
                if operation == LegacyWin64Import::WaitForSingleObjectEx
                    && read_win64_import_argument(unicorn, 2)? as u32 != 0
                {
                    return Err("alertable mutex waits are unsupported".into());
                }
                unicorn
                    .get_data_mut()
                    .windows_mutexes
                    .wait(handle, thread, timeout)
            }
            _ => Err("invalid mutex operation".into()),
        }
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
