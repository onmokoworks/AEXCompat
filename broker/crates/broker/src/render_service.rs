//! Session-owned lifecycle for the registered Three renderer.
use crate::render_service_registration::resolve_service_in_directory;
use crate::secure_launch::LaunchEnvironment;
use crate::windows_process::{LaunchedIsolatedProcess, launch_isolated_render_service};
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
use windows_sys::Win32::System::Threading::{
    CreateSemaphoreW, ReleaseSemaphore, WaitForSingleObject,
};

pub(crate) struct RenderServiceLease {
    owned: Option<LaunchedIsolatedProcess>,
    _reservation: ServiceReservation,
}

struct ServiceReservation(usize);

impl ServiceReservation {
    fn acquire(name: &str) -> io::Result<Self> {
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let semaphore = unsafe { CreateSemaphoreW(null(), 1, 1, name.as_ptr()) };
        if semaphore.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = unsafe { WaitForSingleObject(semaphore, 0) };
        if result == WAIT_OBJECT_0 {
            return Ok(Self(semaphore as usize));
        }
        let error = if result == WAIT_TIMEOUT {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "Three renderer is reserved by another render session",
            )
        } else {
            io::Error::last_os_error()
        };
        unsafe {
            CloseHandle(semaphore);
        }
        Err(error)
    }
}

impl Drop for ServiceReservation {
    fn drop(&mut self) {
        unsafe {
            ReleaseSemaphore(self.0 as HANDLE, 1, std::ptr::null_mut());
            CloseHandle(self.0 as HANDLE);
        }
    }
}

impl Drop for RenderServiceLease {
    fn drop(&mut self) {
        if let Some(process) = self.owned.take() {
            let _ = process.terminate_job();
            let _ = process.wait_and_collect(Some(Duration::from_secs(5)));
        }
    }
}

impl RenderServiceLease {
    pub(crate) fn acquire(plugin_paths: &[&Path]) -> io::Result<Option<Self>> {
        let Some(local) = std::env::var_os("LOCALAPPDATA") else {
            return Ok(None);
        };
        let registration = PathBuf::from(local).join("AEXCompat/render-services/three-v1");
        let mut installation = None;
        for plugin in plugin_paths {
            if let Some(found) = resolve_service_in_directory(&registration, plugin)? {
                installation = Some(found);
                break;
            }
        }
        let Some(installation) = installation else {
            return Ok(None);
        };
        let reservation = ServiceReservation::acquire("Local\\AEXCompat-ThreeRenderer-v1-session")?;
        let mut lease = Self {
            owned: None,
            _reservation: reservation,
        };
        let pipe: Vec<u16> = r"\\.\pipe\ae-three-renderer-v1"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        if unsafe { WaitNamedPipeW(pipe.as_ptr(), 0) } != 0 {
            // An existing service is borrowed, never terminated by this lease.
            return Ok(Some(lease));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(2) {
            return Err(error);
        }
        let environment = LaunchEnvironment::default()
            .without_child_var("ELECTRON_RENDERER_URL")
            .without_child_var("ELECTRON_RUN_AS_NODE");
        lease.owned = Some(launch_isolated_render_service(
            &installation.executable,
            &[installation.entry.to_string_lossy().into_owned()],
            &installation.root,
            &environment,
        )?);
        let start = Instant::now();
        loop {
            if lease.owned.as_ref().unwrap().has_exited() {
                return Err(io::Error::other(
                    "owned Three renderer exited before readiness",
                ));
            }
            if unsafe { WaitNamedPipeW(pipe.as_ptr(), 0) } != 0 {
                break;
            }
            if start.elapsed() >= Duration::from_secs(30) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Three renderer did not become ready",
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(Some(lease))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_lease_reservation_excludes_then_releases_across_threads() {
        let name = format!("Local\\AEXCompat-service-test-{}", rand::random::<u64>());
        let lease = RenderServiceLease {
            owned: None,
            _reservation: ServiceReservation::acquire(&name).unwrap(),
        };
        let result = ServiceReservation::acquire(&name);
        assert!(matches!(result, Err(ref e) if e.kind() == io::ErrorKind::WouldBlock));
        // The process-containing lease stays on its owning thread. The
        // semaphore reservation itself has no thread-affine ownership.
        drop(lease);
        let reacquired = ServiceReservation::acquire(&name).unwrap();
        assert!(ServiceReservation::acquire(&name).is_err());
        std::thread::spawn(move || drop(reacquired)).join().unwrap();
        assert!(ServiceReservation::acquire(&name).is_ok());
    }
}
