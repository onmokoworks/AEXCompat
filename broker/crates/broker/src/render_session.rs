//! Resident render session (issue #98 stage 1 PR-C,
//! docs/RENDER_SESSION_PROTOCOL_2026-07-19.md).
//!
//! The broker opens one sealed worker process per session, keeps the trust
//! artifacts (sealed tree, staged worker) alive for the
//! session's lifetime, and drives a frame loop over the inherited transport:
//! two anonymous pipes carrying length-prefixed JSON control messages and one
//! anonymous file mapping carrying copy-through pixel slots. The worker never
//! opens a path for session transport (issue #18 lesson).
//!
//! Safety boundaries stay per-frame: every `render_frame` arms a deadline and
//! a dead worker, a stale generation, a mutated header, a checksum mismatch,
//! or an out-of-bounds geometry invalidates the whole session fail-closed.

use crate::image_render::{
    GpuRuntimePolicyInput, INTERACTIVE_RENDER_TIMEOUT_MS, InteractiveParameter, MAX_DIMENSION,
    MAX_PIXELS, MAX_RGBA_TRANSPORT_BYTES, ParameterAnimation, RenderGpuBackend, RenderPixelFormat,
    RenderUiAction, decode_bounded_image, decode_sha256_hex, encode_default_interactive_payload,
    encode_interactive_payload, isolated_worker_diagnostics, native_rgba_to_preview,
    parameter_animation_sidecar_json, runtime_backend, validate_animation_bindings,
};
use crate::runtime_module_policy::{WorkerModuleValidation, authenticate_gpu_worker_report};
use crate::secure_image_dispatch::{
    ApprovedImageArtifact, GpuRuntimeAuthorization, SecureImageDispatch, WorkerKind,
    dispatch_secure_gpu_image_session, dispatch_secure_image_session,
};
use crate::secure_launch::{SecureLaunchResult, SecureSessionProcess};
use crate::windows_process::{SessionChildHandles, WorkerDesktopPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, SetHandleInformation,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    PAGE_READWRITE, UnmapViewOfFile,
};
use windows_sys::Win32::System::Pipes::CreatePipe;

// Protocol constants mirroring the worker (`worker_render_session.hpp`).
const HEADER_BYTES: usize = 4096;
const SLOT_ALIGNMENT: usize = 4096;
const HEADER_MAGIC: u32 = 0x5358_4541; // "AEXS" little-endian
/// Base version of the control-message envelope (`frame_done`/`render_frame`
/// `v`). Kept at 1; the per-frame-attributes messages use v2
/// (`kRenderFrameParametersVersion`).
const PROTOCOL_VERSION: u32 = 1;
/// Version stamped in the shared-section header (`VERSION_OFFSET`) and validated
/// by both broker and worker. Distinct from the message version so it can track
/// the shared-memory LAYOUT: bumped to 2 when layer slots became per-layer sized
/// (#264), then to 3 when layer pixels left the section entirely and now travel
/// as inherited per-layer file HANDLEs (#268), so the section is header + input +
/// output only. A stale broker/worker pair whose layouts disagree fails closed on
/// the header check (in both directions) instead of reading the wrong bytes.
const SESSION_HEADER_VERSION: u32 = 3;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// A discovery session's `inspect_done` carries the full parameter report, so
/// its reader allows 4 MiB frames (design §4); every other session flavor
/// stays under the 64 KiB cap.
const MAX_DISCOVERY_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// Fail-closed cap on the whole section for pathological configurations
/// (protocol §6); ordinary full-HD sessions stay far below it.
const SECTION_HARD_CAP_BYTES: u64 = 1 << 30;
pub const EXIT_PROTOCOL_VIOLATION: u32 = 23;
pub const EXIT_INVARIANT_FAILURE: u32 = 24;

/// The worker's reserved session error codes that accompany a host-protection
/// invariant failure (`l2_main.cpp` `kSessionGenerationMismatch` ..
/// `kSessionOutputValidationError`) or a failed deferred SEQUENCE_SETUP
/// (`kSessionSequenceSetupFailed`, -47: the session can never render). The
/// worker exits fail-closed right after sending such a response, so the
/// broker must invalidate the session rather than surface them as reusable
/// frame-local diagnostics. Time-scale (-40), time-range (-46) and the
/// audio-passthrough ui_action refusal (-48, issue #1048) stay frame-local by
/// the worker's contract.
fn is_fatal_session_error(render_error: i64) -> bool {
    matches!(render_error, -47 | -45..=-41)
}

/// The worker already filters `PF_OutData::return_msg`, but the worker is the
/// side the plug-in runs in, so the text is re-validated where it crosses into
/// the broker (issue #707): bounded by the SDK buffer, printable ASCII, no path
/// separator, and a selector name that looks like one the host emits. A
/// plug-in must not be able to put a private path into a report through a field
/// that exists only to be read by a human.
///
/// A message that fails this is dropped, not escalated. Unlike
/// `missing_dependency`, nothing decides anything on this text - discarding it
/// costs a sentence of diagnosis, while invalidating the session over it would
/// turn a plug-in's stray backslash into a render failure.
fn admissible_return_message(message: &FrameReturnMessage) -> bool {
    // PF_MAX_EFFECT_MSG_LEN + 1 is 256, so the text cannot exceed 255 bytes.
    !message.text.is_empty()
        && message.text.len() <= 255
        && message
            .text
            .bytes()
            .all(|byte| (0x20..0x7F).contains(&byte) && byte != b'\\' && byte != b'/')
        && !message.selector.is_empty()
        && message.selector.len() <= 64
        && message
            .selector
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_dependency_basename(name: &str) -> bool {
    name.len() >= 5
        && name.len() <= 260
        && name.to_ascii_lowercase().ends_with(".dll")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Resize-output bounds mirroring the worker's `validate_output_extent`
/// (render_subsystem.cpp): each dimension <= 4096 and <= 16,777,216 pixels
/// total. The broker re-caps a resize_needed request so a misbehaving worker
/// cannot force an unbounded re-open (#261).
const MAX_RESIZE_DIMENSION: u32 = 4096;
const MAX_RESIZE_PIXELS: u64 = 16_777_216;

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

/// How long a close-handshake write failure waits for the process object to
/// catch up with the pipe. Windows closes a dying process's handles before it
/// signals the process itself, so a worker that exits on its own leaves a
/// window where the write already fails while `has_exited` still reports it
/// running. Only a worker that is genuinely alive and unreachable pays this.
const EXIT_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

/// Waits up to [`EXIT_SETTLE_TIMEOUT`] for `process` to report its exit.
pub(crate) fn settled_as_exited(process: Option<&SecureSessionProcess>) -> bool {
    let Some(process) = process else {
        return false;
    };
    let deadline = Instant::now() + EXIT_SETTLE_TIMEOUT;
    loop {
        if process.has_exited() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

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

/// Maps the session flavor to the worker command word (protocol §3, v1.1).
/// SmartFX ARGB32f carries the GPU backend in the command word, mirroring the
/// deleted one-shot `--smart-image32[-cpu|-opencl|-directx]` family; every other
/// depth is CPU-only, and classic sessions reject explicit GPU backends.
fn session_command(
    pixel_format: RenderPixelFormat,
    smart: bool,
    gpu_backend: RenderGpuBackend,
) -> io::Result<&'static str> {
    if !smart {
        return match gpu_backend {
            RenderGpuBackend::Auto | RenderGpuBackend::Cpu => Ok(match pixel_format {
                RenderPixelFormat::Argb8 => "--render-session-v1",
                RenderPixelFormat::Argb16 => "--render-session16-v1",
                RenderPixelFormat::Argb32f => "--render-session32-v1",
            }),
            _ => Err(invalid("GPU backends require a SmartFX ARGB32f session")),
        };
    }
    match (pixel_format, gpu_backend) {
        (RenderPixelFormat::Argb32f, RenderGpuBackend::Auto | RenderGpuBackend::Cuda) => {
            Ok("--smart-session32-v1")
        }
        (RenderPixelFormat::Argb32f, RenderGpuBackend::OpenCl) => Ok("--smart-session32-opencl-v1"),
        (RenderPixelFormat::Argb32f, RenderGpuBackend::DirectX) => {
            Ok("--smart-session32-directx-v1")
        }
        (RenderPixelFormat::Argb32f, RenderGpuBackend::Cpu) => Ok("--smart-session32-cpu-v1"),
        (RenderPixelFormat::Argb8, RenderGpuBackend::Auto | RenderGpuBackend::Cpu) => {
            Ok("--smart-session-v1")
        }
        (RenderPixelFormat::Argb16, RenderGpuBackend::Auto | RenderGpuBackend::Cpu) => {
            Ok("--smart-session16-v1")
        }
        _ => Err(invalid(
            "an explicit GPU backend requires a SmartFX ARGB32f session",
        )),
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
    let broker_end = if broker_end_is_read {
        read.raw()
    } else {
        write.raw()
    };
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

    /// Replaces the mapped section with a larger one (in-session grow, protocol
    /// §3), keeping the request pipe. Unmaps the current view and drops the
    /// current section handle (CloseHandle), then adopts the grown section.
    fn adopt_section(&mut self, section: OwnedHandle, view: *mut u8, section_bytes: usize) {
        if !self.view.is_null() {
            let address = MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view as *mut _,
            };
            unsafe {
                UnmapViewOfFile(address);
            }
        }
        // Assigning drops the previous OwnedHandle, closing the old section.
        self.section = section;
        self.view = view;
        self.section_bytes = section_bytes;
    }
}

/// Writes a header u32 into a raw mapped view (used to initialise a grown
/// section before it is adopted, when no `SessionTransport` owns it yet).
fn write_header_u32_raw(view: *mut u8, offset: usize, value: u32) {
    debug_assert!(offset + 4 <= HEADER_BYTES);
    unsafe {
        std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), view.add(offset), 4);
    }
}

/// Initialises the broker-owned static header (and generation/frame fields) of a
/// freshly mapped section, exactly as `open` does, so the worker's
/// `static_header_matches` passes after it adopts the grown section. The worker
/// overwrites the output generation and frame dimensions when it transfers the
/// pending frame; `generation` seeds them consistently in the meantime.
fn init_section_header(view: *mut u8, geometry: &SessionGeometry, generation: u32) {
    write_header_u32_raw(view, MAGIC_OFFSET, HEADER_MAGIC);
    write_header_u32_raw(view, VERSION_OFFSET, SESSION_HEADER_VERSION);
    write_header_u32_raw(view, DEPTH_CODE_OFFSET, depth_code(geometry.pixel_format));
    write_header_u32_raw(view, MAX_WIDTH_OFFSET, geometry.width);
    write_header_u32_raw(view, MAX_HEIGHT_OFFSET, geometry.height);
    write_header_u32_raw(view, LAYER_SLOT_COUNT_OFFSET, geometry.layer_slot_count);
    write_header_u32_raw(view, INPUT_GENERATION_OFFSET, generation);
    write_header_u32_raw(view, OUTPUT_GENERATION_OFFSET, generation);
    write_header_u32_raw(view, FRAME_WIDTH_OFFSET, geometry.width);
    write_header_u32_raw(view, FRAME_HEIGHT_OFFSET, geometry.height);
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
    /// Output slot capacity, decoupled from the render dimensions (#261): the
    /// output slot holds up to `output_capacity_width * output_capacity_height`
    /// pixels so an expand-output effect can render larger than the input
    /// without changing the render geometry (in_data extent / full resolution,
    /// which stay `width`/`height`). Starts equal to `width`/`height` and is
    /// raised in place by an in-session grow (#262) when an expand overruns the
    /// launch slot.
    output_capacity_width: u32,
    output_capacity_height: u32,
    pixel_format: RenderPixelFormat,
    /// Number of layers, retained only for the header `LAYER_SLOT_COUNT_OFFSET`
    /// field so the worker can cross-check it against the `session-layers`
    /// trailer count. Layer PIXELS no longer occupy the section (#268): they
    /// travel as inherited per-layer file HANDLEs, so this count does not size
    /// the section. Zero when there are no layers.
    layer_slot_count: u32,
}

impl SessionGeometry {
    fn input_slot_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
    fn output_slot_bytes(&self) -> usize {
        self.output_capacity_width as usize
            * self.output_capacity_height as usize
            * self.pixel_format.bytes_per_pixel() as usize
    }
    fn output_slot_offset(&self) -> usize {
        HEADER_BYTES + align_slot(self.input_slot_bytes())
    }
    /// The section holds only header + input + output (#268); layer pixels
    /// travel as inherited file HANDLEs, not section slots.
    fn section_bytes(&self) -> usize {
        self.output_slot_offset() + align_slot(self.output_slot_bytes())
    }
}

/// The section holds only header + input + a worst-case-expanded output slot
/// (#268): layer pixels left the section and travel as inherited per-layer file
/// HANDLEs, so the aggregate section cap is no longer a function of layer count
/// or size. For any render whose primary dimensions pass the shared image-buffer
/// bounds (`MAX_DIMENSION`/`MAX_PIXELS`), input (<= 64 MiB) plus a
/// `MAX_RESIZE_DIMENSION`-per-axis 32-bit-float output (<= 256 MiB) is far under
/// `SECTION_HARD_CAP_BYTES`, so the resident classic session can structurally
/// represent every layered config the one-shot path can (#98 W4, #264). `open`
/// still fail-closes on `section_bytes() > SECTION_HARD_CAP_BYTES` as pure
/// defense in depth; there is no longer a section-fit eligibility carve-out that
/// keeps a large-layer render on the one-shot path.
pub struct SessionOpenRequest<'a> {
    pub repository: &'a Path,
    pub plugin_path: &'a Path,
    pub plugin_sha256: &'a str,
    pub parameters: Option<&'a [InteractiveParameter]>,
    /// A pre-encoded worker payload used verbatim instead of encoding
    /// `parameters`. The fixture-manifest route (`render_scattermap_fixture`)
    /// builds its payload from a descriptor profile via `encode_worker_payload`, which the
    /// `InteractiveParameter` list cannot represent; before #365 that route was
    /// the reason a one-shot argv transport had to exist at all. Both encoders
    /// emit the same `v2|`/`v3|` grammar the worker decodes, so the session
    /// carries either one in the same argv slot. `None` encodes `parameters`.
    pub payload_override: Option<&'a str>,
    /// Parameter animation timeline evaluated by the worker at each frame's
    /// current_time (issue #132). Bindings are validated against `parameters`
    /// before launch, exactly like the one-shot entry.
    pub parameter_animation: Option<&'a [ParameterAnimation]>,
    /// External aux channel manifest (`--aux-manifest-v1`), strictly parsed
    /// by the worker; must be an absolute path to an existing file.
    pub aux_manifest: Option<&'a Path>,
    /// World snapshot dump directory (`--dump-worlds-v1`); must be an
    /// absolute path to an existing directory.
    pub world_dump_dir: Option<&'a Path>,
    /// Enables the worker's per-output checksum detail records
    /// (`--output-checksum-detail-v1`).
    pub output_checksum_detail: bool,
    /// Static mask context trailer (`v2|`), already encoded by
    /// `encode_mask_context`; carried in the session launch argv (issue #98
    /// W1-3b). Precedes the spatial trailer in the one-shot positional order.
    pub mask_trailer: Option<String>,
    /// Static spatial context trailer (`spatial:v*`), already encoded by
    /// `encode_spatial_context`; carried in the session launch argv so the
    /// hoisted SEQUENCE_SETUP and every frame observe it (issue #98 W1-3).
    pub spatial_trailer: Option<String>,
    /// Static render-environment trailer (`render:v1|`), already encoded by
    /// `encode_render_environment`.
    pub render_environment_trailer: Option<String>,
    /// Static audio-source trailer (`session-audio:v1|<samples>|<rate>|<path>`),
    /// carrying the same span the one-shot passes as three bare argv slots under
    /// deleted `--render-image-audio` (issue #339). The plug-in sees one source for the
    /// whole session, so it rides the launch argv rather than the frame message.
    /// Rides at the tail of the *positional* section, behind the other optional
    /// trailers, so the worker peels it first of those. The auxiliary option
    /// pairs are appended after it and are stripped before any of this.
    pub audio_trailer: Option<String>,
    /// Alpha-as-coverage parameter slots (`--alpha-as-coverage-v1`), issue #98
    /// W1-4c. The worker publishes the alpha-coverage provider once at launch
    /// (a global the classic render runtime reads on every frame), matching the
    /// session's set-once lifetime, so only the slot list travels. Empty means
    /// the option is not emitted.
    pub alpha_as_coverage_params: &'a [u32],
    /// Conformance render-settings trailer (`--conformance-render-settings-v1`),
    /// already validated by the wrapper. The broker pre-transforms the input for
    /// the requested alpha mode on both routes; forwarding the trailer makes the
    /// session worker report the same `render_settings` block the one-shot path
    /// reports (it only feeds the worker's report, not the render). `None` leaves
    /// the option unset (the worker reports the legacy null settings) (#275).
    pub conformance_render_settings: Option<&'a str>,
    /// Secondary layers, static for the whole session (issue #98 W1-4). The
    /// pixels travel as inherited per-layer file HANDLEs, not section slots
    /// (#268): open streams each layer to its own file under
    /// target/image-transport, passes the read handle through the inherited
    /// handle list, and rides the slot/geometry plus the handle value on the
    /// `session-layers:v2|` launch trailer.
    pub layers: &'a [SessionLayer],
    pub dependencies: Vec<ApprovedImageArtifact>,
    /// Discovery-confirmed AEGP providers initialized in this worker before
    /// the PF module. Their broker-owned manifest stays alive with the
    /// resident session and declares the exact suites each provider exposed.
    pub companions: Vec<crate::companion_manifest::ApprovedCompanion>,
    /// In-place load mode (issue #751): non-empty opens the session on the
    /// plug-in's real path with these directories admitted into the worker's
    /// DLL search set, instead of staging a sealed tree. Mutually exclusive
    /// with `dependencies`. GPU sessions carry their AEXRMA1 authorization
    /// through the broker-owned image transport (#815). Cluster sessions
    /// combine with it since step 3 (#812): they ride cluster-manifest-v2.
    pub dependency_search_dirs: Vec<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: RenderPixelFormat,
    pub time_step: i32,
    pub total_time: i32,
    pub time_scale: u32,
    /// Per-frame watchdog deadline; the job is terminated when a frame's
    /// response does not arrive in time (protocol §7).
    pub frame_deadline: Duration,
    /// Selects the SmartFX resident session (protocol v1.1): the smart worker
    /// runs PreRender→SmartRender per frame under the hoisted sequence.
    pub smart: bool,
    /// GPU backend for smart ARGB32f sessions, carried in the command word
    /// like the one-shot smart dispatch. No runtime policy receipt is required
    /// to select GPU negotiation; all routes use the ordinary execution floor.
    pub gpu_backend: RenderGpuBackend,
    /// Optional legacy session-bound runtime module policy inputs. Existing
    /// explicit-policy callers retain their evidence path. A policy is inert
    /// for classic/CPU sessions that never attempt GPU.
    pub gpu_runtime_policy: Option<GpuRuntimePolicyInput<'a>>,
    /// Per-launch environment inputs (issue #910): extra child environment
    /// variables and the opt-in minidump directory, carried explicitly so a
    /// caller never has to set them on the broker process (which every
    /// concurrent session would then see).
    pub launch_environment: crate::secure_launch::LaunchEnvironment,
}

