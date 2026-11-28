use crate::guest_parameter_transport::{fixture_parameter_payload, write_control_message};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use crate::macos_worker_controller::{
    MAX_STDERR_BYTES, ResourceLimits, SecurityTier, WorkerSession, audit_process, read_bounded,
    run_staged_setup, terminate_process_group,
};
use aexcompat_broker::render_artifacts::{CapturedWorldRecord, read_captured_world};
use aexcompat_broker::render_artifacts::{
    RenderArtifactConditions, write_float32_exr_artifact, write_raw_world_artifact,
    write_raw_world_checkpoint_artifact, write_strided_world_checkpoint_artifact,
};
use aexcompat_broker::render_fixture::{
    FixtureCaseIdentity, FixtureFinalArtifact, FixturePixelFormat, FixtureTiming,
    FixtureWorldLayout, FixtureWorlds, InteractiveParameter, LoadedRenderFixture,
    expand_fixture_cases, fixture_case_identity, load_render_fixture,
};
use aexcompat_broker::render_pixel_format::RenderPixelFormat;

#[path = "macos_backend.rs"]
pub(crate) mod backend;

const MAX_WIDTH: u32 = 3840;
const MAX_HEIGHT: u32 = 2160;
const NATIVE_SETUP_DEADLINE: Duration = Duration::from_secs(2);
const RESIDENT_START_DEADLINE: Duration = Duration::from_secs(10);
const RESIDENT_RENDER_DEADLINE: Duration = Duration::from_secs(30);
const RESIDENT_CLOSE_DEADLINE: Duration = Duration::from_secs(2);
const RESIDENT_RESPONSE_POLL: Duration = Duration::from_millis(10);
const MAX_RESIDENT_RESPONSE_BYTES: usize =
    aexcompat_broker::parameter_limits::MAX_PARAMETER_DOCUMENT_BYTES;
const MAX_RESIDENT_PROTOCOL_BYTES: usize = MAX_RESIDENT_RESPONSE_BYTES * 2;

struct RenderResult {
    report: String,
    output: PathBuf,
    preview: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum MacRenderFormat {
    #[default]
    PngArgb8,
    RawArgb16,
    ExrArgb32f,
}

impl MacRenderFormat {
    fn pixel_format(self) -> &'static str {
        match self {
            Self::PngArgb8 => "argb8",
            Self::RawArgb16 => "argb16",
            Self::ExrArgb32f => "argb32f",
        }
    }

    fn bytes_per_pixel(self) -> usize {
        match self {
            Self::PngArgb8 => 4,
            Self::RawArgb16 => 8,
            Self::ExrArgb32f => 16,
        }
    }

    fn artifact_format(self) -> RenderPixelFormat {
        match self {
            Self::PngArgb8 => RenderPixelFormat::Argb8,
            Self::RawArgb16 => RenderPixelFormat::Argb16,
            Self::ExrArgb32f => RenderPixelFormat::Argb32f,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentFrameOutput {
    // Legacy workers omit rect only for the fixed, zero-origin full-frame path.
    #[serde(default)]
    rect: Option<[i32; 4]>,
    width: u32,
    height: u32,
    rowbytes: u32,
    pixel_format: String,
    render_path: String,
    checksum: String,
    guards_intact: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentFrameDone {
    v: u32,
    #[serde(rename = "type")]
    kind: String,
    frame_index: u64,
    status: String,
    output: Option<ResidentFrameOutput>,
    render_error: i32,
    generation: Option<u64>,
    #[serde(default)]
    timings_us: Option<ResidentFrameTimingsUs>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentFrameTimingsUs {
    request_prepare: u64,
    input_read: u64,
    effect_render: u64,
    output_write: u64,
    checksum: u64,
}

fn validate_resident_frame(
    value: &Value,
    frame_index: u64,
    width: u32,
    height: u32,
    format: MacRenderFormat,
) -> Result<String, String> {
    validate_resident_frame_geometry(value, frame_index, Some((width, height)), format)
        .map(|output| output.checksum)
}

fn validate_resident_frame_geometry(
    value: &Value,
    frame_index: u64,
    fixed_size: Option<(u32, u32)>,
    format: MacRenderFormat,
) -> Result<ResidentFrameOutput, String> {
    let done: ResidentFrameDone = serde_json::from_value(value.clone())
        .map_err(|error| format!("malformed resident frame response: {error}"))?;
    let expected_generation = frame_index
        .checked_add(1)
        .ok_or_else(|| "resident frame generation overflow".to_string())?;
    let Some(output) = done.output else {
        return Err(format!("resident frame has no output: {value}"));
    };
    let _profiled_micros = done.timings_us.map(|timings| {
        timings
            .request_prepare
            .saturating_add(timings.input_read)
            .saturating_add(timings.effect_render)
            .saturating_add(timings.output_write)
            .saturating_add(timings.checksum)
    });
    if done.v != 1
        || done.kind != "frame_done"
        || done.frame_index != frame_index
        || done.status != "ok"
        || done.render_error != 0
        || done.generation != Some(expected_generation)
        || output.width == 0
        || output.height == 0
        || output.width > MAX_WIDTH
        || output.height > MAX_HEIGHT
        || fixed_size.is_some_and(|size| size != (output.width, output.height))
        || (fixed_size.is_none() && output.rect.is_none())
        || output.rect.is_some_and(|rect| {
            (fixed_size.is_some() && (rect[0] != 0 || rect[1] != 0))
                || rect[0].unsigned_abs() > 4096
                || rect[1].unsigned_abs() > 4096
                || i64::from(rect[2]) - i64::from(rect[0]) != i64::from(output.width)
                || i64::from(rect[3]) - i64::from(rect[1]) != i64::from(output.height)
        })
        || output.rowbytes != output.width.saturating_mul(format.bytes_per_pixel() as u32)
        || output.pixel_format != format.pixel_format()
        || !matches!(output.render_path.as_str(), "classic" | "smartfx")
        || output.checksum.len() != 64
        || !output.checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !output.guards_intact
    {
        return Err(format!(
            "resident frame response failed invariants: {value}"
        ));
    }
    Ok(output)
}

fn validate_resident_close(value: &Value, worker_pid: u32) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "resident close response is not an object".to_string())?;
    if object.len() != 5
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("session_closed")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || !value["setup"].is_object()
    {
        return Err(format!("resident close envelope is invalid: {value}"));
    }
    let close = value["close"]
        .as_object()
        .ok_or_else(|| format!("resident close report is missing: {value}"))?;
    if close.len() != 10
        || close.get("session_clean").and_then(Value::as_bool) != Some(true)
        || close.get("frame_setdown_error").and_then(Value::as_i64) != Some(0)
        || close.get("sequence_setdown_error").and_then(Value::as_i64) != Some(0)
        || close.get("global_setdown_error").and_then(Value::as_i64) != Some(0)
    {
        return Err(format!("resident cleanup was not clean: {value}"));
    }
    Ok(())
}

fn validate_resident_ready(value: &Value, worker_pid: u32) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "resident ready response is not an object".to_string())?;
    if object.len() != 4
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("session_ready")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || !value["setup"].is_object()
    {
        return Err(format!("resident ready envelope is invalid: {value}"));
    }
    Ok(())
}

fn validate_resident_probe(
    value: &Value,
    worker_pid: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "resident probe response is not an object".to_string())?;
    if object.len() != 6
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("session_probed")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || value["status"].as_str() != Some("ok")
        || value["guards_intact"].as_bool() != Some(true)
        || value["render_error"].as_i64() != Some(0)
    {
        return Err(format!(
            "resident {width}x{height} probe failed invariants: {value}"
        ));
    }
    Ok(())
}

fn validate_user_changed(
    value: &Value,
    worker_pid: u32,
    requested_slot: u32,
) -> Result<Vec<DynamicParameterUi>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "USER_CHANGED_PARAM response is not an object".to_string())?;
    if object.len() != 5
        || value["v"].as_u64() != Some(1)
        || value["type"].as_str() != Some("user_changed_done")
        || value["worker_pid"].as_u64() != Some(worker_pid as u64)
        || value["status"].as_str() != Some("ok")
    {
        return Err(format!("invalid USER_CHANGED_PARAM response: {value}"));
    }
    let report = value["report"]
        .as_object()
        .ok_or_else(|| "USER_CHANGED_PARAM report is not an object".to_string())?;
    if report.len() != 3
        || report["slot"].as_u64() != Some(requested_slot as u64)
        || report["selector_error"].as_i64() != Some(0)
    {
        return Err(format!(
            "invalid USER_CHANGED_PARAM report: {}",
            value["report"]
        ));
    }
    serde_json::from_value(report["parameters"].clone())
        .map_err(|error| format!("invalid USER_CHANGED_PARAM parameter table: {error}"))
}

enum ResidentCommand {
    Render {
        frame_index: u64,
        parameters: Vec<InteractiveParameter>,
        output: PathBuf,
    },
    UserChanged {
        slot: u32,
        parameters: String,
        reply: Sender<Result<Vec<DynamicParameterUi>, String>>,
    },
    Close(Sender<Result<(), String>>),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
struct DynamicParameterUi {
    slot: u32,
    ui_flags: u32,
    flags: u32,
}

struct ResidentSessionHandle {
    /// SHA-256 of the AEX bytes the guest staged and loaded.
    staged_plugin_sha256: String,
    sender: Sender<ResidentCommand>,
    receiver: Receiver<Result<RenderResult, String>>,
    next_frame: u64,
    child: SharedResidentChild,
    shutdown_requested: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

type SharedResidentChild = Arc<Mutex<Option<Child>>>;

impl ResidentSessionHandle {
    fn shutdown(&mut self) -> Result<(), String> {
        self.shutdown_with_deadline(RESIDENT_CLOSE_DEADLINE)
    }

    fn shutdown_with_deadline(&mut self, deadline: Duration) -> Result<(), String> {
        let Some(join) = self.join.take() else {
            return Ok(());
        };
        self.shutdown_requested.store(true, Ordering::Release);
        let (reply_sender, reply_receiver) = mpsc::channel();
        if let Err(error) = self.sender.send(ResidentCommand::Close(reply_sender)) {
            terminate_shared_resident_child(&self.child);
            enqueue_resident_join(join);
            return Err(format!("request resident close: {error}"));
        }
        match reply_receiver.recv_timeout(deadline) {
            Ok(result) => {
                // The close outcome is sent only after protocol cleanup and child
                // ownership have been settled. Joining is never allowed to block
                // the GUI thread, even for that normally-complete case.
                enqueue_resident_join(join);
                result
            }
            Err(error) => {
                let ownership_transferred = terminate_shared_resident_child(&self.child);
                enqueue_resident_join(join);
                Err(format!(
                    "resident close exceeded the {} ms shutdown deadline: {error}; child termination {} and cleanup continues in background",
                    deadline.as_millis(),
                    if ownership_transferred {
                        "was transferred from the owner"
                    } else {
                        "remains with the controller"
                    }
                ))
            }
        }
    }
}

impl Drop for ResidentSessionHandle {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("aexcompat resident cleanup failed: {error}");
        }
    }
}

pub(crate) fn gui_repository_root() -> PathBuf {
    repository_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(test)]
fn read_control_message(reader: &mut impl Read) -> Result<Option<Value>, String> {
    let mut total = 0;
    read_control_message_accounted(reader, &mut total)
}

fn read_control_message_accounted(
    reader: &mut impl Read,
    total: &mut usize,
) -> Result<Option<Value>, String> {
    let mut prefix = [0u8; 4];
    match reader.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("read resident response prefix: {error}")),
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > MAX_RESIDENT_RESPONSE_BYTES {
        return Err(format!("resident response length is invalid: {length}"));
    }
    *total = total
        .checked_add(length + prefix.len())
        .ok_or_else(|| "macos_worker_protocol_limit: byte accounting overflow".to_string())?;
    if *total > MAX_RESIDENT_PROTOCOL_BYTES {
        return Err(format!(
            "macos_worker_protocol_limit: resident responses exceeded {MAX_RESIDENT_PROTOCOL_BYTES} bytes"
        ));
    }
    let mut bytes = vec![0u8; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("read resident response: {error}"))?;
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        format!(
            "parse resident response JSON: {error}; payload={}",
            String::from_utf8_lossy(&bytes)
        )
    })
}

fn parameter_payload(parameters: &[InteractiveParameter]) -> Result<String, String> {
    fixture_parameter_payload(parameters).map(|payload| payload.transport)
}

fn write_resident_input_slot(
    input: &Path,
    slot: &Path,
    format: MacRenderFormat,
) -> Result<(u32, u32), String> {
    let (width, height, argb) = encode_resident_image(input, format)?;
    std::fs::write(slot, argb).map_err(|error| format!("write resident input slot: {error}"))?;
    Ok((width, height))
}

fn validate_resident_image_dimensions(input: &Path) -> Result<(u32, u32), String> {
    let (width, height) = image::image_dimensions(input)
        .map_err(|error| format!("read input dimensions: {error}"))?;
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(format!(
            "resident input dimensions must be 1x1..={MAX_WIDTH}x{MAX_HEIGHT}; got {width}x{height}"
        ));
    }
    Ok((width, height))
}

fn encode_resident_image(
    input: &Path,
    format: MacRenderFormat,
) -> Result<(u32, u32, Vec<u8>), String> {
    // Bound allocation before decoding, including all secondary layers.
    validate_resident_image_dimensions(input)?;
    if format == MacRenderFormat::PngArgb8 {
        let rgba = image::open(input)
            .map_err(|error| format!("open resident input image: {error}"))?
            .into_rgba8();
        let (width, height) = rgba.dimensions();
        if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(format!(
                "resident input dimensions must be 1x1..={MAX_WIDTH}x{MAX_HEIGHT}; got {width}x{height}"
            ));
        }
        let mut argb = Vec::with_capacity(rgba.as_raw().len());
        for pixel in rgba.as_raw().chunks_exact(4) {
            argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
        }
        return Ok((width, height, argb));
    }
    let decoded =
        image::open(input).map_err(|error| format!("open resident input image: {error}"))?;
    let (width, height) = (decoded.width(), decoded.height());
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(format!(
            "resident input dimensions must be 1x1..={MAX_WIDTH}x{MAX_HEIGHT}; got {width}x{height}"
        ));
    }
    let rgba = aexcompat_broker::decoded_image::decoded_rgba(decoded, format.artifact_format());
    if format == MacRenderFormat::ExrArgb32f
        && rgba
            .chunks_exact(4)
            .any(|word| !f32::from_le_bytes(word.try_into().unwrap()).is_finite())
    {
        return Err("resident float input contains a non-finite sample".into());
    }
    let component_bytes = format.bytes_per_pixel() / 4;
    let mut argb = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(format.bytes_per_pixel()) {
        argb.extend_from_slice(&pixel[3 * component_bytes..]);
        argb.extend_from_slice(&pixel[..3 * component_bytes]);
    }
    Ok((width, height, argb))
}

fn argb_to_rgba_words(bytes: &[u8], component_bytes: usize) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bytes.len());
    for pixel in bytes.chunks_exact(component_bytes * 4) {
        rgba.extend_from_slice(&pixel[component_bytes..component_bytes * 4]);
        rgba.extend_from_slice(&pixel[..component_bytes]);
    }
    rgba
}

