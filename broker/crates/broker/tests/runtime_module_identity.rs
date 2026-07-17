#![cfg(windows)]

use aexcompat_broker::runtime_module_identity::{
    capture_runtime_module_identity, require_verified_authenticode, AuthenticodeEvidence,
    IdentityEvidenceErrorKind, PeMachine,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

fn fixture(machine: u16) -> (PathBuf, Vec<u8>) {
    let mut bytes = vec![0u8; 0x86];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    let path = std::env::temp_dir().join(format!(
        "aexcompat-runtime-identity-{}-{}.dll",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

#[test]
fn captures_canonical_hash_machine_and_stable_file_identity() {
    let (path, bytes) = fixture(0x8664);
    let first = capture_runtime_module_identity(&path).unwrap();
    let second = capture_runtime_module_identity(&path).unwrap();
    assert_eq!(first.canonical_path, fs::canonicalize(&path).unwrap());
    assert_eq!(first.size, bytes.len() as u64);
    assert_eq!(first.sha256.as_slice(), Sha256::digest(&bytes).as_slice());
    assert_eq!(first.pe_machine, PeMachine::Amd64);
    assert_eq!(first.file_identity, second.file_identity);
    assert_eq!(first.authenticode, AuthenticodeEvidence::Unsupported);
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_malformed_pe_and_fails_closed_for_authenticode() {
    let (path, _) = fixture(0x014c);
    let evidence = capture_runtime_module_identity(&path).unwrap();
    assert_eq!(evidence.pe_machine, PeMachine::I386);
    assert_eq!(
        require_verified_authenticode(&evidence).unwrap_err().kind,
        IdentityEvidenceErrorKind::Unsupported
    );
    fs::write(&path, b"not a PE").unwrap();
    assert_eq!(
        capture_runtime_module_identity(&path).unwrap_err().kind,
        IdentityEvidenceErrorKind::InvalidPe
    );
    fs::remove_file(path).unwrap();
}
