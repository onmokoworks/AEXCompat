//! Resident render session (issue #98 stage 1 PR-C,
//! docs/RENDER_SESSION_PROTOCOL_2026-07-19.md).
//!
//! The broker opens one sealed worker process per session, keeps the trust
//! artifacts (sealed tree, staged worker, restricted token) alive for the
//! session's lifetime, and drives a frame loop over the inherited transport:
//! two anonymous pipes carrying length-prefixed JSON control messages and one
//! anonymous file mapping carrying copy-through pixel slots. The worker never
//! opens a path for session transport (issue #18 lesson).
//!
//! Safety boundaries stay per-frame: every `render_frame` arms a deadline and
//! a dead worker, a stale generation, a mutated header, a checksum mismatch,
//! or an out-of-bounds geometry invalidates the whole session fail-closed.

use crate::image_render::{
    decode_bounded_image, decode_sha256_hex, encode_interactive_payload,
    isolated_worker_diagnostics, native_rgba_to_preview, parameter_animation_sidecar_json,
    validate_animation_bindings, InteractiveParameter, ParameterAnimation, RenderPixelFormat,
    INTERACTIVE_RENDER_TIMEOUT_MS, MAX_DIMENSION, MAX_PIXELS, MAX_RGBA_TRANSPORT_BYTES,
};
use crate::secure_image_dispatch::{
    dispatch_secure_image_session, ApprovedImageArtifact, SecureImageDispatch, WorkerKind,
};
use crate::secure_launch::{SecureLaunchResult, SecureSessionProcess};
use crate::windows_process::SessionChildHandles;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
    MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;

// Protocol constants mirroring the worker (`worker_render_session.hpp`).
const HEADER_BYTES: usize = 4096;
const SLOT_ALIGNMENT: usize = 4096;
const HEADER_MAGIC: u32 = 0x5358_4541; // "AEXS" little-endian
const PROTOCOL_VERSION: u32 = 1;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Fail-closed cap on the whole section for pathological configurations
/// (protocol §6); ordinary full-HD sessions stay far below it.
const SECTION_HARD_CAP_BYTES: u64 = 1 << 30;
pub const EXIT_PROTOCOL_VIOLATION: u32 = 23;
pub const EXIT_INVARIANT_FAILURE: u32 = 24;

/// The worker's reserved session error codes that accompany a host-protection
/// invariant failure (`l2_main.cpp` `kSessionGenerationMismatch` ..
/// `kSessionOutputValidationError`). The worker exits fail-closed right after
/// sending such a response, so the broker must invalidate the session rather
/// than surface them as reusable frame-local diagnostics. Time-scale (-40)
/// and time-range (-46) rejections stay frame-local by the worker's contract.
fn is_fatal_session_error(render_error: i64) -> bool {
    matches!(render_error, -45..=-41)
}

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

const MAX_BATCH_FRAMES: usize = 10_000;
const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const CLOSE_COLLECT_TIMEOUT: Duration = Duration::from_millis(INTERACTIVE_RENDER_TIMEOUT_MS);
/// After a broker-initiated job termination the process is already gone;
/// collection just drains readers and accounting.
const POST_TERMINATION_COLLECT_TIMEOUT: Duration = Duration::from_secs(10);

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn align_slot(bytes: usize) -> usize {
    bytes.div_ceil(SLOT_ALIGNMENT) * SLOT_ALIGNMENT
}

fn depth_code(pixel_format: RenderPixelFormat) -> u32 {
    match pixel_format {
        RenderPixelFormat::Argb8 => 8,
        RenderPixelFormat::Argb16 => 16,
        RenderPixelFormat::Argb32f => 32,
    }
}

fn session_command(pixel_format: RenderPixelFormat) -> &'static str {
    match pixel_format {
        RenderPixelFormat::Argb8 => "--render-session-v1",
        RenderPixelFormat::Argb16 => "--render-session16-v1",
        RenderPixelFormat::Argb32f => "--render-session32-v1",
    }
}

struct OwnedHandle(HANDLE);
impl OwnedHandle {
    fn new(value: HANDLE) -> io::Result<Self> {
        if value.is_null() || value == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(value))
        }
    }
    fn raw(&self) -> HANDLE {
        self.0
    }
    fn close_now(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
            self.0 = null_mut();
        }
    }
    fn take(mut self) -> HANDLE {
        let value = self.0;
        self.0 = null_mut();
        value
    }
}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        self.close_now();
    }
}

fn inheritable_security() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    }
}

fn inheritable_pipe(broker_end_is_read: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read: HANDLE = null_mut();
    let mut write: HANDLE = null_mut();
    let mut security = inheritable_security();
    if unsafe { CreatePipe(&mut read, &mut write, &mut security, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let read = OwnedHandle::new(read)?;
    let write = OwnedHandle::new(write)?;
    // Only the child-side end stays inheritable, matching windows_process.rs.
    let broker_end = if broker_end_is_read { read.raw() } else { write.raw() };
    if unsafe { SetHandleInformation(broker_end, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((read, write))
}

fn read_exact_handle(handle: HANDLE, destination: &mut [u8]) -> bool {
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

fn write_all_handle(handle: HANDLE, source: &[u8]) -> bool {
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

/// Broker-side transport: the mapped section view plus the request pipe write
/// end. The response pipe read end is owned by the reader thread.
struct SessionTransport {
    request_write: Option<OwnedHandle>,
    section: OwnedHandle,
    view: *mut u8,
    section_bytes: usize,
}

impl SessionTransport {
    fn read_header_u32(&self, offset: usize) -> u32 {
        debug_assert!(offset + 4 <= HEADER_BYTES);
        let mut bytes = [0u8; 4];
        unsafe {
            std::ptr::copy_nonoverlapping(self.view.add(offset), bytes.as_mut_ptr(), 4);
        }
        u32::from_le_bytes(bytes)
    }

    fn write_header_u32(&mut self, offset: usize, value: u32) {
        debug_assert!(offset + 4 <= HEADER_BYTES);
        unsafe {
            std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), self.view.add(offset), 4);
        }
    }

    fn write_input_slot(&mut self, rgba: &[u8]) {
        debug_assert!(HEADER_BYTES + rgba.len() <= self.section_bytes);
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), self.view.add(HEADER_BYTES), rgba.len());
        }
    }

    fn read_output_slot(&self, offset: usize, bytes: usize) -> Vec<u8> {
        debug_assert!(offset + bytes <= self.section_bytes);
        let mut output = vec![0u8; bytes];
        unsafe {
            std::ptr::copy_nonoverlapping(self.view.add(offset), output.as_mut_ptr(), bytes);
        }
        output
    }

    fn send_message(&mut self, payload: &str) -> bool {
        let Some(request_write) = self.request_write.as_ref() else {
            return false;
        };
        if payload.is_empty() || payload.len() > MAX_MESSAGE_BYTES {
            return false;
        }
        let prefix = (payload.len() as u32).to_le_bytes();
        write_all_handle(request_write.raw(), &prefix)
            && write_all_handle(request_write.raw(), payload.as_bytes())
    }

    fn close_request_pipe(&mut self) {
        self.request_write = None;
    }
}

impl Drop for SessionTransport {
    fn drop(&mut self) {
        if !self.view.is_null() {
            let address = MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view as *mut _,
            };
            unsafe {
                UnmapViewOfFile(address);
            }
        }
    }
}

