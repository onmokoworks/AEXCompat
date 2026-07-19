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
    use windows_sys::Win32::Foundation::HANDLE;
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

    fn final_report(frames: u32) -> String {
        // The broker validates a module audit on a clean exit exactly like the
        // one-shot path; this fixture reports its own honest minimal audit.
        serde_json::json!({
            "status": "render_completed",
            "render_error": 0,
            "global_setdown_error": 0,
            "session_frames": frames,
            // The clean-close contract fields the broker validates, mirroring
            // the real worker's final report.
            "persistent_sequence_setup_error": 0,
            "persistent_sequence_setdown_error": 0,
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
                    "plugin": ["plugin.aex"],
                    "system32": ["kernel32.dll"]
                },
                "pre_unload": {
                    "status": "passed", "unknown_count": 0,
                    "worker": ["session_protocol_worker.exe"],
                    "plugin": ["plugin.aex"],
                    "system32": ["kernel32.dll"]
                },
                "observed_union": {
                    "status": "passed", "unknown_count": 0,
                    "worker": ["session_protocol_worker.exe"],
                    "plugin": ["plugin.aex"],
                    "system32": ["kernel32.dll"]
                }
            }
        })
        .to_string()
    }

    pub fn run() -> i32 {
        let args: Vec<String> = std::env::args().collect();
        if args.len() == 2 && args[1] == "--sleep-child" {
            std::thread::sleep(std::time::Duration::from_secs(120));
            return 0;
        }
        if args.len() != 10 || args[1] != "--render-session-v1" {
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
            || view.read_u32(VERSION_OFFSET) != 1
            || view.read_u32(DEPTH_CODE_OFFSET) != 8
            || view.read_u32(MAX_WIDTH_OFFSET) != width as u32
            || view.read_u32(MAX_HEIGHT_OFFSET) != height as u32
            || view.read_u32(LAYER_SLOT_COUNT_OFFSET) != 0
        {
            return EXIT_PROTOCOL_VIOLATION;
        }
        let slot_bytes = width * height * 4;
        let aligned = |bytes: usize| bytes.div_ceil(SLOT_ALIGNMENT) * SLOT_ALIGNMENT;
        let input_offset = HEADER_BYTES;
        let output_offset = HEADER_BYTES + aligned(slot_bytes);

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
                println!("{}", final_report(frames));
                return 0;
            }
        }
        println!("{}", final_report(frames));
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
