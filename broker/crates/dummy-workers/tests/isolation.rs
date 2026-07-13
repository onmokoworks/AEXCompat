#[cfg(windows)]
#[test]
fn five_isolation_scenarios_pass() {
    use aexcompat_broker::selftest::{run, Workers};
    use std::path::PathBuf;
    let output = std::env::temp_dir().join(format!(
        "aexcompat-broker-selftest-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&output);
    let workers = Workers {
        exit0: PathBuf::from(env!("CARGO_BIN_EXE_dummy_exit0")),
        sleep: PathBuf::from(env!("CARGO_BIN_EXE_dummy_sleep")),
        abort: PathBuf::from(env!("CARGO_BIN_EXE_dummy_abort")),
    };
    assert!(run(&workers, &output).expect("run selftest"));
    let report = std::fs::read_to_string(&output).expect("read report");
    assert!(report.contains("\"scenario_count\": 5"));
    assert!(report.contains("sentinel_not_inherited"));
    assert!(report.contains("\"accepts_aex_path\": false"));
    std::fs::remove_file(output).expect("remove report");
}
