//! Audio render session protocol test fixture (issue #239).
//!
//! Speaks the worker side of docs/RENDER_SESSION_PROTOCOL_2026-07-19.md §10
//! over the inherited audio transport exactly like
//! `aex_render_worker --render-audio-session-v1`, so broker `AudioRenderSession`
//! integration tests can exercise the render_span protocol, generation
//! checking, and close handshake against a real isolated process without a
//! native minihost build. The "render" negates each f32 input sample.

#[cfg(windows)]
mod worker {
    use sha2::{Digest, Sha256};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile};

    const HEADER_BYTES: usize = 4096;
    const SLOT_ALIGNMENT: usize = 4096;
    const HEADER_MAGIC: u32 = 0x5355_4141; // "AAUS"
    const MAX_MESSAGE_BYTES: usize = 64 * 1024;
    const MAGIC_OFFSET: usize = 0;
    const VERSION_OFFSET: usize = 4;
    const MAX_SAMPLES_OFFSET: usize = 8;
    const CHANNELS_OFFSET: usize = 12;
    const INPUT_GENERATION_OFFSET: usize = 16;
    const OUTPUT_GENERATION_OFFSET: usize = 20;
    const OUTPUT_SAMPLES_OFFSET: usize = 28;

    const EXIT_PROTOCOL_VIOLATION: i32 = 23;

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

    fn align(bytes: usize) -> usize {
        (bytes + SLOT_ALIGNMENT - 1) / SLOT_ALIGNMENT * SLOT_ALIGNMENT
    }

    fn final_report(requests: u32, clean: bool) -> String {
        serde_json::json!({
            "schema_version": 1,
            "stage": "audio_session",
            "status": if clean { "session_completed" } else { "session_failed" },
            "global_setup_error": 0,
            "params_setup_error": 0,
            "session_requests_ok": requests,
            "session_protocol_violation": !clean,
            "session_invariant_failure": false,
            "global_setdown_error": 0,
            "audio_lifetimes_balanced": true,
            "invalid_audio_operations": 0,
            "session_clean": clean,
            "module_audit": {
                "schema": 1, "status": "passed", "phase_count": 3, "unknown_count": 0,
                "post_load": {"status": "passed", "unknown_count": 0,
                    "worker": ["audio_session_protocol_worker.exe"],
                    "plugin": ["plugin.plugin"], "system32": ["kernel32.dll"]},
                "pre_unload": {"status": "passed", "unknown_count": 0,
                    "worker": ["audio_session_protocol_worker.exe"],
                    "plugin": ["plugin.plugin"], "system32": ["kernel32.dll"]},
                "observed_union": {"status": "passed", "unknown_count": 0,
                    "worker": ["audio_session_protocol_worker.exe"],
                    "plugin": ["plugin.plugin"], "system32": ["kernel32.dll"]}
            }
        })
        .to_string()
    }

    pub fn run() -> i32 {
        // In-place dependency search directories (issue #751): the real
        // worker's apply_dependency_search_dirs requires non-empty absolute
        // directories joined by ';', bounded at 16. Mirror the image session
        // fixture's shape gate so a malformed broker join fails the audio
        // in-place tests too instead of passing silently.
        let args: Vec<String> = std::env::args().collect();
        if let Some(position) = args.iter().position(|arg| arg == "--dependency-dirs-v1") {
            let Some(value) = args.get(position + 1) else {
                return 3;
            };
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
        // Reuses the image session's inherited-handle env names (the broker's
        // SessionChildHandles sets these); the audio session is distinguished by
        // the CLI command word, not the env name.
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
        if view.read_u32(MAGIC_OFFSET) != HEADER_MAGIC || view.read_u32(VERSION_OFFSET) != 1 {
            return EXIT_PROTOCOL_VIOLATION;
        }
        let max_samples = view.read_u32(MAX_SAMPLES_OFFSET) as usize;
        let channels = view.read_u32(CHANNELS_OFFSET) as usize;
        let input_offset = HEADER_BYTES;
        let output_offset = HEADER_BYTES + align(max_samples * channels * 4);

        // Test-only misbehavior: report an output window that runs past the
        // submitted input span so the broker's range validation must reject it
        // (Codex #252). Read once; the broker sets this env on the child.
        let out_of_range_start = std::env::var("AEXCOMPAT_TEST_SESSION_BEHAVIOR").as_deref()
            == Ok("audio_out_of_range_start");

        let mut requests_ok = 0u32;
        loop {
            let mut prefix = [0u8; 4];
            if !read_exact(request, &mut prefix) {
                // Clean EOF on a frame boundary is the broker's close signal.
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
            let message: serde_json::Value = match serde_json::from_slice(&body) {
                Ok(value) => value,
                Err(_) => return EXIT_PROTOCOL_VIOLATION,
            };
            match message["type"].as_str() {
                Some("close") => break,
                Some("audio_render") => {}
                _ => return EXIT_PROTOCOL_VIOLATION,
            }
            if message["v"].as_u64() != Some(1) {
                return EXIT_PROTOCOL_VIOLATION;
            }
            let Some(request_index) = message["request_index"].as_u64() else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            let Some(input_samples) = message["input_samples"].as_u64() else {
                return EXIT_PROTOCOL_VIOLATION;
            };
            let input_samples = input_samples as usize;
            if input_samples > max_samples * channels {
                return EXIT_PROTOCOL_VIOLATION;
            }
            let expected_generation = request_index as u32 + 1;
            if view.read_u32(INPUT_GENERATION_OFFSET) != expected_generation {
                return EXIT_PROTOCOL_VIOLATION;
            }
            // "Render": negate each input sample into the output slot.
            let mut output_bytes = vec![0u8; input_samples * 4];
            for index in 0..input_samples {
                let mut sample = [0u8; 4];
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        view.0.add(input_offset + index * 4),
                        sample.as_mut_ptr(),
                        4,
                    );
                }
                let negated = (-f32::from_le_bytes(sample)).to_le_bytes();
                output_bytes[index * 4..index * 4 + 4].copy_from_slice(&negated);
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    output_bytes.as_ptr(),
                    view.0.add(output_offset),
                    output_bytes.len(),
                );
            }
            view.write_u32(OUTPUT_SAMPLES_OFFSET, input_samples as u32);
            view.write_u32(OUTPUT_GENERATION_OFFSET, expected_generation);
            requests_ok += 1;
            let checksum = format!("{:x}", Sha256::digest(&output_bytes));
            let start_sample: i64 = if out_of_range_start {
                input_samples as i64 + 1
            } else {
                0
            };
            let reply = format!(
                "{{\"v\":1,\"type\":\"audio_done\",\"request_index\":{request_index},\
                 \"status\":\"ok\",\"output\":{{\"start_sample\":{start_sample},\"sample_count\":{input_samples},\"rate\":44100,\
                 \"channels\":{channels},\"sample_size\":4,\"checksum\":\"{checksum}\",\
                 \"guards_intact\":true}},\"audio_render_error\":0,\"generation\":{expected_generation}}}"
            );
            if !write_message(response, &reply) {
                return EXIT_PROTOCOL_VIOLATION;
            }
        }
        println!("{}", final_report(requests_ok, true));
        0
    }
}

#[cfg(windows)]
fn main() {
    std::process::exit(worker::run());
}

#[cfg(not(windows))]
fn main() {}
