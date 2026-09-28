use crate::classic::{
    ClassicError, ClassicHost, PARAM_ANGLE, PARAM_COLOR, PARAM_LAYER, PARAM_POINT, PARAM_POINT3D,
    ParameterValue, RenderReport, ResidentFailureDiagnostic, ResidentLayer, ResidentWorldLayout,
    SetupReport, UserChangedReport,
};
use crate::pe::PeImage;
use crate::pixel::FramePixelFormat;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_CONTROL_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_PARAMETER_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentLayerManifest {
    v: u32,
    #[serde(default)]
    primary: Option<ResidentWorldLayout>,
    #[serde(default)]
    primary_pixel_format: Option<String>,
    layers: Vec<ResidentLayerEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResidentLayerEntry {
    slot: usize,
    width: u32,
    height: u32,
    path: PathBuf,
    #[serde(default)]
    pixel_format: Option<String>,
    #[serde(default)]
    layout: Option<ResidentWorldLayout>,
}

struct OwnedResidentLayer {
    slot: usize,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    format: FramePixelFormat,
    layout: Option<ResidentWorldLayout>,
}

#[derive(Debug)]
pub enum SessionError {
    Io(String),
    Protocol(String),
    Classic(ClassicError),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::Protocol(message) => formatter.write_str(message),
            Self::Classic(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<ClassicError> for SessionError {
    fn from(error: ClassicError) -> Self {
        Self::Classic(error)
    }
}

#[derive(Serialize)]
struct FrameOutput {
    width: u32,
    height: u32,
    rowbytes: u32,
    pixel_format: &'static str,
    render_path: &'static str,
    checksum: String,
    guards_intact: bool,
}

#[derive(Serialize)]
struct FrameDone {
    v: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    frame_index: u64,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<FrameOutput>,
    render_error: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timings_us: Option<FrameTimingsUs>,
}

#[derive(Default, Serialize)]
struct FrameTimingsUs {
    request_prepare: u64,
    input_read: u64,
    effect_render: u64,
    output_write: u64,
    checksum: u64,
}

#[derive(Serialize)]
struct SessionReady<'a> {
    v: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    worker_pid: u32,
    setup: &'a SetupReport,
}

#[derive(Serialize)]
struct SessionProbed {
    v: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    worker_pid: u32,
    status: &'static str,
    guards_intact: bool,
    render_error: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<ResidentFailureDiagnostic>,
}

#[derive(Serialize)]
struct SessionClosed<'a> {
    v: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    worker_pid: u32,
    setup: &'a SetupReport,
    close: Value,
}

#[derive(Serialize)]
struct UserChangedDone<'a> {
    v: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    worker_pid: u32,
    status: &'static str,
    report: &'a UserChangedReport,
}

pub fn run_resident_session(
    image: &PeImage,
    input_slot: &Path,
    output_slot: &Path,
    width: u32,
    height: u32,
    time_scale: u32,
    pixel_format: FramePixelFormat,
    effect_selector: Option<&str>,
    fixture_layers: Option<&Path>,
    fixture_smart: Option<bool>,
    mut request: impl Read,
    mut response: impl Write,
) -> Result<(), SessionError> {
    let (primary_layout, input_format, layers) =
        load_resident_layers(input_slot, fixture_layers, pixel_format)?;
    let input_pixel_bytes = input_format
        .byte_count(width, height)
        .map_err(|error| SessionError::Protocol(error.to_string()))?;
    let mut host = ClassicHost::new_with_effect(image, effect_selector)?;
    let setup = host.begin_resident_session(width, height, time_scale)?;
    if fixture_smart == Some(true) && setup.out_flags2 & (1 << 10) == 0 {
        return Err(SessionError::Protocol(
            "fixture requested Smart Render but the AEX did not advertise it".into(),
        ));
    }
    for layer in &layers {
        let Some(parameter) = setup
            .parameters
            .iter()
            .find(|parameter| parameter.slot == layer.slot)
        else {
            return Err(SessionError::Protocol(format!(
                "fixture layer slot {} was not declared by the AEX",
                layer.slot
            )));
        };
        if parameter.param_type != PARAM_LAYER {
            return Err(SessionError::Protocol(format!(
                "fixture layer slot {} is not a layer parameter",
                layer.slot
            )));
        }
    }
    write_message(
        &mut response,
        &SessionReady {
            v: 1,
            kind: "session_ready",
            worker_pid: std::process::id(),
            setup: &setup,
        },
    )?;
    let report_frame_timings = std::env::var_os("AEXCOMPAT_RESIDENT_TIMINGS").is_some();
    let mut generation = 0u64;
    let processing = (|| -> Result<(), SessionError> {
        loop {
            let Some(message) = read_message(&mut request)? else {
                return Ok(());
            };
            let value: Value = serde_json::from_slice(&message).map_err(|error| {
                SessionError::Protocol(format!("invalid request JSON: {error}"))
            })?;
            let object = value
                .as_object()
                .ok_or_else(|| SessionError::Protocol("request must be a JSON object".into()))?;
            match object.get("type").and_then(Value::as_str) {
                Some("probe") => {
                    require_exact_keys(object.keys().map(String::as_str), &["type", "v"])?;
                    if object.get("v").and_then(Value::as_u64) != Some(1) {
                        return Err(SessionError::Protocol(
                            "probe requires protocol version 1".into(),
                        ));
                    }
                    let input = fs::read(input_slot).map_err(|error| {
                        SessionError::Io(format!("read resident probe input slot: {error}"))
                    })?;
                    if input.len() != input_pixel_bytes {
                        return Err(SessionError::Protocol(format!(
                            "resident probe input slot has {} bytes, expected {input_pixel_bytes}",
                            input.len()
                        )));
                    }
                    let borrowed_layers = layers
                        .iter()
                        .map(|layer| ResidentLayer {
                            slot: layer.slot,
                            width: layer.width,
                            height: layer.height,
                            pixels: &layer.pixels,
                            format: layer.format,
                            layout: layer.layout,
                        })
                        .collect::<Vec<_>>();
                    let probe = match fixture_smart {
                        Some(smart) => host.probe_resident_fixture_pixels(
                            width,
                            height,
                            time_scale,
                            pixel_format,
                            input_format,
                            &input,
                            &borrowed_layers,
                            smart,
                            primary_layout,
                        ),
                        None => host.probe_resident_pixels(
                            width,
                            height,
                            time_scale,
                            pixel_format,
                            &input,
                        ),
                    };
                    match probe {
                        Ok(report) => write_message(
                            &mut response,
                            &SessionProbed {
                                v: 1,
                                kind: "session_probed",
                                worker_pid: std::process::id(),
                                status: "ok",
                                guards_intact: report.guards_intact,
                                render_error: 0,
                                failure: None,
                            },
                        )?,
                        Err(error) => {
                            let render_error = error.selector_error_code().unwrap_or(-40);
                            let failure =
                                host.resident_failure_diagnostic("admission_probe", &error);
                            write_message(
                                &mut response,
                                &SessionProbed {
                                    v: 1,
                                    kind: "session_probed",
                                    worker_pid: std::process::id(),
                                    status: "error",
                                    guards_intact: false,
                                    render_error,
                                    failure: Some(failure),
                                },
                            )?;
                            return Err(SessionError::Classic(error));
                        }
                    }
                }
                Some("close") => {
                    require_exact_keys(object.keys().map(String::as_str), &["type", "v"])?;
                    if object.get("v").and_then(Value::as_u64) != Some(1) {
                        return Err(SessionError::Protocol(
                            "close requires protocol version 1".into(),
                        ));
                    }
                    return Ok(());
                }
                Some("render_frame") => {
                    parse_render_frame(
                        object,
                        &setup,
                        time_scale,
                        input_slot,
                        output_slot,
                        width,
                        height,
                        pixel_format,
                        input_pixel_bytes,
                        input_format,
                        &layers,
                        primary_layout,
                        fixture_smart,
                        &mut host,
                        &mut generation,
                        report_frame_timings,
                        &mut response,
                    )?;
                }
                Some("user_changed_param") => {
                    let (slot, parameters) = parse_user_changed_request(object, &setup)?;
                    host.apply_resident_parameter_values(&parameters)?;
                    let report = host.user_changed_parameter(slot)?;
                    write_message(
                        &mut response,
                        &UserChangedDone {
                            v: 1,
                            kind: "user_changed_done",
                            worker_pid: std::process::id(),
                            status: "ok",
                            report: &report,
                        },
                    )?;
                }
                Some(other) => {
                    return Err(SessionError::Protocol(format!(
                        "unsupported request type {other:?}"
                    )));
                }
                None => {
                    return Err(SessionError::Protocol("request has no string type".into()));
                }
            }
        }
    })();

    let close = host.close_resident_session();
    host.flush_guest_console_diagnostics()?;
    let mut close_value = serde_json::to_value(&close)
        .map_err(|error| SessionError::Protocol(format!("serialize close report: {error}")))?;
    bound_session_close(&setup, std::process::id(), &mut close_value)?;
    write_message(
        &mut response,
        &SessionClosed {
            v: 1,
            kind: "session_closed",
            worker_pid: std::process::id(),
            setup: &setup,
            close: close_value,
        },
    )?;
    processing?;
    if !close.session_clean {
        return Err(SessionError::Protocol(
            "resident session cleanup returned an error".into(),
        ));
    }
    Ok(())
}

