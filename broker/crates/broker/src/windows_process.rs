use crate::restricted_worker_acl::RestrictedWorkerSid;
use crate::restricted_worker_token::RestrictedWorkerToken;
use crate::{ExitClassification, classify_exit, redact_windows_paths};
use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, LocalFree,
    SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, CreateDesktopW, GetProcessWindowStation, GetThreadDesktop,
    GetUserObjectInformationW, HDESK, UOI_NAME,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateEventW,
    CreateProcessAsUserW, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentThreadId, GetExitCodeProcess,
    InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
    ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

// Native render stdout is one bounded JSON report. Its 65,536 Suite events
// can occupy more than 16 MiB after escaping 96-byte safely copied names.
// Keep enough for the worker contract while retaining a hard memory bound.
pub const STDOUT_CAPTURE_LIMIT: usize = 24 * 1024 * 1024;
const STDERR_CAPTURE_LIMIT: usize = 64 * 1024;
const PROCESS_MEMORY_LIMIT: usize = 512 * 1024 * 1024;
const TERMINATION_GRACE_MS: u32 = 5_000;
// See memory_limit_reached: the largest single failed allocation the
// detection tolerates between the recorded peak and the cap.
const MEMORY_LIMIT_DETECTION_SLACK: u64 = 16 * 1024 * 1024;
const DESKTOP_WORKER_ACCESS: u32 = 0x0000_01ff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerDesktopPolicy {
    /// Non-interactive discovery/render workers get a private desktop so a
    /// modal UI cannot appear on the user's input desktop.
    Dedicated,
    /// Explicit GUI harnesses retain the caller's current desktop.
    Current,
}

struct DesktopSecurityDescriptor(std::ptr::NonNull<std::ffi::c_void>);

impl DesktopSecurityDescriptor {
    fn new(worker_sid: Option<&RestrictedWorkerSid>) -> io::Result<Self> {
        // Keep the desktop private to the broker's window-station owner and
        // SYSTEM. A restricted worker gets only the object-level rights needed
        // to create and operate UI objects; it cannot mutate the desktop ACL.
        // The protected DACL prevents the parent station ACL from being
        // inherited into this boundary.
        let worker_ace = worker_sid
            .map(|sid| format!("(A;;0x000001ff;;;{})", sid.as_str()))
            .unwrap_or_default();
        let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;OW){worker_ace}");
        let text: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
        let mut raw: PSECURITY_DESCRIPTOR = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut raw,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(
            std::ptr::NonNull::new(raw).ok_or_else(io::Error::last_os_error)?,
        ))
    }

    fn raw(&self) -> PSECURITY_DESCRIPTOR {
        self.0.as_ptr()
    }
}

impl Drop for DesktopSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0.as_ptr());
        }
    }
}

fn user_object_name(handle: HANDLE) -> io::Result<String> {
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut required_bytes = 0u32;
    unsafe {
        GetUserObjectInformationW(handle, UOI_NAME, null_mut(), 0, &mut required_bytes);
    }
    if required_bytes < 2 {
        return Err(io::Error::last_os_error());
    }
    let mut name = vec![0u16; (required_bytes as usize).div_ceil(size_of::<u16>())];
    if unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            (name.len() * size_of::<u16>()) as u32,
            &mut required_bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let end = name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(name.len());
    String::from_utf16(&name[..end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "window-station name is invalid"))
}

fn current_window_station_name() -> io::Result<String> {
    user_object_name(unsafe { GetProcessWindowStation() })
}

fn current_desktop_startup_path() -> io::Result<Vec<u16>> {
    let station = current_window_station_name()?;
    let desktop = user_object_name(unsafe { GetThreadDesktop(GetCurrentThreadId()) })?;
    Ok(format!("{station}\\{desktop}")
        .encode_utf16()
        .chain([0])
        .collect())
}

/// A desktop created exclusively for a non-interactive worker. The startup
/// path and the HDESK stay alive until the process/job and pipe readers have
/// finished; closing the handle immediately after CreateProcess would make
/// later UI calls fail nondeterministically.
struct WorkerDesktop {
    // `None` represents the caller's existing desktop. It is not owned by the
    // broker and must never be closed here.
    handle: Option<HDESK>,
    startup_path: Vec<u16>,
}

