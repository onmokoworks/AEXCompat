use crate::host_core::parameter::{ColorValue, ValidatedAssignments};
use sha2::{Digest, Sha256};

pub const PROFILE_ID: &str = "maskoffset";

#[derive(Clone, Copy)]
pub struct OracleMaskVertex {
    pub x: f64,
    pub y: f64,
    pub tangent_in_x: f64,
    pub tangent_in_y: f64,
    pub tangent_out_x: f64,
    pub tangent_out_y: f64,
}

pub struct OracleMask {
    pub open: bool,
    pub vertices: Vec<OracleMaskVertex>,
}

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
    mask_scene_argb8_hash(assignments, "rectangle").expect("registered rectangle scene")
}

pub fn mask_scene_argb8_hash(assignments: &ValidatedAssignments, scene_id: &str) -> Option<String> {
    const WIDTH: i32 = 16;
    const HEIGHT: i32 = 12;
    let mut source = source_argb8();
    let mask_index = assignments
        .get("mask_index")
        .and_then(|value| value.numeric())
        .unwrap_or(1.0) as usize;
    let bounds = match (scene_id, mask_index) {
        ("empty", _) => None,
        ("rectangle", 1) => Some((4, 3, 12, 9)),
        ("translated_rectangle", 1) => Some((2, 2, 10, 8)),
        ("two_rectangles", 1) => Some((1, 1, 7, 6)),
        ("two_rectangles", 2) => Some((9, 5, 15, 11)),
        ("rectangle" | "translated_rectangle", _) => None,
        _ => return None,
    };
    if bounds.is_none() {
        return Some(format!("{:X}", Sha256::digest(source)));
    }
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
            let mut inside = bounds.is_some_and(|(left, top, right, bottom)| {
                (left..right).contains(&x) && (top..bottom).contains(&y)
            });
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
    Some(format!("{:X}", Sha256::digest(source)))
}

pub fn polygon_mask_argb8_hash(
    assignments: &ValidatedAssignments,
    masks: &[Vec<(f64, f64)>],
) -> Option<String> {
    const WIDTH: i32 = 16;
    const HEIGHT: i32 = 12;
    let mask_index = assignments
        .get("mask_index")
        .and_then(|value| value.numeric())
        .unwrap_or(1.0) as usize;
    let Some(points) = mask_index.checked_sub(1).and_then(|index| masks.get(index)) else {
        return Some(source_argb8_hash());
    };
    if points.is_empty() {
        return Some(source_argb8_hash());
    }
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
    let mut source = source_argb8();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let sample_x = x as f64 + 0.5;
            let sample_y = y as f64 + 0.5;
            let mut inside = false;
            let mut previous = points.len() - 1;
            for current in 0..points.len() {
                let (x1, y1) = points[current];
                let (x2, y2) = points[previous];
                if (y1 > sample_y) != (y2 > sample_y)
                    && sample_x < (x2 - x1) * (sample_y - y1) / (y2 - y1) + x1
                {
                    inside = !inside;
                }
                previous = current;
            }
            if invert {
                inside = !inside;
            }
            let offset = ((y * WIDTH + x) * 4) as usize;
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
    Some(format!("{:X}", Sha256::digest(source)))
}

