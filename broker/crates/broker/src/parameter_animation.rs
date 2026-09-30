//! Platform-independent parameter-animation sidecar contract.
//!
//! Windows render dispatch consumes these values, but their validation and
//! serialization are bounded data processing that must remain testable from
//! the Apple Silicon broker workspace.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;

const MAX_PARAMETERS: u32 = 1024;
const MAX_ANIMATION_KEYS_PER_PARAMETER: usize = 256;
const MAX_ANIMATION_KEYS_TOTAL: usize = 4096;
const MAX_ANIMATION_JSON_BYTES: usize = 1024 * 1024;
const MAX_ARBITRARY_KEY_BYTES: usize = 64 * 1024;
const MAX_ARBITRARY_TOTAL_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationTime {
    pub value: i32,
    pub scale: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationInterpolation {
    Hold,
    Linear,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationValue {
    Scalar {
        value: f64,
    },
    Color {
        value: [u8; 4],
    },
    /// Component values use the selected parameter's public units. In
    /// particular, POINT and POINT_3D components are percentages of the input
    /// layer, matching the corresponding interactive parameter values.
    Components {
        value: Vec<f64>,
    },
    Arbitrary {
        value: Vec<u8>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterAnimationKey {
    pub time: AnimationTime,
    pub interpolation: AnimationInterpolation,
    pub value: AnimationValue,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterAnimation {
    pub slot: u32,
    pub keys: Vec<ParameterAnimationKey>,
}

#[derive(Serialize)]
struct ParameterAnimationSidecar<'a> {
    schema_version: u32,
    parameters: &'a [ParameterAnimation],
}

pub fn parameter_animation_sidecar_json(animations: &[ParameterAnimation]) -> io::Result<Vec<u8>> {
    let mut slots = HashSet::new();
    let mut total_keys = 0usize;
    let mut total_arbitrary_bytes = 0usize;
    for animation in animations {
        if animation.slot == 0 || animation.slot > MAX_PARAMETERS || !slots.insert(animation.slot) {
            return Err(invalid(
                "parameter animation slots must be unique and in range",
            ));
        }
        if animation.keys.is_empty() || animation.keys.len() > MAX_ANIMATION_KEYS_PER_PARAMETER {
            return Err(invalid("parameter animation key count is out of range"));
        }
        total_keys = total_keys
            .checked_add(animation.keys.len())
            .ok_or_else(|| invalid("parameter animation key count overflow"))?;
        if total_keys > MAX_ANIMATION_KEYS_TOTAL {
            return Err(invalid("total parameter animation key count exceeds limit"));
        }
        let mut previous: Option<AnimationTime> = None;
        for key in &animation.keys {
            if key.time.scale == 0 {
                return Err(invalid("parameter animation time scale must be nonzero"));
            }
            if let Some(prior) = previous {
                let left = i64::from(prior.value)
                    .checked_mul(i64::from(key.time.scale))
                    .ok_or_else(|| invalid("parameter animation time comparison overflow"))?;
                let right = i64::from(key.time.value)
                    .checked_mul(i64::from(prior.scale))
                    .ok_or_else(|| invalid("parameter animation time comparison overflow"))?;
                if left >= right {
                    return Err(invalid(
                        "parameter animation times must be strictly ascending",
                    ));
                }
            }
            match &key.value {
                AnimationValue::Scalar { value } if !value.is_finite() => {
                    return Err(invalid("parameter animation scalar must be finite"));
                }
                AnimationValue::Components { value }
                    if value.is_empty()
                        || value.len() > 3
                        || value.iter().any(|component| !component.is_finite()) =>
                {
                    return Err(invalid("parameter animation components are invalid"));
                }
                AnimationValue::Arbitrary { value }
                    if value.is_empty() || value.len() > MAX_ARBITRARY_KEY_BYTES =>
                {
                    return Err(invalid("parameter animation arbitrary key is invalid"));
                }
                AnimationValue::Arbitrary { value } => {
                    total_arbitrary_bytes = total_arbitrary_bytes
                        .checked_add(value.len())
                        .ok_or_else(|| {
                            invalid("parameter animation arbitrary byte count overflow")
                        })?;
                    if total_arbitrary_bytes > MAX_ARBITRARY_TOTAL_BYTES {
                        return Err(invalid(
                            "parameter animation arbitrary bytes exceed total limit",
                        ));
                    }
                }
                _ => {}
            }
            previous = Some(key.time);
        }
        let has_arbitrary = animation
            .keys
            .iter()
            .any(|key| matches!(key.value, AnimationValue::Arbitrary { .. }));
        if has_arbitrary
            && animation
                .keys
                .iter()
                .any(|key| !matches!(key.value, AnimationValue::Arbitrary { .. }))
        {
            return Err(invalid(
                "arbitrary parameter animation keys cannot mix value types",
            ));
        }
    }
    let bytes = serde_json::to_vec(&ParameterAnimationSidecar {
        schema_version: 1,
        parameters: animations,
    })
    .map_err(|error| invalid(format!("parameter animation JSON failed: {error}")))?;
    if bytes.len() > MAX_ANIMATION_JSON_BYTES {
        return Err(invalid("parameter animation JSON exceeds byte limit"));
    }
    Ok(bytes)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
