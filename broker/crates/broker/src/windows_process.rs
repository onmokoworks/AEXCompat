use crate::secure_launch::LaunchEnvironment;
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
    CreateDesktopW, GetProcessWindowStation, GetThreadDesktop, GetUserObjectInformationW, HDESK,
    UOI_NAME,
};
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateEventW, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentThreadId,
    GetExitCodeProcess, InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

// Native render stdout is one bounded JSON report. Its 65,536 Suite events
// can occupy more than 16 MiB after escaping 96-byte safely copied names.
// Keep enough for the worker contract while retaining a hard memory bound.
pub const STDOUT_CAPTURE_LIMIT: usize = 24 * 1024 * 1024;
const STDERR_CAPTURE_LIMIT: usize = 64 * 1024;
const PROCESS_MEMORY_LIMIT: usize = 512 * 1024 * 1024;
// Resident render sessions carry the plug-in, its inference/runtime closure,
// and one or more frame worlds at the same time. Keep that path bounded while
// allowing models whose measured working set legitimately exceeds the
// one-shot/discovery budget (issues #1138 and #1142).
const RENDER_SESSION_PROCESS_MEMORY_LIMIT: usize = 2 * 1024 * 1024 * 1024;
const MAX_PROBE_PROCESS_MEMORY_LIMIT: usize = 2 * 1024 * 1024 * 1024;
const TERMINATION_GRACE_MS: u32 = 5_000;
// See memory_limit_reached: the largest single failed allocation the
// detection tolerates between the recorded peak and the cap.
const MEMORY_LIMIT_DETECTION_SLACK: u64 = 16 * 1024 * 1024;
const DESKTOP_WORKER_ACCESS: u32 = 0x0000_01ff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerDesktopPolicy {
    /// Non-interactive discovery/render workers run on a private desktop so a
    /// modal UI cannot appear on the user's input desktop. All such workers
    /// share one desktop for the life of the broker process (issue #1194):
    /// Windows leaks DWM composition state on every desktop create/destroy
    /// cycle, so per-worker desktops progressively degraded the interactive
    /// session over a long sweep. The isolation property is "not the user's
    /// desktop", which sharing preserves; window attribution stays per-worker
    /// because the dialog sweep tests Job Object membership, not the desktop.
    ///
    /// What sharing does concede: concurrent workers see each other's windows
    /// and can post to them or set desktop-scoped hooks. That was already
    /// reachable before — the same-token worker could open a sibling's
    /// desktop by name, and the DACL's owner ACE granted it access — so this
    /// is a default, not a new privilege, and the worker was never a
    /// confidentiality boundary. It does mean a hostile plug-in running
    /// concurrently can perturb another worker's UI-related diagnostics.
    Dedicated,
    /// Explicit GUI harnesses retain the caller's current desktop.
    Current,
}

struct DesktopSecurityDescriptor(std::ptr::NonNull<std::ffi::c_void>);

