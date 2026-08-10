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
use std::sync::{Arc, Barrier};
use std::thread;

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
fn poisoned_content_root_is_preserved_and_a_stable_fallback_slot_recovers() {
    let (_source, path, hash) = source(b"cleanup fixture");
    let stage = TrustedWorkerStage::create(&path, hash, 15).unwrap();
    let root = stage.root().to_owned();
    let unexpected = root.join("unexpected");
    fs::create_dir(&unexpected).unwrap();
    drop(stage);
    assert!(unexpected.is_dir());
    let recovered = TrustedWorkerStage::create(&path, hash, 15).unwrap();
    assert_ne!(recovered.root(), root);
    assert!(unexpected.is_dir());
    let recovered_again = TrustedWorkerStage::create(&path, hash, 15).unwrap();
    assert_eq!(recovered_again.root(), recovered.root());
    let recovered_root = recovered.root().to_owned();
    drop(recovered);
    assert!(recovered_root.is_dir());
    drop(recovered_again);
    assert!(!recovered_root.exists());
    fs::remove_dir(&unexpected).unwrap();
    fs::remove_dir(&root).unwrap();
}

#[test]
fn live_stage_root_and_worker_cannot_be_replaced_by_name() {
    let (source, path, hash) = source(b"identity binding fixture");
    let stage = TrustedWorkerStage::create(&path, hash, 24).unwrap();
    let replacement = source.0.join("replacement");
    let rename_error = fs::rename(stage.root(), &replacement).unwrap_err();
    assert!(matches!(rename_error.raw_os_error(), Some(5 | 32 | 33)));
    let write_error = fs::write(stage.worker_path(), b"replacement bytes").unwrap_err();
    assert!(matches!(write_error.raw_os_error(), Some(5 | 32 | 33)));
    assert_eq!(
        fs::read(stage.worker_path()).unwrap(),
        b"identity binding fixture"
    );
}

#[test]
fn live_auxiliary_parent_directory_cannot_be_replaced_by_name() {
    let (source, path, hash) = source(b"auxiliary binding fixture");
    let asset = source.0.join("kernel.bin");
    fs::write(&asset, b"kernel fixture").unwrap();
    let stage = TrustedWorkerStage::create_with_assets(
        &path,
        hash,
        25,
        &[(asset, PathBuf::from("kernels/kernel.bin"))],
    )
    .unwrap();
    let parent = stage.root().join("kernels");
    let replacement = stage.root().join("replacement");
    let error = fs::rename(&parent, &replacement).unwrap_err();
    assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)));
    assert_eq!(
        fs::read(parent.join("kernel.bin")).unwrap(),
        b"kernel fixture"
    );
}

#[test]
fn rejects_caller_hash_and_size_mismatch_without_leaking_a_root() {
    let (_source, path, hash) = source(b"mismatch fixture");
    assert_eq!(
        TrustedWorkerStage::create(&path, [0; 32], 16)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    assert_eq!(
        TrustedWorkerStage::create(&path, hash, 17)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[test]
fn rejects_reparse_source_without_following_it() {
    use std::os::windows::fs::symlink_file;
    let (source, target, hash) = source(b"symlink fixture");
    let link = source.0.join("linked.exe");
    if let Err(error) = symlink_file(&target, &link) {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("failed to create symlink: {error}");
    }
    let error =
        TrustedWorkerStage::create(&link, hash, b"symlink fixture".len() as u64).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().to_ascii_lowercase().contains("reparse"));
}

#[test]
fn rejects_precreated_reparse_stage_root_without_following_it() {
    use std::os::windows::fs::symlink_dir;
    let (source, worker, hash) = source(b"stage root reparse fixture");
    let stage = TrustedWorkerStage::create(&worker, hash, 26).unwrap();
    let root = stage.root().to_owned();
    drop(stage);

    let redirect = source.0.join("redirect");
    fs::create_dir(&redirect).unwrap();
    if let Err(error) = symlink_dir(&redirect, &root) {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("failed to create stage-root symlink: {error}");
    }
    let error = TrustedWorkerStage::create(&worker, hash, 26).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    fs::remove_dir(&root).unwrap();
}

#[test]
fn identical_contents_reuse_one_windows_application_identity() {
    let (_source, path, hash) = source(b"stable identity fixture");
    let first = TrustedWorkerStage::create(&path, hash, 23).unwrap();
    let first_worker = first.worker_path().to_owned();
    let first_root = first.root().to_owned();

    let second = TrustedWorkerStage::create(&path, hash, 23).unwrap();
    assert_eq!(second.worker_path(), first_worker);
    assert_eq!(second.root(), first_root);

    drop(first);
    assert!(first_worker.is_file());
    drop(second);
    assert!(!first_root.exists());

    let third = TrustedWorkerStage::create(&path, hash, 23).unwrap();
    assert_eq!(third.worker_path(), first_worker);
}

#[test]
fn simultaneous_identical_stages_share_one_content_root() {
    let (_source, path, hash) = source(b"simultaneous identity fixture");
    let barrier = Arc::new(Barrier::new(3));
    let launch = |path: PathBuf, barrier: Arc<Barrier>| {
        thread::spawn(move || {
            barrier.wait();
            TrustedWorkerStage::create(&path, hash, 29).unwrap()
        })
    };
    let first = launch(path.clone(), Arc::clone(&barrier));
    let second = launch(path, Arc::clone(&barrier));
    barrier.wait();

    let first = first.join().unwrap();
    let second = second.join().unwrap();
    assert_eq!(first.worker_path(), second.worker_path());
    let root = first.root().to_owned();
    drop(first);
    assert!(root.is_dir());
    drop(second);
    assert!(!root.exists());
}

#[test]
fn worker_or_auxiliary_content_changes_the_application_identity() {
    let (source_dir, first_worker, first_hash) = source(b"identity worker one");
    let second_worker = source_dir.0.join("worker-two.exe");
    fs::write(&second_worker, b"identity worker two").unwrap();
    let second_hash = Sha256::digest(b"identity worker two").into();
    let asset = source_dir.0.join("kernel.bin");
    fs::write(&asset, b"kernel one").unwrap();

    let plain = TrustedWorkerStage::create(&first_worker, first_hash, 19).unwrap();
    let changed_worker = TrustedWorkerStage::create(&second_worker, second_hash, 19).unwrap();
    let with_asset = TrustedWorkerStage::create_with_assets(
        &first_worker,
        first_hash,
        19,
        &[(asset.clone(), PathBuf::from("kernels/kernel.bin"))],
    )
    .unwrap();
    let with_asset_again = TrustedWorkerStage::create_with_assets(
        &first_worker,
        first_hash,
        19,
        &[(asset.clone(), PathBuf::from("kernels/kernel.bin"))],
    )
    .unwrap();

    assert_ne!(plain.worker_path(), changed_worker.worker_path());
    assert_ne!(plain.worker_path(), with_asset.worker_path());
    assert_eq!(with_asset.worker_path(), with_asset_again.worker_path());
    assert_eq!(
        fs::read(with_asset.root().join("kernels/kernel.bin")).unwrap(),
        b"kernel one"
    );
    let asset_root = with_asset.root().to_owned();
    drop(with_asset);
    assert!(asset_root.join("kernels/kernel.bin").is_file());
    drop(with_asset_again);
    assert!(!asset_root.exists());
}
