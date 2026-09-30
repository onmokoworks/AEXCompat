use aexcompat_broker::render_fixture::InteractiveParameter;
use serde_json::Value;

pub(crate) fn parameters_from_guest_setup(
    value: &Value,
) -> Result<(Vec<InteractiveParameter>, Vec<InteractiveParameter>), String> {
    let declared = value["parameters"]
        .as_array()
        .ok_or_else(|| "setup report has no parameters array".to_string())?;
    let custom_ui_events = value["custom_ui"]["events"].as_u64().unwrap_or(0) as u32;
    let mut parameters = Vec::new();
    let mut defaults = Vec::new();
    for parameter in declared {
        let param_type = parameter["param_type"]
            .as_i64()
            .ok_or_else(|| "parameter has no numeric param_type".to_string())?;
        let slot = parameter["slot"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| "parameter has no numeric slot".to_string())?;
        let kind = match param_type {
            0 => "layer",
            1 | 4 | 7 => "integer",
            2 | 10 => "float",
            3 => "angle",
            5 => "color",
            6 => "point",
            8 => "custom",
            9 => "no_data",
            11 => "arbitrary_data",
            12 => "path",
            13 => "group_start",
            14 => "group_end",
            15 => "button",
            18 => "point3d",
            other => {
                return Err(format!(
                    "parameter slot {slot} has unsupported type {other}"
                ));
            }
        };
        let name = parameter["name"]
            .as_str()
            .ok_or_else(|| "parameter has no name".to_string())?
            .to_string();
        let default_value = parameter["default_value"]
            .as_f64()
            .or_else(|| parameter["default"].as_f64())
            .unwrap_or(0.0);
        let value = parameter["current_value"]
            .as_f64()
            .or_else(|| parameter["current"].as_f64())
            .unwrap_or(default_value);
        let minimum = parameter["slider_min"]
            .as_f64()
            .or_else(|| parameter["valid_min"].as_f64())
            .unwrap_or(value);
        let maximum = parameter["slider_max"]
            .as_f64()
            .or_else(|| parameter["valid_max"].as_f64())
            .unwrap_or(value);
        if !value.is_finite()
            || !default_value.is_finite()
            || !minimum.is_finite()
            || !maximum.is_finite()
            || minimum > maximum
        {
            return Err(format!(
                "editable parameter {name:?} has an invalid range {minimum}..={maximum}"
            ));
        }
        let component_count = match kind {
            "angle" => 1,
            "point" => 2,
            "point3d" => 3,
            _ => 0,
        };
        let default_color = if kind == "color" {
            parse_argb8(parameter, "default_color", &name)?
        } else {
            [255, 0, 0, 0]
        };
        let color = if kind == "color" {
            parse_argb8(parameter, "current_color", &name).unwrap_or(default_color)
        } else {
            [255, 0, 0, 0]
        };
        let mut components = [0.0; 3];
        let mut default_components = [0.0; 3];
        if component_count > 0 {
            let source = parameter["default_components"]
                .as_array()
                .ok_or_else(|| format!("component parameter {name:?} has no default values"))?;
            if source.len() != component_count {
                return Err(format!(
                    "component parameter {name:?} must contain {component_count} values"
                ));
            }
            copy_finite_components(&mut default_components, source, &name)?;
            components = default_components;
            if let Some(source) = parameter["current_components"].as_array() {
                if source.len() != component_count {
                    return Err(format!(
                        "component parameter {name:?} current value must contain {component_count} values"
                    ));
                }
                copy_finite_components(&mut components, source, &name)?;
            }
        }
        let ui_flags = parameter["ui_flags"].as_u64().unwrap_or(0);
        let current = InteractiveParameter {
            slot,
            name,
            kind: kind.into(),
            minimum,
            maximum,
            value: value.clamp(minimum, maximum),
            choices: parameter["choices"]
                .as_str()
                .filter(|choices| !choices.is_empty())
                .map(|choices| choices.split('|').map(str::to_owned).collect())
                .unwrap_or_default(),
            color,
            components,
            component_count,
            layer_path: None,
            enabled: ui_flags & (1 << 5) == 0,
            visible: ui_flags & (1 << 9) == 0,
            supervised: parameter["flags"].as_u64().unwrap_or(0) & (1 << 6) != 0,
            debug_summary: parameter["arbitrary_summary"]
                .as_str()
                .filter(|summary| !summary.is_empty())
                .map(str::to_owned),
            custom_ui_events,
            control_size: [
                parameter["ui_width"]
                    .as_u64()
                    .unwrap_or(0)
                    .min(u16::MAX as u64) as u16,
                parameter["ui_height"]
                    .as_u64()
                    .unwrap_or(0)
                    .min(u16::MAX as u64) as u16,
            ],
        };
        let mut default = current.clone();
        default.value = default_value.clamp(minimum, maximum);
        default.color = default_color;
        default.components = default_components;
        parameters.push(current);
        defaults.push(default);
    }
    Ok((parameters, defaults))
}