impl DesktopSecurityDescriptor {
    fn new() -> io::Result<Self> {
        // Keep the desktop private to the broker's window-station owner and
        // SYSTEM. The worker runs under the same user token, so the owner ACE
        // is what grants it the rights to create and operate UI objects. The
        // protected DACL prevents the parent station ACL from being inherited
        // into this boundary.
        let sddl = "D:P(A;;GA;;;SY)(A;;GA;;;OW)".to_owned();
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

/// The one private desktop every [`WorkerDesktopPolicy::Dedicated`] worker
/// shares, created on first use and owned by the broker process for its whole
/// lifetime (issue #1194). Creating a desktop per worker asked Windows for a
/// create/destroy cycle per launch, and DWM leaks composition state on every
/// such cycle on the OS side, so a long sweep degraded the interactive
/// session until `dwm.exe` was restarted. One desktop per broker keeps the
/// number of cycles independent of worker count.
///
/// Teardown is process exit: the kernel closes the HDESK when the broker
/// terminates, normally or not, and the kill-on-close Job Objects guarantee
/// no worker outlives the broker to keep the desktop object alive. Nothing
/// else ever closes this handle, so the sweep threads and `lpDesktop` may
/// read it at any point in the process's life.
struct SharedWorkerDesktop {
    handle: HDESK,
    startup_path: Vec<u16>,
}

// The HDESK is a process-wide kernel handle, only ever read after creation,
// and never closed before process exit; the path is immutable after creation.
unsafe impl Send for SharedWorkerDesktop {}
unsafe impl Sync for SharedWorkerDesktop {}

impl SharedWorkerDesktop {
    fn create() -> io::Result<Self> {
        let station = current_window_station_name()?;
        // Random so concurrent broker processes on one window station cannot
        // collide (a name collision would silently share across brokers with
        // whatever DACL the first one applied).
        let desktop_name = format!("AEXCompatWorkerDesktop-{:032x}", rand::random::<u128>());
        let desktop_text: Vec<u16> = desktop_name.encode_utf16().chain([0]).collect();
        let startup_path: Vec<u16> = format!("{station}\\{desktop_name}")
            .encode_utf16()
            .chain([0])
            .collect();
        let descriptor = DesktopSecurityDescriptor::new()?;
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
            handle,
            startup_path,
        })
    }
}

/// The shared worker desktop, created on the first dedicated launch. A failed
/// creation is returned to that launch and not cached, so a transient failure
/// does not condemn every later launch; the lock keeps a racing first launch
/// from creating a second desktop whose handle would then leak unclosed.
fn shared_worker_desktop() -> io::Result<&'static SharedWorkerDesktop> {
    static DESKTOP: std::sync::OnceLock<SharedWorkerDesktop> = std::sync::OnceLock::new();
    static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if let Some(desktop) = DESKTOP.get() {
        return Ok(desktop);
    }
    let _guard = INIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(desktop) = DESKTOP.get() {
        return Ok(desktop);
    }
    let created = SharedWorkerDesktop::create()?;
    // `set`, not `get_or_init`: initialization is serialized by `INIT` and
    // re-checked above, so a second initializer cannot exist, and `set` makes
    // a violation a panic instead of a silently leaked desktop handle.
    if DESKTOP.set(created).is_err() {
        unreachable!("the shared worker desktop was initialized twice");
    }
    Ok(DESKTOP.get().expect("set above"))
}

/// One launch's view of the desktop its worker starts on. Owns nothing: the
/// dedicated desktop belongs to the process ([`SharedWorkerDesktop`]) and the
/// caller's desktop was never the broker's to close. The startup path is a
/// per-launch copy because `lpDesktop` wants a mutable pointer.
struct WorkerDesktop {
    // `None` represents the caller's existing desktop, which is never swept.
    handle: Option<HDESK>,
    startup_path: Vec<u16>,
}

