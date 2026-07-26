use crate::classic::{
    ClassicError, ClassicHost, PARAM_COLOR, PARAM_POINT, ParameterValue, ResidentFailureDiagnostic,
    SetupReport,
};
use crate::pe::PeImage;
use crate::pixel::FramePixelFormat;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_PARAMETER_PAYLOAD_BYTES: usize = 16 * 1024;

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

pub fn run_resident_session(
    image: &PeImage,
    input_slot: &Path,
    output_slot: &Path,
    width: u32,
    height: u32,
    time_scale: u32,
    pixel_format: FramePixelFormat,
    effect_selector: Option<&str>,
    mut request: impl Read,
    mut response: impl Write,
) -> Result<(), SessionError> {
    let pixel_bytes = pixel_format
        .byte_count(width, height)
        .map_err(|error| SessionError::Protocol(error.to_string()))?;
    let mut host = ClassicHost::new_with_effect(image, effect_selector)?;
    let setup = host.begin_resident_session(width, height, time_scale)?;
    write_message(
        &mut response,
        &SessionReady {
            v: 1,
            kind: "session_ready",
            worker_pid: std::process::id(),
            setup: &setup,
        },
    )?;
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
                    if input.len() != pixel_bytes {
                        return Err(SessionError::Protocol(format!(
                            "resident probe input slot has {} bytes, expected {pixel_bytes}",
                            input.len()
                        )));
                    }
                    match host.probe_resident_pixels(
                        width,
                        height,
                        time_scale,
                        pixel_format,
                        &input,
                    ) {
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
                            let render_error = match &error {
                                ClassicError::Selector { error, .. } => *error,
                                _ => -40,
                            };
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
                        pixel_bytes,
                        pixel_format,
                        &mut host,
                        &mut generation,
                        &mut response,
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
    let close_value = serde_json::to_value(&close)
        .map_err(|error| SessionError::Protocol(format!("serialize close report: {error}")))?;
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

#[allow(clippy::too_many_arguments)]
fn parse_render_frame(
    object: &serde_json::Map<String, Value>,
    setup: &SetupReport,
    time_scale: u32,
    input_slot: &Path,
    output_slot: &Path,
    width: u32,
    height: u32,
    pixel_bytes: usize,
    pixel_format: FramePixelFormat,
    host: &mut ClassicHost,
    generation: &mut u64,
    response: &mut impl Write,
) -> Result<(), SessionError> {
    let version = object
        .get("v")
        .and_then(Value::as_u64)
        .ok_or_else(|| SessionError::Protocol("render_frame has no integer v".into()))?;
    let allowed = if version == 1 {
        &["current_time", "frame_index", "type", "v"][..]
    } else if version == 2 {
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
    require_exact_keys(current_time.keys().map(String::as_str), &["scale", "value"])?;
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
    if current_scale != time_scale {
        return Err(SessionError::Protocol(format!(
            "current_time.scale {current_scale} does not match session scale {time_scale}"
        )));
    }
    let parameters = match version {
        1 => Vec::new(),
        2 => {
            let payload = object
                .get("parameters")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SessionError::Protocol(
                        "render_frame v2 requires a string parameters field".into(),
                    )
                })?;
            parse_parameter_payload(payload, setup)?
        }
        _ => unreachable!(),
    };
    let input = fs::read(input_slot)
        .map_err(|error| SessionError::Io(format!("read resident input slot: {error}")))?;
    if input.len() != pixel_bytes {
        return Err(SessionError::Protocol(format!(
            "resident input slot has {} bytes, expected {pixel_bytes}",
            input.len()
        )));
    }
    let rowbytes = pixel_format
        .rowbytes(width)
        .map_err(|error| SessionError::Protocol(error.to_string()))?;
    match host.render_resident_pixels(
        width,
        height,
        current_value,
        current_scale,
        pixel_format,
        &input,
        &parameters,
    ) {
        Ok(report) => {
            fs::write(output_slot, &report.raw_pixels).map_err(|error| {
                SessionError::Io(format!("write resident output slot: {error}"))
            })?;
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
                        checksum: format!("{:x}", Sha256::digest(&report.raw_pixels)),
                        guards_intact: report.guards_intact,
                    }),
                    render_error: 0,
                    generation: Some(*generation),
                },
            )
        }
        Err(error) => {
            let render_error = match error {
                ClassicError::Selector { error, .. } => error,
                _ => -40,
            };
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
                },
            )?;
            Err(SessionError::Classic(error))
        }
    }
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
    let Some(body) = payload.strip_prefix("v2|") else {
        return Err(SessionError::Protocol(
            "macOS resident numeric parameters require a v2 payload".into(),
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
        let (value, color, point) = match kind {
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
                (None, Some(color), None)
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
                (None, None, Some(point))
            }
            "i32" | "f64" => {
                if parameter.param_type == PARAM_COLOR || parameter.param_type == PARAM_POINT {
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
                (Some(value), None, None)
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
    if payload.is_empty() || payload.len() > MAX_CONTROL_MESSAGE_BYTES {
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
    use crate::classic::{MAX_FAILURE_SUITE_REQUEST_BYTES, MAX_FAILURE_TEXT_BYTES};

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
        assert!(length <= MAX_CONTROL_MESSAGE_BYTES);
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
        assert!(framed.len() - 4 <= MAX_CONTROL_MESSAGE_BYTES);
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
            parameters: vec![crate::classic::ParameterReport {
                slot: 1,
                index: 1,
                param_type: 10,
                name: "Amount".into(),
                default_value: Some(5.0),
                valid_min: Some(0.0),
                valid_max: Some(100.0),
                slider_min: Some(0.0),
                slider_max: Some(100.0),
                precision: Some(2),
                current_color: None,
                default_color: None,
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
            parameters: vec![crate::classic::ParameterReport {
                slot: 1,
                index: 1,
                param_type: PARAM_COLOR,
                name: "Key Color".into(),
                default_value: None,
                valid_min: None,
                valid_max: None,
                slider_min: None,
                slider_max: None,
                precision: None,
                current_color: Some([0, 0, 0, 0]),
                default_color: Some([255, 1, 2, 3]),
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
            parameters: vec![crate::classic::ParameterReport {
                slot: 2,
                index: 2,
                param_type: PARAM_POINT,
                name: "Center".into(),
                default_value: None,
                valid_min: None,
                valid_max: None,
                slider_min: None,
                slider_max: None,
                precision: None,
                current_color: None,
                default_color: None,
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
    fn length_prefix_rejects_zero_and_oversized_messages() {
        assert!(read_message(&mut &0u32.to_le_bytes()[..]).is_err());
        assert!(
            read_message(&mut &((MAX_CONTROL_MESSAGE_BYTES + 1) as u32).to_le_bytes()[..]).is_err()
        );
    }
}
