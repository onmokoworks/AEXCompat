//! Windows x64 debug-API adapter. Compile-checked from macOS; runtime validation
//! must be performed on Windows. It attaches to an existing isolated worker;
//! it does not replace the broker's Job Object/private desktop launch path.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    mem::{size_of, zeroed},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::GetFinalPathNameByHandleW,
    System::{Diagnostics::Debug::*, Memory::*, ProcessStatus::*, Threading::*},
};

#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub transport: &'static str,
    pub success: bool,
    pub reason: String,
    pub attached: bool,
    pub detached: bool,
    pub target_terminated_on_error: bool,
    pub config: Config,
    pub module: Option<Module>,
    pub hits: Vec<u64>,
    pub memory_witnesses: Vec<Witness>,
}
#[derive(Serialize)]
pub struct Module {
    pub name: String,
    pub base: u64,
    pub size: u64,
    pub file_sha256: String,
}

fn error(context: &str) -> String {
    format!("{context}: {}", std::io::Error::last_os_error())
}
fn check(ok: i32, context: &str) -> Result<(), String> {
    if ok == 0 { Err(error(context)) } else { Ok(()) }
}

#[repr(C, align(16))]
struct AlignedContext(CONTEXT);
fn context(thread: HANDLE, flags: u32) -> Result<AlignedContext, String> {
    let mut c = AlignedContext(unsafe { zeroed() });
    c.0.ContextFlags = flags;
    check(
        unsafe { GetThreadContext(thread, &mut c.0) },
        "GetThreadContext",
    )?;
    Ok(c)
}
fn set(thread: HANDLE, c: &AlignedContext) -> Result<(), String> {
    check(
        unsafe { SetThreadContext(thread, &c.0) },
        "SetThreadContext",
    )
}

// These are our own handles, not the event handles automatically closed by
// ContinueDebugEvent on EXIT_THREAD/EXIT_PROCESS.
struct Thread {
    handle: OwnedHandle,
    original: Option<[u64; 6]>,
}
impl Thread {
    fn open(id: u32) -> Result<Self, String> {
        let h = unsafe {
            OpenThread(
                THREAD_GET_CONTEXT | THREAD_SET_CONTEXT | THREAD_QUERY_INFORMATION,
                0,
                id,
            )
        };
        if h.is_null() {
            return Err(error("OpenThread"));
        }
        Ok(Self {
            handle: unsafe { OwnedHandle::from_raw_handle(h) },
            original: None,
        })
    }
    fn handle(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }
    fn arm(&mut self, addresses: &[u64]) -> Result<(), String> {
        if self.original.is_some() {
            return Ok(());
        }
        let mut c = context(self.handle(), CONTEXT_DEBUG_REGISTERS_AMD64)?;
        if c.0.Dr7 & 0x20ff != 0 {
            return Err("existing debug registers are in use".into());
        }
        self.original = Some([c.0.Dr0, c.0.Dr1, c.0.Dr2, c.0.Dr3, c.0.Dr6, c.0.Dr7]);
        let mut sites = [0; 4];
        sites[..addresses.len()].copy_from_slice(addresses);
        [c.0.Dr0, c.0.Dr1, c.0.Dr2, c.0.Dr3] = sites;
        c.0.Dr6 = 0;
        // Local enable, execution type, one-byte length for every occupied slot.
        c.0.Dr7 = (0..addresses.len()).fold(0, |v, i| v | (1 << (2 * i)));
        set(self.handle(), &c)
    }
    fn restore(&mut self) -> Result<(), String> {
        if let Some(original) = self.original {
            let mut c = context(self.handle(), CONTEXT_DEBUG_REGISTERS_AMD64)?;
            [c.0.Dr0, c.0.Dr1, c.0.Dr2, c.0.Dr3, c.0.Dr6, c.0.Dr7] = original;
            set(self.handle(), &c)?;
            self.original = None;
        }
        Ok(())
    }
}
struct Remote(HANDLE);
impl Memory for Remote {
    fn read_exact(&mut self, address: u64, bytes: &mut [u8]) -> bool {
        let mut read = 0;
        unsafe {
            ReadProcessMemory(
                self.0,
                address as *const _,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut read,
            ) != 0
                && read == bytes.len()
        }
    }
}