fn save_argb8_slot(
    slot: &Path,
    output: &Path,
    width: u32,
    height: u32,
    expected_checksum: &str,
) -> Result<String, String> {
    let argb =
        std::fs::read(slot).map_err(|error| format!("read resident output slot: {error}"))?;
    let expected = width as usize * height as usize * 4;
    if argb.len() != expected {
        return Err(format!(
            "resident output has {} bytes, expected {expected}",
            argb.len()
        ));
    }
    let checksum = format!("{:x}", Sha256::digest(&argb));
    if checksum != expected_checksum {
        return Err(format!(
            "resident output checksum mismatch: response={expected_checksum} slot={checksum}"
        ));
    }
    let mut rgba = Vec::with_capacity(argb.len());
    for pixel in argb.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[1], pixel[2], pixel[3], pixel[0]]);
    }
    let image = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "resident output dimensions do not match bytes".to_string())?;
    image
        .save(output)
        .map_err(|error| format!("save resident output PNG: {error}"))?;
    Ok(checksum)
}

/// Display/export is derived from the verified world, never fed back into rendering.
/// PNG16 uses the native Windows AE16 expansion; float PNG is an SDR display copy.
fn save_deep_preview(
    slot: &Path,
    output: &Path,
    width: u32,
    height: u32,
    format: MacRenderFormat,
    expected_checksum: &str,
) -> Result<(), String> {
    use aexcompat_broker::decoded_image::{native_rgba_to_preview, rgba16_transport_to_png16};
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err("deep preview dimensions exceed GUI bounds".into());
    }
    let expected = u64::from(width) * u64::from(height) * format.bytes_per_pixel() as u64;
    let mut argb = Vec::new();
    std::fs::File::open(slot)
        .map_err(|e| e.to_string())?
        .take(expected + 1)
        .read_to_end(&mut argb)
        .map_err(|e| e.to_string())?;
    if argb.len() as u64 != expected || format!("{:x}", Sha256::digest(&argb)) != expected_checksum
    {
        return Err("deep preview output size/checksum mismatch".into());
    }
    let rgba = argb_to_rgba_words(&argb, format.bytes_per_pixel() / 4);
    let image = match format {
        MacRenderFormat::RawArgb16 => {
            let (samples, _) = rgba16_transport_to_png16(&rgba).map_err(|e| e.to_string())?;
            image::DynamicImage::ImageRgba16(
                image::ImageBuffer::from_raw(width, height, samples)
                    .ok_or("invalid PNG16 dimensions")?,
            )
        }
        MacRenderFormat::ExrArgb32f => {
            let samples = native_rgba_to_preview(&rgba, format.artifact_format())
                .map_err(|e| e.to_string())?;
            image::DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(width, height, samples)
                    .ok_or("invalid float preview dimensions")?,
            )
        }
        MacRenderFormat::PngArgb8 => return Err("deep preview requires a deep world".into()),
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| e.to_string())?;
    image
        .write_to(&mut file, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
fn save_argb16_raw_slot(
    slot: &Path,
    directory: &Path,
    width: u32,
    height: u32,
    plugin_sha256: &str,
    input_sha256: &str,
    parameters: &str,
    render_path: &str,
    expected_checksum: &str,
) -> Result<String, String> {
    save_argb16_raw_slot_at_origin(
        slot,
        directory,
        width,
        height,
        plugin_sha256,
        input_sha256,
        parameters,
        render_path,
        expected_checksum,
        (0, 0),
    )
}

fn save_argb16_raw_slot_at_origin(
    slot: &Path,
    directory: &Path,
    width: u32,
    height: u32,
    plugin_sha256: &str,
    input_sha256: &str,
    parameters: &str,
    render_path: &str,
    expected_checksum: &str,
    origin: (i32, i32),
) -> Result<String, String> {
    let expected = u64::from(width) * u64::from(height) * 8;
    let file = std::fs::File::open(slot)
        .map_err(|error| format!("open resident ARGB16 output slot: {error}"))?;
    if file.metadata().map_err(|error| error.to_string())?.len() != expected {
        return Err(format!(
            "resident ARGB16 output size mismatch; expected {expected} bytes"
        ));
    }
    let mut argb = Vec::new();
    file.take(expected + 1)
        .read_to_end(&mut argb)
        .map_err(|error| format!("read resident ARGB16 output slot: {error}"))?;
    if argb.len() as u64 != expected {
        return Err("resident ARGB16 output changed size while reading".into());
    }
    let checksum = format!("{:x}", Sha256::digest(&argb));
    if checksum != expected_checksum {
        return Err(format!(
            "resident output checksum mismatch: response={expected_checksum} slot={checksum}"
        ));
    }
    let mut conditions = fixture_conditions(
        plugin_sha256,
        input_sha256,
        &argb,
        "argb16",
        render_path,
        &json!([parameters]),
        "premultiplied",
        0,
        1,
        1,
        30,
        None,
    );
    conditions.comparison_identity["origin"] = json!({"x": origin.0, "y": origin.1});
    write_raw_world_artifact(
        directory,
        &argb_to_rgba_words(&argb, 2),
        width,
        height,
        RenderPixelFormat::Argb16,
        origin.0,
        origin.1,
        conditions,
    )
    .map_err(|error| format!("write ARGB16 raw artifact: {error}"))?;
    Ok(checksum)
}

#[cfg(test)]
fn save_argb32f_exr_slot(
    slot: &Path,
    directory: &Path,
    width: u32,
    height: u32,
    plugin_sha256: &str,
    input_sha256: &str,
    parameters: &str,
    render_path: &str,
    expected_checksum: &str,
) -> Result<String, String> {
    save_argb32f_exr_slot_at_origin(
        slot,
        directory,
        width,
        height,
        plugin_sha256,
        input_sha256,
        parameters,
        render_path,
        expected_checksum,
        (0, 0),
    )
}

fn save_argb32f_exr_slot_at_origin(
    slot: &Path,
    directory: &Path,
    width: u32,
    height: u32,
    plugin_sha256: &str,
    input_sha256: &str,
    parameters: &str,
    render_path: &str,
    expected_checksum: &str,
    origin: (i32, i32),
) -> Result<String, String> {
    let argb = std::fs::read(slot)
        .map_err(|error| format!("read resident ARGB32F output slot: {error}"))?;
    let expected = width as usize * height as usize * 16;
    if argb.len() != expected {
        return Err(format!(
            "resident ARGB32F output has {} bytes, expected {expected}",
            argb.len()
        ));
    }
    let world_sha256 = format!("{:x}", Sha256::digest(&argb));
    if world_sha256 != expected_checksum {
        return Err(format!(
            "resident output checksum mismatch: response={expected_checksum} slot={world_sha256}"
        ));
    }
    let mut rgba = Vec::with_capacity(argb.len());
    for pixel in argb.chunks_exact(16) {
        rgba.extend_from_slice(&pixel[4..16]);
        rgba.extend_from_slice(&pixel[0..4]);
    }
    let conditions = RenderArtifactConditions {
        premultiplication: "premultiplied".into(),
        working_space: "None".into(),
        render_mode: "software".into(),
        comparison_identity: json!({
            "plugin_sha256": plugin_sha256,
            "input_sha256": input_sha256,
            "world_sha256": world_sha256,
            "render_path": render_path,
            "pixel_format": "argb32f",
            "timing": {"current_time": 0, "time_step": 1, "total_time": 1, "time_scale": 30},
            "requested_parameters": [parameters],
            "origin": {"x": origin.0, "y": origin.1}
        }),
    };
    write_float32_exr_artifact(
        directory, &rgba, width, height, origin.0, origin.1, conditions,
    )
    .map_err(|error| format!("write FLOAT32 EXR artifact: {error}"))?;
    std::fs::write(directory.join("output.argb"), &argb)
        .map_err(|error| format!("write FLOAT32 raw sidecar: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(&argb)))
}

struct StartedResidentWorker {
    child: Child,
    stdin: ChildStdin,
    response_receiver: Receiver<Result<Option<Value>, String>>,
    stderr_receiver: Receiver<Result<String, String>>,
    worker_pid: u32,
    session: WorkerSession,
    output_slot: PathBuf,
    width: u32,
    height: u32,
    security_tier: SecurityTier,
    fallback_reasons: Vec<String>,
    plugin_sha256: String,
    input_sha256: String,
    additional_session_files: usize,
    fixture_input_world: Option<PathBuf>,
    fixture_layer_worlds: Vec<(u32, u32, u32, MacRenderFormat, PathBuf)>,
}

struct MacFixtureLaunch<'a> {
    layers: &'a [(u32, PathBuf)],
    worlds: Option<&'a FixtureWorlds>,
    smart: bool,
    time_scale: u32,
}

#[derive(Serialize)]
struct StagedLayerManifest<'a> {
    v: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    primary: Option<StagedWorldLayout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    primary_pixel_format: Option<&'a str>,
    layers: &'a [StagedLayerEntry],
}

#[derive(Serialize)]
struct StagedWorldLayout {
    rowbytes: u32,
    padding_byte: u8,
    origin_x: i32,
    origin_y: i32,
    extent: [i32; 4],
}

impl From<&FixtureWorldLayout> for StagedWorldLayout {
    fn from(world: &FixtureWorldLayout) -> Self {
        Self {
            rowbytes: world.rowbytes,
            padding_byte: world.padding_byte,
            origin_x: world.origin.x,
            origin_y: world.origin.y,
            extent: [
                world.extent.left,
                world.extent.top,
                world.extent.right,
                world.extent.bottom,
            ],
        }
    }
}

#[derive(Serialize)]
struct StagedLayerEntry {
    slot: u32,
    width: u32,
    height: u32,
    path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pixel_format: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<StagedWorldLayout>,
}

fn start_resident_worker(
    candidates: &[GuestWorkerCandidate],
    aex: &Path,
    input: &Path,
    format: MacRenderFormat,
    fixture: Option<&MacFixtureLaunch<'_>>,
    negotiate_output: bool,
) -> Result<StartedResidentWorker, String> {
    let mut failures = Vec::new();
    for candidate in candidates {
        let mut probe_worker = match launch_resident_candidate(
            candidate,
            aex,
            input,
            format,
            fixture,
            negotiate_output,
        ) {
            Ok(worker) => worker,
            Err(error) => {
                failures.push(format!(
                    "{} ({}): {error}",
                    candidate.path.display(),
                    if candidate.native {
                        "native"
                    } else {
                        "fallback"
                    },
                ));
                continue;
            }
        };
        let admission =
            write_control_message(&mut probe_worker.stdin, &json!({"v": 1, "type": "probe"}))
                .and_then(|()| {
                    let response = probe_worker
                        .response_receiver
                        .recv_timeout(RESIDENT_RENDER_DEADLINE)
                        .map_err(|error| format!("probe response timeout: {error}"))?
                        .map_err(|error| format!("probe response reader: {error}"))?
                        .ok_or_else(|| "worker closed before session_probed".to_string())?;
                    validate_resident_probe(
                        &response,
                        probe_worker.worker_pid,
                        probe_worker.width,
                        probe_worker.height,
                    )
                });
        if let Err(error) = admission {
            let stderr = terminate_resident_worker(probe_worker);
            failures.push(format!(
                "{} ({}): {error}{}",
                candidate.path.display(),
                if candidate.native {
                    "native"
                } else {
                    "fallback"
                },
                stderr
                    .filter(|stderr| !stderr.trim().is_empty())
                    .map(|stderr| format!("; stderr: {}", stderr.trim()))
                    .unwrap_or_default(),
            ));
            continue;
        }
        if let Err(error) = close_probe_worker(probe_worker) {
            failures.push(format!(
                "{} ({}): probe cleanup failed: {error}",
                candidate.path.display(),
                if candidate.native {
                    "native"
                } else {
                    "fallback"
                },
            ));
            continue;
        }
        match launch_resident_candidate(candidate, aex, input, format, fixture, negotiate_output) {
            Ok(mut fresh_worker) => {
                fresh_worker.fallback_reasons = failures;
                return Ok(fresh_worker);
            }
            Err(error) => {
                failures.push(format!(
                    "{} ({}): fresh session after probe failed: {error}",
                    candidate.path.display(),
                    if candidate.native {
                        "native"
                    } else {
                        "fallback"
                    },
                ));
            }
        }
    }
    Err(format!(
        "all resident guest workers failed readiness: {}",
        failures.join(" | ")
    ))
}

fn launch_resident_candidate(
    candidate: &GuestWorkerCandidate,
    aex: &Path,
    input: &Path,
    format: MacRenderFormat,
    fixture: Option<&MacFixtureLaunch<'_>>,
    negotiate_output: bool,
) -> Result<StartedResidentWorker, String> {
    let session = WorkerSession::create()?;
    let worker = session.stage_file(&candidate.path, "worker")?;
    let plugin = session.stage_file(aex, "plugin.aex")?;
    let primary_format = fixture
        .and_then(|fixture| fixture.worlds)
        .map(|worlds| match worlds.primary.pixel_format {
            FixturePixelFormat::Argb8 => MacRenderFormat::PngArgb8,
            FixturePixelFormat::Argb16 => MacRenderFormat::RawArgb16,
            FixturePixelFormat::Argb32f => MacRenderFormat::ExrArgb32f,
        })
        .unwrap_or(format);
    let input_slot = session
        .root()
        .join(format!("input.{}", primary_format.pixel_format()));
    let output_slot = session
        .root()
        .join(format!("output.{}", format.pixel_format()));
    let (width, height) = write_resident_input_slot(input, &input_slot, primary_format)?;
    let plugin_sha256 = format!(
        "{:x}",
        Sha256::digest(
            std::fs::read(&plugin).map_err(|error| format!("hash staged AEX: {error}"))?
        )
    );
    let input_sha256 = format!(
        "{:x}",
        Sha256::digest(
            std::fs::read(&input_slot)
                .map_err(|error| format!("hash staged resident input: {error}"))?
        )
    );
    std::fs::write(
        &output_slot,
        vec![0u8; width as usize * height as usize * format.bytes_per_pixel()],
    )
    .map_err(|error| format!("initialize resident output slot: {error}"))?;
    let mut arguments = vec![
        "session".to_string(),
        plugin.to_string_lossy().into_owned(),
        input_slot.to_string_lossy().into_owned(),
        output_slot.to_string_lossy().into_owned(),
        width.to_string(),
        height.to_string(),
        fixture.map_or(30, |fixture| fixture.time_scale).to_string(),
        "--pixel-format".to_string(),
        format.pixel_format().to_string(),
    ];
    if negotiate_output {
        arguments.extend(["--negotiate-output".into(), "1".into()]);
    }
    let mut fixture_layer_worlds = Vec::new();
    if let Some(fixture) = fixture {
        let mixed_primary = primary_format != format;
        let mixed_secondary = fixture.worlds.is_some_and(|worlds| {
            worlds
                .secondary
                .iter()
                .any(|world| world.pixel_format.name() != format.pixel_format())
        });
        if let Some(worlds) = fixture.worlds {
            if worlds.primary.width != width
                || worlds.primary.height != height
                || worlds.secondary.iter().any(|world| {
                    !fixture
                        .layers
                        .iter()
                        .any(|(slot, _)| Some(*slot) == world.slot)
                })
            {
                return Err("fixture world layout does not match staged image or depth".into());
            }
        }
        let mut staged_layers = Vec::with_capacity(fixture.layers.len());
        for (slot, source) in fixture.layers {
            let declared_layout = fixture.worlds.and_then(|worlds| {
                worlds
                    .secondary
                    .iter()
                    .find(|world| world.slot == Some(*slot))
            });
            let layer_format = declared_layout
                .map(|world| match world.pixel_format {
                    FixturePixelFormat::Argb8 => MacRenderFormat::PngArgb8,
                    FixturePixelFormat::Argb16 => MacRenderFormat::RawArgb16,
                    FixturePixelFormat::Argb32f => MacRenderFormat::ExrArgb32f,
                })
                .unwrap_or(format);
            let path = session
                .root()
                .join(format!("layer-{slot}.{}", layer_format.pixel_format()));
            let (layer_width, layer_height) =
                write_resident_input_slot(source, &path, layer_format)?;
            if declared_layout
                .is_some_and(|world| world.width != layer_width || world.height != layer_height)
            {
                return Err(format!(
                    "fixture layer slot {slot} layout dimensions differ from image"
                ));
            }
            staged_layers.push(StagedLayerEntry {
                slot: *slot,
                width: layer_width,
                height: layer_height,
                path,
                pixel_format: (mixed_primary || mixed_secondary)
                    .then_some(layer_format.pixel_format()),
                layout: declared_layout.map(StagedWorldLayout::from),
            });
            fixture_layer_worlds.push((
                *slot,
                layer_width,
                layer_height,
                layer_format,
                session
                    .root()
                    .join(format!("fixture-layer-slot{slot}-world.bin")),
            ));
        }
        let manifest_path = session.root().join("fixture-layers-v1.json");
        let manifest = serde_json::to_vec(&StagedLayerManifest {
            v: if mixed_primary {
                4
            } else if mixed_secondary {
                3
            } else if fixture.worlds.is_some() {
                2
            } else {
                1
            },
            primary: fixture
                .worlds
                .map(|worlds| StagedWorldLayout::from(&worlds.primary)),
            primary_pixel_format: mixed_primary.then_some(primary_format.pixel_format()),
            layers: &staged_layers,
        })
        .map_err(|error| format!("serialize fixture layer manifest: {error}"))?;
        std::fs::write(&manifest_path, manifest)
            .map_err(|error| format!("write fixture layer manifest: {error}"))?;
        arguments.extend([
            "--fixture-layers-v1".into(),
            manifest_path.to_string_lossy().into_owned(),
            "--fixture-render-path".into(),
            if fixture.smart { "smart" } else { "classic" }.into(),
        ]);
    }
    let mut command = session.command(
        &worker,
        candidate.security_tier(),
        ResourceLimits::default(),
    );
    let mut child = command
        .args(arguments)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("macos_worker_launch: {error}"))?;
    let worker_pid = child.id();
    let (Some(stdin), Some(mut stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = terminate_process_group(&mut child);
        return Err("one or more resident worker pipes are unavailable".into());
    };
    let (response_sender, response_receiver) = mpsc::channel();
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut protocol_bytes = 0;
        loop {
            let response = read_control_message_accounted(&mut stdout, &mut protocol_bytes);
            let terminal = !matches!(response, Ok(Some(_)));
            if response_sender.send(response).is_err() || terminal {
                break;
            }
        }
    });
    thread::spawn(move || {
        let text =
            read_bounded(&mut stderr, MAX_STDERR_BYTES, "resident stderr").and_then(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|error| format!("resident stderr is not UTF-8: {error}"))
            });
        let _ = stderr_sender.send(text);
    });
    let ready = response_receiver
        .recv_timeout(RESIDENT_START_DEADLINE)
        .map_err(|error| format!("ready response timeout: {error}"))
        .and_then(|response| response.map_err(|error| format!("ready response reader: {error}")))
        .and_then(|response| {
            response.ok_or_else(|| "worker closed before session_ready".to_string())
        })
        .and_then(|response| validate_resident_ready(&response, worker_pid));
    if let Err(error) = ready {
        drop(stdin);
        let _ = terminate_process_group(&mut child);
        let stderr = stderr_receiver
            .recv_timeout(Duration::from_millis(200))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        return Err(format!(
            "{error}{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!("; stderr: {}", stderr.trim())
            }
        ));
    }
    let additional_session_files = fixture.map_or(0, |fixture| fixture.layers.len() * 2 + 2);
    if let Err(error) = session.audit_tree_with_additional_files(additional_session_files) {
        drop(stdin);
        let _ = terminate_process_group(&mut child);
        return Err(error);
    }
    let fixture_input_world = fixture.map(|_| session.root().join("fixture-input-world.bin"));
    Ok(StartedResidentWorker {
        child,
        stdin,
        response_receiver,
        stderr_receiver,
        worker_pid,
        session,
        output_slot,
        width,
        height,
        security_tier: candidate.security_tier(),
        fallback_reasons: Vec::new(),
        plugin_sha256,
        input_sha256,
        additional_session_files,
        fixture_input_world,
        fixture_layer_worlds,
    })
}