/// One frame's worth of session geometry, computed identically on both sides
/// (protocol §6); the worker refuses to start when its own computation of the
/// expected section size is not covered by the mapped view.
#[derive(Clone, Copy)]
struct SessionGeometry {
    width: u32,
    height: u32,
    pixel_format: RenderPixelFormat,
}

impl SessionGeometry {
    fn input_slot_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
    fn output_slot_bytes(&self) -> usize {
        self.width as usize * self.height as usize * self.pixel_format.bytes_per_pixel() as usize
    }
    fn output_slot_offset(&self) -> usize {
        HEADER_BYTES + align_slot(self.input_slot_bytes())
    }
    fn section_bytes(&self) -> usize {
        self.output_slot_offset() + align_slot(self.output_slot_bytes())
    }
}

pub struct SessionOpenRequest<'a> {
    pub repository: &'a Path,
    pub plugin_path: &'a Path,
    pub plugin_sha256: &'a str,
    pub parameters: Option<&'a [InteractiveParameter]>,
    /// Parameter animation timeline evaluated by the worker at each frame's
    /// current_time (issue #132). Bindings are validated against `parameters`
    /// before launch, exactly like the one-shot entry.
    pub parameter_animation: Option<&'a [ParameterAnimation]>,
    pub dependencies: Vec<ApprovedImageArtifact>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: RenderPixelFormat,
    pub time_step: i32,
    pub total_time: i32,
    pub time_scale: u32,
    /// Per-frame watchdog deadline; the job is terminated when a frame's
    /// response does not arrive in time (protocol §7).
    pub frame_deadline: Duration,
}

#[derive(Debug)]
pub enum FrameStatus {
    /// The frame rendered and every per-frame invariant held. `pixels` are
    /// the validated native RGBA bytes copied out of the output slot;
    /// `checksum` is their lowercase SHA-256 (matching the worker's).
    Rendered { pixels: Vec<u8>, checksum: String },
    /// A frame-local compatibility diagnostic (selector error, time scale
    /// mismatch). The session stays usable; continuing is the caller's call.
    FrameError { render_error: i64 },
}

#[derive(Debug)]
pub struct FrameOutcome {
    pub frame_index: u32,
    pub status: FrameStatus,
}

/// Why a session stopped accepting frames. Everything here is fail-closed:
/// the worker job is dead (or being killed) by the time the reason is stored.
#[derive(Clone)]
pub struct SessionInvalidation {
    pub reason: &'static str,
    pub detail: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameDoneOutput {
    width: u32,
    height: u32,
    rowbytes: u64,
    pixel_format: String,
    checksum: String,
    guards_intact: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    frame_index: u32,
    status: String,
    #[serde(default)]
    output: Option<FrameDoneOutput>,
    render_error: i64,
    #[serde(default)]
    generation: Option<u32>,
}

struct CollectedExit {
    result: Option<SecureLaunchResult>,
    error: Option<String>,
}

/// Frame-wait events (protocol §7's three-way wait): a control message from
/// the reader thread, or the process-death watcher firing. The deadline is
/// the receive timeout itself.
enum SessionEvent {
    Message(Vec<u8>),
    /// The reader rejected the response framing (zero/oversized length or a
    /// truncated body). Explicit because the watcher keeps the channel open,
    /// so a disconnect can no longer signal this.
    ReaderViolation,
    ProcessExited,
}

/// How long a frame wait keeps draining after the process-death watcher
/// fires, so a frame_done the worker wrote just before dying (already in the
/// pipe buffer) is still honored instead of racing the watcher.
const PROCESS_EXIT_DRAIN: Duration = Duration::from_millis(500);

enum FrameWait {
    Message(Vec<u8>),
    Deadline,
    WorkerGone,
    FramingViolation,
}

pub struct RenderSession {
    process: Option<SecureSessionProcess>,
    collected: Option<CollectedExit>,
    transport: SessionTransport,
    receiver: mpsc::Receiver<SessionEvent>,
    process_exit_observed: bool,
    geometry: SessionGeometry,
    time_scale: u32,
    total_time: i32,
    frame_deadline: Duration,
    invalidation: Option<SessionInvalidation>,
    last_output_generation: u32,
    frames_ok: u32,
    frames_errored: u32,
    opened: Instant,
    plugin_sha256: String,
    /// Keeps the animation sidecar alive for the whole session; the worker
    /// reads it once at launch, but leaving transport files behind on drop
    /// would leak into target/image-transport.
    _animation_sidecar: Option<AnimationSidecar>,
}

struct AnimationSidecar(PathBuf);

impl Drop for AnimationSidecar {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl RenderSession {
    pub fn open(request: SessionOpenRequest<'_>) -> io::Result<RenderSession> {
        if request.time_step <= 0
            || request.total_time <= 0
            || request.time_scale == 0
            // The worker parses the per-frame current_time.scale as signed
            // 32-bit, so a larger launch time_scale could never render a
            // frame; reject it here instead of failing at the first frame.
            || request.time_scale > i32::MAX as u32
            || request.frame_deadline.is_zero()
        {
            return Err(invalid("render session timing is invalid"));
        }
        crate::render_request::validate_image_buffer_layout(
            u64::from(request.width),
            u64::from(request.height),
            u64::from(request.width) * 4,
            4,
            None,
            u64::from(MAX_DIMENSION),
            MAX_PIXELS,
            MAX_RGBA_TRANSPORT_BYTES,
        )?;
        let geometry = SessionGeometry {
            width: request.width,
            height: request.height,
            pixel_format: request.pixel_format,
        };
        if geometry.section_bytes() as u64 > SECTION_HARD_CAP_BYTES {
            return Err(invalid("render session section exceeds the hard cap"));
        }
        let payload = encode_interactive_payload(request.parameters.unwrap_or_default())?;
        // The sidecar mirrors the one-shot transport: validated bindings,
        // JSON under <repository>/target/image-transport (the only directory
        // the worker's strict sidecar loader accepts), removed when the
        // session ends.
        let animation_sidecar = match request
            .parameter_animation
            .filter(|animations| !animations.is_empty())
        {
            Some(animations) => {
                validate_animation_bindings(request.parameters.unwrap_or_default(), animations)?;
                let bytes = parameter_animation_sidecar_json(animations)?;
                let root = request.repository.join("target/image-transport");
                fs::create_dir_all(&root)?;
                let nonce = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|error| invalid(error.to_string()))?
                    .as_nanos();
                let path = root.join(format!("parameter-animation-session-{nonce}.json"));
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?
                    .write_all(&bytes)?;
                Some(AnimationSidecar(path))
            }
            None => None,
        };