fn module_from_event(
    file: HANDLE,
    base: u64,
    process: HANDLE,
    wanted: &str,
) -> Result<Option<Module>, String> {
    if file.is_null() || file == INVALID_HANDLE_VALUE {
        return Ok(None);
    }
    let mut file = unsafe { File::from_raw_handle(file) }; // owns and always closes event hFile
    let mut path = vec![0u16; 32768];
    let n = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            path.as_mut_ptr(),
            path.len() as u32,
            0,
        )
    } as usize;
    if n == 0 || n >= path.len() {
        return Err(error("GetFinalPathNameByHandleW"));
    }
    let path = String::from_utf16(&path[..n]).map_err(|_| "module name encoding")?;
    let name = path.rsplit('\\').next().ok_or("module basename")?;
    if !name.eq_ignore_ascii_case(wanted) {
        return Ok(None);
    }
    let mut info: MODULEINFO = unsafe { zeroed() };
    check(
        unsafe {
            K32GetModuleInformation(
                process,
                base as _,
                &mut info,
                size_of::<MODULEINFO>() as u32,
            )
        },
        "GetModuleInformation",
    )?;
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(Some(Module {
        name: name.into(),
        base,
        size: info.SizeOfImage.into(),
        file_sha256: format!("{:x}", hash.finalize()),
    }))
}

fn registers(c: &CONTEXT) -> BTreeMap<Register, u64> {
    use Register::*;
    BTreeMap::from([
        (Rax, c.Rax),
        (Rbx, c.Rbx),
        (Rcx, c.Rcx),
        (Rdx, c.Rdx),
        (Rsi, c.Rsi),
        (Rdi, c.Rdi),
        (Rbp, c.Rbp),
        (Rsp, c.Rsp),
        (R8, c.R8),
        (R9, c.R9),
        (R10, c.R10),
        (R11, c.R11),
        (R12, c.R12),
        (R13, c.R13),
        (R14, c.R14),
        (R15, c.R15),
    ])
}

fn executable(process: HANDLE, address: u64) -> bool {
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { zeroed() };
    unsafe {
        VirtualQueryEx(
            process,
            address as _,
            &mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        ) != 0
            && info.State == MEM_COMMIT
            && info.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0
            && info.Protect
                & (PAGE_EXECUTE
                    | PAGE_EXECUTE_READ
                    | PAGE_EXECUTE_READWRITE
                    | PAGE_EXECUTE_WRITECOPY)
                != 0
    }
}

/// Public CLI is deliberately restricted to the existing worker executable.
/// This is process-role admission, not an identity/hash execution gate.
pub fn require_worker(pid: u32) -> Result<(), String> {
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return Err(error("OpenProcess for role check"));
    }
    let h = unsafe { OwnedHandle::from_raw_handle(h) };
    let mut name = vec![0u16; 32768];
    let mut n = name.len() as u32;
    check(
        unsafe { QueryFullProcessImageNameW(h.as_raw_handle(), 0, name.as_mut_ptr(), &mut n) },
        "QueryFullProcessImageNameW",
    )?;
    let name = String::from_utf16(&name[..n as usize]).map_err(|_| "worker name encoding")?;
    if !name
        .rsplit('\\')
        .next()
        .unwrap_or("")
        .eq_ignore_ascii_case("aex_worker.exe")
    {
        return Err(
            "attach target must be the broker's aex_worker.exe, held idle in a resident session"
                .into(),
        );
    }
    Ok(())
}

