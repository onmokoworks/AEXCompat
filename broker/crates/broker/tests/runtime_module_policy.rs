use aexcompat_broker::runtime_module_policy::{
    RuntimeBackend, WorkerModuleValidation, authenticate_gpu_worker_report_at,
    parse_and_validate_at, path_token, validate_worker_report,
};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    module: PathBuf,
    digest: String,
}

#[test]
fn gpu_report_is_bound_to_backend_session_and_policy_expiration() {
    let f = Fixture::new();
    let policy = parse_and_validate_at(&f.json(""), UNIX_EPOCH).unwrap();
    let session = [0x5a; 32];
    let modules = format!(
        r#"[{{"classification":"policy","basename":"runtime.dll","path_token":"{}","sha256":"{}","size":16}}]"#,
        path_token(&f.module),
        f.digest
    );
    let report = format!(
        r#"{{"session_identity":"{}","backend":"cuda","modules":{modules}}}"#,
        hex(&session)
    );
    let authenticated = authenticate_gpu_worker_report_at(
        report.as_bytes(),
        &session,
        RuntimeBackend::Cuda,
        WorkerModuleValidation {
            policy: &policy,
            sealed: &[],
            trusted: &[],
            system32: &f.root,
        },
        UNIX_EPOCH,
    )
    .unwrap();

    authenticated
        .authorize_dispatch_at(&session, RuntimeBackend::Cuda, UNIX_EPOCH)
        .unwrap();
    assert!(
        authenticated
            .authorize_dispatch_at(&[0x6b; 32], RuntimeBackend::Cuda, UNIX_EPOCH)
            .is_err()
    );
    assert!(
        authenticated
            .authorize_dispatch_at(&session, RuntimeBackend::Opencl, UNIX_EPOCH)
            .is_err()
    );
    assert!(
        authenticated
            .authorize_dispatch_at(
                &session,
                RuntimeBackend::Cuda,
                policy.expires() + Duration::from_secs(1)
            )
            .is_err()
    );
}

