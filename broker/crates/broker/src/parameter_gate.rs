use serde::Serialize;

#[derive(Clone, Copy)]
pub enum ValueKind {
    Integer,
    Float,
}

#[derive(Clone, Copy)]
pub struct Descriptor {
    pub name: &'static str,
    pub minimum: f64,
    pub maximum: f64,
    pub kind: ValueKind,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct ValidationError {
    pub code: &'static str,
    pub parameter: &'static str,
    pub valid_min: f64,
    pub valid_max: f64,
}

pub const SCATTERMAP_DESCRIPTORS: [Descriptor; 5] = [
    Descriptor {
        name: "Scatter Amount",
        minimum: 0.0,
        maximum: 500.0,
        kind: ValueKind::Integer,
    },
    Descriptor {
        name: "Direction",
        minimum: 1.0,
        maximum: 3.0,
        kind: ValueKind::Integer,
    },
    Descriptor {
        name: "Random Seed",
        minimum: 0.0,
        maximum: 10_000.0,
        kind: ValueKind::Integer,
    },
    Descriptor {
        name: "Mix with Original",
        minimum: 0.0,
        maximum: 100.0,
        kind: ValueKind::Float,
    },
    Descriptor {
        name: "Invert Map",
        minimum: 0.0,
        maximum: 1.0,
        kind: ValueKind::Integer,
    },
];

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
        parameter: descriptor.name,
        valid_min: descriptor.minimum,
        valid_max: descriptor.maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_production_ae_negative_matrix() {
        let cases = [
            (-1.0, 0),
            (501.0, 0),
            (0.0, 1),
            (4.0, 1),
            (-1.0, 2),
            (10_001.0, 2),
            (-0.1, 3),
            (100.1, 3),
            (-1.0, 4),
            (2.0, 4),
        ];
        for (value, index) in cases {
            assert_eq!(
                validate(SCATTERMAP_DESCRIPTORS[index], value).unwrap().code,
                "parameter_out_of_range"
            );
        }
    }

    #[test]
    fn accepts_observed_endpoints_and_rejects_fractional_integers() {
        for (value, index) in [(500.0, 0), (1.0, 1), (10_000.0, 2), (0.0, 3), (1.0, 4)] {
            assert!(validate(SCATTERMAP_DESCRIPTORS[index], value).is_none());
        }
        assert_eq!(
            validate(SCATTERMAP_DESCRIPTORS[0], 1.5).unwrap().code,
            "invalid_parameter_value"
        );
    }
}
