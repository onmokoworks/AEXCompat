#[allow(dead_code)]
#[path = "../examples/aex_image_probe.rs"]
mod aex_image_probe;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use aex_image_probe::run_probe_request_text;
use serde_json::Value;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn target_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-image-probe")
        .join(name)
}

#[cfg(windows)]
fn create_dir_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(original, link)
}

#[cfg(unix)]
fn create_dir_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

static WORKER_BUILD_LOCK: Mutex<()> = Mutex::new(());

fn build_test_worker(mode: &str) -> PathBuf {
    let _guard = WORKER_BUILD_LOCK.lock().unwrap();
    let root = target_path("test-workers");
    std::fs::create_dir_all(&root).unwrap();
    let exe_suffix = if cfg!(windows) { ".exe" } else { "" };
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let exe = root.join(format!(
        "aex_effect_worker_stub_{mode}_{}_{}{exe_suffix}",
        std::process::id(),
        unique
    ));

    let source = root.join(format!(
        "aex_effect_worker_stub_{mode}_{}_{}.rs",
        std::process::id(),
        unique
    ));
    std::fs::write(
        &source,
        format!(
            r##"
const MODE: &str = "{mode}";

fn json_escape(value: &str) -> String {{
    value.replace('\\', "\\\\").replace('"', "\\\"")
}}

fn normalize_path_text(path: &str) -> String {{
    path.replace('\\', "/").to_ascii_lowercase()
}}

fn sandbox_attestation_json() -> String {{
    let current_dir = std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let worker_exe_name = std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
        .unwrap_or_default();
    let generated_root_confined = normalize_path_text(&current_dir)
        .contains("/target/aex-image-probe/");
    let shell_env_name = ["Com", "Spec"].concat();
    let inheritance_sentinel = inheritance_sentinel_arg();
    let sentinel_inherited = inheritance_sentinel
        .as_deref()
        .and_then(inheritance_sentinel_inherited);
    let handle_inheritance_disabled =
        inheritance_sentinel.is_some() && sentinel_inherited == Some(false);
    format!(
        r#""sandbox_attestation":{{{{"schema_version":1,"platform":"{{}}","current_dir":"{{}}","generated_root_confined":{{}},"env_path_absent":{{}},"env_comspec_absent":{{}},"env_count":{{}},"stdin_contract":"null","worker_exe_name":"{{}}","inheritance_sentinel_provided":{{}},"inheritance_sentinel_inherited":{{}},"handle_inheritance_disabled":{{}}}}}}"#,
        std::env::consts::OS,
        json_escape(&current_dir),
        generated_root_confined,
        std::env::var_os("PATH").is_none(),
        std::env::var_os(shell_env_name).is_none(),
        std::env::vars_os().count(),
        json_escape(&worker_exe_name),
        inheritance_sentinel.is_some(),
        sentinel_inherited
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_owned()),
        handle_inheritance_disabled
    )
}}

fn inheritance_sentinel_arg() -> Option<String> {{
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|window| window[0] == "--inheritance-sentinel")
        .map(|window| window[1].clone())
}}

#[cfg(windows)]
fn inheritance_sentinel_inherited(raw: &str) -> Option<bool> {{
    let handle_value = raw.parse::<usize>().ok()?;
    let mut flags = 0u32;
    #[link(name = "kernel32")]
    extern "system" {{
        fn GetHandleInformation(hobject: *mut std::ffi::c_void, lpdwflags: *mut u32) -> i32;
    }}
    let ok = unsafe {{ GetHandleInformation(handle_value as *mut _, &mut flags) != 0 }};
    Some(ok)
}}

#[cfg(not(windows))]
fn inheritance_sentinel_inherited(_raw: &str) -> Option<bool> {{
    None
}}

fn arg_value(args: &[String], name: &str) -> Option<String> {{
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}}

fn wait_for_file(path: &std::path::Path) {{
    for _ in 0..100 {{
        if path.exists() {{
            return;
        }}
        std::thread::sleep(std::time::Duration::from_millis(10));
    }}
}}

fn main() {{
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|arg| arg == "--handshake") {{
        std::process::exit(2);
    }}
    if let Some(pid_path) = arg_value(&args, "--descendant-pid") {{
        let _ = std::fs::write(&pid_path, std::process::id().to_string());
        std::thread::sleep(std::time::Duration::from_secs(30));
        return;
    }}
    match MODE {{
        "timeout" => {{
            std::thread::sleep(std::time::Duration::from_millis(500));
        }}
        "spawn_descendant_timeout" => {{
            let pid_path = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("descendant.pid");
            let _child = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--handshake")
                .arg("--descendant-pid")
                .arg(&pid_path)
                .spawn()
                .expect("failed to spawn descendant");
            wait_for_file(&pid_path);
            std::thread::sleep(std::time::Duration::from_secs(30));
        }}
        "noisy_crash" => {{
            println!(
                "worker stdout leak D:/Example/Projects/Fixture/ClassicTest.aex\n{{}}",
                "A".repeat(4096)
            );
            eprintln!(
                "worker stderr leak C:/Users/Example/private/input.aepx\n{{}}",
                "B".repeat(4096)
            );
            std::process::exit(9);
        }}
        "crash" => {{
            eprintln!("synthetic crash");
            std::process::exit(7);
        }}
        "malformed" => {{
            println!("not-json");
        }}
        "bad_protocol" => {{
            println!(
                "{{}}",
                r#"{{"worker_protocol_version":2,"status":"worker_ready","aex_loading":"disabled"}}"#
            );
        }}
        "enabled_without_revalidation" => {{
            println!(
                "{{}}",
                r#"{{"worker_protocol_version":1,"status":"worker_ready","aex_loading":"enabled","transport_status":"validated","worker":"test"}}"#
            );
        }}
        "revalidation_absent" => {{
            println!(
                "{{{{\"worker_protocol_version\":1,\"status\":\"worker_ready\",\"aex_loading\":\"disabled\",\"transport_status\":\"validated\",\"worker_revalidation\":{{{{\"status\":\"absent\"}}}},{{}},\"worker\":\"test\"}}}}",
                sandbox_attestation_json()
            );
        }}
        "revalidation_passed" => {{
            if args.iter().any(|arg| arg == "--identity-manifest") {{
                let loader_ticket = if args.iter().any(|arg| arg == "--loader-ticket") {{
                    r#","loader_ticket":{{"status":"accepted_no_load","allowlist_id":"classic-test","native_load_performed":false,"worker_may_load_plugin":false}}"#
                }} else {{
                    ""
                }};
                println!(
                    "{{{{\"worker_protocol_version\":1,\"status\":\"worker_ready\",\"aex_loading\":\"disabled\",\"transport_status\":\"validated\",\"worker_revalidation\":{{{{\"status\":\"passed\",\"allowlist_id\":\"classic-test\"}}}}{{}},{{}},\"worker\":\"test\"}}}}",
                    loader_ticket,
                    sandbox_attestation_json()
                );
            }} else {{
                println!(
                    "{{{{\"worker_protocol_version\":1,\"status\":\"worker_ready\",\"aex_loading\":\"disabled\",\"transport_status\":\"validated\",\"worker_revalidation\":{{{{\"status\":\"absent\"}}}},{{}},\"worker\":\"test\"}}}}",
                    sandbox_attestation_json()
                );
            }}
        }}
        "no_transport" => {{
            println!(
                "{{{{\"worker_protocol_version\":1,\"status\":\"worker_ready\",\"aex_loading\":\"disabled\",{{}},\"worker\":\"test\"}}}}",
                sandbox_attestation_json()
            );
        }}
        _ => {{
            if args.iter().any(|arg| arg == "--transport-manifest") {{
                println!(
                    "{{{{\"worker_protocol_version\":1,\"status\":\"worker_ready\",\"aex_loading\":\"disabled\",\"transport_status\":\"validated\",{{}},\"worker\":\"test\"}}}}",
                    sandbox_attestation_json()
                );
            }} else {{
                println!(
                    "{{{{\"worker_protocol_version\":1,\"status\":\"worker_ready\",\"aex_loading\":\"disabled\",\"transport_status\":\"not_provided\",{{}},\"worker\":\"test\"}}}}",
                    sandbox_attestation_json()
                );
            }}
        }}
    }}
}}
"##
        ),
    )
    .unwrap();

    let status = Command::new("rustc")
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .status()
        .unwrap_or_else(|err| panic!("failed to run rustc for test worker: {err}"));
    assert!(status.success(), "rustc failed for {}", source.display());
    exe
}