fn copy_finite_components(
    destination: &mut [f64; 3],
    source: &[Value],
    name: &str,
) -> Result<(), String> {
    for (destination, source) in destination.iter_mut().zip(source) {
        *destination = source
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("component parameter {name:?} contains a non-finite value"))?;
    }
    Ok(())
}

fn parse_argb8(parameter: &Value, field: &str, name: &str) -> Result<[u8; 4], String> {
    let components = parameter[field]
        .as_array()
        .ok_or_else(|| format!("color parameter {name:?} has no {field} ARGB8 array"))?;
    if components.len() != 4 {
        return Err(format!(
            "color parameter {name:?} {field} must contain four components"
        ));
    }
    let mut color = [0u8; 4];
    for (destination, component) in color.iter_mut().zip(components) {
        *destination = component
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                format!("color parameter {name:?} {field} components must be 0..=255")
            })?;
    }
    Ok(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn descriptor_conversion_preserves_order_state_and_complete_kinds() {
        let kinds = [
            (13, "group_start"),
            (1, "integer"),
            (2, "float"),
            (3, "angle"),
            (5, "color"),
            (6, "point"),
            (18, "point3d"),
            (0, "layer"),
            (11, "arbitrary_data"),
            (12, "path"),
            (15, "button"),
            (8, "custom"),
            (9, "no_data"),
            (14, "group_end"),
        ];
        let rows = kinds
            .iter()
            .enumerate()
            .map(|(index, (param_type, _))| {
                let mut row = json!({
                    "slot": index + 1, "name": format!("p{index}"), "param_type": param_type,
                    "default_value": 1.0, "current_value": 2.0, "slider_min": 0.0,
                    "slider_max": 10.0, "ui_flags": if index == 1 { 1 << 9 } else { 0 },
                    "flags": if index == 2 { 1 << 6 } else { 0 },
                });
                if *param_type == 5 {
                    row["default_color"] = json!([255, 1, 2, 3]);
                    row["current_color"] = json!([255, 4, 5, 6]);
                }
                let count = match *param_type {
                    3 => 1,
                    6 => 2,
                    18 => 3,
                    _ => 0,
                };
                if count > 0 {
                    row["default_components"] = json!([1.0, 2.0, 3.0][..count]);
                    row["current_components"] = json!([4.0, 5.0, 6.0][..count]);
                }
                row
            })
            .collect::<Vec<_>>();
        let (current, defaults) = parameters_from_guest_setup(&json!({
            "parameters": rows, "custom_ui": {"events": 7}
        }))
        .unwrap();
        assert_eq!(
            current.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(),
            kinds.iter().map(|(_, kind)| *kind).collect::<Vec<_>>()
        );
        assert!(!current[1].visible);
        assert!(current[2].supervised);
        assert_eq!(current[4].color, [255, 4, 5, 6]);
        assert_eq!(defaults[4].color, [255, 1, 2, 3]);
        assert_eq!(current[6].components, [4.0, 5.0, 6.0]);
        assert_eq!(defaults[6].components, [1.0, 2.0, 3.0]);
        assert!(
            current
                .iter()
                .all(|parameter| parameter.custom_ui_events == 7)
        );
    }

    #[test]
    fn descriptor_conversion_rejects_unknown_and_malformed_values() {
        for row in [
            json!({"slot": 1, "name": "bad", "param_type": 99}),
            json!({"slot": 1, "name": "bad", "param_type": 18, "default_components": [1.0, 2.0]}),
            json!({"slot": 1, "name": "bad", "param_type": 5, "default_color": [256, 0, 0, 0]}),
        ] {
            assert!(parameters_from_guest_setup(&json!({"parameters": [row]})).is_err());
        }
    }
}
