#![cfg(windows)]

use aexcompat_broker::restricted_worker_acl::RestrictedWorkerSid;
use aexcompat_broker::restricted_worker_token::create_restricted_worker_token;
use aexcompat_broker::trusted_worker_stage::TrustedWorkerStage;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;

struct Source(PathBuf);
impl Drop for Source {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source(bytes: &[u8]) -> (Source, PathBuf, [u8; 32]) {
    let dir = Source(std::env::temp_dir().join(format!(
        "aexcompat-worker-source-{:032x}",
        rand::random::<u128>()
    )));
    fs::create_dir(&dir.0).unwrap();
    let path = dir.0.join("worker.exe");
    fs::write(&path, bytes).unwrap();
    (dir, path, Sha256::digest(bytes).into())
}

#[test]
fn stages_authenticated_exe_and_cleans_up_non_recursively() {
    let (_source, path, hash) = source(b"trusted worker fixture");
    let stage =
        TrustedWorkerStage::create(&path, hash, 22, &RestrictedWorkerSid::generate()).unwrap();
    assert_eq!(
        fs::read(stage.worker_path()).unwrap(),
        b"trusted worker fixture"
    );
    assert_eq!(
        stage.worker_path().file_name().unwrap(),
        "trusted-worker.exe"
    );
    let root = stage.root().to_owned();
    drop(stage);
    assert!(!root.exists());
}

#[test]
fn cleanup_never_recurses_into_an_unexpected_child() {
    let (_source, path, hash) = source(b"fixture");
    let stage =
        TrustedWorkerStage::create(&path, hash, 7, &RestrictedWorkerSid::generate()).unwrap();
    let root = stage.root().to_owned();
    let unexpected = root.join("unexpected");
    fs::create_dir(&unexpected).unwrap();
    drop(stage);
    assert!(unexpected.is_dir());
    fs::remove_dir(&unexpected).unwrap();
    fs::remove_dir(&root).unwrap();
}

#[test]
fn rejects_caller_hash_and_size_mismatch_without_leaking_a_root() {
    let (_source, path, hash) = source(b"fixture");
    let sid = RestrictedWorkerSid::generate();
    assert_eq!(
        TrustedWorkerStage::create(&path, [0; 32], 7, &sid)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        TrustedWorkerStage::create(&path, hash, 8, &sid)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[test]
fn rejects_reparse_source_without_following_it() {
    use std::os::windows::fs::symlink_file;
    let (source, target, hash) = source(b"fixture");
    let link = source.0.join("linked.exe");
    if let Err(error) = symlink_file(&target, &link) {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("failed to create symlink: {error}");
    }
    let error =
        TrustedWorkerStage::create(&link, hash, 7, &RestrictedWorkerSid::generate()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn worker_sid_has_rx_but_not_write_delete_or_write_dac() {
    let (_source, path, hash) = source(b"fixture");
    let sid = RestrictedWorkerSid::generate();
    let stage = TrustedWorkerStage::create(&path, hash, 7, &sid).unwrap();
    let token = create_restricted_worker_token(&sid).unwrap();

    with_impersonation(token.as_raw_handle(), || {
        const GENERIC_READ: u32 = 0x8000_0000;
        const GENERIC_EXECUTE: u32 = 0x2000_0000;
        const GENERIC_WRITE: u32 = 0x4000_0000;
        const DELETE: u32 = 0x0001_0000;
        const WRITE_DAC: u32 = 0x0004_0000;

        assert!(open_with_access(stage.worker_path(), GENERIC_READ | GENERIC_EXECUTE).is_ok());
        for denied in [GENERIC_WRITE, DELETE, WRITE_DAC] {
            assert_access_refused(open_with_access(stage.worker_path(), denied), denied);
        }
        for denied in [GENERIC_WRITE, DELETE, WRITE_DAC] {
            assert_access_refused(open_with_access(stage.root(), denied), denied);
        }
    })
    .unwrap();
}

#[test]
fn other_worker_can_read_but_cannot_modify_a_compatibility_stage() {
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_EXECUTE: u32 = 0x2000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const DELETE: u32 = 0x0001_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    let (_source, path, hash) = source(b"cross-tree fixture");
    let owner_sid = RestrictedWorkerSid::generate();
    let other_sid = RestrictedWorkerSid::generate();
    let stage = TrustedWorkerStage::create(&path, hash, 18, &owner_sid).unwrap();
    let other_token = create_restricted_worker_token(&other_sid).unwrap();

    with_impersonation(other_token.as_raw_handle(), || {
        assert!(open_with_access(stage.worker_path(), GENERIC_READ | GENERIC_EXECUTE).is_ok());
        for denied in [GENERIC_WRITE, DELETE, WRITE_DAC] {
            assert_access_refused(open_with_access(stage.worker_path(), denied), denied);
        }
    })
    .unwrap();
}

fn assert_access_refused(result: io::Result<fs::File>, access: u32) {
    let error = result.expect_err("restricted worker unexpectedly received denied access");
    assert!(
        matches!(error.raw_os_error(), Some(5 | 32)),
        "access {access:#010x} failed for an unexpected reason: {error}"
    );
}

fn open_with_access(path: &std::path::Path, access: u32) -> io::Result<fs::File> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    fs::OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn with_impersonation<T>(token: HANDLE, body: impl FnOnce() -> T) -> io::Result<T> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn DuplicateTokenEx(
            existing: HANDLE,
            access: u32,
            attributes: *const SECURITY_ATTRIBUTES,
            level: i32,
            token_type: i32,
            duplicate: *mut HANDLE,
        ) -> i32;
        fn SetThreadToken(thread: *const HANDLE, token: HANDLE) -> i32;
        fn RevertToSelf() -> i32;
    }
    const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
    const SECURITY_IMPERSONATION: i32 = 2;
    const TOKEN_IMPERSONATION: i32 = 2;
    let mut duplicate = null_mut();
    if unsafe {
        DuplicateTokenEx(
            token,
            MAXIMUM_ALLOWED,
            null(),
            SECURITY_IMPERSONATION,
            TOKEN_IMPERSONATION,
            &mut duplicate,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { SetThreadToken(null(), duplicate) } == 0 {
        unsafe { CloseHandle(duplicate) };
        return Err(io::Error::last_os_error());
    }
    let result = body();
    let reverted = unsafe { RevertToSelf() };
    unsafe { CloseHandle(duplicate) };
    if reverted == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(result)
}