fn classic_render_request(worker_exe: Option<&Path>, launch_timeout_ms: Option<u64>) -> String {
    let worker_field = worker_exe
        .map(|path| {
            format!(
                r#","worker_exe":"{}""#,
                path.to_string_lossy().replace('\\', "/")
            )
        })
        .unwrap_or_default();
    let timeout_field = launch_timeout_ms
        .map(|timeout| {
            format!(
                r#","timeouts_ms":{{"launch":{timeout},"setup":3000,"render":5000,"teardown":1000}}"#
            )
        })
        .unwrap_or_default();

    format!(
        r#"{{
        "schema_version": 1,
        "operation": "render_png",
        "plugin_path": "D:/AviUtlas/local/ClassicTest.aex",
        "allowlist": "aex_image_probe_allowlist.classic.json",
        "input_png": "input.png",
        "output_png": "target/aex-image-probe/output.png",
        "pixel_format": "rgba8",
        "frame": {{
            "width": 64,
            "height": 64,
            "frame_index": 0,
            "time_seconds": 0.0
        }},
        "limits": {{
            "max_width": 128,
            "max_height": 128,
            "max_bytes": 1048576
        }},
        "params": {{}}{worker_field}{timeout_field}
    }}"#
    )
}

fn identity_transport_request(input_png: &Path, output_png: &Path) -> String {
    format!(
        r#"{{
        "schema_version": 1,
        "operation": "identity_transport",
        "input_png": "{}",
        "output_png": "{}",
        "pixel_format": "rgba8",
        "limits": {{"max_width": 128, "max_height": 128, "max_bytes": 1048576}},
        "params": {{}}
    }}"#,
        path_text(input_png),
        path_text(output_png)
    )
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn loader_preflight_path_key_text(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for part in path.trim().replace('/', "\\").split('\\') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            match parts.last() {
                Some(previous) if !previous.ends_with(':') && previous != ".." => {
                    parts.pop();
                }
                _ => parts.push(part.to_string()),
            }
            continue;
        }
        parts.push(part.to_string());
    }
    parts.join("\\").to_ascii_lowercase()
}

fn sandbox_check<'a>(
    sandbox: &'a aex_image_probe::SandboxPreflightReport,
    name: &str,
) -> &'a aex_image_probe::SandboxCheckReport {
    sandbox
        .checks
        .iter()
        .find(|check| check.name == name)
        .unwrap_or_else(|| panic!("missing sandbox check {name}"))
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 258;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut std::ffi::c_void;
        fn WaitForSingleObject(hHandle: *mut std::ffi::c_void, dwMilliseconds: u32) -> u32;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }

    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let wait = unsafe { WaitForSingleObject(handle, 0) };
    let _ = unsafe { CloseHandle(handle) };
    wait == WAIT_TIMEOUT
}

#[cfg(windows)]
fn wait_for_process_exit(pid: u32, timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !process_is_running(pid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    !process_is_running(pid)
}

fn write_synthetic_png(name: &str, width: u32, height: u32) -> PathBuf {
    let path = target_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let image = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255])
    });
    image.save(&path).unwrap();
    path
}

fn write_fake_aex(name: &str, size: usize) -> PathBuf {
    let path = target_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, vec![0xA5; size]).unwrap();
    path
}

fn write_classic_allowlist(name: &str, plugin_path: &Path) -> PathBuf {
    write_classic_allowlist_with(name, plugin_path, "local-only", "local-only-reviewed", 4096)
}

fn write_classic_allowlist_with(
    name: &str,
    plugin_path: &Path,
    publication_status: &str,
    license_status: &str,
    max_plugin_bytes: u64,
) -> PathBuf {
    let path = target_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let canonical =
        std::fs::canonicalize(plugin_path).unwrap_or_else(|_| plugin_path.to_path_buf());
    let metadata = plugin_path.metadata().ok();
    let observed_size = metadata
        .as_ref()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let allowlist = serde_json::json!({
        "schema_version": 1,
        "allowlist_publication_status": "local-only",
        "default_max_plugin_bytes": 268435456u64,
        "entries": [{
            "id": "classic-test",
            "plugin_path": path_text(plugin_path),
            "canonical_plugin_path": path_text(&canonical),
            "expected_class": "classic-effect",
            "allowed_operations": ["describe", "render_png"],
            "max_width": 128,
            "max_height": 128,
            "timeout_ms": 1000,
            "fixture_status": "local-build-candidate",
            "publication_status": publication_status,
            "license_status": license_status,
            "classifier_status": "candidate_for_contract_probe",
            "classifier_inferred_class": "classic-effect",
            "max_plugin_bytes": max_plugin_bytes,
            "observed_size_bytes": observed_size
        }],
        "blocked_classes": ["aegp", "aeio", "smartfx-only", "gpu-only", "unknown"]
    });
    std::fs::write(&path, serde_json::to_string_pretty(&allowlist).unwrap()).unwrap();
    path
}

fn write_classic_allowlist_with_loader_gate(
    name: &str,
    plugin_path: &Path,
    loader_approval_status: Option<&str>,
    sandbox_profile_status: Option<&str>,
    worker_revalidation_status: Option<&str>,
) -> PathBuf {
    let path = write_classic_allowlist(name, plugin_path);
    let mut allowlist: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    if let Some(status) = loader_approval_status {
        allowlist["entries"][0]["loader_approval_status"] = serde_json::json!(status);
    }
    if let Some(status) = sandbox_profile_status {
        allowlist["entries"][0]["sandbox_profile_status"] = serde_json::json!(status);
    }
    if let Some(status) = worker_revalidation_status {
        allowlist["entries"][0]["worker_revalidation_status"] = serde_json::json!(status);
    }
    std::fs::write(&path, serde_json::to_string_pretty(&allowlist).unwrap()).unwrap();
    path
}

fn classic_render_request_with_paths(
    plugin_path: &Path,
    allowlist: &Path,
    input_png: &Path,
    output_png: &Path,
    worker_exe: Option<&Path>,
    launch_timeout_ms: Option<u64>,
) -> String {
    let worker_field = worker_exe
        .map(|path| format!(r#","worker_exe":"{}""#, path_text(path)))
        .unwrap_or_default();
    let timeout_field = launch_timeout_ms
        .map(|timeout| {
            format!(
                r#","timeouts_ms":{{"launch":{timeout},"setup":3000,"render":5000,"teardown":1000}}"#
            )
        })
        .unwrap_or_default();
    format!(
        r#"{{
        "schema_version": 1,
        "operation": "render_png",
        "plugin_path": "{}",
        "allowlist": "{}",
        "input_png": "{}",
        "output_png": "{}",
        "pixel_format": "rgba8",
        "frame": {{"width": 64, "height": 64, "frame_index": 0, "time_seconds": 0.0}},
        "limits": {{"max_width": 128, "max_height": 128, "max_bytes": 1048576}},
        "params": {{}}{worker_field}{timeout_field}
    }}"#,
        path_text(plugin_path),
        path_text(allowlist),
        path_text(input_png),
        path_text(output_png)
    )
}

fn add_loader_intent(
    request_text: &str,
    approval_status: Option<&str>,
    sandbox_profile: Option<&str>,
    worker_revalidation: Option<&str>,
) -> String {
    let mut request: serde_json::Value = serde_json::from_str(request_text).unwrap();
    let mut intent = serde_json::json!({
        "request_real_aex_load": true
    });
    if let Some(status) = approval_status {
        intent["approval_status"] = serde_json::json!(status);
    }
    if let Some(profile) = sandbox_profile {
        intent["sandbox_profile"] = serde_json::json!(profile);
    }
    if let Some(revalidation) = worker_revalidation {
        intent["worker_revalidation"] = serde_json::json!(revalidation);
    }
    request["loader_intent"] = intent;
    serde_json::to_string(&request).unwrap()
}

fn add_loader_preflight(request_text: &str, preflight: &Path) -> String {
    let mut request: serde_json::Value = serde_json::from_str(request_text).unwrap();
    request["loader_preflight"] = serde_json::json!(path_text(preflight));
    serde_json::to_string(&request).unwrap()
}

fn write_loader_preflight_evidence(name: &str, plugin_path: &Path, allowlist_id: &str) -> PathBuf {
    write_loader_preflight_evidence_with_ids(name, plugin_path, allowlist_id, allowlist_id)
}

