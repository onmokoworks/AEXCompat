use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AEXCOMPAT_BUILD_REVISION");
    let revision = std::env::var("AEXCOMPAT_BUILD_REVISION")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=AEXCOMPAT_BUILD_REVISION={revision}");
}
