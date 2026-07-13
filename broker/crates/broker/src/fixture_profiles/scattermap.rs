use crate::host_core::parameter::{Descriptor, PluginProfile, ValidatedAssignments, ValueKind};
use sha2::{Digest, Sha256};

pub const PROFILE_ID: &str = "scattermap";

pub const DESCRIPTORS: [Descriptor; 5] = [
    Descriptor {
        id: "amount",
        display_name: "Scatter Amount",
        minimum: 0.0,
        maximum: 500.0,
        kind: ValueKind::Integer,
    },
    Descriptor {
        id: "direction",
        display_name: "Direction",
        minimum: 1.0,
        maximum: 3.0,
        kind: ValueKind::Integer,
    },
    Descriptor {
        id: "seed",
        display_name: "Random Seed",
        minimum: 0.0,
        maximum: 10_000.0,
        kind: ValueKind::Integer,
    },
    Descriptor {
        id: "mix",
        display_name: "Mix with Original",
        minimum: 0.0,
        maximum: 100.0,
        kind: ValueKind::Float,
    },
    Descriptor {
        id: "invert_map",
        display_name: "Invert Map",
        minimum: 0.0,
        maximum: 1.0,
        kind: ValueKind::Integer,
    },
];

pub static PROFILE: PluginProfile = PluginProfile {
    id: PROFILE_ID,
    descriptors: &DESCRIPTORS,
};

#[derive(Clone, Copy)]
pub struct RenderParameters {
    pub amount: i32,
    pub direction: i32,
    pub seed: i32,
    pub mix: f64,
    pub invert_map: i32,
}

pub fn bind(assignments: &ValidatedAssignments) -> RenderParameters {
    RenderParameters {
        amount: assignments.get("amount").copied().unwrap_or(5.0) as i32,
        direction: assignments.get("direction").copied().unwrap_or(3.0) as i32,
        seed: assignments.get("seed").copied().unwrap_or(0.0) as i32,
        mix: assignments.get("mix").copied().unwrap_or(100.0),
        invert_map: assignments.get("invert_map").copied().unwrap_or(0.0) as i32,
    }
}

pub fn worker_payload(parameters: RenderParameters) -> String {
    format!(
        "v2|amount@1:i32={};direction@2:i32={};seed@3:i32={};mix@5:f64={};invert_map@7:i32={}",
        parameters.amount,
        parameters.direction,
        parameters.seed,
        parameters.mix,
        parameters.invert_map
    )
}

fn hash_pixel(x: i32, y: i32, seed: i32, channel: i32) -> f32 {
    let mut value = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263))
        .wrapping_add((seed as u32).wrapping_mul(2_246_822_519))
        .wrapping_add((channel as u32).wrapping_mul(3_266_489_917));
    value ^= value >> 13;
    value = value.wrapping_mul(274_177);
    value ^= value >> 16;
    value = value.wrapping_mul(1_900_813);
    value ^= value >> 13;
    (value as f32 / u32::MAX as f32) * 2.0 - 1.0
}

pub fn argb8_hash(amount: i32, direction: i32, seed: i32, mix: f64) -> String {
    const WIDTH: i32 = 16;
    const HEIGHT: i32 = 12;
    let mut source = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let offset = ((y * WIDTH + x) * 4) as usize;
            source[offset] = 255;
            source[offset + 1] = (x * 255 / (WIDTH - 1)) as u8;
            source[offset + 2] = (y * 255 / (HEIGHT - 1)) as u8;
            source[offset + 3] = ((x + y) * 255 / (WIDTH + HEIGHT - 2)) as u8;
        }
    }
    let mut output = vec![0u8; source.len()];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let dx = if direction == 1 || direction == 3 {
                (hash_pixel(x, y, seed, 0) * amount as f32).round() as i32
            } else {
                0
            };
            let dy = if direction == 2 || direction == 3 {
                (hash_pixel(x, y, seed, 1) * amount as f32).round() as i32
            } else {
                0
            };
            let source_x = (x + dx).clamp(0, WIDTH - 1);
            let source_y = (y + dy).clamp(0, HEIGHT - 1);
            let from = ((source_y * WIDTH + source_x) * 4) as usize;
            let to = ((y * WIDTH + x) * 4) as usize;
            output[to..to + 4].copy_from_slice(&source[from..from + 4]);
        }
    }
    if mix < 100.0 {
        let ratio = mix as f32 / 100.0f32;
        let inverse = 1.0f32 - ratio;
        for (result, original) in output.iter_mut().zip(source) {
            *result = ((original as f32 * inverse) + (*result as f32 * ratio)) as u8;
        }
    }
    format!("{:X}", Sha256::digest(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_core::parameter::validate;

    #[test]
    fn profile_matches_the_production_ae_negative_matrix() {
        for (value, index) in [
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
        ] {
            assert_eq!(
                validate(DESCRIPTORS[index], value).unwrap().code,
                "parameter_out_of_range"
            );
        }
    }

    #[test]
    fn oracle_matches_the_independent_matrix() {
        for (parameters, expected) in [
            (
                (5, 3, 0, 100.0),
                "19CEA826F356E0D94BC29FF10CB9E7F5A770FE5B288CB3D190A58372353102D9",
            ),
            (
                (9, 1, 17, 100.0),
                "82E72A2E7C05E831A45980FA4042940B8B84C4B6CCF020057968E88ACA323779",
            ),
            (
                (13, 2, 1234, 33.333333333),
                "3905F287DBF3042CD73527154B8B6DA89E21A86C3ECDB6902D51A67F2CB79CF1",
            ),
        ] {
            assert_eq!(
                argb8_hash(parameters.0, parameters.1, parameters.2, parameters.3),
                expected
            );
        }
    }

    #[test]
    fn adapter_applies_observed_defaults_after_generic_validation() {
        let values = ValidatedAssignments::from([("amount", 13.0), ("mix", 25.5)]);
        let bound = bind(&values);
        assert_eq!(bound.amount, 13);
        assert_eq!(bound.direction, 3);
        assert_eq!(bound.seed, 0);
        assert_eq!(bound.mix, 25.5);
        assert_eq!(bound.invert_map, 0);
        assert_eq!(
            worker_payload(bound),
            "v2|amount@1:i32=13;direction@2:i32=3;seed@3:i32=0;mix@5:f64=25.5;invert_map@7:i32=0"
        );
    }
}