fn write_loader_preflight_evidence_with_ids(
    name: &str,
    plugin_path: &Path,
    fixture_id: &str,
    allowlist_id: &str,
) -> PathBuf {
    let path = target_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let plugin_path_text = path_text(plugin_path);
    let normalized_plugin_path = loader_preflight_path_key_text(&plugin_path_text);
    let evidence = serde_json::json!({
        "schema_version": 1,
        "publication_status": "local-only",
        "status": "preflight_passed_no_load",
        "preflight_passed": true,
        "native_load_performed": false,
        "broker_may_load_plugin": false,
        "selected_fixture": fixture_id,
        "selected_candidate": {
            "id": fixture_id,
            "display_name": "ClassicTest",
            "plugin_path": plugin_path_text,
            "normalized_plugin_path": normalized_plugin_path,
            "loader_gate_plugin_path": plugin_path_text,
            "loader_gate_effect_id": allowlist_id,
            "path_match_status": "matched_normalized_path"
        },
        "selected_loader_entry": {
            "effect_id": allowlist_id,
            "plugin_path": plugin_path_text,
            "normalized_plugin_path": normalized_plugin_path,
            "path_match_status": "matched_normalized_path",
            "pre_loader_status": "approved-local-only",
            "loader_approval_status": "approved-local-only",
            "allowlist_operation_status": "render_png",
            "handle_inheritance_required": "sentinel_not_inherited-with-explicit-handle-list",
            "worker_identity_revalidation_required": "passed",
            "worker_attestation_required": "passed",
            "sandbox_preflight_required": "passed",
            "job_object_required": "assigned-with-kill-on-close",
            "entry_ready": true
        },
        "fixture_gate": {
            "status": "approved-local-only",
            "selected_fixture": fixture_id,
            "approval_approved": true,
            "approval_loader_enabled": true,
            "approval_real_aex_load_enabled": true,
            "approval_render_png_enabled": true,
            "approval_describe_enabled_for_real_aex": true,
            "candidate_count": 1
        },
        "loader_gate": {
            "status": "loader_gate_opened_for_single_candidate",
            "approved": true,
            "loader_enabled": true,
            "real_aex_load_enabled": true,
            "open_candidate_count": 1,
            "entry_count": 1
        },
        "checks": [
            {"name": "fixture_gate_schema_version", "status": "passed"},
            {"name": "loader_gate_schema_version", "status": "passed"},
            {"name": "fixture_gate_unique_candidates", "status": "passed"},
            {"name": "loader_gate_unique_entries", "status": "passed"},
            {"name": "selected_fixture", "status": "passed"},
            {"name": "selected_fixture_metadata", "status": "passed"},
            {"name": "fixture_gate_approval", "status": "passed"},
            {"name": "loader_gate_single_ready_entry", "status": "passed"},
            {"name": "selected_fixture_in_loader_gate", "status": "passed"},
            {"name": "loader_gate_open", "status": "passed"},
            {"name": "selected_loader_entry", "status": "passed"}
        ],
        "blocked_reasons": [],
        "next_action": "separate explicit loader implementation slice",
        "notes": [
            "Preflight reads JSON metadata only.",
            "A passing preflight is permission to start a separate loader slice, not proof that native loading is implemented."
        ]
    });
    std::fs::write(&path, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    path
}

fn rgba_transport_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rgba8"))
                .collect()
        })
        .unwrap_or_default()
}

fn transport_manifest_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| {
                            name.starts_with("worker-transport-") && name.ends_with(".json")
                        })
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn identity_manifest_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.starts_with("worker-identity-") && name.ends_with(".json"))
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn loader_ticket_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| {
                            name.starts_with("worker-loader-ticket-") && name.ends_with(".json")
                        })
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|err| panic!("failed to read fixture {name}: {err}"))
}

fn assert_no_payload_fields(value: &Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let lowered = key.to_ascii_lowercase();
                assert!(
                    !matches!(
                        lowered.as_str(),
                        "hash"
                            | "sha256"
                            | "payload"
                            | "contents"
                            | "binary"
                            | "file_bytes"
                            | "bytes_base64"
                            | "image_bytes"
                    ),
                    "report should not include payload/hash key: {key}"
                );
                assert_no_payload_fields(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_payload_fields(item);
            }
        }
        _ => {}
    }
}

#[test]
fn blocked_request_returns_allowlist_denied_report() {
    let request_text = read_fixture("aex_image_probe_request.blocked.json");
    let request_path = fixture_path("aex_image_probe_request.blocked.json");

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    let report_json = serde_json::to_value(&report).unwrap();

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.status, "allowlist_denied");
    assert_eq!(
        report.plugin_path.as_deref(),
        Some("D:/AviUtlas/local/NotAllowlisted.aex")
    );
    assert_eq!(report.plugin_class, "unknown");
    assert!(report.output_png.is_none());
    assert!(report.crash.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("not present in allowlist")));
    assert_no_payload_fields(&report_json);
}

#[test]
fn invalid_requests_return_structured_invalid_request_reports() {
    for (request, expected_warning) in [
        (
            r#"{"schema_version":2,"operation":"catalog"}"#,
            "schema_version",
        ),
        (
            r#"{"schema_version":1,"operation":"execute"}"#,
            "unsupported operation",
        ),
        (
            r#"{"schema_version":1,"operation":"describe","plugin_path":"x.aex"}"#,
            "absolute",
        ),
        (
            r#"{"schema_version":1,"operation":"describe","plugin_path":"D:/AviUtlas/local/Test.aex"}"#,
            "allowlist",
        ),
        (
            r#"{"schema_version":1,"operation":"render_png","plugin_path":"D:/AviUtlas/local/Test.aex","allowlist":"missing.json"}"#,
            "input_png",
        ),
        (
            r#"{"schema_version":1,"operation":"catalog","params":[]}"#,
            "params",
        ),
        (
            r#"{"schema_version":1,"operation":"catalog","timeouts_ms":{"render":0}}"#,
            "timeout",
        ),
    ] {
        let report = run_probe_request_text(request, None).unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains(expected_warning)),
            "expected warning containing {expected_warning:?}, got {:?}",
            report.warnings
        );
    }
}

#[test]
fn identity_transport_copies_synthetic_rgba8_without_worker_or_aex() {
    let input = write_synthetic_png("identity-transport/input.png", 16, 16);
    let output = target_path("identity-transport/output.png");
    let _ = std::fs::remove_file(&output);

    let report =
        run_probe_request_text(&identity_transport_request(&input, &output), None).unwrap();

    assert_eq!(report.status, "ok");
    assert_eq!(report.plugin_path, None);
    assert_eq!(report.plugin_class, "identity-transport");
    assert_eq!(
        report.output_png.as_deref(),
        Some(path_text(&output).as_str())
    );
    assert!(report.identity_preflight.is_none());
    assert!(report.loader_approval.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.worker_loader_ticket.is_none());
    assert!(report.sandbox_preflight.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("no .aex loaded or rendered")));
    assert!(report
        .unsupported
        .iter()
        .any(|warning| warning.contains("not AEX render correctness evidence")));

    let input_rgba = image::open(&input).unwrap().to_rgba8();
    let output_rgba = image::open(&output).unwrap().to_rgba8();
    assert_eq!(output_rgba.dimensions(), (16, 16));
    assert_eq!(output_rgba.as_raw(), input_rgba.as_raw());
}

#[test]
fn identity_transport_rejects_output_outside_crate_generated_root() {
    let input = write_synthetic_png("identity-transport/reject/input.png", 8, 8);
    let sibling_prefix = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("aex-image-probe-evil")
        .join("output.png");
    let _ = std::fs::remove_file(&sibling_prefix);

    let report =
        run_probe_request_text(&identity_transport_request(&input, &sibling_prefix), None).unwrap();

    assert_eq!(report.status, "invalid_request");
    assert!(!sibling_prefix.exists());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("target/aex-image-probe")));
}

#[test]
fn identity_transport_rejects_output_through_generated_root_symlink_escape() {
    let input = write_synthetic_png("identity-transport/reject-link/input.png", 8, 8);
    let escape_root = target_path("identity-transport/reject-link-outside");
    let link = target_path("identity-transport/reject-link");
    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir_all(&escape_root);
    std::fs::create_dir_all(&escape_root).unwrap();
    if create_dir_symlink(&escape_root, &link).is_err() {
        return;
    }
    let output = link.join("output.png");

    let report =
        run_probe_request_text(&identity_transport_request(&input, &output), None).unwrap();

    assert_eq!(report.status, "invalid_request");
    assert!(!escape_root.join("output.png").exists());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("target/aex-image-probe")));

    let _ = std::fs::remove_dir(&link);
    let _ = std::fs::remove_file(&link);
}

#[test]
fn allowlisted_classic_effect_requires_explicit_worker_exe() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let request_text = classic_render_request(None, None);

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "invalid_request");
    assert_eq!(report.plugin_class, "classic-effect");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(
        report.output_png.is_none(),
        "failed worker-launch reports should not claim an output image"
    );
    assert!(report
        .warnings
        .iter()
        .any(|item| item.contains("worker_exe")));
}

