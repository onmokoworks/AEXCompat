#![cfg(target_os = "macos")]

use std::process::Command;

#[test]
fn headless_description_rejects_bad_requests_without_success_json_or_path_leaks() {
    let executable = env!("CARGO_BIN_EXE_aexcompat-harness");
    let missing_argument = Command::new(executable)
        .args(["--headless", "--describe-aex"])
        .output()
        .unwrap();
    assert_eq!(missing_argument.status.code(), Some(64));
    assert!(missing_argument.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&missing_argument.stderr)
            .contains("aexcompat_description_error: expected exactly one AEX path")
    );

    let missing_path = std::env::temp_dir().join(format!(
        "aexcompat-description-missing-{}-{}.aex",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let missing_plugin = Command::new(executable)
        .args(["--headless", "--describe-aex"])
        .arg(&missing_path)
        .output()
        .unwrap();
    assert_eq!(missing_plugin.status.code(), Some(1));
    assert!(missing_plugin.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&missing_plugin.stderr);
    assert!(diagnostic.contains("aexcompat_description_error: plugin_missing"));
    assert!(!diagnostic.contains(&missing_path.to_string_lossy().to_string()));

    let scratch = missing_path.with_extension("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let plugin = scratch.join("synthetic.aex");
    let unusable_worker = scratch.join("unusable-worker");
    std::fs::write(&plugin, b"synthetic AEX for error classification").unwrap();
    std::fs::write(&unusable_worker, b"not a Mach-O worker").unwrap();
    let failed_setup = Command::new(executable)
        .args(["--headless", "--describe-aex"])
        .arg(&plugin)
        .env("AEXCOMPAT_GUEST_WORKER", &unusable_worker)
        .output()
        .unwrap();
    assert_eq!(failed_setup.status.code(), Some(1));
    assert!(failed_setup.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&failed_setup.stderr);
    assert!(diagnostic.contains("aexcompat_description_error: description_failed"));
    assert!(!diagnostic.contains(&scratch.to_string_lossy().to_string()));
    std::fs::remove_dir_all(scratch).unwrap();
}