impl WorkerDesktop {
    fn create(worker_sid: Option<&RestrictedWorkerSid>) -> io::Result<Self> {
        let station = current_window_station_name()?;
        let desktop_name = format!("AEXCompatWorkerDesktop-{:032x}", rand::random::<u128>());
        let desktop_text: Vec<u16> = desktop_name.encode_utf16().chain([0]).collect();
        let startup_path: Vec<u16> = format!("{station}\\{desktop_name}")
            .encode_utf16()
            .chain([0])
            .collect();
        let descriptor = DesktopSecurityDescriptor::new(worker_sid)?;
        let security = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.raw(),
            bInheritHandle: 0,
        };
        let handle = unsafe {
            CreateDesktopW(
                desktop_text.as_ptr(),
                null(),
                null(),
                0,
                DESKTOP_WORKER_ACCESS,
                &security,
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            handle: Some(handle),
            startup_path,
        })
    }

    fn current() -> io::Result<Self> {
        Ok(Self {
            handle: None,
            startup_path: current_desktop_startup_path()?,
        })
    }

    fn startup_path(&mut self) -> *mut u16 {
        self.startup_path.as_mut_ptr()
    }
}

impl Drop for WorkerDesktop {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                CloseDesktop(handle);
            }
        }
    }
}

pub struct ProcessResult {
    pub classification: ExitClassification,
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// Why the worker was killed, when that is knowable: "timeout" when the
    /// broker terminated the job on deadline, "memory_limit" when a non-ok
    /// exit coincides with the launched worker's own peak commit having
    /// reached the Job Object memory cap (allocations were failing). None
    /// otherwise.
    pub kill_reason: Option<&'static str>,
    /// Peak commit of the launched worker process itself, from
    /// GetProcessMemoryInfo on its handle. This is the value kill-reason
    /// detection uses; a descendant blowing the cap does not implicate the
    /// worker. None when the query itself failed.
    pub worker_peak_commit_bytes: Option<u64>,
    /// Peak commit of any single process ever associated with the job
    /// (worker or descendant), from Job Object accounting.
    pub peak_process_memory_bytes: Option<u64>,
    /// Peak committed memory of the whole job (worker plus descendants).
    pub peak_job_memory_bytes: Option<u64>,
    /// The per-process commit cap the job enforces, for context.
    pub process_memory_limit_bytes: u64,
    /// True when the worker's own peak commit reached the cap, meaning
    /// allocations beyond it were failing inside the worker.
    pub memory_limit_reached: bool,
    /// Windows the worker put on its private desktop, which the broker closed
    /// on its behalf (issue #351). Normally empty. An entry with `closed:
    /// false` is the one that matters: the window ignored `WM_CLOSE`, so the
    /// worker is still waiting on something nobody can answer.
    pub dismissed_windows: Vec<crate::worker_dialog::DismissedWindow>,
}

pub fn run_sentinel_check(program: &Path, timeout: Duration) -> io::Result<ProcessResult> {
    let mut security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let sentinel = OwnedHandle::new(unsafe { CreateEventW(&mut security, 0, 0, null()) })?;
    let args = vec![
        "--sentinel-handle".to_string(),
        (sentinel.raw() as usize).to_string(),
    ];
    run_isolated(program, &args, timeout)
}

