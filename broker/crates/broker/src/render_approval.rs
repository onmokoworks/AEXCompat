use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalIdentity {
    pub sealed_manifest_sha256: [u8; 32],
    pub worker_sha256: [u8; 32],
    pub worker_byte_size: u64,
    pub timeout_ms: u64,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Admits one render launch and returns the canonical receipt-selected worker.
pub fn admit_launch(
    selected_worker: &Path,
    receipt_worker: &Path,
    previous_identity: Option<&ApprovalIdentity>,
    identity: &ApprovalIdentity,
) -> io::Result<PathBuf> {
    let selected_worker = fs::canonicalize(selected_worker)?;
    let receipt_worker = fs::canonicalize(receipt_worker)?;
    if selected_worker != receipt_worker {
        return Err(invalid(
            "selected worker differs from receipt trusted worker",
        ));
    }
    if previous_identity.is_some_and(|approved| approved != identity) {
        return Err(invalid("render approval changed between determinism runs"));
    }
    Ok(receipt_worker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-render-approval-{}-{nonce}-{}",
            std::process::id(),
            TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn identity() -> ApprovalIdentity {
        ApprovalIdentity {
            sealed_manifest_sha256: [0x11; 32],
            worker_sha256: [0x22; 32],
            worker_byte_size: 123,
            timeout_ms: 5_000,
        }
    }

    #[test]
    fn matching_selection_and_receipt_are_admitted() {
        let root = test_directory();
        let worker = root.join("worker.exe");
        fs::write(&worker, b"worker").unwrap();
        let admitted = admit_launch(&worker, &worker, None, &identity()).unwrap();
        assert_eq!(admitted, fs::canonicalize(&worker).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn different_selected_worker_is_rejected_before_launch() {
        let root = test_directory();
        let selected = root.join("selected.exe");
        let receipt = root.join("receipt.exe");
        fs::write(&selected, b"selected").unwrap();
        fs::write(&receipt, b"receipt").unwrap();
        let error = admit_launch(&selected, &receipt, None, &identity()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("differs from receipt"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_identity_is_admitted_but_each_identity_mutation_is_rejected() {
        let root = test_directory();
        let worker = root.join("worker.exe");
        fs::write(&worker, b"worker").unwrap();
        let approved = identity();
        assert!(admit_launch(&worker, &worker, Some(&approved), &approved).is_ok());
        let mutations = [
            ApprovalIdentity {
                sealed_manifest_sha256: [0x33; 32],
                ..approved.clone()
            },
            ApprovalIdentity {
                worker_sha256: [0x44; 32],
                ..approved.clone()
            },
            ApprovalIdentity {
                worker_byte_size: approved.worker_byte_size + 1,
                ..approved.clone()
            },
            ApprovalIdentity {
                timeout_ms: approved.timeout_ms + 1,
                ..approved.clone()
            },
        ];
        for mutation in mutations {
            let error = admit_launch(&worker, &worker, Some(&approved), &mutation).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(
                error
                    .to_string()
                    .contains("changed between determinism runs")
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
