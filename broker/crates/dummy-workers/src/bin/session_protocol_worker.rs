//! Render-session protocol test fixture (issue #98 PR-C).
//!
//! Speaks the worker side of docs/RENDER_SESSION_PROTOCOL_2026-07-19.md over
//! the inherited transport exactly like `aex_render_worker --render-session-v1`
//! so broker `RenderSession` integration tests can exercise per-frame
//! validation, the watchdog, and crash invalidation against a real isolated
//! process without a native minihost build. The "render" is a byte inversion
//! of the input slot. `AEXCOMPAT_TEST_SESSION_BEHAVIOR` selects misbehaviors:
//!
//! - `hang_frame`: never answers the first `render_frame` (watchdog target).
//! - `crash_frame`: dies with an access-violation exit code mid-frame.
//! - `heap_corruption_after_frame_0`: returns one valid Smart frame, then dies
//!   with Windows heap-corruption status before reading the close request.
//! - `heap_corruption_on_close_after_frame_0`: returns one valid Smart frame,
//!   accepts the close request, then dies with Windows heap-corruption status.
//! - `exit_leaving_descendant`: spawns a sleeping child that inherits the
//!   session handles, then exits; pipe EOF never fires, only the process
//!   watcher can see the death (protocol §7 three-way wait).
//! - `stale_generation`: answers with a stale generation and no header update.
//! - `mutate_header`: rewrites a broker-owned static header field.
//! - `bad_extent`: reports a packed byte count that disagrees with the
//!   dimensions it reports alongside it.
//! - `error_frame_0`: answers frame 0 with a frame-local error response.
//! - `modal_frame`: opens a MessageBox on its first frame and waits for the
//!   broker watchdog (issue #351). The desktop report itself is written at
//!   launch, independent of this behavior.
//!
//! Cluster session support (issue #405,
//! docs/CLOSURE_SESSION_PROTOCOL_2026-07-23.md): when the launch argv carries
//! `--cluster-manifest-v2 <path>`, the fixture validates the in-place transport
//! the way the real loader does (4 MiB bound, v2 schema), cross-checks the positional plugin against
//! `plugins[0]`, answers `swap_plugin` with `swap_done`, and records swap
//! epochs for the final report's cluster module audit. It also serves
//! `--discovery-session-v1 --cluster-manifest-v2 <path>`, answering
//! `inspect_plugin` with `inspect_done`. Additional misbehaviors:
//!
//! - `crash_on_swap`: dies with an access-violation exit code on the first
//!   `swap_plugin` (worker-death three-way wait target).
//! - `swap_done_wrong_index`: answers the swap with a mismatched
//!   `plugin_index` (broker-side protocol-violation target).
//! - `swap_global_setup_error`: answers the swap with
//!   `status:"error","global_setup_error":25` (plugin-local error path).
//! - `audit_undeclared_module`: adds an undeclared module to the final
//!   report's observed union (declared-set audit rejection target).
//! - `crash_on_inspect`: dies on the first `inspect_plugin`.
//! - `inspect_error_plugin_1`: answers `inspect_plugin` for plugin 1 with a
//!   structured parameter-local error.
//! - `inspect_entrypoint_aegp_plugin_1`: answers plugin 1 with the real
//!   entrypoint-error shape and a partial AEGP classification report.

