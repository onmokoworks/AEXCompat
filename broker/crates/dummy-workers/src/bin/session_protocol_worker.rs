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
//! - `exit_leaving_descendant`: spawns a sleeping child that inherits the
//!   session handles, then exits; pipe EOF never fires, only the process
//!   watcher can see the death (protocol §7 three-way wait).
//! - `stale_generation`: answers with a stale generation and no header update.
//! - `mutate_header`: rewrites a broker-owned static header field.
//! - `bad_checksum`: reports a checksum that does not match the slot bytes.
//! - `error_frame_0`: answers frame 0 with a frame-local error response.

#[cfg(windows)]
mod worker {
    use sha2::{Digest, Sha256};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Memory::{MapViewOfFile, FILE_MAP_ALL_ACCESS};

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
            let _ = write!(std::io::stderr(), "stage:minidump_written bytes={}\n", payload.len());
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

    fn final_report(frames: u32, smart: bool) -> String {
        // The broker validates a module audit on a clean exit exactly like the
        // one-shot path; this fixture reports its own honest minimal audit.
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
            "session_frames": frames,
            "guard_bytes_intact": true,
            "suite_leases_balanced": true,
            "handle_lifetimes_balanced": true,
            "world_lifetimes_balanced": true,
            "param_checkouts_balanced": true,
            "module_audit": {
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
            }
        });
        report
            .as_object_mut()
            .unwrap()
            .extend(mechanics.as_object().unwrap().clone());
        report.to_string()
    }

    pub fn run() -> i32 {
        let args: Vec<String> = std::env::args().collect();
        if args.len() == 2 && args[1] == "--sleep-child" {
            std::thread::sleep(std::time::Duration::from_secs(120));
            return 0;
        }
        // Trailing auxiliary option pairs mirror the real worker's
        // strip_auxiliary_options contract: peel them off the tail, and for
        // --parameter-animation-v1 enforce the native loader's pin — the
        // sidecar must exist AND its parent must canonicalize to the worker's
        // cwd + target/image-transport (parameter_animation_transport.cpp) —
        // so a broker writing the sidecar somewhere the real worker would
        // reject fails these tests too.
        let mut effective = args.len();
        while effective >= 12 && args[effective - 2].starts_with("--") {
            let value = &args[effective - 1];
            match args[effective - 2].as_str() {
                "--parameter-animation-v1" => {
                    let sidecar = std::path::Path::new(value);
                    let pinned = std::env::current_dir()
                        .ok()
                        .and_then(|cwd| cwd.join("target/image-transport").canonicalize().ok());
                    let parent = sidecar.parent().and_then(|parent| parent.canonicalize().ok());
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
                    let schema_ok = object.get("schema").and_then(|v| v.as_str())
                        == Some("aux-manifest-v1");
                    let nonce_ok = object
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
                "--output-checksum-detail-v1" => {
                    if value != "1" {
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
        let (Ok(width), Ok(height), Ok(time_scale)) = (
            args[5].parse::<usize>(),
            args[6].parse::<usize>(),
            args[9].parse::<u32>(),
        ) else {
            return 2;
        };
        let behavior = std::env::var("AEXCOMPAT_TEST_SESSION_BEHAVIOR").unwrap_or_default();
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
        if let Some(trailer) = &layer_trailer {
            for entry in trailer[LAYER_TRAILER_PREFIX.len()..].split(';') {
                let fields: Vec<&str> = entry.split(',').collect();
                if fields.len() != 4 && fields.len() != 6 {
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
                    fields.last().map(|s| s.parse::<usize>()),
                ) else {
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
                unsafe {
                    CloseHandle(handle);
                }
            }
        }

        let mut frames = 0u32;
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
                Some("close") => break,
                Some("render_frame") => {}
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

            match behavior.as_str() {
                "hang_frame" => loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                },
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
                std::ptr::copy_nonoverlapping(output.as_ptr(), view.0.add(output_offset), slot_bytes);
            }
            frames += 1;

            let mut reported_generation = expected_generation;
            let mut checksum = format!("{:x}", Sha256::digest(&output));
            match behavior.as_str() {
                "stale_generation" => {
                    reported_generation = frame_index as u32;
                    // Deliberately leaves output_generation untouched.
                }
                "mutate_header" => {
                    view.write_u32(MAX_WIDTH_OFFSET, width as u32 + 1);
                    view.write_u32(OUTPUT_GENERATION_OFFSET, expected_generation);
                }
                "bad_checksum" => {
                    checksum = "0".repeat(64);
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
                 \"rowbytes\":{rowbytes},\"pixel_format\":\"argb8\",\"checksum\":\"{checksum}\",\
                 \"guards_intact\":true}},\"render_error\":0,\"generation\":{reported_generation}}}",
                rowbytes = width * 4,
            );
            if !write_message(response, &reply) {
                return EXIT_PROTOCOL_VIOLATION;
            }
            if behavior == "exit_after_frame_0" && frame_index == 0 {
                // A unilateral exit with a clean-looking report and exit code
                // 0, violating only the close-handshake contract.
                println!("{}", final_report(frames, smart));
                return 0;
            }
        }
        println!("{}", final_report(frames, smart));
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