pub fn observe(
    pid: u32,
    mut observer: Observer,
    timeout: Duration,
    mut armed: impl FnMut(),
) -> Report {
    let mut report = Report {
        schema: "aexcompat.native-instruction-watch/1",
        transport: "windows-x64-hardware-execution-breakpoint",
        success: false,
        reason: String::new(),
        attached: false,
        detached: false,
        target_terminated_on_error: false,
        config: observer.config.clone(),
        module: None,
        hits: Vec::new(),
        memory_witnesses: Vec::new(),
    };
    let result = (|| -> Result<(), String> {
        let h = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_TERMINATE,
                0,
                pid,
            )
        };
        if h.is_null() {
            return Err(error("OpenProcess"));
        }
        let process = unsafe { OwnedHandle::from_raw_handle(h) };
        let mut wow64 = 0;
        check(unsafe { IsWow64Process(h, &mut wow64) }, "IsWow64Process")?;
        if wow64 != 0 {
            return Err("WOW64 target is not supported".into());
        }
        check(unsafe { DebugActiveProcess(pid) }, "DebugActiveProcess")?;
        report.attached = true;
        // Default kill-on-debugger-exit is retained: an observer crash must not
        // leave an armed, unowned debug-register trap in a live worker.
        let mut threads = BTreeMap::<u32, Thread>::new();
        let mut addresses = Vec::new();
        let deadline = Instant::now() + timeout;
        let mut initial_break = true;
        let mut events = 0u64;
        let mut announced = false;
        let outcome = (|| -> Result<(), String> {
            loop {
                let mut event: DEBUG_EVENT = unsafe { zeroed() };
                if unsafe { WaitForDebugEvent(&mut event, 50) } == 0 {
                    let last = unsafe { GetLastError() };
                    if last != ERROR_SEM_TIMEOUT {
                        return Err(format!("WaitForDebugEvent: {last}"));
                    }
                    if Instant::now() >= deadline {
                        return Err("idle observation timeout; terminating isolated worker because no stopped event is available for restoration".into());
                    }
                    continue;
                }
                events += 1;
                let mut continuation = DBG_CONTINUE;
                let mut exited = false;
                let event_result = (|| -> Result<(), String> {
                    match event.dwDebugEventCode {
                        CREATE_PROCESS_DEBUG_EVENT => {
                            let info = unsafe { event.u.CreateProcessInfo };
                            // Process image can be the watched module too.
                            report.module = module_from_event(
                                info.hFile,
                                info.lpBaseOfImage as u64,
                                h,
                                &observer.config.module,
                            )?;
                            threads.insert(event.dwThreadId, Thread::open(event.dwThreadId)?);
                        }
                        CREATE_THREAD_DEBUG_EVENT => {
                            threads.insert(event.dwThreadId, Thread::open(event.dwThreadId)?);
                        }
                        EXIT_THREAD_DEBUG_EVENT => {
                            threads.remove(&event.dwThreadId);
                        }
                        LOAD_DLL_DEBUG_EVENT => {
                            let info = unsafe { event.u.LoadDll };
                            if let Some(module) = module_from_event(
                                info.hFile,
                                info.lpBaseOfDll as u64,
                                h,
                                &observer.config.module,
                            )? {
                                if report.module.is_some() {
                                    return Err("ambiguous duplicate module basename".into());
                                }
                                report.module = Some(module);
                            }
                        }
                        UNLOAD_DLL_DEBUG_EVENT => {
                            if report.module.as_ref().is_some_and(|m| {
                                m.base == unsafe { event.u.UnloadDll.lpBaseOfDll as u64 }
                            }) {
                                return Err("watched module unloaded".into());
                            }
                        }
                        EXIT_PROCESS_DEBUG_EVENT => {
                            exited = true;
                        }
                        EXCEPTION_DEBUG_EVENT => {
                            let ex = unsafe { event.u.Exception };
                            continuation = DBG_EXCEPTION_NOT_HANDLED;
                            if ex.ExceptionRecord.ExceptionCode == EXCEPTION_BREAKPOINT
                                && initial_break
                                && ex.dwFirstChance != 0
                            {
                                initial_break = false;
                                continuation = DBG_CONTINUE;
                            } else if ex.ExceptionRecord.ExceptionCode == EXCEPTION_SINGLE_STEP
                                && ex.dwFirstChance != 0
                            {
                                let thread = threads
                                    .get(&event.dwThreadId)
                                    .ok_or("unknown exception thread")?;
                                let mut c = context(
                                    thread.handle(),
                                    CONTEXT_CONTROL_AMD64
                                        | CONTEXT_INTEGER_AMD64
                                        | CONTEXT_DEBUG_REGISTERS_AMD64,
                                )?;
                                let mask = (1u64 << addresses.len()) - 1;
                                let touched = c.0.Dr6 & mask;
                                if touched != 0 {
                                    if c.0.Dr6 & (0xe000 | (15 & !mask)) != 0 {
                                        return Err("mixed owned/unowned debug exception".into());
                                    }
                                    let index = touched.trailing_zeros() as usize;
                                    if touched.count_ones() != 1 || c.0.Rip != addresses[index] {
                                        return Err(
                                            "debug exception address disagrees with watch".into()
                                        );
                                    }
                                    let rva = observer.config.watches[index].instruction_rva;
                                    observer.hit(
                                        rva,
                                        event.dwThreadId,
                                        &registers(&c.0),
                                        &mut Remote(h),
                                    )?;
                                    c.0.EFlags |= 1 << 16; // RF: resume this instruction without retriggering.
                                    c.0.Dr6 &= !mask;
                                    set(thread.handle(), &c)?;
                                    continuation = DBG_CONTINUE;
                                }
                            }
                        }
                        _ => {}
                    }
                    if addresses.is_empty() {
                        if let Some(m) = &report.module {
                            addresses = observer.addresses(m.base, m.size)?;
                            if addresses.iter().any(|a| !executable(h, *a)) {
                                return Err("watch site is not executable committed memory".into());
                            }
                        }
                    }
                    if !addresses.is_empty() && !exited {
                        for thread in threads.values_mut() {
                            thread.arm(&addresses)?;
                        }
                        if !initial_break && !announced {
                            armed();
                            announced = true;
                        }
                    }
                    Ok(())
                })();
                let done = observer.complete()
                    || Instant::now() >= deadline
                    || events >= 1_000_000
                    || event_result.is_err()
                    || exited;
                let mut cleanup = Ok(());
                if done && !exited {
                    // Debug events suspend *all* target threads. Restore before
                    // continuing so detach cannot expose stale hardware traps.
                    for thread in threads.values_mut() {
                        if let Err(e) = thread.restore() {
                            cleanup = Err(e);
                        }
                    }
                }
                let continued = check(
                    unsafe {
                        ContinueDebugEvent(event.dwProcessId, event.dwThreadId, continuation)
                    },
                    "ContinueDebugEvent",
                );
                cleanup?;
                continued?;
                event_result?;
                if done {
                    if initial_break && !exited {
                        return Err("observation ended during attach; terminating worker with pending initial breakpoint".into());
                    }
                    if !exited {
                        check(
                            unsafe { DebugActiveProcessStop(pid) },
                            "DebugActiveProcessStop",
                        )?;
                    }
                    report.detached = !exited;
                    if observer.captured() {
                        return Ok(());
                    }
                    return Err(if observer.complete() {
                        "capture contained unreadable memory"
                    } else if exited {
                        "target exited before requested occurrences"
                    } else {
                        "observation limit reached before requested occurrences"
                    }
                    .into());
                }
            }
        })();
        // If normal restoration/detach did not finish, contain failure in the
        // already isolated worker. Never silently resume an armed target.
        if outcome.is_err() && !report.detached {
            report.target_terminated_on_error =
                unsafe { TerminateProcess(process.as_raw_handle(), 91) } != 0;
            unsafe {
                DebugActiveProcessStop(pid);
            }
        }
        outcome
    })();
    report.success = result.is_ok();
    report.reason = result
        .err()
        .unwrap_or_else(|| "requested occurrences captured".into());
    report.hits = observer.hits;
    report.memory_witnesses = observer.witnesses;
    report
}

