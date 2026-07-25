use std::process::Command;

fn git_output(arguments: &[&str]) -> Option<String> {
    Command::new("git")
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn main() {
    println!("cargo:rerun-if-env-changed=AEXCOMPAT_BUILD_REVISION");
    if let Some(head) = git_output(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(reference) = git_output(&["symbolic-ref", "-q", "HEAD"])
        && let Some(reference_path) = git_output(&["rev-parse", "--git-path", &reference])
    {
        println!("cargo:rerun-if-changed={reference_path}");
    }
    if let Some(packed_refs) = git_output(&["rev-parse", "--git-path", "packed-refs"]) {
        println!("cargo:rerun-if-changed={packed_refs}");
    }
    let revision = std::env::var("AEXCOMPAT_BUILD_REVISION")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git_output(&["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=AEXCOMPAT_BUILD_REVISION={revision}");
}
