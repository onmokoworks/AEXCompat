use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    Integer,
    Float,
    Color,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColorValue {
    pub alpha: u8,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ParameterValue {
    Numeric(f64),
    Color(ColorValue),
}

impl ParameterValue {
    pub fn numeric(self) -> Option<f64> {
        match self {
            Self::Numeric(value) => Some(value),
            Self::Color(_) => None,
        }
    }

    pub fn color(self) -> Option<ColorValue> {
        match self {
            Self::Color(value) => Some(value),
            Self::Numeric(_) => None,
        }
    }
}

#[derive(Clone)]
pub struct Descriptor {
    pub id: String,
    pub display_name: String,
    pub slot: u32,
    pub observed_type: i32,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub default_value: ParameterValue,
    pub kind: ValueKind,
}

pub struct PluginProfile {
    pub id: String,
    pub descriptors: Vec<Descriptor>,
}

pub type ValidatedAssignments = BTreeMap<String, ParameterValue>;

#[derive(Debug, Serialize, PartialEq)]
pub struct ValidationError {
    pub code: &'static str,
    pub parameter: String,
    pub valid_min: f64,
    pub valid_max: f64,
}

pub fn validate(descriptor: &Descriptor, value: ParameterValue) -> Option<ValidationError> {
    if matches!(
        (descriptor.kind, value),
        (ValueKind::Color, ParameterValue::Color(_))
    ) {
        return None;
    }
    let Some(numeric) = value.numeric() else {
        return Some(ValidationError {
            code: "invalid_parameter_value",
            parameter: descriptor.display_name.clone(),
            valid_min: descriptor.minimum.unwrap_or(0.0),
            valid_max: descriptor.maximum.unwrap_or(255.0),
        });
    };
    let minimum = descriptor.minimum.unwrap_or(0.0);
    let maximum = descriptor.maximum.unwrap_or(0.0);
    let invalid_shape = !numeric.is_finite()
        || matches!(descriptor.kind, ValueKind::Integer) && numeric.fract() != 0.0
        || matches!(descriptor.kind, ValueKind::Color);
    let code = if invalid_shape {
        "invalid_parameter_value"
    } else if numeric < minimum || numeric > maximum {
        "parameter_out_of_range"
    } else {
        return None;
    };
    Some(ValidationError {
        code,
        parameter: descriptor.display_name.clone(),
        valid_min: minimum,
        valid_max: maximum,
    })
}

pub fn validate_assignments(
    profile: &PluginProfile,
    assignments: &BTreeMap<String, ParameterValue>,
) -> Result<(ValidatedAssignments, Vec<ValidationError>), String> {
    if assignments.len() > profile.descriptors.len() {
        return Err("assignment count exceeds descriptor count".into());
    }
    let mut validated = BTreeMap::new();
    let mut errors = Vec::new();
    for (id, value) in assignments {
        let descriptor = profile
            .descriptors
            .iter()
            .find(|descriptor| descriptor.id.as_str() == id)
            .ok_or_else(|| format!("unknown parameter id: {id}"))?;
        if let Some(error) = validate(descriptor, *value) {
            errors.push(error);
        } else {
            validated.insert(descriptor.id.clone(), *value);
        }
    }
    Ok((validated, errors))
}

pub fn apply_defaults(
    profile: &PluginProfile,
    assignments: &ValidatedAssignments,
) -> ValidatedAssignments {
    profile
        .descriptors
        .iter()
        .map(|descriptor| {
            (
                descriptor.id.clone(),
                assignments
                    .get(&descriptor.id)
                    .copied()
                    .unwrap_or(descriptor.default_value),
            )
        })
        .collect()
}

pub fn encode_worker_payload(
    profile: &PluginProfile,
    assignments: &ValidatedAssignments,
) -> Result<String, String> {
    let has_color = profile
        .descriptors
        .iter()
        .any(|descriptor| descriptor.kind == ValueKind::Color);
    let mut encoded = String::from(if has_color { "v3|" } else { "v2|" });
    for (index, descriptor) in profile.descriptors.iter().enumerate() {
        let value = assignments
            .get(&descriptor.id)
            .ok_or_else(|| format!("missing effective parameter: {}", descriptor.id))?;
        if index != 0 {
            encoded.push(';');
        }
        match (descriptor.kind, value) {
            (ValueKind::Integer, ParameterValue::Numeric(value)) => encoded.push_str(&format!(
                "{}@{}:i32={value}",
                descriptor.id, descriptor.slot
            )),
            (ValueKind::Float, ParameterValue::Numeric(value)) => encoded.push_str(&format!(
                "{}@{}:f64={value}",
                descriptor.id, descriptor.slot
            )),
            (ValueKind::Color, ParameterValue::Color(value)) => encoded.push_str(&format!(
                "{}@{}:argb8={},{},{},{}",
                descriptor.id, descriptor.slot, value.alpha, value.red, value.green, value.blue
            )),
            _ => return Err(format!("parameter kind/value mismatch: {}", descriptor.id)),
        }
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integer() -> Descriptor {
        Descriptor {
            id: "count".into(),
            display_name: "Count".into(),
            slot: 2,
            observed_type: 1,
            minimum: Some(-2.0),
            maximum: Some(4.0),
            default_value: ParameterValue::Numeric(1.0),
            kind: ValueKind::Integer,
        }
    }

    #[test]
    fn validates_generic_numeric_shape_and_range() {
        let descriptor = integer();
        assert!(validate(&descriptor, ParameterValue::Numeric(-2.0)).is_none());
        assert!(validate(&descriptor, ParameterValue::Numeric(4.0)).is_none());
        assert_eq!(
            validate(&descriptor, ParameterValue::Numeric(1.5))
                .unwrap()
                .code,
            "invalid_parameter_value"
        );
        assert_eq!(
            validate(&descriptor, ParameterValue::Numeric(5.0))
                .unwrap()
                .code,
            "parameter_out_of_range"
        );
        assert_eq!(
            validate(&descriptor, ParameterValue::Numeric(f64::NAN))
                .unwrap()
                .code,
            "invalid_parameter_value"
        );
    }

    #[test]
    fn assignment_validation_is_descriptor_driven_and_fail_closed() {
        let profile = PluginProfile {
            id: "example".into(),
            descriptors: vec![integer()],
        };
        let valid = BTreeMap::from([("count".to_string(), ParameterValue::Numeric(3.0))]);
        let (values, errors) = validate_assignments(&profile, &valid).unwrap();
        assert_eq!(values.get("count"), Some(&ParameterValue::Numeric(3.0)));
        assert!(errors.is_empty());

        let unknown = BTreeMap::from([("other".to_string(), ParameterValue::Numeric(1.0))]);
        assert_eq!(
            validate_assignments(&profile, &unknown).unwrap_err(),
            "unknown parameter id: other"
        );
        let effective = apply_defaults(&profile, &ValidatedAssignments::new());
        assert_eq!(effective.get("count"), Some(&ParameterValue::Numeric(1.0)));
        assert_eq!(
            encode_worker_payload(&profile, &effective).unwrap(),
            "v2|count@2:i32=1"
        );
    }

    #[test]
    fn color_values_are_typed_and_use_the_v3_worker_payload() {
        let color = ColorValue {
            alpha: 255,
            red: 20,
            green: 180,
            blue: 70,
        };
        let descriptor = Descriptor {
            id: "fill_color".into(),
            display_name: "Fill Color".into(),
            slot: 2,
            observed_type: 5,
            minimum: None,
            maximum: None,
            default_value: ParameterValue::Color(ColorValue {
                alpha: 255,
                red: 255,
                green: 255,
                blue: 255,
            }),
            kind: ValueKind::Color,
        };
        assert!(validate(&descriptor, ParameterValue::Color(color)).is_none());
        assert_eq!(
            validate(&descriptor, ParameterValue::Numeric(1.0))
                .unwrap()
                .code,
            "invalid_parameter_value"
        );
        let profile = PluginProfile {
            id: "example".into(),
            descriptors: vec![descriptor],
        };
        let effective =
            ValidatedAssignments::from([("fill_color".into(), ParameterValue::Color(color))]);
        assert_eq!(
            encode_worker_payload(&profile, &effective).unwrap(),
            "v3|fill_color@2:argb8=255,20,180,70"
        );
    }
}