fn terminate_resident_worker(mut worker: StartedResidentWorker) -> Option<String> {
    drop(worker.stdin);
    let _ = terminate_process_group(&mut worker.child);
    worker
        .stderr_receiver
        .recv_timeout(Duration::from_millis(200))
        .ok()
        .and_then(Result::ok)
}

fn close_probe_worker(mut worker: StartedResidentWorker) -> Result<(), String> {
    let mut errors = Vec::new();
    let close_request = write_control_message(&mut worker.stdin, &json!({"v": 1, "type": "close"}));
    drop(worker.stdin);
    match close_request {
        Ok(()) => match worker
            .response_receiver
            .recv_timeout(RESIDENT_CLOSE_DEADLINE)
        {
            Ok(Ok(Some(response))) => {
                if let Err(error) = validate_resident_close(&response, worker.worker_pid) {
                    errors.push(error);
                }
            }
            Ok(Ok(None)) => errors.push("probe worker closed before session_closed".into()),
            Ok(Err(error)) => errors.push(format!("probe close response reader: {error}")),
            Err(error) => errors.push(format!("probe close response timeout: {error}")),
        },
        Err(error) => errors.push(error),
    }
    if let Err(error) = wait_or_kill_resident_child(worker.child, RESIDENT_CLOSE_DEADLINE) {
        errors.push(error);
    }
    let stderr = worker
        .stderr_receiver
        .recv_timeout(RESIDENT_CLOSE_DEADLINE)
        .unwrap_or_else(|error| Err(format!("probe stderr collection failed: {error}")))
        .unwrap_or_else(|error| error);
    if !stderr.trim().is_empty() {
        errors.push(format!("probe worker stderr: {}", stderr.trim()));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" | "))
    }
}

fn combine_fixture_inspection_and_close<T>(
    inspected: Result<T, String>,
    close: Result<(), String>,
) -> Result<T, String> {
    match (inspected, close) {
        (Ok(output), Ok(())) => Ok(output),
        (Err(render_error), Ok(())) => Err(render_error),
        (Ok(_), Err(close_error)) => Err(close_error),
        (Err(render_error), Err(close_error)) => {
            Err(format!("{render_error}; worker cleanup: {close_error}"))
        }
    }
}

fn selected_layer_paths(parameters: &[InteractiveParameter]) -> Vec<(u32, PathBuf)> {
    parameters
        .iter()
        .filter(|parameter| parameter.kind == "layer")
        .filter_map(|parameter| {
            parameter
                .layer_path
                .as_ref()
                .map(|path| (parameter.slot, path.clone()))
        })
        .collect()
}

fn fixture_staging_path(output: &Path) -> Result<PathBuf, String> {
    let parent = output
        .parent()
        .ok_or_else(|| "fixture output has no parent".to_string())?;
    let name = output
        .file_name()
        .ok_or_else(|| "fixture output has no name".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create fixture output parent: {error}"))?;
    for nonce in 0..1024u32 {
        let candidate = parent.join(format!(
            ".{}.fixture-tmp-{}-{nonce}",
            name.to_string_lossy(),
            std::process::id()
        ));
        if !candidate.exists() {
            std::fs::create_dir(&candidate)
                .map_err(|error| format!("create fixture staging directory: {error}"))?;
            return Ok(candidate);
        }
    }
    Err("no fixture staging name available".into())
}

fn fixture_conditions(
    plugin_sha256: &str,
    input_sha256: &str,
    world: &[u8],
    pixel_format: &str,
    render_path: &str,
    requested_parameters: &Value,
    premultiplication: &str,
    current_time: i32,
    time_step: i32,
    total_time: i32,
    time_scale: u32,
    case_identity: Option<&FixtureCaseIdentity>,
) -> RenderArtifactConditions {
    let mut comparison_identity = json!({
        "plugin_sha256": plugin_sha256,
        "input_sha256": input_sha256,
        "world_sha256": format!("{:x}", Sha256::digest(world)),
        "render_path": render_path,
        "pixel_format": pixel_format,
        "timing": {
            "current_time": current_time,
            "time_step": time_step,
            "total_time": total_time,
            "time_scale": time_scale
        },
        "requested_parameters": requested_parameters,
        "origin": {"x": 0, "y": 0}
    });
    if let Some(case_identity) = case_identity {
        comparison_identity["fixture_case"] = json!(case_identity);
    }
    RenderArtifactConditions {
        premultiplication: premultiplication.into(),
        working_space: "None".into(),
        render_mode: "software".into(),
        comparison_identity,
    }
}

fn fixture_checkpoint_source<'a>(
    suffix: &str,
    primary: &'a CapturedWorldRecord,
    output: &'a [u8],
    output_format: MacRenderFormat,
    primary_format: MacRenderFormat,
    layers: &'a [(u32, MacRenderFormat, CapturedWorldRecord)],
) -> Result<
    (
        u32,
        u32,
        &'a [u8],
        Option<&'a CapturedWorldRecord>,
        MacRenderFormat,
    ),
    String,
> {
    match suffix {
        "input" => Ok((
            primary.width,
            primary.height,
            &primary.raw_argb,
            Some(primary),
            primary_format,
        )),
        "output" => Ok((primary.width, primary.height, output, None, output_format)),
        layer if layer.starts_with("layer-slot") => {
            let slot = layer["layer-slot".len()..]
                .parse::<u32>()
                .map_err(|_| "checkpoint layer slot is invalid".to_string())?;
            let (_, format, world) = layers
                .iter()
                .find(|(candidate, _, _)| *candidate == slot)
                .ok_or_else(|| format!("requested checkpoint layer slot {slot} was not staged"))?;
            Ok((
                world.width,
                world.height,
                &world.raw_argb,
                Some(world),
                *format,
            ))
        }
        _ => Err("checkpoint stage is unsupported".into()),
    }
}

fn fixture_render_request(timing: &FixtureTiming, parameters: &str) -> Value {
    json!({
        "v": 4,
        "type": "render_frame",
        "frame_index": 0,
        "current_time": {
            "value": timing.current_time,
            "step": timing.time_step,
            "total": timing.total_time,
            "scale": timing.time_scale
        },
        "parameters": parameters
    })
}

fn validate_fixture_world_record(
    observed: &CapturedWorldRecord,
    expected: Option<&FixtureWorldLayout>,
    width: u32,
    height: u32,
    format: MacRenderFormat,
) -> Result<(), String> {
    let packed = width
        .checked_mul(format.bytes_per_pixel() as u32)
        .ok_or_else(|| "fixture world packed row overflow".to_string())?;
    let (rowbytes, origin_x, origin_y, extent, padding_byte) = match expected {
        Some(world) => (
            world.rowbytes,
            world.origin.x,
            world.origin.y,
            [
                world.extent.left,
                world.extent.top,
                world.extent.right,
                world.extent.bottom,
            ],
            Some(world.padding_byte),
        ),
        None => (packed, 0, 0, [0, 0, width as i32, height as i32], None),
    };
    if observed.width != width
        || observed.height != height
        || observed.pixel_bytes != format.bytes_per_pixel() as u32
        || observed.rowbytes != rowbytes
        || observed.origin_x != origin_x
        || observed.origin_y != origin_y
        || observed.extent != extent
        || padding_byte.is_some_and(|fill| {
            observed
                .raw_argb
                .chunks_exact(rowbytes as usize)
                .any(|row| row[packed as usize..].iter().any(|byte| *byte != fill))
        })
    {
        return Err("fixture observed world differs from declared layout".into());
    }
    Ok(())
}

