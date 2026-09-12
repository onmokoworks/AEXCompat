// Kernel synchronization objects belong to this guest process/session; no host handles escape.
const WINDOWS_KERNEL_OBJECT_BASE: u64 = 0x0000_0006_0000_0000;
#[derive(Default)]
struct WindowsKernelObjects {
    handles: BTreeMap<u64, u64>,
    objects: BTreeMap<u64, WindowsMutex>,
    semaphores: BTreeMap<u64, WindowsSemaphore>,
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
struct WindowsSemaphore {
    name: Option<String>,
    count: i32,
    maximum: i32,
    references: u32,
}
impl WindowsKernelObjects {
    fn create(&mut self, name: Option<String>, owner: Option<u32>) -> Result<(u64, u32), String> {
        if self.handles.len() >= 4096 || self.issued >= 65536 {
            return Ok((0, 8));
        }
        let existing = name.as_ref().and_then(|name| self.names.get(name)).copied();
        if existing.is_some_and(|id| self.semaphores.contains_key(&id)) {
            return Ok((0, 6));
        }
        if existing.is_none() && self.objects.len() + self.semaphores.len() >= 1024 {
            return Ok((0, 8));
        }
        let handle = WINDOWS_KERNEL_OBJECT_BASE + self.issued * 8;
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
    fn create_semaphore(
        &mut self,
        name: Option<String>,
        initial: i32,
        maximum: i32,
    ) -> Result<(u64, u32), String> {
        let existing = name.as_ref().and_then(|n| self.names.get(n)).copied();
        if existing.is_some_and(|id| self.objects.contains_key(&id)) {
            return Ok((0, 6));
        }
        if existing.is_none() && (initial < 0 || maximum <= 0 || initial > maximum) {
            return Ok((0, 87));
        }
        if self.handles.len() >= 4096
            || self.issued >= 65536
            || (existing.is_none() && self.objects.len() + self.semaphores.len() >= 1024)
        {
            return Ok((0, 8));
        }
        let handle = WINDOWS_KERNEL_OBJECT_BASE + self.issued * 8;
        self.issued += 1;
        let object = existing.unwrap_or(handle);
        if let Some(semaphore) = self.semaphores.get_mut(&object) {
            semaphore.references += 1;
        } else {
            if let Some(name) = &name {
                self.names.insert(name.clone(), object);
            }
            self.semaphores.insert(
                object,
                WindowsSemaphore {
                    name,
                    count: initial,
                    maximum,
                    references: 1,
                },
            );
        }
        self.handles.insert(handle, object);
        Ok((handle, if existing.is_some() { 183 } else { 0 }))
    }
    fn wait(&mut self, handle: u64, thread: u32, timeout: u32) -> Result<u64, String> {
        let object = self.handles.get(&handle).ok_or("invalid mutex handle")?;
        if let Some(semaphore) = self.semaphores.get_mut(object) {
            if semaphore.count > 0 {
                semaphore.count -= 1;
                return Ok(0);
            }
            return if timeout == 0 {
                Ok(258)
            } else {
                Err("blocking semaphore wait requires scheduler integration".into())
            };
        }
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
        if let Some(semaphore) = self.semaphores.get_mut(&object) {
            semaphore.references -= 1;
            if semaphore.references == 0 {
                let semaphore = self.semaphores.remove(&object).unwrap();
                if let Some(name) = semaphore.name {
                    self.names.remove(&name);
                }
            }
            return Ok(());
        }
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

fn emulate_windows_kernel_object(
    unicorn: &mut Unicorn<'_, GuestState>,
    operation: LegacyWin64Import,
) {
    let result = (|| -> Result<u64, String> {
        let thread = unicorn.get_data().current_windows_thread_id;
        if matches!(
            operation,
            LegacyWin64Import::CreateMutexA | LegacyWin64Import::CreateSemaphoreA
        ) {
            let attributes = read_win64_import_argument(unicorn, 0)?;
            let initial = read_win64_import_argument(unicorn, 1)? as u32;
            let semaphore = operation == LegacyWin64Import::CreateSemaphoreA;
            let maximum = if semaphore {
                read_win64_import_argument(unicorn, 2)? as u32 as i32
            } else {
                0
            };
            let name = read_win64_import_argument(unicorn, if semaphore { 3 } else { 2 })?;
            // Security and inheritance need their own process/object model.
            if attributes != 0 {
                return Err("kernel object security attributes are unsupported".into());
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
            let (handle, status) = if semaphore {
                unicorn.get_data_mut().windows_objects.create_semaphore(
                    name,
                    initial as i32,
                    maximum,
                )?
            } else {
                unicorn
                    .get_data_mut()
                    .windows_objects
                    .create(name, (initial != 0).then_some(thread))?
            };
            unicorn.get_data_mut().windows_last_error = status;
            return Ok(handle);
        }
        let handle = read_win64_import_argument(unicorn, 0)?;
        match operation {
            LegacyWin64Import::ReleaseSemaphore => {
                let amount = read_win64_import_argument(unicorn, 1)? as u32 as i32;
                let output = read_win64_import_argument(unicorn, 2)?;
                let prepared = (|| -> Result<(u64, i32, i32), u32> {
                    let objects = &unicorn.get_data().windows_objects;
                    let object = *objects.handles.get(&handle).ok_or(6u32)?;
                    let sem = objects.semaphores.get(&object).ok_or(6u32)?;
                    if amount <= 0 {
                        return Err(87);
                    }
                    let next = sem
                        .count
                        .checked_add(amount)
                        .filter(|n| *n <= sem.maximum)
                        .ok_or(298u32)?;
                    Ok((object, sem.count, next))
                })();
                let (object, previous, next) = match prepared {
                    Ok(values) => values,
                    Err(error) => {
                        unicorn.get_data_mut().windows_last_error = error;
                        return Ok(0);
                    }
                };
                if output != 0 {
                    if !guest_range_has_permission(unicorn, output, 4, Prot::WRITE)? {
                        return Err("ReleaseSemaphore previous count output is not writable".into());
                    }
                    unicorn
                        .mem_write(output, &previous.to_le_bytes())
                        .map_err(|e| format!("ReleaseSemaphore previous count: {e}"))?;
                }
                unicorn
                    .get_data_mut()
                    .windows_objects
                    .semaphores
                    .get_mut(&object)
                    .unwrap()
                    .count = next;
                Ok(1)
            }
            LegacyWin64Import::ReleaseMutex => {
                match unicorn
                    .get_data_mut()
                    .windows_objects
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
                unicorn.get_data_mut().windows_objects.close(handle)?;
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
                    .windows_objects
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
