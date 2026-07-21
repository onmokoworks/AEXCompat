#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::restricted_worker_acl::{RestrictedWorkerSid, protect_sealed_load_tree};
    use aexcompat_broker::restricted_worker_token::{
        RestrictedWorkerToken, create_restricted_worker_token,
    };
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, PROCESS_INFORMATION, STARTUPINFOW, WaitForSingleObject,
    };

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn DuplicateTokenEx(
            existing_token: HANDLE,
            desired_access: u32,
            token_attributes: *const SECURITY_ATTRIBUTES,
            impersonation_level: i32,
            token_type: i32,
            new_token: *mut HANDLE,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateProcessW(
            application_name: *const u16,
            command_line: *mut u16,
            process_attributes: *const SECURITY_ATTRIBUTES,
            thread_attributes: *const SECURITY_ATTRIBUTES,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *const c_void,
            current_directory: *const u16,
            startup_info: *const STARTUPINFOW,
            process_information: *mut PROCESS_INFORMATION,
        ) -> i32;
    }

    struct TempTree(PathBuf);

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn worker_restricted_operations_can_only_read_the_sealed_directory() {
        let fixture = build_fixture();
        let root = TempTree(std::env::temp_dir().join(format!(
            "aexcompat-sealed-worker-e2e-{}-{:032x}",
            std::process::id(),
            rand::random::<u128>()
        )));
        std::fs::create_dir(&root.0).unwrap();
        std::fs::copy(fixture, root.0.join("sealed_acl_probe.exe")).unwrap();
        std::fs::write(root.0.join("payload.bin"), b"sealed fixture").unwrap();

        let sid = RestrictedWorkerSid::generate();
        protect_sealed_load_tree(&root.0, &["sealed_acl_probe.exe", "payload.bin"], &sid).unwrap();
        let token = create_restricted_worker_token(&sid).unwrap();
        assert!(token.contains_restricting_sid(&sid).unwrap());

        let exit_code = launch_with_inherited_restricted_token(
            &token,
            &root.0.join("sealed_acl_probe.exe"),
            &root.0,
        )
        .unwrap();
        assert_eq!(
            exit_code, 0,
            "probe failure bitmap {exit_code:#09b}: read, write, create, delete, root WRITE_DAC, file WRITE_DAC, token setup"
        );
        assert_eq!(
            std::fs::read(root.0.join("payload.bin")).unwrap(),
            b"sealed fixture"
        );
        assert!(!root.0.join("created.bin").exists());
    }

    fn build_fixture() -> PathBuf {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--manifest-path"])
            .arg(manifest)
            .args(["-p", "dummy-workers", "--bin", "sealed_acl_probe"])
            .status()
            .expect("run cargo build for sealed ACL fixture");
        assert!(status.success(), "sealed ACL fixture build failed");
        let deps = std::env::current_exe().unwrap();
        deps.parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("sealed_acl_probe.exe")
    }

    fn launch_with_inherited_restricted_token(
        token: &RestrictedWorkerToken,
        executable: &Path,
        root: &Path,
    ) -> io::Result<u32> {
        const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
        const SECURITY_IMPERSONATION: i32 = 2;
        const TOKEN_IMPERSONATION: i32 = 2;

        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut child_token = null_mut();
        if unsafe {
            DuplicateTokenEx(
                token.as_raw_handle(),
                MAXIMUM_ALLOWED,
                &mut attributes,
                SECURITY_IMPERSONATION,
                TOKEN_IMPERSONATION,
                &mut child_token,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }

        let application: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
        let mut command: Vec<u16> = format!(
            "\"{}\" \"{}\" {}",
            executable.display(),
            root.display(),
            child_token as usize
        )
        .encode_utf16()
        .chain([0])
        .collect();
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                0,
                null(),
                null(),
                &startup,
                &mut process,
            )
        };
        unsafe { CloseHandle(child_token) };
        if created == 0 {
            return Err(io::Error::last_os_error());
        }

        unsafe { CloseHandle(process.hThread) };
        unsafe { WaitForSingleObject(process.hProcess, u32::MAX) };
        let mut exit_code = 0;
        let result = unsafe { GetExitCodeProcess(process.hProcess, &mut exit_code) };
        unsafe { CloseHandle(process.hProcess) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(exit_code)
    }
}
