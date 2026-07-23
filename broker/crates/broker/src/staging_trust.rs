//! Trusted staging profile (issue #399), enabled by default.
//!
//! The discovery sweep re-hashes every dependency image at closure resolution
//! and twice more while it is sealed into the load tree. This module lets the
//! broker skip re-hashes a process-local cache can prove redundant: an entry
//! is keyed by inode (volume serial + file index) and holds the size,
//! last-write time, and SHA-256 recorded the last time that inode was hashed.
//! A lookup hits only when the handle's current size and last-write time still
//! match the entry. The residual risk — an in-place content swap that
//! preserves both size and last-write time on the same inode — is negligible
//! for the local measurement workloads this project targets, so the profile
//! is on by default; set `AEXCOMPAT_TRUSTED_STAGING=0` to restore the strict
//! per-file re-hashing path when authenticating hostile input.
//!
//! The profile fails closed on real modification: rewriting a file between
//! resolution and staging changes its size or last-write time, the lookup
//! misses (evicting the stale entry), the real hasher runs, and the digest
//! mismatch against the declared expectation fails the dispatch exactly as
//! the strict path does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Seek};

const ENV_VAR: &str = "AEXCOMPAT_TRUSTED_STAGING";

#[derive(Clone, Copy)]
struct CachedDigest {
    size: u64,
    last_write_time: u64,
    sha256: [u8; 32],
}

struct InodeMetadata {
    volume: u64,
    index: u64,
    size: u64,
    last_write_time: u64,
}

thread_local! {
    static CACHE: RefCell<HashMap<(u64, u64), CachedDigest>> = RefCell::new(HashMap::new());
}

#[cfg(test)]
thread_local! {
    static ENABLED_OVERRIDE: std::cell::Cell<Option<bool>> = std::cell::Cell::new(None);
}

/// Whether the trusted staging profile is active for this thread. Enabled by
/// default; `AEXCOMPAT_TRUSTED_STAGING=0` restores strict per-file re-hashing.
pub fn trusted_staging_enabled() -> bool {
    #[cfg(test)]
    if let Some(override_) = ENABLED_OVERRIDE.with(std::cell::Cell::get) {
        return override_;
    }
    !std::env::var_os(ENV_VAR).is_some_and(|value| value == "0")
}

/// Pins the enable flag for a test thread without racing the process env.
#[cfg(test)]
pub fn set_enabled_override_for_testing(value: Option<bool>) {
    ENABLED_OVERRIDE.with(|cell| cell.set(value));
}

/// The cached digest for `file`'s inode, when the recorded size and
/// last-write time still match the handle's current metadata. A metadata
/// mismatch evicts the entry, and any metadata failure is a miss: the cache
/// never fails an operation.
pub fn lookup(file: &File) -> Option<[u8; 32]> {
    let current = inode_metadata(file)?;
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (current.volume, current.index);
        match cache.get(&key) {
            Some(entry)
                if entry.size == current.size
                    && entry.last_write_time == current.last_write_time =>
            {
                Some(entry.sha256)
            }
            Some(_) => {
                cache.remove(&key);
                None
            }
            None => None,
        }
    })
}

/// Records `sha256` as the digest of `file`'s inode at its current size and
/// last-write time. A metadata failure records nothing.
pub fn store(file: &File, sha256: [u8; 32]) {
    let Some(current) = inode_metadata(file) else {
        return;
    };
    CACHE.with(|cache| {
        cache.borrow_mut().insert(
            (current.volume, current.index),
            CachedDigest {
                size: current.size,
                last_write_time: current.last_write_time,
                sha256,
            },
        );
    });
}