fn parse_user_changed_request(
    object: &serde_json::Map<String, Value>,
    setup: &SetupReport,
) -> Result<(usize, Vec<ParameterValue>), SessionError> {
    require_exact_keys(
        object.keys().map(String::as_str),
        &["parameters", "slot", "type", "v"],
    )?;
    if object.get("v").and_then(Value::as_u64) != Some(1) {
        return Err(SessionError::Protocol(
            "user_changed_param requires protocol version 1".into(),
        ));
    }
    let slot = object
        .get("slot")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|slot| *slot > 0)
        .ok_or_else(|| {
            SessionError::Protocol("user_changed_param slot must be a positive usize".into())
        })?;
    let payload = object
        .get("parameters")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            SessionError::Protocol("user_changed_param parameters must be a string".into())
        })?;
    Ok((slot, parse_parameter_payload(payload, setup)?))
}

fn bound_session_close(
    setup: &SetupReport,
    worker_pid: u32,
    close: &mut Value,
) -> Result<(), SessionError> {
    let fits = |close: &Value| {
        serde_json::to_vec(&SessionClosed {
            v: 1,
            kind: "session_closed",
            worker_pid,
            setup,
            close: close.clone(),
        })
        .map(|payload| payload.len() <= MAX_CONTROL_RESPONSE_BYTES)
        .map_err(|error| SessionError::Protocol(format!("serialize close response: {error}")))
    };
    if fits(close)? {
        return Ok(());
    }

    if let Some(snapshot) = close.pointer_mut("/global_setdown_diagnostic/crash_snapshot") {
        *snapshot =
            serde_json::json!({"truncated": true, "reason": "resident control message budget"});
    }
    for pointer in [
        "/global_setdown_diagnostic/suite_requests",
        "/global_setdown_diagnostic/unsupported_suite_calls",
        "/suite_requests",
        "/unsupported_suite_calls",
    ] {
        if fits(close)? {
            return Ok(());
        }
        if let Some(value) = close.pointer_mut(pointer) {
            *value = serde_json::Value::Array(Vec::new());
        }
    }
    if fits(close)? {
        Ok(())
    } else {
        Err(SessionError::Protocol(
            "resident close response exceeds the control bound after diagnostic truncation".into(),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_render_frame(
    object: &serde_json::Map<String, Value>,
    setup: &SetupReport,
    time_scale: u32,
    input_slot: &Path,
    output_slot: &Path,
    width: u32,
    height: u32,
    pixel_format: FramePixelFormat,
    input_pixel_bytes: usize,
    input_format: FramePixelFormat,
    fixture_layers: &[OwnedResidentLayer],
    primary_layout: Option<ResidentWorldLayout>,
    fixture_smart: Option<bool>,
    host: &mut ClassicHost,
    generation: &mut u64,
    report_timings: bool,
    response: &mut impl Write,
) -> Result<(), SessionError> {
    let request_started = report_timings.then(Instant::now);
    let version = object
        .get("v")
        .and_then(Value::as_u64)
        .ok_or_else(|| SessionError::Protocol("render_frame has no integer v".into()))?;
    let allowed = if version == 1 {
        &["current_time", "frame_index", "type", "v"][..]
    } else if matches!(version, 2 | 3 | 4) {
        &["current_time", "frame_index", "parameters", "type", "v"][..]
    } else {
        return Err(SessionError::Protocol(format!(
            "unsupported render_frame version {version}"
        )));
    };
    require_exact_keys(object.keys().map(String::as_str), allowed)?;
    let frame_index = object
        .get("frame_index")
        .and_then(Value::as_u64)
        .ok_or_else(|| SessionError::Protocol("render_frame has no frame_index".into()))?;
    let current_time = object
        .get("current_time")
        .and_then(Value::as_object)
        .ok_or_else(|| SessionError::Protocol("render_frame has no current_time object".into()))?;
    let time_keys = if version == 4 {
        &["scale", "step", "total", "value"][..]
    } else if version == 3 {
        &["scale", "step", "value"][..]
    } else {
        &["scale", "value"][..]
    };
    require_exact_keys(current_time.keys().map(String::as_str), time_keys)?;
    let current_value = current_time
        .get("value")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| SessionError::Protocol("current_time.value is outside i32".into()))?;
    let current_scale = current_time
        .get("scale")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| SessionError::Protocol("current_time.scale is outside u32".into()))?;
    let current_step = if matches!(version, 3 | 4) {
        current_time
            .get("step")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                SessionError::Protocol("current_time.step must be a positive i32".into())
            })?
    } else {
        1
    };
    let total_time = if version == 4 {
        parse_fixture_total_time(current_time, current_value)?
    } else {
        0
    };
    if current_scale != time_scale {
        return Err(SessionError::Protocol(format!(
            "current_time.scale {current_scale} does not match session scale {time_scale}"
        )));
    }
    let parameters = match version {
        1 => Vec::new(),
        2 | 3 | 4 => {
            let payload = object
                .get("parameters")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SessionError::Protocol(
                        "render_frame v2/v3/v4 requires a string parameters field".into(),
                    )
                })?;
            parse_parameter_payload(payload, setup)?
        }
        _ => unreachable!(),
    };
    let input_started = report_timings.then(Instant::now);
    let input = fs::read(input_slot)
        .map_err(|error| SessionError::Io(format!("read resident input slot: {error}")))?;
    let input_read = input_started.map(|started| started.elapsed());
    if input.len() != input_pixel_bytes {
        return Err(SessionError::Protocol(format!(
            "resident input slot has {} bytes, expected {input_pixel_bytes}",
            input.len()
        )));
    }
    let rowbytes = pixel_format
        .rowbytes(width)
        .map_err(|error| SessionError::Protocol(error.to_string()))?;
    let borrowed_layers = fixture_layers
        .iter()
        .map(|layer| ResidentLayer {
            slot: layer.slot,
            width: layer.width,
            height: layer.height,
            pixels: &layer.pixels,
            format: layer.format,
            layout: layer.layout,
        })
        .collect::<Vec<_>>();
    let request_prepare = request_started
        .zip(input_read)
        .map(|(started, input_read)| started.elapsed().saturating_sub(input_read));
    let render_started = report_timings.then(Instant::now);
    let rendered = match fixture_smart {
        Some(smart) => host.render_resident_fixture_pixels(
            width,
            height,
            current_value,
            current_step,
            total_time,
            current_scale,
            pixel_format,
            input_format,
            &input,
            &parameters,
            &borrowed_layers,
            smart,
            primary_layout,
        ),
        None => host.render_resident_frame_pixels(
            width,
            height,
            current_value,
            current_scale,
            pixel_format,
            &input,
            &parameters,
        ),
    };
    let effect_render = render_started.map(|started| started.elapsed());
    match rendered {
        Ok(report) => {
            let output_started = report_timings.then(Instant::now);
            fs::write(output_slot, &report.raw_pixels).map_err(|error| {
                SessionError::Io(format!("write resident output slot: {error}"))
            })?;
            let output_write = output_started.map(|started| started.elapsed());
            if fixture_smart.is_some() {
                write_fixture_world_dumps(input_slot, &report)?;
            }
            let checksum_started = report_timings.then(Instant::now);
            let checksum_value = report.raw_pixel_sha256.clone();
            let checksum_time = checksum_started.map(|started| started.elapsed());
            *generation += 1;
            write_message(
                response,
                &FrameDone {
                    v: 1,
                    kind: "frame_done",
                    frame_index,
                    status: "ok",
                    output: Some(FrameOutput {
                        width,
                        height,
                        rowbytes,
                        pixel_format: pixel_format.name(),
                        render_path: if report.render_mode.starts_with("smart") {
                            "smartfx"
                        } else {
                            "classic"
                        },
                        checksum: checksum_value,
                        guards_intact: report.guards_intact,
                    }),
                    render_error: 0,
                    generation: Some(*generation),
                    timings_us: report_timings.then(|| FrameTimingsUs {
                        request_prepare: micros(request_prepare.expect("timing enabled")),
                        input_read: micros(input_read.expect("timing enabled")),
                        effect_render: micros(effect_render.expect("timing enabled")),
                        output_write: micros(output_write.expect("timing enabled")),
                        checksum: micros(checksum_time.expect("timing enabled")),
                    }),
                },
            )
        }
        Err(error) => {
            let render_error = error.selector_error_code().unwrap_or(-40);
            write_message(
                response,
                &FrameDone {
                    v: 1,
                    kind: "frame_done",
                    frame_index,
                    status: "error",
                    output: None,
                    render_error,
                    generation: None,
                    timings_us: report_timings.then(|| FrameTimingsUs {
                        request_prepare: micros(request_prepare.expect("timing enabled")),
                        input_read: micros(input_read.expect("timing enabled")),
                        effect_render: micros(effect_render.expect("timing enabled")),
                        ..FrameTimingsUs::default()
                    }),
                },
            )?;
            Err(SessionError::Classic(error))
        }
    }
}

