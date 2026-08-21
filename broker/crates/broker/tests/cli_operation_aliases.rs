#![cfg(windows)]

use std::process::{Command, Output};

fn run_broker(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_broker"))
        .args(arguments)
        .output()
        .expect("run broker CLI")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn discovery_and_l2_reach_the_same_json_output_boundary() {
    for operation in ["discovery", "l2"] {
        let output = run_broker(&[operation, "unused-fixture", "not-json"]);
        assert_eq!(output.status.code(), Some(2), "{operation}");
        assert!(
            stderr(&output).contains("observation output must be JSON"),
            "{operation}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn classic_and_render_parameter_routes_reach_the_same_request_boundary() {
    for operation in ["classic-parameter-request", "render-parameter-request"] {
        let output = run_broker(&[operation, "missing-request.json", "missing-output.json"]);
        assert_eq!(output.status.code(), Some(2), "{operation}");
        assert!(
            stderr(&output).contains("parameterized render request failed"),
            "{operation}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn extended_classic_and_render_aliases_reach_the_same_route_boundary() {
    for operation in [
        "classic-threaded-default-scattermap",
        "render-threaded-default-scattermap",
    ] {
        let output = run_broker(&[operation, "../forbidden.json"]);
        assert_eq!(output.status.code(), Some(101), "{operation}");
        assert!(
            stderr(&output).contains("output traversal forbidden"),
            "{operation}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn unknown_operation_still_fails_closed() {
    let output = run_broker(&["classic-unknown-scattermap", "unused.json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown broker operation"));
}
