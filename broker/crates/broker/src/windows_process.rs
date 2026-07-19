use crate::restricted_worker_token::RestrictedWorkerToken;
use crate::{classify_exit, redact_windows_paths, ExitClassification};
use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessAsUserW, CreateProcessW, DeleteProcThreadAttributeList,
    GetExitCodeProcess, InitializeProcThreadAttributeList, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
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

/// Child-side transport handles for a resident render session launch. All
/// three must already be inheritable; the caller keeps ownership and closes
/// its copies after the launch. The worker receives only these numbers via
/// the session environment variables and never opens a path for transport.
pub struct SessionChildHandles {
    pub request_read: HANDLE,
    pub response_write: HANDLE,
    pub section: HANDLE,
}

fn child_environment(trace_handle: Option<HANDLE>, session: Option<&SessionChildHandles>) -> Vec<u16> {
    let mut entries: Vec<(String, std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let normalized = key.to_string_lossy().to_ascii_uppercase();
            if normalized == "AEX_INSTRUMENT_TRACE_DIR"
                || normalized == "AEX_INSTRUMENT_TRACE_HANDLE"
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
    run_isolated_impl(program, args, timeout, None)
}

pub fn run_isolated_with_restricted_token(
    program: &Path,
    args: &[String],
    timeout: Duration,
    token: &RestrictedWorkerToken,
    current_directory: &Path,
) -> io::Result<ProcessResult> {
    run_isolated_impl(
        program,
        args,
        timeout,
        Some((token.as_raw_handle(), current_directory)),
    )
}

fn run_isolated_impl(
    program: &Path,
    args: &[String],
    timeout: Duration,
    token: Option<(HANDLE, &Path)>,
) -> io::Result<ProcessResult> {
    launch_isolated_impl(program, args, token, None)?.wait_and_collect(timeout)
}

/// A resumed isolated worker whose exit has not been awaited yet. One-shot
/// dispatch awaits it immediately (`run_isolated_impl`); a render session
/// keeps it alive across the frame loop and collects it at close. Dropping
/// this without collecting terminates the worker through the job's
/// kill-on-close limit.
pub struct LaunchedIsolatedProcess {
    process: OwnedHandle,
    job: OwnedHandle,
    stdout_reader: thread::JoinHandle<io::Result<(String, bool)>>,
    stderr_reader: thread::JoinHandle<io::Result<(String, bool)>>,
}

impl LaunchedIsolatedProcess {
    /// Terminates the whole job now (session watchdog path) and waits for the
    /// worker process to disappear. Collection still happens via
    /// `wait_and_collect`, which then returns immediately.
    pub fn terminate_job(&self) -> io::Result<()> {
        terminate_job_and_wait(self.job.raw(), self.process.raw(), TERMINATION_GRACE_MS)
    }

    /// A duplicated handle to the worker process itself, for a session's
    /// process-death watcher (protocol §7: the frame wait observes the
    /// response channel, the deadline, AND process death — a descendant
    /// holding the inherited response pipe must not mask a dead worker).
    /// The caller owns the duplicate and must close it.
    pub fn duplicated_process_handle(&self) -> io::Result<usize> {
        use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS};
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
            process: process_handle,
            job,
            stdout_reader,
            stderr_reader,
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
        let (peak_process_memory_bytes, peak_job_memory_bytes) = query_job_memory_peaks(job.raw());
        let worker_peak_commit_bytes = query_worker_peak_commit(process_handle.raw());
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
        })
    }
}

/// Session launch: same isolation shape as `run_isolated_with_restricted_token`
/// (restricted token, kill-on-close Job Object, memory cap, handle-list
/// inheritance), plus the three session transport handles inherited and
/// advertised via environment variables. The caller drives the frame loop and
/// collects the exit through the returned `LaunchedIsolatedProcess`.
pub fn launch_isolated_session_with_restricted_token(
    program: &Path,
    args: &[String],
    token: &RestrictedWorkerToken,
    current_directory: &Path,
    session: &SessionChildHandles,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_impl(
        program,
        args,
        Some((token.as_raw_handle(), current_directory)),
        Some(session),
    )
}

fn launch_isolated_impl(
    program: &Path,
    args: &[String],
    token: Option<(HANDLE, &Path)>,
    session: Option<&SessionChildHandles>,
) -> io::Result<LaunchedIsolatedProcess> {
    let trace_file = crate::trace_policy::create_trace_file_for_launch()?;
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
    if let Some(session) = session {
        inherited.extend([
            session.request_read,
            session.response_write,
            session.section,
        ]);
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
    let mut environment = child_environment(trace_file.as_ref().map(|file| file.raw()), session);
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = stdout_write.raw();
    startup.StartupInfo.hStdError = stderr_write.raw();
    startup.StartupInfo.hStdInput = null_mut();
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
    drop(thread_handle);
    drop(stdout_write);
    drop(stderr_write);
    drop(trace_file);
    let stdout_reader = reader(stdout_read.take() as usize, STDOUT_CAPTURE_LIMIT);
    let stderr_reader = reader(stderr_read.take() as usize, STDERR_CAPTURE_LIMIT);
    Ok(LaunchedIsolatedProcess {
        process: process_handle,
        job,
        stdout_reader,
        stderr_reader,
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
