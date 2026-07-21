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
    runtime_backend, validate_animation_bindings, GpuRuntimePolicyInput, InteractiveParameter,
    ParameterAnimation, RenderGpuBackend, RenderPixelFormat, RenderUiAction,
    INTERACTIVE_RENDER_TIMEOUT_MS,
    MAX_DIMENSION, MAX_PIXELS, MAX_RGBA_TRANSPORT_BYTES,
};
use crate::runtime_module_policy::{authenticate_gpu_worker_report, WorkerModuleValidation};
use crate::secure_image_dispatch::{
    dispatch_secure_gpu_image_session, dispatch_secure_image_session, ApprovedImageArtifact,
    GpuRuntimeAuthorization, SecureImageDispatch, WorkerKind,
};
use crate::secure_launch::{SecureLaunchResult, SecureSessionProcess};
use crate::windows_process::SessionChildHandles;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
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
/// frame-local diagnostics. Time-scale (-40) and time-range (-46) rejections
/// stay frame-local by the worker's contract.
fn is_fatal_session_error(render_error: i64) -> bool {
    matches!(render_error, -47 | -45..=-41)
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
/// one-shot `--smart-image32[-cpu|-opencl|-directx]` family; every other
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
    /// Alpha-as-coverage parameter slots (`--alpha-as-coverage-v1`), issue #98
    /// W1-4c. The worker publishes the alpha-coverage provider once at launch
    /// (a global the classic render runtime reads on every frame), matching the
    /// session's set-once lifetime, so only the slot list travels. Empty means
    /// the option is not emitted.
    pub alpha_as_coverage_params: &'a [u32],
    /// Secondary layers, static for the whole session (issue #98 W1-4). The
    /// pixels ride the shared layer slots; the slot/geometry metadata rides
    /// the `session-layers:v1|` launch trailer. Borrowed so open copies the
    /// bytes straight into the section without a second heap copy.
    pub layers: &'a [SessionLayer],
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
    /// Selects the SmartFX resident session (protocol v1.1): the smart worker
    /// runs PreRender→SmartRender per frame under the hoisted sequence.
    pub smart: bool,
    /// GPU backend for smart ARGB32f sessions, carried in the command word
    /// like the one-shot smart dispatch. `Auto` without a runtime policy
    /// degrades to the CPU command (a session cannot retry mid-flight, so the
    /// one-shot's preflight fallback happens at open instead); an explicit
    /// GPU backend without a policy fails closed.
    pub gpu_backend: RenderGpuBackend,
    /// Session-bound authenticated runtime module policy inputs, required for
    /// every GPU-backed launch, exactly like the one-shot GPU path.
    pub gpu_runtime_policy: Option<GpuRuntimePolicyInput<'a>>,
}

/// A secondary layer whose RGBA8 pixels occupy one shared layer slot for the
/// whole session. Width/height are the layer's own geometry, bounded by the
/// input slot. A timed layer (issue #98 W1-4b) additionally carries the frame
/// time at which the worker admits it; the worker selects the matching timed
/// entry per frame with the same rational-time test the one-shot path uses.
#[derive(Clone)]
pub struct SessionLayer {
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// `Some((time, time_scale))` marks this a timed layer; `None` is a static
    /// secondary that renders on every frame.
    pub timed: Option<(i32, u32)>,
}

