#![cfg(windows)]

use aexcompat_broker::runtime_module_identity::{
    AuthenticodeEvidence, IdentityEvidenceErrorKind, PeMachine, capture_runtime_module_identity,
    require_verified_authenticode,
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
    // Core KnownDLLs are servicing hard links; this Microsoft component is a regular file.
    let path = PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("System32\\appverifUI.dll");
    let bytes = fs::read(&path).unwrap();
    let first = capture_runtime_module_identity(&path).unwrap();
    let second = capture_runtime_module_identity(&path).unwrap();
    assert_eq!(first.canonical_path, fs::canonicalize(&path).unwrap());
    assert_eq!(first.size, bytes.len() as u64);
    assert_eq!(first.sha256.as_slice(), Sha256::digest(&bytes).as_slice());
    assert!(matches!(
        first.pe_machine,
        PeMachine::Amd64 | PeMachine::Arm64
    ));
    assert_eq!(first.file_identity, second.file_identity);
    assert!(matches!(
        first.authenticode,
        AuthenticodeEvidence::Embedded | AuthenticodeEvidence::Catalog
    ));
    if first.signing_catalog_sha256.is_some() {
        assert_eq!(first.authenticode, AuthenticodeEvidence::Catalog);
    }
    if first.authenticode == AuthenticodeEvidence::Embedded {
        assert!(first.signing_catalog_sha256.is_none());
    }
    require_verified_authenticode(&first).unwrap();
}

#[test]
fn rejects_unsigned_pe_fixture_fail_closed() {
    let (path, _) = fixture(0x8664);
    assert_eq!(
        capture_runtime_module_identity(&path).unwrap_err().kind,
        IdentityEvidenceErrorKind::UntrustedSignature
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_malformed_pe_and_fails_closed_for_authenticode() {
    let (path, _) = fixture(0x014c);
    assert_eq!(
        capture_runtime_module_identity(&path).unwrap_err().kind,
        IdentityEvidenceErrorKind::UntrustedSignature
    );
    fs::write(&path, b"not a PE").unwrap();
    assert_eq!(
        capture_runtime_module_identity(&path).unwrap_err().kind,
        IdentityEvidenceErrorKind::InvalidPe
    );
    fs::remove_file(path).unwrap();
}
