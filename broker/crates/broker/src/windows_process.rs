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
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};

const CAPTURE_LIMIT: usize = 64 * 1024;

pub struct ProcessResult {
    pub classification: ExitClassification,
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
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
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn reader(handle_value: usize) -> thread::JoinHandle<io::Result<(String, bool)>> {
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
            let available = CAPTURE_LIMIT.saturating_sub(collected.len());
            let take = available.min(read as usize);
            collected.extend_from_slice(&buffer[..take]);
            truncated |= take < read as usize;
        }
        let text = String::from_utf8_lossy(&collected);
        let (redacted, redaction_truncated) = redact_windows_paths(&text, CAPTURE_LIMIT);
        Ok((redacted, truncated || redaction_truncated))
    })
}

pub fn run_isolated(
    program: &Path,
    args: &[String],
    timeout: Duration,
) -> io::Result<ProcessResult> {
    let (stdout_read, stdout_write) = pipe()?;
    let (stderr_read, stderr_write) = pipe()?;
    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
    let mut inherited = [stdout_write.raw(), stderr_write.raw()];
    if unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_mut_ptr().cast(),
            size_of_val(&inherited),
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
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = stdout_write.raw();
    startup.StartupInfo.hStdError = stderr_write.raw();
    startup.StartupInfo.hStdInput = null_mut();
    startup.lpAttributeList = attribute_list;
    let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
    let created = unsafe {
        CreateProcessW(
            null(),
            command_wide.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_NO_WINDOW,
            null(),
            null(),
            &startup.StartupInfo,
            &mut process,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    let process_handle = OwnedHandle::new(process.hProcess)?;
    let thread_handle = OwnedHandle::new(process.hThread)?;
    if unsafe { AssignProcessToJobObject(job.raw(), process_handle.raw()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { ResumeThread(thread_handle.raw()) } == u32::MAX {
        return Err(io::Error::last_os_error());
    }
    drop(thread_handle);
    drop(stdout_write);
    drop(stderr_write);
    let stdout_reader = reader(stdout_read.take() as usize);
    let stderr_reader = reader(stderr_read.take() as usize);
    let wait = unsafe {
        WaitForSingleObject(
            process_handle.raw(),
            timeout.as_millis().min(u32::MAX as u128) as u32,
        )
    };
    let timed_out = wait == WAIT_TIMEOUT;
    if timed_out {
        unsafe {
            TerminateJobObject(job.raw(), 0xDEAD);
            WaitForSingleObject(process_handle.raw(), INFINITE);
        }
    } else if wait != WAIT_OBJECT_0 {
        return Err(io::Error::last_os_error());
    }
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(process_handle.raw(), &mut exit_code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| io::Error::other("stdout reader panicked"))??;
    let (stderr, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| io::Error::other("stderr reader panicked"))??;
    Ok(ProcessResult {
        classification: classify_exit(exit_code, timed_out),
        exit_code,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}