// These tests compile on the Windows cross-check path. They must NOT be
// described as executed by a macOS test run; all are ignored until explicitly
// selected on Windows. The child contains no AEX and needs no broker launch.
#[cfg(test)]
mod runtime_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

    #[inline(never)]
    extern "C" fn probe(pointer: *const u8) -> usize {
        std::hint::black_box(pointer as usize)
    }

    #[test]
    #[ignore = "Windows debuggee child; invoked only by the parent test"]
    fn debuggee_child() {
        if std::env::var_os("AEXCOMPAT_WATCH_TEST_CHILD").is_none() {
            return;
        }
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as u64;
        println!("WATCH_READY {}", probe as *const () as u64 - base);
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        for bytes in [[1u8, 2, 3, 4], [5u8, 6, 7, 8]] {
            std::thread::spawn(move || {
                probe(
                    if std::env::var_os("AEXCOMPAT_WATCH_TEST_UNREADABLE").is_some() {
                        std::ptr::null()
                    } else {
                        bytes.as_ptr()
                    },
                );
            })
            .join()
            .unwrap();
        }
        if std::env::var_os("AEXCOMPAT_WATCH_TEST_IDLE").is_some() {
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    fn run(occurrence: u64, unreadable: bool, idle: bool) -> Report {
        let exe = std::env::current_exe().unwrap();
        let mut command = Command::new(&exe);
        if unreadable {
            command.env("AEXCOMPAT_WATCH_TEST_UNREADABLE", "1");
        }
        if idle {
            command.env("AEXCOMPAT_WATCH_TEST_IDLE", "1");
        }
        let mut child = command
            .args([
                "--exact",
                "native_watch::windows::runtime_tests::debuggee_child",
                "--ignored",
                "--nocapture",
            ])
            .env("AEXCOMPAT_WATCH_TEST_CHILD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let rva = loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
            if let Some((_, rva)) = line.split_once("WATCH_READY ") {
                break rva.trim().parse().unwrap();
            }
        };
        let config = Config {
            module: exe.file_name().unwrap().to_str().unwrap().into(),
            watches: vec![Watch {
                id: "second-thread".into(),
                instruction_rva: rva,
                register: Register::Rcx,
                dereference_offsets: if unreadable { vec![0] } else { vec![] },
                size: 4,
                occurrence,
            }],
        };
        let mut stdin = child.stdin.take().unwrap();
        let report = observe(
            child.id(),
            Observer::new(config).unwrap(),
            if idle {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(15)
            },
            || {
                writeln!(stdin, "go").unwrap();
            },
        );
        println!(
            "native_watch_test_report {}",
            serde_json::to_string(&report).unwrap()
        );
        drop(stdin);
        // Prevent a failed attach from leaving the blocked helper alive.
        if !report.attached {
            child.kill().unwrap();
        }
        let status = child.wait().unwrap();
        if report.detached {
            assert!(status.success());
        }
        report
    }

    #[test]
    #[ignore = "requires real Windows debug API execution"]
    fn native_hardware_watch_captures_second_thread_and_detaches() {
        let r = run(2, false, false);
        assert!(r.success, "{}", r.reason);
        assert!(r.detached);
        assert_eq!(r.hits, vec![2]);
        assert_eq!(
            r.memory_witnesses[0].before.hex.as_deref(),
            Some("05060708")
        );
        assert!(!r.target_terminated_on_error);
    }
    #[test]
    #[ignore = "requires real Windows debug API execution"]
    fn native_hardware_watch_reports_unreadable_without_fake_capture() {
        let r = run(1, true, false);
        assert!(!r.success);
        assert!(r.detached);
        assert!(r.memory_witnesses[0].before.hex.is_none());
        assert!(!r.target_terminated_on_error);
    }
    #[test]
    #[ignore = "requires real Windows debug API execution"]
    fn native_hardware_watch_reports_missing_occurrence() {
        let r = run(99, false, false);
        assert!(!r.success);
        assert_eq!(r.hits, vec![2]);
        assert!(r.memory_witnesses.is_empty());
        assert!(r.reason.contains("target exited"));
    }
    #[test]
    #[ignore = "requires real Windows debug API execution"]
    fn native_hardware_watch_idle_timeout_contains_target() {
        let r = run(99, false, true);
        assert!(!r.success);
        assert!(r.reason.contains("timeout"), "{}", r.reason);
        assert!(r.target_terminated_on_error);
        assert!(!r.detached);
    }
}