#[test]
fn worker_exe_path_must_be_absolute_existing_and_reviewed() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    for (worker_path, expected_warning) in [
        (
            PathBuf::from("aex_effect_worker_stub_ready.exe"),
            "absolute",
        ),
        (
            target_path("missing/aex_effect_worker_stub_ready.exe"),
            "existing file",
        ),
        (target_path("test-workers"), "existing file"),
    ] {
        let request_text = classic_render_request(Some(&worker_path), None);
        let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
        assert_eq!(report.status, "invalid_request");
        assert!(report.output_png.is_none());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains(expected_warning)),
            "expected warning containing {expected_warning:?}, got {:?}",
            report.warnings
        );
    }

    let bad_name = target_path(if cfg!(windows) {
        "test-workers/not_the_reviewed_worker.exe"
    } else {
        "test-workers/not_the_reviewed_worker"
    });
    if let Some(parent) = bad_name.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&bad_name, b"not executable").unwrap();
    let request_text = classic_render_request(Some(&bad_name), None);
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("allowlisted AEX worker stub filename")));

    let bad_dynamic_name = target_path(if cfg!(windows) {
        "test-workers/aex_effect_worker_stub_evil.exe"
    } else {
        "test-workers/aex_effect_worker_stub_evil"
    });
    if let Some(parent) = bad_dynamic_name.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&bad_dynamic_name, b"not executable").unwrap();
    let request_text = classic_render_request(Some(&bad_dynamic_name), None);
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report.output_png.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("allowlisted AEX worker stub filename")));

    let bad_dynamic_pid = target_path(if cfg!(windows) {
        "test-workers/aex_effect_worker_stub_ready_0_1.exe"
    } else {
        "test-workers/aex_effect_worker_stub_ready_0_1"
    });
    if let Some(parent) = bad_dynamic_pid.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&bad_dynamic_pid, b"not executable").unwrap();
    let request_text = classic_render_request(Some(&bad_dynamic_pid), None);
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report.output_png.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("allowlisted AEX worker stub filename")));

    let bad_prefix_name = target_path(if cfg!(windows) {
        "test-workers/aex_effect_worker_stubevil.exe"
    } else {
        "test-workers/aex_effect_worker_stubevil"
    });
    if let Some(parent) = bad_prefix_name.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&bad_prefix_name, b"not executable").unwrap();
    let request_text = classic_render_request(Some(&bad_prefix_name), None);
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report.output_png.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("allowlisted AEX worker stub filename")));
}

#[test]
fn worker_launch_enforces_allowlist_identity_gate() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let worker = build_test_worker("ready");
    let input = write_synthetic_png("identity/gate/input.png", 64, 64);
    let output = target_path("identity/gate/output.png");

    let missing_plugin = target_path("identity/gate/missing.aex");
    let missing_allowlist = write_classic_allowlist_with(
        "identity/gate/missing-allowlist.json",
        &missing_plugin,
        "local-only",
        "local-only-reviewed",
        4096,
    );
    let request_text = classic_render_request_with_paths(
        &missing_plugin,
        &missing_allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report.output_png.is_none());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("existing file")));

    let plugin = write_fake_aex("identity/gate/UnknownStatus.aex", 32);
    let unknown_allowlist = write_classic_allowlist_with(
        "identity/gate/unknown-allowlist.json",
        &plugin,
        "unknown",
        "local-only-reviewed",
        4096,
    );
    let request_text = classic_render_request_with_paths(
        &plugin,
        &unknown_allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .identity_preflight
        .as_ref()
        .and_then(|identity| identity.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("publication_status"));

    let oversized = write_fake_aex("identity/gate/Oversized.aex", 64);
    let oversized_allowlist = write_classic_allowlist_with(
        "identity/gate/oversized-allowlist.json",
        &oversized,
        "local-only",
        "local-only-reviewed",
        4,
    );
    let request_text = classic_render_request_with_paths(
        &oversized,
        &oversized_allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("max_plugin_bytes")));
}

#[test]
fn identity_preflight_reports_metadata_without_hashes_or_payloads() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("identity/report/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("identity/report/allowlist.json", &plugin);
    let input = write_synthetic_png("identity/report/input.png", 64, 64);
    let output = target_path("identity/report/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("ready");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    let report_json = serde_json::to_value(&report).unwrap();
    let identity = report.identity_preflight.as_ref().unwrap();

    assert_eq!(identity.status, "allowed");
    assert_eq!(identity.observed_size_bytes, Some(32));
    assert!(identity.observed_modified_unix_ms.is_some());
    assert_no_payload_fields(&report_json);
    assert!(!serde_json::to_string(&report_json)
        .unwrap()
        .contains("sha256"));
}

#[test]
fn worker_launch_reports_measured_sandbox_preflight() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("sandbox/preflight/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("sandbox/preflight/allowlist.json", &plugin);
    let input = write_synthetic_png("sandbox/preflight/input.png", 64, 64);
    let output = target_path("sandbox/preflight/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("ready");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.output_png.is_none());
    let sandbox = report.sandbox_preflight.as_ref().unwrap();
    assert!(sandbox.performed);
    assert_eq!(sandbox.profile, "windows-job-object-v0");
    assert!(sandbox.explicit_worker_path);
    assert!(sandbox.no_shell);
    assert!(sandbox.environment_sanitized);
    assert!(sandbox.bounded_stdio);
    assert!(sandbox.controlled_working_directory);
    assert!(sandbox.generated_root_confined);
    assert!(!sandbox.network_required);
    let attestation = sandbox.worker_attestation.as_ref().unwrap();
    assert_eq!(attestation.status, "passed");
    assert!(attestation.current_dir_matches_broker);
    assert!(attestation.generated_root_confined);
    assert!(attestation.env_path_absent);
    assert!(attestation.env_comspec_absent);
    assert_eq!(attestation.stdin_contract.as_deref(), Some("null"));
    if cfg!(windows) {
        assert!(attestation.inheritance_sentinel_provided);
        assert_eq!(attestation.inheritance_sentinel_inherited, Some(false));
        assert!(attestation.handle_inheritance_disabled);
        assert!(sandbox.handle_inheritance_disabled);
        assert_eq!(sandbox.handle_inheritance_status, "sentinel_not_inherited");
        assert_eq!(
            sandbox_check(sandbox, "handle_inheritance").status,
            "measured_pass"
        );
    } else {
        assert!(!attestation.inheritance_sentinel_provided);
        assert_eq!(attestation.inheritance_sentinel_inherited, None);
        assert!(!attestation.handle_inheritance_disabled);
        assert_eq!(sandbox.handle_inheritance_status, "not_applicable");
        assert_eq!(
            sandbox_check(sandbox, "handle_inheritance").status,
            "not_applicable"
        );
    }
    assert!(attestation
        .current_dir
        .as_deref()
        .unwrap_or_default()
        .replace('\\', "/")
        .contains("/target/aex-image-probe/"));
    assert_eq!(
        sandbox_check(sandbox, "worker_sandbox_attestation").status,
        "measured_pass"
    );
    assert_eq!(
        sandbox_check(sandbox, "environment_sanitized").status,
        "measured_pass"
    );
    assert_eq!(
        sandbox_check(sandbox, "controlled_working_directory").status,
        "measured_pass"
    );
    assert!(sandbox
        .checks
        .iter()
        .any(|check| check.name == "job_object_kill_on_close"));
    if cfg!(windows) {
        assert!(sandbox.job_object_attempted);
        assert!(matches!(
            sandbox.job_object_status.as_str(),
            "assigned" | "assign_failed" | "create_failed" | "set_limit_failed"
        ));
        if sandbox.job_object_assigned {
            assert_eq!(sandbox.status, "passed");
            assert!(sandbox.kill_on_job_close);
        } else {
            assert_eq!(sandbox.status, "failed");
        }
    } else {
        assert_eq!(sandbox.job_object_status, "unsupported_platform");
        assert_eq!(sandbox.status, "failed");
    }

    let approval = report.loader_approval.as_ref().unwrap();
    assert!(!approval.approved);
    assert!(!approval.loader_enabled);
    assert!(!approval.real_aex_load_enabled);
    assert_eq!(approval.job_object, sandbox.job_object_assigned);
    assert_eq!(
        approval.handle_inheritance_disabled,
        sandbox.handle_inheritance_disabled
    );
    assert_eq!(
        approval.controlled_working_directory,
        sandbox.controlled_working_directory
    );
}

#[test]
fn real_load_intent_without_approval_fails_closed_before_worker_launch() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/missing-approval/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("loader-gate/missing-approval/allowlist.json", &plugin);
    let input = write_synthetic_png("loader-gate/missing-approval/input.png", 64, 64);
    let output = target_path("loader-gate/missing-approval/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("ready");
    let request_text = add_loader_intent(
        &classic_render_request_with_paths(
            &plugin,
            &allowlist,
            &input,
            &output,
            Some(&worker),
            None,
        ),
        None,
        None,
        None,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.identity_preflight.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let approval = report.loader_approval.as_ref().unwrap();
    assert!(approval.requested);
    assert!(!approval.loader_enabled);
    assert_eq!(approval.status, "denied");
    assert!(approval
        .denied_reason
        .as_deref()
        .unwrap_or_default()
        .contains("approval"));
}

#[test]
fn real_load_intent_rejects_false_or_unknown_approval() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    for approval_status in ["not-approved", "unknown", "draft"] {
        let plugin = write_fake_aex(
            &format!("loader-gate/approval/{approval_status}/ClassicTest.aex"),
            32,
        );
        let allowlist = write_classic_allowlist_with_loader_gate(
            &format!("loader-gate/approval/{approval_status}/allowlist.json"),
            &plugin,
            Some("approved-local-only"),
            Some("implemented-v0"),
            Some("required"),
        );
        let input = write_synthetic_png(
            &format!("loader-gate/approval/{approval_status}/input.png"),
            64,
            64,
        );
        let output = target_path(&format!(
            "loader-gate/approval/{approval_status}/output.png"
        ));
        let _ = std::fs::remove_file(&output);
        let worker = build_test_worker("ready");
        let request_text = add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some(approval_status),
            Some("windows-job-object-v0"),
            Some("required"),
        );

        let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

        assert_eq!(report.status, "worker_protocol_error");
        assert!(report.output_png.is_none());
        assert!(!output.exists());
        assert!(report
            .loader_approval
            .as_ref()
            .and_then(|approval| approval.denied_reason.as_deref())
            .unwrap_or_default()
            .contains("approval"));
    }
}

#[test]
fn real_load_intent_rejects_missing_or_unknown_sandbox() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    for (case, request_sandbox, allowlist_sandbox) in [
        ("missing-request", None, Some("implemented-v0")),
        ("unknown-request", Some("unknown"), Some("implemented-v0")),
        ("missing-allowlist", Some("windows-job-object-v0"), None),
        (
            "unknown-allowlist",
            Some("windows-job-object-v0"),
            Some("unknown"),
        ),
    ] {
        let plugin = write_fake_aex(&format!("loader-gate/sandbox/{case}/ClassicTest.aex"), 32);
        let allowlist = write_classic_allowlist_with_loader_gate(
            &format!("loader-gate/sandbox/{case}/allowlist.json"),
            &plugin,
            Some("approved-local-only"),
            allowlist_sandbox,
            Some("required"),
        );
        let input = write_synthetic_png(&format!("loader-gate/sandbox/{case}/input.png"), 64, 64);
        let output = target_path(&format!("loader-gate/sandbox/{case}/output.png"));
        let _ = std::fs::remove_file(&output);
        let worker = build_test_worker("ready");
        let request_text = add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            request_sandbox,
            Some("required"),
        );

        let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

        assert_eq!(report.status, "worker_protocol_error");
        assert_eq!(report.stage.as_deref(), Some("handshake"));
        assert!(report.entrypoint.is_none());
        assert!(report.output_png.is_none());
        assert!(!output.exists());
        assert!(report
            .loader_approval
            .as_ref()
            .and_then(|approval| approval.denied_reason.as_deref())
            .unwrap_or_default()
            .contains("sandbox"));
    }
}