#[cfg(windows)]
mod worker {
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BEGIN, ReadFile, SetFilePointer, WriteFile,
    };
    use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile};
    use windows_sys::Win32::System::StationsAndDesktops::{
        GetThreadDesktop, GetUserObjectInformationW, UOI_NAME,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    const HEADER_BYTES: usize = 4096;
    const SLOT_ALIGNMENT: usize = 4096;
    const HEADER_MAGIC: u32 = 0x5358_4541;
    const MAX_MESSAGE_BYTES: usize = 64 * 1024;
    const MAGIC_OFFSET: usize = 0;
    const VERSION_OFFSET: usize = 4;
    const DEPTH_CODE_OFFSET: usize = 8;
    const MAX_WIDTH_OFFSET: usize = 12;
    const MAX_HEIGHT_OFFSET: usize = 16;
    const LAYER_SLOT_COUNT_OFFSET: usize = 20;
    const INPUT_GENERATION_OFFSET: usize = 24;
    const OUTPUT_GENERATION_OFFSET: usize = 28;
    const FRAME_WIDTH_OFFSET: usize = 32;
    const FRAME_HEIGHT_OFFSET: usize = 36;

    const EXIT_PROTOCOL_VIOLATION: i32 = 23;
    const EXIT_INVARIANT_FAILURE: i32 = 24;
    // Layer pixels travel as inherited file handles (#268); the trailer carries
    // each layer's slot/geometry plus its read handle value (v2).
    const LAYER_TRAILER_PREFIX: &str = "session-layers:v2|";

    fn env_handle(name: &str) -> Option<HANDLE> {
        let value = std::env::var(name).ok()?.parse::<usize>().ok()?;
        (value != 0).then_some(value as HANDLE)
    }

    fn current_desktop_name() -> Option<String> {
        let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if desktop.is_null() {
            return None;
        }
        let mut required = 0u32;
        unsafe {
            GetUserObjectInformationW(desktop, UOI_NAME, null_mut(), 0, &mut required);
        }
        if required < 2 {
            return None;
        }
        let mut buffer = vec![0u16; (required as usize).div_ceil(2)];
        if unsafe {
            GetUserObjectInformationW(
                desktop,
                UOI_NAME,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * 2) as u32,
                &mut required,
            )
        } == 0
        {
            return None;
        }
        let end = buffer.iter().position(|value| *value == 0).unwrap_or(0);
        String::from_utf16(&buffer[..end]).ok()
    }

    fn report_desktop_for_test() {
        let Some(path) = std::env::var_os("AEXCOMPAT_TEST_SESSION_DESKTOP_REPORT") else {
            return;
        };
        // Report the lookup failure too: a caller that sees no file at all
        // cannot tell "never reached this point" from "ran with no readable
        // desktop", and those want different fixes.
        let name = current_desktop_name().unwrap_or_else(|| "<desktop-name-unavailable>".into());
        let _ = std::fs::write(path, name);
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            title: *const u16,
            kind: u32,
        ) -> i32;
    }

    fn show_modal_dialog_and_wait() -> ! {
        let text: Vec<u16> = "AEXCompat modal session fixture"
            .encode_utf16()
            .chain([0])
            .collect();
        let title: Vec<u16> = "noninteractive worker".encode_utf16().chain([0]).collect();
        unsafe {
            MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), 0);
        }
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    }

    fn read_exact(handle: HANDLE, destination: &mut [u8]) -> bool {
        let mut collected = 0usize;
        while collected < destination.len() {
            let mut read = 0u32;
            let ok = unsafe {
                ReadFile(
                    handle,
                    destination[collected..].as_mut_ptr(),
                    (destination.len() - collected) as u32,
                    &mut read,
                    null_mut(),
                )
            };
            if ok == 0 || read == 0 {
                return false;
            }
            collected += read as usize;
        }
        true
    }

    fn write_all(handle: HANDLE, source: &[u8]) -> bool {
        let mut sent = 0usize;
        while sent < source.len() {
            let mut written = 0u32;
            let ok = unsafe {
                WriteFile(
                    handle,
                    source[sent..].as_ptr(),
                    (source.len() - sent) as u32,
                    &mut written,
                    null_mut(),
                )
            };
            if ok == 0 || written == 0 {
                return false;
            }
            sent += written as usize;
        }
        true
    }

    // Opt-in crash minidump transport (issue #18/#224). The native worker
    // writes the DbgHelp image to the broker-inherited pipe from its dedicated
    // writer thread and terminates it with a fixed completion marker; the broker
    // reader withholds that marker-sized suffix and publishes the prefix as the
    // `.dmp`. This fixture stands in for the DbgHelp payload with a recognizable
    // `MDMP`-prefixed body so a session crash exercises the broker's session
    // launch handle plumbing, bounded copy, and finalize-on-close end to end.
    fn write_opt_in_minidump() {
        const COMPLETION_MARKER: &[u8; 16] = b"AEXDUMP-COMPLETE";
        let Some(handle) = env_handle("AEXCOMPAT_MINIDUMP_HANDLE") else {
            return;
        };
        let mut payload = b"MDMP session-crash-minidump-fixture".to_vec();
        payload.resize(4096, 0x5a);
        if write_all(handle, &payload) && write_all(handle, COMPLETION_MARKER) {
            // Mirror the native worker's stderr note so the broker's diagnostics
            // parser (`minidump_marker`) surfaces the capture in the session
            // report. Flush before the crash exit so the line is not lost.
            use std::io::Write;
            let _ = write!(
                std::io::stderr(),
                "stage:minidump_written bytes={}\n",
                payload.len()
            );
            let _ = std::io::stderr().flush();
        }
    }

    fn write_message(handle: HANDLE, payload: &str) -> bool {
        let prefix = (payload.len() as u32).to_le_bytes();
        write_all(handle, &prefix) && write_all(handle, payload.as_bytes())
    }

    struct View(*mut u8);
    impl View {
        fn read_u32(&self, offset: usize) -> u32 {
            let mut bytes = [0u8; 4];
            unsafe {
                std::ptr::copy_nonoverlapping(self.0.add(offset), bytes.as_mut_ptr(), 4);
            }
            u32::from_le_bytes(bytes)
        }
        fn write_u32(&self, offset: usize, value: u32) {
            unsafe {
                std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), self.0.add(offset), 4);
            }
        }
    }

    /// The fixture's view of the `cluster-manifest-v2` document.
    struct ClusterManifest {
        plugins: Vec<(String, String)>,
    }

    /// Loads and validates the manifest transport the way the real worker's
    /// loader does: an absolute existing file bounded to 4 MiB, with the v2
    /// schema, absolute plug-in paths, and absolute dependency search roots.
    fn load_cluster_manifest(path: &str) -> Option<ClusterManifest> {
        let path = std::path::Path::new(path);
        if !path.is_absolute() || !path.is_file() {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() || bytes.len() > 4 * 1024 * 1024 {
            return None;
        }
        let document: Value = serde_json::from_slice(&bytes).ok()?;
        if document.get("schema").and_then(Value::as_str) != Some("cluster-manifest-v2") {
            return None;
        }
        let mut plugins = Vec::new();
        for plugin in document.get("plugins")?.as_array()? {
            let path = plugin.get("path")?.as_str()?;
            if !std::path::Path::new(path).is_absolute() {
                return None;
            }
            let basename = std::path::Path::new(path).file_name()?.to_str()?.to_owned();
            let sha256 = plugin.get("sha256")?.as_str()?.to_owned();
            plugins.push((basename, sha256));
        }
        let dirs = document.get("search_dirs")?.as_array()?;
        if plugins.is_empty()
            || dirs.is_empty()
            || dirs.len() > 16
            || !dirs.iter().all(|dir| {
                dir.as_str()
                    .is_some_and(|dir| std::path::Path::new(dir).is_absolute())
            })
        {
            return None;
        }
        Some(ClusterManifest { plugins })
    }
    /// One audit snapshot for the cluster report: the worker image plus the
    /// plugin-class modules loaded from the sealed root — the given plugin
    /// and the pinned closure.
    fn cluster_audit_snapshot(manifest: &ClusterManifest, plugin_index: usize) -> Value {
        let plugin = vec![manifest.plugins[plugin_index].0.clone()];
        json!({
            "status": "passed",
            "unknown_count": 0,
            "worker": ["session_protocol_worker.exe"],
            "plugin": plugin,
            "system32": ["kernel32.dll"]
        })
    }

    /// The cluster session's module audit (design §5): terminal snapshots of
    /// the last loaded plugin, per-swap epochs, and the cumulative union of
    /// every visited plugin plus the closure. `audit_undeclared_module`
    /// injects a module outside the manifest's declared set so the broker's
    /// declared-set validation must reject the report.
    fn cluster_module_audit(
        manifest: &ClusterManifest,
        current_plugin: usize,
        visited: &[usize],
        epochs: &[(usize, usize)],
        behavior: &str,
    ) -> Value {
        let mut union_plugin: Vec<String> = Vec::new();
        for index in visited {
            let basename = &manifest.plugins[*index].0;
            if !union_plugin.contains(basename) {
                union_plugin.push(basename.clone());
            }
        }
        if behavior == "audit_undeclared_module" {
            union_plugin.push("evil.dll".to_owned());
        }
        let union = json!({
            "status": "passed",
            "unknown_count": 0,
            "worker": ["session_protocol_worker.exe"],
            "plugin": union_plugin,
            "system32": ["kernel32.dll"]
        });
        let epochs: Vec<Value> = epochs
            .iter()
            .map(|(old, new)| {
                json!({
                    "plugin_index": old,
                    "pre_unload": cluster_audit_snapshot(manifest, *old),
                    "post_load": cluster_audit_snapshot(manifest, *new)
                })
            })
            .collect();
        json!({
            "schema": 1,
            "status": "passed",
            "phase_count": 3,
            "unknown_count": 0,
            "post_load": cluster_audit_snapshot(manifest, current_plugin),
            "pre_unload": cluster_audit_snapshot(manifest, current_plugin),
            "observed_union": union,
            "epochs": epochs
        })
    }

    fn default_module_audit() -> Value {
        // The broker validates a module audit on a clean exit exactly like the
        // one-shot path; this fixture reports its own honest minimal audit.
        json!({
            "schema": 1,
            "status": "passed",
            "phase_count": 3,
            "unknown_count": 0,
            "post_load": {
                "status": "passed", "unknown_count": 0,
                "worker": ["session_protocol_worker.exe"],
                "plugin": ["plugin.plugin"],
                "system32": ["kernel32.dll"]
            },
            "pre_unload": {
                "status": "passed", "unknown_count": 0,
                "worker": ["session_protocol_worker.exe"],
                "plugin": ["plugin.plugin"],
                "system32": ["kernel32.dll"]
            },
            "observed_union": {
                "status": "passed", "unknown_count": 0,
                "worker": ["session_protocol_worker.exe"],
                "plugin": ["plugin.plugin"],
                "system32": ["kernel32.dll"]
            }
        })
    }

    fn final_report(
        frames: u32,
        smart: bool,
        launch_payload: &str,
        module_audit: Value,
        behavior: &str,
    ) -> String {
        // The session-mechanics keys follow the worker flavor: the classic
        // report reuses its persistent-sequence fields while the smart report
        // carries dedicated session_* fields (protocol v1.1).
        let mechanics = if smart {
            serde_json::json!({
                "session_mode": true,
                "session_frames_attempted": frames,
                "session_render_error": 0,
                "session_sequence_setup_error": 0,
                "session_sequence_setdown_error": 0,
            })
        } else {
            serde_json::json!({
                "render_error": 0,
                "persistent_sequence_setup_error": 0,
                "persistent_sequence_setdown_error": 0,
            })
        };
        let mut report = serde_json::json!({
            "status": "render_completed",
            "global_setdown_error": 0,
            // Echo the launch payload argv slot verbatim so a test can prove
            // what reached the worker. The real worker parses it instead
            // (`hooks.parse_parameters(argv[4], ...)`); this fixture only has
            // to show which bytes arrived, which is the whole claim behind
            // SessionOpenRequest::payload_override (#365).
            "launch_payload": launch_payload,
            "session_frames": frames,
            "guard_bytes_intact": true,
            "suite_leases_balanced": true,
            "handle_lifetimes_balanced": true,
            "world_lifetimes_balanced": true,
            "param_checkouts_balanced": true,
            // The fixture has no AEX entry point, but it does receive the
            // exact classic or SmartFX session command the broker selected.
            // Publish bounded path counters so the integration test can prove
            // that route instead of inferring it from a success status.
            "selector_counters": {
                "classic_render": if smart { 0 } else { frames },
                "smart_pre_render": if smart { frames } else { 0 },
                "smart_render": if smart { frames } else { 0 }
            },
            "module_audit": module_audit
        });
        report
            .as_object_mut()
            .unwrap()
            .extend(mechanics.as_object().unwrap().clone());
        if smart && behavior == "smart_output_untouched" {
            report.as_object_mut().unwrap().extend(
                serde_json::json!({
                    "pre_render_error": 0,
                    "smart_render_selector_error": 0,
                    "smart_render_error": -6,
                    "output_pixels_valid": false,
                    "empty_result_rect": false,
                    "result_rects_valid": true
                })
                .as_object()
                .unwrap()
                .clone(),
            );
        }
        report.to_string()
    }

    pub fn run() -> i32 {
        let args: Vec<String> = std::env::args().collect();
        if args.len() == 2 && args[1] == "--sleep-child" {
            std::thread::sleep(std::time::Duration::from_secs(120));
            return 0;
        }
        // Discovery session mode (issue #405, design §2.2): no positional
        // plugin rides argv, only the cluster manifest transport.
        if args.len() >= 2 && args[1] == "--discovery-session-v1" {
            if args.len() != 4 || args[2] != "--cluster-manifest-v2" {
                return 2;
            }
            let Some(manifest) = load_cluster_manifest(&args[3]) else {
                return 3;
            };
            let behavior = std::env::var("AEXCOMPAT_TEST_SESSION_BEHAVIOR").unwrap_or_default();
            let (Some(request), Some(response)) = (
                env_handle("AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE"),
                env_handle("AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE"),
            ) else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            return run_discovery(request, response, &manifest, &behavior);
        }
        // Trailing auxiliary option pairs mirror the real worker's
        // strip_auxiliary_options contract: peel them off the tail, and for
        // --parameter-animation-v1 enforce the native loader's pin — the
        // sidecar must exist AND its parent must canonicalize to the worker's
        // cwd + target/image-transport (parameter_animation_transport.cpp) —
        // so a broker writing the sidecar somewhere the real worker would
        // reject fails these tests too.
        let mut cluster_manifest: Option<ClusterManifest> = None;
        let mut effective = args.len();
        while effective >= 12 && args[effective - 2].starts_with("--") {
            let value = &args[effective - 1];
            match args[effective - 2].as_str() {
                "--parameter-animation-v1" => {
                    let sidecar = std::path::Path::new(value);
                    let pinned = std::env::current_dir()
                        .ok()
                        .and_then(|cwd| cwd.join("image-transport").canonicalize().ok());
                    let parent = sidecar
                        .parent()
                        .and_then(|parent| parent.canonicalize().ok());
                    if !sidecar.is_file() || pinned.is_none() || pinned != parent {
                        return 3;
                    }
                }
                // The real worker's auxiliary gates: a strictly shaped
                // manifest, an existing dump directory, and the literal "1".
                // The manifest check mirrors load_aux_manifest's top-level
                // contract (absolute existing file, bounded size, exactly
                // {schema, nonce, channels} with the v1 schema string and a
                // digit nonce); per-channel sidecar validation stays with the
                // real worker.
                "--aux-manifest-v1" => {
                    let manifest = std::path::Path::new(value);
                    if !manifest.is_absolute() || !manifest.is_file() {
                        return 3;
                    }
                    let Ok(bytes) = std::fs::read(manifest) else {
                        return 3;
                    };
                    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
                        return 3;
                    }
                    let Ok(document) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                        return 3;
                    };
                    let Some(object) = document.as_object() else {
                        return 3;
                    };
                    let schema_ok =
                        object.get("schema").and_then(|v| v.as_str()) == Some("aux-manifest-v1");
                    let nonce_ok =
                        object
                            .get("nonce")
                            .and_then(|v| v.as_str())
                            .is_some_and(|nonce| {
                                !nonce.is_empty() && nonce.bytes().all(|b| b.is_ascii_digit())
                            });
                    let channels_ok = object
                        .get("channels")
                        .and_then(serde_json::Value::as_array)
                        // The real gate rejects an empty channel list.
                        .is_some_and(|channels| {
                            !channels.is_empty()
                                && channels.iter().all(serde_json::Value::is_object)
                        });
                    if object.len() != 3 || !schema_ok || !nonce_ok || !channels_ok {
                        return 3;
                    }
                }
                "--dump-worlds-v1" => {
                    if !std::path::Path::new(value).is_dir() {
                        return 3;
                    }
                }
                // In-place dependency search directories (issue #751): the
                // real worker's apply_dependency_search_dirs requires
                // non-empty absolute directories joined by ';', bounded at
                // 16. Mirror the shape gate so a malformed broker join fails
                // these tests too.
                "--dependency-dirs-v1" => {
                    let dirs: Vec<&str> = value.split(';').collect();
                    let shape_ok = !value.is_empty()
                        && dirs.len() <= 16
                        && dirs
                            .iter()
                            .all(|dir| !dir.is_empty() && std::path::Path::new(dir).is_absolute());
                    if !shape_ok {
                        return 3;
                    }
                }
                "--output-checksum-detail-v1" => {
                    if value != "1" {
                        return 3;
                    }
                }
                // The in-place cluster manifest transport, with the same shape
                // gate the real worker loader enforces.
                "--cluster-manifest-v2" => {
                    cluster_manifest = load_cluster_manifest(value);
                    if cluster_manifest.is_none() {
                        return 3;
                    }
                }
                _ => {}
            }
            effective -= 2;
        }
        // Static context trailers ride the positional tail in the one-shot
        // order (mask, spatial, render); the real classifier peels render,
        // then spatial, then mask before the 10-slot session contract. The
        // fixture mirrors that shape check so a mangled trailer would break
        // these tests, but leaves the deep payload validation to the real
        // worker's context parsers.
        if effective >= 11 && args[effective - 1].starts_with("render:v1|") {
            effective -= 1;
        }
        if effective >= 11 && args[effective - 1].starts_with("spatial:v") {
            effective -= 1;
        }
        if effective >= 11 && args[effective - 1].starts_with("v2|") {
            effective -= 1;
        }
        // The secondary-layer trailer sits ahead of the context trailers; keep
        // it to validate the header's layer_slot_count and the per-layer
        // inherited handles (#268).
        let mut layer_trailer: Option<String> = None;
        if effective >= 11 && args[effective - 1].starts_with(LAYER_TRAILER_PREFIX) {
            layer_trailer = Some(args[effective - 1].clone());
            effective -= 1;
        }
        let expected_layer_count = layer_trailer
            .as_deref()
            .map(|trailer| trailer[LAYER_TRAILER_PREFIX.len()..].split(';').count() as u32)
            .unwrap_or(0);
        // The smart session command selects the smart final-report contract
        // (protocol v1.1); the transport behavior is identical.
        let smart = args[1] == "--smart-session-v1";
        if effective != 10 || (args[1] != "--render-session-v1" && !smart) {
            return 2;
        }
        // The positional argv contract names plugins[0] (design §2.2): the
        // plugin path's basename and the sha256 slot must match the manifest
        // exactly, or the launch fails.
        if let Some(manifest) = &cluster_manifest {
            let basename_matches = std::path::Path::new(&args[2])
                .file_name()
                .and_then(|name| name.to_str())
                == Some(manifest.plugins[0].0.as_str());
            // The fixture matches the positional plugin by basename + sha only —
            // weaker than the real worker's full-path canonical identity
            // (worker_cluster_manifest.cpp matches_launch_plugin), which the
            // real-worker integration tests cover.
            if !basename_matches || !args[3].eq_ignore_ascii_case(&manifest.plugins[0].1) {
                return 3;
            }
        }
        let (Ok(width), Ok(height), Ok(time_scale)) = (
            args[5].parse::<usize>(),
            args[6].parse::<usize>(),
            args[9].parse::<u32>(),
        ) else {
            return 2;
        };
        let behavior = std::env::var("AEXCOMPAT_TEST_SESSION_BEHAVIOR").unwrap_or_default();
        // The desktop is assigned at launch, so report it here rather than
        // from the frame handler: a caller whose frame deadline expires before
        // this process reaches the handler still gets to observe which desktop
        // it ran on.
        report_desktop_for_test();
        let (Some(request), Some(response), Some(section)) = (
            env_handle("AEXCOMPAT_RENDER_SESSION_REQUEST_HANDLE"),
            env_handle("AEXCOMPAT_RENDER_SESSION_RESPONSE_HANDLE"),
            env_handle("AEXCOMPAT_RENDER_SESSION_SECTION_HANDLE"),
        ) else {
            return EXIT_PROTOCOL_VIOLATION;
        };
        let view_address = unsafe { MapViewOfFile(section, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view_address.Value.is_null() {
            return EXIT_PROTOCOL_VIOLATION;
        }
        let view = View(view_address.Value as *mut u8);
        if view.read_u32(MAGIC_OFFSET) != HEADER_MAGIC
            // Session header layout version: 3 since layer pixels left the
            // section for inherited file handles (#268).
            || view.read_u32(VERSION_OFFSET) != 3
            || view.read_u32(DEPTH_CODE_OFFSET) != 8
            || view.read_u32(MAX_WIDTH_OFFSET) != width as u32
            || view.read_u32(MAX_HEIGHT_OFFSET) != height as u32
            || view.read_u32(LAYER_SLOT_COUNT_OFFSET) != expected_layer_count
        {
            return EXIT_PROTOCOL_VIOLATION;
        }
        let slot_bytes = width * height * 4;
        let aligned = |bytes: usize| bytes.div_ceil(SLOT_ALIGNMENT) * SLOT_ALIGNMENT;
        let input_offset = HEADER_BYTES;
        let output_offset = HEADER_BYTES + aligned(slot_bytes);

        // Layer pixels travel as inherited per-layer read handles (#268), not
        // section slots. The integration test fills each layer's file with its
        // slot number as a byte, so reading w*h*4 bytes from the handle and
        // checking the first byte proves the right file reached the right layer
        // entry (issue #98 W1-4). Each entry is static `slot,w,h,handle` (4
        // fields) or timed `slot,w,h,time,scale,handle` (6); the handle value is
        // always last. The handle is inherited, so its numeric value matches the
        // broker's.
        // Layers the broker rewrites between frames (issue #674) keep their
        // handle open here, exactly as the real worker does, so the frame loop
        // below can re-read them.
        let mut dynamic_layers: Vec<(u32, HANDLE, usize)> = Vec::new();
        if let Some(trailer) = &layer_trailer {
            for entry in trailer[LAYER_TRAILER_PREFIX.len()..].split(';') {
                let fields: Vec<&str> = entry.split(',').collect();
                if fields.len() != 4 && fields.len() != 5 && fields.len() != 6 {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                // `slot,w,h,handle,1`: the trailing 1 is the dynamic marker, so
                // the handle is the second-to-last field rather than the last.
                let dynamic = fields.len() == 5;
                if dynamic && fields.last() != Some(&"1") {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                let (
                    Some(Ok(slot)),
                    Some(Ok(layer_width)),
                    Some(Ok(layer_height)),
                    Some(Ok(handle_value)),
                ) = (
                    fields.first().map(|s| s.parse::<u32>()),
                    fields.get(1).map(|s| s.parse::<usize>()),
                    fields.get(2).map(|s| s.parse::<usize>()),
                    fields
                        .get(if dynamic { 3 } else { fields.len() - 1 })
                        .map(|s| s.parse::<usize>()),
                )
                else {
                    return EXIT_PROTOCOL_VIOLATION;
                };
                let layer_bytes = layer_width * layer_height * 4;
                if layer_bytes == 0 {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                let handle = handle_value as HANDLE;
                let mut buffer = vec![0u8; layer_bytes];
                if !read_exact(handle, &mut buffer) || buffer[0] != slot as u8 {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                if dynamic {
                    dynamic_layers.push((slot, handle, layer_bytes));
                    continue;
                }
                unsafe {
                    CloseHandle(handle);
                }
            }
        }

        let mut frames = 0u32;
        // Cluster session state (issue #405): the current plugin, every
        // plugin loaded so far (for the audit union), and the swap epochs.
        let mut current_plugin: usize = 0;
        let mut visited: Vec<usize> = vec![0];
        let mut swap_epochs: Vec<(usize, usize)> = Vec::new();
        loop {
            let mut prefix = [0u8; 4];
            if !read_exact(request, &mut prefix) {
                break;
            }
            let length = u32::from_le_bytes(prefix) as usize;
            if length == 0 || length > MAX_MESSAGE_BYTES {
                return EXIT_PROTOCOL_VIOLATION;
            }
            let mut body = vec![0u8; length];
            if !read_exact(request, &mut body) {
                return EXIT_PROTOCOL_VIOLATION;
            }
            let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            match message["type"].as_str() {
                Some("close") => {
                    if behavior == "heap_corruption_on_close_after_frame_0" && frames == 1 {
                        std::process::exit(0xC000_0374_u32 as i32);
                    }
                    break;
                }
                Some("render_frame") => {}
                Some("swap_plugin") => {
                    // Exact-key strictness (design §4.1): only v, type, and
                    // plugin_index may ride the message, and the index must
                    // select another manifest member.
                    let keys_ok = message.as_object().map(|object| object.len()) == Some(3)
                        && message.get("v").is_some()
                        && message.get("plugin_index").is_some();
                    let Some(new_index) =
                        message["plugin_index"].as_u64().map(|index| index as usize)
                    else {
                        return EXIT_PROTOCOL_VIOLATION;
                    };
                    let Some(manifest) = &cluster_manifest else {
                        return EXIT_PROTOCOL_VIOLATION;
                    };
                    if !keys_ok
                        || message["v"].as_u64() != Some(1)
                        || new_index >= manifest.plugins.len()
                        || new_index == current_plugin
                    {
                        return EXIT_PROTOCOL_VIOLATION;
                    }
                    match behavior.as_str() {
                        "crash_on_swap" => std::process::exit(0xC000_0005_u32 as i32),
                        "swap_done_wrong_index" => {
                            // A mismatched swap_done, then silence: only the
                            // broker's strict response validation can catch
                            // this before the deadline.
                            let reply = format!(
                                "{{\"v\":1,\"type\":\"swap_done\",\"plugin_index\":{},\"status\":\"ok\"}}",
                                new_index + 1
                            );
                            let _ = write_message(response, &reply);
                            loop {
                                std::thread::sleep(std::time::Duration::from_secs(3600));
                            }
                        }
                        _ => {}
                    }
                    swap_epochs.push((current_plugin, new_index));
                    visited.push(new_index);
                    current_plugin = new_index;
                    let reply = if behavior == "swap_global_setup_error" {
                        // Plugin-local GLOBAL_SETUP failure (design §4.1): the
                        // swap happened, the session continues.
                        format!(
                            "{{\"v\":1,\"type\":\"swap_done\",\"plugin_index\":{new_index},\"status\":\"error\",\"global_setup_error\":25}}"
                        )
                    } else {
                        format!(
                            "{{\"v\":1,\"type\":\"swap_done\",\"plugin_index\":{new_index},\"status\":\"ok\"}}"
                        )
                    };
                    if !write_message(response, &reply) {
                        return EXIT_PROTOCOL_VIOLATION;
                    }
                    continue;
                }
                _ => return EXIT_PROTOCOL_VIOLATION,
            }
            // Mirrors the real worker's message version gate (protocol
            // §4.2.1): v:1 carries no dynamic attribute; v:2 carries at least
            // one of `parameters` / `ui_action` as a string; anything else is a
            // violation.
            let (frame_parameters, frame_ui_action) = match message["v"].as_u64() {
                Some(1) => {
                    if message.get("parameters").is_some() || message.get("ui_action").is_some() {
                        return EXIT_PROTOCOL_VIOLATION;
                    }
                    (None, None)
                }
                Some(2) => {
                    let read_attribute = |key: &str| match message.get(key) {
                        Some(value) => match value.as_str() {
                            Some(text) => Ok(Some(text.to_owned())),
                            None => Err(()),
                        },
                        None => Ok(None),
                    };
                    let Ok(parameters) = read_attribute("parameters") else {
                        return EXIT_PROTOCOL_VIOLATION;
                    };
                    let Ok(ui_action) = read_attribute("ui_action") else {
                        return EXIT_PROTOCOL_VIOLATION;
                    };
                    if parameters.is_none() && ui_action.is_none() {
                        return EXIT_PROTOCOL_VIOLATION;
                    }
                    (parameters, ui_action)
                }
                _ => return EXIT_PROTOCOL_VIOLATION,
            };
            let Some(frame_index) = message["frame_index"].as_u64() else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            let scale = message["current_time"]["scale"].as_u64().unwrap_or(0) as u32;
            let expected_generation = frame_index as u32 + 1;

            // A dynamic layer must show this frame's bytes, not the ones the
            // session opened with (issue #674). The broker writes
            // `slot + frame_index` into the first byte before sending the
            // frame, so a worker that never re-read, or an update that did not
            // land, fails here instead of rendering a stale map.
            for (slot, handle, bytes) in &dynamic_layers {
                unsafe {
                    SetFilePointer(*handle, 0, null_mut(), FILE_BEGIN);
                }
                let mut buffer = vec![0u8; *bytes];
                if !read_exact(*handle, &mut buffer)
                    || buffer[0] != (*slot as u8).wrapping_add(frame_index as u8)
                {
                    return EXIT_PROTOCOL_VIOLATION;
                }
            }

            match behavior.as_str() {
                "hang_frame" => loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                },
                "modal_frame" => show_modal_dialog_and_wait(),
                "crash_frame" => std::process::exit(0xC000_0005_u32 as i32),
                "crash_frame_minidump" => {
                    // Same access-violation death as `crash_frame`, but first
                    // stream an opt-in minidump through the broker-inherited
                    // pipe so the session capture path is exercised end to end.
                    write_opt_in_minidump();
                    std::process::exit(0xC000_0005_u32 as i32);
                }
                "bad_framing_frame_0" => {
                    // A zero-length prefix from a worker that then stays
                    // alive: only an explicit reader-violation event can
                    // surface this before the frame deadline.
                    let zero = [0u8; 4];
                    let _ = write_all(response, &zero);
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(3600));
                    }
                }
                "exit_leaving_descendant" => {
                    // Rust spawns with bInheritHandles=TRUE, so the sleeping
                    // child keeps the inherited (still inheritable) session
                    // pipe handles open across this process's death.
                    let exe = std::env::current_exe().expect("own path");
                    let child = std::process::Command::new(exe)
                        .arg("--sleep-child")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    if child.is_ok() {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        std::process::exit(9);
                    }
                    return EXIT_PROTOCOL_VIOLATION;
                }
                _ => {}
            }
            if behavior == "error_frame_0" && frame_index == 0 {
                let reply = format!(
                    "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                     \"status\":\"error\",\"render_error\":-40}}"
                );
                if !write_message(response, &reply) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                continue;
            }
            if matches!(
                behavior.as_str(),
                "smart_output_untouched"
                    | "smart_output_untouched_wrong_error"
                    | "smart_output_untouched_dependency"
                    | "smart_output_untouched_return_message"
                    | "smart_output_untouched_ok"
            ) && frame_index == 0
            {
                let render_error = if behavior == "smart_output_untouched" {
                    -6
                } else {
                    -40
                };
                let reply = match behavior.as_str() {
                    "smart_output_untouched_dependency" => format!(
                        "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                         \"status\":\"error\",\"render_error\":-6,\
                         \"missing_dependency\":\"runtime.dll\",\
                         \"smart_output_untouched\":true}}"
                    ),
                    "smart_output_untouched_return_message" => format!(
                        "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                         \"status\":\"error\",\"render_error\":-6,\
                         \"return_message\":{{\"text\":\"failed\",\"display_requested\":false}},\
                         \"smart_output_untouched\":true}}"
                    ),
                    "smart_output_untouched_ok" => format!(
                        "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                         \"status\":\"ok\",\"render_error\":0,\
                         \"smart_output_untouched\":true}}"
                    ),
                    _ => format!(
                        "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                         \"status\":\"error\",\"render_error\":{render_error},\
                         \"smart_output_untouched\":true}}"
                    ),
                };
                if !write_message(response, &reply) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                continue;
            }
            if behavior == "fatal_error_frame_0" && frame_index == 0 {
                // Mirrors the real worker's invariant path: a reserved fatal
                // session error response followed by a fail-closed exit.
                let reply = format!(
                    "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                     \"status\":\"error\",\"render_error\":-43}}"
                );
                let _ = write_message(response, &reply);
                std::process::exit(EXIT_INVARIANT_FAILURE);
            }
            if behavior == "error_mutates_header" && frame_index == 0 {
                view.write_u32(MAX_WIDTH_OFFSET, width as u32 + 1);
                let reply = format!(
                    "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                     \"status\":\"error\",\"render_error\":-40}}"
                );
                if !write_message(response, &reply) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                continue;
            }
            if behavior == "empty_result_frame_0" && frame_index == 0 {
                // A legally empty SmartFX result (#278): no slot write, advance
                // the output generation like a normal frame, and report a valid
                // 0x0 ok frame with the explicit empty_result flag. Nothing was
                // packed, which is what the broker reads for an empty frame.
                view.write_u32(FRAME_WIDTH_OFFSET, 0);
                view.write_u32(FRAME_HEIGHT_OFFSET, 0);
                view.write_u32(OUTPUT_GENERATION_OFFSET, expected_generation);
                let reply = format!(
                    "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                     \"status\":\"ok\",\"output\":{{\"width\":0,\"height\":0,\"rowbytes\":0,\
                     \"pixel_format\":\"argb8\",\"packed_bytes\":0,\
                     \"guards_intact\":true,\"empty_result\":true}},\"render_error\":0,\
                     \"generation\":{expected_generation}}}"
                );
                if !write_message(response, &reply) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                continue;
            }
            if scale != time_scale {
                let reply = format!(
                    "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                     \"status\":\"error\",\"render_error\":-40}}"
                );
                if !write_message(response, &reply) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                continue;
            }
            if view.read_u32(INPUT_GENERATION_OFFSET) != expected_generation {
                return EXIT_INVARIANT_FAILURE;
            }
            if matches!(
                behavior.as_str(),
                "heap_corruption_after_frame_0" | "heap_corruption_on_close_after_frame_0"
            ) && frame_index == 0
            {
                eprintln!("stage:smart_render_begin");
                let mut stderr = std::io::stderr();
                let _ = std::io::Write::flush(&mut stderr);
            }
            let mut output = vec![0u8; slot_bytes];
            unsafe {
                std::ptr::copy_nonoverlapping(
                    view.0.add(input_offset),
                    output.as_mut_ptr(),
                    slot_bytes,
                );
            }
            for byte in &mut output {
                *byte = 255 - *byte;
            }
            // Stamp the received per-frame attribute digests into the frame so
            // integration tests can prove the v:2 fields actually reached the
            // worker (the real worker proves this by rendering with them):
            // `parameters` into bytes [0,32), `ui_action` into [32,64).
            if let Some(payload) = &frame_parameters {
                let digest = Sha256::digest(payload.as_bytes());
                let stamp = digest.len().min(output.len());
                output[..stamp].copy_from_slice(&digest[..stamp]);
            }
            if let Some(action) = &frame_ui_action {
                let digest = Sha256::digest(action.as_bytes());
                if output.len() >= 64 {
                    output[32..64].copy_from_slice(&digest);
                }
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    output.as_ptr(),
                    view.0.add(output_offset),
                    slot_bytes,
                );
            }
            frames += 1;

            let mut reported_generation = expected_generation;
            let mut packed_bytes = output.len();
            match behavior.as_str() {
                "stale_generation" => {
                    reported_generation = frame_index as u32;
                    // Deliberately leaves output_generation untouched.
                }
                "mutate_header" => {
                    view.write_u32(MAX_WIDTH_OFFSET, width as u32 + 1);
                    view.write_u32(OUTPUT_GENERATION_OFFSET, expected_generation);
                }
                "bad_extent" => {
                    packed_bytes = output.len() + 4;
                    view.write_u32(FRAME_WIDTH_OFFSET, width as u32);
                    view.write_u32(FRAME_HEIGHT_OFFSET, height as u32);
                    view.write_u32(OUTPUT_GENERATION_OFFSET, expected_generation);
                }
                _ => {
                    view.write_u32(FRAME_WIDTH_OFFSET, width as u32);
                    view.write_u32(FRAME_HEIGHT_OFFSET, height as u32);
                    view.write_u32(OUTPUT_GENERATION_OFFSET, expected_generation);
                }
            }
            let reply = format!(
                "{{\"v\":1,\"type\":\"frame_done\",\"frame_index\":{frame_index},\
                 \"status\":\"ok\",\"output\":{{\"width\":{width},\"height\":{height},\
                 \"rowbytes\":{rowbytes},\"pixel_format\":\"argb8\",\"packed_bytes\":{packed_bytes},\
                 \"guards_intact\":true}},\"render_error\":0,\"generation\":{reported_generation}}}",
                rowbytes = width * 4,
            );
            if !write_message(response, &reply) {
                return EXIT_PROTOCOL_VIOLATION;
            }
            if behavior == "heap_corruption_after_frame_0" && frame_index == 0 {
                std::process::exit(0xC000_0374_u32 as i32);
            }
            if behavior == "exit_after_frame_0" && frame_index == 0 {
                // A unilateral exit with a clean-looking report and exit code
                // 0, violating only the close-handshake contract.
                println!(
                    "{}",
                    final_report(
                        frames,
                        smart,
                        &args[4],
                        session_module_audit(
                            &cluster_manifest,
                            current_plugin,
                            &visited,
                            &swap_epochs,
                            &behavior
                        ),
                        &behavior
                    )
                );
                return 0;
            }
        }
        println!(
            "{}",
            final_report(
                frames,
                smart,
                &args[4],
                session_module_audit(
                    &cluster_manifest,
                    current_plugin,
                    &visited,
                    &swap_epochs,
                    &behavior
                ),
                &behavior
            )
        );
        0
    }

    /// The module audit for the session's final report: the cluster epoch
    /// audit when a cluster manifest rode the launch, the single-plugin
    /// default otherwise.
    fn session_module_audit(
        cluster_manifest: &Option<ClusterManifest>,
        current_plugin: usize,
        visited: &[usize],
        swap_epochs: &[(usize, usize)],
        behavior: &str,
    ) -> Value {
        match cluster_manifest {
            Some(manifest) => {
                cluster_module_audit(manifest, current_plugin, visited, swap_epochs, behavior)
            }
            None => default_module_audit(),
        }
    }

    /// The discovery session loop (issue #405, design §4.2): no plugin is
    /// loaded at launch; each `inspect_plugin` swaps to plugins[N] (recording
    /// the swap epoch, like the render session's swap) and answers with the
    /// parameter report the one-shot `--l2-params-only` would print.
    fn run_discovery(
        request: HANDLE,
        response: HANDLE,
        manifest: &ClusterManifest,
        behavior: &str,
    ) -> i32 {
        let mut current: Option<usize> = None;
        let mut visited: Vec<usize> = Vec::new();
        let mut epochs: Vec<(usize, usize)> = Vec::new();
        let mut next_request_index: u64 = 0;
        loop {
            let mut prefix = [0u8; 4];
            if !read_exact(request, &mut prefix) {
                break;
            }
            let length = u32::from_le_bytes(prefix) as usize;
            if length == 0 || length > MAX_MESSAGE_BYTES {
                return EXIT_PROTOCOL_VIOLATION;
            }
            let mut body = vec![0u8; length];
            if !read_exact(request, &mut body) {
                return EXIT_PROTOCOL_VIOLATION;
            }
            let Ok(message) = serde_json::from_slice::<Value>(&body) else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            match message["type"].as_str() {
                Some("close") => break,
                Some("inspect_plugin") => {}
                _ => return EXIT_PROTOCOL_VIOLATION,
            }
            // Exact-key strictness (design §4.2): only v, type, plugin_index,
            // and request_index may ride the message; the index must select a
            // manifest member and the request serial must advance.
            let keys_ok = message.as_object().map(|object| object.len()) == Some(4)
                && message.get("v").is_some()
                && message.get("plugin_index").is_some()
                && message.get("request_index").is_some();
            let (Some(plugin_index), Some(request_index)) = (
                message["plugin_index"].as_u64(),
                message["request_index"].as_u64(),
            ) else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            if !keys_ok
                || message["v"].as_u64() != Some(1)
                || plugin_index as usize >= manifest.plugins.len()
                || request_index != next_request_index
            {
                return EXIT_PROTOCOL_VIOLATION;
            }
            next_request_index += 1;
            if behavior == "crash_on_inspect" {
                std::process::exit(0xC000_0005_u32 as i32);
            }
            let new_index = plugin_index as usize;
            if matches!(
                behavior,
                "checkpoint_then_crash"
                    | "checkpoint_then_done"
                    | "duplicate_checkpoint"
                    | "wrong_checkpoint_sha"
                    | "checkpoint_bad_status"
                    | "checkpoint_bad_setup"
                    | "checkpoint_bad_params"
                    | "checkpoint_bad_setdown"
                    | "checkpoint_missing_parameters"
                    | "checkpoint_then_noncrash_exit"
            ) {
                let (_, manifest_sha256) = &manifest.plugins[new_index];
                let mut checkpoint = json!({
                    "v": 1,
                    "type": "inspect_checkpoint",
                    "plugin_index": plugin_index,
                    "request_index": request_index,
                    "plugin_sha256": if behavior == "wrong_checkpoint_sha" {
                        "00".repeat(32)
                    } else {
                        manifest_sha256.clone()
                    },
                    "report": {
                        "status": "parameters_inspected_pre_setdown",
                        "global_setup_error": 0,
                        "params_setup_error": 0,
                        "global_setdown_error": -1,
                        "parameters": []
                    }
                });
                match behavior {
                    "checkpoint_bad_status" => checkpoint["report"]["status"] = json!("wrong"),
                    "checkpoint_bad_setup" => checkpoint["report"]["global_setup_error"] = json!(4),
                    "checkpoint_bad_params" => {
                        checkpoint["report"]["params_setup_error"] = json!(4)
                    }
                    "checkpoint_bad_setdown" => {
                        checkpoint["report"]["global_setdown_error"] = json!(0)
                    }
                    "checkpoint_missing_parameters" => {
                        checkpoint["report"]
                            .as_object_mut()
                            .unwrap()
                            .remove("parameters");
                    }
                    _ => {}
                }
                let checkpoint = checkpoint.to_string();
                if !write_message(response, &checkpoint) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                if behavior == "duplicate_checkpoint" && !write_message(response, &checkpoint) {
                    return EXIT_PROTOCOL_VIOLATION;
                }
                if behavior == "checkpoint_then_crash" {
                    std::process::exit(0xC000_0005_u32 as i32);
                }
                if behavior == "checkpoint_then_noncrash_exit" {
                    std::process::exit(7);
                }
            }
            if current != Some(new_index) {
                if let Some(old_index) = current {
                    epochs.push((old_index, new_index));
                }
                visited.push(new_index);
                current = Some(new_index);
            }
            let reply = if behavior == "inspect_error_plugin_1" && new_index == 1 {
                // Parameter-local failure (design §4.2), in the shape the
                // real worker sends: a structured error_kind, no report. The
                // session continues; continuing is the broker's decision.
                json!({
                    "v": 1,
                    "type": "inspect_done",
                    "plugin_index": plugin_index,
                    "request_index": request_index,
                    "status": "error",
                    "error_kind": "selector_error"
                })
                .to_string()
            } else if behavior == "inspect_entrypoint_aegp_plugin_1" && new_index == 1 {
                json!({
                    "v": 1,
                    "type": "inspect_done",
                    "plugin_index": plugin_index,
                    "request_index": request_index,
                    "status": "error",
                    "error_kind": "entrypoint_unresolved",
                    "report": { "plugin_kind": "aegp_candidate" }
                })
                .to_string()
            } else if behavior == "inspect_identity_changed_plugin_1" && new_index == 1 {
                // In-place identity mismatch (issue #751): the bytes on disk
                // no longer match the manifest — plug-in-local, the session
                // continues (the #309 state transition, never a session
                // failure).
                json!({
                    "v": 1,
                    "type": "inspect_done",
                    "plugin_index": plugin_index,
                    "request_index": request_index,
                    "status": "error",
                    "error_kind": "identity_changed"
                })
                .to_string()
            } else {
                let (basename, sha256) = &manifest.plugins[new_index];
                json!({
                    "v": 1,
                    "type": "inspect_done",
                    "plugin_index": plugin_index,
                    "request_index": request_index,
                    "status": "ok",
                    "report": {
                        "status": "inspected",
                        "plugin": {"basename": basename, "sha256": sha256},
                        "parameters": []
                    }
                })
                .to_string()
            };
            if !write_message(response, &reply) {
                return EXIT_PROTOCOL_VIOLATION;
            }
        }
        let module_audit = match current {
            Some(current) => cluster_module_audit(manifest, current, &visited, &epochs, behavior),
            None => {
                // Nothing was ever inspected: the honest audit reports only
                // the pinned closure. With an empty closure the union carries
                // no plugin-class module and the broker's audit fails closed,
                // which is the correct verdict for an inspect-less session.
                let deps_only = json!({
                    "status": "passed",
                    "unknown_count": 0,
                    "worker": ["session_protocol_worker.exe"],
                    "plugin": [],
                    "system32": ["kernel32.dll"]
                });
                json!({
                    "schema": 1,
                    "status": "passed",
                    "phase_count": 3,
                    "unknown_count": 0,
                    "post_load": deps_only,
                    "pre_unload": deps_only,
                    "observed_union": deps_only,
                    "epochs": []
                })
            }
        };
        println!(
            "{}",
            json!({
                "schema_version": 1,
                "stage": "discovery_session",
                "status": "discovery_session_completed",
                "module_audit": module_audit
            })
        );
        0
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(worker::run());
}

#[cfg(not(windows))]
fn main() {
    std::process::exit(2);
}
