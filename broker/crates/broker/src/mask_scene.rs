//! Platform-independent host mask schema, validation, and wire encoding.
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskScene {
    pub masks: Vec<MaskShape>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskShape {
    pub open: bool,
    pub vertices: Vec<MaskPoint>,
}

#[derive(Clone, Debug, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub tangent_in: Option<MaskTangent>,
    #[serde(default)]
    pub tangent_out: Option<MaskTangent>,
}

#[derive(Clone, Debug, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaskTangent {
    pub x: f64,
    pub y: f64,
}

pub(crate) fn validate_mask_scene(scene: &MaskScene) -> io::Result<(usize, usize)> {
    if scene.masks.len() > 8 {
        return Err(invalid("host mask count exceeds 8"));
    }
    let mut vertex_count = 0usize;
    for mask in &scene.masks {
        let minimum = if mask.open { 2 } else { 3 };
        if !(minimum..=64).contains(&mask.vertices.len()) {
            return Err(invalid("host mask vertex count outside enabled range"));
        }
        vertex_count += mask.vertices.len();
        if vertex_count > 128 {
            return Err(invalid("host mask total vertex count exceeds 128"));
        }
        if mask.vertices.iter().any(|point| {
            [
                Some((point.x, point.y)),
                point.tangent_in.map(|value| (value.x, value.y)),
                point.tangent_out.map(|value| (value.x, value.y)),
            ]
            .into_iter()
            .flatten()
            .any(|(x, y)| {
                !x.is_finite()
                    || !y.is_finite()
                    || x < -32768.0
                    || x > 32768.0
                    || y < -32768.0
                    || y > 32768.0
            })
        }) {
            return Err(invalid("host mask coordinate is outside bounded range"));
        }
    }
    Ok((scene.masks.len(), vertex_count))
}

pub(crate) fn encode_mask_scene(scene: &MaskScene) -> io::Result<String> {
    validate_mask_scene(scene)?;
    let mut encoded = String::from("v2|");
    for (mask_index, mask) in scene.masks.iter().enumerate() {
        if mask_index != 0 {
            encoded.push(';');
        }
        encoded.push_str(if mask.open { "1:" } else { "0:" });
        for (vertex_index, point) in mask.vertices.iter().enumerate() {
            if vertex_index != 0 {
                encoded.push('/');
            }
            let tangent_in = point.tangent_in.unwrap_or(MaskTangent { x: 0.0, y: 0.0 });
            let tangent_out = point.tangent_out.unwrap_or(MaskTangent { x: 0.0, y: 0.0 });
            encoded.push_str(&format!(
                "{},{},{},{},{},{}",
                point.x, point.y, tangent_in.x, tangent_in.y, tangent_out.x, tangent_out.y
            ));
        }
    }
    if encoded.len() > 8192 {
        return Err(invalid("host mask transport exceeds 8192 bytes"));
    }
    Ok(encoded)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