pub fn render_fixture_headless(
    repository: &Path,
    aex: &Path,
    fixture_path: &Path,
    output_directory: &Path,
) -> Result<Value, String> {
    if output_directory.exists() {
        return Err("fixture output exists".into());
    }
    let loaded = load_render_fixture(fixture_path)
        .map_err(|error| format!("load declarative fixture: {error}"))?;
    loaded
        .document
        .validate_render_platform()
        .map_err(|error| error.to_string())?;
    if loaded.document.schema_version == 1 {
        return render_single_fixture_headless(
            repository,
            aex,
            &loaded,
            &loaded.parameters,
            None,
            output_directory,
        );
    }
    let plugin_sha256 = format!(
        "{:x}",
        Sha256::digest(std::fs::read(aex).map_err(|error| error.to_string())?)
    );
    let cases = expand_fixture_cases(&loaded);
    let staging = fixture_staging_path(output_directory)?;
    let result = (|| -> Result<Value, String> {
        let mut case_reports = Vec::with_capacity(cases.len());
        for case in &cases {
            let identity = fixture_case_identity(&loaded, case, &plugin_sha256)
                .map_err(|error| format!("fixture case identity: {error}"))?;
            let relative = format!("cases/{}", identity.sha256);
            let case_output = staging.join(&relative);
            let report = render_single_fixture_headless(
                repository,
                aex,
                &loaded,
                &case.parameters,
                Some(&identity),
                &case_output,
            )
            .map_err(|error| format!("fixture case {} failed: {error}", case.index))?;
            case_reports.push(json!({
                "case_identity": identity,
                "artifact_directory": relative,
                "report": report
            }));
        }
        std::fs::rename(&staging, output_directory)
            .map_err(|error| format!("publish fixture matrix: {error}"))?;
        Ok(json!({
            "schema":"aexcompat.render_fixture_report", "schema_version":2,
            "fixture_sha256":loaded.sha256, "complete":true, "cases":case_reports
        }))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn render_single_fixture_headless(
    repository: &Path,
    aex: &Path,
    loaded: &LoadedRenderFixture,
    case_parameters: &[InteractiveParameter],
    case_identity: Option<&FixtureCaseIdentity>,
    output_directory: &Path,
) -> Result<Value, String> {
    if output_directory.exists() {
        return Err("fixture output exists".into());
    }
    let fixture = &loaded.document;
    let format = match fixture.pixel_format {
        FixturePixelFormat::Argb8 => MacRenderFormat::PngArgb8,
        FixturePixelFormat::Argb16 => MacRenderFormat::RawArgb16,
        FixturePixelFormat::Argb32f => MacRenderFormat::ExrArgb32f,
    };
    let primary_format = fixture
        .worlds
        .as_ref()
        .map(|worlds| match worlds.primary.pixel_format {
            FixturePixelFormat::Argb8 => MacRenderFormat::PngArgb8,
            FixturePixelFormat::Argb16 => MacRenderFormat::RawArgb16,
            FixturePixelFormat::Argb32f => MacRenderFormat::ExrArgb32f,
        })
        .unwrap_or(format);
    let parameters = fixture_parameter_payload(case_parameters)?;
    let layer_paths = case_parameters
        .iter()
        .filter(|parameter| parameter.kind == "layer")
        .filter_map(|parameter| {
            parameter
                .layer_path
                .as_ref()
                .map(|path| (parameter.slot, path.clone()))
        })
        .collect::<Vec<_>>();
    if layer_paths.len() > 8 {
        return Err("fixture secondary layer count exceeds 8".into());
    }
    let launch = MacFixtureLaunch {
        layers: &layer_paths,
        worlds: fixture.worlds.as_ref(),
        smart: fixture.render_path == "smart",
        time_scale: fixture.timing.time_scale,
    };
    let workers = guest_worker_candidates(repository)?;
    let mut started = start_resident_worker(
        &workers,
        aex,
        &loaded.primary_layer,
        format,
        Some(&launch),
        false,
    )?;
    let primary_width = started.width;
    let primary_height = started.height;
    let request = fixture_render_request(&fixture.timing, &parameters.transport);
    if let Err(error) = write_control_message(&mut started.stdin, &request) {
        let _ = close_probe_worker(started);
        return Err(error);
    }
    let response = match started
        .response_receiver
        .recv_timeout(RESIDENT_RENDER_DEADLINE)
    {
        Ok(Ok(Some(response))) => response,
        Ok(Ok(None)) => {
            let _ = close_probe_worker(started);
            return Err("resident worker closed before fixture frame response".into());
        }
        Ok(Err(error)) => {
            let _ = close_probe_worker(started);
            return Err(format!("resident fixture response reader: {error}"));
        }
        Err(error) => {
            let _ = close_probe_worker(started);
            return Err(format!("resident fixture response timeout: {error}"));
        }
    };
    let plugin_sha256 = started.plugin_sha256.clone();
    let input_sha256 = started.input_sha256.clone();
    let inspected = (|| -> Result<_, String> {
        let expected_checksum =
            validate_resident_frame(&response, 0, primary_width, primary_height, format)?;
        let actual_path = response["output"]["render_path"]
            .as_str()
            .ok_or_else(|| "fixture response has no render path".to_string())?;
        let expected_path = if fixture.render_path == "smart" {
            "smartfx"
        } else {
            "classic"
        };
        if actual_path != expected_path {
            return Err(format!(
                "fixture requested {} but worker reported {actual_path}",
                fixture.render_path
            ));
        }
        started
            .session
            .audit_tree_with_additional_files(started.additional_session_files)?;
        let output = std::fs::read(&started.output_slot)
            .map_err(|error| format!("read fixture output slot: {error}"))?;
        if format!("{:x}", Sha256::digest(&output)) != expected_checksum {
            return Err("fixture output checksum differs from worker response".into());
        }
        let input_path = started
            .fixture_input_world
            .as_ref()
            .ok_or_else(|| "fixture input world dump path is unavailable".to_string())?;
        let input_world = read_captured_world(input_path)
            .map_err(|error| format!("read fixture input world dump: {error}"))?;
        validate_fixture_world_record(
            &input_world,
            fixture.worlds.as_ref().map(|worlds| &worlds.primary),
            primary_width,
            primary_height,
            primary_format,
        )?;
        let mut layer_worlds = Vec::with_capacity(started.fixture_layer_worlds.len());
        for (slot, width, height, layer_format, path) in &started.fixture_layer_worlds {
            let observed = read_captured_world(path)
                .map_err(|error| format!("read fixture layer slot {slot} world dump: {error}"))?;
            let declared = fixture.worlds.as_ref().and_then(|worlds| {
                worlds
                    .secondary
                    .iter()
                    .find(|world| world.slot == Some(*slot))
            });
            validate_fixture_world_record(&observed, declared, *width, *height, *layer_format)?;
            layer_worlds.push((*slot, *layer_format, observed));
        }
        Ok((output, input_world, layer_worlds))
    })();
    let close = close_probe_worker(started);
    let (output_argb, primary_world, layer_worlds) =
        combine_fixture_inspection_and_close(inspected, close)?;

    let staging = fixture_staging_path(output_directory)?;
    let result = (|| -> Result<Value, String> {
        let component_bytes = format.bytes_per_pixel() / 4;
        let artifact_render_path = if fixture.render_path == "smart" {
            "smartfx"
        } else {
            "classic"
        };
        let output_rgba = argb_to_rgba_words(&output_argb, component_bytes);
        let conditions = fixture_conditions(
            &plugin_sha256,
            &input_sha256,
            &output_argb,
            format.pixel_format(),
            artifact_render_path,
            &parameters.identity,
            &fixture.premultiplication,
            fixture.timing.current_time,
            fixture.timing.time_step,
            fixture.timing.total_time,
            fixture.timing.time_scale,
            case_identity,
        );
        let final_metadata = match fixture.final_artifact {
            FixtureFinalArtifact::Raw => write_raw_world_artifact(
                &staging.join("final"),
                &output_rgba,
                primary_width,
                primary_height,
                format.artifact_format(),
                0,
                0,
                conditions.clone(),
            ),
            FixtureFinalArtifact::Exr => write_float32_exr_artifact(
                &staging.join("final"),
                &output_rgba,
                primary_width,
                primary_height,
                0,
                0,
                conditions.clone(),
            ),
        }
        .map_err(|error| format!("write fixture final artifact: {error}"))?;

        let mut checkpoint_reports = serde_json::Map::new();
        for checkpoint in &fixture.checkpoints {
            let suffix = checkpoint
                .stage
                .strip_prefix(&format!("{}-", fixture.render_path))
                .ok_or_else(|| "checkpoint stage does not match render path".to_string())?;
            let (width, height, argb, observed, checkpoint_format) = fixture_checkpoint_source(
                suffix,
                &primary_world,
                &output_argb,
                format,
                primary_format,
                &layer_worlds,
            )?;
            let rgba = observed
                .is_none()
                .then(|| argb_to_rgba_words(argb, checkpoint_format.bytes_per_pixel() / 4));
            let mut checkpoint_conditions = fixture_conditions(
                &plugin_sha256,
                &input_sha256,
                rgba.as_deref().unwrap_or(argb),
                format.pixel_format(),
                artifact_render_path,
                &parameters.identity,
                &fixture.premultiplication,
                fixture.timing.current_time,
                fixture.timing.time_step,
                fixture.timing.total_time,
                fixture.timing.time_scale,
                case_identity,
            );
            let metadata = if fixture.worlds.is_some()
                && let Some(world) = observed
            {
                checkpoint_conditions.comparison_identity["origin"] =
                    json!({"x":world.origin_x,"y":world.origin_y});
                write_strided_world_checkpoint_artifact(
                    &staging.join("checkpoints").join(&checkpoint.id),
                    &world.raw_argb,
                    width,
                    height,
                    checkpoint_format.artifact_format(),
                    world.rowbytes,
                    world.origin_x,
                    world.origin_y,
                    world.extent,
                    checkpoint_conditions,
                    &checkpoint.id,
                    &checkpoint.stage,
                    &loaded.sha256,
                )
            } else {
                let rgba = rgba.unwrap_or_else(|| {
                    argb_to_rgba_words(argb, checkpoint_format.bytes_per_pixel() / 4)
                });
                write_raw_world_checkpoint_artifact(
                    &staging.join("checkpoints").join(&checkpoint.id),
                    &rgba,
                    width,
                    height,
                    checkpoint_format.artifact_format(),
                    0,
                    0,
                    checkpoint_conditions,
                    &checkpoint.id,
                    &checkpoint.stage,
                    &loaded.sha256,
                )
            }
            .map_err(|error| format!("write fixture checkpoint: {error}"))?;
            checkpoint_reports.insert(checkpoint.id.clone(), metadata);
        }
        std::fs::rename(&staging, output_directory)
            .map_err(|error| format!("publish fixture output: {error}"))?;
        Ok(json!({
            "schema": "aexcompat.render_fixture_report",
            "schema_version": 1,
            "pixel_format": format.pixel_format(),
            "render_path": fixture.render_path,
            "final_artifact": final_metadata,
            "checkpoints": checkpoint_reports
        }))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn wait_or_kill_resident_child(
    mut child: Child,
    deadline: Duration,
) -> Result<std::process::ExitStatus, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(status),
            Ok(Some(status)) => {
                return Err(format!("resident worker exited with {status}"));
            }
            Ok(None) if started.elapsed() < deadline => {
                thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let termination = terminate_process_group(&mut child);
                return Err(format!(
                    "resident worker exceeded the {} ms close deadline; process-group cleanup: {}",
                    deadline.as_millis(),
                    termination
                        .map(|()| "complete".to_string())
                        .unwrap_or_else(|error| error)
                ));
            }
        }
    }
}

fn recv_resident_response(
    receiver: &Receiver<Result<Option<Value>, String>>,
    child: &SharedResidentChild,
    shutdown_requested: &AtomicBool,
    deadline: Duration,
) -> Result<Option<Value>, String> {
    let started = Instant::now();
    loop {
        if shutdown_requested.load(Ordering::Acquire) {
            terminate_shared_resident_child(child);
            return Err("resident render cancelled for session shutdown".into());
        }
        let worker_pid = match child.lock() {
            Ok(child) => child.as_ref().map(Child::id),
            Err(error) => error.into_inner().as_ref().map(Child::id),
        };
        if let Some(worker_pid) = worker_pid
            && let Err(error) = audit_process(worker_pid)
        {
            terminate_shared_resident_child(child);
            return Err(error);
        }
        let remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            terminate_shared_resident_child(child);
            return Err(format!(
                "resident render response timeout after {} ms",
                deadline.as_millis()
            ));
        }
        match receiver.recv_timeout(remaining.min(RESIDENT_RESPONSE_POLL)) {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(error)) => {
                terminate_shared_resident_child(child);
                return Err(error);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                terminate_shared_resident_child(child);
                return Err("resident response reader disconnected".into());
            }
        }
    }
}

#[cfg(test)]
fn start_resident_session(
    candidates: &[GuestWorkerCandidate],
    aex: &Path,
    input: &Path,
    output_directory: &Path,
    format: MacRenderFormat,
    layers: Vec<(u32, PathBuf)>,
) -> Result<ResidentSessionHandle, String> {
    start_resident_session_with_geometry(
        candidates,
        aex,
        input,
        output_directory,
        format,
        layers,
        false,
    )
}

fn start_resident_session_with_geometry(
    candidates: &[GuestWorkerCandidate],
    aex: &Path,
    input: &Path,
    _output_directory: &Path,
    format: MacRenderFormat,
    layers: Vec<(u32, PathBuf)>,
    negotiate_output: bool,
) -> Result<ResidentSessionHandle, String> {
    if layers.len() > 8 {
        return Err("resident secondary layer count exceeds 8".into());
    }
    let fixture = (!layers.is_empty()).then_some(MacFixtureLaunch {
        layers: &layers,
        worlds: None,
        smart: false,
        time_scale: 30,
    });
    let started = start_resident_worker(
        candidates,
        aex,
        input,
        format,
        fixture.as_ref(),
        negotiate_output,
    )?;
    let plugin_sha256 = started.plugin_sha256.clone();
    let staged_plugin_sha256 = plugin_sha256.clone();
    let input_sha256 = started.input_sha256.clone();
    let width = started.width;
    let height = started.height;
    let output_slot = started.output_slot.clone();
    let session = started.session;
    let additional_session_files = started.additional_session_files;
    let security_tier = started.security_tier;
    let fallback_reasons = started.fallback_reasons;
    let child = Arc::new(Mutex::new(Some(started.child)));
    let controller_child = Arc::clone(&child);
    let worker_pid = started.worker_pid;
    let mut stdin = started.stdin;
    let response_receiver = started.response_receiver;
    let stderr_receiver = started.stderr_receiver;
    let (command_sender, command_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let worker_shutdown_requested = Arc::clone(&shutdown_requested);
    let join = thread::spawn(move || {
        // Session ownership follows the controller thread so all exits remove
        // staged worker/plugin/slots only after process-group cleanup.
        let mut session = session;
        let mut running = true;
        let mut close_reply = None;
        while running {
            match command_receiver.recv() {
                Ok(ResidentCommand::Render {
                    frame_index,
                    parameters,
                    output,
                }) => {
                    let result = parameter_payload(&parameters).and_then(|payload| {
                        let request = json!({
                        "v": 2,
                        "type": "render_frame",
                        "frame_index": frame_index,
                        "current_time": {"value": 0, "scale": 30},
                        "parameters": payload,
                        });
                        write_control_message(&mut stdin, &request).map(|()| payload)
                    }).and_then(|payload| {
                        let response = recv_resident_response(
                            &response_receiver,
                            &controller_child,
                            &worker_shutdown_requested,
                            RESIDENT_RENDER_DEADLINE,
                        )?
                            .ok_or_else(|| "resident worker closed stdout".to_string())?;
                        let frame = validate_resident_frame_geometry(&response, frame_index,
                            (!negotiate_output).then_some((width, height)), format)?;
                        let (width, height) = (frame.width, frame.height);
                        let origin = frame.rect.map_or((0, 0), |rect| (rect[0], rect[1]));
                        let expected_checksum = frame.checksum;
                        let render_path = response["output"]["render_path"]
                            .as_str()
                            .expect("validated resident render path");
                        session.audit_tree_with_additional_files(additional_session_files)?;
                        let (observed_checksum, final_output, preview) = match format {
                            MacRenderFormat::PngArgb8 => (
                                save_argb8_slot(
                                    &output_slot,
                                    &output,
                                    width,
                                    height,
                                    &expected_checksum,
                                )?,
                                output.clone(),
                                Some(output.clone()),
                            ),
                            MacRenderFormat::ExrArgb32f => {
                                let checksum = save_argb32f_exr_slot_at_origin(
                                    &output_slot,
                                    &output,
                                    width,
                                    height,
                                    &plugin_sha256,
                                    &input_sha256,
                                    &payload,
                                    render_path,
                                    &expected_checksum,
                                    origin,
                                )?;
                                save_deep_preview(&output_slot, &output.join("output.png"), width, height, format, &checksum)?;
                                (checksum, output.join("output.exr"), Some(output.join("output.png")))
                            }
                            MacRenderFormat::RawArgb16 => {
                                let checksum = save_argb16_raw_slot_at_origin(
                                    &output_slot, &output, width, height,
                                    &plugin_sha256, &input_sha256, &payload,
                                    render_path, &expected_checksum,
                                    origin,
                                )?;
                                save_deep_preview(&output_slot, &output.join("output.png"), width, height, format, &checksum)?;
                                (checksum, output.join("output.bin"), Some(output.join("output.png")))
                            }
                        };
                        debug_assert_eq!(observed_checksum, expected_checksum);
                        Ok(RenderResult {
                            report: serde_json::to_string_pretty(&json!({
                                "schema": "aexcompat.macos-resident-render",
                                "worker_pid": worker_pid,
                                "security_tier": security_tier.as_str(),
                                "native_carrier": security_tier == SecurityTier::NativeCarrierTrustedOnly,
                                "native_carrier_trusted_only": security_tier == SecurityTier::NativeCarrierTrustedOnly,
                                "fallback_reasons": fallback_reasons,
                                "frame": response,
                            }))
                            .expect("resident report is serializable"),
                            output: final_output,
                            preview,
                        })
                    });
                    let _ = result_sender.send(result);
                }
                Ok(ResidentCommand::UserChanged {
                    slot,
                    parameters,
                    reply,
                }) => {
                    let result = write_control_message(
                        &mut stdin,
                        &json!({
                            "v": 1,
                            "type": "user_changed_param",
                            "slot": slot,
                            "parameters": parameters,
                        }),
                    )
                    .and_then(|()| {
                        recv_resident_response(
                            &response_receiver,
                            &controller_child,
                            &worker_shutdown_requested,
                            RESIDENT_RENDER_DEADLINE,
                        )?
                        .ok_or_else(|| "resident worker closed stdout".to_string())
                    })
                    .and_then(|response| validate_user_changed(&response, worker_pid, slot));
                    let _ = reply.send(result);
                }
                Ok(ResidentCommand::Close(reply)) => {
                    close_reply = Some(reply);
                    running = false;
                }
                Err(_) => running = false,
            }
        }
        let mut close_errors = Vec::new();
        let close_result = write_control_message(&mut stdin, &json!({"v": 1, "type": "close"}));
        drop(stdin);
        match close_result {
            Ok(()) => match response_receiver.recv_timeout(RESIDENT_CLOSE_DEADLINE) {
                Ok(Ok(Some(response))) => {
                    if let Err(error) = validate_resident_close(&response, worker_pid) {
                        close_errors.push(error);
                    }
                }
                Ok(Ok(None)) => {
                    close_errors.push("resident worker closed before session_closed".into())
                }
                Ok(Err(error)) => {
                    close_errors.push(format!("resident close response reader: {error}"))
                }
                Err(error) => {
                    close_errors.push(format!("resident close response timeout: {error}"))
                }
            },
            Err(error) => close_errors.push(error),
        }
        if let Some(child) = take_shared_resident_child(&controller_child)
            && let Err(error) = wait_or_kill_resident_child(child, RESIDENT_CLOSE_DEADLINE)
        {
            close_errors.push(error);
        }
        let stderr = stderr_receiver
            .recv_timeout(RESIDENT_CLOSE_DEADLINE)
            .unwrap_or_else(|error| Err(format!("stderr collection failed: {error}")))
            .unwrap_or_else(|error| error);
        if !stderr.trim().is_empty() {
            close_errors.push(format!(
                "resident worker {worker_pid} stderr: {}",
                stderr.trim()
            ));
        }
        if let Err(error) = session.cleanup() {
            close_errors.push(error);
        }
        let close_outcome = if close_errors.is_empty() {
            Ok(())
        } else {
            Err(close_errors.join(" | "))
        };
        if let Some(reply) = close_reply {
            let _ = reply.send(close_outcome.clone());
        } else if let Err(error) = close_outcome {
            let _ = result_sender.send(Err(error));
        }
    });
    Ok(ResidentSessionHandle {
        staged_plugin_sha256,
        sender: command_sender,
        receiver: result_receiver,
        next_frame: 0,
        child,
        shutdown_requested,
        join: Some(join),
    })
}

fn description_packet(
    staged_sha_before: &str,
    staged_sha_after: Option<&str>,
    mut parameters: Vec<InteractiveParameter>,
    mut defaults: Vec<InteractiveParameter>,
) -> Result<Value, &'static str> {
    if parameters.len() != defaults.len() {
        return Err("invalid_parameter_descriptor");
    }
    let mut slots = HashSet::new();
    for (parameter, default) in parameters.iter().zip(&defaults) {
        if parameter.slot == 0
            || parameter.slot != default.slot
            || parameter.kind != default.kind
            || parameter.layer_path.is_some()
            || default.layer_path.is_some()
            || contains_absolute_path(&parameter.name)
            || contains_absolute_path(&default.name)
            || parameter
                .choices
                .iter()
                .any(|choice| contains_absolute_path(choice))
            || default
                .choices
                .iter()
                .any(|choice| contains_absolute_path(choice))
            || !slots.insert(parameter.slot)
        {
            return Err("invalid_parameter_descriptor");
        }
    }
    for parameter in parameters.iter_mut().chain(defaults.iter_mut()) {
        parameter.debug_summary = None;
    }
    Ok(json!({
        "schema": "aexcompat.macos_aex_description",
        "schema_version": 1,
        "plugin_identity": {
            "sha256": staged_sha_before,
            "post_setup_sha256": staged_sha_after,
            "files_unchanged": staged_sha_after.map(|after| staged_sha_before == after),
        },
        "parameters": parameters,
        "defaults": defaults,
    }))
}

fn contains_absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        let previous = index.checked_sub(1).map(|previous| bytes[previous]);
        let boundary = previous.is_none_or(|previous| {
            !previous.is_ascii_alphanumeric()
                && !matches!(previous, b'_' | b'.' | b'-' | b'/' | b'\\')
        });
        (*byte == b'/'
            && boundary
            && bytes
                .get(index + 1)
                .is_some_and(|next| !next.is_ascii_whitespace()))
            || (*byte == b'\\' && bytes.get(index + 1) == Some(&b'\\') && boundary)
            || (byte.is_ascii_alphabetic()
                && bytes.get(index + 1) == Some(&b':')
                && matches!(bytes.get(index + 2), Some(b'/') | Some(b'\\'))
                && boundary)
    })
}