#[test]
fn real_load_intent_requires_passing_loader_preflight_evidence() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/preflight-required/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/preflight-required/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/preflight-required/input.png", 64, 64);
    let output = target_path("loader-gate/preflight-required/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_passed");
    let request_text = add_loader_intent(
        &classic_render_request_with_paths(
            &plugin,
            &allowlist,
            &input,
            &output,
            Some(&worker),
            None,
        ),
        Some("approved-local-only"),
        Some("windows-job-object-v0"),
        Some("required"),
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let approval = report.loader_approval.as_ref().unwrap();
    assert!(!approval.loader_enabled);
    assert!(approval
        .denied_reason
        .as_deref()
        .unwrap_or_default()
        .contains("loader_preflight"));
}

#[test]
fn real_load_intent_rejects_mismatched_loader_preflight_evidence() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/preflight-mismatch/ClassicTest.aex", 32);
    let other_plugin = write_fake_aex("loader-gate/preflight-mismatch/OtherTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/preflight-mismatch/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/preflight-mismatch/input.png", 64, 64);
    let output = target_path("loader-gate/preflight-mismatch/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_passed");
    let preflight = write_loader_preflight_evidence(
        "loader-gate/preflight-mismatch/preflight.json",
        &other_plugin,
        "classic-test",
    );
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("plugin_path"));
}

#[test]
fn real_load_intent_rejects_mismatched_loader_gate_plugin_path_receipt() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let unique = format!(
        "loader-gate/preflight-loader-path-mismatch/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let plugin = write_fake_aex(&format!("{unique}/ClassicTest.aex"), 32);
    let other_plugin = write_fake_aex(&format!("{unique}/OtherTest.aex"), 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        &format!("{unique}/allowlist.json"),
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png(&format!("{unique}/input.png"), 64, 64);
    let output = target_path(&format!("{unique}/output.png"));
    let preflight = write_loader_preflight_evidence(
        &format!("{unique}/preflight.json"),
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["selected_candidate"]["loader_gate_plugin_path"] =
        serde_json::json!(path_text(&other_plugin));
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(&plugin, &allowlist, &input, &output, None, None),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("loader_gate_plugin_path"));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("request plugin_path")));
}

#[test]
fn real_load_intent_rejects_mismatched_selected_loader_entry_id_receipt() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let unique = format!(
        "loader-gate/preflight-loader-entry-id-mismatch/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let plugin = write_fake_aex(&format!("{unique}/ClassicTest.aex"), 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        &format!("{unique}/allowlist.json"),
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png(&format!("{unique}/input.png"), 64, 64);
    let output = target_path(&format!("{unique}/output.png"));
    let _ = std::fs::remove_file(&output);
    let preflight = write_loader_preflight_evidence(
        &format!("{unique}/preflight.json"),
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["selected_loader_entry"]["effect_id"] = serde_json::json!("other-allowlist-id");
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(&plugin, &allowlist, &input, &output, None, None),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("selected_loader_entry.effect_id"));
}

#[test]
fn real_load_intent_rejects_mismatched_selected_loader_entry_path_receipt() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let unique = format!(
        "loader-gate/preflight-loader-entry-path-mismatch/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let plugin = write_fake_aex(&format!("{unique}/ClassicTest.aex"), 32);
    let other_plugin = write_fake_aex(&format!("{unique}/OtherTest.aex"), 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        &format!("{unique}/allowlist.json"),
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png(&format!("{unique}/input.png"), 64, 64);
    let output = target_path(&format!("{unique}/output.png"));
    let _ = std::fs::remove_file(&output);
    let preflight = write_loader_preflight_evidence(
        &format!("{unique}/preflight.json"),
        &plugin,
        "classic-test",
    );
    let other_path_text = path_text(&other_plugin);
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["selected_loader_entry"]["plugin_path"] = serde_json::json!(other_path_text);
    evidence["selected_loader_entry"]["normalized_plugin_path"] =
        serde_json::json!(loader_preflight_path_key_text(&other_path_text));
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(&plugin, &allowlist, &input, &output, None, None),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("selected_loader_entry.plugin_path"));
}

#[test]
fn real_load_intent_rejects_nonready_selected_loader_entry_receipt() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let unique = format!(
        "loader-gate/preflight-loader-entry-status-mismatch/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let plugin = write_fake_aex(&format!("{unique}/ClassicTest.aex"), 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        &format!("{unique}/allowlist.json"),
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png(&format!("{unique}/input.png"), 64, 64);
    let output = target_path(&format!("{unique}/output.png"));
    let _ = std::fs::remove_file(&output);
    let preflight = write_loader_preflight_evidence(
        &format!("{unique}/preflight.json"),
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["selected_loader_entry"]["allowlist_operation_status"] =
        serde_json::json!("describe-only");
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(&plugin, &allowlist, &input, &output, None, None),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("selected_loader_entry.allowlist_operation_status"));
}

