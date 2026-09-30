//! Bounded, pointer-free parameter-animation values for the staged host core.
//!
//! The production worker keeps its C++ evaluator. This module evaluates the
//! same typed values for an opt-in native dual run; it does not own SDK state.

use crate::error::{HostError, HostErrorCode};
use std::mem::{align_of, offset_of, size_of};

pub const HOST_PARAMETER_ANIMATION_ABI_VERSION: u32 = 1;
pub const HOST_PARAMETER_ANIMATION_ABI_DESCRIPTOR_MAGIC: u64 = 0x4145_5850_414e_4931;
pub const HOST_PARAMETER_ANIMATION_CAPABILITY_EVALUATE_V1: u64 = 1;
pub const HOST_PARAMETER_ANIMATION_MAX_KEYS: usize = 256;

pub mod value_kind {
    pub const SCALAR: u32 = 1;
    pub const COLOR: u32 = 2;
    pub const COMPONENTS: u32 = 3;
}

pub mod interpolation {
    pub const HOLD: u32 = 1;
    pub const LINEAR: u32 = 2;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostRationalTime {
    pub value: i32,
    pub scale: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HostAnimationKey {
    pub time: HostRationalTime,
    pub kind: u32,
    pub interpolation: u32,
    pub scalar: f64,
    pub components: [f64; 3],
    pub color: [u8; 4],
    pub component_count: u32,
    pub reserved: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HostAnimationValue {
    pub kind: u32,
    pub component_count: u32,
    pub scalar: f64,
    pub components: [f64; 3],
    pub color: [u8; 4],
    pub reserved: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostParameterAnimationAbiDescriptorV1 {
    pub magic: u64,
    pub abi_version: u32,
    pub struct_size: u32,
    pub time_size: u32,
    pub time_alignment: u32,
    pub key_size: u32,
    pub key_alignment: u32,
    pub output_size: u32,
    pub output_alignment: u32,
    pub max_keys: u32,
    pub reserved: u32,
    pub capabilities: u64,
}

impl HostParameterAnimationAbiDescriptorV1 {
    pub const fn current() -> Self {
        Self {
            magic: HOST_PARAMETER_ANIMATION_ABI_DESCRIPTOR_MAGIC,
            abi_version: HOST_PARAMETER_ANIMATION_ABI_VERSION,
            struct_size: size_of::<Self>() as u32,
            time_size: size_of::<HostRationalTime>() as u32,
            time_alignment: align_of::<HostRationalTime>() as u32,
            key_size: size_of::<HostAnimationKey>() as u32,
            key_alignment: align_of::<HostAnimationKey>() as u32,
            output_size: size_of::<HostAnimationValue>() as u32,
            output_alignment: align_of::<HostAnimationValue>() as u32,
            max_keys: HOST_PARAMETER_ANIMATION_MAX_KEYS as u32,
            reserved: 0,
            capabilities: HOST_PARAMETER_ANIMATION_CAPABILITY_EVALUATE_V1,
        }
    }
}

fn invalid(operation: &'static str) -> HostError {
    HostError::new(HostErrorCode::InvalidArgument, operation)
}

fn rational_less(left: HostRationalTime, right: HostRationalTime) -> bool {
    i128::from(left.value) * i128::from(right.scale)
        < i128::from(right.value) * i128::from(left.scale)
}

fn validate_key(key: &HostAnimationKey) -> Result<(), HostError> {
    if key.time.scale == 0
        || key.reserved != 0
        || !matches!(
            key.interpolation,
            interpolation::HOLD | interpolation::LINEAR
        )
        || !key.scalar.is_finite()
        || key
            .components
            .iter()
            .any(|component| !component.is_finite())
    {
        return Err(invalid("validate_parameter_animation_key"));
    }
    match key.kind {
        value_kind::SCALAR | value_kind::COLOR if key.component_count == 0 => Ok(()),
        value_kind::COMPONENTS if (1..=3).contains(&key.component_count) => Ok(()),
        _ => Err(invalid("validate_parameter_animation_kind")),
    }
}

fn copy_value(key: &HostAnimationKey) -> HostAnimationValue {
    HostAnimationValue {
        kind: key.kind,
        component_count: key.component_count,
        scalar: key.scalar,
        components: key.components,
        color: key.color,
        reserved: [0; 4],
    }
}

/// Calculates the typed value selected by the C++ worker's current rules.
///
/// MSVC treats `long double` as `double`, so the rational interpolation factor
/// follows the worker's double-precision operation order. Floating outputs
/// are compared with a tight tolerance by the native dual run; colors and
/// kind classifications are exact.
pub fn evaluate(
    now: HostRationalTime,
    keys: &[HostAnimationKey],
) -> Result<HostAnimationValue, HostError> {
    if now.scale == 0 || keys.is_empty() || keys.len() > HOST_PARAMETER_ANIMATION_MAX_KEYS {
        return Err(invalid("validate_parameter_animation_timeline"));
    }
    for (index, key) in keys.iter().enumerate() {
        validate_key(key)?;
        if index != 0 && !rational_less(keys[index - 1].time, key.time) {
            return Err(invalid("validate_parameter_animation_order"));
        }
    }
    if !rational_less(keys[0].time, now) {
        return Ok(copy_value(&keys[0]));
    }
    for pair in keys.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if rational_less(now, right.time) {
            let mut value = copy_value(left);
            if left.interpolation == interpolation::HOLD
                || left.kind != right.kind
                || left.component_count != right.component_count
            {
                return Ok(value);
            }
            let current = f64::from(now.value) / f64::from(now.scale);
            let start = f64::from(left.time.value) / f64::from(left.time.scale);
            let end = f64::from(right.time.value) / f64::from(right.time.scale);
            let factor = (current - start) / (end - start);
            if !factor.is_finite() {
                return Err(invalid("evaluate_parameter_animation_time"));
            }
            match value.kind {
                value_kind::SCALAR => {
                    value.scalar += (right.scalar - value.scalar) * factor;
                }
                value_kind::COLOR => {
                    for (index, channel) in value.color.iter_mut().enumerate() {
                        let initial = f64::from(*channel);
                        let delta = f64::from(right.color[index]) - initial;
                        *channel = (initial + delta * factor).round().clamp(0.0, 255.0) as u8;
                    }
                }
                value_kind::COMPONENTS => {
                    for index in 0..value.component_count as usize {
                        value.components[index] +=
                            (right.components[index] - value.components[index]) * factor;
                    }
                }
                _ => unreachable!("validated kind"),
            }
            if !value.scalar.is_finite()
                || value
                    .components
                    .iter()
                    .any(|component| !component.is_finite())
            {
                return Err(invalid("evaluate_parameter_animation_nonfinite"));
            }
            return Ok(value);
        }
    }
    Ok(copy_value(keys.last().expect("nonempty timeline")))
}

const _: () = {
    assert!(size_of::<HostRationalTime>() == 8);
    assert!(align_of::<HostRationalTime>() == 4);
    assert!(size_of::<HostAnimationKey>() == 64);
    assert!(align_of::<HostAnimationKey>() == 8);
    assert!(offset_of!(HostAnimationKey, scalar) == 16);
    assert!(offset_of!(HostAnimationKey, components) == 24);
    assert!(offset_of!(HostAnimationKey, color) == 48);
    assert!(offset_of!(HostAnimationKey, component_count) == 52);
    assert!(offset_of!(HostAnimationKey, reserved) == 56);
    assert!(size_of::<HostAnimationValue>() == 48);
    assert!(align_of::<HostAnimationValue>() == 8);
    assert!(size_of::<HostParameterAnimationAbiDescriptorV1>() == 56);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: i32, scale: u32, scalar: f64) -> HostAnimationKey {
        HostAnimationKey {
            time: HostRationalTime { value: time, scale },
            kind: value_kind::SCALAR,
            interpolation: interpolation::LINEAR,
            scalar,
            ..HostAnimationKey::default()
        }
    }

    #[test]
    fn scalar_endpoints_fractional_and_negative_time() {
        let keys = [key(-3, 2, -8.0), key(1, 2, 12.0)];
        assert_eq!(
            evaluate(
                HostRationalTime {
                    value: -2,
                    scale: 1
                },
                &keys
            )
            .unwrap()
            .scalar,
            -8.0
        );
        assert_eq!(
            evaluate(
                HostRationalTime {
                    value: -1,
                    scale: 2
                },
                &keys
            )
            .unwrap()
            .scalar,
            2.0
        );
        assert_eq!(
            evaluate(HostRationalTime { value: 1, scale: 2 }, &keys)
                .unwrap()
                .scalar,
            12.0
        );
        assert_eq!(
            evaluate(HostRationalTime { value: 9, scale: 2 }, &keys)
                .unwrap()
                .scalar,
            12.0
        );
    }

    #[test]
    fn hold_color_and_incompatible_left_value() {
        let mut left = key(0, 1, 1.0);
        left.kind = value_kind::COLOR;
        left.color = [1, 2, 3, 4];
        let mut right = left;
        right.time.value = 2;
        right.color = [2, 3, 4, 5];
        let midpoint = HostRationalTime { value: 1, scale: 1 };
        assert_eq!(
            evaluate(midpoint, &[left, right]).unwrap().color,
            [2, 3, 4, 5]
        );
        left.interpolation = interpolation::HOLD;
        assert_eq!(
            evaluate(midpoint, &[left, right]).unwrap().color,
            [1, 2, 3, 4]
        );
        left.interpolation = interpolation::LINEAR;
        right.kind = value_kind::SCALAR;
        assert_eq!(
            evaluate(midpoint, &[left, right]).unwrap().kind,
            value_kind::COLOR
        );
    }

    #[test]
    fn component_interpolation_and_mismatch() {
        let mut left = key(-1, 1, 0.0);
        left.kind = value_kind::COMPONENTS;
        left.component_count = 2;
        left.components = [0.0, 4.0, 0.0];
        let mut right = left;
        right.time.value = 1;
        right.components = [8.0, 12.0, 0.0];
        let midpoint = HostRationalTime { value: 0, scale: 1 };
        assert_eq!(
            evaluate(midpoint, &[left, right]).unwrap().components,
            [4.0, 8.0, 0.0]
        );
        right.component_count = 3;
        assert_eq!(
            evaluate(midpoint, &[left, right]).unwrap().components,
            left.components
        );
    }

    #[test]
    fn malformed_timelines_fail_closed() {
        let valid = key(0, 1, 1.0);
        assert!(evaluate(HostRationalTime { value: 0, scale: 0 }, &[valid]).is_err());
        assert!(evaluate(HostRationalTime { value: 0, scale: 1 }, &[]).is_err());
        assert!(evaluate(HostRationalTime { value: 0, scale: 1 }, &[valid; 257]).is_err());
        assert!(evaluate(HostRationalTime { value: 0, scale: 1 }, &[valid, valid]).is_err());
        for invalid in [
            HostAnimationKey {
                time: HostRationalTime { value: 0, scale: 0 },
                ..valid
            },
            HostAnimationKey {
                scalar: f64::NAN,
                ..valid
            },
            HostAnimationKey {
                components: [f64::INFINITY, 0.0, 0.0],
                ..valid
            },
            HostAnimationKey { kind: 99, ..valid },
            HostAnimationKey {
                interpolation: 99,
                ..valid
            },
            HostAnimationKey {
                component_count: 4,
                ..valid
            },
            HostAnimationKey {
                reserved: 1,
                ..valid
            },
        ] {
            assert!(evaluate(HostRationalTime { value: 0, scale: 1 }, &[invalid]).is_err());
        }
    }

    #[test]
    fn ordered_rationals_collapsing_to_one_double_fail_closed() {
        let left = key(999_999_999, 1_000_000_000, 1.0);
        let right = key(1_000_000_000, 1_000_000_001, 2.0);
        let now = HostRationalTime {
            value: 1_999_999_999,
            scale: 2_000_000_001,
        };
        assert!(rational_less(left.time, now));
        assert!(rational_less(now, right.time));
        assert!(evaluate(now, &[left, right]).is_err());
    }
}
