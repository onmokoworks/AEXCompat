//! Records what this broker was built from, so it can recognize a worker built
//! from the same tree (issue #649).
//!
//! The worker admission gate compares `minihost/src` mtimes against the worker's
//! own, which needs the repository to be there. A worker shipped beside the
//! plugin has none, so that comparison can only ever answer "indeterminate" and
//! the worker is refused. Comparing the revision the worker carries against the
//! revision the broker carries needs neither the sources nor git at run time,
//! and gives the same answer in a checkout and in a distributed bundle.
//!
//! The revision is resolved the way `guest/crates/aex-guest-worker/build.rs`
//! resolves its own, and the whole record the way `minihost/CMakeLists.txt` does
//! — that one has to agree exactly, because a difference in the rule would read
//! as a difference in the build. (The guest script records no dirty flag; only
//! the revision rule is shared with it.)

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

/// Whether git ran at all, separately from what it printed. `git_output` folds
/// "reported nothing" and "could not ask" together, and for dirtiness those must
/// not be the same answer.
fn git_ran(arguments: &[&str]) -> bool {
    Command::new("git")
        .args(arguments)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn main() {
    println!("cargo:rerun-if-env-changed=AEXCOMPAT_BUILD_REVISION");
    // The dirtiness this records is about the sources compiled into this crate,
    // so it has to be re-asked when they change. Without this, cargo reruns the
    // script only when a git ref moves, and an edited source keeps whatever
    // answer was cached before the edit.
    println!("cargo:rerun-if-changed=src");
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
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| git_output(&["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=AEXCOMPAT_BUILD_REVISION={revision}");

    // Whether tracked files differed from that commit. A revision match between
    // two dirty builds says nothing, so the gate must not read it as one.
    //
    // Fails closed: a `git status` that could not run (no git, an index lock held
    // by a concurrent fetch) records dirty. Reading that as clean would let a
    // modified tree assert an identity it has not earned, and the two git calls
    // are independent — `rev-parse` succeeding says nothing about `status`.
    let status = ["status", "--porcelain", "--untracked-files=no"];
    let dirty = !git_ran(&status) || git_output(&status).is_some();
    println!("cargo:rustc-env=AEXCOMPAT_BUILD_DIRTY={}", u8::from(dirty));
}