pub fn describe_aex_headless(repository: &Path, aex: &Path) -> Result<Value, &'static str> {
    if !aex.is_file() {
        return Err("plugin_missing");
    }
    let (parameters, defaults, _, staged_identity) =
        discover_parameters(repository, aex).map_err(|_| "description_failed")?;
    let staged_sha = staged_identity.0.ok_or("identity_unavailable")?;
    description_packet(
        &staged_sha,
        staged_identity.1.as_deref(),
        parameters,
        defaults,
    )
}

fn discover_parameters(
    repository: &Path,
    aex: &Path,
) -> Result<
    (
        Vec<InteractiveParameter>,
        Vec<InteractiveParameter>,
        String,
        (Option<String>, Option<String>),
    ),
    String,
> {
    let workers = guest_worker_candidates(repository)?;
    let mut failures = Vec::new();
    let mut process = None;
    let mut selected_tier = None;
    for candidate in &workers {
        let deadline = if candidate.native {
            NATIVE_SETUP_DEADLINE
        } else {
            RESIDENT_RENDER_DEADLINE
        };
        match run_staged_setup(&candidate.path, aex, candidate.security_tier(), deadline) {
            Ok(output) if output.output.status.success() => {
                process = Some(output);
                selected_tier = Some(candidate.security_tier());
                break;
            }
            Ok(output) => failures.push(format!(
                "{} [{}] exited {}: {}; worker stdout: {}",
                candidate.path.display(),
                candidate.security_tier().as_str(),
                output.output.status,
                String::from_utf8_lossy(&output.output.stderr).trim(),
                String::from_utf8_lossy(&output.output.stdout).trim(),
            )),
            Err(error) => failures.push(format!(
                "{} [{}]: {error}",
                candidate.path.display(),
                candidate.security_tier().as_str(),
            )),
        }
    }
    let process = process
        .ok_or_else(|| format!("all staged setup workers failed: {}", failures.join(" | ")))?;
    let staged_identity = (process.staged_plugin_sha256, process.post_setup_sha256);
    let report = String::from_utf8(process.output.stdout)
        .map_err(|error| format!("worker setup report is not UTF-8: {error}"))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&report).map_err(|error| format!("parse setup report: {error}"))?;
    let tier = selected_tier.expect("a successful process records its security tier");
    let object = value
        .as_object_mut()
        .ok_or_else(|| "setup report root is not an object".to_string())?;
    object.insert(
        "macos_security".into(),
        json!({
            "security_tier": tier.as_str(),
            "native_carrier": tier == SecurityTier::NativeCarrierTrustedOnly,
            "native_carrier_trusted_only": tier == SecurityTier::NativeCarrierTrustedOnly,
            "fallback_reasons": failures,
        }),
    );
    let report = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("serialize macOS setup report: {error}"))?;
    let (parameters, defaults) = crate::shared_descriptor::parameters_from_guest_setup(&value)?;
    Ok((parameters, defaults, report, staged_identity))
}

#[derive(Clone, Debug)]
struct GuestWorkerCandidate {
    path: PathBuf,
    native: bool,
}

impl GuestWorkerCandidate {
    fn security_tier(&self) -> SecurityTier {
        if self.native {
            SecurityTier::NativeCarrierTrustedOnly
        } else {
            SecurityTier::UnicornGuest
        }
    }
}

fn guest_worker_candidates(repository: &Path) -> Result<Vec<GuestWorkerCandidate>, String> {
    if let Some(path) = std::env::var_os("AEXCOMPAT_GUEST_WORKER").map(PathBuf::from) {
        if path.is_file() {
            let native = thin_macho_is_x86_64(&path)?;
            if native && !native_carrier_opted_in()? {
                return Err("x86_64 AEXCOMPAT_GUEST_WORKER requires AEXCOMPAT_NATIVE_CARRIER=1 and AEXCOMPAT_NATIVE_CARRIER_TRUSTED=1".into());
            }
            return Ok(vec![GuestWorkerCandidate { path, native }]);
        }
        return Err(format!(
            "AEXCOMPAT_GUEST_WORKER does not identify a file: {}",
            path.display()
        ));
    }
    let mut candidates = Vec::new();
    if native_carrier_opted_in()? {
        candidates.push(GuestWorkerCandidate {
            path: repository.join("guest/target/x86_64-apple-darwin/release/aex-guest-worker"),
            native: true,
        });
    }
    candidates.extend([
        GuestWorkerCandidate {
            path: repository.join("guest/target/release/aex-guest-worker"),
            native: false,
        },
        GuestWorkerCandidate {
            path: repository.join("guest/target/debug/aex-guest-worker"),
            native: false,
        },
    ]);
    let existing = candidates
        .into_iter()
        .filter(|candidate| candidate.path.is_file())
        .collect::<Vec<_>>();
    if existing.is_empty() {
        Err({
            format!(
                "Build a guest worker first. Native: cargo build --release --target x86_64-apple-darwin --features native-carrier --manifest-path {}/guest/Cargo.toml -p aex-guest-worker; Unicorn fallback: cargo build --release --manifest-path {}/guest/Cargo.toml -p aex-guest-worker",
                repository.display(),
                repository.display()
            )
        })
    } else {
        Ok(existing)
    }
}

fn native_carrier_opted_in() -> Result<bool, String> {
    let enabled = std::env::var_os("AEXCOMPAT_NATIVE_CARRIER");
    let trusted = std::env::var_os("AEXCOMPAT_NATIVE_CARRIER_TRUSTED");
    native_carrier_opted_in_values(enabled.as_deref(), trusted.as_deref())
}

fn native_carrier_opted_in_values(
    enabled: Option<&std::ffi::OsStr>,
    trusted: Option<&std::ffi::OsStr>,
) -> Result<bool, String> {
    if enabled.is_none() && trusted.is_none() {
        return Ok(false);
    }
    if enabled.as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err("AEXCOMPAT_NATIVE_CARRIER must be exactly 1 when set".into());
    }
    if trusted.as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err("native carrier is trusted-plug-ins-only; set AEXCOMPAT_NATIVE_CARRIER_TRUSTED=1 to acknowledge that boundary".into());
    }
    Ok(true)
}

fn thin_macho_is_x86_64(path: &Path) -> Result<bool, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("read guest worker architecture {}: {error}", path.display()))?;
    read_thin_macho_is_x86_64(file).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            format!("guest worker is too small to be Mach-O: {}", path.display())
        } else {
            format!("read guest worker architecture {}: {error}", path.display())
        }
    })
}

fn read_thin_macho_is_x86_64(mut reader: impl Read) -> std::io::Result<bool> {
    // Architecture detection needs only magic/cputype, even for a large worker.
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    let magic = u32::from_le_bytes(bytes[0..4].try_into().expect("four-byte slice"));
    let cpu = match magic {
        0xfeedfacf => u32::from_le_bytes(bytes[4..8].try_into().expect("four-byte slice")),
        0xcffaedfe => u32::from_be_bytes(bytes[4..8].try_into().expect("four-byte slice")),
        _ => return Ok(false),
    };
    Ok(cpu == 0x0100_0007)
}

fn take_shared_resident_child(child: &SharedResidentChild) -> Option<Child> {
    match child.try_lock() {
        Ok(mut child) => child.take(),
        Err(TryLockError::Poisoned(error)) => error.into_inner().take(),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn terminate_shared_resident_child(child: &SharedResidentChild) -> bool {
    let Some(mut child) = take_shared_resident_child(child) else {
        return false;
    };
    if let Err(error) = terminate_process_group(&mut child) {
        eprintln!("aexcompat resident process-group cleanup failed: {error}");
    }
    true
}

fn resident_join_reaper() -> &'static Sender<thread::JoinHandle<()>> {
    static REAPER: OnceLock<Sender<thread::JoinHandle<()>>> = OnceLock::new();
    REAPER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<thread::JoinHandle<()>>();
        thread::Builder::new()
            .name("aexcompat-resident-thread-reaper".into())
            .spawn(move || {
                while let Ok(join) = receiver.recv() {
                    let _ = join.join();
                }
            })
            .expect("start resident thread reaper");
        sender
    })
}

fn enqueue_resident_join(join: thread::JoinHandle<()>) {
    if let Err(error) = resident_join_reaper().send(join) {
        // The receiver can disappear only after its reaper panics. Dropping
        // the recovered handle still detaches it without blocking the caller.
        drop(error.0);
    }
}