#[test]
fn real_load_intent_rejects_blocked_loader_preflight_check() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/preflight-check/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/preflight-check/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/preflight-check/input.png", 64, 64);
    let output = target_path("loader-gate/preflight-check/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_passed");
    let preflight = write_loader_preflight_evidence(
        "loader-gate/preflight-check/preflight.json",
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["checks"][0]["status"] = serde_json::json!("blocked");
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("required check"));
}

#[test]
fn real_load_intent_rejects_loader_preflight_forbidden_output_png_token() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/preflight-output-png/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/preflight-output-png/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/preflight-output-png/input.png", 64, 64);
    let output = target_path("loader-gate/preflight-output-png/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_passed");
    let preflight = write_loader_preflight_evidence(
        "loader-gate/preflight-output-png/preflight.json",
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["output_png"] = serde_json::json!("target/aex-image-probe/forbidden.png");
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("output_png"));
}

#[test]
fn real_load_intent_rejects_mismatched_fixture_gate_receipt() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/preflight-fixture-mismatch/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/preflight-fixture-mismatch/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/preflight-fixture-mismatch/input.png", 64, 64);
    let output = target_path("loader-gate/preflight-fixture-mismatch/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_passed");
    let preflight = write_loader_preflight_evidence(
        "loader-gate/preflight-fixture-mismatch/preflight.json",
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["fixture_gate"]["selected_fixture"] = serde_json::json!("other-fixture");
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("fixture_gate.selected_fixture"));
}

#[test]
fn real_load_intent_rejects_closed_fixture_gate_receipt() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/preflight-fixture-closed/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/preflight-fixture-closed/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/preflight-fixture-closed/input.png", 64, 64);
    let output = target_path("loader-gate/preflight-fixture-closed/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_passed");
    let preflight = write_loader_preflight_evidence(
        "loader-gate/preflight-fixture-closed/preflight.json",
        &plugin,
        "classic-test",
    );
    let mut evidence: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
    evidence["fixture_gate"]["approval_loader_enabled"] = serde_json::json!(false);
    std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert!(report.identity_preflight.is_none());
    assert!(report.worker_identity_revalidation.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .loader_approval
        .as_ref()
        .and_then(|approval| approval.denied_reason.as_deref())
        .unwrap_or_default()
        .contains("fixture_gate approval"));
}

#[test]
fn real_load_intent_rejects_zero_count_gate_receipts() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    for (case, pointer, expected) in [
        (
            "fixture-zero",
            "/fixture_gate/candidate_count",
            "fixture_gate candidate_count",
        ),
        (
            "loader-zero",
            "/loader_gate/entry_count",
            "loader_gate entry_count",
        ),
    ] {
        let plugin = write_fake_aex(
            &format!("loader-gate/preflight-count/{case}/ClassicTest.aex"),
            32,
        );
        let allowlist = write_classic_allowlist_with_loader_gate(
            &format!("loader-gate/preflight-count/{case}/allowlist.json"),
            &plugin,
            Some("approved-local-only"),
            Some("implemented-v0"),
            Some("required"),
        );
        let input = write_synthetic_png(
            &format!("loader-gate/preflight-count/{case}/input.png"),
            64,
            64,
        );
        let output = target_path(&format!("loader-gate/preflight-count/{case}/output.png"));
        let _ = std::fs::remove_file(&output);
        let worker = build_test_worker("revalidation_passed");
        let preflight = write_loader_preflight_evidence(
            &format!("loader-gate/preflight-count/{case}/preflight.json"),
            &plugin,
            "classic-test",
        );
        let mut evidence: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&preflight).unwrap()).unwrap();
        *evidence.pointer_mut(pointer).unwrap() = serde_json::json!(0);
        std::fs::write(&preflight, serde_json::to_string_pretty(&evidence).unwrap()).unwrap();
        let request_text = add_loader_preflight(
            &add_loader_intent(
                &classic_render_request_with_paths(
                    &plugin,
                    &allowlist,
                    &input,
                    &output,
                    Some(&worker),
                    None,
                ),
                Some("approved-local-only"),
                Some("windows-job-object-v0"),
                Some("required"),
            ),
            &preflight,
        );

        let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

        assert_eq!(report.status, "worker_protocol_error");
        assert!(report.identity_preflight.is_none());
        assert!(report.worker_identity_revalidation.is_none());
        assert!(report.output_png.is_none());
        assert!(!output.exists());
        assert!(report
            .loader_approval
            .as_ref()
            .and_then(|approval| approval.denied_reason.as_deref())
            .unwrap_or_default()
            .contains(expected));
    }
}

#[test]
fn real_load_intent_rejects_absent_worker_side_revalidation() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/revalidation/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/revalidation/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/revalidation/input.png", 64, 64);
    let output = target_path("loader-gate/revalidation/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("revalidation_absent");
    let preflight = write_loader_preflight_evidence(
        "loader-gate/revalidation/preflight.json",
        &plugin,
        "classic-test",
    );
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let approval = report.loader_approval.as_ref().unwrap();
    assert!(!approval.worker_side_revalidation);
    assert!(approval
        .denied_reason
        .as_deref()
        .unwrap_or_default()
        .contains("revalidation"));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("revalidation")));
}

#[test]
fn real_load_intent_with_worker_identity_revalidation_still_disables_loader() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("loader-gate/revalidation-passed/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist_with_loader_gate(
        "loader-gate/revalidation-passed/allowlist.json",
        &plugin,
        Some("approved-local-only"),
        Some("implemented-v0"),
        Some("required"),
    );
    let input = write_synthetic_png("loader-gate/revalidation-passed/input.png", 64, 64);
    let output = target_path("loader-gate/revalidation-passed/output.png");
    let _ = std::fs::remove_file(&output);
    let before_transport = transport_manifest_files(output.parent().unwrap()).len();
    let before_identity = identity_manifest_files(output.parent().unwrap()).len();
    let before_ticket = loader_ticket_files(output.parent().unwrap()).len();
    let worker = build_test_worker("revalidation_passed");
    let preflight = write_loader_preflight_evidence_with_ids(
        "loader-gate/revalidation-passed/preflight.json",
        &plugin,
        "adaptive-filter-local",
        "classic-test",
    );
    let request_text = add_loader_preflight(
        &add_loader_intent(
            &classic_render_request_with_paths(
                &plugin,
                &allowlist,
                &input,
                &output,
                Some(&worker),
                None,
            ),
            Some("approved-local-only"),
            Some("windows-job-object-v0"),
            Some("required"),
        ),
        &preflight,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    let report_json = serde_json::to_value(&report).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let approval = report.loader_approval.as_ref().unwrap();
    assert!(!approval.approved);
    assert!(!approval.loader_enabled);
    assert!(!approval.real_aex_load_enabled);
    assert!(approval.worker_side_revalidation);
    assert!(approval
        .denied_reason
        .as_deref()
        .unwrap_or_default()
        .contains("disabled"));
    let sandbox = report.sandbox_preflight.as_ref().unwrap();
    assert!(sandbox.performed);
    assert_eq!(sandbox.profile, "windows-job-object-v0");
    assert_eq!(approval.job_object, sandbox.job_object_assigned);
    assert_eq!(
        approval.handle_inheritance_disabled,
        sandbox.handle_inheritance_disabled
    );
    assert_eq!(
        approval.controlled_working_directory,
        sandbox.controlled_working_directory
    );
    let revalidation = report.worker_identity_revalidation.as_ref().unwrap();
    assert!(revalidation.performed);
    assert_eq!(revalidation.status, "passed");
    assert_eq!(revalidation.allowlist_id.as_deref(), Some("classic-test"));
    let ticket = report.worker_loader_ticket.as_ref().unwrap();
    assert!(ticket.performed);
    assert_eq!(ticket.status, "accepted_no_load");
    assert_eq!(ticket.allowlist_id.as_deref(), Some("classic-test"));
    assert!(!ticket.native_load_performed);
    assert!(!ticket.worker_may_load_plugin);
    assert!(ticket.denied_reason.is_none());

    let transport_manifests = transport_manifest_files(output.parent().unwrap());
    assert_eq!(transport_manifests.len(), before_transport + 1);
    let transport_text = std::fs::read_to_string(transport_manifests.last().unwrap()).unwrap();
    assert!(!transport_text.contains(".aex"));
    assert!(!transport_text.contains("canonical_plugin_path"));

    let identity_manifests = identity_manifest_files(output.parent().unwrap());
    assert_eq!(identity_manifests.len(), before_identity + 1);
    let identity_text = std::fs::read_to_string(identity_manifests.last().unwrap()).unwrap();
    let identity_json: serde_json::Value = serde_json::from_str(&identity_text).unwrap();
    assert_eq!(identity_json["identity_protocol_version"], 1);
    assert_eq!(identity_json["binary_evidence_mode"], "metadata-only");
    assert_eq!(identity_json["sandbox_profile"], "windows-job-object-v0");
    assert!(identity_text.contains("canonical_plugin_path"));
    assert!(identity_text.to_ascii_lowercase().contains(".aex"));
    let loader_tickets = loader_ticket_files(output.parent().unwrap());
    assert_eq!(loader_tickets.len(), before_ticket + 1);
    let ticket_text = std::fs::read_to_string(loader_tickets.last().unwrap()).unwrap();
    let ticket_json: serde_json::Value = serde_json::from_str(&ticket_text).unwrap();
    assert_eq!(ticket_json["ticket_protocol_version"], 1);
    assert_eq!(ticket_json["status"], "accepted_no_load");
    assert_eq!(ticket_json["native_load_performed"], false);
    assert_eq!(ticket_json["worker_may_load_plugin"], false);
    assert_eq!(ticket_json["broker_may_load_plugin"], false);
    assert_eq!(
        ticket_json["selected_loader_entry"]["effect_id"],
        "classic-test"
    );
    assert_eq!(
        ticket_json["selected_loader_entry"]["path_match_status"],
        "matched_normalized_path"
    );
    assert!(ticket_json["planned_stages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|stage| stage["status"] == "planned_not_run"));
    assert!(!ticket_text.to_ascii_lowercase().contains("output_png"));
    assert_no_payload_fields(&identity_json);
    assert_no_payload_fields(&ticket_json);
    assert_no_payload_fields(&report_json);
}

#[test]
fn render_transport_rejects_bad_inputs_and_outputs() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("transport/reject/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("transport/reject/allowlist.json", &plugin);
    let worker = build_test_worker("ready");
    let output = target_path("transport/reject/output.png");
    let missing_input = target_path("transport/reject/missing.png");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &missing_input,
        &output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("input_png")));

    let invalid_input = target_path("transport/reject/not-png.png");
    std::fs::write(&invalid_input, b"not a png").unwrap();
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &invalid_input,
        &output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("decode")));

    let input = write_synthetic_png("transport/reject/input.png", 64, 64);
    let outside_output = target_path("../outside-output.png");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &outside_output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("generated root")));

    let existing_output = target_path("transport/reject/existing.png");
    if let Some(parent) = existing_output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&existing_output, b"existing").unwrap();
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &existing_output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert_eq!(std::fs::read(&existing_output).unwrap(), b"existing");
}

