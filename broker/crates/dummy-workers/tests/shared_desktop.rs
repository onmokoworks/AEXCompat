//! Dedicated workers share one private desktop for the life of the broker
//! process (issue #1194). A desktop per worker asked Windows for a
//! create/destroy cycle per launch, and DWM leaks composition state on every
//! cycle on the OS side, so a long sweep progressively degraded the whole
//! interactive session. These tests observe the desktop from inside the
//! workers: many launches — including one that fails to start and one that is
//! killed on its deadline — must all land on the same private desktop.

#![cfg(windows)]

use std::path::Path;
use std::time::Duration;

/// The desktop the current thread is on, read the same way the probe worker
/// reads its own, so "the workers were not on the caller's desktop" compares
/// like with like.
fn current_desktop_name() -> String {
    use std::mem::size_of;
    use windows_sys::Win32::System::StationsAndDesktops::{
        GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    unsafe {
        let desktop = GetThreadDesktop(GetCurrentThreadId());
        let mut required_bytes = 0u32;
        GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            std::ptr::null_mut(),
            0,
            &mut required_bytes,
        );
        let mut name = vec![0u16; (required_bytes as usize).div_ceil(size_of::<u16>()).max(1)];
        assert_ne!(
            GetUserObjectInformationW(
                desktop,
                UOI_NAME,
                name.as_mut_ptr().cast(),
                (name.len() * size_of::<u16>()) as u32,
                &mut required_bytes,
            ),
            0,
            "query the caller's desktop name"
        );
        let end = name
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(name.len());
        String::from_utf16_lossy(&name[..end])
    }
}

/// Launches the probe worker and answers with the desktop it saw itself on.
fn observed_worker_desktop() -> String {
    use aexcompat_broker::windows_process::run_isolated;
    let result = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_desktop_probe")),
        &[],
        Some(Duration::from_secs(30)),
    )
    .expect("run the desktop probe worker");
    assert_eq!(
        result.classification.as_str(),
        "ok",
        "probe worker failed: {:?}",
        result.stderr
    );
    let name = result.stdout.trim().to_owned();
    assert!(!name.is_empty(), "probe reported no desktop name");
    name
}

#[test]
fn many_workers_share_one_private_desktop_across_failures_and_timeouts() {
    use aexcompat_broker::windows_process::run_isolated;

    let first = observed_worker_desktop();
    assert!(
        first.starts_with("AEXCompatWorkerDesktop-"),
        "worker desktop was not broker-created: {first:?}"
    );
    assert_ne!(
        first,
        current_desktop_name(),
        "worker ran on the caller's desktop"
    );

    // A worker that never starts: CreateProcessW fails after the shared
    // desktop is resolved. The failure must not tear the desktop down or make
    // the next launch create a replacement.
    let missing = std::env::temp_dir().join("aexcompat-1194-missing-worker.exe");
    let _ = std::fs::remove_file(&missing);
    assert!(
        run_isolated(&missing, &[], Some(Duration::from_secs(5))).is_err(),
        "a missing worker binary must fail to launch"
    );

    // A worker killed on its deadline: the timeout path collects the launch
    // without closing the shared desktop either.
    let killed = run_isolated(
        Path::new(env!("CARGO_BIN_EXE_dummy_sleep")),
        &["30000".into()],
        Some(Duration::from_millis(200)),
    )
    .expect("run the sleeping worker");
    assert_eq!(killed.classification.as_str(), "timeout_killed");

    // Sequential relaunch after both failure shapes, then a concurrent batch:
    // every worker must observe the exact desktop the first one did. A
    // regression back to per-worker desktops fails here on the first compare
    // (the desktop name carries a per-desktop random suffix).
    assert_eq!(observed_worker_desktop(), first);
    let concurrent: Vec<std::thread::JoinHandle<String>> = (0..4)
        .map(|_| std::thread::spawn(observed_worker_desktop))
        .collect();
    for worker in concurrent {
        assert_eq!(worker.join().expect("probe thread"), first);
    }
}
