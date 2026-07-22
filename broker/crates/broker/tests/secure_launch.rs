#[cfg(windows)]
mod windows_e2e {
    use aexcompat_broker::ExitClassification;
    use aexcompat_broker::sealed_load_tree::{LoadEntry, SealedLoadTree};
    use aexcompat_broker::secure_launch::{SecureLaunchRequest, secure_launch};
    use sha2::{Digest, Sha256};
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    static SECURE_LAUNCH_LOCK: Mutex<()> = Mutex::new(());

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("{prefix}-{:032x}", rand::random::<u128>()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn external_worker_reads_sealed_plugin_and_tree_is_cleaned_after_exit() {
        let _lock = SECURE_LAUNCH_LOCK.lock().unwrap();
        let worker_dir = TempDir::new("aexcompat-secure-launch-worker");
        let worker = build_worker(&worker_dir.0);
        let worker_bytes = fs::read(&worker).unwrap();
        let source = TempDir::new("aexcompat-secure-launch-payload");
        let basename = "fixture.plugin";
        let plugin_source = source.0.join(basename);
        let plugin_bytes = b"authenticated sealed plugin";
        fs::write(&plugin_source, plugin_bytes).unwrap();
        let entry = LoadEntry {
            source: plugin_source,
            relative_basename: basename.into(),
            expected_sha256: Sha256::digest(plugin_bytes).into(),
            expected_size: plugin_bytes.len() as u64,
        };
        let tree = SealedLoadTree::create(entry, vec![]).unwrap();
        let sealed_root = tree.root().to_owned();
        // The worker asserts its cwd against this expected repository path
        // (issue #141: one-shot launches must run from the repository, not
        // from the staging root).
        let before = vec![worker_dir.0.to_string_lossy().into_owned()];
        let after = vec!["after".to_owned()];
        let request = SecureLaunchRequest {
            worker_program: &worker,
            worker_expected_sha256: Sha256::digest(&worker_bytes).into(),
            worker_expected_size: worker_bytes.len() as u64,
            plugin_basename: basename,
            args_before_plugin: &before,
            args_after_plugin: &after,
            repository: &worker_dir.0,
            require_module_audit: false,
        };

        let result = secure_launch(tree, request, Some(Duration::from_secs(10))).unwrap();

        assert_eq!(
            result.classification,
            ExitClassification::Ok,
            "worker result: {result:?}"
        );
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "payload-ok");
        assert!(
            !sealed_root.exists(),
            "tree must be removed after worker exit"
        );
    }

