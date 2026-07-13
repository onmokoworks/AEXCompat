use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    Integer,
    Float,
}

#[derive(Clone)]
pub struct Descriptor {
    pub id: String,
    pub display_name: String,
    pub slot: u32,
    pub observed_type: i32,
    pub minimum: f64,
    pub maximum: f64,
    pub default_value: f64,
    pub kind: ValueKind,
}

pub struct PluginProfile {
    pub id: String,
    pub descriptors: Vec<Descriptor>,
}

pub type ValidatedAssignments = BTreeMap<String, f64>;

#[derive(Debug, Serialize, PartialEq)]
pub struct ValidationError {
    pub code: &'static str,
    pub parameter: String,
    pub valid_min: f64,
    pub valid_max: f64,
}

pub fn validate(descriptor: &Descriptor, value: f64) -> Option<ValidationError> {
    let invalid_shape =
        !value.is_finite() || matches!(descriptor.kind, ValueKind::Integer) && value.fract() != 0.0;
    let code = if invalid_shape {
        "invalid_parameter_value"
    } else if value < descriptor.minimum || value > descriptor.maximum {
        "parameter_out_of_range"
    } else {
        return None;
    };
    Some(ValidationError {
        code,
        parameter: descriptor.display_name.clone(),
        valid_min: descriptor.minimum,
        valid_max: descriptor.maximum,
    })
}

pub fn validate_assignments(
    profile: &PluginProfile,
    assignments: &BTreeMap<String, f64>,
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
    let mut encoded = String::from("v2|");
    for (index, descriptor) in profile.descriptors.iter().enumerate() {
        let value = assignments
            .get(&descriptor.id)
            .ok_or_else(|| format!("missing effective parameter: {}", descriptor.id))?;
        if index != 0 {
            encoded.push(';');
        }
        let kind = match descriptor.kind {
            ValueKind::Integer => "i32",
            ValueKind::Float => "f64",
        };
        encoded.push_str(&format!(
            "{}@{}:{}={}",
            descriptor.id, descriptor.slot, kind, value
        ));
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
            minimum: -2.0,
            maximum: 4.0,
            default_value: 1.0,
            kind: ValueKind::Integer,
        }
    }

    #[test]
    fn validates_generic_numeric_shape_and_range() {
        let descriptor = integer();
        assert!(validate(&descriptor, -2.0).is_none());
        assert!(validate(&descriptor, 4.0).is_none());
        assert_eq!(
            validate(&descriptor, 1.5).unwrap().code,
            "invalid_parameter_value"
        );
        assert_eq!(
            validate(&descriptor, 5.0).unwrap().code,
            "parameter_out_of_range"
        );
        assert_eq!(
            validate(&descriptor, f64::NAN).unwrap().code,
            "invalid_parameter_value"
        );
    }

    #[test]
    fn assignment_validation_is_descriptor_driven_and_fail_closed() {
        let profile = PluginProfile {
            id: "example".into(),
            descriptors: vec![integer()],
        };
        let valid = BTreeMap::from([("count".to_string(), 3.0)]);
        let (values, errors) = validate_assignments(&profile, &valid).unwrap();
        assert_eq!(values.get("count"), Some(&3.0));
        assert!(errors.is_empty());

        let unknown = BTreeMap::from([("other".to_string(), 1.0)]);
        assert_eq!(
            validate_assignments(&profile, &unknown).unwrap_err(),
            "unknown parameter id: other"
        );
        let effective = apply_defaults(&profile, &ValidatedAssignments::new());
        assert_eq!(effective.get("count"), Some(&1.0));
        assert_eq!(
            encode_worker_payload(&profile, &effective).unwrap(),
            "v2|count@2:i32=1"
        );
    }
}