fn micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn parse_fixture_total_time(
    current_time: &serde_json::Map<String, Value>,
    current_value: i32,
) -> Result<i32, SessionError> {
    current_time
        .get("total")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value >= 0 && current_value >= 0 && current_value <= *value)
        .ok_or_else(|| {
            SessionError::Protocol(
                "current_time.total must be a non-negative i32 at or after value".into(),
            )
        })
}

fn write_fixture_world_dumps(input_slot: &Path, report: &RenderReport) -> Result<(), SessionError> {
    let root = input_slot
        .parent()
        .ok_or_else(|| SessionError::Protocol("resident input slot has no parent".into()))?;
    let write_new = |path: &Path, bytes: &[u8]| -> Result<(), SessionError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                SessionError::Io(format!(
                    "create fixture world dump {}: {error}",
                    path.display()
                ))
            })?;
        file.write_all(bytes).map_err(|error| {
            SessionError::Io(format!(
                "write fixture world dump {}: {error}",
                path.display()
            ))
        })
    };
    let encode = |width: u32,
                  height: u32,
                  layout: ResidentWorldLayout,
                  raw: &[u8],
                  format: FramePixelFormat| {
        if raw.len() != layout.rowbytes as usize * height as usize {
            return Err(SessionError::Protocol(
                "fixture world dump length differs from live stride".into(),
            ));
        }
        let mut record = Vec::with_capacity(56 + raw.len());
        record.extend_from_slice(b"AEXWRAW1");
        for word in [
            width as i32,
            height as i32,
            format.bytes_per_pixel() as i32,
            layout.rowbytes as i32,
            layout.origin_x,
            layout.origin_y,
            layout.extent[0],
            layout.extent[1],
            layout.extent[2],
            layout.extent[3],
        ] {
            record.extend_from_slice(&word.to_le_bytes());
        }
        record.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        record.extend_from_slice(raw);
        Ok(record)
    };
    let primary_layout = report.raw_input_layout.ok_or_else(|| {
        SessionError::Protocol("fixture input world layout was not captured".into())
    })?;
    let primary_format = report.raw_input_format.ok_or_else(|| {
        SessionError::Protocol("fixture input world format was not captured".into())
    })?;
    write_new(
        &root.join("fixture-input-world.bin"),
        &encode(
            report.width,
            report.height,
            primary_layout,
            &report.raw_input_pixels,
            primary_format,
        )?,
    )?;
    for layer in &report.raw_secondary_layers {
        write_new(
            &root.join(format!("fixture-layer-slot{}-world.bin", layer.slot)),
            &encode(
                layer.width,
                layer.height,
                layer.layout,
                &layer.raw_pixels,
                layer.format,
            )?,
        )?;
    }
    Ok(())
}

fn load_resident_layers(
    input_slot: &Path,
    manifest_path: Option<&Path>,
    format: FramePixelFormat,
) -> Result<
    (
        Option<ResidentWorldLayout>,
        FramePixelFormat,
        Vec<OwnedResidentLayer>,
    ),
    SessionError,
> {
    let Some(manifest_path) = manifest_path else {
        return Ok((None, format, Vec::new()));
    };
    let session_root = input_slot
        .parent()
        .ok_or_else(|| SessionError::Protocol("resident input slot has no parent".into()))?
        .canonicalize()
        .map_err(|error| SessionError::Io(format!("canonicalize resident session: {error}")))?;
    let canonical_manifest = manifest_path
        .canonicalize()
        .map_err(|error| SessionError::Io(format!("canonicalize layer manifest: {error}")))?;
    if canonical_manifest.parent() != Some(session_root.as_path()) {
        return Err(SessionError::Protocol(
            "resident layer manifest must be staged beside the input slot".into(),
        ));
    }
    let manifest_size = fs::metadata(&canonical_manifest)
        .map_err(|error| SessionError::Io(format!("stat layer manifest: {error}")))?
        .len();
    if manifest_size == 0 || manifest_size > MAX_CONTROL_MESSAGE_BYTES as u64 {
        return Err(SessionError::Protocol(
            "resident layer manifest exceeds the bounded size".into(),
        ));
    }
    let bytes = fs::read(&canonical_manifest)
        .map_err(|error| SessionError::Io(format!("read layer manifest: {error}")))?;
    let manifest: ResidentLayerManifest = serde_json::from_slice(&bytes)
        .map_err(|error| SessionError::Protocol(format!("parse layer manifest: {error}")))?;
    if !matches!(manifest.v, 1 | 2 | 3 | 4)
        || manifest.layers.len() > 8
        || (manifest.v == 1
            && (manifest.primary.is_some()
                || manifest.layers.iter().any(|layer| layer.layout.is_some())))
        || (manifest.v >= 2 && manifest.primary.is_none())
        || (manifest.v != 4 && manifest.primary_pixel_format.is_some())
        || (manifest.v == 4 && manifest.primary_pixel_format.is_none())
        || (manifest.v < 3
            && manifest
                .layers
                .iter()
                .any(|layer| layer.pixel_format.is_some()))
        || (manifest.v >= 3
            && manifest
                .layers
                .iter()
                .any(|layer| layer.pixel_format.is_none()))
    {
        return Err(SessionError::Protocol(
            "resident layer manifest version or count is invalid".into(),
        ));
    }
    let input_format = manifest
        .primary_pixel_format
        .as_deref()
        .map(FramePixelFormat::parse)
        .transpose()
        .map_err(|error| SessionError::Protocol(error.to_string()))?
        .unwrap_or(format);
    let mut seen = BTreeSet::new();
    let mut layers = Vec::with_capacity(manifest.layers.len());
    for layer in manifest.layers {
        let layer_format = layer
            .pixel_format
            .as_deref()
            .map(FramePixelFormat::parse)
            .transpose()
            .map_err(|error| SessionError::Protocol(error.to_string()))?
            .unwrap_or(format);
        if layer.slot == 0 || !seen.insert(layer.slot) || !layer.path.is_absolute() {
            return Err(SessionError::Protocol(
                "resident layer identity is invalid".into(),
            ));
        }
        let path = layer
            .path
            .canonicalize()
            .map_err(|error| SessionError::Io(format!("canonicalize resident layer: {error}")))?;
        if path.parent() != Some(session_root.as_path()) {
            return Err(SessionError::Protocol(
                "resident layer must be staged beside the input slot".into(),
            ));
        }
        let expected = layer_format
            .byte_count(layer.width, layer.height)
            .map_err(|error| SessionError::Protocol(error.to_string()))?;
        let observed = fs::metadata(&path)
            .map_err(|error| SessionError::Io(format!("stat resident layer: {error}")))?
            .len();
        if observed != expected as u64 {
            return Err(SessionError::Protocol(format!(
                "resident layer byte count {observed} does not match {expected}"
            )));
        }
        let pixels = fs::read(&path)
            .map_err(|error| SessionError::Io(format!("read resident layer: {error}")))?;
        layer_format
            .validate_bytes(layer.width, layer.height, &pixels)
            .map_err(|error| SessionError::Protocol(error.to_string()))?;
        layers.push(OwnedResidentLayer {
            slot: layer.slot,
            width: layer.width,
            height: layer.height,
            pixels,
            format: layer_format,
            layout: layer.layout,
        });
    }
    Ok((manifest.primary, input_format, layers))
}

