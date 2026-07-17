use aexcompat_broker::image_render::{
    parameter_animation_sidecar_json, AnimationInterpolation, AnimationTime, AnimationValue,
    ParameterAnimation, ParameterAnimationKey,
};
use serde_json::Value;

fn scalar(time: (i32, u32), value: f64) -> ParameterAnimationKey {
    ParameterAnimationKey {
        time: AnimationTime {
            value: time.0,
            scale: time.1,
        },
        interpolation: AnimationInterpolation::Linear,
        value: AnimationValue::Scalar { value },
    }
}

#[test]
fn sidecar_is_versioned_and_keeps_typed_values_out_of_cli_payloads() {
    let bytes = parameter_animation_sidecar_json(&[ParameterAnimation {
        slot: 2,
        keys: vec![
            scalar((0, 30), 1.0),
            ParameterAnimationKey {
                time: AnimationTime {
                    value: 1,
                    scale: 30,
                },
                interpolation: AnimationInterpolation::Hold,
                value: AnimationValue::Color {
                    value: [255, 10, 20, 30],
                },
            },
        ],
    }])
    .unwrap();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["parameters"][0]["slot"], 2);
    assert_eq!(
        document["parameters"][0]["keys"][1]["value"]["type"],
        "color"
    );
}

#[test]
fn rational_order_is_strict_and_duplicate_slots_are_rejected() {
    let duplicate_time = ParameterAnimation {
        slot: 1,
        keys: vec![scalar((1, 2), 0.0), scalar((2, 4), 1.0)],
    };
    assert!(parameter_animation_sidecar_json(&[duplicate_time]).is_err());

    let one = ParameterAnimation {
        slot: 7,
        keys: vec![scalar((0, 1), 0.0)],
    };
    assert!(parameter_animation_sidecar_json(&[one.clone(), one]).is_err());
}

#[test]
fn zero_scale_nonfinite_and_invalid_components_fail_closed() {
    let cases = [
        ParameterAnimationKey {
            time: AnimationTime { value: 0, scale: 0 },
            interpolation: AnimationInterpolation::Hold,
            value: AnimationValue::Scalar { value: 0.0 },
        },
        scalar((0, 1), f64::NAN),
        ParameterAnimationKey {
            time: AnimationTime { value: 0, scale: 1 },
            interpolation: AnimationInterpolation::Linear,
            value: AnimationValue::Components { value: vec![] },
        },
    ];
    for key in cases {
        assert!(parameter_animation_sidecar_json(&[ParameterAnimation {
            slot: 1,
            keys: vec![key],
        }])
        .is_err());
    }
}

#[test]
fn per_parameter_and_total_key_limits_are_strict() {
    let too_many = ParameterAnimation {
        slot: 1,
        keys: (0..257).map(|time| scalar((time, 1), 0.0)).collect(),
    };
    assert!(parameter_animation_sidecar_json(&[too_many]).is_err());

    let animations = (1..=17)
        .map(|slot| ParameterAnimation {
            slot,
            keys: (0..256).map(|time| scalar((time, 1), 0.0)).collect(),
        })
        .collect::<Vec<_>>();
    assert!(parameter_animation_sidecar_json(&animations).is_err());
}

#[test]
fn arbitrary_keys_are_versioned_and_byte_bounded() {
    let animation = |value| ParameterAnimation {
        slot: 3,
        keys: vec![ParameterAnimationKey {
            time: AnimationTime { value: 0, scale: 1 },
            interpolation: AnimationInterpolation::Linear,
            value: AnimationValue::Arbitrary { value },
        }],
    };
    let bytes = parameter_animation_sidecar_json(&[animation(vec![0x43, 0x47, 1, 2])]).unwrap();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(
        document["parameters"][0]["keys"][0]["value"]["type"],
        "arbitrary"
    );
    assert!(parameter_animation_sidecar_json(&[animation(vec![])]).is_err());
    assert!(parameter_animation_sidecar_json(&[animation(vec![0; 64 * 1024 + 1])]).is_err());
}