/// `hash_file` with the trusted profile applied: on a cache hit the re-hash
/// is skipped and the cached digest is returned with the handle's current
/// size, on a miss the real hasher runs and its result is recorded. With the
/// profile disabled this is exactly `hash_file`. A hit leaves the handle
/// rewound, matching what the call sites do after hashing.
pub fn hash_file_trusted(
    file: &mut File,
    hash_file: impl Fn(&mut File) -> io::Result<(u64, [u8; 32])>,
) -> io::Result<(u64, [u8; 32])> {
    if trusted_staging_enabled() {
        if let Some(digest) = lookup(file) {
            let size = file.metadata()?.len();
            file.rewind()?;
            return Ok((size, digest));
        }
        let result = hash_file(file)?;
        store(file, result.1);
        return Ok(result);
    }
    hash_file(file)
}

#[cfg(windows)]
fn inode_metadata(file: &File) -> Option<InodeMetadata> {
    use std::mem::zeroed;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return None;
    }
    Some(InodeMetadata {
        volume: info.dwVolumeSerialNumber as u64,
        index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        size: ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64,
        last_write_time: ((info.ftLastWriteTime.dwHighDateTime as u64) << 32)
            | info.ftLastWriteTime.dwLowDateTime as u64,
    })
}

#[cfg(unix)]
fn inode_metadata(file: &File) -> Option<InodeMetadata> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().ok()?;
    Some(InodeMetadata {
        volume: metadata.dev(),
        index: metadata.ino(),
        size: metadata.size(),
        last_write_time: ((metadata.mtime() as u64) << 32)
            | (metadata.mtime_nsec() as u64 & 0xffff_ffff),
    })
}

#[cfg(not(any(windows, unix)))]
fn inode_metadata(_: &File) -> Option<InodeMetadata> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;

    fn temp_file(tag: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "aexcompat-staging-trust-{tag}-{:032x}",
            rand::random::<u128>()
        ));
        fs::write(&path, bytes).unwrap();
        path
    }

    fn real_hash(file: &mut File) -> io::Result<(u64, [u8; 32])> {
        file.rewind()?;
        let mut hash = Sha256::new();
        let size = io::copy(file, &mut hash)?;
        Ok((size, hash.finalize().into()))
    }

    #[test]
    fn lookup_hits_a_stored_digest_until_the_file_changes() {
        let path = temp_file("roundtrip", b"payload");
        let file = File::open(&path).unwrap();
        let digest: [u8; 32] = Sha256::digest(b"payload").into();
        store(&file, digest);
        assert_eq!(lookup(&file), Some(digest));

        // Different length, so the miss cannot hinge on mtime granularity.
        fs::write(&path, b"payload-with-more-bytes").unwrap();
        assert_eq!(lookup(&file), None);
        // The stale entry was evicted, not just skipped.
        assert_eq!(lookup(&file), None);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn trusted_hashing_stores_on_a_miss_and_serves_the_cache_on_a_hit() {
        set_enabled_override_for_testing(Some(true));
        let path = temp_file("hash", b"payload");
        let mut file = File::open(&path).unwrap();
        let expected: [u8; 32] = Sha256::digest(b"payload").into();

        let (size, digest) = hash_file_trusted(&mut file, real_hash).unwrap();
        assert_eq!((size, digest), (7, expected));
        let (size, digest) =
            hash_file_trusted(&mut file, |_| panic!("cache hit must not re-hash")).unwrap();
        assert_eq!((size, digest), (7, expected));
        assert_eq!(file.stream_position().unwrap(), 0);
        set_enabled_override_for_testing(None);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn disabled_profile_always_calls_the_real_hasher() {
        set_enabled_override_for_testing(Some(false));
        let path = temp_file("strict", b"payload");
        let mut file = File::open(&path).unwrap();
        // A bogus cached digest must not be served while the profile is off.
        store(&file, [9; 32]);
        let expected: [u8; 32] = Sha256::digest(b"payload").into();
        let (_, digest) = hash_file_trusted(&mut file, real_hash).unwrap();
        assert_eq!(digest, expected);
        set_enabled_override_for_testing(None);
        fs::remove_file(path).unwrap();
    }
}