#[test]
fn render_transport_rejects_dimension_mismatch_and_decoded_byte_limit() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("transport/dim/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("transport/dim/allowlist.json", &plugin);
    let worker = build_test_worker("ready");
    let input = write_synthetic_png("transport/dim/input.png", 32, 64);
    let output = target_path("transport/dim/output.png");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("dimensions")));

    let input = write_synthetic_png("transport/bytes/input.png", 64, 64);
    let output = target_path("transport/bytes/output.png");
    let mut request_value: serde_json::Value =
        serde_json::from_str(&classic_render_request_with_paths(
            &plugin,
            &allowlist,
            &input,
            &output,
            Some(&worker),
            None,
        ))
        .unwrap();
    request_value["limits"]["max_bytes"] = serde_json::json!(1024);
    let report = run_probe_request_text(
        &serde_json::to_string(&request_value).unwrap(),
        Some(&request_path),
    )
    .unwrap();
    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("max_bytes")));
}

#[test]
fn allowlisted_classic_effect_launches_handshake_stub_only() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("identity/ready/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("identity/ready/allowlist.json", &plugin);
    let input = write_synthetic_png("identity/ready/input.png", 64, 64);
    let output = target_path("identity/ready/output.png");
    let _ = std::fs::remove_file(&output);
    let before_raw = rgba_transport_files(output.parent().unwrap()).len();
    let before_manifests = transport_manifest_files(output.parent().unwrap()).len();
    let worker = build_test_worker("ready");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    let report_json = serde_json::to_value(&report).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.plugin_class, "classic-effect");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert_eq!(
        report
            .identity_preflight
            .as_ref()
            .map(|identity| identity.status.as_str()),
        Some("allowed")
    );
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let raw_files = rgba_transport_files(output.parent().unwrap());
    assert_eq!(raw_files.len(), before_raw + 1);
    assert_eq!(
        std::fs::metadata(raw_files.last().unwrap()).unwrap().len(),
        64 * 64 * 4
    );
    let manifests = transport_manifest_files(output.parent().unwrap());
    assert_eq!(manifests.len(), before_manifests + 1);
    let manifest_text = std::fs::read_to_string(manifests.last().unwrap()).unwrap();
    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest_json["row_stride_bytes"], 64 * 4);
    assert!(!manifest_text.contains(".aex"));
    assert!(!manifest_text.contains("plugin_path"));
    assert!(!manifest_text.contains("canonical_plugin_path"));
    assert!(report
        .warnings
        .iter()
        .any(|item| item.contains("loading remains disabled")));
    assert!(report
        .warnings
        .iter()
        .any(|item| item.contains("transport manifest validated")));
    assert!(report
        .unsupported
        .iter()
        .any(|item| item.contains("before native entrypoint")));
    assert_no_payload_fields(&report_json);
}

#[test]
fn worker_handshake_protocol_failures_are_structured() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    for mode in ["malformed", "bad_protocol", "no_transport"] {
        let plugin = write_fake_aex(&format!("identity/{mode}/ClassicTest.aex"), 32);
        let allowlist =
            write_classic_allowlist(&format!("identity/{mode}/allowlist.json"), &plugin);
        let input = write_synthetic_png(&format!("identity/{mode}/input.png"), 64, 64);
        let output = target_path(&format!("identity/{mode}/output.png"));
        let _ = std::fs::remove_file(&output);
        let worker = build_test_worker(mode);
        let request_text = classic_render_request_with_paths(
            &plugin,
            &allowlist,
            &input,
            &output,
            Some(&worker),
            None,
        );

        let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

        assert_eq!(report.status, "worker_protocol_error");
        assert_eq!(report.stage.as_deref(), Some("handshake"));
        assert!(report.output_png.is_none());
        assert!(!output.exists());
        if mode == "no_transport" {
            assert!(report
                .warnings
                .iter()
                .any(|warning| warning.contains("transport validation")));
        }
    }
}

#[test]
fn worker_handshake_rejects_enabled_loading_without_revalidation_and_sandbox() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("identity/enabled-without-revalidation/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist(
        "identity/enabled-without-revalidation/allowlist.json",
        &plugin,
    );
    let input = write_synthetic_png("identity/enabled-without-revalidation/input.png", 64, 64);
    let output = target_path("identity/enabled-without-revalidation/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("enabled_without_revalidation");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_protocol_error");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.entrypoint.is_none());
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("disabled-loading contract")));
}

#[test]
fn worker_timeout_and_crash_statuses_are_canonical() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");

    let timeout_plugin = write_fake_aex("identity/timeout/ClassicTest.aex", 32);
    let timeout_allowlist =
        write_classic_allowlist("identity/timeout/allowlist.json", &timeout_plugin);
    let timeout_input = write_synthetic_png("identity/timeout/input.png", 64, 64);
    let timeout_output = target_path("identity/timeout/output.png");
    let _ = std::fs::remove_file(&timeout_output);
    let timeout_worker = build_test_worker("timeout");
    let request_text = classic_render_request_with_paths(
        &timeout_plugin,
        &timeout_allowlist,
        &timeout_input,
        &timeout_output,
        Some(&timeout_worker),
        Some(20),
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "timeout");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.output_png.is_none());
    assert!(!timeout_output.exists());

    let crash_plugin = write_fake_aex("identity/crash/ClassicTest.aex", 32);
    let crash_allowlist = write_classic_allowlist("identity/crash/allowlist.json", &crash_plugin);
    let crash_input = write_synthetic_png("identity/crash/input.png", 64, 64);
    let crash_output = target_path("identity/crash/output.png");
    let _ = std::fs::remove_file(&crash_output);
    let crash_worker = build_test_worker("crash");
    let request_text = classic_render_request_with_paths(
        &crash_plugin,
        &crash_allowlist,
        &crash_input,
        &crash_output,
        Some(&crash_worker),
        None,
    );
    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();
    assert_eq!(report.status, "worker_crash");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.output_png.is_none());
    assert!(!crash_output.exists());
}