#[test]
fn gpu_report_rejects_cpu_and_unbound_or_wrong_backend_reports() {
    let f = Fixture::new();
    let policy = parse_and_validate_at(&f.json(""), UNIX_EPOCH).unwrap();
    let session = [0x33; 32];
    let report = format!(
        r#"{{"session_identity":"{}","backend":"opencl","modules":[]}}"#,
        hex(&session)
    );
    let validate = |backend| {
        authenticate_gpu_worker_report_at(
            report.as_bytes(),
            &session,
            backend,
            WorkerModuleValidation {
                policy: &policy,
                sealed: &[],
                trusted: &[],
                system32: &f.root,
            },
            UNIX_EPOCH,
        )
    };
    assert!(validate(RuntimeBackend::Cuda).is_err());
    assert!(validate(RuntimeBackend::Cpu).is_err());
    assert!(
        authenticate_gpu_worker_report_at(
            report.as_bytes(),
            &[0x44; 32],
            RuntimeBackend::Opencl,
            WorkerModuleValidation {
                policy: &policy,
                sealed: &[],
                trusted: &[],
                system32: &f.root,
            },
            UNIX_EPOCH,
        )
        .is_err()
    );
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "aex-runtime-policy-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let module = root.join("runtime.dll");
        fs::write(&module, b"approved runtime").unwrap();
        let digest = hex(&Sha256::digest(b"approved runtime"));
        Self {
            root,
            module,
            digest,
        }
    }

    fn json(&self, extra: &str) -> Vec<u8> {
        format!(r#"{{"schema_version":1,"expires":"2099-01-02T03:04:05Z","modules":[{{"path":{},"basename":"runtime.dll","sha256":"{}","size":16,"backend":"cuda"{extra}}}]}}"#,
            serde_json::to_string(&self.module).unwrap(), self.digest).into_bytes()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn parses_v1_and_validates_policy_worker_report() {
    let f = Fixture::new();
    let policy = parse_and_validate_at(
        &f.json(",\"signer_thumbprint\":\"AABB\",\"version\":\"1.2.3\""),
        UNIX_EPOCH,
    )
    .unwrap();
    assert_eq!(policy.modules().len(), 1);
    let report = format!(
        r#"[{{"classification":"policy","basename":"runtime.dll","path_token":"{}","sha256":"{}","size":16}}]"#,
        path_token(&f.module),
        f.digest
    );
    validate_worker_report(
        report.as_bytes(),
        WorkerModuleValidation {
            policy: &policy,
            sealed: &[],
            trusted: &[],
            system32: &f.root,
        },
    )
    .unwrap();
}

#[test]
fn winsxs_report_is_bound_to_a_direct_assembly_child() {
    let f = Fixture::new();
    let winsxs = f.root.parent().unwrap().join("WinSxS");
    let assembly = winsxs.join("amd64_microsoft.windows.common-controls_6595b64144ccf1df_5.82");
    fs::create_dir_all(&assembly).unwrap();
    let module = assembly.join("COMCTL32.dll");
    let bytes = b"WinSxS fixture";
    fs::write(&module, bytes).unwrap();
    let digest = hex(&Sha256::digest(bytes));
    let report = format!(
        r#"[{{"classification":"winsxs","basename":"COMCTL32.dll","path_token":"{}","sha256":"{}","size":14}}]"#,
        path_token(&module),
        digest
    );
    let result = validate_worker_report(
        report.as_bytes(),
        WorkerModuleValidation {
            policy: &parse_and_validate_at(&f.json(""), UNIX_EPOCH).unwrap(),
            sealed: &[],
            trusted: &[],
            system32: &f.root,
        },
    );
    let _ = fs::remove_dir_all(&winsxs);
    result.unwrap();
}

#[test]
fn rejects_unknown_fields_expiration_and_unsafe_name() {
    let f = Fixture::new();
    assert!(parse_and_validate_at(&f.json(",\"surprise\":true"), UNIX_EPOCH).is_err());
    let expired = String::from_utf8(f.json(""))
        .unwrap()
        .replace("2099-01-02T03:04:05Z", "1970-01-01T00:00:01Z");
    assert!(
        parse_and_validate_at(expired.as_bytes(), UNIX_EPOCH + Duration::from_secs(1)).is_err()
    );
    let unsafe_name = String::from_utf8(f.json(""))
        .unwrap()
        .replace("runtime.dll", "../runtime.dll");
    assert!(parse_and_validate_at(unsafe_name.as_bytes(), UNIX_EPOCH).is_err());
}

#[test]
fn rejects_collisions_and_changed_file() {
    let f = Fixture::new();
    let one = String::from_utf8(f.json("")).unwrap();
    let module = one
        .split("\"modules\":[")
        .nth(1)
        .unwrap()
        .strip_suffix("}")
        .unwrap();
    let duplicate = one.replace(
        module,
        &format!("{},{}", module.strip_suffix(']').unwrap(), module),
    );
    assert!(parse_and_validate_at(duplicate.as_bytes(), UNIX_EPOCH).is_err());

    let policy = parse_and_validate_at(&f.json(""), UNIX_EPOCH).unwrap();
    fs::write(&f.module, b"modified runtime").unwrap();
    let report = format!(
        r#"[{{"classification":"policy","basename":"runtime.dll","path_token":"{}","sha256":"{}","size":16}}]"#,
        path_token(&f.module),
        f.digest
    );
    assert!(
        validate_worker_report(
            report.as_bytes(),
            WorkerModuleValidation {
                policy: &policy,
                sealed: &[],
                trusted: &[],
                system32: &f.root
            }
        )
        .is_err()
    );
}

#[test]
fn classification_cannot_be_substituted() {
    let f = Fixture::new();
    let policy = parse_and_validate_at(&f.json(""), UNIX_EPOCH).unwrap();
    let report = format!(
        r#"[{{"classification":"trusted","basename":"runtime.dll","path_token":"{}","sha256":"{}","size":16}}]"#,
        path_token(&f.module),
        f.digest
    );
    assert!(
        validate_worker_report(
            report.as_bytes(),
            WorkerModuleValidation {
                policy: &policy,
                sealed: &[],
                trusted: &[],
                system32: &f.root
            }
        )
        .is_err()
    );
}

#[test]
fn rejects_hardlinked_module() {
    let f = Fixture::new();
    fs::hard_link(&f.module, f.root.join("alias.dll")).unwrap();
    assert!(parse_and_validate_at(&f.json(""), UNIX_EPOCH).is_err());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
