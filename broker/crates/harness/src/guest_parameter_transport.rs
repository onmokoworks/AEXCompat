// Pure producer used by macOS and exercised on every test platform.
use aexcompat_broker::parameter_limits::{
    MAX_PARAMETER_MESSAGE_BYTES, MAX_PARAMETER_PAYLOAD_BYTES, MAX_PARAMETERS,
};
use aexcompat_broker::render_fixture::InteractiveParameter;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::Write;

pub(crate) struct FixtureParameterPayload {
    pub(crate) transport: String,
    pub(crate) identity: Value,
}
pub(crate) fn write_control_message(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("serialize resident request: {error}"))?;
    if bytes.is_empty() || bytes.len() > MAX_PARAMETER_MESSAGE_BYTES {
        return Err(format!(
            "resident request exceeds {MAX_PARAMETER_MESSAGE_BYTES} bytes"
        ));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| "resident request exceeds u32 framing".to_string())?;
    writer
        .write_all(&length.to_le_bytes())
        .and_then(|()| writer.write_all(&bytes))
        .and_then(|()| writer.flush())
        .map_err(|error| format!("write resident request: {error}"))
}
pub(crate) fn fixture_parameter_payload(
    parameters: &[InteractiveParameter],
) -> Result<FixtureParameterPayload, String> {
    if parameters.len() > MAX_PARAMETERS as usize {
        return Err("fixture parameter count exceeds 4096".into());
    }
    let mut slots = HashSet::new();
    if parameters
        .iter()
        .any(|p| p.slot == 0 || p.slot > MAX_PARAMETERS || !slots.insert(p.slot))
    {
        return Err("fixture parameter slots must be unique and in 1..4096".into());
    }
    let mut assignments = Vec::new();
    let mut identity = Vec::new();
    let mut component_payload = false;
    for parameter in parameters {
        let (encoded, kind, value) = match parameter.kind.as_str() {
            "layer" | "group_start" | "group_end" | "button" | "custom" | "no_data" => {
                continue;
            }
            "integer" | "path" => {
                if !parameter.value.is_finite() || parameter.value.fract() != 0.0 {
                    return Err(format!(
                        "fixture scalar slot {} requires an integer value",
                        parameter.slot
                    ));
                }
                (
                    format!("i32={}", parameter.value as i64),
                    "integer",
                    json!(parameter.value as i64),
                )
            }
            "popup" => {
                if !parameter.value.is_finite()
                    || parameter.value.fract() != 0.0
                    || parameter.choices.is_empty()
                    || parameter.choices.len() > 64
                    || parameter.minimum != 1.0
                    || parameter.maximum != parameter.choices.len() as f64
                {
                    return Err(format!(
                        "fixture popup slot {} requires a one-based choice",
                        parameter.slot
                    ));
                }
                (
                    format!("i32={}", parameter.value as i64),
                    "popup",
                    json!(parameter.value as i64),
                )
            }
            "float" => {
                if !parameter.value.is_finite() {
                    return Err(format!(
                        "fixture scalar slot {} requires a finite value",
                        parameter.slot
                    ));
                }
                (
                    format!("f64={}", parameter.value),
                    "float",
                    json!(parameter.value),
                )
            }
            "color" => (
                format!(
                    "argb8={},{},{},{}",
                    parameter.color[0], parameter.color[1], parameter.color[2], parameter.color[3]
                ),
                "color",
                json!({
                    "alpha": parameter.color[0],
                    "red": parameter.color[1],
                    "green": parameter.color[2],
                    "blue": parameter.color[3]
                }),
            ),
            "angle" | "point" | "point3d" => {
                let expected = match parameter.kind.as_str() {
                    "angle" => 1,
                    "point" => 2,
                    _ => 3,
                };
                if parameter.component_count != expected
                    || parameter.components[..expected]
                        .iter()
                        .any(|value| !value.is_finite() || !(-32768.0..=32768.0).contains(value))
                {
                    return Err(format!(
                        "fixture component slot {} is invalid",
                        parameter.slot
                    ));
                }
                component_payload = true;
                let components = parameter.components[..expected].to_vec();
                (
                    format!(
                        "{}={}",
                        parameter.kind,
                        components
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    parameter.kind.as_str(),
                    json!(components),
                )
            }
            other => {
                return Err(format!(
                    "macOS fixture parameter kind {other:?} is not supported"
                ));
            }
        };
        if parameter.slot == 0
            || parameter.value < parameter.minimum
            || parameter.value > parameter.maximum
        {
            return Err(format!(
                "fixture scalar slot {} is outside its declared range",
                parameter.slot
            ));
        }
        assignments.push(format!(
            "param_{}@{}:{encoded}",
            parameter.slot, parameter.slot
        ));
        identity.push(json!({
            "id": format!("param_{}", parameter.slot),
            "slot": parameter.slot,
            "kind": kind,
            "value": value
        }));
    }
    let transport = format!(
        "{}|{}",
        if component_payload { "v4" } else { "v2" },
        assignments.join(";")
    );
    if transport.len() > MAX_PARAMETER_PAYLOAD_BYTES {
        return Err(format!(
            "fixture parameter payload exceeds {MAX_PARAMETER_PAYLOAD_BYTES} bytes"
        ));
    }
    Ok(FixtureParameterPayload {
        transport,
        identity: Value::Array(identity),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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
    fn guest_producer_large_parameter_boundaries() {
        for count in [1025, 1623, 4096] {
            let parameters = (1..=count)
                .map(|slot| fixture_parameter(slot, "integer", 23.))
                .collect::<Vec<_>>();
            let payload = fixture_parameter_payload(&parameters).unwrap();
            assert!(payload.transport.len() > 16384);
            assert_eq!(payload.identity.as_array().unwrap().len(), count as usize);
            assert!(
                payload
                    .transport
                    .ends_with(&format!("param_{count}@{count}:i32=23"))
            );
            let mut framed = Vec::new();
            write_control_message(&mut framed, &json!({"parameters":payload.transport})).unwrap();
            assert_eq!(
                u32::from_le_bytes(framed[..4].try_into().unwrap()) as usize,
                framed.len() - 4
            );
        }
        let excess = (1..=4097)
            .map(|slot| fixture_parameter(slot, "integer", 23.))
            .collect::<Vec<_>>();
        assert!(fixture_parameter_payload(&excess).is_err());
        assert!(fixture_parameter_payload(&[fixture_parameter(4097, "integer", 23.)]).is_err());
        assert!(
            fixture_parameter_payload(&[
                fixture_parameter(1, "integer", 23.),
                fixture_parameter(1, "integer", 23.)
            ])
            .is_err()
        );
    }
    #[test]
    fn guest_producer_request_framing_rejects_overflow_without_writing() {
        let limit = 1048576 + 65536;
        let value = json!({"parameters":"x".repeat(limit-17)});
        assert_eq!(serde_json::to_vec(&value).unwrap().len(), limit);
        let mut out = Vec::new();
        write_control_message(&mut out, &value).unwrap();
        assert_eq!(out.len(), limit + 4);
        let mut out = Vec::new();
        assert!(write_control_message(&mut out, &json!({"parameters":"x".repeat(limit)})).is_err());
        assert!(out.is_empty());
    }
}