#[test]
fn worker_failure_stdio_previews_are_bounded_and_sanitized() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let plugin = write_fake_aex("identity/noisy-crash/ClassicTest.aex", 32);
    let allowlist = write_classic_allowlist("identity/noisy-crash/allowlist.json", &plugin);
    let input = write_synthetic_png("identity/noisy-crash/input.png", 64, 64);
    let output = target_path("identity/noisy-crash/output.png");
    let _ = std::fs::remove_file(&output);
    let worker = build_test_worker("noisy_crash");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        None,
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "worker_crash");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let crash = report.crash.as_ref().expect("crash preview should exist");
    let stdout = crash["stdout_preview"].as_str().unwrap();
    let stderr = crash["stderr_preview"].as_str().unwrap();
    assert!(stdout.chars().count() <= 2048);
    assert!(stderr.chars().count() <= 2048);
    assert!(stdout.contains("<redacted-local-path>"));
    assert!(stderr.contains("<redacted-local-path>"));
    assert!(stdout.contains("...[truncated]"));
    assert!(stderr.contains("...[truncated]"));
    assert!(!stdout.contains("D:/Private"));
    assert!(!stderr.contains("C:/Users"));
    assert!(!stdout.to_ascii_lowercase().contains(".aex"));
    assert!(!stderr.to_ascii_lowercase().contains(".aepx"));
}

#[test]
#[cfg(windows)]
fn worker_timeout_cleans_descendant_process_tree() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let unique = format!(
        "identity/descendant-timeout/{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let plugin = write_fake_aex(&format!("{unique}/ClassicTest.aex"), 32);
    let allowlist = write_classic_allowlist(&format!("{unique}/allowlist.json"), &plugin);
    let input = write_synthetic_png(&format!("{unique}/input.png"), 64, 64);
    let output = target_path(&format!("{unique}/output.png"));
    let descendant_pid_path = output.parent().unwrap().join("descendant.pid");
    let worker = build_test_worker("spawn_descendant_timeout");
    let request_text = classic_render_request_with_paths(
        &plugin,
        &allowlist,
        &input,
        &output,
        Some(&worker),
        Some(150),
    );

    let report = run_probe_request_text(&request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "timeout");
    assert_eq!(report.stage.as_deref(), Some("handshake"));
    assert!(report.output_png.is_none());
    assert!(!output.exists());
    let sandbox = report.sandbox_preflight.as_ref().unwrap();
    assert!(sandbox.job_object_assigned);
    assert!(sandbox.kill_on_job_close);
    assert_eq!(
        sandbox_check(sandbox, "job_object_kill_on_close").status,
        "measured_pass"
    );
    let descendant_pid: u32 = std::fs::read_to_string(&descendant_pid_path)
        .unwrap_or_else(|err| panic!("missing descendant pid file: {err}"))
        .trim()
        .parse()
        .expect("descendant pid should parse");
    assert!(
        wait_for_process_exit(descendant_pid, std::time::Duration::from_secs(3)),
        "descendant process {descendant_pid} should be gone after job close"
    );
}

#[test]
fn allowlisted_render_rejects_frame_beyond_allowlist_limits() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let request_text = r#"{
        "schema_version": 1,
        "operation": "render_png",
        "plugin_path": "D:/AviUtlas/local/ClassicTest.aex",
        "allowlist": "aex_image_probe_allowlist.classic.json",
        "input_png": "input.png",
        "output_png": "target/aex-image-probe/output.png",
        "pixel_format": "rgba8",
        "frame": {
            "width": 256,
            "height": 64
        },
        "params": {}
    }"#;

    let report = run_probe_request_text(request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("allowlist max_width")));
}

#[test]
fn allowlisted_render_rejects_frame_beyond_request_byte_limit() {
    let request_path = fixture_path("aex_image_probe_request.synthetic.json");
    let request_text = r#"{
        "schema_version": 1,
        "operation": "render_png",
        "plugin_path": "D:/AviUtlas/local/ClassicTest.aex",
        "allowlist": "aex_image_probe_allowlist.classic.json",
        "input_png": "input.png",
        "output_png": "target/aex-image-probe/output.png",
        "pixel_format": "rgba8",
        "frame": {
            "width": 64,
            "height": 64
        },
        "limits": {
            "max_width": 128,
            "max_height": 128,
            "max_bytes": 1024
        },
        "params": {}
    }"#;

    let report = run_probe_request_text(request_text, Some(&request_path)).unwrap();

    assert_eq!(report.status, "invalid_request");
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("max_bytes")));
}

#[test]
fn broker_source_allows_process_spawn_only_in_worker_launcher() {
    let broker_source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("aex_image_probe.rs"),
    )
    .unwrap();
    let worker_source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("aex_effect_worker_stub.rs"),
    )
    .unwrap();

    for forbidden in [
        "libloading::Library",
        "libloading",
        "Library::new",
        "EffectMain",
        "LoadLibrary",
        "LoadLibraryA",
        "LoadLibraryW",
        "LoadLibraryExA",
        "LoadLibraryExW",
        "GetProcAddress",
        "PluginManager",
        "Win32_System_LibraryLoader",
        "windows::Win32::System::LibraryLoader",
        "PF_Cmd_GLOBAL_SETUP",
        "PF_Cmd_SEQUENCE_SETUP",
        "PF_Cmd_RENDER",
        "after-effects",
        "after_effects",
        "pipl",
        "cmd.exe",
        "powershell",
        "pwsh",
        "wscript",
        "ShellExecute",
        "ComSpec",
        " /C ",
        "\"transport_invalid\"",
        "\"aex_loading\":\"enabled\"",
        "\"load_ok\"",
        "\"loaded_ok\"",
        "\"worker_loaded\"",
        "\"plugin_loaded\"",
        "\"render_ok\"",
        "\"load_failed\"",
        "\"setup_failed\"",
    ] {
        assert!(
            !broker_source.contains(forbidden),
            "broker source should not contain {forbidden}"
        );
        assert!(
            !worker_source.contains(forbidden),
            "worker source should not contain {forbidden}"
        );
    }

    for forbidden in [".arg(plugin_path)", "--input-png"] {
        assert!(
            !broker_source.contains(forbidden),
            "broker should not pass {forbidden} to the worker"
        );
    }

    let launcher_index = broker_source
        .find("fn launch_worker_stub_report")
        .expect("dedicated worker launcher should exist");
    let spawn_helper_index = broker_source
        .find("fn spawn_worker_child")
        .expect("dedicated worker spawn helper should exist");
    assert!(
        broker_source[spawn_helper_index..launcher_index].contains("CreateProcessW"),
        "Windows spawn helper should use CreateProcessW instead of Command"
    );
    assert!(
        broker_source[spawn_helper_index..launcher_index]
            .contains("PROC_THREAD_ATTRIBUTE_HANDLE_LIST"),
        "Windows spawn helper should use an explicit inherited-handle list"
    );
    assert!(
        broker_source[spawn_helper_index..launcher_index].contains("EXTENDED_STARTUPINFO_PRESENT"),
        "Windows spawn helper should enable STARTUPINFOEX attributes"
    );
    assert!(
        broker_source[spawn_helper_index..launcher_index].contains("CREATE_SUSPENDED"),
        "Windows spawn helper should create the worker suspended until job assignment"
    );
    assert!(
        broker_source[..launcher_index].contains("ResumeThread"),
        "Windows child wrapper should resume the suspended worker explicitly"
    );
    let spawn_call_index = broker_source[launcher_index..]
        .find("spawn_worker_child(")
        .map(|index| launcher_index + index)
        .expect("dedicated worker launcher should call the spawn helper");
    let job_assign_index = broker_source[launcher_index..]
        .find("job.assign_child(&child)")
        .map(|index| launcher_index + index)
        .expect("worker launcher should assign the suspended child to the job");
    let resume_index = broker_source[launcher_index..]
        .find("child.resume()")
        .map(|index| launcher_index + index)
        .expect("worker launcher should resume the child after job assignment");
    let command_index = broker_source
        .find("std::process::Command::new(worker_path)")
        .expect("non-Windows fallback should spawn only the explicit worker_path");
    let non_windows_spawn_index = broker_source[spawn_helper_index..launcher_index]
        .find("#[cfg(not(windows))]")
        .map(|index| spawn_helper_index + index)
        .expect("Command fallback should be cfg(not(windows)) only");
    assert!(
        spawn_call_index > launcher_index,
        "worker launch should stay inside the dedicated worker launcher"
    );
    assert!(
        job_assign_index > spawn_call_index && resume_index > job_assign_index,
        "worker should be assigned to its job before it is resumed"
    );
    assert!(
        command_index > non_windows_spawn_index && command_index < launcher_index,
        "Command::new should stay inside the cfg(not(windows)) spawn helper"
    );
}