#[derive(Debug)]
pub enum FrameStatus {
    /// The frame rendered and every per-frame invariant held. `pixels` are
    /// the validated native RGBA bytes copied out of the output slot;
    /// `checksum` is their lowercase SHA-256 (matching the worker's).
    Rendered {
        pixels: Vec<u8>,
        checksum: String,
        /// The frame's actual rendered dimensions. Equal to the session
        /// dimensions for a fixed-size effect, smaller for a shrink-output
        /// effect (#261); `pixels` is packed at exactly `width*height*bpp`.
        width: u32,
        height: u32,
    },
    /// A frame-local compatibility diagnostic (selector error, time scale
    /// mismatch). The session stays usable; continuing is the caller's call.
    FrameError { render_error: i64 },
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
    parameter_update_frames: u32,
    opened: Instant,
    plugin_sha256: String,
    /// SmartFX session (protocol v1.1); selects the smart worker's final
    /// report contract when validating a clean close.
    smart: bool,
    /// Keeps the animation sidecar alive for the whole session; the worker
    /// reads it once at launch, but leaving transport files behind on drop
    /// would leak into target/image-transport.
    _animation_sidecar: Option<AnimationSidecar>,
    /// Keeps the per-layer transport files (#268) alive for the whole session
    /// and removes them on drop; the worker reads each layer once at open via
    /// its inherited handle.
    _layer_sidecars: LayerSidecars,
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
            if layer.slot == 0
                || layer.slot > 1024
                || layer.width == 0
                || layer.height == 0
                || layer.width > MAX_DIMENSION
                || layer.height > MAX_DIMENSION
                || u64::from(layer.width) * u64::from(layer.height) > MAX_PIXELS
            {
                return Err(invalid("render session layer slot or dimensions are invalid"));
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
                return Err(invalid("render session layer pixels do not match dimensions"));
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
        if !request.smart && request.gpu_runtime_policy.is_some() {
            return Err(invalid(
                "a runtime module policy only applies to SmartFX GPU sessions",
            ));
        }
        // v1.1 smart sessions carry no layer slots and no static context
        // trailers; the worker's smart session contract is the bare 10-slot
        // argv, so reject the combination here instead of as an opaque
        // worker command rejection.
        if request.smart
            && (!request.layers.is_empty()
                || request.mask_trailer.is_some()
                || request.spatial_trailer.is_some()
                || request.render_environment_trailer.is_some())
        {
            return Err(invalid(
                "smart sessions do not carry layers or static context trailers yet",
            ));
        }
        // A session cannot retry mid-flight, so the one-shot's Auto GPU
        // preflight fallback collapses to open time: Auto without a policy is
        // a CPU session, Auto with a policy is a CUDA session with no CPU
        // retry, and an explicit GPU backend without a policy fails closed
        // here before any transport work.
        let gpu_capable = request.smart && request.pixel_format == RenderPixelFormat::Argb32f;
        let effective_backend = if gpu_capable
            && request.gpu_backend == RenderGpuBackend::Auto
            && request.gpu_runtime_policy.is_none()
        {
            RenderGpuBackend::Cpu
        } else {
            request.gpu_backend
        };
        let gpu_attempt = gpu_capable && runtime_backend(effective_backend).is_some();
        if gpu_attempt && request.gpu_runtime_policy.is_none() {
            return Err(invalid(
                "GPU render requires a session-bound authenticated runtime module policy report; supply gpu_runtime_policy or select the CPU backend",
            ));
        }
        let command = session_command(request.pixel_format, request.smart, effective_backend)?;
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
        // once at open into a private vector and never re-opens a path for
        // transport (issue #18 TOCTOU lesson). Because the pixels leave the
        // bounded section, layer count/size no longer feed the aggregate section
        // cap, so the one-shot per-file layered path has no capability the session
        // lacks. `layer_files` keeps the broker's inheritable read handles alive
        // through the spawn (dropped at the end of open once the worker inherited
        // its own copies); `layer_sidecars` deletes the files when the session
        // ends. Built incrementally so an early return still cleans up.
        let mut layer_sidecars = LayerSidecars(Vec::with_capacity(request.layers.len()));
        let mut layer_files: Vec<std::fs::File> = Vec::with_capacity(request.layers.len());
        let mut layer_handles: Vec<HANDLE> = Vec::with_capacity(request.layers.len());
        if !request.layers.is_empty() {
            let root = request.repository.join("target/image-transport");
            fs::create_dir_all(&root)?;
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| invalid(error.to_string()))?
                .as_nanos();
            for (index, layer) in request.layers.iter().enumerate() {
                let path = root.join(format!("layer-session-{nonce}-{index}.rgba"));
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?
                    .write_all(&layer.rgba)?;
                layer_sidecars.0.push(path.clone());
                let file = OpenOptions::new().read(true).open(&path)?;
                let handle = file.as_raw_handle() as HANDLE;
                if unsafe {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                layer_handles.push(handle);
                layer_files.push(file);
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
                match layer.timed {
                    Some((time, time_scale)) => encoded.push_str(&format!(
                        "{},{},{},{},{},{}",
                        layer.slot, layer.width, layer.height, time, time_scale, handle
                    )),
                    None => encoded.push_str(&format!(
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
        if let Some(sidecar) = &animation_sidecar {
            args_after_plugin.extend([
                "--parameter-animation-v1".to_owned(),
                sidecar.0.to_string_lossy().into_owned(),
            ]);
        }
        // The session always launches at the render dimensions; an expand grows
        // the output slot in place mid-session (#262), so there is no launch-time
        // output-capacity trailer.
        let dispatch = SecureImageDispatch {
            repository: request.repository,
            worker_kind: if request.smart {
                WorkerKind::Smart
            } else {
                WorkerKind::Render
            },
            plugin,
            dependencies: request.dependencies,
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            timeout: request.frame_deadline,
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
        let process = if gpu_attempt {
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
            dispatch_secure_gpu_image_session(
                dispatch,
                GpuRuntimeAuthorization {
                    backend,
                    session_identity: policy_input.session_identity,
                    module_report: &report,
                },
                &child_handles,
            )?
        } else {
            dispatch_secure_image_session(dispatch, &child_handles)?
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
            parameter_update_frames: 0,
            opened: Instant::now(),
            plugin_sha256: request.plugin_sha256.to_ascii_lowercase(),
            smart: request.smart,
            _animation_sidecar: animation_sidecar,
            _layer_sidecars: layer_sidecars,
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
                object.insert("ui_action".into(), Value::String(ui_action.encode_ui_field()?));
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
        // The top-level width/height fields belong only to a resize_needed
        // response; deny_unknown_fields treats them as known for every status,
        // so reject them here on ok/error to keep the frame_done schema strict.
        let carries_resize_fields = done.width.is_some() || done.height.is_some();
        match done.status.as_str() {
            "error" => {
                if done.output.is_some()
                    || done.generation.is_some()
                    || done.render_error == 0
                    || carries_resize_fields
                {
                    return Err(self.invalidate(
                        "malformed_error_response",
                        format!("frame {frame_index} error response carried output/resize fields"),
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
                return Ok(FrameOutcome {
                    frame_index,
                    status: FrameStatus::FrameError {
                        render_error: done.render_error,
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
                if carries_resize_fields {
                    return Err(self.invalidate(
                        "malformed_ok_response",
                        format!("frame {frame_index} ok response carried resize fields"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                if let Err(detail) = self.validate_ok_frame(expected_generation, &output, generation, done.render_error)
                {
                    return Err(self.invalidate(
                        "frame_invariant_failure",
                        format!("frame {frame_index}: {detail}"),
                        true,
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                // Read only the frame's actual packed bytes, not the whole
                // launch slot: a shrink-output effect fills less than the slot,
                // and the worker's checksum covers those actual bytes (#261).
                let actual_bytes = output.width as usize
                    * output.height as usize
                    * self.geometry.pixel_format.bytes_per_pixel() as usize;
                let pixels = self
                    .transport
                    .read_output_slot(self.geometry.output_slot_offset(), actual_bytes);
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
                return Ok(FrameOutcome {
                    frame_index,
                    status: FrameStatus::Rendered {
                        pixels,
                        checksum,
                        width: output.width,
                        height: output.height,
                    },
                });
            }
            "resize_needed" => {
                // The effect rendered larger than the launch slot; the worker
                // wrote nothing and left the generation untouched (#261). It
                // carries only width/height. Bound the requested size so a
                // misbehaving worker cannot force an unbounded re-open, and
                // require it to actually exceed the current slot.
                if done.output.is_some() || done.generation.is_some() || done.render_error != 0 {
                    return Err(self.invalidate(
                        "malformed_resize_response",
                        format!("frame {frame_index} resize_needed carried output/generation/error"),
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
                        format!("frame {frame_index} resize response advanced the output generation"),
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
            let address = MEMORY_MAPPED_VIEW_ADDRESS { Value: view as *mut _ };
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
        // A resize-output effect may render at any positive dimensions that
        // still fit the launch output slot (#261): shrink, or an expand small
        // enough to fit. An expand that overruns the slot arrives as a
        // "resize_needed" status instead, never here.
        let bpp = self.geometry.pixel_format.bytes_per_pixel();
        if output.width == 0 || output.height == 0 {
            return Err(format!("output geometry {}x{} is empty", output.width, output.height));
        }
        // A resized output (dimensions other than the render dimensions) must
        // obey the same per-dimension and total-pixel caps as the worker's
        // validate_output_extent, so a buggy or compromised worker cannot report
        // an absurd shape (e.g. 1000000x1) that happens to fit a large slot by
        // total bytes. A fixed-size output was already bounded at session open.
        let is_resize = output.width != self.geometry.width || output.height != self.geometry.height;
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
            && final_report
                .as_ref()
                .is_some_and(|report| final_report_clean(report, self.smart));
        json!({
            "stage": "render_session_close",
            "render_path": if self.smart { "smart" } else { "classic" },
            "plugin_sha256": self.plugin_sha256,
            "pixel_format": self.geometry.pixel_format.report_name(),
            "width": self.geometry.width,
            "height": self.geometry.height,
            "frames_ok": self.frames_ok,
            "frames_errored": self.frames_errored,
            "parameter_update_frames": self.parameter_update_frames,
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
/// exit code: the hoisted sequence must have set up and torn down without
/// error, guards must be intact, and every ownership ledger must balance.
/// Missing keys fail closed. The classic and smart workers report session
/// mechanics under different keys (the classic report reuses its
/// persistent-sequence fields; the smart report carries dedicated session_*
/// fields, protocol v1.1).
fn final_report_clean(report: &Value, smart: bool) -> bool {
    let shared = report.get("status") == Some(&json!("render_completed"))
        && report.get("global_setdown_error") == Some(&json!(0))
        && report.get("guard_bytes_intact") == Some(&Value::Bool(true))
        && report.get("suite_leases_balanced") == Some(&Value::Bool(true))
        && report.get("handle_lifetimes_balanced") == Some(&Value::Bool(true))
        && report.get("world_lifetimes_balanced") == Some(&Value::Bool(true))
        && report.get("param_checkouts_balanced") == Some(&Value::Bool(true));
    if smart {
        shared
            && report.get("session_mode") == Some(&Value::Bool(true))
            && report.get("session_render_error") == Some(&json!(0))
            && report.get("session_sequence_setup_error") == Some(&json!(0))
            && report.get("session_sequence_setdown_error") == Some(&json!(0))
    } else {
        shared
            && report.get("render_error") == Some(&json!(0))
            && report.get("persistent_sequence_setup_error") == Some(&json!(0))
            && report.get("persistent_sequence_setdown_error") == Some(&json!(0))
    }
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
    /// GPU backend for smart ARGB32f batches. The batch CLI carries no
    /// runtime module policy, so `auto` degrades to the CPU command at open
    /// and the explicit GPU backends fail closed there.
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
        aux_manifest: request.aux_manifest.as_deref().map(Path::new),
        world_dump_dir: request.world_dump_dir.as_deref().map(Path::new),
        output_checksum_detail: request.output_checksum_detail,
        mask_trailer: None,
        spatial_trailer: None,
        render_environment_trailer: None,
        alpha_as_coverage_params: &request.alpha_as_coverage_params,
        layers: &[],
        dependencies: Vec::new(),
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
                    checksum,
                    width: frame_width,
                    height: frame_height,
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
                        "checksum": checksum,
                        // The frame's actual (possibly shrunk) dimensions, so a
                        // consumer reading the raw sidecar interprets it with the
                        // right geometry instead of the input dimensions (#261).
                        "width": frame_width,
                        "height": frame_height,
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

// ---------------------------------------------------------------------------
// Audio render session (protocol §10). Independent of the image session above;
// it reuses the same launch/transport/reader/watcher machinery (inheritable
// pipes, SessionTransport, dispatch_secure_image_session, SessionChildHandles,
// the reader thread + process-death watcher, SecureSessionProcess/CollectedExit,
// the three-way await) but carries a bulk audio span instead of image frames.
// ---------------------------------------------------------------------------

const AUDIO_HEADER_MAGIC: u32 = 0x5355_4141; // "AAUS" little-endian
const AUDIO_MAX_SAMPLES_OFFSET: usize = 8;
const AUDIO_CHANNELS_OFFSET: usize = 12;
const AUDIO_INPUT_GENERATION_OFFSET: usize = 16;
const AUDIO_OUTPUT_GENERATION_OFFSET: usize = 20;
const AUDIO_OUTPUT_SAMPLES_OFFSET: usize = 28;
// v1 audio is mono; the channel bound stays in the geometry for the extension.
const AUDIO_MAX_SAMPLES_CAP: u32 = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
struct AudioSessionGeometry {
    max_samples: u32,
    channels: u32,
}

impl AudioSessionGeometry {
    fn slot_bytes(&self) -> usize {
        self.max_samples as usize * self.channels as usize * 4
    }
    fn output_slot_offset(&self) -> usize {
        HEADER_BYTES + align_slot(self.slot_bytes())
    }
    fn section_bytes(&self) -> usize {
        self.output_slot_offset() + align_slot(self.slot_bytes())
    }
}

pub struct AudioSessionOpenRequest<'a> {
    pub repository: &'a Path,
    pub plugin_path: &'a Path,
    pub plugin_sha256: &'a str,
    pub parameters: Option<&'a [InteractiveParameter]>,
    pub dependencies: Vec<ApprovedImageArtifact>,
    pub max_samples: u32,
    pub channels: u32,
    pub time_scale: u32,
    pub frame_deadline: Duration,
}

#[derive(Debug)]
pub enum AudioSpanStatus {
    /// The span rendered and every invariant held. `samples` are the rendered
    /// f32 output bytes copied out of the output slot (LE, matching the
    /// one-shot .f32 output); `checksum` is their lowercase SHA-256.
    Rendered {
        samples: Vec<u8>,
        checksum: String,
        /// The plugin's AUDIO_SETUP output start sample; the wrapper reports it
        /// as `output_start_sample` instead of hard-coding 0 (Codex #252).
        output_start: i64,
    },
    /// A per-span compatibility diagnostic; the session stays usable.
    SpanError { render_error: i64 },
}

#[derive(Debug)]
pub struct AudioSpanOutcome {
    pub request_index: u32,
    pub status: AudioSpanStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioDoneOutput {
    start_sample: i64,
    sample_count: u32,
    rate: u32,
    channels: u32,
    sample_size: u32,
    checksum: String,
    guards_intact: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudioDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    request_index: u32,
    status: String,
    #[serde(default)]
    output: Option<AudioDoneOutput>,
    audio_render_error: i64,
    #[serde(default)]
    generation: Option<u32>,
}

pub struct AudioRenderSession {
    process: Option<SecureSessionProcess>,
    collected: Option<CollectedExit>,
    transport: SessionTransport,
    receiver: mpsc::Receiver<SessionEvent>,
    process_exit_observed: bool,
    geometry: AudioSessionGeometry,
    rate: u32,
    frame_deadline: Duration,
    invalidation: Option<SessionInvalidation>,
    last_output_generation: u32,
    requests_ok: u32,
    requests_errored: u32,
    opened: Instant,
    plugin_sha256: String,
}

impl AudioRenderSession {
    pub fn open(request: AudioSessionOpenRequest<'_>) -> io::Result<AudioRenderSession> {
        if request.time_scale == 0
            || request.time_scale > i32::MAX as u32
            || request.frame_deadline.is_zero()
        {
            return Err(invalid("audio session timing is invalid"));
        }
        if request.max_samples == 0 || request.max_samples > AUDIO_MAX_SAMPLES_CAP {
            return Err(invalid("audio session max_samples is out of range"));
        }
        if request.channels != 1 {
            return Err(invalid("audio session v1 is mono (channels must be 1)"));
        }
        let geometry = AudioSessionGeometry {
            max_samples: request.max_samples,
            channels: request.channels,
        };
        if geometry.section_bytes() as u64 > SECTION_HARD_CAP_BYTES {
            return Err(invalid("audio session section exceeds the hard cap"));
        }
        let payload = encode_interactive_payload(request.parameters.unwrap_or_default())?;

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
        transport.write_header_u32(MAGIC_OFFSET, AUDIO_HEADER_MAGIC);
        // The audio session's shared-memory layout is unchanged (no layer
        // slots), so its header version stays 1 (matching the audio worker's
        // own `kProtocolVersion`); only the classic layout bumped to 2 (#264).
        transport.write_header_u32(VERSION_OFFSET, PROTOCOL_VERSION);
        transport.write_header_u32(AUDIO_MAX_SAMPLES_OFFSET, geometry.max_samples);
        transport.write_header_u32(AUDIO_CHANNELS_OFFSET, geometry.channels);
        transport.write_header_u32(AUDIO_INPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(AUDIO_OUTPUT_GENERATION_OFFSET, 0);
        transport.write_header_u32(AUDIO_OUTPUT_SAMPLES_OFFSET, 0);

        let plugin = ApprovedImageArtifact {
            path: request.plugin_path.to_path_buf(),
            expected_sha256: decode_sha256_hex(request.plugin_sha256)?,
            expected_size: fs::metadata(request.plugin_path)?.len(),
        };
        let args_before_plugin = vec!["--render-audio-session-v1".to_owned()];
        let args_after_plugin = vec![
            request.plugin_sha256.to_ascii_lowercase(),
            payload,
            geometry.max_samples.to_string(),
            geometry.channels.to_string(),
            request.time_scale.to_string(),
        ];
        let dispatch = SecureImageDispatch {
            repository: request.repository,
            worker_kind: WorkerKind::Render,
            plugin,
            dependencies: request.dependencies,
            args_before_plugin: &args_before_plugin,
            args_after_plugin: &args_after_plugin,
            timeout: request.frame_deadline,
        };
        let child_handles = SessionChildHandles {
            request_read: request_read.raw(),
            response_write: response_write.raw(),
            section: transport.section.raw(),
            // Audio sessions carry no layers.
            layers: Vec::new(),
        };
        let process = dispatch_secure_image_session(dispatch, &child_handles)?;
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
                    return;
                }
                let length = u32::from_le_bytes(prefix) as usize;
                if length == 0 || length > MAX_MESSAGE_BYTES {
                    let _ = reader_sender.send(SessionEvent::ReaderViolation);
                    return;
                }
                let mut body = vec![0u8; length];
                if !read_exact_handle(handle, &mut body) {
                    let _ = reader_sender.send(SessionEvent::ReaderViolation);
                    return;
                }
                if reader_sender.send(SessionEvent::Message(body)).is_err() {
                    return;
                }
            }
        });
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

        Ok(AudioRenderSession {
            process: Some(process),
            collected: None,
            transport,
            receiver,
            process_exit_observed: false,
            geometry,
            rate: request.time_scale,
            frame_deadline: request.frame_deadline,
            invalidation: None,
            last_output_generation: 0,
            requests_ok: 0,
            requests_errored: 0,
            opened: Instant::now(),
            plugin_sha256: request.plugin_sha256.to_ascii_lowercase(),
        })
    }

    fn collect_exit(&mut self, wait: Duration) {
        if self.collected.is_some() {
            return;
        }
        let Some(process) = self.process.take() else {
            return;
        };
        self.collected = Some(match process.finish(wait) {
            Ok(result) => CollectedExit { result: Some(result), error: None },
            Err(error) => CollectedExit { result: None, error: Some(error.to_string()) },
        });
    }

    fn invalidate(&mut self, reason: &'static str, detail: String, wait: Duration) -> io::Error {
        if let Some(process) = self.process.as_ref() {
            let _ = process.terminate_job();
        }
        self.collect_exit(wait);
        self.invalidation = Some(SessionInvalidation { reason, detail });
        let stored = self.invalidation.as_ref().expect("just stored");
        invalid(format!("audio session invalidated ({}): {}", stored.reason, stored.detail))
    }

    fn await_response(&mut self) -> FrameWait {
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

    fn static_header_ok(&self) -> bool {
        self.transport.read_header_u32(MAGIC_OFFSET) == AUDIO_HEADER_MAGIC
            // Audio layout unchanged: header version stays 1 (see open).
            && self.transport.read_header_u32(VERSION_OFFSET) == PROTOCOL_VERSION
            && self.transport.read_header_u32(AUDIO_MAX_SAMPLES_OFFSET) == self.geometry.max_samples
            && self.transport.read_header_u32(AUDIO_CHANNELS_OFFSET) == self.geometry.channels
    }

    /// Renders one bulk audio span: `input` is the interleaved f32 samples the
    /// broker places in the input slot; the worker reads them, drives
    /// AUDIO_SETUP/RENDER/SETDOWN, and returns the rendered f32 output.
    pub fn render_span(
        &mut self,
        request_index: u32,
        input: &[f32],
    ) -> io::Result<AudioSpanOutcome> {
        if let Some(invalidation) = &self.invalidation {
            return Err(invalid(format!(
                "audio session is invalidated ({}): {}",
                invalidation.reason, invalidation.detail
            )));
        }
        loop {
            match self.receiver.try_recv() {
                Ok(SessionEvent::ProcessExited) => self.process_exit_observed = true,
                Ok(SessionEvent::ReaderViolation) => {
                    return Err(self.invalidate(
                        "response_framing_violation",
                        format!("the worker broke the response framing before {request_index}"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Ok(SessionEvent::Message(_)) => {
                    return Err(self.invalidate(
                        "unsolicited_response",
                        format!("a response arrived with no request in flight before {request_index}"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                Err(_) => break,
            }
        }
        if self.process_exit_observed {
            return Err(self.invalidate(
                "worker_exited",
                format!("the worker exited before request {request_index} was dispatched"),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let max_samples = self.geometry.max_samples as usize * self.geometry.channels as usize;
        if input.len() > max_samples {
            return Err(invalid("audio span sample count exceeds the session slot"));
        }
        let expected_generation = request_index
            .checked_add(1)
            .ok_or_else(|| invalid("request index overflows the generation counter"))?;
        if expected_generation <= self.last_output_generation {
            return Err(invalid(format!(
                "request index {request_index} does not advance the last completed generation {}",
                self.last_output_generation
            )));
        }
        let input_bytes =
            unsafe { std::slice::from_raw_parts(input.as_ptr() as *const u8, input.len() * 4) };
        self.transport.write_input_slot(input_bytes);
        self.transport
            .write_header_u32(AUDIO_INPUT_GENERATION_OFFSET, expected_generation);
        let message = format!(
            "{{\"v\":1,\"type\":\"audio_render\",\"request_index\":{request_index},\"input_samples\":{}}}",
            input.len()
        );
        if !self.transport.send_message(&message) {
            return Err(self.invalidate(
                "request_pipe_closed",
                "the session request pipe rejected an audio_render message".into(),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        let body = match self.await_response() {
            FrameWait::Message(body) => body,
            FrameWait::Deadline => {
                return Err(self.invalidate(
                    "request_deadline",
                    format!("request {request_index} exceeded the deadline"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::WorkerGone => {
                return Err(self.invalidate(
                    "worker_exited",
                    format!("the worker was gone before request {request_index} completed"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
            FrameWait::FramingViolation => {
                return Err(self.invalidate(
                    "response_framing_violation",
                    format!("the worker broke the response framing during request {request_index}"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        let done: AudioDone = match serde_json::from_slice(&body) {
            Ok(done) => done,
            Err(error) => {
                return Err(self.invalidate(
                    "malformed_audio_done",
                    format!("request {request_index} response did not parse strictly: {error}"),
                    POST_TERMINATION_COLLECT_TIMEOUT,
                ));
            }
        };
        if done.v != PROTOCOL_VERSION
            || done.kind != "audio_done"
            || done.request_index != request_index
        {
            return Err(self.invalidate(
                "audio_done_mismatch",
                format!(
                    "request {request_index} response carried v={} type={} request_index={}",
                    done.v, done.kind, done.request_index
                ),
                POST_TERMINATION_COLLECT_TIMEOUT,
            ));
        }
        match done.status.as_str() {
            "error" => {
                // A zero error code on an "error" status is a success in
                // disguise; reject it so a failed selector cannot be recorded
                // as SpanError(0) (Codex #252, mirroring the image session).
                if done.output.is_some()
                    || done.generation.is_some()
                    || done.audio_render_error == 0
                {
                    return Err(self.invalidate(
                        "malformed_error_response",
                        format!("request {request_index} error response carried output fields or zero error"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                if !self.static_header_ok()
                    || self.transport.read_header_u32(AUDIO_OUTPUT_GENERATION_OFFSET)
                        != self.last_output_generation
                {
                    return Err(self.invalidate(
                        "request_invariant_failure",
                        format!("request {request_index} error response mutated the header"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                self.requests_errored += 1;
                Ok(AudioSpanOutcome {
                    request_index,
                    status: AudioSpanStatus::SpanError {
                        render_error: done.audio_render_error,
                    },
                })
            }
            "ok" => {
                let (Some(output), Some(generation)) = (done.output, done.generation) else {
                    return Err(self.invalidate(
                        "malformed_ok_response",
                        format!("request {request_index} ok response missed output or generation"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                };
                if done.audio_render_error != 0
                    || !output.guards_intact
                    || output.channels != self.geometry.channels
                    || output.rate != self.rate
                    || output.sample_size != 4
                    || output.sample_count as usize > max_samples
                    // Bound the reported output window against the submitted
                    // input span (Codex #252): a negative start or a
                    // start+count past input.len() is an invalid range the
                    // one-shot path rejects via setup_range_valid; the session
                    // must independently reject it before publishing it.
                    || output.start_sample < 0
                    // saturating_add so a huge start_sample cannot overflow i64
                    // (debug panic / release wrap) before the range check (Codex #252).
                    || output.start_sample.saturating_add(output.sample_count as i64)
                        > input.len() as i64
                    || generation != expected_generation
                    || !self.static_header_ok()
                    || self.transport.read_header_u32(AUDIO_OUTPUT_GENERATION_OFFSET)
                        != expected_generation
                    || self.transport.read_header_u32(AUDIO_OUTPUT_SAMPLES_OFFSET)
                        != output.sample_count
                {
                    return Err(self.invalidate(
                        "request_invariant_failure",
                        format!("request {request_index} ok response failed validation"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                let samples = self.transport.read_output_slot(
                    self.geometry.output_slot_offset(),
                    output.sample_count as usize * 4,
                );
                // The worker's checksum is the sha256 of the output bytes it
                // wrote; recompute over the bytes the broker read to confirm the
                // shared slot was not disturbed mid-flight.
                let checksum = {
                    use sha2::{Digest, Sha256};
                    format!("{:x}", Sha256::digest(&samples))
                };
                if checksum != output.checksum {
                    return Err(self.invalidate(
                        "request_invariant_failure",
                        format!("request {request_index} output checksum mismatch"),
                        POST_TERMINATION_COLLECT_TIMEOUT,
                    ));
                }
                self.last_output_generation = expected_generation;
                self.requests_ok += 1;
                Ok(AudioSpanOutcome {
                    request_index,
                    status: AudioSpanStatus::Rendered {
                        samples,
                        checksum,
                        output_start: output.start_sample,
                    },
                })
            }
            other => Err(self.invalidate(
                "audio_done_status",
                format!("request {request_index} carried unknown status {other}"),
                POST_TERMINATION_COLLECT_TIMEOUT,
            )),
        }
    }

    pub fn close(mut self) -> Value {
        if self.invalidation.is_none() && self.process.is_some() {
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
                            detail: "a response arrived with no request in flight before close".into(),
                        });
                        break;
                    }
                    Err(_) => break,
                }
            }
            if self.process_exit_observed
                || self.process.as_ref().is_some_and(SecureSessionProcess::has_exited)
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
            Some(CollectedExit { result: Some(result), .. }) => {
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
            Some(CollectedExit { error: Some(error), .. }) => {
                (json!({ "collection_error": error }), None)
            }
            _ => (json!({ "collection_error": "worker was never collected" }), None),
        };
        let session_clean = self.invalidation.is_none()
            && matches!(
                &collected,
                Some(CollectedExit { result: Some(result), .. })
                    if result.classification == crate::ExitClassification::Ok
            )
            && final_report
                .as_ref()
                .is_some_and(audio_final_report_clean);
        json!({
            "stage": "audio_session_close",
            "plugin_sha256": self.plugin_sha256,
            "max_samples": self.geometry.max_samples,
            "channels": self.geometry.channels,
            "requests_ok": self.requests_ok,
            "requests_errored": self.requests_errored,
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

/// A clean audio session close requires the worker's aggregate report to agree
/// (protocol §10.3): the session completed, GLOBAL_SETDOWN was clean, no
/// protocol violation or invariant failure, and the audio ownership ledger
/// balanced. Missing keys fail closed.
fn audio_final_report_clean(report: &Value) -> bool {
    report.get("status") == Some(&json!("session_completed"))
        && report.get("global_setup_error") == Some(&json!(0))
        && report.get("params_setup_error") == Some(&json!(0))
        && report.get("global_setdown_error") == Some(&json!(0))
        && report.get("session_protocol_violation") == Some(&Value::Bool(false))
        && report.get("session_invariant_failure") == Some(&Value::Bool(false))
        && report.get("audio_lifetimes_balanced") == Some(&Value::Bool(true))
        && report.get("invalid_audio_operations") == Some(&json!(0))
        && report.get("session_clean") == Some(&Value::Bool(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_geometry_slot_layout_matches_the_protocol() {
        let geometry = SessionGeometry {
            width: 33,
            height: 17,
            output_capacity_width: 33,
            output_capacity_height: 17,
            pixel_format: RenderPixelFormat::Argb16,
            layer_slot_count: 0,
        };
        assert_eq!(geometry.input_slot_bytes(), 33 * 17 * 4); // 2244
        assert_eq!(geometry.output_slot_bytes(), 33 * 17 * 8); // 4488
        // Slots are 4096-aligned after the one-page header (protocol §6).
        assert_eq!(geometry.output_slot_offset() % SLOT_ALIGNMENT, 0);
        assert_eq!(geometry.output_slot_offset(), 4096 + 4096);
        // The section holds only header + input + output (#268); layers travel
        // as inherited file handles, so layer_slot_count no longer sizes it.
        assert_eq!(geometry.section_bytes(), 4096 + 4096 + 8192);
    }

    #[test]
    fn section_bytes_ignore_layer_count() {
        // Layer pixels left the section (#268), so a section with many layers is
        // byte-for-byte the same size as one with none: header + input + output.
        let base = SessionGeometry {
            width: 8,
            height: 8,
            output_capacity_width: 8,
            output_capacity_height: 8,
            pixel_format: RenderPixelFormat::Argb8,
            layer_slot_count: 0,
        };
        let with_layers = SessionGeometry {
            layer_slot_count: 64,
            ..base
        };
        assert_eq!(base.section_bytes(), with_layers.section_bytes());
        // header(4096) + align(8*8*4=256 -> 4096) + align(8*8*4=256 -> 4096).
        assert_eq!(base.section_bytes(), 4096 + 4096 + 4096);
    }

    #[test]
    fn worst_case_expanded_section_stays_under_the_cap() {
        // The largest representable section is a max-dimension 32-bit-float
        // render whose output expands to the resize bound: header + input
        // (4096*4096*4 = 64 MiB) + output (4096*4096*16 = 256 MiB) ~= 320 MiB,
        // far under the 1 GiB hard cap, for any layer count (#268).
        let geometry = SessionGeometry {
            width: MAX_DIMENSION,
            height: MAX_DIMENSION,
            output_capacity_width: MAX_RESIZE_DIMENSION,
            output_capacity_height: MAX_RESIZE_DIMENSION,
            pixel_format: RenderPixelFormat::Argb32f,
            layer_slot_count: 64,
        };
        assert!(geometry.section_bytes() as u64 <= SECTION_HARD_CAP_BYTES);
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
        assert!(final_report_clean(&clean, false));
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
            assert!(!final_report_clean(&report, false), "{key} must fail closed");
            let mut missing = clean.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                !final_report_clean(&missing, false),
                "missing {key} must fail closed"
            );
        }
        // A parseable but unrelated report (an older worker) is not clean.
        assert!(!final_report_clean(&serde_json::json!({"status": "ok"}), false));
    }

    #[test]
    fn smart_final_report_clean_requires_the_session_fields() {
        let clean = serde_json::json!({
            "status": "render_completed",
            "global_setdown_error": 0,
            "guard_bytes_intact": true,
            "suite_leases_balanced": true,
            "handle_lifetimes_balanced": true,
            "world_lifetimes_balanced": true,
            "param_checkouts_balanced": true,
            "session_mode": true,
            "session_render_error": 0,
            "session_sequence_setup_error": 0,
            "session_sequence_setdown_error": 0,
        });
        assert!(final_report_clean(&clean, true));
        // The classic gate must not accept a smart report and vice versa:
        // each worker's session mechanics live under different keys, and a
        // missing key fails closed.
        assert!(!final_report_clean(&clean, false));
        for (key, dirty) in [
            ("session_mode", serde_json::json!(false)),
            // A clean close after frame-local errors keeps
            // session_render_error 0; -1 means the session mechanics broke.
            ("session_render_error", serde_json::json!(-1)),
            ("session_sequence_setup_error", serde_json::json!(25)),
            ("session_sequence_setdown_error", serde_json::json!(-1)),
            // The last rendered frame's selector errors do not gate a clean
            // close, but the shared host-state keys still do.
            ("guard_bytes_intact", serde_json::json!(false)),
        ] {
            let mut report = clean.clone();
            report[key] = dirty;
            assert!(!final_report_clean(&report, true), "{key} must fail closed");
            let mut missing = clean.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                !final_report_clean(&missing, true),
                "missing {key} must fail closed"
            );
        }
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
                aux_manifest: None,
                world_dump_dir: None,
                output_checksum_detail: false,
                mask_trailer: None,
                spatial_trailer: None,
                render_environment_trailer: None,
                alpha_as_coverage_params: &[],
                layers: &[],
                dependencies: Vec::new(),
                width: 8,
                height: 4,
                pixel_format: RenderPixelFormat::Argb8,
                time_step,
                total_time,
                time_scale,
                frame_deadline: Duration::from_secs(1),
                smart: false,
                gpu_backend: RenderGpuBackend::Cpu,
                gpu_runtime_policy: None,
            });
            let Err(error) = result else {
                panic!("invalid timing must be rejected before launch");
            };
            assert_eq!(error.to_string(), "render session timing is invalid");
        }
    }

    #[test]
    fn fatal_session_error_codes_match_the_worker_contract() {
        // kSessionGenerationMismatch .. kSessionOutputValidationError, plus
        // the deferred-setup failure (-47).
        for code in [-41, -42, -43, -44, -45, -47] {
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
        for (pixel_format, expected) in [
            (RenderPixelFormat::Argb8, "--render-session-v1"),
            (RenderPixelFormat::Argb16, "--render-session16-v1"),
            (RenderPixelFormat::Argb32f, "--render-session32-v1"),
        ] {
            assert_eq!(
                session_command(pixel_format, false, RenderGpuBackend::Cpu).unwrap(),
                expected
            );
            assert_eq!(
                session_command(pixel_format, false, RenderGpuBackend::Auto).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn smart_session_commands_carry_the_gpu_backend() {
        for (backend, expected) in [
            (RenderGpuBackend::Auto, "--smart-session32-v1"),
            (RenderGpuBackend::Cuda, "--smart-session32-v1"),
            (RenderGpuBackend::OpenCl, "--smart-session32-opencl-v1"),
            (RenderGpuBackend::DirectX, "--smart-session32-directx-v1"),
            (RenderGpuBackend::Cpu, "--smart-session32-cpu-v1"),
        ] {
            assert_eq!(
                session_command(RenderPixelFormat::Argb32f, true, backend).unwrap(),
                expected
            );
        }
        assert_eq!(
            session_command(RenderPixelFormat::Argb8, true, RenderGpuBackend::Auto).unwrap(),
            "--smart-session-v1"
        );
        assert_eq!(
            session_command(RenderPixelFormat::Argb16, true, RenderGpuBackend::Cpu).unwrap(),
            "--smart-session16-v1"
        );
        // Explicit GPU backends exist only for SmartFX ARGB32f; every other
        // combination fails closed instead of silently degrading.
        for (pixel_format, smart) in [
            (RenderPixelFormat::Argb8, true),
            (RenderPixelFormat::Argb16, true),
            (RenderPixelFormat::Argb32f, false),
        ] {
            for backend in [
                RenderGpuBackend::Cuda,
                RenderGpuBackend::OpenCl,
                RenderGpuBackend::DirectX,
            ] {
                assert!(session_command(pixel_format, smart, backend).is_err());
            }
        }
    }
}
