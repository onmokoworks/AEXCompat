#![cfg(windows)]

#[test]
fn render_fixture_argv_dispatch_rejects_noncanonical_fixture_without_output() {
    let scratch = std::env::temp_dir().join(format!(
        "aexcompat-fixture-cli-{}-{:032x}",
        std::process::id(),
        rand_seed()
    ));
    std::fs::create_dir_all(&scratch).unwrap();
    let plugin = scratch.join("fixture.aex");
    let fixture = scratch.join("fixture.json");
    let output = scratch.join("output");
    std::fs::write(&plugin, b"synthetic-not-an-aex").unwrap();
    std::fs::write(
        &fixture,
        br#"{"schema":"aexcompat.render_fixture","schema_version":99}"#,
    )
    .unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_aexcompat-harness"))
        .args(["--headless", "--render-fixture"])
        .arg(&plugin)
        .arg(&fixture)
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists(), "invalid CLI fixture published output");
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("fixture parameters must be an array"),
        "fixture argv did not reach strict JSON dispatch: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let _ = std::fs::remove_dir_all(scratch);
}

fn rand_seed() -> u128 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    now ^ u128::from(std::process::id())
}