/// Stable identity of one effect registered by a PluginData bundle (#1260).
/// Both fields are required: the index makes selection unambiguous within the
/// bounded registration order, while the exact match-name bytes prevent a
/// changed/reordered bundle from silently dispatching another effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginDataEffectSelector {
    pub index: u32,
    pub match_name_hex: String,
}

impl PluginDataEffectSelector {
    pub(crate) fn encoded(&self) -> io::Result<String> {
        if self.index >= 64
            || self.match_name_hex.is_empty()
            || self.match_name_hex.len() > 512
            || !self.match_name_hex.len().is_multiple_of(2)
            || !self
                .match_name_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid("PluginData effect selector is invalid"));
        }
        for pair in self.match_name_hex.as_bytes().chunks_exact(2) {
            let value = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
            if value < 0x20 || value == 0x7f {
                return Err(invalid("PluginData effect selector contains unsafe bytes"));
            }
        }
        Ok(format!("v1|{}|{}", self.index, self.match_name_hex))
    }
}

fn append_plugin_data_selector_args(
    args: &mut Vec<String>,
    selector: Option<&PluginDataEffectSelector>,
) -> io::Result<()> {
    if let Some(selector) = selector {
        args.extend(["--plugin-data-selector-v1".to_owned(), selector.encoded()?]);
    }
    Ok(())
}

/// A secondary layer whose RGBA8 pixels travel as an inherited per-layer file
/// HANDLE for the whole session (#268), read once by the worker at open.
/// Width/height are the layer's own geometry, independent of the primary input.
/// A timed layer (issue #98 W1-4b) additionally carries the frame time at which
/// the worker admits it; the worker selects the matching timed entry per frame
/// with the same rational-time test the one-shot path uses.
#[derive(Clone)]
pub struct SessionLayer {
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// `Some((time, time_scale))` marks this a timed layer; `None` is a static
    /// secondary that renders on every frame.
    pub timed: Option<(i32, u32)>,
    /// The caller intends to replace these pixels between frames through
    /// [`RenderSession::update_dynamic_layer`] (issue #674: AviUtl2's virtual
    /// buffer driving an animated displacement map). The worker then keeps the
    /// layer's file handle open and re-reads it before every frame instead of
    /// consuming it at open. Geometry stays fixed at open either way, so only
    /// the bytes may change. Not combinable with `timed`, which already means
    /// "this layer belongs to one point in time".
    pub dynamic: bool,
}

#[derive(Debug)]
pub enum FrameStatus {
    /// The frame rendered and every per-frame invariant held. `pixels` are
    /// the validated native RGBA bytes copied out of the output slot.
    Rendered {
        pixels: Vec<u8>,
        /// The frame's actual rendered dimensions. Equal to the session
        /// dimensions for a fixed-size effect, smaller for a shrink-output
        /// effect (#261); `pixels` is packed at exactly `width*height*bpp`.
        width: u32,
        height: u32,
        /// The frame's top-left relative to the layer origin. Negative when an
        /// effect grew its output past the layer (#914), and positive when a
        /// classic effect cropped: both render paths fill it now, the smart one
        /// from `result_rect`'s top-left and the classic one by negating
        /// `PF_OutData::origin` (#984). Zero when nothing resized, which is
        /// where the output already starts.
        origin_x: i32,
        origin_y: i32,
    },
    /// A frame-local compatibility diagnostic (selector error, time scale
    /// mismatch). The session stays usable; continuing is the caller's call.
    FrameError {
        render_error: i64,
        missing_dependency: Option<String>,
        /// What the plug-in itself wrote into `PF_OutData::return_msg` while
        /// failing. The SDK's own suite helper writes "Couldn't load suite."
        /// there, and plug-ins write their own reason, so this is often the
        /// whole diagnosis (issue #707). Absent when the plug-in said nothing.
        return_message: Option<FrameReturnMessage>,
    },
    /// The Smart selector returned success, but the guarded output retained
    /// its initialization sentinel. This typed host observation is not a
    /// plug-in-returned numeric -6.
    SmartOutputUntouched,
    // An expand-output effect that overruns the launch slot no longer surfaces to
    // the caller: `render_frame` grows the shared section in place and waits for
    // the worker's follow-up ok (protocol §3, issue #262), so it always resolves
    // to `Rendered` at the expanded dimensions. The re-open path (a distinct
    // `ResizeNeeded` outcome) is gone, and with it the SEQUENCE/FRAME setup and
    // setdown replay it caused.
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
    /// How many bytes the worker packed into the slot. The broker derives the
    /// same extent from `width`/`height` and refuses a disagreement, which is
    /// the layout cross-check the per-frame SHA-256 used to carry (#690).
    packed_bytes: u64,
    guards_intact: bool,
    /// Where the frame sits relative to the layer origin. An effect that grows
    /// its output starts these pixels above and left of the layer's (0,0), so
    /// this is negative; a classic effect that crops starts them inside it, so
    /// this is positive. A caller placing the frame back into a fixed-size
    /// image needs it to know which part covers the layer (#914). SmartFX fills
    /// it from `result_rect`'s top-left; classic negates `PF_OutData::origin`
    /// on an accepted resize (#984). Absent (0) when nothing resized and on any
    /// worker that predates the field, which is where the output already
    /// starts.
    #[serde(default)]
    origin_x: i32,
    #[serde(default)]
    origin_y: i32,
    /// A SmartFX frame whose PreRender returned a legally empty result_rect
    /// (#278): width/height are 0 and there are no output pixels. Absent (false)
    /// for every normal frame, where a zero dimension stays an invariant
    /// failure. Only the worker's smart session frame loop sets it.
    #[serde(default)]
    empty_result: bool,
}

/// A selector's own account of why it failed, as left in
/// `PF_OutData::return_msg` (issue #707). The worker only reports it for a
/// selector that also returned an error, and only when it is printable ASCII.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrameReturnMessage {
    /// The selector that wrote it; the buffer is reused across selectors.
    pub selector: String,
    pub text: String,
    pub error: i64,
    /// The plug-in raised `PF_OutFlag_DISPLAY_ERROR_MESSAGE` alongside it, i.e.
    /// it meant this for the user rather than for a log.
    pub display_requested: bool,
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
    smart_output_untouched: bool,
    #[serde(default)]
    missing_dependency: Option<String>,
    #[serde(default)]
    return_message: Option<FrameReturnMessage>,
    #[serde(default)]
    generation: Option<u32>,
    /// Present only on a "resize_needed" status (#262): the dimensions the
    /// in-place grown output slot must accommodate.
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
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
    _render_service: Option<crate::render_service::RenderServiceLease>,
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
    smart_output_untouched_frames: u32,
    parameter_update_frames: u32,
    opened: Instant,
    plugin_sha256: String,
    /// SmartFX session (protocol v1.1); selects the smart worker's final
    /// report contract when validating a clean close.
    smart: bool,
    /// Cluster session state (issue #405): present when the session was
    /// opened with `open_cluster` and may swap plugins within the launch
    /// manifest.
    cluster: Option<ClusterSessionState>,
    /// Keeps the in-place cluster manifest transport (issue #751) alive for
    /// the whole session; the worker read it at launch, and the drop removes
    /// the document from target/image-transport.
    _in_place_transport: Option<crate::cluster_manifest::ClusterManifestTransport>,
    /// Keeps `--companion-manifest-v1` alive for the resident worker lifetime.
    _companion_transport: Option<crate::companion_manifest::CompanionManifestTransport>,
    /// Keeps the AEXRMA1 document alive for the whole GPU session. Staged
    /// launches copy it into the sealed tree; in-place launches read this
    /// broker-owned absolute transport path before loading plug-in code.
    _runtime_authorization:
        Option<crate::runtime_module_authorization::RuntimeAuthorizationTransport>,
    /// Keeps the animation sidecar alive for the whole session; the worker
    /// reads it once at launch, but leaving transport files behind on drop
    /// would leak into target/image-transport.
    _animation_sidecar: Option<AnimationSidecar>,
    /// Keeps the per-layer transport files (#268) alive for the whole session
    /// and removes them on drop; the worker reads each layer once at open via
    /// its inherited handle.
    _layer_sidecars: LayerSidecars,
    /// Writable handles for the layers the caller declared `dynamic` (issue
    /// #674), keyed by slot with the byte length fixed at open. The worker
    /// holds its own read handle to the same file and re-reads it before each
    /// frame, so replacing the bytes here is what animates the map.
    dynamic_layers: Vec<DynamicLayer>,
}