struct OwnedHandle(HANDLE);
impl OwnedHandle {
    fn new(value: HANDLE) -> io::Result<Self> {
        if value.is_null() || value == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(value))
        }
    }
    fn raw(&self) -> HANDLE {
        self.0
    }
    fn take(mut self) -> HANDLE {
        let value = self.0;
        self.0 = null_mut();
        value
    }
}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read: HANDLE = null_mut();
    let mut write: HANDLE = null_mut();
    let mut security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    if unsafe { CreatePipe(&mut read, &mut write, &mut security, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = OwnedHandle::new(read)?;
    let write = OwnedHandle::new(write)?;
    if unsafe { SetHandleInformation(read.raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}

fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;
    for character in value.chars() {
        if character == '\\' {
            backslashes += 1;
        } else {
            if character == '"' {
                quoted.extend(std::iter::repeat_n('\\', backslashes + 1));
            }
            quoted.extend(std::iter::repeat_n('\\', backslashes));
            backslashes = 0;
            quoted.push(character);
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// Environment variables that advertise inherited render-session handle
/// numbers to the worker (docs/RENDER_SESSION_PROTOCOL_2026-07-19.md §2).
pub const SESSION_REQUEST_HANDLE_VARIABLE: &str = "AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE";
pub const SESSION_RESPONSE_HANDLE_VARIABLE: &str = "AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE";
pub const SESSION_SECTION_HANDLE_VARIABLE: &str = "AEXCOMPAT_RENDER_SESSION_SECTION_HANDLE";

/// Child-side transport handles for a resident render session launch. Every
/// handle must already be inheritable; the caller keeps ownership and closes
/// its copies after the launch. The worker receives the three transport handle
/// numbers via the session environment variables and the per-layer read handle
/// numbers via the `session-layers` launch trailer, and never opens a path for
/// transport (#268).
pub struct SessionChildHandles {
    pub request_read: HANDLE,
    pub response_write: HANDLE,
    pub section: HANDLE,
    /// Per-layer RGBA8 read handles (#268), one per session layer in launch
    /// order. Inherited so their numeric values match in the worker; the trailer
    /// pairs each value with its layer's slot/geometry.
    pub layers: Vec<HANDLE>,
}

fn child_environment(
    trace_handle: Option<HANDLE>,
    minidump_handle: Option<HANDLE>,
    minidump_ack_handle: Option<HANDLE>,
    session: Option<&SessionChildHandles>,
) -> Vec<u16> {
    let mut entries: Vec<(String, std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let normalized = key.to_string_lossy().to_ascii_uppercase();
            if normalized == "AEX_INSTRUMENT_TRACE_DIR"
                || normalized == "AEX_INSTRUMENT_TRACE_HANDLE"
                || normalized == "AEXCOMPAT_MINIDUMP_DIR"
                || normalized == "AEXCOMPAT_MINIDUMP_HANDLE"
                || normalized == "AEXCOMPAT_MINIDUMP_ACK_HANDLE"
                || normalized == SESSION_REQUEST_HANDLE_VARIABLE
                || normalized == SESSION_RESPONSE_HANDLE_VARIABLE
                || normalized == SESSION_SECTION_HANDLE_VARIABLE
            {
                None
            } else {
                Some((normalized, key, value))
            }
        })
        .collect();
    if let Some(handle) = trace_handle {
        entries.push((
            "AEX_INSTRUMENT_TRACE_HANDLE".into(),
            "AEX_INSTRUMENT_TRACE_HANDLE".into(),
            (handle as usize).to_string().into(),
        ));
    }
    if let Some(handle) = minidump_handle {
        entries.push((
            "AEXCOMPAT_MINIDUMP_HANDLE".into(),
            "AEXCOMPAT_MINIDUMP_HANDLE".into(),
            (handle as usize).to_string().into(),
        ));
    }
    if let Some(handle) = minidump_ack_handle {
        entries.push((
            "AEXCOMPAT_MINIDUMP_ACK_HANDLE".into(),
            "AEXCOMPAT_MINIDUMP_ACK_HANDLE".into(),
            (handle as usize).to_string().into(),
        ));
    }
    if let Some(session) = session {
        for (name, handle) in [
            (SESSION_REQUEST_HANDLE_VARIABLE, session.request_read),
            (SESSION_RESPONSE_HANDLE_VARIABLE, session.response_write),
            (SESSION_SECTION_HANDLE_VARIABLE, session.section),
        ] {
            entries.push((
                name.into(),
                name.into(),
                (handle as usize).to_string().into(),
            ));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut block = Vec::new();
    for (_, key, value) in entries {
        block.extend(key.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn reader(
    handle_value: usize,
    capture_limit: usize,
) -> thread::JoinHandle<io::Result<(String, bool)>> {
    thread::spawn(move || {
        let handle = handle_value as HANDLE;
        let handle = OwnedHandle::new(handle)?;
        let mut collected = Vec::new();
        let mut truncated = false;
        loop {
            let mut buffer = [0u8; 4096];
            let mut read = 0;
            let ok = unsafe {
                ReadFile(
                    handle.raw(),
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut read,
                    null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                break;
            }
            let available = capture_limit.saturating_sub(collected.len());
            let take = available.min(read as usize);
            collected.extend_from_slice(&buffer[..take]);
            truncated |= take < read as usize;
        }
        let text = String::from_utf8_lossy(&collected);
        let (redacted, redaction_truncated) = redact_windows_paths(&text, capture_limit);
        Ok((redacted, truncated || redaction_truncated))
    })
}

/// Job Object accounting survives worker exit for as long as the job handle
/// is open, so this can run after the process is gone. A failed query yields
/// (None, None) rather than failing the whole run; the peaks are diagnostics,
/// not a contract.
fn query_job_memory_peaks(job: HANDLE) -> (Option<u64>, Option<u64>) {
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        )
    };
    if ok == 0 {
        (None, None)
    } else {
        (
            Some(info.PeakProcessMemoryUsed as u64),
            Some(info.PeakJobMemoryUsed as u64),
        )
    }
}

/// Peak commit of one specific process, queryable after exit while a handle
/// stays open. Unlike the job accounting, this cannot be inflated by
/// descendants.
fn query_worker_peak_commit(process: HANDLE) -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            process,
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (ok != 0).then_some(counters.PeakPagefileUsage as u64)
}

fn terminate_job_and_wait(job: HANDLE, process: HANDLE, wait_ms: u32) -> io::Result<()> {
    if unsafe { TerminateJobObject(job, 0xDEAD) } == 0 {
        return Err(io::Error::last_os_error());
    }

    match unsafe { WaitForSingleObject(process, wait_ms) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "process did not exit after its job was terminated",
        )),
        _ => Err(io::Error::last_os_error()),
    }
}

pub fn run_isolated(
    program: &Path,
    args: &[String],
    timeout: Duration,
) -> io::Result<ProcessResult> {
    run_isolated_impl(
        program,
        args,
        timeout,
        None,
        None,
        WorkerDesktopPolicy::Dedicated,
        None,
    )
}

pub fn run_isolated_with_restricted_token(
    program: &Path,
    args: &[String],
    timeout: Duration,
    token: &RestrictedWorkerToken,
    current_directory: &Path,
    repository: &Path,
) -> io::Result<ProcessResult> {
    run_isolated_impl(
        program,
        args,
        timeout,
        Some((token.as_raw_handle(), current_directory)),
        Some(token.worker_sid()),
        WorkerDesktopPolicy::Dedicated,
        Some(repository),
    )
}

fn run_isolated_impl(
    program: &Path,
    args: &[String],
    timeout: Duration,
    token: Option<(HANDLE, &Path)>,
    worker_sid: Option<&RestrictedWorkerSid>,
    desktop_policy: WorkerDesktopPolicy,
    repository: Option<&Path>,
) -> io::Result<ProcessResult> {
    launch_isolated_impl(
        program,
        args,
        token,
        None,
        worker_sid,
        desktop_policy,
        repository,
    )?
    .wait_and_collect(timeout)
}

/// A resumed isolated worker whose exit has not been awaited yet. One-shot
/// dispatch awaits it immediately (`run_isolated_impl`); a render session
/// keeps it alive across the frame loop and collects it at close. Dropping
/// this without collecting terminates the worker through the job's
/// kill-on-close limit.
pub struct LaunchedIsolatedProcess {
    /// Present only for a private desktop: the interactive desktop is never
    /// swept, so a GUI harness worker's windows are left exactly as they are.
    ///
    /// Declared first so it is also dropped first. The sweep thread reads the
    /// desktop and job handles below, and a launch dropped instead of
    /// collected — how a session kills its worker — would otherwise close both
    /// while the thread was still between polls. Windows hands handle values
    /// out again, so that is not merely a failed call.
    dialog_sweep: Option<crate::worker_dialog::DialogSweep>,
    process: OwnedHandle,
    job: OwnedHandle,
    desktop: Option<WorkerDesktop>,
    stdout_reader: thread::JoinHandle<io::Result<(String, bool)>>,
    stderr_reader: thread::JoinHandle<io::Result<(String, bool)>>,
    // Opt-in crash minidump (issue #18). The broker keeps the pipe read side
    // and its reader thread alive here until the worker exits; dropping this
    // after `wait_and_collect` finalizes the capture into a `.dmp`. `None` for
    // the render-session path and whenever no repository was supplied.
    minidump_file: Option<crate::minidump_policy::MinidumpLaunchFile>,
}

impl LaunchedIsolatedProcess {
    /// Terminates the whole job now (session watchdog path) and waits for the
    /// worker process to disappear. Collection still happens via
    /// `wait_and_collect`, which then returns immediately.
    pub fn terminate_job(&self) -> io::Result<()> {
        terminate_job_and_wait(self.job.raw(), self.process.raw(), TERMINATION_GRACE_MS)
    }

    /// Synchronous liveness check on the worker process itself, for the
    /// session close handshake: the async watcher event may not have been
    /// delivered yet when close() needs to know whether the worker already
    /// exited (a descendant can keep the request pipe writable after the
    /// worker died, so a successful close write proves nothing).
    pub fn has_exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.raw(), 0) == WAIT_OBJECT_0 }
    }

    /// A duplicated handle to the worker process itself, for a session's
    /// process-death watcher (protocol §7: the frame wait observes the
    /// response channel, the deadline, AND process death — a descendant
    /// holding the inherited response pipe must not mask a dead worker).
    /// The caller owns the duplicate and must close it.
    pub fn duplicated_process_handle(&self) -> io::Result<usize> {
        use windows_sys::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut duplicated: HANDLE = null_mut();
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.process.raw(),
                GetCurrentProcess(),
                &mut duplicated,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(duplicated as usize)
    }

    /// Waits up to `timeout` for the worker to exit (terminating the job on
    /// deadline, exactly like the one-shot path), then collects output and
    /// job accounting into a `ProcessResult`.
    pub fn wait_and_collect(self, timeout: Duration) -> io::Result<ProcessResult> {
        let LaunchedIsolatedProcess {
            dialog_sweep,
            process: process_handle,
            job,
            desktop,
            stdout_reader,
            stderr_reader,
            minidump_file,
        } = self;
        let wait = unsafe {
            WaitForSingleObject(
                process_handle.raw(),
                timeout.as_millis().min(u32::MAX as u128) as u32,
            )
        };
        let timed_out = wait == WAIT_TIMEOUT;
        if timed_out {
            terminate_job_and_wait(job.raw(), process_handle.raw(), TERMINATION_GRACE_MS)?;
        } else if wait != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut exit_code = 0;
        if unsafe { GetExitCodeProcess(process_handle.raw(), &mut exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // A plug-in or GPU driver may leave descendants holding inherited pipe
        // handles. End the job before joining readers so capture cannot wait
        // for an unrelated descendant after the worker itself has exited.
        if !timed_out && unsafe { TerminateJobObject(job.raw(), exit_code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let (stdout, stdout_truncated) = stdout_reader
            .join()
            .map_err(|_| io::Error::other("stdout reader panicked"))??;
        let (stderr, stderr_truncated) = stderr_reader
            .join()
            .map_err(|_| io::Error::other("stderr reader panicked"))??;
        // The worker and its job are gone, so any crash minidump the worker
        // wrote is fully buffered in the pipe. Drop the retained file now to
        // drain the reader and finalize the `.dmp` (no-op when opt-in was off).
        drop(minidump_file);
        let (peak_process_memory_bytes, peak_job_memory_bytes) = query_job_memory_peaks(job.raw());
        let worker_peak_commit_bytes = query_worker_peak_commit(process_handle.raw());
        // Ended after the worker is gone so the last sweep sees whether the
        // windows it closed actually went away, and before the desktop handle
        // is released so it is never enumerated after being closed.
        let dismissed_windows = dialog_sweep.map(|sweep| sweep.finish()).unwrap_or_default();
        // A worker that finished got past whatever was on its desktop, so only
        // one that did not is worth warning about.
        let worker_finished = !timed_out && exit_code == 0;
        for window in crate::worker_dialog::still_blocking(&dismissed_windows) {
            if worker_finished {
                break;
            }
            tracing::warn!(
                class = %window.class,
                title = %window.title,
                asked_to_close = window.asked_to_close,
                "a worker window may have held the worker up"
            );
        }
        // Release the desktop only after the worker, job, readers, and
        // diagnostics have all been collected. The object is intentionally
        // not part of the inherited handle list; lpDesktop names it.
        drop(desktop);
        let classification = classify_exit(exit_code, timed_out);
        // The hard commit cap rejects the allocation that would cross it, so the
        // recorded peak stops short of the limit by up to one failed request.
        // Treat "peaked within the slack below the cap" as having hit it; this is
        // a heuristic marker, not proof, and it only escalates to a kill reason
        // when the worker also died. The check uses the worker's own peak, not
        // the job aggregate, so a descendant blowing the cap does not implicate
        // the worker.
        let memory_limit_reached = worker_peak_commit_bytes
            .is_some_and(|peak| peak >= PROCESS_MEMORY_LIMIT as u64 - MEMORY_LIMIT_DETECTION_SLACK);
        let kill_reason = if timed_out {
            Some("timeout")
        } else if classification != ExitClassification::Ok && memory_limit_reached {
            // The job's hard commit cap makes allocations fail rather than
            // killing the process, so a non-ok exit at the cap is the observable
            // form of an out-of-memory death.
            Some("memory_limit")
        } else {
            None
        };
        tracing::debug!(
            classification = classification.as_str(),
            exit_code = format_args!("{exit_code:#010x}"),
            timed_out,
            kill_reason,
            stdout_bytes = stdout.len(),
            stderr_bytes = stderr.len(),
            stdout_truncated,
            stderr_truncated,
            "worker exited"
        );
        if stdout_truncated || stderr_truncated {
            tracing::warn!(
                stdout_truncated,
                stderr_truncated,
                "worker output truncated at capture limit"
            );
        }
        Ok(ProcessResult {
            classification,
            exit_code,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            kill_reason,
            worker_peak_commit_bytes,
            peak_process_memory_bytes,
            peak_job_memory_bytes,
            process_memory_limit_bytes: PROCESS_MEMORY_LIMIT as u64,
            memory_limit_reached,
            dismissed_windows,
        })
    }
}

/// Session launch: same isolation shape as `run_isolated_with_restricted_token`
/// (restricted token, kill-on-close Job Object, memory cap, handle-list
/// inheritance), plus the three session transport handles inherited and
/// advertised via environment variables. The caller drives the frame loop and
/// collects the exit through the returned `LaunchedIsolatedProcess`.
///
/// `repository` feeds the opt-in crash minidump handle (issue #18/#224): the
/// resident worker gets the same broker-created inherited dump pipe as the
/// one-shot path, retained across the frame loop on the returned process and
/// finalized when the caller collects it at close (`wait_and_collect`).
pub fn launch_isolated_session_with_restricted_token(
    program: &Path,
    args: &[String],
    token: &RestrictedWorkerToken,
    current_directory: &Path,
    session: &SessionChildHandles,
    repository: &Path,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_session_with_desktop_policy(
        program,
        args,
        token,
        current_directory,
        session,
        WorkerDesktopPolicy::Dedicated,
        repository,
    )
}

pub(crate) fn launch_isolated_session_on_current_desktop(
    program: &Path,
    args: &[String],
    token: &RestrictedWorkerToken,
    current_directory: &Path,
    session: &SessionChildHandles,
    repository: &Path,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_session_with_desktop_policy(
        program,
        args,
        token,
        current_directory,
        session,
        WorkerDesktopPolicy::Current,
        repository,
    )
}

fn launch_isolated_session_with_desktop_policy(
    program: &Path,
    args: &[String],
    token: &RestrictedWorkerToken,
    current_directory: &Path,
    session: &SessionChildHandles,
    desktop_policy: WorkerDesktopPolicy,
    repository: &Path,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_impl(
        program,
        args,
        Some((token.as_raw_handle(), current_directory)),
        Some(session),
        Some(token.worker_sid()),
        desktop_policy,
        Some(repository),
    )
}

fn launch_isolated_impl(
    program: &Path,
    args: &[String],
    token: Option<(HANDLE, &Path)>,
    session: Option<&SessionChildHandles>,
    worker_sid: Option<&RestrictedWorkerSid>,
    desktop_policy: WorkerDesktopPolicy,
    repository: Option<&Path>,
) -> io::Result<LaunchedIsolatedProcess> {
    let trace_file = crate::trace_policy::create_trace_file_for_launch()?;
    let mut minidump_file = repository
        .map(crate::minidump_policy::create_minidump_file_for_launch)
        .transpose()?
        .flatten();
    // Capture the opt-in feature flags before the handles are dropped below so
    // the launch trace can report them. Never log the program path: it may be a
    // private absolute path, which must not escape into diagnostics.
    let trace_active = trace_file.is_some();
    let minidump_active = minidump_file.is_some();
    let session_active = session.is_some();
    let restricted_token = token.is_some();
    let mut desktop = match desktop_policy {
        WorkerDesktopPolicy::Dedicated => Some(WorkerDesktop::create(worker_sid)?),
        WorkerDesktopPolicy::Current => Some(WorkerDesktop::current()?),
    };
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.ProcessMemoryLimit = PROCESS_MEMORY_LIMIT;
    if unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const c_void,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    let mut attribute_size = 0usize;
    unsafe {
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_size);
    }
    let words = (attribute_size + size_of::<usize>() - 1) / size_of::<usize>();
    let mut attribute_storage = vec![0usize; words];
    let attribute_list = attribute_storage.as_mut_ptr().cast();
    if unsafe { InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_size) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    struct AttributeGuard(*mut c_void);
    impl Drop for AttributeGuard {
        fn drop(&mut self) {
            unsafe {
                DeleteProcThreadAttributeList(self.0);
            }
        }
    }
    let _attribute_guard = AttributeGuard(attribute_list);
    let mut inherited = vec![stdout_write.raw(), stderr_write.raw()];
    if let Some(trace_file) = trace_file.as_ref() {
        inherited.push(trace_file.raw());
    }
    if let Some(minidump_file) = minidump_file.as_ref() {
        inherited.push(minidump_file.raw());
        inherited.push(minidump_file.ack_raw());
    }
    if let Some(session) = session {
        inherited.extend([
            session.request_read,
            session.response_write,
            session.section,
        ]);
        // Per-layer RGBA8 read handles (#268) inherit alongside the transport
        // handles; their numeric values reach the worker via the session-layers
        // trailer.
        inherited.extend(session.layers.iter().copied());
    }
    if unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_mut_ptr().cast(),
            std::mem::size_of_val(inherited.as_slice()),
            null_mut(),
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    let program_text = program.as_os_str().to_string_lossy();
    let mut command = quote(&program_text);
    for arg in args {
        command.push(' ');
        command.push_str(&quote(arg));
    }
    let mut command_wide: Vec<u16> = std::ffi::OsStr::new(&command)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let application_wide: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut environment = child_environment(
        trace_file.as_ref().map(|file| file.raw()),
        minidump_file.as_ref().map(|file| file.raw()),
        minidump_file.as_ref().map(|file| file.ack_raw()),
        session,
    );
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = stdout_write.raw();
    startup.StartupInfo.hStdError = stderr_write.raw();
    startup.StartupInfo.hStdInput = null_mut();
    if let Some(desktop) = desktop.as_mut() {
        startup.StartupInfo.lpDesktop = desktop.startup_path();
    }
    startup.lpAttributeList = attribute_list;
    let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
    let creation_flags = EXTENDED_STARTUPINFO_PRESENT
        | CREATE_SUSPENDED
        | CREATE_NO_WINDOW
        | CREATE_UNICODE_ENVIRONMENT;
    let created = unsafe {
        match token {
            Some((token, current_directory)) => {
                let current_directory_wide: Vec<u16> = current_directory
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect();
                CreateProcessAsUserW(
                    token,
                    application_wide.as_ptr(),
                    command_wide.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    creation_flags,
                    environment.as_mut_ptr().cast(),
                    current_directory_wide.as_ptr(),
                    &startup.StartupInfo,
                    &mut process,
                )
            }
            None => CreateProcessW(
                null(),
                command_wide.as_mut_ptr(),
                null(),
                null(),
                1,
                creation_flags,
                environment.as_mut_ptr().cast(),
                null(),
                &startup.StartupInfo,
                &mut process,
            ),
        }
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    let process_handle = OwnedHandle::new(process.hProcess)?;
    let thread_handle = OwnedHandle::new(process.hThread)?;
    let mut suspended_cleanup = SuspendedProcessCleanup::new(process_handle.raw(), job.raw());
    if unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    suspended_cleanup.assigned_to_job = true;
    if unsafe { ResumeThread(thread_handle.raw()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    suspended_cleanup.disarm();
    tracing::debug!(
        worker = %program
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        process_id = process.dwProcessId,
        args = args.len(),
        trace_active,
        minidump_active,
        session_active,
        restricted_token,
        "worker launched"
    );
    drop(thread_handle);
    drop(stdout_write);
    drop(stderr_write);
    drop(trace_file);
    // The child holds its own inherited copies now. Close the broker's worker
    // copies (pipe write side + ack) so the reader observes EOF when the worker
    // exits; the broker keeps the read side and reader thread alive inside the
    // retained `minidump_file` until `wait_and_collect` drops it.
    if let Some(minidump_file) = minidump_file.as_mut() {
        minidump_file.close_worker_handles();
    }
    let stdout_reader = reader(stdout_read.take() as usize, STDOUT_CAPTURE_LIMIT);
    let stderr_reader = reader(stderr_read.take() as usize, STDERR_CAPTURE_LIMIT);
    // Only a desktop the broker created is swept. `None` here means the
    // caller asked for its own desktop (the GUI harness path), where closing a
    // window would be closing the user's.
    let dialog_sweep = desktop
        .as_ref()
        .and_then(|desktop| desktop.handle)
        .map(|handle| {
            crate::worker_dialog::DialogSweep::start(handle, job.raw(), process_handle.raw())
        });
    Ok(LaunchedIsolatedProcess {
        dialog_sweep,
        process: process_handle,
        job,
        desktop,
        stdout_reader,
        stderr_reader,
        minidump_file,
    })
}

struct SuspendedProcessCleanup {
    process: HANDLE,
    job: HANDLE,
    assigned_to_job: bool,
    armed: bool,
}

impl SuspendedProcessCleanup {
    fn new(process: HANDLE, job: HANDLE) -> Self {
        Self {
            process,
            job,
            assigned_to_job: false,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SuspendedProcessCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        unsafe {
            if self.assigned_to_job {
                TerminateJobObject(self.job, 0xDEAD);
            } else {
                TerminateProcess(self.process, 0xDEAD);
            }
            WaitForSingleObject(self.process, TERMINATION_GRACE_MS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_cleanup_reports_job_termination_failure_without_waiting() {
        let error = terminate_job_and_wait(null_mut(), null_mut(), u32::MAX).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(6)); // ERROR_INVALID_HANDLE
    }

    #[test]
    fn timeout_cleanup_uses_a_bounded_process_wait() {
        let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) }).unwrap();
        let process = OwnedHandle::new(unsafe { CreateEventW(null(), 0, 0, null()) }).unwrap();

        let error = terminate_job_and_wait(job.raw(), process.raw(), 0).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