pub fn bezier_mask_argb8_hash(
    assignments: &ValidatedAssignments,
    masks: &[OracleMask],
) -> Option<String> {
    let polygons = masks
        .iter()
        .map(|mask| {
            // MaskOffset reads [0..num_segments), while the SDK exposes vertex
            // [0..num_segments]. Preserve that observable behavior for open paths.
            let observed_len = if mask.open {
                mask.vertices.len().saturating_sub(1)
            } else {
                mask.vertices.len()
            };
            let vertices = &mask.vertices[..observed_len];
            if vertices.is_empty() {
                return Vec::new();
            }
            let segment_count = if mask.open {
                vertices.len().saturating_sub(1)
            } else {
                vertices.len()
            };
            let mut polygon = Vec::with_capacity(segment_count * 32 + usize::from(mask.open));
            for segment in 0..segment_count {
                let first = vertices[segment];
                let second = if mask.open {
                    vertices[segment + 1]
                } else {
                    vertices[(segment + 1) % vertices.len()]
                };
                let p0 = (first.x, first.y);
                let p1 = (first.x + first.tangent_out_x, first.y + first.tangent_out_y);
                let p2 = (
                    second.x + second.tangent_in_x,
                    second.y + second.tangent_in_y,
                );
                let p3 = (second.x, second.y);
                for sample in 0..32 {
                    let t = sample as f64 / 32.0;
                    let u = 1.0 - t;
                    polygon.push((
                        u * u * u * p0.0
                            + 3.0 * u * u * t * p1.0
                            + 3.0 * u * t * t * p2.0
                            + t * t * t * p3.0,
                        u * u * u * p0.1
                            + 3.0 * u * u * t * p1.1
                            + 3.0 * u * t * t * p2.1
                            + t * t * t * p3.1,
                    ));
                }
            }
            if mask.open {
                if let Some(last) = vertices.last() {
                    polygon.push((last.x, last.y));
                }
            }
            polygon
        })
        .collect::<Vec<_>>();
    polygon_mask_argb8_hash(assignments, &polygons)
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

    #[test]
    fn host_scene_oracles_are_distinct_and_mask_index_bound() {
        let first = ValidatedAssignments::from([
            ("mode".into(), ParameterValue::Numeric(2.0)),
            ("mask_index".into(), ParameterValue::Numeric(1.0)),
        ]);
        let second = ValidatedAssignments::from([
            ("mode".into(), ParameterValue::Numeric(2.0)),
            ("mask_index".into(), ParameterValue::Numeric(2.0)),
        ]);
        assert_ne!(
            mask_scene_argb8_hash(&first, "translated_rectangle"),
            mask_scene_argb8_hash(&first, "rectangle")
        );
        assert_ne!(
            mask_scene_argb8_hash(&first, "two_rectangles"),
            mask_scene_argb8_hash(&second, "two_rectangles")
        );
        assert!(mask_scene_argb8_hash(&first, "arbitrary").is_none());
    }

    #[test]
    fn polygon_oracle_matches_the_original_rectangle() {
        let values = ValidatedAssignments::from([
            ("mode".into(), ParameterValue::Numeric(2.0)),
            ("mask_index".into(), ParameterValue::Numeric(1.0)),
            ("invert".into(), ParameterValue::Numeric(0.0)),
        ]);
        let rectangle = vec![vec![(4.0, 3.0), (12.0, 3.0), (12.0, 9.0), (4.0, 9.0)]];
        assert_eq!(
            polygon_mask_argb8_hash(&values, &rectangle),
            Some(rectangle_mask_argb8_hash(&values))
        );
    }

    #[test]
    fn zero_tangent_bezier_oracle_matches_polygon_oracle() {
        let values = ValidatedAssignments::from([
            ("mode".into(), ParameterValue::Numeric(2.0)),
            ("mask_index".into(), ParameterValue::Numeric(1.0)),
        ]);
        let points = [(4.0, 3.0), (12.0, 3.0), (12.0, 9.0), (4.0, 9.0)];
        let mask = OracleMask {
            open: false,
            vertices: points
                .iter()
                .map(|&(x, y)| OracleMaskVertex {
                    x,
                    y,
                    tangent_in_x: 0.0,
                    tangent_in_y: 0.0,
                    tangent_out_x: 0.0,
                    tangent_out_y: 0.0,
                })
                .collect(),
        };
        assert_eq!(
            bezier_mask_argb8_hash(&values, &[mask]),
            polygon_mask_argb8_hash(&values, &[points.to_vec()])
        );
    }
}