/// The launch plugin set of a cluster render session (issue #405, design
/// §2): the whole ordered cluster, staged and sealed once, so a
/// `swap_plugin` message only ever selects an index into this
/// launch-authenticated list. `plugins[0]` is the launch plugin and must be
/// the same artifact the base `SessionOpenRequest` names. The shared
/// dependency closure travels in the base request's `dependencies` field;
/// the launch manifest declares both together.
pub struct ClusterRenderPlugins {
    /// The ordered cluster including `plugins[0]`; non-empty, at most 256
    /// (enforced by the manifest validation at launch).
    pub plugins: Vec<ApprovedImageArtifact>,
    /// Parallel to `plugins`: the payload applied when swapping to that
    /// plugin. The entry for `plugins[0]` is ignored — the launch argv
    /// payload wins (design §2.2).
    pub swap_payloads: Vec<Option<String>>,
    /// The declared module bound the session's module audit is validated
    /// against at close (design §5).
    pub module_bound: u32,
}

struct ClusterSessionState {
    plugin_count: u32,
    current_plugin_index: u32,
}

/// The outcome of a `swap_plugin` exchange (design §4.1).
#[derive(Debug)]
pub enum SwapOutcome {
    /// The worker unloaded the previous plugin, loaded the requested one,
    /// and ran GLOBAL_SETUP cleanly; the requested plugin is now current.
    Swapped,
    /// GLOBAL_SETUP returned non-zero for the new plugin: a plugin-local
    /// error, so the session stays usable and whether to continue is the
    /// caller's decision (design §4.1). The requested plugin is loaded and
    /// current; the next frame follows the existing deferred-setup contract.
    PluginError { global_setup_error: i64 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwapDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    plugin_index: u32,
    status: String,
    #[serde(default)]
    global_setup_error: Option<i64>,
}

struct AnimationSidecar(PathBuf);

impl Drop for AnimationSidecar {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Keeps the per-layer transport files (#268) for the whole session and deletes
/// them on drop. The worker reads each layer once at open through its inherited
/// read handle and closes that handle immediately, and the broker drops its own
/// read handles at the end of `open`, so by drop time no handle references these
/// files and the removals succeed. Owning the paths (not the handles) means an
/// early return from `open` still cleans up the files it already wrote.
struct LayerSidecars(Vec<PathBuf>);

/// One layer the caller may rewrite between frames (issue #674).
struct DynamicLayer {
    slot: u32,
    /// Byte length fixed at open: the worker reads exactly this much, and its
    /// PF world was built for these dimensions, so a differently sized update
    /// is rejected rather than resized.
    bytes: usize,
    file: std::fs::File,
}

impl Drop for LayerSidecars {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

impl RenderSession {
    /// Opens a resident render session. The output slot starts sized to the
    /// render dimensions; an expand-output effect that overruns it grows the
    /// slot in place mid-session (protocol §3, issue #262), so there is no
    /// launch-time output-capacity parameter.
    pub fn open(request: SessionOpenRequest<'_>) -> io::Result<RenderSession> {
        Self::open_with_desktop_policy(request, WorkerDesktopPolicy::Dedicated, None, None)
    }

    pub fn open_plugin_data_effect(
        request: SessionOpenRequest<'_>,
        selector: &PluginDataEffectSelector,
    ) -> io::Result<RenderSession> {
        Self::open_with_desktop_policy(
            request,
            WorkerDesktopPolicy::Dedicated,
            None,
            Some(selector),
        )
    }

    /// Opens a session for an explicitly interactive GUI harness. This is
    /// intentionally opt-in; normal discovery/render sessions use a private
    /// desktop so plugin UI cannot interrupt the user's desktop.
    pub(crate) fn open_on_current_desktop(
        request: SessionOpenRequest<'_>,
    ) -> io::Result<RenderSession> {
        Self::open_with_desktop_policy(request, WorkerDesktopPolicy::Current, None, None)
    }

    fn open_with_desktop_policy(
        request: SessionOpenRequest<'_>,
        desktop_policy: WorkerDesktopPolicy,
        cluster: Option<ClusterRenderPlugins>,
        plugin_data_selector: Option<&PluginDataEffectSelector>,
    ) -> io::Result<RenderSession> {
        if request.time_step <= 0
            // A zero-duration render (total_time == 0) is valid: the shared
            // RenderTiming::is_valid admits it at current_time == 0, and the
            // one-shot worker renders the single t=0 frame, so the session must
            // too (#272). Only a negative total_time is rejected; per-frame the
            // worker still rejects current_time > total_time, so total_time == 0
            // admits exactly the current_time == 0 frame.
            || request.total_time < 0
            || request.time_scale == 0
            // The worker parses the per-frame current_time.scale as signed
            // 32-bit, so a larger launch time_scale could never render a
            // frame; reject it here instead of failing at the first frame.
            || request.time_scale > i32::MAX as u32
            || request.frame_deadline.is_zero()
        {
            return Err(invalid("render session timing is invalid"));
        }
        // Cluster validation fails fast at open, before any transport work:
        // plugins[0] must be the same artifact the base request names, since
        // the positional argv contract, the launch payload, and the manifest
        // entry all refer to it (design §2.2). The manifest's own structural
        // bounds are enforced by the dispatch when it builds the document.
        if let Some(cluster) = &cluster {
            if plugin_data_selector.is_some() {
                return Err(invalid(
                    "PluginData effect selection is not supported in a cluster session",
                ));
            }
            let launch_sha256 = decode_sha256_hex(request.plugin_sha256)?;
            if cluster.plugins.first().map(|plugin| plugin.expected_sha256) != Some(launch_sha256) {
                return Err(invalid("cluster plugins[0] must match the launch plugin"));
            }
            if cluster.swap_payloads.len() != cluster.plugins.len() {
                return Err(invalid(
                    "cluster swap payloads must parallel the plugin list",
                ));
            }
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
        if request.layers.len() > 64 {
            return Err(invalid("render session layer count exceeds 64"));
        }
        // Validate each layer. Layer pixels travel as inherited per-layer file
        // HANDLEs (#268), not section slots, so a secondary or timed layer of any
        // representable size renders on the session (no aggregate section cap on
        // layers). Slot dedup mirrors the one-shot layered_image_mode parser
        // exactly (issue #98 W1-4b) so a directly built request accepts and
        // rejects the same sets the one-shot path does: a slot rejects only a
        // second static entry or a timed entry at a rational time already
        // present. A static plus timed entries at one slot is the valid
        // representation of a layer parameter sampled at current_time and at
        // other times, so it is admitted.
        for (index, layer) in request.layers.iter().enumerate() {
            // Same slot and dimension bounds the worker parser enforces.
            if (layer.slot == 0 && (layer.timed.is_none() || layer.dynamic))
                || layer.slot > 1024
                || layer.width == 0
                || layer.height == 0
                || layer.width > MAX_DIMENSION
                || layer.height > MAX_DIMENSION
                || u64::from(layer.width) * u64::from(layer.height) > MAX_PIXELS
            {
                return Err(invalid(
                    "render session layer slot or dimensions are invalid",
                ));
            }
            if let Some((_, time_scale)) = layer.timed {
                if time_scale == 0 {
                    return Err(invalid("render session timed layer time scale is zero"));
                }
            }
            for other in &request.layers[..index] {
                if other.slot != layer.slot {
                    continue;
                }
                let conflict = match (other.timed, layer.timed) {
                    // Both timed: a collision only when the rational times are
                    // equal (cross-multiplied to avoid dividing).
                    (Some((lt, ls)), Some((rt, rs))) => {
                        i64::from(lt) * i64::from(rs) == i64::from(rt) * i64::from(ls)
                    }
                    // Two static entries at one slot are ambiguous per frame.
                    (None, None) => true,
                    // A static entry plus a timed entry is the valid mix.
                    _ => false,
                };
                if conflict {
                    return Err(invalid("render session layer slots must be unique"));
                }
            }
            // The transported RGBA must match the declared per-layer dimensions
            // exactly; each layer occupies its own primary-independent slot.
            let expected = layer.width as usize * layer.height as usize * 4;
            if layer.rgba.len() != expected {
                return Err(invalid(
                    "render session layer pixels do not match dimensions",
                ));
            }
        }
        // The output slot starts at the render dimensions; an in-session grow
        // (#262) raises the capacity when an expand overruns it.
        let (output_capacity_width, output_capacity_height) = (request.width, request.height);
        let geometry = SessionGeometry {
            width: request.width,
            height: request.height,
            output_capacity_width,
            output_capacity_height,
            pixel_format: request.pixel_format,
            layer_slot_count: request.layers.len() as u32,
        };
        if geometry.section_bytes() as u64 > SECTION_HARD_CAP_BYTES {
            return Err(invalid("render session section exceeds the hard cap"));
        }
        // Smart sessions now carry the same secondary-layer trailer as the
        // classic session (issue #294) and the same static context trailers
        // (mask/spatial/render, issue #331): both ride the positional tail below
        // in the one-shot order, and the worker's smart session command peels
        // them exactly as the classic session command does.
        // GPU selection follows the requested backend, not possession of a
        // legacy module-policy receipt. The ordinary isolated session records
        // loaded modules and keeps the same Job Object and output invariants.
        let gpu_capable = request.smart && request.pixel_format == RenderPixelFormat::Argb32f;
        let effective_backend = request.gpu_backend;
        let gpu_attempt = gpu_capable && runtime_backend(effective_backend).is_some();
        // Issue #816: resident render is in-place only. A non-empty search
        // root set is now part of the protocol rather than a mode selector.
        if request.dependency_search_dirs.is_empty() {
            return Err(invalid(
                "an in-place session requires dependency search directories",
            ));
        }
        if !request.dependencies.is_empty() {
            return Err(invalid(
                "an in-place session resolves dependencies by search directory, not by staged artifact",
            ));
        }
        // A GPU attempt authenticates a single-plugin runtime module policy;
        // combining it with a cluster manifest is out of scope for the
        // cluster session design, so it fails closed rather than widening the
        // trust decision silently.
        if gpu_attempt && cluster.is_some() {
            return Err(invalid(
                "cluster sessions do not carry a GPU runtime module policy",
            ));
        }
        let command = session_command(request.pixel_format, request.smart, effective_backend)?;
        // A pre-encoded payload is bounded here the way the default encoder
        // bounds the one it builds, so no caller can widen the launch argv past
        // the limit the worker's parser is written against.
        let payload = match request.payload_override {
            Some(payload) => {
                if payload.len() > 16384 {
                    return Err(invalid("interactive parameter payload is too large"));
                }
                payload.to_owned()
            }
            None => encode_default_interactive_payload(request.parameters.unwrap_or_default())?,
        };
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
        transport.write_header_u32(VERSION_OFFSET, SESSION_HEADER_VERSION);
        transport.write_header_u32(DEPTH_CODE_OFFSET, depth_code(request.pixel_format));
        transport.write_header_u32(MAX_WIDTH_OFFSET, request.width);
        transport.write_header_u32(MAX_HEIGHT_OFFSET, request.height);
        transport.write_header_u32(LAYER_SLOT_COUNT_OFFSET, geometry.layer_slot_count);
        transport.write_header_u32(INPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(OUTPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(FRAME_WIDTH_OFFSET, request.width);
        transport.write_header_u32(FRAME_HEIGHT_OFFSET, request.height);
        // Layers are static and no longer occupy the section (#268): stream each
        // layer's RGBA8 to its own file under target/image-transport and hand the
        // worker an inherited, path-authenticated read HANDLE. The worker reads it
        // once at open into a private vector and never re-opens a path for *pixel*
        // transport (issue #18 TOCTOU lesson). Broker-written sidecars whose
        // contents are not pixels still travel by path (`--aux-manifest-v1`,
        // `--parameter-animation-v1`, and the audio source of #339); those are
        // read during argv parsing, before the plug-in module is loaded, so no
        // plug-in code is running in that process to swap the leaf. Because the
        // pixels leave the
        // bounded section, layer count/size no longer feed the aggregate section
        // cap, so the one-shot per-file layered path has no capability the session
        // lacks. `layer_files` keeps the broker's inheritable read handles alive
        // through the spawn (dropped at the end of open once the worker inherited
        // its own copies); `layer_sidecars` deletes the files when the session
        // ends. Built incrementally so an early return still cleans up.
        let mut layer_sidecars = LayerSidecars(Vec::with_capacity(request.layers.len()));
        let mut layer_files: Vec<std::fs::File> = Vec::with_capacity(request.layers.len());
        let mut layer_handles: Vec<HANDLE> = Vec::with_capacity(request.layers.len());
        let mut dynamic_layers: Vec<DynamicLayer> = Vec::new();
        if !request.layers.is_empty() {
            let root = request.repository.join("target/image-transport");
            fs::create_dir_all(&root)?;
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| invalid(error.to_string()))?
                .as_nanos();
            for (index, layer) in request.layers.iter().enumerate() {
                let path = root.join(format!("layer-session-{nonce}-{index}.rgba"));
                let mut writer = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                // The file now exists on disk; track it for cleanup BEFORE the
                // fallible write so a mid-write failure (or any later early
                // return) still removes it instead of leaking a partial file.
                layer_sidecars.0.push(path.clone());
                writer.write_all(&layer.rgba)?;
                drop(writer);
                let file = OpenOptions::new().read(true).open(&path)?;
                let handle = file.as_raw_handle() as HANDLE;
                if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }
                    == 0
                {
                    return Err(io::Error::last_os_error());
                }
                layer_handles.push(handle);
                layer_files.push(file);
                if layer.dynamic {
                    // A second, writable handle to the same file. The worker's
                    // inherited handle is read-only, and this one is not
                    // inheritable, so the plug-in's process can never write the
                    // map it is being shown.
                    dynamic_layers.push(DynamicLayer {
                        slot: layer.slot,
                        bytes: layer.rgba.len(),
                        file: OpenOptions::new().write(true).open(&path)?,
                    });
                }
            }
        }

        let plugin = ApprovedImageArtifact {
            path: request.plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(request.plugin_sha256)?,
            expected_size: fs::metadata(request.plugin_path)?.len(),
        };
        let args_before_plugin = vec![command.to_owned()];
        let mut args_after_plugin = vec![
            request.plugin_sha256.to_ascii_lowercase(),
            payload,
            request.width.to_string(),
            request.height.to_string(),
            request.time_step.to_string(),
            request.total_time.to_string(),
            request.time_scale.to_string(),
        ];
        let mut service_plugins = vec![request.plugin_path];
        if let Some(cluster) = &cluster {
            service_plugins.extend(cluster.plugins.iter().map(|plugin| plugin.path.as_path()));
        }
        let render_service = crate::render_service::RenderServiceLease::acquire(&service_plugins)?;
        let companion_transport =
            crate::companion_manifest::write_transport(request.repository, &request.companions)?;
        // The secondary-layer trailer sits ahead of the context trailers in
        // the positional tail (issue #98 W1-4). The pixels travel as inherited
        // file HANDLEs (#268), so each entry now carries its layer's read handle
        // value as the final field (v2): static `slot,w,h,handle`, timed
        // `slot,w,h,time,scale,handle`. The handle is inherited, so its numeric
        // value is identical in the worker; the worker reads exactly w*h*4 bytes
        // from it into the layer's private vector.
        if !request.layers.is_empty() {
            let mut encoded = String::from("session-layers:v2|");
            for (index, layer) in request.layers.iter().enumerate() {
                if index != 0 {
                    encoded.push(';');
                }
                let handle = layer_handles[index] as usize;
                match (layer.timed, layer.dynamic) {
                    (Some((time, time_scale)), _) => encoded.push_str(&format!(
                        "{},{},{},{},{},{}",
                        layer.slot, layer.width, layer.height, time, time_scale, handle
                    )),
                    // Five fields, the trailing 1 marking a layer the broker
                    // rewrites between frames (issue #674).
                    (None, true) => encoded.push_str(&format!(
                        "{},{},{},{},1",
                        layer.slot, layer.width, layer.height, handle
                    )),
                    (None, false) => encoded.push_str(&format!(
                        "{},{},{},{}",
                        layer.slot, layer.width, layer.height, handle
                    )),
                }
            }
            args_after_plugin.push(encoded);
        }
        // Static context trailers ride the positional tail in the one-shot
        // order (mask, spatial, render), ahead of the auxiliary option pairs
        // the worker peels first.
        if let Some(mask) = &request.mask_trailer {
            args_after_plugin.push(mask.clone());
        }
        if let Some(spatial) = &request.spatial_trailer {
            args_after_plugin.push(spatial.clone());
        }
        if let Some(render_environment) = &request.render_environment_trailer {
            args_after_plugin.push(render_environment.clone());
        }
        // The audio trailer sits after the context trailers so the worker peels it
        // first and the context/layer chain keeps the positions it already had
        // (issue #339). Auxiliary option pairs are stripped before any of this.
        if let Some(audio) = &request.audio_trailer {
            args_after_plugin.push(audio.clone());
        }
        // Auxiliary option pairs ride argv's tail; the worker peels them
        // before the positional session contract (strip_auxiliary_options)
        // and validates each strictly. The broker pre-checks the path shapes
        // so a misconfigured request fails fast at open instead of as an
        // opaque worker exit on the first frame.
        if !request.alpha_as_coverage_params.is_empty() {
            // Same validation and encoding as the one-shot path
            // (image_render.rs): sorted, unique, slot <= 1024. The worker's
            // shared auxiliary hook parses this identically for the session and
            // one-shot Render entries.
            let mut slots = request.alpha_as_coverage_params.to_vec();
            slots.sort_unstable();
            if slots.windows(2).any(|pair| pair[0] == pair[1])
                || slots.iter().any(|slot| *slot > 1024)
            {
                return Err(invalid("alpha-as-coverage parameter slots are invalid"));
            }
            args_after_plugin.extend([
                "--alpha-as-coverage-v1".to_owned(),
                slots
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            ]);
        }
        if let Some(manifest) = request.aux_manifest {
            if !manifest.is_absolute() || !manifest.is_file() {
                return Err(invalid("aux manifest must be an absolute path to a file"));
            }
            args_after_plugin.extend([
                "--aux-manifest-v1".to_owned(),
                manifest.to_string_lossy().into_owned(),
            ]);
        }
        if let Some(requested) = request.world_dump_dir {
            // The one-shot managed dump resolver enforces the broker's dump
            // boundary (canonically under <repository>/target) and the
            // fresh-directory rule, since the worker never clears snapshots
            // it does not overwrite.
            let dump =
                crate::image_render::resolve_managed_dump_dir(request.repository, requested, true)?;
            args_after_plugin.extend([
                "--dump-worlds-v1".to_owned(),
                dump.path.to_string_lossy().into_owned(),
            ]);
        }
        if request.output_checksum_detail {
            args_after_plugin.extend(["--output-checksum-detail-v1".to_owned(), "1".to_owned()]);
        }
        // Conformance render settings ride the same shared auxiliary option the
        // one-shot path forwards (#275); the worker peels it from the tail and
        // feeds it to its report only (the pixels are already pre-transformed by
        // the broker on both routes), so a conformance render reports the same
        // render_settings block on the session and one-shot routes.
        if let Some(settings) = request.conformance_render_settings {
            args_after_plugin.extend([
                "--conformance-render-settings-v1".to_owned(),
                settings.to_owned(),
            ]);
        }
        if let Some(sidecar) = &animation_sidecar {
            args_after_plugin.extend([
                "--parameter-animation-v1".to_owned(),
                sidecar.0.to_string_lossy().into_owned(),
            ]);
        }
        if let Some(transport) = &companion_transport {
            // Auxiliary pairs must be at argv's tail. The worker strips them
            // backwards before interpreting any layer/context positional
            // trailers, so inserting this beside the fixed session arguments
            // makes every layered companion launch malformed.
            args_after_plugin.extend([
                "--companion-manifest-v1".to_owned(),
                transport.path().to_string_lossy().into_owned(),
            ]);
        }
        append_plugin_data_selector_args(&mut args_after_plugin, plugin_data_selector)?;
        // The session always launches at the render dimensions; an expand grows
        // the output slot in place mid-session (#262), so there is no launch-time
        // output-capacity trailer.
        // A GPU render carries its authenticated policy's modules to the render
        // worker as an AEXRMA1 manifest (#300): the worker parses it so the GPU
        // runtime DLLs it loads classify as authorized `policy` in the required
        // module audit instead of `unknown` (which fails the audit and the GPU
        // device setup). The transport is retained by RenderSession so an
        // in-place worker can read it during admission without a launch/drop race.
        let mut dependencies = request.dependencies;
        let _runtime_authorization = if gpu_attempt && request.gpu_runtime_policy.is_some() {
            let policy_input = request
                .gpu_runtime_policy
                .expect("gpu attempt was validated to carry a policy at open");
            let backend =
                runtime_backend(effective_backend).expect("GPU attempt has a runtime backend");
            // Reuse the preflight's session identity so the manifest the worker
            // parses matches the identity the report was authenticated against
            // below, keeping the manifest/report/session binding intact
            // (#301 review).
            let transport = crate::runtime_module_authorization::prepare_runtime_authorization_transport_with_identity(
                    request.repository,
                    policy_input.policy,
                    backend,
                    policy_input.session_identity,
                )?;
            transport.append_launch(
                !request.dependency_search_dirs.is_empty(),
                &mut args_after_plugin,
                &mut dependencies,
            );
            Some(transport)
        } else {
            None
        };
        let child_handles = SessionChildHandles {
            request_read: request_read.raw(),
            response_write: response_write.raw(),
            section: transport.section.raw(),
            // Per-layer inherited read handles (#268); their numeric values also
            // ride the session-layers trailer so the worker knows which handle
            // carries which layer.
            layers: layer_handles.clone(),
        };
        let mut cluster_state = None;
        let mut in_place_transport = None;
        let process = if let Some(cluster) = cluster {
            let cluster_dispatch = crate::secure_image_dispatch::SecureInPlaceClusterDispatch {
                repository: request.repository,
                worker_kind: if request.smart {
                    WorkerKind::Smart
                } else {
                    WorkerKind::Classic
                },
                plugins: cluster.plugins,
                dependency_search_dirs: request.dependency_search_dirs.clone(),
                positional_plugin: true,
                swap_payloads: Some(&cluster.swap_payloads),
                module_bound: cluster.module_bound,
                args_before_plugin: &args_before_plugin,
                args_after_plugin: &args_after_plugin,
                launch_environment: request.launch_environment.clone(),
            };
            let launch = match desktop_policy {
                WorkerDesktopPolicy::Dedicated => {
                    crate::secure_image_dispatch::dispatch_secure_in_place_cluster_session(
                        cluster_dispatch,
                        &child_handles,
                    )?
                }
                WorkerDesktopPolicy::Current => {
                    crate::secure_image_dispatch::dispatch_secure_in_place_cluster_session_with_policy(
                        cluster_dispatch,
                        &child_handles,
                        WorkerDesktopPolicy::Current,
                    )?
                }
            };
            cluster_state = Some(ClusterSessionState {
                plugin_count: launch.manifest.plugin_count() as u32,
                current_plugin_index: 0,
            });
            in_place_transport = Some(launch.transport);
            launch.process
        } else {
            let dispatch = SecureImageDispatch {
                repository: request.repository,
                worker_kind: if request.smart {
                    WorkerKind::Smart
                } else {
                    WorkerKind::Classic
                },
                plugin,
                dependencies,
                dependency_search_dirs: request.dependency_search_dirs.clone(),
                args_before_plugin: &args_before_plugin,
                args_after_plugin: &args_after_plugin,
                timeout: Some(request.frame_deadline),
                launch_environment: request.launch_environment.clone(),
            };
            if gpu_attempt && request.gpu_runtime_policy.is_some() {
                let policy_input = request
                    .gpu_runtime_policy
                    .expect("gpu attempt was validated to carry a policy at open");
                let backend =
                    runtime_backend(effective_backend).expect("GPU attempt has a runtime backend");
                let report = authenticate_gpu_worker_report(
                    policy_input.module_report_json,
                    &policy_input.session_identity,
                    backend,
                    WorkerModuleValidation {
                        policy: policy_input.policy,
                        sealed: policy_input.sealed_modules,
                        trusted: policy_input.trusted_modules,
                        system32: policy_input.system32,
                    },
                )?;
                match desktop_policy {
                    WorkerDesktopPolicy::Dedicated => dispatch_secure_gpu_image_session(
                        dispatch,
                        GpuRuntimeAuthorization {
                            backend,
                            session_identity: policy_input.session_identity,
                            module_report: &report,
                        },
                        &child_handles,
                    )?,
                    WorkerDesktopPolicy::Current => {
                        crate::secure_image_dispatch::dispatch_secure_gpu_image_session_on_current_desktop(
                            dispatch,
                            GpuRuntimeAuthorization {
                                backend,
                                session_identity: policy_input.session_identity,
                                module_report: &report,
                            },
                            &child_handles,
                        )?
                    }
                }
            } else {
                match desktop_policy {
                    WorkerDesktopPolicy::Dedicated => {
                        dispatch_secure_image_session(dispatch, &child_handles)?
                    }
                    WorkerDesktopPolicy::Current => {
                        crate::secure_image_dispatch::dispatch_secure_image_session_on_current_desktop(
                            dispatch,
                            &child_handles,
                        )?
                    }
                }
            }
        };
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
            use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
            let handle = watched_process as HANDLE;
            unsafe {
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
            }
            let _ = sender.send(SessionEvent::ProcessExited);
        });

        Ok(RenderSession {
            process: Some(process),
            _render_service: render_service,
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
            smart_output_untouched_frames: 0,
            parameter_update_frames: 0,
            opened: Instant::now(),
            plugin_sha256: request.plugin_sha256.to_ascii_lowercase(),
            smart: request.smart,
            cluster: cluster_state,
            _in_place_transport: in_place_transport,
            _companion_transport: companion_transport,
            _runtime_authorization,
            _animation_sidecar: animation_sidecar,
            _layer_sidecars: layer_sidecars,
            dynamic_layers,
        })
    }

    /// Opens a resident render session over a whole plugin cluster (issue
    /// #405/#816): every plugin is authenticated at its real path, the launch
    /// carries a `cluster-manifest-v2` document, and `swap_plugin` later
    /// selects another manifest member without a new process. The base
    /// request's plugin identity must name `cluster.plugins[0]`.
    pub fn open_cluster(
        request: SessionOpenRequest<'_>,
        cluster: ClusterRenderPlugins,
    ) -> io::Result<RenderSession> {
        Self::open_with_desktop_policy(request, WorkerDesktopPolicy::Dedicated, Some(cluster), None)
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
        self.collected = Some(match process.finish(Some(wait)) {
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

    /// Swaps the loaded plugin to another member of the launch-authenticated
    /// cluster manifest (issue #405, design §4.1): sends
    /// `{"v":1,"type":"swap_plugin","plugin_index":N}` and waits for
    /// `swap_done` under the same three-way wait a frame uses (response pipe,
    /// process-death watcher, deadline). The message carries only an index
    /// into the manifest — never a path or hash — so no launch-time trust
    /// decision is revisited. An out-of-manifest or current index is a plain
    /// caller error rejected before anything is sent; a protocol violation,
    /// worker death, or a missed deadline invalidates the whole session
    /// fail-closed. The broker answers the swap synchronously, so the
    /// "no render_frame until swap_done" contract (design §4.1) holds by
    /// construction.
    pub fn swap_plugin(&mut self, plugin_index: u32) -> io::Result<SwapOutcome> {
        if let Some(invalidation) = &self.invalidation {
            return Err(invalid(format!(
                "render session is invalidated ({}): {}",
                invalidation.reason, invalidation.detail
            )));
        }
        let (plugin_count, current_index) = self
            .cluster
            .as_ref()
            .map(|cluster| (cluster.plugin_count, cluster.current_plugin_index))
            .ok_or_else(|| invalid("render session was not opened as a cluster session"))?;
        if plugin_index >= plugin_count {
            return Err(invalid("swap plugin index is outside the cluster manifest"));
        }
        if plugin_index == current_index {
            return Err(invalid("swap plugin index is the current plugin"));
        }
        // Between exchanges, a queued process-death event fails the swap
        // before anything is sent; a queued message with no exchange in
        // flight is a protocol violation.
        loop {
            match self.receiver.try_recv() {
                Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                Ok(SessionEvent::ReaderViolation) => {
                    return Err(self.invalidate(
                        "response_framing_violation",
                        "the worker broke the response framing before the swap".into(),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Ok(SessionEvent::Message(_)) => {
                    return Err(self.invalidate(
                        "unsolicited_response",
                        "a response arrived with no exchange in flight before the swap".into(),
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
                "the worker exited before the swap was dispatched".into(),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let message =
            format!("{{\"v\":1,\"type\":\"swap_plugin\",\"plugin_index\":{plugin_index}}}");
        if !self.transport.send_message(&message) {
            return Err(self.invalidate(
                "request_pipe_closed",
                "the session request pipe rejected a swap_plugin message".into(),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let body = match self.await_frame_response(Instant::now() + self.frame_deadline) {
            FrameWait::Message(body) => body,
            FrameWait::Deadline => {
                return Err(self.invalidate(
                    "swap_deadline",
                    format!(
                        "the swap to plugin {plugin_index} exceeded the {}ms deadline",
                        self.frame_deadline.as_millis()
                    ),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::WorkerGone => {
                return Err(self.invalidate(
                    "worker_exited",
                    format!(
                        "the worker was gone before the swap to plugin {plugin_index} completed"
                    ),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::FramingViolation => {
                return Err(self.invalidate(
                    "response_framing_violation",
                    "the worker broke the response framing during the swap".into(),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        let done: SwapDone = match serde_json::from_slice(&body) {
            Ok(done) => done,
            Err(error) => {
                return Err(self.invalidate(
                    "malformed_swap_done",
                    format!("the swap response did not parse strictly: {error}"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        if done.v != PROTOCOL_VERSION
            || done.kind != "swap_done"
            || done.plugin_index != plugin_index
        {
            return Err(self.invalidate(
                "swap_done_mismatch",
                format!(
                    "the swap response carried v={} type={} plugin_index={}",
                    done.v, done.kind, done.plugin_index
                ),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let outcome = match done.status.as_str() {
            "ok" => {
                if done.global_setup_error.is_some() {
                    return Err(self.invalidate(
                        "malformed_swap_done",
                        "an ok swap response carried a global_setup_error".into(),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                SwapOutcome::Swapped
            }
            // A GLOBAL_SETUP failure is plugin-local (design §4.1): the
            // session stays usable and the caller decides whether to
            // continue. The worker did swap to the requested plugin, so the
            // broker's current index follows it.
            "error" => match done.global_setup_error {
                Some(global_setup_error) if global_setup_error != 0 => {
                    SwapOutcome::PluginError { global_setup_error }
                }
                _ => {
                    return Err(self.invalidate(
                        "malformed_swap_done",
                        "an error swap response missed a non-zero global_setup_error".into(),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
            },
            other => {
                return Err(self.invalidate(
                    "unknown_swap_status",
                    format!("the swap response reported status {other:?}"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        self.cluster
            .as_mut()
            .expect("cluster state checked above")
            .current_plugin_index = plugin_index;
        Ok(outcome)
    }

    /// Replaces the pixels of a layer opened as `dynamic` (issue #674), for
    /// every frame from the next one on.
    ///
    /// The worker re-reads the layer before each frame it renders, and this
    /// session is the only writer, so the ordering that keeps a frame from
    /// seeing half an update is the request/response cycle itself: write here,
    /// then send the frame and wait for its reply. Calling this while a frame
    /// is in flight is not possible through `&mut self`.
    ///
    /// Geometry is fixed at open, so an update of a different length is
    /// rejected: the worker's PF world was built for the opening dimensions and
    /// would otherwise read past what it was handed.
    pub fn update_dynamic_layer(&mut self, slot: u32, rgba: &[u8]) -> io::Result<()> {
        if let Some(invalidation) = &self.invalidation {
            return Err(invalid(format!(
                "render session is invalidated ({}): {}",
                invalidation.reason, invalidation.detail
            )));
        }
        let Some(layer) = self
            .dynamic_layers
            .iter_mut()
            .find(|layer| layer.slot == slot)
        else {
            return Err(invalid(format!(
                "layer slot {slot} was not opened as a dynamic layer"
            )));
        };
        if rgba.len() != layer.bytes {
            return Err(invalid(format!(
                "dynamic layer {slot} was opened for {} bytes but the update carries {}",
                layer.bytes,
                rgba.len()
            )));
        }
        layer.file.seek(SeekFrom::Start(0))?;
        layer.file.write_all(rgba)?;
        layer.file.flush()?;
        Ok(())
    }

    pub fn render_frame(
        &mut self,
        frame_index: u32,
        current_time: i32,
        rgba: &[u8],
    ) -> io::Result<FrameOutcome> {
        self.render_frame_with_parameters(frame_index, current_time, rgba, None)
    }

    /// Renders one frame carrying the launch payload's parameters (v:2
    /// `render_frame` with a per-frame `parameters` attribute) but no custom-UI
    /// action. Thin delegate kept for callers that only drive parameters.
    pub fn render_frame_with_parameters(
        &mut self,
        frame_index: u32,
        current_time: i32,
        rgba: &[u8],
        parameters: Option<&[InteractiveParameter]>,
    ) -> io::Result<FrameOutcome> {
        self.render_frame_with_attributes(frame_index, current_time, rgba, parameters, None)
    }

    /// Renders one frame, optionally carrying per-frame dynamic attributes that
    /// override the launch configuration for this frame only through the v:2
    /// `render_frame` message (protocol §4.2.1): `parameters` (issue #107) and
    /// `ui_action` (issue #238). Each attribute rides the message in the same
    /// encoding as its one-shot argv form and goes through the identical
    /// broker-side validation before anything is sent, so an invalid value is a
    /// plain caller error and the session stays usable. With neither attribute
    /// a plain v:1 frame goes out.
    pub fn render_frame_with_attributes(
        &mut self,
        frame_index: u32,
        current_time: i32,
        rgba: &[u8],
        parameters: Option<&[InteractiveParameter]>,
        ui_action: Option<&RenderUiAction>,
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
        // Encode and validate the per-frame attributes before any slot write,
        // so a rejected value leaves the transport state untouched. v:2 carries
        // the present attributes as presence-driven fields (protocol §4.2.1);
        // with neither, a plain v:1 frame goes out. Key order is irrelevant to
        // the worker's exact-key set check.
        let message = if parameters.is_none() && ui_action.is_none() {
            format!(
                "{{\"v\":1,\"type\":\"render_frame\",\"frame_index\":{frame_index},\
                 \"current_time\":{{\"value\":{current_time},\"scale\":{}}}}}",
                self.time_scale
            )
        } else {
            let mut message_value = json!({
                "v": 2,
                "type": "render_frame",
                "frame_index": frame_index,
                "current_time": {"value": current_time, "scale": self.time_scale},
            });
            let object = message_value
                .as_object_mut()
                .expect("object literal is an object");
            if let Some(parameters) = parameters {
                object.insert(
                    "parameters".into(),
                    Value::String(encode_interactive_payload(parameters)?),
                );
            }
            if let Some(ui_action) = ui_action {
                object.insert(
                    "ui_action".into(),
                    Value::String(ui_action.encode_ui_field()?),
                );
            }
            serde_json::to_string(&message_value).map_err(|error| invalid(error.to_string()))?
        };
        // The encoder's 16 KiB payload cap keeps every message far below the
        // protocol's 64 KiB framing limit today, but the bound is enforced
        // here regardless: an oversized message must be a caller error before
        // any transport mutation, never a send failure that kills a healthy
        // session.
        if message.len() > MAX_MESSAGE_BYTES {
            return Err(invalid(
                "per-frame parameter message exceeds the protocol message cap",
            ));
        }
        self.transport.write_input_slot(rgba);
        self.transport
            .write_header_u32(INPUT_GENERATION_OFFSET, expected_generation);
        if !self.transport.send_message(&message) {
            return Err(self.invalidate(
                "request_pipe_closed",
                "the session request pipe rejected a frame message".into(),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        if parameters.is_some() {
            self.parameter_update_frames += 1;
        }
        // The worker may answer a single dispatched frame with more than one
        // control message: an expand that overruns the launch slot replies
        // resize_needed, the broker grows the shared section in place (protocol
        // §3, issue #262), and the worker then answers ok for the same frame.
        // Loop until a terminal (ok/error) status; a resize_needed grows and
        // waits for the follow-up without re-dispatching the frame. A legitimate
        // frame needs at most one grow (the worker reports its full required size
        // and renders exactly once into a private buffer), so a second
        // resize_needed for the same frame is a fail-closed protocol violation,
        // bounding section-allocation amplification from a buggy worker.
        let mut grows: u32 = 0;
        // One watchdog for the whole dispatched frame: a resize/grow handshake
        // (#262) reuses this instant across the resize wait, the grow work, and
        // the follow-up ok, so an expand frame cannot run for nearly two full
        // per-frame deadlines by resetting the clock at each control message.
        let frame_deadline_at = Instant::now() + self.frame_deadline;
        loop {
            let body = match self.await_frame_response(frame_deadline_at) {
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
                        format!("the worker broke the response framing during frame {frame_index}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
            };
            let mut done: FrameDone = match serde_json::from_slice(&body) {
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
            if done.v != PROTOCOL_VERSION
                || done.kind != "frame_done"
                || done.frame_index != frame_index
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
            // The top-level width/height fields belong only to a resize_needed
            // response; deny_unknown_fields treats them as known for every status,
            // so reject them here on ok/error to keep the frame_done schema strict.
            let carries_resize_fields = done.width.is_some() || done.height.is_some();
            if done
                .missing_dependency
                .as_deref()
                .is_some_and(|name| !valid_dependency_basename(name))
            {
                return Err(self.invalidate(
                    "malformed_dependency_diagnostic",
                    format!("frame {frame_index} carried an unsafe dependency name"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            // Dropped rather than escalated: see `admissible_return_message`.
            if done
                .return_message
                .as_ref()
                .is_some_and(|message| !admissible_return_message(message))
            {
                done.return_message = None;
            }
            match done.status.as_str() {
                "error" => {
                    if done.output.is_some()
                        || done.generation.is_some()
                        || done.render_error == 0
                        || (done.smart_output_untouched
                            && (!self.smart
                                || done.render_error != -6
                                || done.missing_dependency.is_some()
                                || done.return_message.is_some()))
                        || carries_resize_fields
                    {
                        return Err(self.invalidate(
                            "malformed_error_response",
                            format!(
                                "frame {frame_index} error response carried output/resize fields"
                            ),
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
                        let dependency = done
                            .missing_dependency
                            .as_deref()
                            .map(|name| format!(", missing dependency {name}"))
                            .unwrap_or_default();
                        return Err(self.invalidate(
                            "worker_invariant_failure",
                            format!(
                                "frame {frame_index} reported the fatal session error {}{}",
                                done.render_error, dependency
                            ),
                            false,
                            CLOSE_COLLECT_TIMEOUT,
                        ));
                    }
                    // Frame-local diagnostic (protocol §4.3): the sequence state
                    // is still host-owned, so the session continues; whether to
                    // proceed is the caller's decision.
                    self.frames_errored += 1;
                    if done.smart_output_untouched {
                        self.smart_output_untouched_frames += 1;
                    }
                    return Ok(FrameOutcome {
                        frame_index,
                        status: if done.smart_output_untouched {
                            FrameStatus::SmartOutputUntouched
                        } else {
                            FrameStatus::FrameError {
                                render_error: done.render_error,
                                missing_dependency: done.missing_dependency,
                                return_message: done.return_message,
                            }
                        },
                    });
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
                    if carries_resize_fields
                        || done.missing_dependency.is_some()
                        || done.smart_output_untouched
                    {
                        return Err(self.invalidate(
                            "malformed_ok_response",
                            format!("frame {frame_index} ok response carried resize fields"),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                    if let Err(detail) = self.validate_ok_frame(
                        expected_generation,
                        &output,
                        generation,
                        done.render_error,
                    ) {
                        return Err(self.invalidate(
                            "frame_invariant_failure",
                            format!("frame {frame_index}: {detail}"),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                    // Read only the frame's actual packed bytes, not the whole
                    // launch slot: a shrink-output effect fills less than the
                    // slot, and the worker packs exactly these bytes (#261).
                    let actual_bytes = output.width as usize
                        * output.height as usize
                        * self.geometry.pixel_format.bytes_per_pixel() as usize;
                    // Both sides must agree on how much of the slot this frame
                    // occupies. Hashing those bytes twice per frame proved
                    // nothing beyond this agreement -- the worker computed its
                    // own hash, so it could never attest to its own honesty --
                    // and cost ~8 MiB of SHA-256 twice at 1080p (issue #690).
                    if output.packed_bytes != actual_bytes as u64 {
                        return Err(self.invalidate(
                            "output_extent_mismatch",
                            format!(
                                "frame {frame_index} reports {}x{} ({actual_bytes} bytes) but the worker packed {} bytes",
                                output.width, output.height, output.packed_bytes
                            ),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                    let pixels = self
                        .transport
                        .read_output_slot(self.geometry.output_slot_offset(), actual_bytes);
                    self.frames_ok += 1;
                    self.last_output_generation = expected_generation;
                    return Ok(FrameOutcome {
                        frame_index,
                        status: FrameStatus::Rendered {
                            pixels,
                            width: output.width,
                            height: output.height,
                            origin_x: output.origin_x,
                            origin_y: output.origin_y,
                        },
                    });
                }
                "resize_needed" => {
                    // The effect rendered larger than the launch slot; the worker
                    // wrote nothing and left the generation untouched (#261). It
                    // carries only width/height. Bound the requested size so a
                    // misbehaving worker cannot force an unbounded re-open, and
                    // require it to actually exceed the current slot.
                    if done.output.is_some() || done.generation.is_some() || done.render_error != 0
                    {
                        return Err(self.invalidate(
                            "malformed_resize_response",
                            format!(
                                "frame {frame_index} resize_needed carried output/generation/error"
                            ),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                    let (Some(width), Some(height)) = (done.width, done.height) else {
                        return Err(self.invalidate(
                            "malformed_resize_response",
                            format!("frame {frame_index} resize_needed missed width or height"),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    };
                    let requested_pixels = width as u64 * height as u64;
                    // Compare against the current output-slot capacity, not the
                    // render dimensions: a re-opened session already has a larger
                    // slot, and the worker only reports resize_needed when the
                    // output overruns that slot (#261).
                    let current_pixels = u64::from(self.geometry.output_capacity_width)
                        * u64::from(self.geometry.output_capacity_height);
                    if width == 0
                        || height == 0
                        || width > MAX_RESIZE_DIMENSION
                        || height > MAX_RESIZE_DIMENSION
                        || requested_pixels > MAX_RESIZE_PIXELS
                        || requested_pixels <= current_pixels
                    {
                        return Err(self.invalidate(
                        "resize_out_of_range",
                        format!(
                            "frame {frame_index} resize_needed {width}x{height} is out of range \
                             (current capacity {}x{})",
                            self.geometry.output_capacity_width,
                            self.geometry.output_capacity_height
                        ),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                    }
                    // The header and generation must be untouched, like an error
                    // response: nothing was written to the slot.
                    if let Err(detail) = self.validate_static_header() {
                        return Err(self.invalidate(
                            "frame_invariant_failure",
                            format!("frame {frame_index} (resize response): {detail}"),
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
                                "frame {frame_index} resize response advanced the output generation"
                            ),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                    // A legitimate frame grows at most once; a second resize_needed
                    // for the same frame is a fail-closed violation (bounds the
                    // section-allocation work a buggy worker can force).
                    grows += 1;
                    if grows > 1 {
                        return Err(self.invalidate(
                            "repeated_resize_needed",
                            format!("frame {frame_index} reported resize_needed more than once"),
                            true,
                            POST_TERMINATION_COLLECT_TIMEOUT,
                        ));
                    }
                    // Grow the shared section in place and wait for the worker's
                    // follow-up ok for this same frame (protocol §3, issue #262). The
                    // render lifecycle already ran once into the worker's private
                    // buffer, so this transfers those pixels into the larger slot
                    // instead of re-opening (which would replay SEQUENCE/FRAME setup
                    // and setdown). `?` invalidates the session on a grow failure.
                    self.grow_output_capacity(frame_index, width, height)?;
                    continue;
                }
                other => {
                    return Err(self.invalidate(
                        "unknown_frame_status",
                        format!("frame {frame_index} reported status {other:?}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
            }
        }
    }

    /// In-session output-slot grow (protocol §3, issue #262). The worker
    /// reported an expand that overran the launch slot; rather than tearing the
    /// session down and re-opening (which replays SEQUENCE/FRAME setup and
    /// setdown in a fresh worker), create a larger section, duplicate it into the
    /// same worker, and let it transfer the already-rendered frame. The render
    /// lifecycle ran exactly once, matching the one-shot path. The caller already
    /// bounded `(width, height)` and confirmed it exceeds the current capacity.
    fn grow_output_capacity(
        &mut self,
        frame_index: u32,
        width: u32,
        height: u32,
    ) -> io::Result<()> {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // The grown geometry keeps the render dimensions and layout, raising only
        // the output-slot capacity to the effect's requested output size.
        let mut grown = self.geometry;
        grown.output_capacity_width = width;
        grown.output_capacity_height = height;
        let section_bytes = grown.section_bytes();
        if section_bytes as u64 > SECTION_HARD_CAP_BYTES {
            return Err(self.invalidate(
                "resize_out_of_range",
                format!("frame {frame_index} grow to {width}x{height} exceeds the section cap"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        // The grown section is handed to the worker by DuplicateHandle, not by
        // inheritance, so it is created non-inheritable (null security): no other
        // spawned process should ever inherit this section carrying rendered
        // image bytes. On any failure below the current section stays live and
        // the session is invalidated.
        let section = match OwnedHandle::new(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                null(),
                PAGE_READWRITE,
                ((section_bytes as u64) >> 32) as u32,
                section_bytes as u32,
                null(),
            )
        }) {
            Ok(section) => section,
            Err(error) => {
                return Err(self.invalidate(
                    "grow_section_failed",
                    format!("frame {frame_index} grow could not create the section: {error}"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        let view_address = unsafe { MapViewOfFile(section.raw(), FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view_address.Value.is_null() {
            return Err(self.invalidate(
                "grow_section_failed",
                format!("frame {frame_index} grow could not map the section"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let view = view_address.Value as *mut u8;
        // Establish the static header the worker re-validates after adopting; the
        // layer slots stay uninitialised because the worker cached the layers
        // privately at open and never re-reads them from the section.
        init_section_header(view, &grown, self.last_output_generation);
        let unmap_new = || {
            let address = MEMORY_MAPPED_VIEW_ADDRESS {
                Value: view as *mut _,
            };
            unsafe {
                UnmapViewOfFile(address);
            }
        };
        // Duplicate the section into the worker process; it maps the value the
        // grow message carries. Uses the worker's own process handle, so a path
        // the worker re-opens (a TOCTOU surface) is never handed over.
        let Some(process) = self.process.as_ref() else {
            unmap_new();
            return Err(self.invalidate(
                "grow_section_failed",
                format!("frame {frame_index} grow has no worker process handle"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        };
        let worker_process = match process.duplicated_process_handle() {
            Ok(handle) => handle,
            Err(error) => {
                unmap_new();
                return Err(self.invalidate(
                    "grow_section_failed",
                    format!("frame {frame_index} grow could not open the worker process: {error}"),
                    true,
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        let mut duplicated: HANDLE = null_mut();
        let dup_ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                section.raw(),
                worker_process as HANDLE,
                &mut duplicated,
                0,
                0, // not inheritable
                DUPLICATE_SAME_ACCESS,
            )
        };
        unsafe {
            CloseHandle(worker_process as HANDLE);
        }
        if dup_ok == 0 {
            unmap_new();
            return Err(self.invalidate(
                "grow_section_failed",
                format!("frame {frame_index} grow could not duplicate the section into the worker"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        // Hand the duplicated handle and new capacity to the worker, then swap in
        // the broker-side section. A send failure invalidates; the duplicated
        // handle is left with the worker, which is being torn down.
        let grow_message = format!(
            "{{\"v\":1,\"type\":\"grow\",\"section_handle\":\"{}\",\
             \"output_capacity_width\":{width},\"output_capacity_height\":{height}}}",
            duplicated as usize
        );
        if !self.transport.send_message(&grow_message) {
            unmap_new();
            return Err(self.invalidate(
                "request_pipe_closed",
                format!("frame {frame_index} grow could not send the grow message"),
                true,
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        self.transport.adopt_section(section, view, section_bytes);
        self.geometry = grown;
        Ok(())
    }

    /// Protocol §7's three-way frame wait: the response channel, the process
    /// handle (via the watcher event), and the deadline. After a process-death
    /// event, a short drain still honors a frame_done the worker flushed
    /// before dying rather than racing the watcher. `deadline` is the single
    /// wall-clock instant for the whole dispatched frame, carried across a
    /// resize/grow handshake (#262) so an expand frame honors one watchdog
    /// rather than a fresh deadline per control message.
    fn await_frame_response(&mut self, deadline: Instant) -> FrameWait {
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
        // A legally empty SmartFX result (#278): PreRender skipped the render
        // selector, so there are no pixels and the geometry is 0x0. The worker
        // flags it explicitly (a zero dimension without the flag stays an
        // invariant failure below). Validate the empty shape and the generation,
        // then accept it — there are no slot bytes to read.
        if output.empty_result {
            // Only SmartFX PreRender produces a legally empty result; a classic
            // session must never claim one. Enforce this broker-side too so a
            // buggy or compromised classic worker cannot pass a 0x0 frame off as
            // valid by setting the flag (the worker also keeps classic frames
            // from setting it).
            if !self.smart {
                return Err("a classic session reported an empty SmartFX result".into());
            }
            if output.width != 0 || output.height != 0 || output.rowbytes != 0 {
                return Err(format!(
                    "empty-result frame must be 0x0 with 0 rowbytes, got {}x{} rowbytes {}",
                    output.width, output.height, output.rowbytes
                ));
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
            // The worker stamps the frame dimensions into the header like a
            // normal frame; for an empty result they must read back as 0x0,
            // matching the non-empty path's frame-dimension check.
            if self.transport.read_header_u32(FRAME_WIDTH_OFFSET) != 0
                || self.transport.read_header_u32(FRAME_HEIGHT_OFFSET) != 0
            {
                return Err("header frame dimensions are not empty".into());
            }
            self.validate_static_header()?;
            return Ok(());
        }
        // A resize-output effect may render at any positive dimensions that
        // still fit the launch output slot (#261): shrink, or an expand small
        // enough to fit. An expand that overruns the slot arrives as a
        // "resize_needed" status instead, never here.
        let bpp = self.geometry.pixel_format.bytes_per_pixel();
        if output.width == 0 || output.height == 0 {
            return Err(format!(
                "output geometry {}x{} is empty",
                output.width, output.height
            ));
        }
        // A resized output (dimensions other than the render dimensions) must
        // obey the same per-dimension and total-pixel caps as the worker's
        // validate_output_extent, so a buggy or compromised worker cannot report
        // an absurd shape (e.g. 1000000x1) that happens to fit a large slot by
        // total bytes. A fixed-size output was already bounded at session open.
        let is_resize =
            output.width != self.geometry.width || output.height != self.geometry.height;
        if is_resize
            && (output.width > MAX_RESIZE_DIMENSION
                || output.height > MAX_RESIZE_DIMENSION
                || u64::from(output.width) * u64::from(output.height) > MAX_RESIZE_PIXELS)
        {
            return Err(format!(
                "resized output geometry {}x{} exceeds the resize bounds",
                output.width, output.height
            ));
        }
        let actual_bytes = u64::from(output.width) * u64::from(output.height) * u64::from(bpp);
        if actual_bytes > self.geometry.output_slot_bytes() as u64 {
            return Err(format!(
                "output geometry {}x{} overruns the session slot capacity {}x{}",
                output.width,
                output.height,
                self.geometry.output_capacity_width,
                self.geometry.output_capacity_height
            ));
        }
        if output.rowbytes != u64::from(output.width) * u64::from(bpp) {
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
        // The worker writes the frame's actual (possibly resized) dimensions to
        // these headers (#261), so they must match the reported output, not the
        // launch max.
        if self.transport.read_header_u32(FRAME_WIDTH_OFFSET) != output.width
            || self.transport.read_header_u32(FRAME_HEIGHT_OFFSET) != output.height
        {
            return Err("frame dimension header does not match the reported output".into());
        }
        Ok(())
    }

    fn validate_static_header(&self) -> Result<(), String> {
        if self.transport.read_header_u32(MAGIC_OFFSET) != HEADER_MAGIC
            || self.transport.read_header_u32(VERSION_OFFSET) != SESSION_HEADER_VERSION
            || self.transport.read_header_u32(DEPTH_CODE_OFFSET)
                != depth_code(self.geometry.pixel_format)
            || self.transport.read_header_u32(MAX_WIDTH_OFFSET) != self.geometry.width
            || self.transport.read_header_u32(MAX_HEIGHT_OFFSET) != self.geometry.height
            || self.transport.read_header_u32(LAYER_SLOT_COUNT_OFFSET)
                != self.geometry.layer_slot_count
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
                // The liveness check above races the send: a worker that exits
                // between the two breaks the pipe, and reporting the failed
                // write would name the symptom instead of the exit that caused
                // it. Re-check before deciding which of the two this was.
                self.process_exit_observed =
                    self.process_exit_observed || settled_as_exited(self.process.as_ref());
                self.invalidation = Some(if self.process_exit_observed {
                    SessionInvalidation {
                        reason: "premature_exit",
                        detail: "the worker exited before the close handshake".into(),
                    }
                } else {
                    SessionInvalidation {
                        reason: "close_send_failed",
                        detail: "the close message could not be delivered".into(),
                    }
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
                let report: Option<Value> =
                    crate::worker_module_audit::parse_report_prefix(&result.stdout).ok();
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
            _ => (
                json!({ "collection_error": "worker was never collected" }),
                None,
            ),
        };
        // A worker can accept the close write and then crash before producing
        // its terminal report.  The pre-send liveness checks cannot classify
        // that ordering, but leaving it as `invalidated=false` makes the close
        // summary contradict the collected OS outcome.  Preserve a distinct
        // reason so callers can separate this from a pre-handshake exit and
        // continue to gate any recovery on the exact crash/report evidence.
        if self.invalidation.is_none()
            && final_report.is_none()
            && matches!(
                &collected,
                Some(CollectedExit {
                    result: Some(result),
                    ..
                }) if result.classification == crate::ExitClassification::Crashed
            )
        {
            self.invalidation = Some(SessionInvalidation {
                reason: "worker_exited_during_close",
                detail: "the worker crashed after the close request but before its final report"
                    .into(),
            });
        }
        // Cluster sessions check the final report's module audit against the
        // launch manifest's declared set (design §5), replacing the one-shot
        // fixed-cap validator the cluster dispatch disabled at launch. Since
        // issue #730 the outcome is recorded on the close report instead of
        // invalidating the session: the module list explains observations
        // (a runtime DLL difference can change pixels), it does not decide
        // whether the frames were valid.
        let module_audit_warning = match (&self.cluster, &collected) {
            (
                Some(cluster),
                Some(CollectedExit {
                    result: Some(result),
                    ..
                }),
            ) if result.classification == crate::ExitClassification::Ok => {
                let _ = cluster;
                crate::worker_module_audit::observe_in_place_cluster_audit(
                    &result.stdout,
                    result.stdout_truncated,
                )
            }
            _ => None,
        };
        let session_clean = self.invalidation.is_none()
            && matches!(
                &collected,
                Some(CollectedExit { result: Some(result), .. })
                    if result.classification == crate::ExitClassification::Ok
            )
            && final_report
                .as_ref()
                .is_some_and(|report| validate_final_report(report, self.smart).is_ok());
        json!({
            "stage": "render_session_close",
            "render_path": if self.smart { "smart" } else { "classic" },
            "plugin_sha256": self.plugin_sha256,
            "pixel_format": self.geometry.pixel_format.report_name(),
            "width": self.geometry.width,
            "height": self.geometry.height,
            "frames_ok": self.frames_ok,
            "frames_errored": self.frames_errored,
            "smart_output_untouched_frames": self.smart_output_untouched_frames,
            "parameter_update_frames": self.parameter_update_frames,
            "invalidated": self.invalidation.is_some(),
            "invalidated_reason": self.invalidation.as_ref().map(|invalidation| json!({
                "reason": invalidation.reason,
                "detail": invalidation.detail,
            })),
            "module_audit_warning": module_audit_warning,
            "worker": worker,
            "final_report": final_report,
            "session_clean": session_clean,
        })
    }
}

/// The validated source of truth for a render session's close report.  A
/// non-owned global suite lease is observable but not a host ownership fault;
/// callers may retain the rendered image only for that explicitly proven case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalReportValidation {
    Clean,
    CleanWithSuiteLeaseWarning {
        suite_acquires: u64,
        suite_releases: u64,
        live_suite_lease_count: u64,
    },
}

/// Bounded names for a rejected close.  They intentionally carry no worker
/// strings or paths, so the wrapper can expose useful failure evidence without
/// turning a worker-controlled report into an unbounded diagnostic channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseReportInvariant {
    CloseInvalidated,
    WorkerNotOk,
    FinalReportMissing,
    Status,
    GlobalSetdownError,
    GuardBytes,
    SuiteLeaseWarningMetadata,
    MissingSuiteFaultEvidence,
    SuiteFaultObserved,
    SuiteLeaseCounts,
    SuiteLeaseList,
    UnexpectedLiveSuiteLease,
    HandleLifetimes,
    WorldLifetimes,
    ParameterCheckouts,
    ClassicRenderError,
    ClassicSequenceSetup,
    ClassicSequenceSetdown,
    SmartSessionMode,
    SmartRenderError,
    SmartSequenceSetup,
    SmartSequenceSetdown,
}

impl CloseReportInvariant {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CloseInvalidated => "close_invalidated",
            Self::WorkerNotOk => "worker_not_ok",
            Self::FinalReportMissing => "final_report_missing",
            Self::Status => "status",
            Self::GlobalSetdownError => "global_setdown_error",
            Self::GuardBytes => "guard_bytes_intact",
            Self::SuiteLeaseWarningMetadata => "suite_lease_warning_metadata",
            Self::MissingSuiteFaultEvidence => "missing_suite_fault_evidence",
            Self::SuiteFaultObserved => "suite_fault_observed",
            Self::SuiteLeaseCounts => "suite_lease_counts",
            Self::SuiteLeaseList => "live_suite_leases",
            Self::UnexpectedLiveSuiteLease => "unexpected_live_suite_lease",
            Self::HandleLifetimes => "handle_lifetimes_balanced",
            Self::WorldLifetimes => "world_lifetimes_balanced",
            Self::ParameterCheckouts => "param_checkouts_balanced",
            Self::ClassicRenderError => "render_error",
            Self::ClassicSequenceSetup => "persistent_sequence_setup_error",
            Self::ClassicSequenceSetdown => "persistent_sequence_setdown_error",
            Self::SmartSessionMode => "session_mode",
            Self::SmartRenderError => "session_render_error",
            Self::SmartSequenceSetup => "session_sequence_setup_error",
            Self::SmartSequenceSetdown => "session_sequence_setdown_error",
        }
    }
}

/// Validates the final report used by both `RenderSession::close` and the
/// length-one classic wrapper.  Missing fields and an unproven lease warning
/// fail closed; the typed result prevents wrappers from independently
/// re-implementing (and drifting from) the suite-lease exception.
pub(crate) fn validate_final_report(
    report: &Value,
    smart: bool,
) -> Result<FinalReportValidation, CloseReportInvariant> {
    validate_final_report_mode(report, smart, FinalReportMode::Normal)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalReportMode {
    Normal,
    AbandonedSmartOutputUntouched,
}

fn validate_final_report_mode(
    report: &Value,
    smart: bool,
    mode: FinalReportMode,
) -> Result<FinalReportValidation, CloseReportInvariant> {
    if report.get("status") != Some(&json!("render_completed")) {
        return Err(CloseReportInvariant::Status);
    }
    if report.get("global_setdown_error") != Some(&json!(0)) {
        return Err(CloseReportInvariant::GlobalSetdownError);
    }
    if report.get("guard_bytes_intact") != Some(&Value::Bool(true)) {
        return Err(CloseReportInvariant::GuardBytes);
    }
    let lease_validation = validate_suite_lease_state(report)?;
    if report.get("handle_lifetimes_balanced") != Some(&Value::Bool(true)) {
        return Err(CloseReportInvariant::HandleLifetimes);
    }
    if report.get("world_lifetimes_balanced") != Some(&Value::Bool(true)) {
        return Err(CloseReportInvariant::WorldLifetimes);
    }
    if report.get("param_checkouts_balanced") != Some(&Value::Bool(true)) {
        return Err(CloseReportInvariant::ParameterCheckouts);
    }
    if smart {
        if report.get("session_mode") != Some(&Value::Bool(true)) {
            return Err(CloseReportInvariant::SmartSessionMode);
        }
        if report.get("session_render_error") != Some(&json!(0)) {
            return Err(CloseReportInvariant::SmartRenderError);
        }
        if mode == FinalReportMode::AbandonedSmartOutputUntouched
            && (report.get("pre_render_error") != Some(&json!(0))
                || report.get("smart_render_selector_error") != Some(&json!(0))
                || report.get("smart_render_error") != Some(&json!(-6))
                || report.get("output_pixels_valid") != Some(&Value::Bool(false))
                || report.get("empty_result_rect") != Some(&Value::Bool(false))
                || report.get("result_rects_valid") != Some(&Value::Bool(true)))
        {
            return Err(CloseReportInvariant::SmartRenderError);
        }
        if report.get("session_sequence_setup_error") != Some(&json!(0)) {
            return Err(CloseReportInvariant::SmartSequenceSetup);
        }
        if report.get("session_sequence_setdown_error") != Some(&json!(0)) {
            return Err(CloseReportInvariant::SmartSequenceSetdown);
        }
        // The typed lease validator above is render-path independent: an
        // explicit, non-faulting, count-consistent residual lease is contained
        // by worker exit on SmartFX just as it is on the classic length-one
        // path. All malformed or faulting evidence still fails closed there.
    } else {
        if report.get("render_error") != Some(&json!(0)) {
            return Err(CloseReportInvariant::ClassicRenderError);
        }
        if report.get("persistent_sequence_setup_error") != Some(&json!(0)) {
            return Err(CloseReportInvariant::ClassicSequenceSetup);
        }
        if report.get("persistent_sequence_setdown_error") != Some(&json!(0)) {
            return Err(CloseReportInvariant::ClassicSequenceSetdown);
        }
    }
    Ok(lease_validation)
}

fn validate_suite_lease_state(
    report: &Value,
) -> Result<FinalReportValidation, CloseReportInvariant> {
    let warning = report.get("suite_lease_warning").and_then(Value::as_bool);
    let fault_observed = report.get("suite_fault_observed").and_then(Value::as_bool);
    let live_count = report.get("live_suite_lease_count").and_then(Value::as_u64);
    let leases = report.get("live_suite_leases").and_then(Value::as_str);
    match report.get("suite_leases_balanced") {
        Some(Value::Bool(true)) => {
            if warning == Some(true)
                || fault_observed == Some(true)
                || live_count.is_some_and(|count| count != 0)
            {
                return Err(CloseReportInvariant::UnexpectedLiveSuiteLease);
            }
            if leases.is_some_and(|leases| !leases.is_empty()) {
                return Err(CloseReportInvariant::UnexpectedLiveSuiteLease);
            }
            Ok(FinalReportValidation::Clean)
        }
        Some(Value::Bool(false)) => {
            if warning != Some(true) {
                return Err(CloseReportInvariant::SuiteLeaseWarningMetadata);
            }
            match fault_observed {
                Some(false) => {}
                Some(true) => return Err(CloseReportInvariant::SuiteFaultObserved),
                None => return Err(CloseReportInvariant::MissingSuiteFaultEvidence),
            }
            let acquires = report.get("suite_acquires").and_then(Value::as_u64);
            let releases = report.get("suite_releases").and_then(Value::as_u64);
            let live_suite_reference_count = report
                .get("live_suite_reference_count")
                .and_then(Value::as_u64);
            let (
                Some(acquires),
                Some(releases),
                Some(live_suite_lease_count),
                Some(live_suite_reference_count),
                Some(leases),
            ) = (
                acquires,
                releases,
                live_count,
                live_suite_reference_count,
                leases,
            )
            else {
                return Err(CloseReportInvariant::SuiteLeaseWarningMetadata);
            };
            let Some(residual_suite_references) = acquires.checked_sub(releases) else {
                return Err(CloseReportInvariant::SuiteLeaseCounts);
            };
            let (parsed_entry_count, parsed_reference_count) =
                parse_canonical_live_suite_leases(leases)?;
            if live_suite_lease_count == 0
                || parsed_entry_count != live_suite_lease_count
                || parsed_reference_count != live_suite_reference_count
                || parsed_reference_count != residual_suite_references
            {
                return Err(CloseReportInvariant::SuiteLeaseCounts);
            }
            Ok(FinalReportValidation::CleanWithSuiteLeaseWarning {
                suite_acquires: acquires,
                suite_releases: releases,
                live_suite_lease_count,
            })
        }
        _ => Err(CloseReportInvariant::SuiteLeaseWarningMetadata),
    }
}

/// Parse the only accepted representation of `SuiteLeaseTracker::live_summary`:
/// `name@version=count;...`.  The producer writes map keys and integer values
/// directly, so delimiters, whitespace padding, duplicate normalized keys and
/// non-canonical decimal spellings are never emitted by a valid worker report.
/// Keep this parser private to the close validator: callers must not turn the
/// worker-controlled summary into a diagnostic payload.
fn parse_canonical_live_suite_leases(summary: &str) -> Result<(u64, u64), CloseReportInvariant> {
    if summary.is_empty() || summary.len() > 512 || !summary.is_ascii() {
        return Err(CloseReportInvariant::SuiteLeaseList);
    }

    let mut entry_count = 0u64;
    let mut reference_count = 0u64;
    let mut normalized_keys = std::collections::BTreeSet::new();
    for entry in summary.split(';') {
        let Some((key, count_text)) = entry.split_once('=') else {
            return Err(CloseReportInvariant::SuiteLeaseList);
        };
        if entry.is_empty() || count_text.contains('=') {
            return Err(CloseReportInvariant::SuiteLeaseList);
        }
        let Some((name, version_text)) = key.split_once('@') else {
            return Err(CloseReportInvariant::SuiteLeaseList);
        };
        if key.matches('@').count() != 1
            || !canonical_suite_name(name)
            || canonical_i32(version_text).is_none()
        {
            return Err(CloseReportInvariant::SuiteLeaseList);
        }
        let Some(count) = canonical_u64(count_text) else {
            return Err(CloseReportInvariant::SuiteLeaseList);
        };
        if count == 0 {
            return Err(CloseReportInvariant::SuiteLeaseList);
        }
        let normalized_key = format!("{}@{}", name.to_ascii_lowercase(), version_text);
        if !normalized_keys.insert(normalized_key) {
            return Err(CloseReportInvariant::SuiteLeaseList);
        }
        entry_count = entry_count
            .checked_add(1)
            .ok_or(CloseReportInvariant::SuiteLeaseCounts)?;
        reference_count = reference_count
            .checked_add(count)
            .ok_or(CloseReportInvariant::SuiteLeaseCounts)?;
    }
    Ok((entry_count, reference_count))
}

fn canonical_suite_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(' ')
        && !name.ends_with(' ')
        && !name.contains("  ")
        && name.bytes().all(|byte| {
            (byte.is_ascii_graphic() && !matches!(byte, b'@' | b'=' | b';')) || byte == b' '
        })
}

fn canonical_i32(text: &str) -> Option<i32> {
    if text.is_empty() || text == "-0" {
        return None;
    }
    let value = text.parse::<i32>().ok()?;
    (value.to_string() == text).then_some(value)
}

fn canonical_u64(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = text.parse::<u64>().ok()?;
    (value.to_string() == text).then_some(value)
}

/// Validates a serialized close summary for consumers that did not retain the
/// `RenderSession` object.  This deliberately does not trust the convenience
/// `session_clean` bit: the final report plus worker/invalidation boundary is
/// the canonical verdict and preserves a valid global-lifetime lease warning.
pub(crate) fn validate_close_report(
    close: &Value,
    smart: bool,
) -> Result<FinalReportValidation, CloseReportInvariant> {
    if close.get("invalidated") != Some(&Value::Bool(false)) {
        return Err(CloseReportInvariant::CloseInvalidated);
    }
    if close
        .get("worker")
        .and_then(|worker| worker.get("classification"))
        .and_then(Value::as_str)
        != Some("ok")
    {
        return Err(CloseReportInvariant::WorkerNotOk);
    }
    let report = close
        .get("final_report")
        .filter(|value| value.is_object())
        .ok_or(CloseReportInvariant::FinalReportMissing)?;
    validate_final_report(report, smart)
}

/// Validates a discarded Smart attempt whose only frame outcome was the
/// broker-authenticated untouched-output condition. No pixels from this
/// attempt are accepted; this authorization only permits one fresh Classic
/// attempt under the caller's unchanged request.
pub fn validate_abandoned_smart_untouched_close(close: &Value) -> Result<(), &'static str> {
    if close.get("invalidated") != Some(&Value::Bool(false)) {
        return Err("close_invalidated");
    }
    if close
        .pointer("/worker/classification")
        .and_then(Value::as_str)
        != Some("ok")
    {
        return Err("worker_not_ok");
    }
    if close.get("frames_errored").and_then(Value::as_u64) != Some(1)
        || close
            .get("smart_output_untouched_frames")
            .and_then(Value::as_u64)
            != Some(1)
    {
        return Err("unexpected_frame_history");
    }
    let report = close.get("final_report").ok_or("final_report_missing")?;
    validate_final_report_mode(report, true, FinalReportMode::AbandonedSmartOutputUntouched)
        .map(|_| ())
        .map_err(CloseReportInvariant::as_str)
}

/// Authorizes one fresh Classic attempt after the Smart worker was terminated
/// by Windows heap-corruption detection during Smart Render, or after its valid
/// frame reply but before it read the close request. The same exact crash after
/// close delivery is eligible only when no final report was produced. No pixels
/// or report from the crashed process are accepted. This is kept deliberately
/// narrower than a generic crash fallback: transport failures, host invariant
/// exits, other exception codes, and other close-time crashes remain terminal.
pub fn validate_abandoned_smart_heap_corruption_close(close: &Value) -> Result<(), &'static str> {
    const STATUS_HEAP_CORRUPTION: u64 = 0xC000_0374;

    if close.get("render_path").and_then(Value::as_str) != Some("smart") {
        return Err("not_smart_render");
    }
    let invalidated_reason = close
        .pointer("/invalidated_reason/reason")
        .and_then(Value::as_str);
    if close.get("invalidated") != Some(&Value::Bool(true))
        || !matches!(
            invalidated_reason,
            Some("worker_exited" | "premature_exit" | "worker_exited_during_close")
        )
    {
        return Err("not_worker_exit");
    }
    if close
        .pointer("/worker/classification")
        .and_then(Value::as_str)
        != Some("crashed")
        || close.pointer("/worker/exit_code").and_then(Value::as_u64)
            != Some(STATUS_HEAP_CORRUPTION)
    {
        return Err("not_heap_corruption");
    }
    let diagnostics = close
        .pointer("/worker/diagnostics")
        .and_then(Value::as_object)
        .ok_or("worker_diagnostics_missing")?;
    let smart_stage = |key: &str| {
        matches!(
            diagnostics.get(key).and_then(Value::as_str),
            Some("smart_render" | "smart_render_cpu")
        )
    };
    if !smart_stage("failure_stage") && !smart_stage("active_stage") {
        return Err("not_smart_render_stage");
    }
    if close.get("final_report") != Some(&Value::Null) {
        return Err("unexpected_final_report");
    }
    if close.get("session_clean") != Some(&Value::Bool(false))
        || close.get("frames_ok").and_then(Value::as_u64).is_none()
        || close.get("frames_errored").and_then(Value::as_u64) != Some(0)
    {
        return Err("unexpected_crash_history");
    }
    Ok(())
}

/// Public close gate for a completed session whose pixels may be published.
/// It reuses the canonical report validator rather than trusting the summary
/// `session_clean` convenience bit.
pub fn validate_completed_session_close(close: &Value, smart: bool) -> Result<(), &'static str> {
    validate_close_report(close, smart)
        .map(|_| ())
        .map_err(CloseReportInvariant::as_str)
}

#[cfg(test)]
fn final_report_clean(report: &Value, smart: bool) -> bool {
    validate_final_report(report, smart).is_ok()
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
    /// Absolute path to an external aux channel manifest
    /// (`--aux-manifest-v1`).
    #[serde(default)]
    aux_manifest: Option<String>,
    /// Absolute path to an existing directory receiving world snapshots
    /// (`--dump-worlds-v1`).
    #[serde(default)]
    world_dump_dir: Option<String>,
    /// Enables the worker's per-output checksum detail records.
    #[serde(default)]
    output_checksum_detail: bool,
    /// Alpha-as-coverage parameter slots (`--alpha-as-coverage-v1`), issue #98
    /// W1-4c. Published once at launch and read on every frame.
    #[serde(default)]
    alpha_as_coverage_params: Vec<u32>,
    /// Runs the batch through a SmartFX session (protocol v1.1).
    #[serde(default)]
    smart: bool,
    /// GPU backend for smart ARGB32f batches. A legacy runtime module policy
    /// is not a prerequisite for GPU negotiation.
    #[serde(default)]
    gpu_backend: RenderGpuBackend,
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
    // Per-launch environment for the session this batch opens (issue #910).
    // The CLI passes the default; a test drives the fixture worker through it
    // without touching the broker process environment.
    launch_environment: &crate::secure_launch::LaunchEnvironment,
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
    let plugin_directory = plugin_path
        .parent()
        .ok_or_else(|| invalid("batch plugin path has no parent directory"))?
        .to_path_buf();
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
        payload_override: None,
        parameter_animation: (!request.parameter_animation.is_empty())
            .then_some(request.parameter_animation.as_slice()),
        aux_manifest: request.aux_manifest.as_deref().map(Path::new),
        world_dump_dir: request.world_dump_dir.as_deref().map(Path::new),
        output_checksum_detail: request.output_checksum_detail,
        mask_trailer: None,
        spatial_trailer: None,
        render_environment_trailer: None,
        audio_trailer: None,
        alpha_as_coverage_params: &request.alpha_as_coverage_params,
        // The video-batch entry does not apply conformance render settings.
        conformance_render_settings: None,
        layers: &[],
        dependencies: Vec::new(),
        companions: Vec::new(),
        dependency_search_dirs: vec![plugin_directory],
        width,
        height,
        pixel_format: request.pixel_format,
        time_step: request.time_step,
        total_time,
        time_scale: request.time_scale,
        frame_deadline,
        smart: request.smart,
        gpu_backend: request.gpu_backend,
        gpu_runtime_policy: None,
        launch_environment: launch_environment.clone(),
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
                FrameStatus::Rendered {
                    pixels,
                    width: frame_width,
                    height: frame_height,
                    ..
                } if frame_width == 0 && frame_height == 0 => {
                    // A legally empty SmartFX result (#278): the frame rendered
                    // no pixels, so there is no PNG or raw sidecar to write (a 0x0
                    // image cannot be represented). Report a legal empty frame,
                    // mirroring the one-shot empty-result contract, instead of
                    // aborting the batch on a zero-dimension image.
                    debug_assert!(pixels.is_empty());
                    // The non-empty arm refuses to overwrite an existing frame so
                    // a reused output directory cannot mix runs. Enforce the same
                    // fresh-output contract here: a stale frame-*.png (or its raw
                    // sidecar) left from a previous run at this index would keep
                    // old pixels on disk while the report says output_png: null,
                    // so a directory glob would ingest the wrong frame. Reject it.
                    let output_png = output_directory.join(format!("frame-{frame_index:06}.png"));
                    if output_png.exists() {
                        return Err(invalid("output frame already exists"));
                    }
                    if let Some(extension) = raw_extension {
                        if output_png.with_extension(extension).exists() {
                            return Err(invalid("output frame raw sidecar already exists"));
                        }
                    }
                    Ok(json!({
                        "frame_index": frame_index,
                        "status": "ok",
                        // A batch render writes frames to disk, so its
                        // manifest keeps a content hash for consumers that
                        // compare or de-duplicate them. It is computed once
                        // here from bytes the broker already holds, not on the
                        // interactive hot path (issue #690).
                        "checksum": format!("{:x}", Sha256::digest(&pixels)),
                        "width": 0,
                        "height": 0,
                        "empty_result": true,
                        "output_png": Value::Null,
                    }))
                }
                FrameStatus::Rendered {
                    pixels,
                    width: frame_width,
                    height: frame_height,
                    ..
                } => {
                    let output_png = output_directory.join(format!("frame-{frame_index:06}.png"));
                    let preview = native_rgba_to_preview(&pixels, request.pixel_format)?;
                    // Use the frame's actual (possibly shrunk) dimensions (#261).
                    let image = image::RgbaImage::from_raw(frame_width, frame_height, preview)
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
                        // A batch render writes frames to disk, so its
                        // manifest keeps a content hash for consumers that
                        // compare or de-duplicate them. It is computed once
                        // here from bytes the broker already holds, not on the
                        // interactive hot path (issue #690).
                        "checksum": format!("{:x}", Sha256::digest(&pixels)),
                        // The frame's actual (possibly shrunk) dimensions, so a
                        // consumer reading the raw sidecar interprets it with the
                        // right geometry instead of the input dimensions (#261).
                        "width": frame_width,
                        "height": frame_height,
                        "output_png": output_png.file_name().and_then(|name| name.to_str()),
                    }))
                }
                FrameStatus::FrameError {
                    render_error,
                    missing_dependency,
                    return_message,
                } => Ok(json!({
                    "frame_index": frame_index,
                    "status": "error",
                    "render_error": render_error,
                    "missing_dependency": missing_dependency,
                    "return_message": return_message,
                })),
                FrameStatus::SmartOutputUntouched => Ok(json!({
                    "frame_index": frame_index,
                    "status": "error",
                    "render_error": -6,
                    "host_failure_reason": "smart_output_untouched",
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
        "render_path": if request.smart { "smart" } else { "classic" },
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

mod audio;
mod discovery;

pub use audio::{AudioRenderSession, AudioSessionOpenRequest, AudioSpanOutcome, AudioSpanStatus};
pub use discovery::{
    CleanupCrashAuthorization, DiscoverySession, InPlaceDiscoverySessionOpenRequest, InspectOutcome,
};

#[cfg(test)]
mod tests;