    #[test]
    fn worker_hash_mismatch_never_starts_process_and_cleans_tree() {
        let _lock = SECURE_LAUNCH_LOCK.lock().unwrap();
        let worker_dir = TempDir::new("aexcompat-secure-launch-worker-mismatch");
        let marker = worker_dir.0.join("started.marker");
        let worker = build_marker_worker(&worker_dir.0);
        let tree = plugin_tree(b"authenticated plugin");
        let sealed_root = tree.root().to_owned();
        let before = vec![marker.to_string_lossy().into_owned()];
        let request = SecureLaunchRequest {
            worker_program: &worker,
            worker_expected_sha256: [0; 32],
            worker_expected_size: fs::metadata(&worker).unwrap().len(),
            plugin_basename: "fixture.plugin",
            args_before_plugin: &before,
            args_after_plugin: &[],
            repository: &worker_dir.0,
            require_module_audit: false,
        };

        let error = secure_launch(tree, request, Some(Duration::from_secs(2))).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("trusted worker staging"));
        assert!(!marker.exists(), "hash-rejected worker must never start");
        assert!(
            !sealed_root.exists(),
            "failed launch must clean sealed tree"
        );
    }

    #[test]
    fn tampered_plugin_is_rejected_before_process_can_start() {
        let _lock = SECURE_LAUNCH_LOCK.lock().unwrap();
        let worker_dir = TempDir::new("aexcompat-secure-launch-plugin-tamper");
        let marker = worker_dir.0.join("started.marker");
        let _worker = build_marker_worker(&worker_dir.0);
        let source = TempDir::new("aexcompat-secure-launch-tampered-payload");
        let plugin = source.0.join("fixture.plugin");
        let approved = b"approved plugin";
        fs::write(&plugin, approved).unwrap();
        let entry = LoadEntry {
            source: plugin.clone(),
            relative_basename: "fixture.plugin".into(),
            expected_sha256: Sha256::digest(approved).into(),
            expected_size: approved.len() as u64,
        };
        fs::write(plugin, b"tampered plugin").unwrap();

        let error = SealedLoadTree::create(entry, vec![]).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!marker.exists(), "plugin-rejected worker must never start");
    }

    #[test]
    fn timeout_kills_worker_and_cleans_sealed_and_staged_trees() {
        let _lock = SECURE_LAUNCH_LOCK.lock().unwrap();
        let stages_before = trusted_stage_roots();
        let worker_dir = TempDir::new("aexcompat-secure-launch-timeout");
        let marker = worker_dir.0.join("started.marker");
        let worker = build_marker_worker(&worker_dir.0);
        let worker_bytes = fs::read(&worker).unwrap();
        let tree = plugin_tree(b"authenticated plugin");
        let sealed_root = tree.root().to_owned();
        let before = vec![marker.to_string_lossy().into_owned()];
        let after = vec!["sleep".to_owned()];
        let request = SecureLaunchRequest {
            worker_program: &worker,
            worker_expected_sha256: Sha256::digest(&worker_bytes).into(),
            worker_expected_size: worker_bytes.len() as u64,
            plugin_basename: "fixture.plugin",
            args_before_plugin: &before,
            args_after_plugin: &after,
            repository: &worker_dir.0,
            require_module_audit: false,
        };

        // The deadline must outlive worker startup (restricted-token process
        // creation plus antivirus scanning of the freshly compiled fixture can
        // exceed hundreds of milliseconds) while still firing during the
        // fixture's 30 s sleep. 250 ms raced against startup and flaked.
        let result = secure_launch(tree, request, Some(Duration::from_secs(5))).unwrap();

        assert_eq!(result.classification, ExitClassification::TimeoutKilled);
        assert!(
            marker.exists(),
            "timeout fixture must prove the worker started"
        );
        assert!(!sealed_root.exists(), "timeout must clean sealed tree");
        let leaked_stages: Vec<_> = trusted_stage_roots()
            .difference(&stages_before)
            .cloned()
            .collect();
        assert!(
            leaked_stages.is_empty(),
            "staged roots leaked: {leaked_stages:?}"
        );
    }

    #[test]
    fn modal_ui_worker_is_started_on_a_private_desktop_before_timeout() {
        let _lock = SECURE_LAUNCH_LOCK.lock().unwrap();
        let worker_dir = TempDir::new("aexcompat-secure-launch-modal");
        let worker = build_modal_worker(&worker_dir.0);
        let worker_bytes = fs::read(&worker).unwrap();
        let parent_desktop = current_desktop_name();
        let tree = plugin_tree(b"authenticated plugin");
        let sealed_root = tree.root().to_owned();
        let request = SecureLaunchRequest {
            worker_program: &worker,
            worker_expected_sha256: Sha256::digest(&worker_bytes).into(),
            worker_expected_size: worker_bytes.len() as u64,
            plugin_basename: "fixture.plugin",
            args_before_plugin: &[],
            args_after_plugin: &[],
            repository: &worker_dir.0,
            require_module_audit: false,
        };

        let result = secure_launch(tree, request, Some(Duration::from_secs(5))).unwrap();

        assert_eq!(result.classification, ExitClassification::TimeoutKilled);
        assert!(
            result
                .stdout
                .lines()
                .any(|line| line.starts_with("desktop=AEXCompatWorkerDesktop-")),
            "worker desktop was not reported: {}",
            result.stdout
        );
        assert!(
            !result
                .stdout
                .lines()
                .any(|line| line == format!("desktop={parent_desktop}")),
            "modal worker inherited the broker desktop: {}",
            result.stdout
        );
        assert!(!sealed_root.exists(), "timeout must clean sealed tree");
    }

    fn trusted_stage_roots() -> HashSet<PathBuf> {
        fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("aexcompat-trusted-worker-")
            })
            .map(|entry| entry.path())
            .collect()
    }

    fn plugin_tree(bytes: &[u8]) -> SealedLoadTree {
        let source = std::env::temp_dir().join(format!(
            "aexcompat-secure-launch-tree-source-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&source).unwrap();
        let plugin = source.join("fixture.plugin");
        fs::write(&plugin, bytes).unwrap();
        let tree = SealedLoadTree::create(
            LoadEntry {
                source: plugin,
                relative_basename: "fixture.plugin".into(),
                expected_sha256: Sha256::digest(bytes).into(),
                expected_size: bytes.len() as u64,
            },
            vec![],
        )
        .unwrap();
        fs::remove_dir_all(source).unwrap();
        tree
    }

    fn build_marker_worker(dir: &Path) -> PathBuf {
        let source = dir.join("marker_worker.rs");
        let executable = dir.join("marker_worker.exe");
        fs::write(
            &source,
            r#"fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    std::fs::write(&args[0], b"started").unwrap();
    if args.get(2).is_some_and(|arg| arg == "sleep") {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}"#,
        )
        .unwrap();
        let status = std::process::Command::new("rustc")
            .arg(&source)
            .args(["-C", "target-feature=+crt-static"])
            .arg("-o")
            .arg(&executable)
            .status()
            .expect("run rustc for marker worker");
        assert!(status.success(), "marker worker build failed");
        executable
    }

    fn current_desktop_name() -> String {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentThreadId() -> u32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetThreadDesktop(thread_id: u32) -> *mut std::ffi::c_void;
            fn GetUserObjectInformationW(
                object: *mut std::ffi::c_void,
                index: i32,
                buffer: *mut std::ffi::c_void,
                length: u32,
                required: *mut u32,
            ) -> i32;
        }
        unsafe {
            let desktop = GetThreadDesktop(GetCurrentThreadId());
            let mut required = 0u32;
            GetUserObjectInformationW(desktop, 2, std::ptr::null_mut(), 0, &mut required);
            let mut buffer = vec![0u16; (required as usize / 2).saturating_add(1)];
            assert_ne!(
                GetUserObjectInformationW(
                    desktop,
                    2,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * 2) as u32,
                    &mut required,
                ),
                0
            );
            let end = buffer.iter().position(|value| *value == 0).unwrap_or(0);
            String::from_utf16(&buffer[..end]).unwrap()
        }
    }

    fn build_modal_worker(dir: &Path) -> PathBuf {
        let source = dir.join("modal_worker.rs");
        let executable = dir.join("modal_worker.exe");
        fs::write(
            &source,
            r#"
use std::ffi::c_void;
use std::io::Write;
use std::ptr::null_mut;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetThreadDesktop(thread_id: u32) -> *mut c_void;
    fn GetUserObjectInformationW(
        object: *mut c_void,
        index: i32,
        buffer: *mut c_void,
        length: u32,
        required: *mut u32,
    ) -> i32;
    fn MessageBoxW(hwnd: *mut c_void, text: *const u16, title: *const u16, kind: u32) -> i32;
}

fn desktop_name() -> String {
    unsafe {
        let desktop = GetThreadDesktop(GetCurrentThreadId());
        let mut required = 0u32;
        GetUserObjectInformationW(desktop, 2, null_mut(), 0, &mut required);
        let mut buffer = vec![0u16; (required as usize / 2).saturating_add(1)];
        assert_ne!(
            GetUserObjectInformationW(
                desktop,
                2,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 2) as u32,
                &mut required,
            ),
            0
        );
        let end = buffer.iter().position(|value| *value == 0).unwrap_or(0);
        String::from_utf16(&buffer[..end]).unwrap()
    }
}