        let (request_read, request_write) = inheritable_pipe(false)?;
        let (response_read, response_write) = inheritable_pipe(true)?;
        let section_bytes = geometry.section_bytes();
        let mut security = inheritable_security();
        let section = OwnedHandle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                &mut security,
                PAGE_READWRITE,
                ((section_bytes as u64) >> 32) as u32,
                section_bytes as u32,
                null(),
            )
        })?;
        let view_address = unsafe { MapViewOfFile(section.raw(), FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view_address.Value.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut transport = SessionTransport {
            request_write: Some(request_write),
            section,
            view: view_address.Value as *mut u8,
            section_bytes,
        };
        transport.write_header_u32(MAGIC_OFFSET, HEADER_MAGIC);
        transport.write_header_u32(VERSION_OFFSET, PROTOCOL_VERSION);
        transport.write_header_u32(DEPTH_CODE_OFFSET, depth_code(request.pixel_format));
        transport.write_header_u32(MAX_WIDTH_OFFSET, request.width);
        transport.write_header_u32(MAX_HEIGHT_OFFSET, request.height);
        transport.write_header_u32(LAYER_SLOT_COUNT_OFFSET, 0);
        transport.write_header_u32(INPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(OUTPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(FRAME_WIDTH_OFFSET, request.width);
        transport.write_header_u32(FRAME_HEIGHT_OFFSET, request.height);

        let plugin = ApprovedImageArtifact {
            path: request.plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(request.plugin_sha256)?,
            expected_size: fs::metadata(request.plugin_path)?.len(),
        };
        let args_before_plugin = vec![session_command(request.pixel_format).to_owned()];
        let mut args_after_plugin = vec![
            request.plugin_sha256.to_ascii_lowercase(),
            payload,
            request.width.to_string(),
            request.height.to_string(),
            request.time_step.to_string(),
            request.total_time.to_string(),
            request.time_scale.to_string(),
        ];
        if let Some(sidecar) = &animation_sidecar {
            // Auxiliary option pairs ride argv's tail; the worker peels them
            // before the positional session contract (strip_auxiliary_options).
            args_after_plugin.extend([
                "--parameter-animation-v1".to_owned(),
                sidecar.0.to_string_lossy().into_owned(),
            ]);
        }
        let process = dispatch_secure_image_session(
            SecureImageDispatch {
                repository: request.repository,
                worker_kind: WorkerKind::Render,
                plugin,
                dependencies: request.dependencies,
                args_before_plugin: &args_before_plugin,
                args_after_plugin: &args_after_plugin,
                timeout: request.frame_deadline,
            },
            &SessionChildHandles {
                request_read: request_read.raw(),
                response_write: response_write.raw(),
                section: transport.section.raw(),
            },
        )?;
        // The worker inherited its copies; dropping the broker's child-side
        // ends turns a worker exit into pipe EOF instead of a hang.
        drop(request_read);
        drop(response_write);

        let (sender, receiver) = mpsc::channel::<SessionEvent>();
        let response_handle = response_read.take() as usize;
        let reader_sender = sender.clone();
        thread::spawn(move || {
            let handle = response_handle as HANDLE;
            let _owner = match OwnedHandle::new(handle) {
                Ok(owner) => owner,
                Err(_) => return,
            };
            loop {
                let mut prefix = [0u8; 4];
                if !read_exact_handle(handle, &mut prefix) {
                    // EOF before a frame starts is the normal end of the
                    // stream (worker exit); the process watcher reports it.
                    return;
                }
                let length = u32::from_le_bytes(prefix) as usize;
                if length == 0 || length > MAX_MESSAGE_BYTES {
                    let _ = reader_sender.send(SessionEvent::ReaderViolation);
                    return;
                }
                let mut body = vec![0u8; length];
                if !read_exact_handle(handle, &mut body) {
                    // A truncated body after a valid prefix is a framing
                    // violation, not a clean end of stream.
                    let _ = reader_sender.send(SessionEvent::ReaderViolation);
                    return;
                }
                if reader_sender.send(SessionEvent::Message(body)).is_err() {
                    return;
                }
            }
        });
        // Process-death watcher (protocol §7): pipe EOF alone cannot signal a
        // dead worker when a descendant keeps the inherited response pipe
        // handle open, so the frame wait also observes the process handle.
        let watched_process = process.duplicated_process_handle()?;
        thread::spawn(move || {
            use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
            let handle = watched_process as HANDLE;
            unsafe {
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
            }
            let _ = sender.send(SessionEvent::ProcessExited);
        });

        Ok(RenderSession {
            process: Some(process),
            collected: None,
            transport,
            receiver,
            process_exit_observed: false,
            geometry,
            time_scale: request.time_scale,
            total_time: request.total_time,
            frame_deadline: request.frame_deadline,
            invalidation: None,
            last_output_generation: 0,
            frames_ok: 0,
            frames_errored: 0,
            opened: Instant::now(),
            plugin_sha256: request.plugin_sha256.to_ascii_lowercase(),
            _animation_sidecar: animation_sidecar,
        })
    }

    pub fn invalidation(&self) -> Option<&SessionInvalidation> {
        self.invalidation.as_ref()
    }

    fn collect_exit(&mut self, wait: Duration) {
        if self.collected.is_some() {
            return;
        }
        let Some(process) = self.process.take() else {
            return;
        };
        self.collected = Some(match process.finish(wait) {
            Ok(result) => CollectedExit {
                result: Some(result),
                error: None,
            },
            Err(error) => CollectedExit {
                result: None,
                error: Some(error.to_string()),
            },
        });
    }

    fn invalidate(
        &mut self,
        reason: &'static str,
        detail: String,
        terminate: bool,
        wait: Duration,
    ) -> io::Error {
        if terminate {
            if let Some(process) = self.process.as_ref() {
                let _ = process.terminate_job();
            }
        }
        self.collect_exit(wait);
        self.invalidation = Some(SessionInvalidation { reason, detail });
        let stored = self.invalidation.as_ref().expect("just stored");
        invalid(format!(
            "render session invalidated ({}): {}",
            stored.reason, stored.detail
        ))
    }

    pub fn render_frame(
        &mut self,
        frame_index: u32,
        current_time: i32,
        rgba: &[u8],
    ) -> io::Result<FrameOutcome> {
        if let Some(invalidation) = &self.invalidation {
            return Err(invalid(format!(
                "render session is invalidated ({}): {}",
                invalidation.reason, invalidation.detail
            )));
        }
        // Between frames, a queued process-death event fails the frame before
        // any slot write; a queued message with no frame in flight is a
        // protocol violation.
        loop {
            match self.receiver.try_recv() {
                Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                Ok(SessionEvent::ReaderViolation) => {
                    return Err(self.invalidate(
                        "response_framing_violation",
                        format!("the worker broke the response framing before {frame_index}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Ok(SessionEvent::Message(_)) => {
                    return Err(self.invalidate(
                        "unsolicited_response",
                        format!("a response arrived with no frame in flight before {frame_index}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Err(_) => break,
            }
        }
        if self.process_exit_observed {
            return Err(self.invalidate(
                "worker_exited",
                format!("the worker exited before frame {frame_index} was dispatched"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        if rgba.len() != self.geometry.input_slot_bytes() {
            return Err(invalid("input frame byte count does not match the session"));
        }
        if current_time < 0 || current_time > self.total_time {
            return Err(invalid("frame time is outside the session's total time"));
        }
        let expected_generation = frame_index
            .checked_add(1)
            .ok_or_else(|| invalid("frame index overflows the generation counter"))?;
        // frame_index is a serial: reusing or rewinding it would recompute a
        // generation the output slot may already hold, letting a stale
        // frame_done pass the generation checks with old pixels. Reject the
        // dispatch (caller error; the session itself stays usable).
        if expected_generation <= self.last_output_generation {
            return Err(invalid(format!(
                "frame index {frame_index} does not advance the last completed generation {}",
                self.last_output_generation
            )));
        }
        self.transport.write_input_slot(rgba);
        self.transport
            .write_header_u32(INPUT_GENERATION_OFFSET, expected_generation);
        let message = format!(
            "{{\"v\":1,\"type\":\"render_frame\",\"frame_index\":{frame_index},\
             \"current_time\":{{\"value\":{current_time},\"scale\":{}}}}}",
            self.time_scale
        );
        if !self.transport.send_message(&message) {
            return Err(self.invalidate(
                "request_pipe_closed",
                "the session request pipe rejected a frame message".into(),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let body = match self.await_frame_response() {
            FrameWait::Message(body) => body,
            FrameWait::Deadline => {
                return Err(self.invalidate(
                    "frame_deadline",
                    format!(
                        "frame {frame_index} exceeded the {}ms deadline",
                        self.frame_deadline.as_millis()
                    ),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::WorkerGone => {
                // Reached on process death (watcher) or on reader disconnect
                // (worker exit closing the pipe). Terminate the job so
                // descendants are reaped promptly; an already-exited worker
                // keeps its own exit code.
                return Err(self.invalidate(
                    "worker_exited",
                    format!("the worker was gone before frame {frame_index} completed"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::FramingViolation => {
                return Err(self.invalidate(
                    "response_framing_violation",
                    format!(
                        "the worker broke the response framing during frame {frame_index}"
                    ),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        let done: FrameDone = match serde_json::from_slice(&body) {
            Ok(done) => done,
            Err(error) => {
                return Err(self.invalidate(
                    "malformed_frame_done",
                    format!("frame {frame_index} response did not parse strictly: {error}"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        if done.v != PROTOCOL_VERSION || done.kind != "frame_done" || done.frame_index != frame_index
        {
            return Err(self.invalidate(
                "frame_done_mismatch",
                format!(
                    "frame {frame_index} response carried v={} type={} frame_index={}",
                    done.v, done.kind, done.frame_index
                ),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        match done.status.as_str() {
            "error" => {
                if done.output.is_some() || done.generation.is_some() || done.render_error == 0 {
                    return Err(self.invalidate(
                        "malformed_error_response",
                        format!("frame {frame_index} error response carried output fields"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                // The header invariants hold on every response, not only ok
                // ones: an error response must leave the broker-owned static
                // fields and the output generation untouched.
                if let Err(detail) = self.validate_static_header() {
                    return Err(self.invalidate(
                        "frame_invariant_failure",
                        format!("frame {frame_index} (error response): {detail}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                if self.transport.read_header_u32(OUTPUT_GENERATION_OFFSET)
                    != self.last_output_generation
                {
                    return Err(self.invalidate(
                        "frame_invariant_failure",
                        format!(
                            "frame {frame_index} error response advanced the output generation"
                        ),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                if is_fatal_session_error(done.render_error) {
                    // The worker reported a host-protection invariant failure
                    // and is running its own fail-closed teardown (setdown
                    // selectors, final report, exit 24). Wait boundedly for
                    // that exit instead of racing it with a job termination:
                    // collection terminates the job anyway if the worker does
                    // not leave within the timeout.
                    return Err(self.invalidate(
                        "worker_invariant_failure",
                        format!(
                            "frame {frame_index} reported the fatal session error {}",
                            done.render_error
                        ),
                        false,
                        CLOSE_COLLECT_TIMEOUT,
                    ));
                }
                // Frame-local diagnostic (protocol §4.3): the sequence state
                // is still host-owned, so the session continues; whether to
                // proceed is the caller's decision.
                self.frames_errored += 1;
                Ok(FrameOutcome {
                    frame_index,
                    status: FrameStatus::FrameError {
                        render_error: done.render_error,
                    },
                })
            }
            "ok" => {
                let (Some(output), Some(generation)) = (done.output, done.generation) else {
                    return Err(self.invalidate(
                        "malformed_ok_response",
                        format!("frame {frame_index} ok response missed output or generation"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                };
                if let Err(detail) = self.validate_ok_frame(expected_generation, &output, generation, done.render_error)
                {
                    return Err(self.invalidate(
                        "frame_invariant_failure",
                        format!("frame {frame_index}: {detail}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                let pixels = self.transport.read_output_slot(
                    self.geometry.output_slot_offset(),
                    self.geometry.output_slot_bytes(),
                );
                let checksum = format!("{:x}", Sha256::digest(&pixels));
                if !checksum.eq_ignore_ascii_case(&output.checksum) {
                    return Err(self.invalidate(
                        "output_checksum_mismatch",
                        format!(
                            "frame {frame_index} slot bytes hash {checksum} but the worker \
                             reported {}",
                            output.checksum
                        ),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                self.frames_ok += 1;
                self.last_output_generation = expected_generation;
                Ok(FrameOutcome {
                    frame_index,
                    status: FrameStatus::Rendered { pixels, checksum },
                })
            }
            other => Err(self.invalidate(
                "unknown_frame_status",
                format!("frame {frame_index} reported status {other:?}"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            )),
        }
    }

    /// Protocol §7's three-way frame wait: the response channel, the process
    /// handle (via the watcher event), and the deadline. After a process-death
    /// event, a short drain still honors a frame_done the worker flushed
    /// before dying rather than racing the watcher.
    fn await_frame_response(&mut self) -> FrameWait {
        let deadline = Instant::now() + self.frame_deadline;
        loop {
            let mut remaining = deadline.saturating_duration_since(Instant::now());
            if self.process_exit_observed {
                remaining = remaining.min(PROCESS_EXIT_DRAIN);
            }
            if remaining.is_zero() {
                return if self.process_exit_observed {
                    FrameWait::WorkerGone
                } else {
                    FrameWait::Deadline
                };
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(SessionEvent::Message(body)) => return FrameWait::Message(body),
                Ok(SessionEvent::ReaderViolation) => return FrameWait::FramingViolation,
                Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return if self.process_exit_observed {
                        FrameWait::WorkerGone
                    } else {
                        FrameWait::Deadline
                    };
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return FrameWait::WorkerGone,
            }
        }
    }

    fn validate_ok_frame(
        &self,
        expected_generation: u32,
        output: &FrameDoneOutput,
        generation: u32,
        render_error: i64,
    ) -> Result<(), String> {
        if render_error != 0 {
            return Err(format!("ok response carried render_error {render_error}"));
        }
        if !output.guards_intact {
            return Err("guard bytes were reported violated".into());
        }
        // v1 requires every frame at the session dimensions (protocol §3).
        if output.width != self.geometry.width || output.height != self.geometry.height {
            return Err(format!(
                "output geometry {}x{} differs from the session {}x{}",
                output.width, output.height, self.geometry.width, self.geometry.height
            ));
        }
        if output.rowbytes
            != u64::from(self.geometry.width) * self.geometry.pixel_format.bytes_per_pixel()
        {
            return Err(format!("output rowbytes {} is not packed", output.rowbytes));
        }
        if output.pixel_format != self.geometry.pixel_format.report_name() {
            return Err(format!(
                "output pixel format {} differs from the session {}",
                output.pixel_format,
                self.geometry.pixel_format.report_name()
            ));
        }
        if generation != expected_generation {
            return Err(format!(
                "response generation {generation} differs from expected {expected_generation}"
            ));
        }
        if self.transport.read_header_u32(OUTPUT_GENERATION_OFFSET) != expected_generation {
            return Err("header output generation is stale".into());
        }
        self.validate_static_header()?;
        if self.transport.read_header_u32(FRAME_WIDTH_OFFSET) != self.geometry.width
            || self.transport.read_header_u32(FRAME_HEIGHT_OFFSET) != self.geometry.height
        {
            return Err("frame dimension header does not match the session".into());
        }
        Ok(())
    }

    fn validate_static_header(&self) -> Result<(), String> {
        if self.transport.read_header_u32(MAGIC_OFFSET) != HEADER_MAGIC
            || self.transport.read_header_u32(VERSION_OFFSET) != PROTOCOL_VERSION
            || self.transport.read_header_u32(DEPTH_CODE_OFFSET)
                != depth_code(self.geometry.pixel_format)
            || self.transport.read_header_u32(MAX_WIDTH_OFFSET) != self.geometry.width
            || self.transport.read_header_u32(MAX_HEIGHT_OFFSET) != self.geometry.height
            || self.transport.read_header_u32(LAYER_SLOT_COUNT_OFFSET) != 0
        {
            return Err("static session header was mutated".into());
        }
        Ok(())
    }

    /// Ends the session: sends `close`, drops the request pipe so even an
    /// unresponsive worker sees EOF, collects the exit, and returns a summary
    /// including the worker's final stdout report when one was produced.
    pub fn close(mut self) -> Value {
        if self.invalidation.is_none() && self.process.is_some() {
            // The exit contract (protocol §7): a normal worker exit happens
            // only AFTER the broker's close handshake. A worker that is
            // already gone, an unsolicited queued response, or a failed close
            // send is an invalidation even when the exit code and the final
            // report look clean.
            loop {
                match self.receiver.try_recv() {
                    Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                    Ok(SessionEvent::ReaderViolation) => {
                        self.invalidation = Some(SessionInvalidation {
                            reason: "response_framing_violation",
                            detail: "the worker broke the response framing before close".into(),
                        });
                        break;
                    }
                    Ok(SessionEvent::Message(_)) => {
                        self.invalidation = Some(SessionInvalidation {
                            reason: "unsolicited_response",
                            detail: "a response arrived with no frame in flight before close"
                                .into(),
                        });
                        break;
                    }
                    Err(_) => break,
                }
            }
            // The watcher event is asynchronous, so also check the process
            // handle synchronously: a descendant can keep the request pipe
            // writable after the worker died, letting the close write succeed
            // against a worker that never received it.
            if self.process_exit_observed
                || self
                    .process
                    .as_ref()
                    .is_some_and(SecureSessionProcess::has_exited)
            {
                self.process_exit_observed = true;
            }
            if self.invalidation.is_none() && self.process_exit_observed {
                self.invalidation = Some(SessionInvalidation {
                    reason: "premature_exit",
                    detail: "the worker exited before the close handshake".into(),
                });
            }
            if self.invalidation.is_none()
                && !self.transport.send_message("{\"v\":1,\"type\":\"close\"}")
            {
                self.invalidation = Some(SessionInvalidation {
                    reason: "close_send_failed",
                    detail: "the close message could not be delivered".into(),
                });
            }
        }
        self.transport.close_request_pipe();
        self.collect_exit(CLOSE_COLLECT_TIMEOUT);
        let elapsed_ms = self.opened.elapsed().as_millis();
        let collected = self.collected.take();
        let (worker, final_report) = match &collected {
            Some(CollectedExit {
                result: Some(result),
                ..
            }) => {
                let report: Option<Value> = serde_json::from_str(result.stdout.trim()).ok();
                (
                    json!({
                        "classification": result.classification.as_str(),
                        "exit_code": result.exit_code,
                        "diagnostics": isolated_worker_diagnostics(result, elapsed_ms),
                    }),
                    report,
                )
            }
            Some(CollectedExit {
                error: Some(error), ..
            }) => (json!({ "collection_error": error }), None),
            _ => (json!({ "collection_error": "worker was never collected" }), None),
        };
        let session_clean = self.invalidation.is_none()
            && matches!(
                &collected,
                Some(CollectedExit { result: Some(result), .. })
                    if result.classification == crate::ExitClassification::Ok
            )
            && final_report.as_ref().is_some_and(final_report_clean);
        json!({
            "stage": "render_session_close",
            "plugin_sha256": self.plugin_sha256,
            "pixel_format": self.geometry.pixel_format.report_name(),
            "width": self.geometry.width,
            "height": self.geometry.height,
            "frames_ok": self.frames_ok,
            "frames_errored": self.frames_errored,
            "invalidated": self.invalidation.is_some(),
            "invalidated_reason": self.invalidation.as_ref().map(|invalidation| json!({
                "reason": invalidation.reason,
                "detail": invalidation.detail,
            })),
            "worker": worker,
            "final_report": final_report,
            "session_clean": session_clean,
        })
    }
}

/// A clean session close requires the final report to agree, not just the
/// exit code: the persistent sequence must have set up and torn down without
/// error, guards must be intact, and every ownership ledger must balance.
/// Missing keys fail closed.
fn final_report_clean(report: &Value) -> bool {
    report.get("status") == Some(&json!("render_completed"))
        && report.get("render_error") == Some(&json!(0))
        && report.get("global_setdown_error") == Some(&json!(0))
        && report.get("persistent_sequence_setup_error") == Some(&json!(0))
        && report.get("persistent_sequence_setdown_error") == Some(&json!(0))
        && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
        && report.get("suite_leases_balanced") == Some(&Value::Bool(true))
        && report.get("handle_lifetimes_balanced") == Some(&Value::Bool(true))
        && report.get("world_lifetimes_balanced") == Some(&Value::Bool(true))
        && report.get("param_checkouts_balanced") == Some(&Value::Bool(true))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VideoBatchRequest {
    schema_version: u32,
    plugin: String,
    input_frames: Vec<String>,
    output_directory: String,
    #[serde(default)]
    pixel_format: RenderPixelFormat,
    #[serde(default = "default_time_scale")]
    time_scale: u32,
    #[serde(default = "default_time_step")]
    time_step: i32,
    #[serde(default)]
    frame_deadline_ms: Option<u64>,
    /// Frame-local errors abort the batch by default (protocol §4.3); opting
    /// in records the error and keeps rendering the remaining frames.
    #[serde(default)]
    continue_on_frame_error: bool,
    /// Interactive parameter declarations, required to bind
    /// `parameter_animation` slots (issue #132).
    #[serde(default)]
    parameters: Vec<InteractiveParameter>,
    /// Timeline evaluated by the worker at each frame's current_time.
    #[serde(default)]
    parameter_animation: Vec<ParameterAnimation>,
}

fn default_time_scale() -> u32 {
    30
}

fn default_time_step() -> i32 {
    1
}

/// Batch video render CLI: renders a PNG frame sequence through one resident
/// render session and writes a JSON report. Returns whether the whole batch
/// passed. This is the default (crash-containment) tier: the plug-in hash is
/// recorded and binds the observation, and no receipt or allowlist applies.
pub fn run_video_batch(
    repository: &Path,
    request_path: &Path,
    output_path: &Path,
) -> io::Result<bool> {
    let metadata = fs::metadata(request_path)?;
    if metadata.len() > MAX_REQUEST_BYTES {
        return Err(invalid("batch request document exceeds 64 KiB"));
    }
    let request: VideoBatchRequest = serde_json::from_slice(&fs::read(request_path)?)
        .map_err(|error| invalid(format!("batch request is invalid: {error}")))?;
    if request.schema_version != 1 {
        return Err(invalid("batch request schema_version must be 1"));
    }
    if request.input_frames.is_empty() || request.input_frames.len() > MAX_BATCH_FRAMES {
        return Err(invalid("batch requires 1..=10000 input frames"));
    }
    let frame_count = request.input_frames.len();
    let total_time = i32::try_from(frame_count)
        .ok()
        .and_then(|count| count.checked_mul(request.time_step))
        .filter(|total| *total > 0)
        .ok_or_else(|| invalid("batch total time overflows"))?;
    let frame_deadline = Duration::from_millis(
        request
            .frame_deadline_ms
            .unwrap_or(INTERACTIVE_RENDER_TIMEOUT_MS)
            .clamp(1_000, 600_000),
    );

    let plugin_path = PathBuf::from(&request.plugin);
    let plugin_bytes = fs::read(&plugin_path)?;
    if plugin_bytes.is_empty() {
        return Err(invalid("plugin file is empty"));
    }
    let plugin_sha256 = format!("{:x}", Sha256::digest(&plugin_bytes));
    drop(plugin_bytes);

    let output_directory = PathBuf::from(&request.output_directory);
    fs::create_dir_all(&output_directory)?;

    let first = decode_bounded_image(Path::new(&request.input_frames[0]), "input")?;
    let (width, height) = (first.width(), first.height());
    drop(first);

    let mut session = RenderSession::open(SessionOpenRequest {
        repository,
        plugin_path: &plugin_path,
        plugin_sha256: &plugin_sha256,
        parameters: (!request.parameters.is_empty()).then_some(request.parameters.as_slice()),
        parameter_animation: (!request.parameter_animation.is_empty())
            .then_some(request.parameter_animation.as_slice()),
        dependencies: Vec::new(),
        width,
        height,
        pixel_format: request.pixel_format,
        time_step: request.time_step,
        total_time,
        time_scale: request.time_scale,
        frame_deadline,
    })?;

    let raw_extension = request.pixel_format.raw_extension();
    let mut frames = Vec::with_capacity(frame_count);
    let mut aborted = false;
    for (index, input_frame) in request.input_frames.iter().enumerate() {
        let frame_index = index as u32;
        let frame_time = frame_index as i32 * request.time_step;
        let frame_entry = (|| -> io::Result<Value> {
            let decoded = decode_bounded_image(Path::new(input_frame), "input")?;
            if decoded.width() != width || decoded.height() != height {
                return Err(invalid(format!(
                    "frame {frame_index} dimensions differ from the first frame"
                )));
            }
            let rgba = decoded.into_rgba8().into_raw();
            let outcome = session.render_frame(frame_index, frame_time, &rgba)?;
            match outcome.status {
                FrameStatus::Rendered { pixels, checksum } => {
                    let output_png = output_directory.join(format!("frame-{frame_index:06}.png"));
                    let preview = native_rgba_to_preview(&pixels, request.pixel_format)?;
                    let image = image::RgbaImage::from_raw(width, height, preview)
                        .ok_or_else(|| invalid("output dimensions are invalid"))?;
                    if output_png.exists() {
                        return Err(invalid("output frame already exists"));
                    }
                    image
                        .save_with_format(&output_png, image::ImageFormat::Png)
                        .map_err(|error| invalid(format!("output PNG save failed: {error}")))?;
                    if let Some(extension) = raw_extension {
                        let raw_path = output_png.with_extension(extension);
                        OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&raw_path)?
                            .write_all(&pixels)?;
                    }
                    Ok(json!({
                        "frame_index": frame_index,
                        "status": "ok",
                        "checksum": checksum,
                        "output_png": output_png.file_name().and_then(|name| name.to_str()),
                    }))
                }
                FrameStatus::FrameError { render_error } => Ok(json!({
                    "frame_index": frame_index,
                    "status": "error",
                    "render_error": render_error,
                })),
            }
        })();
        match frame_entry {
            Ok(entry) => {
                let frame_errored = entry["status"] == json!("error");
                frames.push(entry);
                if frame_errored && !request.continue_on_frame_error {
                    aborted = true;
                    break;
                }
            }
            Err(error) => {
                frames.push(json!({
                    "frame_index": frame_index,
                    "status": "failed",
                    "error": error.to_string(),
                }));
                aborted = true;
                break;
            }
        }
    }

    let close = session.close();
    let frames_ok = frames
        .iter()
        .filter(|frame| frame["status"] == json!("ok"))
        .count();
    let passed = !aborted
        && frames_ok == frame_count
        && close["session_clean"] == json!(true)
        && close["invalidated"] == json!(false);
    let report = json!({
        "schema_version": 1,
        "stage": "render_video_batch",
        "plugin_sha256": plugin_sha256,
        "render_path": "classic",
        "pixel_format": request.pixel_format.report_name(),
        "width": width,
        "height": height,
        "frame_count": frame_count,
        "frames_ok": frames_ok,
        "aborted": aborted,
        "frames": frames,
        "session": close,
        "passed": passed,
    });
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    output.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    Ok(passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_geometry_slot_layout_matches_the_protocol() {
        let geometry = SessionGeometry {
            width: 33,
            height: 17,
            pixel_format: RenderPixelFormat::Argb16,
        };
        assert_eq!(geometry.input_slot_bytes(), 33 * 17 * 4); // 2244
        assert_eq!(geometry.output_slot_bytes(), 33 * 17 * 8); // 4488
        // Slots are 4096-aligned after the one-page header (protocol §6).
        assert_eq!(geometry.output_slot_offset() % SLOT_ALIGNMENT, 0);
        assert_eq!(geometry.output_slot_offset(), 4096 + 4096);
        assert_eq!(geometry.section_bytes(), 4096 + 4096 + 8192);
    }

    #[test]
    fn frame_done_parsing_is_strict_about_unknown_fields() {
        let ok: Result<FrameDone, _> = serde_json::from_str(
            r#"{"v":1,"type":"frame_done","frame_index":0,"status":"ok",
                "output":{"width":1,"height":1,"rowbytes":4,"pixel_format":"argb8",
                          "checksum":"00","guards_intact":true},
                "render_error":0,"generation":1}"#,
        );
        assert!(ok.is_ok());
        let unknown: Result<FrameDone, _> = serde_json::from_str(
            r#"{"v":1,"type":"frame_done","frame_index":0,"status":"ok",
                "render_error":0,"generation":1,"surprise":true}"#,
        );
        assert!(unknown.is_err());
        let error_shape: FrameDone = serde_json::from_str(
            r#"{"v":1,"type":"frame_done","frame_index":3,"status":"error","render_error":-40}"#,
        )
        .unwrap();
        assert!(error_shape.output.is_none());
        assert!(error_shape.generation.is_none());
        assert_eq!(error_shape.render_error, -40);
    }

    #[test]
    fn final_report_clean_fails_closed_on_missing_or_dirty_fields() {
        let clean = serde_json::json!({
            "status": "render_completed",
            "render_error": 0,
            "global_setdown_error": 0,
            "persistent_sequence_setup_error": 0,
            "persistent_sequence_setdown_error": 0,
            "guard_bytes_intact": true,
            "suite_leases_balanced": true,
            "handle_lifetimes_balanced": true,
            "world_lifetimes_balanced": true,
            "param_checkouts_balanced": true,
        });
        assert!(final_report_clean(&clean));
        for (key, dirty) in [
            // A protocol-violation or invariant-failure session loop ends
            // render_failed with render_error -1 even when the ledgers
            // balance; both fields must gate the clean verdict.
            ("status", serde_json::json!("render_failed")),
            ("render_error", serde_json::json!(-1)),
            // The worker's own exit gate does not include GLOBAL_SETDOWN, so
            // a teardown failure can hide behind exit 0; the broker gate must
            // catch it.
            ("global_setdown_error", serde_json::json!(25)),
            ("persistent_sequence_setup_error", serde_json::json!(25)),
            ("persistent_sequence_setdown_error", serde_json::json!(-1)),
            ("guard_bytes_intact", serde_json::json!(false)),
            ("suite_leases_balanced", serde_json::json!(false)),
            ("handle_lifetimes_balanced", serde_json::json!(false)),
            ("world_lifetimes_balanced", serde_json::json!(false)),
            ("param_checkouts_balanced", serde_json::json!(false)),
        ] {
            let mut report = clean.clone();
            report[key] = dirty;
            assert!(!final_report_clean(&report), "{key} must fail closed");
            let mut missing = clean.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(!final_report_clean(&missing), "missing {key} must fail closed");
        }
        // A parseable but unrelated report (an older worker) is not clean.
        assert!(!final_report_clean(&serde_json::json!({"status": "ok"})));
    }

    #[test]
    fn open_rejects_timing_the_worker_could_never_render() {
        // Timing is validated before any file or transport work, so fake
        // paths never get touched when the timing is invalid.
        for (time_step, total_time, time_scale) in [
            (0, 300, 30),
            (1, 0, 30),
            (1, 300, 0),
            // The worker parses per-frame scales as signed 32-bit.
            (1, 300, i32::MAX as u32 + 1),
        ] {
            let result = RenderSession::open(SessionOpenRequest {
                repository: Path::new("missing-repository"),
                plugin_path: Path::new("missing-plugin.aex"),
                plugin_sha256: &"0".repeat(64),
                parameters: None,
                parameter_animation: None,
                dependencies: Vec::new(),
                width: 8,
                height: 4,
                pixel_format: RenderPixelFormat::Argb8,
                time_step,
                total_time,
                time_scale,
                frame_deadline: Duration::from_secs(1),
            });
            let Err(error) = result else {
                panic!("invalid timing must be rejected before launch");
            };
            assert_eq!(error.to_string(), "render session timing is invalid");
        }
    }

    #[test]
    fn fatal_session_error_codes_match_the_worker_contract() {
        // kSessionGenerationMismatch .. kSessionOutputValidationError.
        for code in [-41, -42, -43, -44, -45] {
            assert!(is_fatal_session_error(code), "{code} is session-fatal");
        }
        // Time-scale (-40) and time-range (-46) rejections are frame-local,
        // as are ordinary positive selector errors.
        for code in [-40, -46, 516, 25, -1] {
            assert!(!is_fatal_session_error(code), "{code} stays frame-local");
        }
    }

    #[test]
    fn depth_codes_and_commands_are_depth_explicit() {
        assert_eq!(depth_code(RenderPixelFormat::Argb8), 8);
        assert_eq!(depth_code(RenderPixelFormat::Argb16), 16);
        assert_eq!(depth_code(RenderPixelFormat::Argb32f), 32);
        assert_eq!(session_command(RenderPixelFormat::Argb8), "--render-session-v1");
        assert_eq!(
            session_command(RenderPixelFormat::Argb16),
            "--render-session16-v1"
        );
        assert_eq!(
            session_command(RenderPixelFormat::Argb32f),
            "--render-session32-v1"
        );
    }
}