fn parse_parameter_payload(
    payload: &str,
    setup: &SetupReport,
) -> Result<Vec<ParameterValue>, SessionError> {
    if !payload.is_ascii() || payload.len() > MAX_PARAMETER_PAYLOAD_BYTES {
        return Err(SessionError::Protocol(
            "parameter payload must be bounded ASCII".into(),
        ));
    }
    let (body, component_payload) = if let Some(body) = payload.strip_prefix("v2|") {
        (body, false)
    } else if let Some(body) = payload.strip_prefix("v4|") {
        (body, true)
    } else {
        return Err(SessionError::Protocol(
            "macOS resident fixture parameters require a v2 or v4 payload".into(),
        ));
    };
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen_slots = BTreeSet::new();
    let mut values = Vec::new();
    for assignment in body.split(';') {
        let (identity, encoded) = assignment
            .split_once('=')
            .ok_or_else(|| SessionError::Protocol("parameter assignment has no '='".into()))?;
        if encoded.is_empty() || encoded.contains('=') {
            return Err(SessionError::Protocol(
                "parameter assignment value is malformed".into(),
            ));
        }
        let (id_and_slot, kind) = identity
            .split_once(':')
            .ok_or_else(|| SessionError::Protocol("parameter assignment has no kind".into()))?;
        let (id, slot_text) = id_and_slot
            .split_once('@')
            .ok_or_else(|| SessionError::Protocol("parameter assignment has no slot".into()))?;
        let slot = slot_text
            .parse::<usize>()
            .map_err(|_| SessionError::Protocol("parameter slot is invalid".into()))?;
        if id != format!("param_{slot}") || !seen_slots.insert(slot) {
            return Err(SessionError::Protocol(
                "parameter identity or uniqueness is invalid".into(),
            ));
        }
        let parameter = setup
            .parameters
            .iter()
            .find(|parameter| parameter.slot == slot)
            .ok_or_else(|| SessionError::Protocol(format!("unknown parameter slot {slot}")))?;
        let (value, color, point, angle, point3d) = match kind {
            "argb8" => {
                if parameter.param_type != PARAM_COLOR {
                    return Err(SessionError::Protocol(format!(
                        "parameter slot {slot} is not a color parameter"
                    )));
                }
                let components = encoded.split(',').collect::<Vec<_>>();
                if components.len() != 4 {
                    return Err(SessionError::Protocol(
                        "ARGB8 parameter requires four components".into(),
                    ));
                }
                let mut color = [0u8; 4];
                for (destination, component) in color.iter_mut().zip(components) {
                    *destination = component.parse::<u8>().map_err(|_| {
                        SessionError::Protocol(
                            "ARGB8 components must be integers from 0 to 255".into(),
                        )
                    })?;
                }
                (None, Some(color), None, None, None)
            }
            "point" => {
                if parameter.param_type != PARAM_POINT {
                    return Err(SessionError::Protocol(format!(
                        "parameter slot {slot} is not a point parameter"
                    )));
                }
                let components = encoded.split(',').collect::<Vec<_>>();
                if components.len() != 2 {
                    return Err(SessionError::Protocol(
                        "point parameter requires two components".into(),
                    ));
                }
                let mut point = [0.0; 2];
                for (destination, component) in point.iter_mut().zip(components) {
                    *destination = component.parse::<f64>().map_err(|_| {
                        SessionError::Protocol("point components must be finite numbers".into())
                    })?;
                    if !destination.is_finite() {
                        return Err(SessionError::Protocol(
                            "point components must be finite numbers".into(),
                        ));
                    }
                }
                (None, None, Some(point), None, None)
            }
            "angle" if component_payload => {
                if parameter.param_type != PARAM_ANGLE {
                    return Err(SessionError::Protocol(format!(
                        "parameter slot {slot} is not an angle parameter"
                    )));
                }
                let angle = encoded.parse::<f64>().map_err(|_| {
                    SessionError::Protocol("angle component must be a finite number".into())
                })?;
                if !angle.is_finite() || !(-32768.0..=32768.0).contains(&angle) {
                    return Err(SessionError::Protocol(
                        "angle component is outside the supported range".into(),
                    ));
                }
                (None, None, None, Some(angle), None)
            }
            "point3d" if component_payload => {
                if parameter.param_type != PARAM_POINT3D {
                    return Err(SessionError::Protocol(format!(
                        "parameter slot {slot} is not a point3d parameter"
                    )));
                }
                let components = encoded.split(',').collect::<Vec<_>>();
                if components.len() != 3 {
                    return Err(SessionError::Protocol(
                        "point3d parameter requires three components".into(),
                    ));
                }
                let mut point3d = [0.0; 3];
                for (destination, component) in point3d.iter_mut().zip(components) {
                    *destination = component.parse::<f64>().map_err(|_| {
                        SessionError::Protocol("point3d components must be finite numbers".into())
                    })?;
                    if !destination.is_finite() || !(-32768.0..=32768.0).contains(destination) {
                        return Err(SessionError::Protocol(
                            "point3d component is outside the supported range".into(),
                        ));
                    }
                }
                (None, None, None, None, Some(point3d))
            }
            "i32" | "f64" => {
                if matches!(
                    parameter.param_type,
                    PARAM_COLOR | PARAM_POINT | PARAM_ANGLE | PARAM_POINT3D
                ) {
                    return Err(SessionError::Protocol(format!(
                        "typed parameter slot {slot} requires its matching payload kind"
                    )));
                }
                let value = encoded
                    .parse::<f64>()
                    .map_err(|_| SessionError::Protocol("parameter value is invalid".into()))?;
                if !value.is_finite() || kind == "i32" && value.fract() != 0.0 {
                    return Err(SessionError::Protocol(
                        "parameter value shape is invalid".into(),
                    ));
                }
                (Some(value), None, None, None, None)
            }
            _ => {
                return Err(SessionError::Protocol(format!(
                    "unsupported parameter kind {kind:?}"
                )));
            }
        };
        values.push(ParameterValue {
            slot: Some(slot),
            name: parameter.name.clone(),
            value,
            color,
            point,
            angle,
            point3d,
        });
    }
    Ok(values)
}