fn main() {
    println!("desktop={}", desktop_name());
    std::io::stdout().flush().unwrap();
    let text: Vec<u16> = "AEXCompat modal fixture".encode_utf16().chain([0]).collect();
    let title: Vec<u16> = "noninteractive worker".encode_utf16().chain([0]).collect();
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), 0); }
    loop { std::thread::sleep(std::time::Duration::from_secs(30)); }
}
"#,
        )
        .unwrap();
        let status = std::process::Command::new("rustc")
            .arg(&source)
            .args(["-C", "target-feature=+crt-static"])
            .arg("-o")
            .arg(&executable)
            .status()
            .expect("run rustc for modal worker");
        assert!(status.success(), "modal worker build failed");
        executable
    }

    fn build_worker(dir: &Path) -> PathBuf {
        let source = dir.join("dummy_secure_worker.rs");
        let executable = dir.join("dummy_secure_worker.exe");
        fs::write(
            &source,
            r#"fn main() {
    let mut all_args = std::env::args_os();
    let argv0 = all_args.next().unwrap();
    let args: Vec<_> = all_args.collect();
    let argv0 = std::path::PathBuf::from(argv0);
    assert!(argv0.is_absolute());
    // The executed binary is the staged copy, but the working directory is
    // the repository the broker passed as args[0] (issue #141), so relative
    // target/image-transport pins resolve against broker-owned transport.
    assert_eq!(argv0.file_name().unwrap(), "trusted-worker.exe");
    assert_eq!(args.len(), 3);
    let current = std::env::current_dir().unwrap();
    assert_eq!(current, std::path::PathBuf::from(&args[0]));
    assert_ne!(argv0.parent(), Some(current.as_path()));
    assert_eq!(args[2], "after");
    let bytes = std::fs::read(&args[1]).expect("read sealed plugin");
    assert_eq!(bytes, b"authenticated sealed plugin");
    println!("payload-ok");
}"#,
        )
        .unwrap();
        let status = std::process::Command::new("rustc")
            .arg(&source)
            .args(["-C", "target-feature=+crt-static"])
            .arg("-o")
            .arg(&executable)
            .status()
            .expect("run rustc for secure launch dummy worker");
        assert!(status.success(), "secure launch dummy worker build failed");
        executable
    }
}
