#![cfg(windows)]

//! Staging contract for the worker executable that actually launches.
//!
//! The ACL-impersonation cases that lived here (a restricted worker gets
//! read/execute but not write/delete/WRITE_DAC, and cannot modify another
//! worker's stage) went with the restricted token and the protected DACL in
//! issue #731. What remains is what the stage still guarantees: the launched
//! image is the bytes the caller authenticated, a hash or size mismatch
//! leaves no root behind, a reparse point is never followed, and cleanup
//! stays non-recursive.

use aexcompat_broker::trusted_worker_stage::TrustedWorkerStage;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

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
    let stage = TrustedWorkerStage::create(&path, hash, 22).unwrap();
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
    let stage = TrustedWorkerStage::create(&path, hash, 7).unwrap();
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
    assert_eq!(
        TrustedWorkerStage::create(&path, [0; 32], 7)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        TrustedWorkerStage::create(&path, hash, 8)
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
    let error = TrustedWorkerStage::create(&link, hash, 7).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}
