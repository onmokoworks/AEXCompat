use serde::Serialize;

#[derive(Clone, Copy)]
pub enum ValueKind {
    Integer,
    Float,
}

#[derive(Clone, Copy)]
pub struct Descriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub minimum: f64,
    pub maximum: f64,
    pub kind: ValueKind,
}

pub struct PluginProfile {
    pub id: &'static str,
    pub descriptors: &'static [Descriptor],
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ValidationError {
    pub code: &'static str,
    pub parameter: &'static str,
    pub valid_min: f64,
    pub valid_max: f64,
}

pub fn validate(descriptor: Descriptor, value: f64) -> Option<ValidationError> {
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
        parameter: descriptor.display_name,
        valid_min: descriptor.minimum,
        valid_max: descriptor.maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEGER: Descriptor = Descriptor {
        id: "count",
        display_name: "Count",
        minimum: -2.0,
        maximum: 4.0,
        kind: ValueKind::Integer,
    };

    #[test]
    fn validates_generic_numeric_shape_and_range() {
        assert!(validate(INTEGER, -2.0).is_none());
        assert!(validate(INTEGER, 4.0).is_none());
        assert_eq!(
            validate(INTEGER, 1.5).unwrap().code,
            "invalid_parameter_value"
        );
        assert_eq!(
            validate(INTEGER, 5.0).unwrap().code,
            "parameter_out_of_range"
        );
        assert_eq!(
            validate(INTEGER, f64::NAN).unwrap().code,
            "invalid_parameter_value"
        );
    }
}