impl WorkerDesktop {
    fn shared() -> io::Result<Self> {
        let shared = shared_worker_desktop()?;
        Ok(Self {
            handle: Some(shared.handle),
            startup_path: shared.startup_path.clone(),
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
    /// Windows the worker put on the shared private desktop, which the broker
    /// closed on its behalf (issue #351). Normally empty. An entry with
    /// `closed: false` is the one that matters: the window ignored `WM_CLOSE`,
    /// so the worker is still waiting on something nobody can answer.
    pub dismissed_windows: Vec<crate::worker_dialog::DismissedWindow>,
}

pub fn run_sentinel_check(program: &Path, timeout: Option<Duration>) -> io::Result<ProcessResult> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionMemoryBudget {
    Standard,
    Render,
}

impl SessionMemoryBudget {
    fn bytes(self) -> usize {
        match self {
            Self::Standard => PROCESS_MEMORY_LIMIT,
            Self::Render => RENDER_SESSION_PROCESS_MEMORY_LIMIT,
        }
    }
}

/// Variables the broker owns at the launch boundary: the handle numbers it
/// injects below, and the broker-side `*_DIR` knobs those handles come from
/// (which are meaningless in the child and must not leak into it). An
/// inherited value is stripped, and a caller-supplied override for one of
/// these is ignored, so no caller can forge a handle number the worker would
/// then treat as broker-created.
fn is_broker_owned_variable(normalized: &str) -> bool {
    matches!(
        normalized,
        "AEX_INSTRUMENT_TRACE_DIR"
            | "AEX_INSTRUMENT_TRACE_HANDLE"
            | "AEXCOMPAT_MINIDUMP_DIR"
            | "AEXCOMPAT_MINIDUMP_HANDLE"
            | "AEXCOMPAT_MINIDUMP_ACK_HANDLE"
    ) || normalized == SESSION_REQUEST_HANDLE_VARIABLE
        || normalized == SESSION_RESPONSE_HANDLE_VARIABLE
        || normalized == SESSION_SECTION_HANDLE_VARIABLE
}

/// Builds the child's environment block: the broker's own environment, minus
/// the broker-owned variables above, plus the handle numbers this launch
/// created, plus the caller's per-launch overrides (issue #910).
///
/// The overrides are applied last so one wins over an inherited value of the
/// same name; they cannot reach a broker-owned key (rejected above) and a
/// malformed key (empty, or containing the `=` separator) is dropped rather
/// than corrupting the block.
fn child_environment(
    trace_handle: Option<HANDLE>,
    minidump_handle: Option<HANDLE>,
    minidump_ack_handle: Option<HANDLE>,
    session: Option<&SessionChildHandles>,
    overrides: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Vec<u16> {
    let mut entries: Vec<(String, std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let normalized = key.to_string_lossy().to_ascii_uppercase();
            if is_broker_owned_variable(&normalized) {
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
    // Applied after the inherited copy and after handle injection: an override
    // replaces whatever the same name already resolved to, and a broker-owned
    // key is never reachable.
    for (key, value) in overrides {
        let normalized = key.to_string_lossy().to_ascii_uppercase();
        if normalized.is_empty()
            || normalized.contains('=')
            || is_broker_owned_variable(&normalized)
        {
            continue;
        }
        entries.retain(|(existing, _, _)| *existing != normalized);
        entries.push((normalized, key.clone(), value.clone()));
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
    timeout: Option<Duration>,
) -> io::Result<ProcessResult> {
    run_isolated_impl(
        program,
        args,
        timeout,
        None,
        WorkerDesktopPolicy::Dedicated,
        None,
        // No repository, so no minidump handle, and the broker's own
        // environment is the whole story for these probe workers.
        &LaunchEnvironment::default(),
        PROCESS_MEMORY_LIMIT,
    )
}

/// Sealed-tree one-shot launch: the staged worker runs on a private desktop
/// with a pinned current directory and the broker-created trace/minidump
/// handles. Same user token as the broker (issue #731): containment is the
/// Job Object and the desktop, not the token.
pub fn run_isolated_staged(
    program: &Path,
    args: &[String],
    timeout: Option<Duration>,
    current_directory: &Path,
    repository: &Path,
    launch_environment: &LaunchEnvironment,
) -> io::Result<ProcessResult> {
    run_isolated_impl(
        program,
        args,
        timeout,
        Some(current_directory),
        WorkerDesktopPolicy::Dedicated,
        Some(repository),
        launch_environment,
        PROCESS_MEMORY_LIMIT,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_isolated_staged_with_memory_limit(
    program: &Path,
    args: &[String],
    timeout: Option<Duration>,
    current_directory: &Path,
    repository: &Path,
    launch_environment: &LaunchEnvironment,
    process_memory_limit: usize,
) -> io::Result<ProcessResult> {
    if !(PROCESS_MEMORY_LIMIT..=MAX_PROBE_PROCESS_MEMORY_LIMIT).contains(&process_memory_limit) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "probe process memory limit is outside the bounded range",
        ));
    }
    run_isolated_impl(
        program,
        args,
        timeout,
        Some(current_directory),
        WorkerDesktopPolicy::Dedicated,
        Some(repository),
        launch_environment,
        process_memory_limit,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_isolated_impl(
    program: &Path,
    args: &[String],
    timeout: Option<Duration>,
    current_directory: Option<&Path>,
    desktop_policy: WorkerDesktopPolicy,
    repository: Option<&Path>,
    launch_environment: &LaunchEnvironment,
    process_memory_limit: usize,
) -> io::Result<ProcessResult> {
    launch_isolated_impl(
        program,
        args,
        current_directory,
        None,
        desktop_policy,
        repository,
        launch_environment,
        process_memory_limit,
    )?
    .wait_and_collect(timeout)
}

/// A resumed isolated worker whose exit has not been awaited yet. One-shot
/// dispatch awaits it immediately (`run_isolated_impl`); a render session
/// keeps it alive across the frame loop and collects it at close. Dropping
/// this without collecting terminates the worker through the job's
/// kill-on-close limit.
pub struct LaunchedIsolatedProcess {
    /// Present only for the shared private desktop: the interactive desktop
    /// is never swept, so a GUI harness worker's windows are left exactly as
    /// they are.
    ///
    /// Declared first so it is also dropped first. The sweep thread reads the
    /// job and process handles below (the desktop handle it also reads lives
    /// for the whole process), and a launch dropped instead of collected —
    /// how a session kills its worker — would otherwise close both while the
    /// thread was still between polls. Windows hands handle values out again,
    /// so that is not merely a failed call.
    dialog_sweep: Option<crate::worker_dialog::DialogSweep>,
    process: OwnedHandle,
    job: OwnedHandle,
    stdout_reader: thread::JoinHandle<io::Result<(String, bool)>>,
    stderr_reader: thread::JoinHandle<io::Result<(String, bool)>>,
    // Opt-in crash minidump (issue #18). The broker keeps the pipe read side
    // and its reader thread alive here until the worker exits; dropping this
    // after `wait_and_collect` finalizes the capture into a `.dmp`. `None` for
    // the render-session path and whenever no repository was supplied.
    minidump_file: Option<crate::minidump_policy::MinidumpLaunchFile>,
    process_memory_limit: usize,
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
    /// Waits for the worker to exit, killing its job if `timeout` elapses first.
    ///
    /// `None` waits indefinitely. That is not the absence of containment: the
    /// job object still kills the tree when this handle drops, and the caller
    /// still owns the process. It is the absence of a *deadline*, for callers
    /// where a slow answer is a better answer than a wrong one — discovery
    /// reports "timed out" for a plug-in that was only still loading, and that
    /// verdict gets cached (issue #354). A worker that blocks on a modal dialog
    /// is released by the dialog sweep below, not by a deadline (issue #359).
    pub fn wait_and_collect(self, timeout: Option<Duration>) -> io::Result<ProcessResult> {
        // Pattern bindings drop in reverse order, so `dialog_sweep` comes last
        // here to be dropped first: an early return below must stop the sweep
        // before the handles it reads are closed.
        let LaunchedIsolatedProcess {
            process: process_handle,
            job,
            stdout_reader,
            stderr_reader,
            minidump_file,
            process_memory_limit,
            dialog_sweep,
        } = self;
        // `INFINITE` is `u32::MAX`, so a `None` deadline and a clamped very long
        // one land on the same wait; naming it keeps that an intent rather than
        // an arithmetic coincidence.
        let wait_ms = match timeout {
            Some(timeout) => timeout.as_millis().min(u32::MAX as u128) as u32,
            None => INFINITE,
        };
        let wait = unsafe { WaitForSingleObject(process_handle.raw(), wait_ms) };
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
        // windows it closed actually went away. (The desktop handle it reads
        // is the process-lifetime shared one, so there is no release to order
        // against.)
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
        let classification = classify_exit(exit_code, timed_out);
        // The hard commit cap rejects the allocation that would cross it, so the
        // recorded peak stops short of the limit by up to one failed request.
        // Treat "peaked within the slack below the cap" as having hit it; this is
        // a heuristic marker, not proof, and it only escalates to a kill reason
        // when the worker also died. The check uses the worker's own peak, not
        // the job aggregate, so a descendant blowing the cap does not implicate
        // the worker.
        let memory_limit_reached = worker_peak_commit_bytes
            .is_some_and(|peak| peak >= process_memory_limit as u64 - MEMORY_LIMIT_DETECTION_SLACK);
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
            process_memory_limit_bytes: process_memory_limit as u64,
            memory_limit_reached,
            dismissed_windows,
        })
    }
}

/// Session launch: same isolation shape as `run_isolated_staged`
/// (kill-on-close Job Object, memory cap, private desktop, handle-list
/// inheritance), plus the three session transport handles inherited and
/// advertised via environment variables. The caller drives the frame loop and
/// collects the exit through the returned `LaunchedIsolatedProcess`.
///
/// `repository` feeds the opt-in crash minidump handle (issue #18/#224): the
/// resident worker gets the same broker-created inherited dump pipe as the
/// one-shot path, retained across the frame loop on the returned process and
/// finalized when the caller collects it at close (`wait_and_collect`).
pub fn launch_isolated_session_staged(
    program: &Path,
    args: &[String],
    current_directory: &Path,
    session: &SessionChildHandles,
    repository: &Path,
    launch_environment: &LaunchEnvironment,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_session_staged_with_budget(
        program,
        args,
        current_directory,
        session,
        repository,
        launch_environment,
        SessionMemoryBudget::Standard,
    )
}

pub(crate) fn launch_isolated_session_staged_with_budget(
    program: &Path,
    args: &[String],
    current_directory: &Path,
    session: &SessionChildHandles,
    repository: &Path,
    launch_environment: &LaunchEnvironment,
    memory_budget: SessionMemoryBudget,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_session_with_desktop_policy(
        program,
        args,
        current_directory,
        session,
        WorkerDesktopPolicy::Dedicated,
        repository,
        launch_environment,
        memory_budget,
    )
}

pub(crate) fn launch_isolated_session_on_current_desktop(
    program: &Path,
    args: &[String],
    current_directory: &Path,
    session: &SessionChildHandles,
    repository: &Path,
    launch_environment: &LaunchEnvironment,
    memory_budget: SessionMemoryBudget,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_session_with_desktop_policy(
        program,
        args,
        current_directory,
        session,
        WorkerDesktopPolicy::Current,
        repository,
        launch_environment,
        memory_budget,
    )
}

#[allow(clippy::too_many_arguments)]
fn launch_isolated_session_with_desktop_policy(
    program: &Path,
    args: &[String],
    current_directory: &Path,
    session: &SessionChildHandles,
    desktop_policy: WorkerDesktopPolicy,
    repository: &Path,
    launch_environment: &LaunchEnvironment,
    memory_budget: SessionMemoryBudget,
) -> io::Result<LaunchedIsolatedProcess> {
    launch_isolated_impl(
        program,
        args,
        Some(current_directory),
        Some(session),
        desktop_policy,
        Some(repository),
        launch_environment,
        memory_budget.bytes(),
    )
}

#[allow(clippy::too_many_arguments)]
fn launch_isolated_impl(
    program: &Path,
    args: &[String],
    current_directory: Option<&Path>,
    session: Option<&SessionChildHandles>,
    desktop_policy: WorkerDesktopPolicy,
    repository: Option<&Path>,
    launch_environment: &LaunchEnvironment,
    process_memory_limit: usize,
) -> io::Result<LaunchedIsolatedProcess> {
    let trace_file = crate::trace_policy::create_trace_file_for_launch()?;
    let mut minidump_file = repository
        .map(|repository| {
            crate::minidump_policy::create_minidump_file_for_launch(
                repository,
                launch_environment.minidump_directory(),
            )
        })
        .transpose()?
        .flatten();
    // Capture the opt-in feature flags before the handles are dropped below so
    // the launch trace can report them. Never log the program path: it may be a
    // private absolute path, which must not escape into diagnostics.
    let trace_active = trace_file.is_some();
    let minidump_active = minidump_file.is_some();
    let session_active = session.is_some();
    let mut desktop = match desktop_policy {
        WorkerDesktopPolicy::Dedicated => WorkerDesktop::shared()?,
        WorkerDesktopPolicy::Current => WorkerDesktop::current()?,
    };
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.ProcessMemoryLimit = process_memory_limit;
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

    // Strip `\\?\` so GPUFoundation resolves its PTX/CUDA dir (#1072). This
    // resubjects the launch path to MAX_PATH; it is safe because the trusted
    // worker stage roots under the temp dir are short. Only the launch
    // application-name/command-line uses the stripped form; provenance, logging,
    // and hashing keep the original `program`.
    let program_launch: std::borrow::Cow<Path> =
        match program.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
            Some(rest)
                if rest.as_bytes().len() >= 2
                    && rest.as_bytes()[0].is_ascii_alphabetic()
                    && rest.as_bytes()[1] == b':' =>
            {
                std::borrow::Cow::Owned(std::path::PathBuf::from(rest))
            }
            _ => std::borrow::Cow::Borrowed(program),
        };
    let program_text = program_launch.as_os_str().to_string_lossy();
    let mut command = quote(&program_text);
    for arg in args {
        command.push(' ');
        command.push_str(&quote(arg));
    }
    let mut command_wide: Vec<u16> = std::ffi::OsStr::new(&command)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let application_wide: Vec<u16> = program_launch
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut environment = child_environment(
        trace_file.as_ref().map(|file| file.raw()),
        minidump_file.as_ref().map(|file| file.raw()),
        minidump_file.as_ref().map(|file| file.ack_raw()),
        session,
        launch_environment.child_overrides(),
    );
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = stdout_write.raw();
    startup.StartupInfo.hStdError = stderr_write.raw();
    startup.StartupInfo.hStdInput = null_mut();
    startup.StartupInfo.lpDesktop = desktop.startup_path();
    startup.lpAttributeList = attribute_list;
    let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
    let creation_flags = EXTENDED_STARTUPINFO_PRESENT
        | CREATE_SUSPENDED
        | CREATE_NO_WINDOW
        | CREATE_UNICODE_ENVIRONMENT;
    // A pinned current directory is what keeps the worker's relative transport
    // access inside the broker-owned <repository>/target boundary; the sealed
    // launch always supplies one, and `lpApplicationName` names the staged
    // executable so the command line cannot redirect it.
    let current_directory_wide: Option<Vec<u16>> = current_directory
        .map(|directory| directory.as_os_str().encode_wide().chain(Some(0)).collect());
    let created = unsafe {
        match &current_directory_wide {
            Some(directory) => CreateProcessW(
                application_wide.as_ptr(),
                command_wide.as_mut_ptr(),
                null(),
                null(),
                1,
                creation_flags,
                environment.as_mut_ptr().cast(),
                directory.as_ptr(),
                &startup.StartupInfo,
                &mut process,
            ),
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
    // Only the desktop the broker created is swept. `None` here means the
    // caller asked for its own desktop (the GUI harness path), where closing a
    // window would be closing the user's. The desktop is shared, but the sweep
    // attributes and closes windows by this launch's Job Object membership, so
    // concurrent workers' windows stay out of each other's reports.
    let dialog_sweep = desktop.handle.map(|handle| {
        crate::worker_dialog::DialogSweep::start(handle, job.raw(), process_handle.raw())
    });
    Ok(LaunchedIsolatedProcess {
        dialog_sweep,
        process: process_handle,
        job,
        stdout_reader,
        stderr_reader,
        minidump_file,
        process_memory_limit,
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

    /// Racing first launches must converge on one desktop (issue #1194): the
    /// desktop count must not scale with worker count, and a losing racer's
    /// desktop would also be a handle nothing ever closes.
    #[test]
    fn concurrent_dedicated_launches_share_one_desktop() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    shared_worker_desktop().map(|desktop| desktop.handle as usize)
                })
            })
            .collect();
        let desktops: std::collections::HashSet<usize> = handles
            .into_iter()
            .map(|thread| thread.join().unwrap().expect("create the shared desktop"))
            .collect();
        assert_eq!(desktops.len(), 1, "every launch must reuse one desktop");
    }

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

    /// Decodes the wide `KEY=VALUE\0...\0\0` block back into pairs.
    fn decode(block: &[u16]) -> Vec<(String, String)> {
        String::from_utf16(block)
            .expect("environment block is UTF-16")
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let (key, value) = entry.split_once('=').expect("KEY=VALUE");
                (key.to_owned(), value.to_owned())
            })
            .collect()
    }

    fn value_of(block: &[u16], key: &str) -> Option<String> {
        decode(block)
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    /// A caller's override replaces whatever the broker's own environment
    /// resolved that name to, and it appears exactly once (issue #910). This
    /// is what lets two concurrent launches disagree about a variable that
    /// `std::env::set_var` could only have set process-wide.
    #[test]
    fn an_override_replaces_the_inherited_value_exactly_once() {
        // PATH is present in every environment this runs in, so overriding it
        // exercises the replace path rather than the append path.
        let overrides = [
            ("PATH".into(), "overridden".into()),
            ("AEXCOMPAT_TEST_ONLY_NEW".into(), "added".into()),
        ];
        let block = child_environment(None, None, None, None, &overrides);
        let decoded = decode(&block);
        assert_eq!(
            decoded
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case("PATH"))
                .count(),
            1,
            "an override must replace, not duplicate: {decoded:?}"
        );
        assert_eq!(value_of(&block, "PATH").as_deref(), Some("overridden"));
        assert_eq!(
            value_of(&block, "AEXCOMPAT_TEST_ONLY_NEW").as_deref(),
            Some("added")
        );
    }

    /// The handle variables are the broker's, not the caller's: an override
    /// naming one is dropped, so the worker still reads the handle number this
    /// launch actually created. Without this a caller could point the worker at
    /// an arbitrary handle value.
    #[test]
    fn an_override_cannot_forge_a_broker_owned_variable() {
        let forged: Vec<(std::ffi::OsString, std::ffi::OsString)> = [
            "AEXCOMPAT_MINIDUMP_HANDLE",
            "AEXCOMPAT_MINIDUMP_ACK_HANDLE",
            "AEX_INSTRUMENT_TRACE_HANDLE",
            "AEXCOMPAT_MINIDUMP_DIR",
            SESSION_REQUEST_HANDLE_VARIABLE,
            SESSION_RESPONSE_HANDLE_VARIABLE,
            SESSION_SECTION_HANDLE_VARIABLE,
            // Malformed keys cannot corrupt the block either.
            "",
            "BROKEN=KEY",
        ]
        .iter()
        .map(|key| ((*key).into(), "1234".into()))
        .collect();

        // No handles created for this launch: every broker-owned name must be
        // absent rather than carrying the caller's value.
        let block = child_environment(None, None, None, None, &forged);
        for (key, _) in &forged {
            let key = key.to_string_lossy();
            assert_eq!(
                value_of(&block, &key),
                None,
                "{key} must not be settable by a caller"
            );
        }

        // With a handle injected, the injected value stands.
        let handle = 0x2a as HANDLE;
        let block = child_environment(Some(handle), None, None, None, &forged);
        assert_eq!(
            value_of(&block, "AEX_INSTRUMENT_TRACE_HANDLE").as_deref(),
            Some("42")
        );
    }
}