fn require_exact_keys<'a>(
    actual: impl Iterator<Item = &'a str>,
    expected: &[&str],
) -> Result<(), SessionError> {
    let actual = actual.collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(SessionError::Protocol(format!(
            "request keys do not match the protocol: got {actual:?}, expected {expected:?}"
        )));
    }
    Ok(())
}

fn read_message(reader: &mut impl Read) -> Result<Option<Vec<u8>>, SessionError> {
    let mut prefix = [0u8; 4];
    match reader.read_exact(&mut prefix) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => {
            return Err(SessionError::Io(format!(
                "read resident request prefix: {error}"
            )));
        }
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > MAX_CONTROL_MESSAGE_BYTES {
        return Err(SessionError::Protocol(format!(
            "resident request length {length} is invalid"
        )));
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| SessionError::Io(format!("read resident request: {error}")))?;
    Ok(Some(payload))
}

fn write_message(writer: &mut impl Write, value: &impl Serialize) -> Result<(), SessionError> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| SessionError::Protocol(format!("serialize response: {error}")))?;
    if payload.is_empty() || payload.len() > MAX_CONTROL_RESPONSE_BYTES {
        return Err(SessionError::Protocol(
            "resident response exceeds the control bound".into(),
        ));
    }
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .and_then(|()| writer.write_all(&payload))
        .and_then(|()| writer.flush())
        .map_err(|error| SessionError::Io(format!("write resident response: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classic::{
        MAX_FAILURE_CRASH_SNAPSHOT_BYTES, MAX_FAILURE_SUITE_REQUEST_BYTES, MAX_FAILURE_TEXT_BYTES,
    };

    #[test]
    fn user_changed_request_has_an_exact_positive_slot_contract() {
        let setup = SetupReport {
            schema_version: 1,
            execution_backend: "fixture",
            global_setup_error: 0,
            params_setup_error: 0,
            advertised_num_params: 8,
            out_flags: 0,
            out_flags2: 0,
            custom_ui: None,
            parameters: vec![crate::classic::ParameterReport {
                slot: 7,
                index: 7,
                name: "Amount".into(),
                param_type: 1,
                ui_flags: 0,
                flags: 0,
                ui_width: 0,
                ui_height: 0,
                choices: None,
                default_value: Some(1.0),
                valid_min: None,
                valid_max: None,
                slider_min: None,
                slider_max: None,
                precision: None,
                current_color: None,
                default_color: None,
                current_components: None,
                default_components: None,
            }],
            suite_requests: Vec::new(),
            unsupported_suite_calls: Vec::new(),
            dropped_unsupported_suite_calls: 0,
        };
        let valid = serde_json::json!({
            "v": 1,
            "type": "user_changed_param",
            "slot": 7,
            "parameters": "v2|param_7@7:i32=9"
        });
        assert_eq!(
            parse_user_changed_request(valid.as_object().unwrap(), &setup)
                .unwrap()
                .0,
            7,
        );
        for invalid in [
            serde_json::json!({"v": 2, "type": "user_changed_param", "slot": 7, "parameters": "v2|"}),
            serde_json::json!({"v": 1, "type": "user_changed_param", "slot": 0, "parameters": "v2|"}),
            serde_json::json!({"v": 1, "type": "user_changed_param", "slot": "7", "parameters": "v2|"}),
            serde_json::json!({"v": 1, "type": "user_changed_param", "slot": 7}),
            serde_json::json!({"v": 1, "type": "user_changed_param", "slot": 7, "parameters": "v2|", "extra": true}),
        ] {
            assert!(parse_user_changed_request(invalid.as_object().unwrap(), &setup).is_err());
        }
    }

    #[test]
    fn fixture_total_time_is_strict_and_allows_zero_duration_at_t_zero() {
        let valid = serde_json::json!({"total":210})
            .as_object()
            .unwrap()
            .clone();
        assert_eq!(parse_fixture_total_time(&valid, 42).unwrap(), 210);
        let zero = serde_json::json!({"total":0}).as_object().unwrap().clone();
        assert_eq!(parse_fixture_total_time(&zero, 0).unwrap(), 0);
        assert!(parse_fixture_total_time(&zero, 1).is_err());
        let negative = serde_json::json!({"total":-1}).as_object().unwrap().clone();
        assert!(parse_fixture_total_time(&negative, 0).is_err());
        let oversized = serde_json::json!({"total":i64::from(i32::MAX) + 1})
            .as_object()
            .unwrap()
            .clone();
        assert!(parse_fixture_total_time(&oversized, 0).is_err());
    }

    #[test]
    fn probe_failure_response_carries_bounded_structured_diagnostics() {
        let diagnostic = ResidentFailureDiagnostic {
            schema_version: 1,
            stage: "admission_probe",
            execution_backend: "fixture",
            category: "callback",
            selector: Some("RENDER"),
            error_code: None,
            message: "guest callback failed".into(),
            crash_reason: None,
            crash_snapshot: None,
            suite_requests: vec!["PF Iterate8 Suite v1".into()],
            dropped_suite_requests: 0,
            unsupported_suite_calls: Vec::new(),
            dropped_unsupported_suite_calls: 0,
        };
        let mut framed = Vec::new();
        write_message(
            &mut framed,
            &SessionProbed {
                v: 1,
                kind: "session_probed",
                worker_pid: 42,
                status: "error",
                guards_intact: false,
                render_error: -40,
                failure: Some(diagnostic),
            },
        )
        .unwrap();
        let length = u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize;
        assert_eq!(length, framed.len() - 4);
        assert!(length <= MAX_CONTROL_RESPONSE_BYTES);
        let value: Value = serde_json::from_slice(&framed[4..]).unwrap();
        assert_eq!(value["failure"]["stage"], "admission_probe");
        assert_eq!(value["failure"]["category"], "callback");
        assert_eq!(value["failure"]["selector"], "RENDER");
        assert_eq!(value["failure"]["message"], "guest callback failed");
        assert!(value["failure"]["crash_reason"].is_null());
    }

    #[test]
    fn maximal_failure_diagnostic_fits_control_message_bound() {
        let diagnostic = ResidentFailureDiagnostic {
            schema_version: 1,
            stage: "admission_probe",
            execution_backend: "unicorn-x86_64",
            category: "callback",
            selector: Some("SMART_RENDER"),
            error_code: Some(-40),
            message: "m".repeat(MAX_FAILURE_TEXT_BYTES),
            crash_reason: Some("c".repeat(MAX_FAILURE_TEXT_BYTES)),
            crash_snapshot: None,
            suite_requests: vec!["s".repeat(MAX_FAILURE_SUITE_REQUEST_BYTES); 64],
            dropped_suite_requests: u64::MAX,
            unsupported_suite_calls: vec![
                crate::x64::UnsupportedSuiteCall {
                    name: "AEGP Utility Suite",
                    version: u32::MAX,
                    slot: usize::MAX,
                    call_count: u64::MAX,
                };
                64
            ],
            dropped_unsupported_suite_calls: u64::MAX,
        };
        let mut framed = Vec::new();
        write_message(
            &mut framed,
            &SessionProbed {
                v: 1,
                kind: "session_probed",
                worker_pid: u32::MAX,
                status: "error",
                guards_intact: false,
                render_error: -40,
                failure: Some(diagnostic),
            },
        )
        .unwrap();
        assert!(framed.len() - 4 <= MAX_CONTROL_RESPONSE_BYTES);
    }

    #[test]
    fn maximal_close_diagnostic_with_snapshot_fits_control_message_bound() {
        let calls = vec![
            crate::x64::UnsupportedSuiteCall {
                name: "AEGP Utility Suite",
                version: u32::MAX,
                slot: usize::MAX,
                call_count: u64::MAX,
            };
            64
        ];
        let setup = SetupReport {
            schema_version: 1,
            execution_backend: "unicorn-x86_64",
            global_setup_error: 0,
            params_setup_error: 0,
            advertised_num_params: 0,
            out_flags: u32::MAX,
            out_flags2: u32::MAX,
            custom_ui: None,
            parameters: Vec::new(),
            suite_requests: vec!["s".repeat(MAX_FAILURE_SUITE_REQUEST_BYTES); 64],
            unsupported_suite_calls: calls.clone(),
            dropped_unsupported_suite_calls: u64::MAX,
        };
        let mut close = serde_json::json!({
            "schema_version": 1,
            "execution_backend": "unicorn-x86_64",
            "frames_rendered": 1,
            "frame_setdown_error": 0,
            "sequence_setdown_error": 0,
            "global_setdown_error": -40,
            "global_setdown_diagnostic": {
                "message": "m".repeat(MAX_FAILURE_TEXT_BYTES),
                "crash_reason": "c".repeat(MAX_FAILURE_TEXT_BYTES),
                "crash_snapshot": {"payload": "x".repeat(MAX_FAILURE_CRASH_SNAPSHOT_BYTES)},
                "suite_requests": vec!["s".repeat(MAX_FAILURE_SUITE_REQUEST_BYTES); 64],
                "unsupported_suite_calls": calls,
            },
            "suite_requests": vec!["s".repeat(MAX_FAILURE_SUITE_REQUEST_BYTES); 64],
            "unsupported_suite_calls": vec![crate::x64::UnsupportedSuiteCall {
                name: "AEGP Utility Suite",
                version: u32::MAX,
                slot: usize::MAX,
                call_count: u64::MAX,
            }; 64],
            "dropped_unsupported_suite_calls": u64::MAX,
            "session_clean": false,
        });
        bound_session_close(&setup, u32::MAX, &mut close).unwrap();
        let mut framed = Vec::new();
        write_message(
            &mut framed,
            &SessionClosed {
                v: 1,
                kind: "session_closed",
                worker_pid: u32::MAX,
                setup: &setup,
                close,
            },
        )
        .unwrap();
        assert!(framed.len() - 4 <= MAX_CONTROL_RESPONSE_BYTES);
    }

    #[test]
    fn numeric_parameter_payload_uses_declared_slots_and_rejects_duplicates() {
        let setup = SetupReport {
            schema_version: 1,
            execution_backend: "fixture",
            global_setup_error: 0,
            params_setup_error: 0,
            advertised_num_params: 2,
            out_flags: 0,
            out_flags2: 0,
            custom_ui: None,
            parameters: vec![crate::classic::ParameterReport {
                slot: 1,
                index: 1,
                param_type: 10,
                name: "Amount".into(),
                ui_flags: 0,
                flags: 0,
                ui_width: 0,
                ui_height: 0,
                choices: None,
                default_value: Some(5.0),
                valid_min: Some(0.0),
                valid_max: Some(100.0),
                slider_min: Some(0.0),
                slider_max: Some(100.0),
                precision: Some(2),
                current_color: None,
                default_color: None,
                current_components: None,
                default_components: None,
            }],
            suite_requests: Vec::new(),
            unsupported_suite_calls: Vec::new(),
            dropped_unsupported_suite_calls: 0,
        };
        let parsed = parse_parameter_payload("v2|param_1@1:f64=12.5", &setup).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Amount");
        assert_eq!(parsed[0].value, Some(12.5));
        assert!(parse_parameter_payload("v2|param_1@1:f64=12.5;param_1@1:f64=20", &setup).is_err());
    }

    #[test]
    fn color_parameter_payload_is_slot_qualified_and_typed() {
        let mut setup = SetupReport {
            schema_version: 1,
            execution_backend: "fixture",
            global_setup_error: 0,
            params_setup_error: 0,
            advertised_num_params: 2,
            out_flags: 0,
            out_flags2: 0,
            custom_ui: None,
            parameters: vec![crate::classic::ParameterReport {
                slot: 1,
                index: 1,
                param_type: PARAM_COLOR,
                name: "Key Color".into(),
                ui_flags: 0,
                flags: 0,
                ui_width: 0,
                ui_height: 0,
                choices: None,
                default_value: None,
                valid_min: None,
                valid_max: None,
                slider_min: None,
                slider_max: None,
                precision: None,
                current_color: Some([0, 0, 0, 0]),
                default_color: Some([255, 1, 2, 3]),
                current_components: None,
                default_components: None,
            }],
            suite_requests: Vec::new(),
            unsupported_suite_calls: Vec::new(),
            dropped_unsupported_suite_calls: 0,
        };
        let parsed = parse_parameter_payload("v2|param_1@1:argb8=255,64,128,192", &setup).unwrap();
        assert_eq!(parsed[0].value, None);
        assert_eq!(parsed[0].color, Some([255, 64, 128, 192]));
        assert!(parse_parameter_payload("v2|param_1@1:f64=1", &setup).is_err());
        assert!(parse_parameter_payload("v2|param_1@1:argb8=256,1,2,3", &setup).is_err());

        setup.parameters[0].param_type = 1;
        assert!(parse_parameter_payload("v2|param_1@1:argb8=255,1,2,3", &setup).is_err());
    }

    #[test]
    fn point_parameter_payload_is_slot_qualified_and_typed() {
        let setup = SetupReport {
            schema_version: 1,
            execution_backend: "fixture",
            global_setup_error: 0,
            params_setup_error: 0,
            advertised_num_params: 2,
            out_flags: 0,
            out_flags2: 0,
            custom_ui: None,
            parameters: vec![crate::classic::ParameterReport {
                slot: 2,
                index: 2,
                param_type: PARAM_POINT,
                name: "Center".into(),
                ui_flags: 0,
                flags: 0,
                ui_width: 0,
                ui_height: 0,
                choices: None,
                default_value: None,
                valid_min: None,
                valid_max: None,
                slider_min: None,
                slider_max: None,
                precision: None,
                current_color: None,
                default_color: None,
                current_components: None,
                default_components: None,
            }],
            suite_requests: Vec::new(),
            unsupported_suite_calls: Vec::new(),
            dropped_unsupported_suite_calls: 0,
        };
        let parsed = parse_parameter_payload("v2|param_2@2:point=1.5,-2.25", &setup).unwrap();
        assert_eq!(parsed[0].slot, Some(2));
        assert_eq!(parsed[0].point, Some([1.5, -2.25]));
        assert_eq!(parsed[0].value, None);
        assert_eq!(parsed[0].color, None);
        assert!(parse_parameter_payload("v2|param_2@2:f64=1", &setup).is_err());
        assert!(parse_parameter_payload("v2|param_2@2:point=1", &setup).is_err());
    }

    #[test]
    fn component_parameter_payload_carries_angle_and_point3d() {
        let parameter = |slot, param_type, name: &str| crate::classic::ParameterReport {
            slot,
            index: slot as i32,
            param_type,
            name: name.into(),
            ui_flags: 0,
            flags: 0,
            ui_width: 0,
            ui_height: 0,
            choices: None,
            default_value: None,
            valid_min: None,
            valid_max: None,
            slider_min: None,
            slider_max: None,
            precision: None,
            current_color: None,
            default_color: None,
            current_components: None,
            default_components: None,
        };
        let setup = SetupReport {
            schema_version: 1,
            execution_backend: "fixture",
            global_setup_error: 0,
            params_setup_error: 0,
            advertised_num_params: 3,
            out_flags: 0,
            out_flags2: 0,
            custom_ui: None,
            parameters: vec![
                parameter(1, PARAM_ANGLE, "Angle"),
                parameter(2, PARAM_POINT3D, "Position"),
            ],
            suite_requests: Vec::new(),
            unsupported_suite_calls: Vec::new(),
            dropped_unsupported_suite_calls: 0,
        };
        let parsed =
            parse_parameter_payload("v4|param_1@1:angle=12.5;param_2@2:point3d=25,50,75", &setup)
                .unwrap();
        assert_eq!(parsed[0].angle, Some(12.5));
        assert_eq!(parsed[1].point3d, Some([25.0, 50.0, 75.0]));
        assert!(parse_parameter_payload("v2|param_1@1:angle=12.5", &setup).is_err());
        assert!(parse_parameter_payload("v4|param_2@2:point3d=1,2", &setup).is_err());
    }

    #[test]
    fn length_prefix_rejects_zero_and_oversized_messages() {
        assert!(read_message(&mut &0u32.to_le_bytes()[..]).is_err());
        assert!(
            read_message(&mut &((MAX_CONTROL_MESSAGE_BYTES + 1) as u32).to_le_bytes()[..]).is_err()
        );
    }

    #[test]
    fn response_budget_is_larger_than_the_request_budget_but_still_bounded() {
        let response = serde_json::json!({
            "setup": "x".repeat(MAX_CONTROL_MESSAGE_BYTES),
        });
        let mut framed = Vec::new();
        write_message(&mut framed, &response).unwrap();
        assert!(framed.len() - 4 > MAX_CONTROL_MESSAGE_BYTES);
        assert!(framed.len() - 4 <= MAX_CONTROL_RESPONSE_BYTES);

        let oversized = serde_json::json!({
            "setup": "x".repeat(MAX_CONTROL_RESPONSE_BYTES),
        });
        assert!(write_message(&mut Vec::new(), &oversized).is_err());
    }

    #[test]
    fn resident_layer_manifest_is_bounded_unique_and_session_local() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-resident-layer-manifest-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let input = root.join("input.argb8");
        fs::write(&input, [0u8; 4]).unwrap();
        let layer = root.join("layer.argb8");
        fs::write(&layer, [255u8, 1, 2, 3]).unwrap();
        let manifest = root.join("layers.json");
        let entry = serde_json::json!({
            "slot": 1, "width": 1, "height": 1, "path": layer
        });
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({"v":1,"layers":[entry.clone()]})).unwrap(),
        )
        .unwrap();
        let (primary, _, loaded) =
            load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).unwrap();
        assert!(primary.is_none());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].format, FramePixelFormat::Argb8);
        assert_eq!(loaded[0].pixels, [255, 1, 2, 3]);

        let layout = serde_json::json!({
            "rowbytes":8,"padding_byte":90,"origin_x":-2,"origin_y":3,
            "extent":[0,0,1,1]
        });
        let mut v2_entry = entry.clone();
        v2_entry["layout"] = layout.clone();
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "v":2,"primary":layout,"layers":[v2_entry]
            }))
            .unwrap(),
        )
        .unwrap();
        let (primary, _, loaded) =
            load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).unwrap();
        assert_eq!(primary.unwrap().rowbytes, 8);
        assert_eq!(loaded[0].layout.unwrap().origin_x, -2);

        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({"v":1,"layers":[entry.clone(),entry]})).unwrap(),
        )
        .unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());

        fs::write(&layer, [0u8; 8]).unwrap();
        let entry = serde_json::json!({
            "slot": 1, "width": 1, "height": 1, "path": layer
        });
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({"v":1,"layers":[entry]})).unwrap(),
        )
        .unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_three_manifest_preserves_independent_secondary_depth() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-resident-mixed-depth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let input = root.join("input.argb8");
        let layer = root.join("layer.argb16");
        let manifest = root.join("layers.json");
        fs::write(&input, [255u8, 40, 60, 80]).unwrap();
        let layer_pixels = [0u8, 128, 0, 16, 0, 32, 0, 48];
        fs::write(&layer, layer_pixels).unwrap();
        let mut document = serde_json::json!({
            "v":3,
            "primary":{"rowbytes":4,"padding_byte":0,"origin_x":0,"origin_y":0,"extent":[0,0,1,1]},
            "layers":[{
                "slot":1,"width":1,"height":1,"path":layer,"pixel_format":"argb16",
                "layout":{"rowbytes":16,"padding_byte":90,"origin_x":-2,"origin_y":3,"extent":[0,0,1,1]}
            }]
        });
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        let (_, _, loaded) =
            load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].format, FramePixelFormat::Argb16);
        assert_eq!(loaded[0].pixels, layer_pixels);
        assert_eq!(loaded[0].layout.unwrap().rowbytes, 16);
        let float_pixels = FramePixelFormat::Argb32f
            .promote_rgba8(&[20, 100, 140, 255])
            .unwrap();
        fs::write(&layer, &float_pixels).unwrap();
        document["layers"][0]["pixel_format"] = serde_json::json!("argb32f");
        document["layers"][0]["layout"]["rowbytes"] = serde_json::json!(32);
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        let (_, _, float_loaded) =
            load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).unwrap();
        assert_eq!(float_loaded[0].format, FramePixelFormat::Argb32f);
        assert_eq!(float_loaded[0].pixels, float_pixels);
        fs::write(&layer, layer_pixels).unwrap();
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());
        document["layers"][0]["pixel_format"] = serde_json::json!("argb16");
        document["v"] = serde_json::json!(2);
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());
        document["v"] = serde_json::json!(3);
        document["layers"][0]
            .as_object_mut()
            .unwrap()
            .remove("pixel_format");
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_four_manifest_preserves_independent_primary_depth() {
        let root = std::env::temp_dir().join(format!(
            "aexcompat-resident-primary-depth-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let input = root.join("input.argb16");
        let manifest = root.join("layers.json");
        let primary = FramePixelFormat::Argb16
            .promote_rgba8(&[20, 100, 140, 255])
            .unwrap();
        fs::write(&input, primary).unwrap();
        let mut document = serde_json::json!({
            "v":4,
            "primary_pixel_format":"argb16",
            "primary":{
                "rowbytes":16,"padding_byte":90,"origin_x":2,"origin_y":-1,
                "extent":[0,0,1,1]
            },
            "layers":[]
        });
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        let (layout, format, layers) =
            load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).unwrap();
        assert_eq!(format, FramePixelFormat::Argb16);
        assert_eq!(layout.unwrap().rowbytes, 16);
        assert!(layers.is_empty());
        document["v"] = serde_json::json!(3);
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());
        document["v"] = serde_json::json!(4);
        document
            .as_object_mut()
            .unwrap()
            .remove("primary_pixel_format");
        fs::write(&manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(load_resident_layers(&input, Some(&manifest), FramePixelFormat::Argb8).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_two_world_manifest_renders_public_probe_and_dumps_exact_rows() {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let aex =
            repository.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
        if !aex.is_file() {
            eprintln!("skipping resident world-manifest AEX smoke: build public layer probe");
            return;
        }
        let image = PeImage::parse_and_map(&fs::read(aex).unwrap()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-resident-world-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let input = root.join("input.argb8");
        let output = root.join("output.argb8");
        let layer = root.join("layer.argb8");
        let manifest = root.join("layers.json");
        fs::write(&input, [255u8, 40, 60, 80].repeat(12)).unwrap();
        fs::write(&output, [0u8; 48]).unwrap();
        fs::write(&layer, [255u8, 20, 100, 140].repeat(12)).unwrap();
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "v":2,
                "primary":{
                    "rowbytes":20,"padding_byte":90,"origin_x":2,"origin_y":-1,
                    "extent":[1,0,4,3]
                },
                "layers":[{
                    "slot":1,"width":4,"height":3,"path":layer,
                    "layout":{
                        "rowbytes":24,"padding_byte":90,"origin_x":-2,"origin_y":3,
                        "extent":[0,0,4,3]
                    }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let mut request = Vec::new();
        write_message(
            &mut request,
            &serde_json::json!({
                "v":4,"type":"render_frame","frame_index":0,
                "current_time":{"value":0,"step":1,"total":1,"scale":1},
                "parameters":"v4|param_2@2:f64=255"
            }),
        )
        .unwrap();
        write_message(&mut request, &serde_json::json!({"v":1,"type":"close"})).unwrap();
        let mut response = Vec::new();
        run_resident_session(
            &image,
            &input,
            &output,
            4,
            3,
            1,
            FramePixelFormat::Argb8,
            None,
            Some(&manifest),
            Some(false),
            request.as_slice(),
            &mut response,
        )
        .unwrap();
        let mut response = response.as_slice();
        let ready: Value =
            serde_json::from_slice(&read_message(&mut response).unwrap().unwrap()).unwrap();
        let frame: Value =
            serde_json::from_slice(&read_message(&mut response).unwrap().unwrap()).unwrap();
        assert_eq!(ready["type"], "session_ready");
        assert_eq!(frame["status"], "ok");
        assert_eq!(&fs::read(&output).unwrap()[8..12], &[255, 3, 8, 126]);
        let primary = fs::read(root.join("fixture-input-world.bin")).unwrap();
        let secondary = fs::read(root.join("fixture-layer-slot1-world.bin")).unwrap();
        assert_eq!(&primary[..8], b"AEXWRAW1");
        assert_eq!(&secondary[..8], b"AEXWRAW1");
        assert_eq!(i32::from_le_bytes(primary[20..24].try_into().unwrap()), 20);
        assert_eq!(
            i32::from_le_bytes(secondary[20..24].try_into().unwrap()),
            24
        );
        assert_eq!(primary.len(), 56 + 60);
        assert_eq!(secondary.len(), 56 + 72);
        assert!(
            primary[56..]
                .chunks_exact(20)
                .all(|row| row[16..] == [90; 4])
        );
        assert!(
            secondary[56..]
                .chunks_exact(24)
                .all(|row| row[16..] == [90; 8])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_three_mixed_depth_manifest_renders_and_dumps_secondary_argb16() {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let aex =
            repository.join("target/pf-layer-param-probe-build/Release/pf_layer_param_probe.aex");
        if !aex.is_file() {
            eprintln!("skipping resident mixed-depth AEX smoke: build public layer probe");
            return;
        }
        let image = PeImage::parse_and_map(&fs::read(aex).unwrap()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-resident-mixed-world-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let input = root.join("input.argb8");
        let output = root.join("output.argb8");
        let layer = root.join("layer.argb16");
        let manifest = root.join("layers.json");
        fs::write(&input, [255u8, 40, 60, 80].repeat(12)).unwrap();
        fs::write(&output, [0u8; 48]).unwrap();
        let secondary = FramePixelFormat::Argb16
            .promote_rgba8(&[20u8, 100, 140, 255].repeat(12))
            .unwrap();
        fs::write(&layer, secondary).unwrap();
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "v":3,
                "primary":{
                    "rowbytes":20,"padding_byte":90,"origin_x":2,"origin_y":-1,
                    "extent":[1,0,4,3]
                },
                "layers":[{
                    "slot":1,"width":4,"height":3,"path":layer,"pixel_format":"argb16",
                    "layout":{
                        "rowbytes":40,"padding_byte":90,"origin_x":-2,"origin_y":3,
                        "extent":[0,0,4,3]
                    }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let mut request = Vec::new();
        write_message(
            &mut request,
            &serde_json::json!({
                "v":4,"type":"render_frame","frame_index":0,
                "current_time":{"value":0,"step":1,"total":1,"scale":1},
                "parameters":"v4|param_2@2:f64=255"
            }),
        )
        .unwrap();
        write_message(&mut request, &serde_json::json!({"v":1,"type":"close"})).unwrap();
        let mut response = Vec::new();
        run_resident_session(
            &image,
            &input,
            &output,
            4,
            3,
            1,
            FramePixelFormat::Argb8,
            None,
            Some(&manifest),
            Some(false),
            request.as_slice(),
            &mut response,
        )
        .unwrap();
        let mut response = response.as_slice();
        let ready: Value =
            serde_json::from_slice(&read_message(&mut response).unwrap().unwrap()).unwrap();
        let frame: Value =
            serde_json::from_slice(&read_message(&mut response).unwrap().unwrap()).unwrap();
        assert_eq!(ready["type"], "session_ready");
        assert_eq!(frame["status"], "ok");
        assert_eq!(&fs::read(&output).unwrap()[8..12], &[255, 3, 24, 126]);
        let captured = fs::read(root.join("fixture-layer-slot1-world.bin")).unwrap();
        assert_eq!(&captured[..8], b"AEXWRAW1");
        assert_eq!(i32::from_le_bytes(captured[16..20].try_into().unwrap()), 8);
        assert_eq!(i32::from_le_bytes(captured[20..24].try_into().unwrap()), 40);
        assert_eq!(captured.len(), 56 + 120);
        assert!(
            captured[56..]
                .chunks_exact(40)
                .all(|row| row[32..] == [90; 8])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_four_mixed_primary_depth_renders_and_dumps_argb16_input() {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let aex = repository.join("target/pf-custom-ui-probe-build/Release/pf_custom_ui_probe.aex");
        if !aex.is_file() {
            eprintln!("skipping resident primary-depth AEX smoke: build public custom UI probe");
            return;
        }
        let image = PeImage::parse_and_map(&fs::read(aex).unwrap()).unwrap();
        let root = std::env::temp_dir().join(format!(
            "aexcompat-resident-primary-world-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let input = root.join("input.argb16");
        let output = root.join("output.argb8");
        let manifest = root.join("layers.json");
        let source = FramePixelFormat::Argb16
            .promote_rgba8(&[40, 60, 80, 255].repeat(12))
            .unwrap();
        fs::write(&input, source).unwrap();
        fs::write(&output, [0u8; 48]).unwrap();
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "v":4,
                "primary_pixel_format":"argb16",
                "primary":{
                    "rowbytes":40,"padding_byte":90,"origin_x":2,"origin_y":-1,
                    "extent":[1,0,4,3]
                },
                "layers":[]
            }))
            .unwrap(),
        )
        .unwrap();
        let mut request = Vec::new();
        write_message(
            &mut request,
            &serde_json::json!({
                "v":4,"type":"render_frame","frame_index":0,
                "current_time":{"value":0,"step":1,"total":1,"scale":1},
                "parameters":"v4|"
            }),
        )
        .unwrap();
        write_message(&mut request, &serde_json::json!({"v":1,"type":"close"})).unwrap();
        let mut response = Vec::new();
        run_resident_session(
            &image,
            &input,
            &output,
            4,
            3,
            1,
            FramePixelFormat::Argb8,
            None,
            Some(&manifest),
            Some(false),
            request.as_slice(),
            &mut response,
        )
        .unwrap();
        let mut response = response.as_slice();
        let ready: Value =
            serde_json::from_slice(&read_message(&mut response).unwrap().unwrap()).unwrap();
        let frame: Value =
            serde_json::from_slice(&read_message(&mut response).unwrap().unwrap()).unwrap();
        assert_eq!(ready["type"], "session_ready");
        assert_eq!(frame["status"], "ok");
        assert_eq!(frame["output"]["pixel_format"], "argb8");
        let captured = fs::read(root.join("fixture-input-world.bin")).unwrap();
        assert_eq!(&captured[..8], b"AEXWRAW1");
        assert_eq!(i32::from_le_bytes(captured[16..20].try_into().unwrap()), 8);
        assert_eq!(i32::from_le_bytes(captured[20..24].try_into().unwrap()), 40);
        assert_eq!(captured.len(), 56 + 120);
        assert!(
            captured[56..]
                .chunks_exact(40)
                .all(|row| row[32..] == [90; 8])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
