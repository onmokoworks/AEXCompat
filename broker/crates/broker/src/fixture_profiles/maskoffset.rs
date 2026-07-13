use crate::host_core::parameter::{ColorValue, ValidatedAssignments};
use sha2::{Digest, Sha256};

pub const PROFILE_ID: &str = "maskoffset";

fn source_argb8() -> Vec<u8> {
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
    source
}

pub fn source_argb8_hash() -> String {
    format!("{:X}", Sha256::digest(source_argb8()))
}

pub fn rectangle_mask_argb8_hash(assignments: &ValidatedAssignments) -> String {
    const WIDTH: i32 = 16;
    const HEIGHT: i32 = 12;
    let mut source = source_argb8();
    let mode = assignments
        .get("mode")
        .and_then(|value| value.numeric())
        .unwrap_or(1.0) as i32;
    let invert = assignments
        .get("invert")
        .and_then(|value| value.numeric())
        .unwrap_or(0.0)
        != 0.0;
    let color = assignments
        .get("fill_color")
        .and_then(|value| value.color())
        .unwrap_or(ColorValue {
            alpha: 255,
            red: 255,
            green: 255,
            blue: 255,
        });
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let offset = ((y * WIDTH + x) * 4) as usize;
            let mut inside = (4..12).contains(&x) && (3..9).contains(&y);
            if invert {
                inside = !inside;
            }
            match mode {
                1 | 2 if !inside => source[offset] = 0,
                3 if inside => {
                    source[offset] = color.alpha;
                    source[offset + 1] = color.red;
                    source[offset + 2] = color.green;
                    source[offset + 3] = color.blue;
                }
                4 if inside => source[offset] = 0,
                5 if !inside => {
                    source[offset] = color.alpha;
                    source[offset + 1] = color.red;
                    source[offset + 2] = color.green;
                    source[offset + 3] = color.blue;
                }
                _ => {}
            }
        }
    }
    format!("{:X}", Sha256::digest(source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_core::parameter::ParameterValue;

    #[test]
    fn source_oracle_is_fixed() {
        assert_eq!(
            source_argb8_hash(),
            "863D238F52F81ABA4017C198AF4D748CB57FE369E6216FDBACF45FD94037ECF7"
        );
    }

    #[test]
    fn rectangle_mask_oracle_is_fixed() {
        assert_eq!(
            rectangle_mask_argb8_hash(&ValidatedAssignments::from([
                ("mode".into(), ParameterValue::Numeric(2.0)),
                ("invert".into(), ParameterValue::Numeric(0.0)),
            ])),
            "D1003EF35A6EA3B037989F00672996FEA59867FBB50283A689AC95EDD9BF2359"
        );
    }

    #[test]
    fn custom_fill_color_oracle_is_fixed() {
        let values = ValidatedAssignments::from([
            ("mode".into(), ParameterValue::Numeric(3.0)),
            ("invert".into(), ParameterValue::Numeric(0.0)),
            (
                "fill_color".into(),
                ParameterValue::Color(ColorValue {
                    alpha: 255,
                    red: 20,
                    green: 180,
                    blue: 70,
                }),
            ),
        ]);
        assert_eq!(
            rectangle_mask_argb8_hash(&values),
            "BF419F44E915901BAC882E9B9E3411B8407DF7F4E3A4A1C2719314BDFBB74B5F"
        );
    }
}