fn repository_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        starts.push(current);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            starts.push(parent.to_path_buf());
        }
    }
    for start in starts {
        for ancestor in start.ancestors() {
            if ancestor.join("guest/Cargo.toml").is_file()
                && ancestor.join("broker/Cargo.toml").is_file()
            {
                return Some(ancestor.to_path_buf());
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "macos_transport_tests.rs"]
mod transport_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use aexcompat_broker::render_fixture::FixtureCheckpoint;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    fn fixture_resident_child() -> Child {
        let mut command = Command::new("/bin/sleep");
        command.arg("5").process_group(0);
        command.spawn().unwrap()
    }

    #[test]
    fn repository_root_is_found_from_worktree() {
        assert!(repository_root().is_some());
    }

    #[test]
    fn fixture_failure_preserves_render_error_when_worker_cleanup_also_fails() {
        let error = combine_fixture_inspection_and_close::<()>(
            Err("SMART_RENDER failed: invalid world destination".into()),
            Err("worker exited unsuccessfully".into()),
        )
        .unwrap_err();

        assert_eq!(
            error,
            "SMART_RENDER failed: invalid world destination; worker cleanup: worker exited unsuccessfully"
        );
    }

    #[test]
    fn fixture_failure_reports_the_only_failed_phase() {
        assert_eq!(
            combine_fixture_inspection_and_close::<()>(Err("render failed".into()), Ok(()))
                .unwrap_err(),
            "render failed"
        );
        assert_eq!(
            combine_fixture_inspection_and_close(Ok(()), Err("cleanup failed".into())).unwrap_err(),
            "cleanup failed"
        );
        assert!(combine_fixture_inspection_and_close(Ok(7), Ok(())).is_ok());
    }

    #[test]
    fn fixture_image_encoding_is_bit_exact_at_all_depths() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-macos-fixture-pixel-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let path = root.join("pixel.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 128, 0, 64]))
            .save(&path)
            .unwrap();
        let (_, _, argb8) = encode_resident_image(&path, MacRenderFormat::PngArgb8).unwrap();
        assert_eq!(argb8, [64, 255, 128, 0]);
        let (_, _, argb16) = encode_resident_image(&path, MacRenderFormat::RawArgb16).unwrap();
        let words = argb16
            .chunks_exact(2)
            .map(|word| u16::from_le_bytes([word[0], word[1]]))
            .collect::<Vec<_>>();
        assert_eq!(words, [8224, 32768, 16448, 0]);
        let (_, _, argb32f) = encode_resident_image(&path, MacRenderFormat::ExrArgb32f).unwrap();
        let floats = argb32f
            .chunks_exact(4)
            .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(floats, [64.0 / 255.0, 1.0, 128.0 / 255.0, 0.0]);
        assert_eq!(
            argb_to_rgba_words(&argb8, 1),
            [255, 128, 0, 64],
            "artifact conversion must preserve component words"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixture_image_encoding_preserves_sixteen_bit_png_precision() {
        let root =
            std::env::temp_dir().join(format!("aexcompat-deep-input-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("deep.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            1,
            1,
            image::Rgba([20001, 7, 40001, 32769]),
        )
        .save(&path)
        .unwrap();
        let (_, _, argb) = encode_resident_image(&path, MacRenderFormat::RawArgb16).unwrap();
        assert_eq!(
            argb,
            [16385u16, 10001, 4, 20001]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        let (_, _, argb) = encode_resident_image(&path, MacRenderFormat::ExrArgb32f).unwrap();
        assert_eq!(
            argb,
            [32769.0f32, 20001.0, 7.0, 40001.0]
                .into_iter()
                .flat_map(|v| (v / 65535.0).to_le_bytes())
                .collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fixture_image_encoding_rejects_nonfinite_and_keeps_finite_hdr() {
        let root =
            std::env::temp_dir().join(format!("aexcompat-float-input-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("float.tiff");
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            image::ImageBuffer::<image::Rgba<f32>, Vec<f32>>::from_pixel(
                1,
                1,
                image::Rgba([bad, 0.0, 0.0, 1.0]),
            )
            .save(&path)
            .unwrap();
            assert!(
                encode_resident_image(&path, MacRenderFormat::ExrArgb32f)
                    .unwrap_err()
                    .contains("non-finite")
            );
        }
        image::ImageBuffer::<image::Rgba<f32>, Vec<f32>>::from_pixel(
            1,
            1,
            image::Rgba([1.25, -0.125, -0.0, 0.5]),
        )
        .save(&path)
        .unwrap();
        let (_, _, argb) = encode_resident_image(&path, MacRenderFormat::ExrArgb32f).unwrap();
        assert_eq!(
            argb,
            [0.5f32, 1.25, -0.125, -0.0]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mixed_primary_checkpoint_keeps_input_depth_and_output_case_identity() {
        let primary = CapturedWorldRecord {
            width: 1,
            height: 1,
            pixel_bytes: 8,
            rowbytes: 16,
            origin_x: 0,
            origin_y: 0,
            extent: [0, 0, 1, 1],
            raw_argb: [
                0u8, 128, 0, 16, 0, 32, 0, 48, 90, 90, 90, 90, 90, 90, 90, 90,
            ]
            .to_vec(),
        };
        let output = [255u8, 40, 60, 80];
        let (width, height, raw, observed, format) = fixture_checkpoint_source(
            "input",
            &primary,
            &output,
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            &[],
        )
        .unwrap();
        assert_eq!(format, MacRenderFormat::RawArgb16);
        assert_eq!(raw, primary.raw_argb);
        assert!(observed.is_some());
        let plugin_sha256 = "a".repeat(64);
        let fixture_sha256 = "c".repeat(64);
        let mut case = FixtureCaseIdentity {
            sha256: String::new(),
            fixture_sha256: fixture_sha256.clone(),
            plugin_sha256: plugin_sha256.clone(),
            input_asset_sha256: "d".repeat(64),
            case_index: 0,
            selections: Vec::new(),
            render_path: "classic".into(),
            pixel_format: "argb8".into(),
            checkpoints: vec![FixtureCheckpoint {
                id: "input".into(),
                stage: "classic-input".into(),
            }],
            world_layout_identity: "e".repeat(64),
        };
        case.sha256 = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    &case.fixture_sha256,
                    &case.plugin_sha256,
                    &case.input_asset_sha256,
                    case.case_index,
                    &case.selections,
                    &case.render_path,
                    &case.pixel_format,
                    &case.checkpoints,
                    &case.world_layout_identity,
                ))
                .unwrap()
            )
        );
        case.validate().unwrap();
        let conditions = fixture_conditions(
            &plugin_sha256,
            &"b".repeat(64),
            raw,
            MacRenderFormat::PngArgb8.pixel_format(),
            "classic",
            &json!([]),
            "straight",
            0,
            1,
            1,
            1,
            Some(&case),
        );
        let root = std::env::temp_dir().join(format!(
            "aexcompat-mac-mixed-checkpoint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let metadata = write_strided_world_checkpoint_artifact(
            &root,
            raw,
            width,
            height,
            format.artifact_format(),
            primary.rowbytes,
            0,
            0,
            primary.extent,
            conditions,
            "input",
            "classic-input",
            &fixture_sha256,
        )
        .unwrap();
        assert_eq!(metadata["pixel_format"], "argb16");
        assert_eq!(metadata["comparison_identity"]["pixel_format"], "argb8");
        assert_eq!(
            metadata["comparison_identity"]["fixture_case"]["sha256"],
            case.sha256
        );
        assert_eq!(
            metadata["comparison_identity"]["fixture_case"]["pixel_format"],
            "argb8"
        );
        assert_eq!(metadata["rowbytes"], 16);
        assert_eq!(std::fs::read(root.join("output.bin")).unwrap(), raw);
        let (_, _, final_raw, _, final_format) = fixture_checkpoint_source(
            "output",
            &primary,
            &output,
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            &[],
        )
        .unwrap();
        assert_eq!(final_format, MacRenderFormat::PngArgb8);
        assert_eq!(final_raw, output);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fixture_parameter(slot: u32, kind: &str, value: f64) -> InteractiveParameter {
        InteractiveParameter {
            slot,
            name: format!("parameter-{slot}"),
            kind: kind.into(),
            minimum: 0.0,
            maximum: 255.0,
            value,
            choices: Vec::new(),
            use_declared_default: false,
            color_float: None,
            color: [255, 1, 2, 3],
            components: [1.5, 2.5, 3.5],
            component_count: match kind {
                "angle" => 1,
                "point" => 2,
                "point3d" => 3,
                _ => 0,
            },
            layer_path: (kind == "layer").then(|| PathBuf::from("secondary.png")),
            enabled: true,
            visible: true,
            supervised: false,
            debug_summary: None,
            custom_ui_events: 0,
            control_size: [0; 2],
        }
    }

    #[test]
    fn fixture_payload_preserves_scalar_types_and_excludes_layers() {
        let payload = fixture_parameter_payload(&[
            fixture_parameter(1, "layer", 0.0),
            fixture_parameter(2, "integer", 12.0),
            fixture_parameter(3, "float", 2.5),
            fixture_parameter(4, "color", 0.0),
        ])
        .unwrap();
        assert_eq!(
            payload.transport,
            "v2|param_2@2:i32=12;param_3@3:f64=2.5;param_4@4:argb8=255,1,2,3"
        );
        assert_eq!(
            payload.identity,
            json!([
                {"id":"param_2","slot":2,"kind":"integer","value":12},
                {"id":"param_3","slot":3,"kind":"float","value":2.5},
                {"id":"param_4","slot":4,"kind":"color","value":{
                    "alpha":255,"red":1,"green":2,"blue":3
                }}
            ])
        );
    }

    #[test]
    fn fixture_payload_carries_one_based_popup_choice() {
        let mut popup = fixture_parameter(3, "popup", 2.0);
        popup.minimum = 1.0;
        popup.maximum = 2.0;
        popup.choices = vec!["Base".into(), "Alternate".into()];
        let payload = fixture_parameter_payload(&[popup.clone()]).unwrap();
        assert_eq!(payload.transport, "v2|param_3@3:i32=2");
        assert_eq!(
            payload.identity,
            json!([{"id":"param_3","slot":3,"kind":"popup","value":2}])
        );
        popup.value = 0.0;
        assert!(fixture_parameter_payload(&[popup.clone()]).is_err());
        popup.value = 3.0;
        assert!(fixture_parameter_payload(&[popup.clone()]).is_err());
        popup.value = 1.5;
        assert!(fixture_parameter_payload(&[popup]).is_err());
    }

    #[test]
    fn fixture_payload_preserves_angle_point_and_point3d_components() {
        let payload = fixture_parameter_payload(&[
            fixture_parameter(1, "angle", 0.0),
            fixture_parameter(2, "point", 0.0),
            fixture_parameter(3, "point3d", 0.0),
        ])
        .unwrap();
        assert_eq!(
            payload.transport,
            "v4|param_1@1:angle=1.5;param_2@2:point=1.5,2.5;param_3@3:point3d=1.5,2.5,3.5"
        );
        assert_eq!(
            payload.identity,
            json!([
                {"id":"param_1","slot":1,"kind":"angle","value":[1.5]},
                {"id":"param_2","slot":2,"kind":"point","value":[1.5,2.5]},
                {"id":"param_3","slot":3,"kind":"point3d","value":[1.5,2.5,3.5]}
            ])
        );
    }

    #[test]
    fn masked_fixture_is_rejected_before_guest_launch_or_output_staging() {
        let directory = std::env::temp_dir().join(format!(
            "aexcompat-macos-mask-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture_path = directory.join("fixture.json");
        let output = directory.join("output");
        // V2 hashes the asset while loading; rendering must reject masks before
        // trying to decode these bytes or load the deliberately absent plug-in.
        std::fs::write(directory.join("primary.png"), b"asset identity").unwrap();
        for version in [1, 2] {
            for masks in [
                json!([]),
                json!([{"open":false,"vertices":[
                    {"x":0,"y":0},{"x":16,"y":0},{"x":0,"y":16}
                ]}]),
            ] {
                let fixture = json!({
                    "schema":"aexcompat.render_fixture","schema_version":version,
                    "primary_layer":"primary.png","parameters":[],
                    "pixel_format":"argb8","render_path":"smart","premultiplication":"straight",
                    "timing":{"current_time":0,"time_step":1,"total_time":1,"time_scale":1},
                    "final_artifact":"raw","checkpoints":[{"id":"output","stage":"smart-output"}],
                    "mask_scene":{"masks":masks}
                });
                std::fs::write(&fixture_path, serde_json::to_vec(&fixture).unwrap()).unwrap();
                let error = render_fixture_headless(
                    &directory,
                    &directory.join("absent.aex"),
                    &fixture_path,
                    &output,
                )
                .unwrap_err();
                assert!(
                    error.contains("fixture mask_scene is unsupported on this platform"),
                    "{error}"
                );
                assert!(!output.exists());
                assert_eq!(
                    std::fs::read_dir(&directory).unwrap().count(),
                    2,
                    "unexpected staged output"
                );
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fixture_render_request_carries_complete_timing() {
        let request = fixture_render_request(
            &FixtureTiming {
                current_time: 42,
                time_step: 7,
                total_time: 210,
                time_scale: 30,
            },
            "v2|",
        );
        assert_eq!(request["v"], 4);
        assert_eq!(
            request["current_time"],
            json!({"value":42,"step":7,"total":210,"scale":30})
        );
        assert_eq!(request["parameters"], "v2|");
    }

    #[test]
    fn native_carrier_requires_explicit_trusted_plugin_acknowledgement() {
        let one = std::ffi::OsStr::new("1");
        assert_eq!(native_carrier_opted_in_values(None, None), Ok(false));
        assert!(native_carrier_opted_in_values(Some(one), None).is_err());
        assert!(native_carrier_opted_in_values(None, Some(one)).is_err());
        assert_eq!(
            native_carrier_opted_in_values(Some(one), Some(one)),
            Ok(true)
        );
    }

    #[test]
    fn explicit_worker_architecture_detects_thin_x86_64() {
        let session = WorkerSession::create().unwrap();
        let x86 = session.root().join("x86-worker");
        let arm = session.root().join("arm-worker");
        std::fs::write(&x86, [0xcf, 0xfa, 0xed, 0xfe, 0x07, 0x00, 0x00, 0x01]).unwrap();
        std::fs::write(&arm, [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01]).unwrap();
        assert!(thin_macho_is_x86_64(&x86).unwrap());
        assert!(!thin_macho_is_x86_64(&arm).unwrap());
    }

    #[test]
    fn explicit_worker_architecture_preserves_header_classification() {
        for (header, expected) in [
            ([0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1], true),
            ([0xfe, 0xed, 0xfa, 0xcf, 1, 0, 0, 7], true),
            ([0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1], false),
            ([0xfe, 0xed, 0xfa, 0xcf, 1, 0, 0, 12], false),
            // 32-bit thin, FAT, FAT64 and unknown magic remain unsupported.
            ([0xce, 0xfa, 0xed, 0xfe, 7, 0, 0, 0], false),
            ([0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 1], false),
            ([0xbe, 0xba, 0xfe, 0xca, 1, 0, 0, 0], false),
            ([0xca, 0xfe, 0xba, 0xbf, 0, 0, 0, 1], false),
            ([0xbf, 0xba, 0xfe, 0xca, 1, 0, 0, 0], false),
            ([0; 8], false),
        ] {
            assert_eq!(read_thin_macho_is_x86_64(&header[..]).unwrap(), expected);
        }
    }

    #[test]
    fn explicit_worker_architecture_preserves_short_and_io_errors() {
        let session = WorkerSession::create().unwrap();
        let path = session.root().join("worker");
        assert!(
            thin_macho_is_x86_64(&path)
                .unwrap_err()
                .starts_with("read guest worker architecture ")
        );
        let header = [0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1];
        for length in 0..header.len() {
            std::fs::write(&path, &header[..length]).unwrap();
            assert_eq!(
                thin_macho_is_x86_64(&path).unwrap_err(),
                format!("guest worker is too small to be Mach-O: {}", path.display())
            );
        }
        // A directory cannot be read as a worker file, including as root.
        assert!(
            thin_macho_is_x86_64(session.root())
                .unwrap_err()
                .starts_with("read guest worker architecture ")
        );
        struct Denied;
        impl Read for Denied {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::PermissionDenied.into())
            }
        }
        assert_eq!(
            read_thin_macho_is_x86_64(Denied).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn explicit_worker_architecture_reads_only_header_with_partial_reads() {
        struct HeaderOnly {
            remaining: &'static [u8],
            interrupted: bool,
        }
        impl Read for HeaderOnly {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                assert!(!buffer.is_empty());
                assert!(buffer.len() <= self.remaining.len(), "read beyond header");
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                let count = buffer.len().min(3);
                buffer[..count].copy_from_slice(&self.remaining[..count]);
                self.remaining = &self.remaining[count..];
                Ok(count)
            }
        }
        let mut reader = HeaderOnly {
            remaining: &[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1],
            interrupted: false,
        };
        assert!(read_thin_macho_is_x86_64(&mut reader).unwrap());
        assert!(reader.remaining.is_empty());
    }

    #[test]
    fn worker_resolution_accepts_the_workspace_debug_binary() {
        let repository = repository_root().unwrap();
        if repository
            .join("guest/target/debug/aex-guest-worker")
            .is_file()
        {
            assert!(guest_worker_candidates(&repository).is_ok());
        }
    }

    #[test]
    fn resident_parameter_payload_preserves_slots_and_numeric_kinds() {
        let mut amount = fixture_parameter(1, "float", 50.25);
        amount.name = "Amount".into();
        amount.maximum = 100.0;
        let mut legacy = fixture_parameter(2, "integer", 1.0);
        legacy.name = "Legacy".into();
        legacy.maximum = 1.0;
        let parameters = [amount, legacy];
        assert_eq!(
            parameter_payload(&parameters).unwrap(),
            "v2|param_1@1:f64=50.25;param_2@2:i32=1"
        );
    }

    #[test]
    fn headless_description_preserves_canonical_records_and_rejects_ambiguous_slots() {
        let mut current = fixture_parameter(2, "float", 75.0);
        current.debug_summary = Some("/private/arbitrary-state".into());
        let default = fixture_parameter(2, "float", 50.0);
        let report = description_packet(
            &"a".repeat(64),
            Some(&"b".repeat(64)),
            vec![current.clone()],
            vec![default.clone()],
        )
        .unwrap();
        assert_eq!(report["schema"], "aexcompat.macos_aex_description");
        assert_eq!(report["plugin_identity"]["files_unchanged"], false);
        assert_eq!(report["parameters"][0]["value"], 75.0);
        assert_eq!(report["defaults"][0]["value"], 50.0);
        assert_eq!(report["parameters"][0]["slot"], 2);
        assert_eq!(report["parameters"][0]["layer_path"], Value::Null);
        assert_eq!(report["parameters"][0]["debug_summary"], Value::Null);
        assert!(
            description_packet(
                &"a".repeat(64),
                Some(&"a".repeat(64)),
                vec![current.clone(), current],
                vec![default.clone(), default],
            )
            .is_err()
        );
        let mut leaking_name = fixture_parameter(4, "float", 1.0);
        leaking_name.name = "Cache: /Users/private/plugin.aex".into();
        assert!(
            description_packet(
                &"a".repeat(64),
                None,
                vec![leaking_name.clone()],
                vec![leaking_name],
            )
            .is_err()
        );
        let mut leaking_choice = fixture_parameter(5, "integer", 0.0);
        leaking_choice.choices = vec!["C:\\Users\\private\\plugin.aex".into()];
        assert!(
            description_packet(
                &"a".repeat(64),
                None,
                vec![leaking_choice.clone()],
                vec![leaking_choice],
            )
            .is_err()
        );
        assert!(contains_absolute_path("\\\\server\\share"));
        assert!(contains_absolute_path("Cache;/Users/private/plugin.aex"));
        assert!(!contains_absolute_path("Red/Green"));
        assert!(!contains_absolute_path("Input / Output"));
        let unknown_identity = description_packet(
            &"a".repeat(64),
            None,
            vec![fixture_parameter(6, "float", 1.0)],
            vec![fixture_parameter(6, "float", 1.0)],
        )
        .unwrap();
        assert_eq!(
            unknown_identity["plugin_identity"]["files_unchanged"],
            Value::Null
        );
        let mut path_parameter = fixture_parameter(3, "layer", 0.0);
        path_parameter.layer_path = Some(PathBuf::from("/private/other-layer.png"));
        assert!(
            description_packet(
                &"a".repeat(64),
                Some(&"a".repeat(64)),
                vec![path_parameter.clone()],
                vec![path_parameter],
            )
            .is_err()
        );
    }

    #[test]
    fn resident_parameter_payload_encodes_slot_qualified_argb8() {
        let mut color = fixture_parameter(2, "color", 0.0);
        color.name = "Color".into();
        color.color = [255, 64, 128, 192];
        let parameters = [color];
        assert_eq!(
            parameter_payload(&parameters).unwrap(),
            "v2|param_2@2:argb8=255,64,128,192"
        );
    }

    #[test]
    fn setup_discovery_exposes_color_parameter_to_gui() {
        let (parameters, defaults) =
            crate::shared_descriptor::parameters_from_guest_setup(&json!({
                "parameters": [{
                    "slot": 2,
                    "param_type": 5,
                    "name": "Color",
                    "current_color": [255, 64, 128, 192],
                    "default_color": [255, 0, 0, 0]
                }]
            }))
            .unwrap();
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].slot, 2);
        assert_eq!(parameters[0].kind, "color");
        assert_eq!(parameters[0].color, [255, 64, 128, 192]);
        assert_eq!(defaults[0].color, [255, 0, 0, 0]);
    }

    #[test]
    fn setup_discovery_preserves_complete_effect_control_descriptors() {
        let descriptor = |slot: u32, param_type: i64, name: &str| {
            json!({
                "slot": slot,
                "param_type": param_type,
                "name": name,
                "ui_flags": if name == "hidden" { 1 << 9 } else { 0 },
                "flags": if name == "button" { 1 << 6 } else { 0 },
                "valid_min": 0.0,
                "valid_max": 100.0,
                "slider_min": 0.0,
                "slider_max": 100.0,
                "default_value": 1.0,
                "current_value": 2.0,
                "choices": if param_type == 7 { "One|Two" } else { "" },
                "default_components": if param_type == 3 { json!([10.0]) } else if param_type == 6 { json!([10.0, 20.0]) } else if param_type == 18 { json!([10.0, 20.0, 30.0]) } else { Value::Null },
                "current_components": if param_type == 3 { json!([11.0]) } else if param_type == 6 { json!([11.0, 21.0]) } else if param_type == 18 { json!([11.0, 21.0, 31.0]) } else { Value::Null },
                "arbitrary_summary": if param_type == 11 { "bounded summary" } else { "" }
            })
        };
        let types = [
            (0, "layer"),
            (1, "integer"),
            (2, "float"),
            (3, "angle"),
            (4, "integer"),
            (6, "point"),
            (7, "integer"),
            (8, "custom"),
            (9, "no_data"),
            (10, "float"),
            (11, "arbitrary_data"),
            (12, "path"),
            (13, "group_start"),
            (14, "group_end"),
            (15, "button"),
            (18, "point3d"),
        ];
        let rows = types
            .iter()
            .enumerate()
            .map(|(index, (param_type, _))| {
                descriptor(
                    index as u32 + 1,
                    *param_type,
                    if index == 1 {
                        "hidden"
                    } else if *param_type == 15 {
                        "button"
                    } else {
                        "p"
                    },
                )
            })
            .collect::<Vec<_>>();
        let (current, defaults) =
            crate::shared_descriptor::parameters_from_guest_setup(&json!({"parameters": rows}))
                .unwrap();

        assert_eq!(
            current
                .iter()
                .map(|parameter| parameter.kind.as_str())
                .collect::<Vec<_>>(),
            types.iter().map(|(_, kind)| *kind).collect::<Vec<_>>()
        );
        assert!(!current[1].visible);
        let button = current
            .iter()
            .find(|parameter| parameter.kind == "button")
            .unwrap();
        assert!(button.supervised);
        let popup = current
            .iter()
            .find(|parameter| parameter.slot == 7)
            .unwrap();
        assert_eq!(popup.choices, ["One", "Two"]);
        let point3d = current.last().unwrap();
        assert_eq!(point3d.components, [11.0, 21.0, 31.0]);
        assert_eq!(defaults.last().unwrap().components, [10.0, 20.0, 30.0]);
        assert_eq!(
            current
                .iter()
                .find(|parameter| parameter.kind == "arbitrary_data")
                .unwrap()
                .debug_summary
                .as_deref(),
            Some("bounded summary")
        );
    }

    #[test]
    fn resident_reader_treats_worker_eof_as_session_invalidation() {
        assert!(read_control_message(&mut &[][..]).unwrap().is_none());
    }

    #[test]
    fn resident_reader_accepts_large_setup_but_enforces_per_message_and_session_bounds() {
        let payload = serde_json::to_vec(&json!({"setup": "x".repeat(400 * 1024)})).unwrap();
        assert!(payload.len() > 64 * 1024);
        assert!(payload.len() <= MAX_RESIDENT_RESPONSE_BYTES);
        let mut framed = Vec::new();
        framed.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        framed.extend_from_slice(&payload);

        let mut total = 0;
        assert!(
            read_control_message_accounted(&mut &framed[..], &mut total)
                .unwrap()
                .is_some()
        );

        let accepted_messages = MAX_RESIDENT_PROTOCOL_BYTES / framed.len();
        let mut messages = framed.repeat(accepted_messages + 1);
        let mut reader = &messages[..];
        let mut total = 0;
        for _ in 0..accepted_messages {
            assert!(
                read_control_message_accounted(&mut reader, &mut total)
                    .unwrap()
                    .is_some()
            );
        }
        assert_eq!(total, accepted_messages * framed.len());
        assert!(total <= MAX_RESIDENT_PROTOCOL_BYTES);
        assert!(
            read_control_message_accounted(&mut reader, &mut total)
                .unwrap_err()
                .contains("resident responses exceeded")
        );

        // Account for the four-byte prefix at the exact session boundary.
        let mut total = MAX_RESIDENT_PROTOCOL_BYTES - framed.len();
        assert!(
            read_control_message_accounted(&mut &framed[..], &mut total)
                .unwrap()
                .is_some()
        );
        assert_eq!(total, MAX_RESIDENT_PROTOCOL_BYTES);
        let mut total = MAX_RESIDENT_PROTOCOL_BYTES - framed.len() + 1;
        assert!(
            read_control_message_accounted(&mut &framed[..], &mut total)
                .unwrap_err()
                .contains("resident responses exceeded")
        );

        messages[..4].copy_from_slice(&((MAX_RESIDENT_RESPONSE_BYTES + 1) as u32).to_le_bytes());
        assert!(
            read_control_message(&mut &messages[..])
                .unwrap_err()
                .contains("response length is invalid")
        );
    }

    #[test]
    fn resident_frame_response_is_strict_and_generation_bound() {
        let valid = json!({
            "v": 1,
            "type": "frame_done",
            "frame_index": 0,
            "status": "ok",
            "output": {
                "width": 2,
                "height": 1,
                "rowbytes": 8,
                "pixel_format": "argb8",
                "render_path": "classic",
                "checksum": "0".repeat(64),
                "guards_intact": true
            },
            "render_error": 0,
            "generation": 1,
            "timings_us": {
                "request_prepare": 11,
                "input_read": 12,
                "effect_render": 13,
                "output_write": 14,
                "checksum": 15
            }
        });
        assert!(validate_resident_frame(&valid, 0, 2, 1, MacRenderFormat::PngArgb8).is_ok());
        let mut geometry = valid.clone();
        geometry["output"]["rect"] = json!([0, 0, 2, 1]);
        assert!(validate_resident_frame(&geometry, 0, 2, 1, MacRenderFormat::PngArgb8).is_ok());
        for rect in [[1, 0, 3, 1], [0, -1, 2, 0], [0, 0, 1, 1], [0, 0, 2, 2]] {
            geometry["output"]["rect"] = json!(rect);
            assert!(
                validate_resident_frame(&geometry, 0, 2, 1, MacRenderFormat::PngArgb8).is_err()
            );
        }
        let mut legacy = valid.clone();
        legacy.as_object_mut().unwrap().remove("timings_us");
        assert!(validate_resident_frame(&legacy, 0, 2, 1, MacRenderFormat::PngArgb8).is_ok());
        let mut stale = valid.clone();
        stale["generation"] = json!(0);
        assert!(validate_resident_frame(&stale, 0, 2, 1, MacRenderFormat::PngArgb8).is_err());
        let mut unknown = valid;
        unknown["unexpected"] = json!(true);
        assert!(validate_resident_frame(&unknown, 0, 2, 1, MacRenderFormat::PngArgb8).is_err());
    }

    #[test]
    fn resident_argb16_raw_export_preserves_words_and_rejects_invalid_output() {
        let root =
            std::env::temp_dir().join(format!("aexcompat-raw16-export-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let slot = root.join("slot");
        let artifact = root.join("artifact");
        let bytes = [32768u16, 10001, 4, 65535]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        std::fs::write(&slot, &bytes).unwrap();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let save = |checksum: &str| {
            save_argb16_raw_slot(
                &slot,
                &artifact,
                1,
                1,
                &"a".repeat(64),
                &"b".repeat(64),
                "[]",
                "smartfx",
                checksum,
            )
        };
        assert!(
            save(&"0".repeat(64))
                .unwrap_err()
                .contains("checksum mismatch")
        );
        assert!(!artifact.exists());
        std::fs::write(&slot, &bytes[..7]).unwrap();
        assert!(save(&hash).unwrap_err().contains("size mismatch"));
        assert!(!artifact.exists());
        std::fs::write(&slot, &bytes).unwrap();
        assert_eq!(save(&hash).unwrap(), hash);
        assert_eq!(std::fs::read(artifact.join("output.bin")).unwrap(), bytes);
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(artifact.join("output.json")).unwrap()).unwrap();
        assert_eq!(metadata["pixel_format"], "argb16");
        assert_eq!(metadata["channel_order"], "ARGB");
        assert_eq!(metadata["data_sha256"], hash);
        assert!(save(&hash).is_err(), "must protect published output");
        assert_eq!(std::fs::read(artifact.join("output.bin")).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn gui_4k_admission_preserves_each_depth_and_rejects_one_past_boundary() {
        let root = std::env::temp_dir().join(format!("aexcompat-gui-4k-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("4k.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            3840,
            2160,
            image::Rgba([20001, 7, 40001, 65535]),
        )
        .save(&input)
        .unwrap();
        for format in [
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            MacRenderFormat::ExrArgb32f,
        ] {
            let (width, height, bytes) = encode_resident_image(&input, format).unwrap();
            assert_eq!((width, height), (3840, 2160));
            assert_eq!(bytes.len(), 3840 * 2160 * format.bytes_per_pixel());
            assert!(
                bytes
                    .chunks_exact(format.bytes_per_pixel())
                    .all(|pixel| pixel == &bytes[..format.bytes_per_pixel()])
            );
        }
        for (width, height) in [(3841, 1), (1, 2161)] {
            image::RgbaImage::new(width, height).save(&input).unwrap();
            assert!(validate_resident_image_dimensions(&input).is_err());
            assert!(encode_resident_image(&input, MacRenderFormat::PngArgb8).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_GUEST_WORKER; full 4K guest render"]
    fn real_gui_4k_render_at_all_depths() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let root = std::env::temp_dir().join(format!("aexcompat-real-4k-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("4k.png");
        image::RgbaImage::from_pixel(3840, 2160, image::Rgba([21, 7, 40, 255]))
            .save(&input)
            .unwrap();
        let workers = guest_worker_candidates(&repository_root().unwrap()).unwrap();
        for format in [
            MacRenderFormat::PngArgb8,
            MacRenderFormat::RawArgb16,
            MacRenderFormat::ExrArgb32f,
        ] {
            let mut session =
                start_resident_session(&workers, &aex, &input, &root, format, Vec::new()).unwrap();
            let output = if format == MacRenderFormat::PngArgb8 {
                root.join("output.png")
            } else {
                root.join(format.pixel_format())
            };
            session
                .sender
                .send(ResidentCommand::Render {
                    frame_index: 0,
                    parameters: Vec::new(),
                    output,
                })
                .unwrap();
            let result = session
                .receiver
                .recv_timeout(RESIDENT_RENDER_DEADLINE + Duration::from_secs(5))
                .unwrap()
                .unwrap();
            let report: Value = serde_json::from_str(&result.report).unwrap();
            if format == MacRenderFormat::PngArgb8 {
                assert_eq!(
                    image::image_dimensions(&result.output).unwrap(),
                    (3840, 2160)
                );
                let (_, _, bytes) = encode_resident_image(&result.output, format).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(&bytes)),
                    report["frame"]["output"]["checksum"]
                );
            } else {
                let metadata: Value = serde_json::from_slice(
                    &std::fs::read(result.output.parent().unwrap().join("output.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(metadata["width"], 3840);
                assert_eq!(metadata["height"], 2160);
                assert_eq!(
                    metadata["comparison_identity"]["world_sha256"],
                    report["frame"]["output"]["checksum"]
                );
                assert_eq!(
                    metadata["data_sha256"],
                    format!(
                        "{:x}",
                        Sha256::digest(std::fs::read(&result.output).unwrap())
                    )
                );
            }
            session.shutdown().unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_GUEST_WORKER"]
    fn real_resident_argb16_gui_export_matches_worker_world() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let root = std::env::temp_dir().join(format!("aexcompat-real16-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("input.png");
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            3,
            2,
            image::Rgba([20001, 7, 40001, 65535]),
        )
        .save(&input)
        .unwrap();
        let workers = guest_worker_candidates(&repository_root().unwrap()).unwrap();
        let mut session = start_resident_session(
            &workers,
            &aex,
            &input,
            &root,
            MacRenderFormat::RawArgb16,
            Vec::new(),
        )
        .unwrap();
        let artifact = root.join("artifact");
        session
            .sender
            .send(ResidentCommand::Render {
                frame_index: 0,
                parameters: Vec::new(),
                output: artifact.clone(),
            })
            .unwrap();
        let result = session
            .receiver
            .recv_timeout(RESIDENT_RENDER_DEADLINE)
            .unwrap()
            .unwrap();
        assert_eq!(result.output, artifact.join("output.bin"));
        assert_eq!(result.preview, Some(artifact.join("output.png")));
        let report: Value = serde_json::from_str(&result.report).unwrap();
        let bytes = std::fs::read(&result.output).unwrap();
        assert_eq!(bytes.len(), 3 * 2 * 8);
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            report["frame"]["output"]["checksum"]
        );
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(artifact.join("output.json")).unwrap()).unwrap();
        assert_eq!(metadata["pixel_format"], "argb16");
        session.shutdown().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resident_argb32f_response_and_exr_artifact_preserve_word_identity() {
        let response = json!({
            "v": 1,
            "type": "frame_done",
            "frame_index": 3,
            "status": "ok",
            "output": {
                "width": 1,
                "height": 1,
                "rowbytes": 16,
                "pixel_format": "argb32f",
                "render_path": "smartfx",
                "checksum": "a".repeat(64),
                "guards_intact": true
            },
            "render_error": 0,
            "generation": 4,
            "timings_us": {
                "request_prepare": 11,
                "input_read": 12,
                "effect_render": 13,
                "output_write": 14,
                "checksum": 15
            }
        });
        assert!(validate_resident_frame(&response, 3, 1, 1, MacRenderFormat::ExrArgb32f).is_ok());
        assert!(validate_resident_frame(&response, 3, 1, 1, MacRenderFormat::PngArgb8).is_err());

        let root = std::env::temp_dir().join(format!(
            "aexcompat-macos-exr-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let slot = root.join("output.argb32f");
        let artifact = root.join("artifact");
        let words = [0x8000_0000u32, 0x7fc1_2345, 0x0000_0001, 0x3f80_0000];
        let bytes = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        std::fs::write(&slot, &bytes).unwrap();
        let expected_checksum = format!("{:x}", Sha256::digest(&bytes));
        let rejected = root.join("rejected");
        assert!(
            save_argb32f_exr_slot(
                &slot,
                &rejected,
                1,
                1,
                &"11".repeat(32),
                &"22".repeat(32),
                "v2",
                "smartfx",
                &"00".repeat(32),
            )
            .is_err()
        );
        assert!(!rejected.exists());
        let checksum = save_argb32f_exr_slot(
            &slot,
            &artifact,
            1,
            1,
            &"11".repeat(32),
            &"22".repeat(32),
            "v2",
            "smartfx",
            &expected_checksum,
        )
        .unwrap();
        assert_eq!(checksum, expected_checksum);
        assert!(artifact.join("output.exr").is_file());
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(artifact.join("output.json")).unwrap()).unwrap();
        assert_eq!(metadata["pixel_format"], "float32");
        assert_eq!(metadata["comparison_identity"]["world_sha256"], checksum);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resident_ready_response_is_strict_and_pid_bound() {
        let ready = json!({
            "v": 1,
            "type": "session_ready",
            "worker_pid": 42,
            "setup": {},
        });
        assert!(validate_resident_ready(&ready, 42).is_ok());
        assert!(validate_resident_ready(&ready, 41).is_err());
        let mut unknown = ready;
        unknown["unexpected"] = json!(true);
        assert!(validate_resident_ready(&unknown, 42).is_err());
    }

    #[test]
    fn resident_probe_response_requires_success_and_guards() {
        let probe = json!({
            "v": 1,
            "type": "session_probed",
            "worker_pid": 42,
            "status": "ok",
            "guards_intact": true,
            "render_error": 0,
        });
        assert!(validate_resident_probe(&probe, 42, 2, 1).is_ok());
        let mut corrupted = probe;
        corrupted["guards_intact"] = json!(false);
        assert!(validate_resident_probe(&corrupted, 42, 2, 1).is_err());
    }

    #[test]
    fn resident_close_timeout_terminates_the_worker() {
        let child = fixture_resident_child();
        let started = Instant::now();
        assert!(wait_or_kill_resident_child(child, Duration::from_millis(20)).is_err());
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn resident_shutdown_preserves_normal_clean_close() {
        let (command_sender, command_receiver) = mpsc::channel();
        let (_result_sender, result_receiver) = mpsc::channel();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let join = thread::spawn(move || match command_receiver.recv().unwrap() {
            ResidentCommand::Close(reply) => reply.send(Ok(())).unwrap(),
            ResidentCommand::Render { .. } => panic!("unexpected render"),
            ResidentCommand::UserChanged { .. } => panic!("unexpected user change"),
        });
        let mut session = ResidentSessionHandle {
            staged_plugin_sha256: String::new(),
            sender: command_sender,
            receiver: result_receiver,
            next_frame: 0,
            child,
            shutdown_requested,
            join: Some(join),
        };

        session
            .shutdown_with_deadline(Duration::from_millis(100))
            .unwrap();
        assert!(session.join.is_none());
    }

    #[test]
    fn user_changed_response_is_exact_and_bound_to_worker_and_slot() {
        let response = json!({
            "v": 1,
            "type": "user_changed_done",
            "worker_pid": 42,
            "status": "ok",
            "report": {
                "slot": 2,
                "selector_error": 0,
                "parameters": [
                    {"slot": 1, "ui_flags": 1 << 5, "flags": 0},
                    {"slot": 2, "ui_flags": 1 << 9, "flags": 1 << 6}
                ]
            }
        });
        let updates = validate_user_changed(&response, 42, 2).unwrap();
        assert_eq!(
            updates,
            vec![
                DynamicParameterUi {
                    slot: 1,
                    ui_flags: 1 << 5,
                    flags: 0
                },
                DynamicParameterUi {
                    slot: 2,
                    ui_flags: 1 << 9,
                    flags: 1 << 6
                },
            ]
        );
        let mut extra = response.clone();
        extra["unexpected"] = json!(true);
        assert!(validate_user_changed(&extra, 42, 2).is_err());
        assert!(validate_user_changed(&response, 41, 2).is_err());
        assert!(validate_user_changed(&response, 42, 3).is_err());
    }

    #[test]
    fn resident_shutdown_cancels_an_active_render_before_close() {
        let (command_sender, command_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let worker_shutdown_requested = Arc::clone(&shutdown_requested);
        let child = Arc::new(Mutex::new(Some(fixture_resident_child())));
        let worker_child = Arc::clone(&child);
        let join = thread::spawn(move || {
            let (_response_sender, response_receiver) = mpsc::channel();
            match command_receiver.recv().unwrap() {
                ResidentCommand::Render { .. } => {
                    let result = recv_resident_response(
                        &response_receiver,
                        &worker_child,
                        &worker_shutdown_requested,
                        Duration::from_secs(5),
                    );
                    result_sender.send(result.map(|_| unreachable!())).unwrap();
                }
                ResidentCommand::Close(_) => panic!("render must be queued first"),
                ResidentCommand::UserChanged { .. } => {
                    panic!("user change must not replace render")
                }
            }
            match command_receiver.recv().unwrap() {
                ResidentCommand::Close(reply) => {
                    assert!(worker_child.lock().unwrap().is_none());
                    reply.send(Ok(())).unwrap();
                }
                ResidentCommand::Render { .. } => panic!("close must follow render"),
                ResidentCommand::UserChanged { .. } => {
                    panic!("user change must not replace close")
                }
            }
        });
        let mut session = ResidentSessionHandle {
            staged_plugin_sha256: String::new(),
            sender: command_sender,
            receiver: result_receiver,
            next_frame: 0,
            child,
            shutdown_requested,
            join: Some(join),
        };
        session
            .sender
            .send(ResidentCommand::Render {
                frame_index: 0,
                parameters: Vec::new(),
                output: PathBuf::new(),
            })
            .unwrap();

        let started = Instant::now();
        session
            .shutdown_with_deadline(Duration::from_millis(250))
            .unwrap();
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(
            session
                .receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
        );
    }

    #[test]
    fn resident_shutdown_terminates_a_hung_worker_without_joining_on_gui_thread() {
        let (command_sender, _command_receiver) = mpsc::channel();
        let (_result_sender, result_receiver) = mpsc::channel();
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(Some(fixture_resident_child())));
        let observer_child = Arc::clone(&child);
        let (finished_sender, finished_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        let join = thread::spawn(move || {
            // This thread cannot finish until shutdown returns and releases it.
            // Waiting on the join in shutdown would therefore fail the test
            // without relying on a tight wall-clock assertion on a busy CI VM.
            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(observer_child.lock().unwrap().is_none());
            finished_sender.send(()).unwrap();
        });
        let mut session = ResidentSessionHandle {
            staged_plugin_sha256: String::new(),
            sender: command_sender,
            receiver: result_receiver,
            next_frame: 0,
            child,
            shutdown_requested,
            join: Some(join),
        };

        let error = session
            .shutdown_with_deadline(Duration::from_millis(20))
            .unwrap_err();
        assert!(error.contains("cleanup continues in background"));
        release_sender.send(()).unwrap();
        finished_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_TEST_INPUT_PNG"]
    fn real_resident_gui_adapter_reuses_pid_and_updates_output() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let input = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_INPUT_PNG").unwrap());
        let repository = repository_root().unwrap();
        let output_directory = std::env::temp_dir().join(format!(
            "aexcompat-resident-gui-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&output_directory).unwrap();
        let mut workers = guest_worker_candidates(&repository).unwrap();
        workers.insert(
            0,
            GuestWorkerCandidate {
                path: PathBuf::from("/usr/bin/false"),
                native: true,
            },
        );
        let mut session = start_resident_session(
            &workers,
            &aex,
            &input,
            &output_directory,
            MacRenderFormat::PngArgb8,
            Vec::new(),
        )
        .unwrap();
        let parameter = |value| {
            let mut parameter = fixture_parameter(5, "integer", value);
            parameter.name = "Amount".into();
            parameter.maximum = 4000.0;
            parameter
        };
        let mut reports = Vec::new();
        let mut outputs = Vec::new();
        for (frame_index, value) in [(0, 0.0), (1, 200.0)] {
            let output = output_directory.join(format!("frame-{frame_index}.png"));
            session
                .sender
                .send(ResidentCommand::Render {
                    frame_index,
                    parameters: vec![parameter(value)],
                    output: output.clone(),
                })
                .unwrap();
            let result = session
                .receiver
                .recv_timeout(RESIDENT_RENDER_DEADLINE)
                .unwrap()
                .unwrap();
            reports.push(serde_json::from_str::<Value>(&result.report).unwrap());
            outputs.push(std::fs::read(output).unwrap());
        }
        assert_eq!(reports[0]["worker_pid"], reports[1]["worker_pid"]);
        assert_eq!(reports[0]["frame"]["generation"], 1);
        assert_eq!(reports[1]["frame"]["generation"], 2);
        assert_ne!(outputs[0], outputs[1]);
        session.shutdown().unwrap();
    }

    #[test]
    #[ignore = "requires AEXCOMPAT_TEST_AEX and AEXCOMPAT_TEST_INPUT_PNG"]
    fn real_resident_argb32f_render_commits_exr_artifact() {
        let aex = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_AEX").unwrap());
        let input = PathBuf::from(std::env::var_os("AEXCOMPAT_TEST_INPUT_PNG").unwrap());
        let repository = repository_root().unwrap();
        let output_directory = std::env::temp_dir().join(format!(
            "aexcompat-resident-exr-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&output_directory);
        std::fs::create_dir_all(&output_directory).unwrap();
        let workers = guest_worker_candidates(&repository).unwrap();
        let mut session = start_resident_session(
            &workers,
            &aex,
            &input,
            &output_directory,
            MacRenderFormat::ExrArgb32f,
            Vec::new(),
        )
        .unwrap();
        let artifact = output_directory.join("frame-0.exr-artifact");
        session
            .sender
            .send(ResidentCommand::Render {
                frame_index: 0,
                parameters: Vec::new(),
                output: artifact.clone(),
            })
            .unwrap();
        let result = session
            .receiver
            .recv_timeout(RESIDENT_RENDER_DEADLINE)
            .unwrap()
            .unwrap();
        assert_eq!(result.output, artifact.join("output.exr"));
        assert!(result.output.is_file());
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(artifact.join("output.json")).unwrap()).unwrap();
        assert_eq!(metadata["pixel_format"], "float32");
        assert_eq!(metadata["comparison_identity"]["pixel_format"], "argb32f");
        session.shutdown().unwrap();
        let _ = std::fs::remove_dir_all(output_directory);
    }
}
